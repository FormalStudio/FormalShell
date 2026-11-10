// The clipssh route's pure half: ~/.clipssh/aliases parsing (clipssh's own
// `name=user@host` format), row building and clipssh's output contract.

use fs_menu::node::Kind;
use fs_menu::providers::{ClipsshAlias, ClipsshOutcome, clipssh_aliases, clipssh_outcome, clipssh_rows};

fn failed(outcome: ClipsshOutcome) -> String {
    match outcome {
        ClipsshOutcome::Failed { error } => error,
        ClipsshOutcome::Ok { .. } => panic!("expected a failure"),
    }
}

#[test]
fn parse_aliases() {
    let parsed = clipssh_aliases("box=kyan@box.lan\nvps=root@203.0.113.7\n");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0], ClipsshAlias { name: "box".into(), target: "kyan@box.lan".into() });
    assert_eq!(parsed[1], ClipsshAlias { name: "vps".into(), target: "root@203.0.113.7".into() });
}

#[test]
fn parse_skips_malformed_lines() {
    // No '=', empty name, empty target, whitespace name, blank lines: all
    // dropped without poisoning the valid row.
    let text = "\nnot-an-alias\n=nobody@host\nempty=\nbad name=user@host\nok=user@host\n\n";
    let parsed = clipssh_aliases(text);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].name, "ok");
    assert_eq!(parsed[0].target, "user@host");
}

#[test]
fn parse_empty_input() {
    assert_eq!(clipssh_aliases("").len(), 0);
}

// Targets keep everything after the FIRST '=': clipssh only bans '=' in the
// name, not the target.
#[test]
fn parse_target_keeps_later_equals() {
    let parsed = clipssh_aliases("odd=user@host=weird");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].target, "user@host=weird");
}

#[test]
fn rows_shape() {
    let rows = clipssh_rows(&[ClipsshAlias { name: "box".into(), target: "kyan@box.lan".into() }]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "clipssh.box");
    assert_eq!(rows[0].label, "box");
    assert_eq!(rows[0].desc.as_deref(), Some("kyan@box.lan"));
    assert_eq!(rows[0].kind, Kind::Action);
    // Dispatched in-process, never spawned: the service has to be the thing
    // that runs clipssh, or nothing can report how it went. No shell quoting is
    // involved because no shell string is built: the alias travels as the
    // argument it is.
    assert_eq!(rows[0].action.as_ref().map(ToString::to_string).as_deref(), Some("@ipc:clipssh.send:box"));
    // The row fires no toast of its own; the service owns every word the user
    // sees, so a row-level toast would only double the "sending" one.
    assert_eq!(rows[0].notify_summary, None);
    assert_eq!(rows[0].notify_body, None);
}

#[test]
fn rows_pass_an_awkward_alias_through_verbatim() {
    let rows = clipssh_rows(&[ClipsshAlias { name: "it's".into(), target: "u@h".into() }]);
    assert_eq!(rows[0].action.as_ref().map(ToString::to_string).as_deref(), Some("@ipc:clipssh.send:it's"));
}

#[test]
fn outcome_success_reads_the_remote_path() {
    // clipssh's own success line, ANSI green included.
    let outcome = clipssh_outcome(
        0,
        "\u{1b}[0;32mUploaded: /tmp/clipboard-1755180000.png\u{1b}[0m\nPath copied to clipboard - paste it directly\n",
        "",
    );
    assert_eq!(outcome, ClipsshOutcome::Ok { path: "/tmp/clipboard-1755180000.png".into() });
}

#[test]
fn outcome_success_without_a_readable_path() {
    // Still a completed transfer: the caller says so without inventing a path
    // it never saw.
    assert_eq!(clipssh_outcome(0, "", ""), ClipsshOutcome::Ok { path: String::new() });
}

#[test]
fn outcome_failure_reads_clipssh_own_reason() {
    let outcome = clipssh_outcome(1, "", "\u{1b}[0;31mError:\u{1b}[0m No image in clipboard. Take a screenshot first\n");
    assert_eq!(failed(outcome), "No image in clipboard. Take a screenshot first");
}

#[test]
fn outcome_missing_binary() {
    assert_eq!(failed(clipssh_outcome(127, "", "sh: line 1: clipssh: command not found\n")), "clipssh is not installed");
}

#[test]
fn outcome_failure_falls_back_to_the_last_line() {
    let outcome = clipssh_outcome(255, "", "ssh: connect to host box.lan port 22: No route to host\n");
    assert_eq!(failed(outcome), "ssh: connect to host box.lan port 22: No route to host");
}

#[test]
fn outcome_failure_with_nothing_to_go_on() {
    assert_eq!(failed(clipssh_outcome(3, "", "")), "clipssh exited with code 3");
}

#[test]
fn rows_empty_is_note() {
    let rows = clipssh_rows(&[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, Kind::Note);
    assert_eq!(rows[0].dim, Some(true));
    assert_eq!(rows[0].label, "No aliases");
    assert_eq!(rows[0].desc.as_deref(), Some("clipssh alias add <name> <user@host>"));
}
