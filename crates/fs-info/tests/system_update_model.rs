use std::collections::HashMap;

use fs_info::system_update::*;
use serde_json::json;

// Node shapes lifted verbatim from this repo's own flake.lock, not invented:
// a github node carries owner/repo/rev in `locked` and its branch as `ref` in
// `original`; a git node carries url/ref/rev all in `locked` and only url in
// `original`. `nixpkgs_2` is a transitive input of quickshell and must never
// surface.
fn lock_fixture() -> String {
    json!({
        "nodes": {
            "root": { "inputs": { "nixpkgs": "nixpkgs", "quickshell": "quickshell" } },
            "nixpkgs": {
                "locked": {
                    "lastModified": 1785090369,
                    "narHash": "sha256-m0pDuRJG7EDo9ri+4Ksu83VsI+PlxNC9lNBfydejce4=",
                    "owner": "NixOS",
                    "repo": "nixpkgs",
                    "rev": "624af665418d3c65d544145b4d34ad696439570e",
                    "type": "github"
                },
                "original": { "owner": "NixOS", "ref": "nixos-unstable", "repo": "nixpkgs", "type": "github" }
            },
            "quickshell": {
                "inputs": { "nixpkgs": "nixpkgs_2" },
                "locked": {
                    "lastModified": 1785134238,
                    "narHash": "sha256-bv5gar+ZAXZCJH7UOv0eRFILKt5RKA/3px/XbVR98Cg=",
                    "ref": "refs/heads/master",
                    "rev": "43d4fa9e883cb03239b3d578c9c57070f4fbd281",
                    "revCount": 834,
                    "type": "git",
                    "url": "https://git.outfoxxed.me/quickshell/quickshell"
                },
                "original": { "type": "git", "url": "https://git.outfoxxed.me/quickshell/quickshell" }
            },
            "nixpkgs_2": {
                "locked": {
                    "owner": "NixOS",
                    "repo": "nixpkgs",
                    "rev": "0000000000000000000000000000000000000000",
                    "type": "github"
                },
                "original": { "owner": "NixOS", "repo": "nixpkgs", "type": "github" }
            }
        },
        "root": "root",
        "version": 7
    })
    .to_string()
}

const NIXPKGS_REV: &str = "624af665418d3c65d544145b4d34ad696439570e";
const QUICKSHELL_REV: &str = "43d4fa9e883cb03239b3d578c9c57070f4fbd281";
const UPSTREAM_REV: &str = "1111111111111111111111111111111111111111";

fn input(name: &str) -> FlakeInput {
    parse_lock(&lock_fixture())
        .inputs
        .into_iter()
        .find(|i| i.name == name)
        .unwrap()
}

fn heads(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
}

fn bare(kind: &str) -> FlakeInput {
    FlakeInput { kind: kind.into(), ..FlakeInput::default() }
}

#[test]
fn parse_lock_reads_only_the_roots_direct_inputs() {
    let parsed = parse_lock(&lock_fixture());
    assert!(parsed.ok);
    assert_eq!(parsed.inputs.len(), 2);
    assert_eq!(parsed.inputs[0].name, "nixpkgs");
    assert_eq!(parsed.inputs[1].name, "quickshell");
}

#[test]
fn parse_lock_github_node_shape() {
    let i = input("nixpkgs");
    assert_eq!(i.kind, "github");
    assert_eq!(i.owner, "NixOS");
    assert_eq!(i.repo, "nixpkgs");
    assert_eq!(i.r#ref, "nixos-unstable");
    assert_eq!(i.rev, NIXPKGS_REV);
    assert_eq!(i.last_modified, 1785090369.0);
}

#[test]
fn parse_lock_github_node_without_ref_leaves_ref_empty() {
    let fixture = json!({
        "root": "root",
        "nodes": {
            "root": { "inputs": { "home": "home" } },
            "home": {
                "locked": { "owner": "nix-community", "repo": "home-manager", "rev": NIXPKGS_REV, "type": "github" },
                "original": { "owner": "nix-community", "repo": "home-manager", "type": "github" }
            }
        }
    })
    .to_string();
    assert_eq!(parse_lock(&fixture).inputs[0].r#ref, "");
}

#[test]
fn parse_lock_git_node_shape() {
    let i = input("quickshell");
    assert_eq!(i.kind, "git");
    assert_eq!(i.url, "https://git.outfoxxed.me/quickshell/quickshell");
    assert_eq!(i.r#ref, "refs/heads/master");
    assert_eq!(i.rev, QUICKSHELL_REV);
}

#[test]
fn parse_lock_malformed_text_is_ok_false_never_throws() {
    assert!(!parse_lock("").ok);
    assert!(!parse_lock("not json at all").ok);
    assert!(!parse_lock(&json!({ "version": 7 }).to_string()).ok);
    assert!(!parse_lock(&json!({ "root": "root", "nodes": {} }).to_string()).ok);
    assert_eq!(parse_lock("not json at all").inputs.len(), 0);
}

#[test]
fn probe_command_github_uses_the_commits_api_with_the_sha_accept_header() {
    let probe = probe_command(&input("nixpkgs"));
    assert_eq!(probe.kind.as_str(), "github");
    assert!(probe.argv.is_empty());
    assert!(probe.headers.iter().any(|a| a == "Accept: application/vnd.github.sha"));
    assert_eq!(
        &probe.url,
        "https://api.github.com/repos/NixOS/nixpkgs/commits/nixos-unstable"
    );
}

#[test]
fn probe_command_github_without_a_ref_asks_for_head() {
    let probe = probe_command(&FlakeInput {
        kind: "github".into(),
        owner: "nix-community".into(),
        repo: "home-manager".into(),
        ..FlakeInput::default()
    });
    assert_eq!(
        &probe.url,
        "https://api.github.com/repos/nix-community/home-manager/commits/HEAD"
    );
}

#[test]
fn probe_command_git_uses_ls_remote_with_the_locked_ref() {
    let probe = probe_command(&input("quickshell"));
    assert_eq!(probe.kind.as_str(), "git");
    assert_eq!(probe.argv[0], "sh");
    assert!(probe.argv[2].contains("git ls-remote"));
    assert!(probe.argv[2].contains("command -v git"));
    // url and ref ride as positional arguments, never interpolated into the
    // script, so nothing out of the lock file is parsed as shell.
    assert_eq!(
        probe.argv[probe.argv.len() - 2],
        "https://git.outfoxxed.me/quickshell/quickshell"
    );
    assert_eq!(probe.argv[probe.argv.len() - 1], "refs/heads/master");
}

#[test]
fn probe_command_gitlab_percent_encodes_the_project_path() {
    let probe = probe_command(&FlakeInput {
        kind: "gitlab".into(),
        owner: "group".into(),
        repo: "proj".into(),
        r#ref: "feat/x".into(),
        ..FlakeInput::default()
    });
    assert_eq!(probe.kind.as_str(), "gitlab");
    assert_eq!(
        &probe.url,
        "https://gitlab.com/api/v4/projects/group%2Fproj/repository/commits/feat%2Fx"
    );
}

#[test]
fn probe_command_path_and_tarball_types_are_kind_none() {
    let with_url = |kind: &str, url: &str| FlakeInput { url: url.into(), ..bare(kind) };
    assert_eq!(probe_command(&with_url("path", "/etc/nixos")).kind.as_str(), "none");
    assert_eq!(
        probe_command(&with_url("tarball", "https://example.invalid/x.tar.gz")).kind.as_str(),
        "none"
    );
    assert_eq!(probe_command(&bare("indirect")).kind.as_str(), "none");
    assert_eq!(probe_command(&bare("path")).argv.len(), 0);
}

#[test]
fn parse_probe_github_takes_the_bare_sha() {
    assert_eq!(
        parse_probe(ProbeKind::Github, 0, &format!("{UPSTREAM_REV}\n")),
        UPSTREAM_REV
    );
}

#[test]
fn parse_probe_git_takes_the_sha_before_the_tab() {
    let stdout = format!("{UPSTREAM_REV}\trefs/heads/master\n");
    assert_eq!(parse_probe(ProbeKind::Git, 0, &stdout), UPSTREAM_REV);
}

#[test]
fn parse_probe_gitlab_takes_the_commit_id() {
    let stdout = json!({ "id": UPSTREAM_REV, "short_id": "1111111" }).to_string();
    assert_eq!(parse_probe(ProbeKind::Gitlab, 0, &stdout), UPSTREAM_REV);
}

#[test]
fn parse_probe_nonzero_exit_or_empty_stdout_is_empty_rev() {
    assert_eq!(parse_probe(ProbeKind::Github, 22, UPSTREAM_REV), "");
    assert_eq!(parse_probe(ProbeKind::Git, 127, ""), "");
    assert_eq!(parse_probe(ProbeKind::Github, 0, ""), "");
    assert_eq!(parse_probe(ProbeKind::Github, 0, "API rate limit exceeded"), "");
    assert_eq!(parse_probe(ProbeKind::None, 0, UPSTREAM_REV), "");
}

#[test]
fn count_behind_counts_only_resolved_heads() {
    let inputs = parse_lock(&lock_fixture()).inputs;
    let counts = count_behind(
        &inputs,
        &heads(&[("nixpkgs", UPSTREAM_REV), ("quickshell", QUICKSHELL_REV)]),
    );
    assert_eq!(counts.behind, 1);
    assert_eq!(counts.current, 1);
    assert_eq!(counts.unknown, 0);
    assert_eq!(counts.behind_names, ["nixpkgs"]);
}

#[test]
fn count_behind_unresolved_head_is_unknown_never_current() {
    let inputs = parse_lock(&lock_fixture()).inputs;
    let counts = count_behind(&inputs, &heads(&[("nixpkgs", "")]));
    assert_eq!(counts.unknown, 2);
    assert_eq!(counts.current, 0);
    assert_eq!(counts.behind, 0);
}

#[test]
fn row_status_agrees_with_the_aggregate_count() {
    let h = heads(&[("nixpkgs", UPSTREAM_REV), ("quickshell", QUICKSHELL_REV)]);
    assert_eq!(row_status(&input("nixpkgs"), &h), "Behind");
    assert_eq!(row_status(&input("quickshell"), &h), "Current");
    assert_eq!(row_status(&input("nixpkgs"), &HashMap::new()), "?");
}

#[test]
fn short_rev_is_the_first_seven_characters() {
    assert_eq!(short_rev(NIXPKGS_REV), "624af66");
    assert_eq!(short_rev(""), "");
}

fn counts(behind: u32, current: u32, unknown: u32) -> Counts {
    Counts { behind, current, unknown, behind_names: Vec::new() }
}

#[test]
fn summary_label_never_says_up_to_date_while_unknown_is_nonzero() {
    assert_eq!(summary_label(PollState::Ok, &counts(0, 3, 2)), "2 ?");
}

#[test]
fn summary_label_honest_states() {
    let none = Counts::default();
    assert_eq!(summary_label(PollState::parse("noflake"), &none), "No flake");
    assert_eq!(summary_label(PollState::parse("nolock"), &none), "No lock");
    assert_eq!(summary_label(PollState::parse("checking"), &none), "Checking");
    assert_eq!(summary_label(PollState::parse("offline"), &none), "No network");
    assert_eq!(summary_label(PollState::Ok, &counts(0, 2, 0)), "Up to date");
    assert_eq!(summary_label(PollState::Ok, &counts(2, 1, 0)), "2 behind");
    assert_eq!(summary_label(PollState::Ok, &counts(2, 0, 1)), "2 behind / 1 ?");
}

#[test]
fn summary_label_of_an_unrecognized_state_holds_at_checking() {
    assert_eq!(summary_label(PollState::parse(""), &counts(0, 0, 0)), "Checking");
    assert_eq!(summary_label(PollState::parse("wat"), &counts(0, 9, 0)), "Checking");
}
