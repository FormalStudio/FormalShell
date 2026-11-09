//! `tests/ipc-golden.jsonl`, the fixed wire contract, replayed through the
//! client's parsing and the registry. The stub registry is the shell's own
//! [`super::targets`] with every handler answering the recorded reply, so
//! the replay pins argument counts, parse errors and listings; `probe` alone
//! echoes what the registry parsed. `tests/ipc-cases.jsonl` holds cases in
//! the same shape for what the fixed file never recorded, and
//! `tests/ipc-signatures.txt` pins every target's signatures.

use serde::Deserialize;

use super::cli;
use super::registry::{Function, Json, Registry, Reply, Target, Type, Value, ok, text};
use super::wire::Request;

#[derive(Deserialize)]
struct Case {
    argv: Vec<String>,
    live: bool,
    stdout: String,
    stderr: String,
    exit: i32,
}

const fn f(name: &'static str, params: &'static [(&'static str, Type)], ret: Type, call: fn(&mut Replay, &[Value]) -> Reply) -> Function<Replay> {
    Function { name, params, ret, call }
}

const STR: (&str, Type) = ("a", Type::String);

/// The recorded stdout of the case being replayed, less its final newline.
struct Replay {
    reply: String,
}

fn replay(ctx: &mut Replay, _: &[Value]) -> Reply {
    text(ctx.reply.clone())
}

/// Every shell target, each handler replaying the recorded reply.
fn shell() -> Vec<Target<Replay>> {
    super::targets()
        .into_iter()
        .map(|t| Target {
            name: t.name,
            functions: t.functions.into_iter().map(|fun| Function { name: fun.name, params: fun.params, ret: fun.ret, call: replay }).collect(),
        })
        .collect()
}

fn stub() -> Registry<Replay> {
    let mut targets = shell();
    targets.push(probe());
    Registry { targets }
}

/// A target only the recordings know, pinning how each argument type parses
/// and each return type prints.
fn probe() -> Target<Replay> {
    Target {
        name: "probe",
        functions: vec![
            f("s", &[STR], Type::String, |_, a| text(format!("[{}]", a[0].str()))),
            f("ss", &[STR, ("b", Type::String)], Type::String, |_, a| {
                text(format!("[{}][{}]", a[0].str(), a[1].str()))
            }),
            f("sss", &[STR, ("b", Type::String), ("c", Type::String)], Type::String, |_, a| {
                text(format!("[{}][{}][{}]", a[0].str(), a[1].str(), a[2].str()))
            }),
            f("i", &[("a", Type::Int)], Type::String, |_, a| text(a[0].int().to_string())),
            f("b", &[("a", Type::Bool)], Type::String, |_, a| text(a[0].bool().to_string())),
            f("r", &[("a", Type::Real)], Type::String, |_, a| text(fs_js::num_str(if let Value::Real(r) = a[0] { r } else { 0.0 }))),
            f("ri", &[("a", Type::Real)], Type::Real, |_, a| Ok(Json::from(a[0].real()))),
            f("ii", &[("a", Type::Int)], Type::Int, |_, a| Ok(Json::from(a[0].int()))),
            f("bb", &[("a", Type::Bool)], Type::Bool, |_, a| Ok(Json::from(a[0].bool()))),
            f("is", &[("a", Type::Int), ("b", Type::String)], Type::String, |_, a| text(format!("[{}][{}]", a[0].int(), a[1].str()))),
            f(
                "sirb",
                &[STR, ("b", Type::Int), ("c", Type::Real), ("d", Type::Bool)],
                Type::String,
                |_, a| text(format!("[{}][{}][{}][{}]", a[0].str(), a[1].int(), fs_js::num_str(a[2].real()), a[3].bool())),
            ),
            f("v", &[], Type::Void, |_, _| ok()),
            f("empty", &[], Type::String, |_, _| text("")),
            f("nl", &[], Type::String, |_, _| text("a\nb\n")),
            f("show", &[], Type::String, |_, _| text("probe-show")),
            f("call", &[], Type::String, |_, _| text("probe-call")),
        ],
    }
}

/// stdout, stderr and the exit code, as formalshell-ipc would leave them.
fn run(registry: &Registry<Replay>, argv: &[String], live: bool, reply: &str) -> (String, String, i32, bool) {
    let request = match cli::parse(argv) {
        Ok(request) => request,
        Err(err) => return (String::new(), err.stderr(), err.exit, false),
    };
    if !live {
        return ("No running instances for \"\"\n".into(), String::new(), 255, false);
    }
    let listing = matches!(request, Request::Show { .. });
    if let Some(message) = cli::missing(&request) {
        return (message.into(), String::new(), 0, listing);
    }
    (registry.answer(&mut Replay { reply: reply.to_owned() }, &request), String::new(), 0, listing)
}

/// The quoted path is the instance's own, so only the words around it are
/// held.
fn no_instance(text: &str) -> bool {
    text.starts_with("No running instances for \"") && text.ends_with("\"\n")
}

/// The recorded listing comes out of a hash, so only the set holds.
/// Each recording saw the targets of its own branch, so every target block
/// it lists must appear in ours with the same functions; ours may list more.
fn blocks(text: &str) -> std::collections::BTreeMap<&str, Vec<&str>> {
    let mut out = std::collections::BTreeMap::new();
    let mut current: Option<&str> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("target ") {
            current = Some(name);
            out.entry(name).or_insert_with(Vec::new);
        } else if let Some(name) = current {
            out.get_mut(name).expect("opened above").push(line);
        }
    }
    for functions in out.values_mut() {
        functions.sort();
    }
    out
}

/// Replays each line of `jsonl` against the stub registry.
fn replay_all(jsonl: &str) {
    let registry = stub();
    let cases: Vec<Case> = jsonl.lines().map(|line| serde_json::from_str(line).expect("a golden line")).collect();
    for case in &cases {
        let reply = case.stdout.strip_suffix('\n').unwrap_or(&case.stdout);
        let (stdout, stderr, exit, listing) = run(&registry, &case.argv, case.live, reply);
        let what = format!("{:?} live={}", case.argv, case.live);
        assert_eq!(exit, case.exit, "exit code of {what}");
        assert_eq!(stderr, case.stderr, "stderr of {what}");
        if !case.live {
            assert!(no_instance(&stdout) && no_instance(&case.stdout), "stdout of {what}: {stdout:?}");
        } else if listing {
            let ours = blocks(&stdout);
            // A recording made on an older branch knows fewer functions of a
            // target; every one it lists must still be served, signature intact.
            for (name, functions) in blocks(&case.stdout) {
                let served = ours.get(name).unwrap_or_else(|| panic!("target {name} in stdout of {what}"));
                for f in functions {
                    assert!(served.contains(&f), "{f:?} of target {name} in stdout of {what}");
                }
            }
        } else {
            assert_eq!(stdout, case.stdout, "stdout of {what}");
        }
    }
}

#[test]
fn matches_qs_ipc() {
    let jsonl = include_str!("../../tests/ipc-golden.jsonl");
    assert!(jsonl.lines().count() > 100, "only {} golden cases", jsonl.lines().count());
    replay_all(jsonl);
}

#[test]
fn matches_shell_cases() {
    replay_all(include_str!("../../tests/ipc-cases.jsonl"));
}

/// The listing `formalshell-ipc show` prints, every target in order.
#[test]
fn signatures_match_snapshot() {
    let show = Request::Show { target: String::new(), name: String::new() };
    let listing = Registry { targets: shell() }.answer(&mut Replay { reply: String::new() }, &show);
    assert_eq!(listing, include_str!("../../tests/ipc-signatures.txt"), "tests/ipc-signatures.txt is out of date");
}
