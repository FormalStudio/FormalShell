//! Open PRs the user authored and open issues assigned to them, off one
//! `gh api graphql` run. gh missing hides the
//! cell; a failed auth reads as NO AUTH, any other failure as NO GH.

use serde_json::Value;

use futures_lite::FutureExt;

use super::{changed, idle, kicked, settings};
use crate::runtime::Ctx;
use crate::services::proc::{self, MISSING};
use crate::services::wants::Source;
use crate::store;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Poll {
    #[default]
    Unknown,
    Missing,
    NoAuth,
    Error,
    Ok,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub title: String,
    pub url: String,
    pub repo: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub poll: Poll,
    pub prs: u64,
    pub issues: u64,
    pub pr_rows: Vec<Row>,
    pub issue_rows: Vec<Row>,
    pub login: String,
}

const QUERY: &str = "{ viewer { login } prs: search(query: \"is:open is:pr author:@me\", type: ISSUE, first: 15) { issueCount nodes { ... on PullRequest { title url repository { nameWithOwner } } } } issues: search(query: \"is:open is:issue assignee:@me\", type: ISSUE, first: 15) { issueCount nodes { ... on Issue { title url repository { nameWithOwner } } } } }";

/// gh's exit code for a failed authentication.
const NO_AUTH: i32 = 4;

fn rows(nodes: Option<&Value>) -> Vec<Row> {
    let Some(nodes) = nodes.and_then(Value::as_array) else { return Vec::new() };
    nodes
        .iter()
        .filter_map(|n| {
            Some(Row {
                title: n.get("title")?.as_str()?.to_owned(),
                url: n.get("url")?.as_str()?.to_owned(),
                repo: n.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("").to_owned(),
            })
        })
        .collect()
}

pub fn parse(code: i32, stdout: &str) -> State {
    let poll = |poll| State { poll, ..State::default() };
    match code {
        MISSING => return poll(Poll::Missing),
        NO_AUTH => return poll(Poll::NoAuth),
        0 => {}
        _ => return poll(Poll::Error),
    }
    let Ok(data) = serde_json::from_str::<Value>(stdout) else { return poll(Poll::Error) };
    let count = |key: &str| data.pointer(&format!("/data/{key}/issueCount")).and_then(Value::as_u64);
    let (Some(prs), Some(issues)) = (count("prs"), count("issues")) else { return poll(Poll::Error) };
    State {
        poll: Poll::Ok,
        prs,
        issues,
        pr_rows: rows(data.pointer("/data/prs/nodes")),
        issue_rows: rows(data.pointer("/data/issues/nodes")),
        login: data.pointer("/data/viewer/login").and_then(Value::as_str).unwrap_or("").to_owned(),
    }
}

fn read() -> std::time::Duration {
    settings().interval("github.intervalMs", 300_000)
}

pub async fn run(ctx: Ctx) {
    let rx = changed();
    let kick = kicked(Source::Github);
    while kick.try_recv().is_ok() {}
    loop {
        let interval = read();
        let done = proc::capture(&proc::argv(&["gh", "api", "graphql", "-f", &format!("query={QUERY}")]), std::time::Duration::from_secs(60)).await;
        ctx.publish(store::Diff::Github(parse(done.code, &done.stdout)));
        let asked = async {
            let _ = kick.recv().await;
        };
        asked.or(idle(interval, &rx, &interval, read)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_and_bodies_resolve_to_a_poll_state() {
        assert_eq!(parse(127, "").poll, Poll::Missing);
        assert_eq!(parse(4, "").poll, Poll::NoAuth);
        assert_eq!(parse(1, "").poll, Poll::Error);
        assert_eq!(parse(0, "nope").poll, Poll::Error);
        assert_eq!(parse(0, r#"{"data":{"prs":{"issueCount":"3"},"issues":{"issueCount":2}}}"#).poll, Poll::Error);
        let ok = parse(
            0,
            r#"{"data":{"viewer":{"login":"k"},"prs":{"issueCount":3,"nodes":[{"title":"t","url":"u","repository":{"nameWithOwner":"o/r"}},{"title":1}]},"issues":{"issueCount":2,"nodes":[]}}}"#,
        );
        assert_eq!((ok.poll, ok.prs, ok.issues, ok.login.as_str()), (Poll::Ok, 3, 2, "k"));
        assert_eq!(ok.pr_rows, [Row { title: "t".into(), url: "u".into(), repo: "o/r".into() }]);
    }
}
