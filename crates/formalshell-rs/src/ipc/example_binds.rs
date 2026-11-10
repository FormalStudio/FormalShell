//! `docs/examples/hyprland/formalshell.lua`, the include `formalshell
//! install` writes, checked against the registry: every bind's call parses
//! the way the client parses it and reaches a function that takes its
//! arguments, and every `menu summon` names a route the shipped menu opens.

use fs_menu::base::{BaseInputs, ShareInputs, base_tree, defaults};
use fs_menu::node::{Entries, Kind};

use super::cli;
use super::wire::Request;
use crate::install::IPC_CALL;

const EXAMPLE: &str = include_str!("../../../../docs/examples/hyprland/formalshell.lua");

/// The string literal opening `s`, and what follows it.
fn literal(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start().strip_prefix('"')?;
    let end = s.find('"')?;
    Some((&s[..end], &s[end + 1..]))
}

/// Every call the example's binds make, as the words after `call`:
/// `fs_bind(chord, "<words>")`, with `.. n` read as 1 and 9 for the
/// numbered loop, and `then_osd(cmd, "<target>")`'s `osd <target>`.
fn calls() -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in EXAMPLE.match_indices("fs_bind(") {
        let rest = &EXAMPLE[i + "fs_bind(".len()..];
        if rest.starts_with("chord") {
            continue;
        }
        let (_, rest) = literal(rest).expect("fs_bind's chord opens with a string literal");
        let rest = &rest[rest.find(',').expect("fs_bind takes a chord, then its call") + 1..];
        let (words, rest) = literal(rest).expect("fs_bind's call is a string literal");
        if rest.trim_start().starts_with("..") {
            out.extend(["1", "9"].map(|n| format!("{words}{n}")));
        } else {
            out.push(words.to_owned());
        }
    }
    for (i, _) in EXAMPLE.match_indices("then_osd(") {
        let rest = &EXAMPLE[i + "then_osd(".len()..];
        if rest.starts_with("cmd") {
            continue;
        }
        let close = rest.find(')').expect("then_osd's call closes");
        let target = rest[..close].rsplit(',').next().and_then(|t| literal(t)).expect("then_osd's target is a string literal").0;
        out.push(format!("osd {target}"));
    }
    out
}

#[test]
fn every_call_goes_through_the_one_client_spelling() {
    // The definition, fs_bind's body and then_osd's body: a bind writing its
    // own command line would escape both the include's path rewrite and the
    // checks below.
    assert_eq!(EXAMPLE.matches(IPC_CALL).count(), 1, "the example spells {IPC_CALL} once, in fs_call");
    assert_eq!(EXAMPLE.matches("fs_call").count(), 3, "fs_call is used by fs_bind and then_osd alone");
}

#[test]
fn every_bind_reaches_a_function() {
    let calls = calls();
    assert!(calls.len() > 40, "read {} calls out of the example", calls.len());
    let registry = super::registry();
    let failures: Vec<String> = calls
        .iter()
        .filter_map(|words| {
            let argv: Vec<String> = std::iter::once("call").chain(words.split_whitespace()).map(str::to_owned).collect();
            match cli::parse(&argv) {
                Ok(Request::Call { target, function, args }) => registry.check(&target, &function, &args).err(),
                Ok(_) => Some("not a call".into()),
                Err(e) => Some(e.message),
            }
            .map(|why| format!("{words}: {why}"))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_summon_names_a_route() {
    let inputs = BaseInputs { share: Some(ShareInputs { installed: true, ..ShareInputs::default() }), ..BaseInputs::default() };
    let tree = base_tree(defaults().expect("the shipped menu parses"), &inputs, &Entries::default());
    let failures: Vec<String> = calls()
        .iter()
        .filter_map(|words| words.strip_prefix("menu summon "))
        .filter(|route| {
            let node = tree.nodes.get(*route).or_else(|| tree.nodes.values().find(|n| n.aliases.iter().any(|a| a == route)));
            !node.is_some_and(|n| matches!(n.kind, Kind::Submenu | Kind::Provider | Kind::Link))
        })
        .map(str::to_owned)
        .collect();
    assert!(failures.is_empty(), "no route opens for {failures:?}");
}
