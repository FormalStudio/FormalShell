//! fspec's operation stream, version 1 (FormalSpec `docs/activity.md`).
//! Every line carries the whole operation, so a line is parsed into an
//! [`Item`] that replaces whatever was held under its id.

use serde::Deserialize;

use crate::progress::{Progress, Unit};
use crate::{Item, Origin, State};

/// The only protocol version this reader knows.
pub const VERSION: u64 = 1;

#[derive(Deserialize)]
struct Line {
    v: u64,
    op: Option<Op>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Op {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub host: Option<String>,
    pub pid: Option<u32>,
    pub started: i64,
    pub updated: i64,
    pub state: String,
    pub phase: Option<String>,
    pub scope: Option<String>,
    pub progress: Option<Steps>,
    pub item: Option<String>,
    pub keys: Vec<String>,
    pub result: Option<Outcome>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Steps {
    pub done: u64,
    pub total: u64,
    pub unit: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Outcome {
    pub changes: u64,
    pub summary: String,
}

/// One line off a socket, without its `\n`. `None` for a line of another
/// version, one that is not JSON, or one with no operation in it.
pub fn parse(line: &str) -> Option<Op> {
    let line: Line = serde_json::from_str(line).ok()?;
    if line.v != VERSION {
        return None;
    }
    line.op.filter(|op| !op.id.is_empty())
}

pub fn item_id(op_id: &str) -> String {
    format!("fspec:{op_id}")
}

impl Op {
    pub fn state(&self) -> State {
        match self.state.as_str() {
            "succeeded" => State::Succeeded,
            "failed" => State::Failed,
            _ => State::Running,
        }
    }

    pub fn item(&self) -> Item {
        let state = self.state();
        let progress = match &self.progress {
            Some(s) => Progress::Steps { done: s.done, total: s.total, unit: Unit::parse(&s.unit) },
            None => Progress::Indeterminate,
        };
        let summary = match state {
            State::Succeeded => self.result.as_ref().map(|r| r.summary.clone()),
            State::Failed => self.error.clone(),
            _ => None,
        };
        let source = if self.source.is_empty() { format!("fspec:{}", self.kind) } else { self.source.clone() };
        Item {
            id: item_id(&self.id),
            origin: Origin::Fspec,
            source,
            kind: if self.kind.is_empty() { "apply".into() } else { self.kind.clone() },
            app: None,
            title: self.title.clone(),
            detail: self.item.clone().or_else(|| self.phase.clone()).unwrap_or_default(),
            phase: self.phase.clone(),
            progress,
            state,
            urgent: state == State::Failed,
            keys: self.keys.clone(),
            summary,
            started_ms: self.started,
            updated_ms: self.updated,
            ended_ms: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::Fraction;

    const RUNNING: &str = r#"{"v":1,"type":"update","op":{"id":"1a1886361f4-71100","kind":"apply","source":"fspec:apply:g815","title":"Applying g815","host":"g815","pid":71100,"started":1793289576948,"updated":1793289577014,"state":"running","phase":"committing","scope":"system","progress":{"done":3,"total":12,"unit":"resources"},"item":"zypper: firefox and git","keys":["pkg:firefox","pkg:git"],"result":null,"error":null,"later":"ignored"}}"#;

    #[test]
    fn reads_the_documented_operation() {
        let item = parse(RUNNING).unwrap().item();
        assert_eq!(item.id, "fspec:1a1886361f4-71100");
        assert_eq!(item.source, "fspec:apply:g815");
        assert_eq!(item.state, State::Running);
        assert_eq!(item.detail, "zypper: firefox and git");
        assert_eq!(item.progress.fraction(), Fraction::of(3, 12));
        assert_eq!(item.keys, ["pkg:firefox", "pkg:git"]);
        assert!(!item.urgent);
    }

    #[test]
    fn final_lines_carry_their_sentence() {
        let ok = r#"{"v":1,"type":"update","op":{"id":"a","kind":"run","source":"fspec:run:hello","title":"Running hello","state":"succeeded","phase":"preparing","progress":null,"result":{"changes":0,"summary":"started"}}}"#;
        let item = parse(ok).unwrap().item();
        assert_eq!((item.state, item.summary.as_deref(), &item.kind), (State::Succeeded, Some("started"), &"run".to_string()));
        assert_eq!(item.progress, Progress::Indeterminate);
        assert_eq!(item.detail, "preparing");
        let bad = r#"{"v":1,"type":"update","op":{"id":"b","kind":"apply","state":"failed","error":"zypper exited 104"}}"#;
        let item = parse(bad).unwrap().item();
        assert_eq!((item.state, item.urgent, item.summary.as_deref()), (State::Failed, true, Some("zypper exited 104")));
    }

    #[test]
    fn skips_other_versions_and_garbage() {
        assert!(parse(&RUNNING.replace(r#""v":1"#, r#""v":2"#)).is_none());
        assert!(parse(r#"{"v":1,"type":"update","op":{"id":"1a18"#).is_none());
        assert!(parse("").is_none());
        assert!(parse(r#"{"v":1,"type":"snapshot"}"#).is_none());
    }

    #[test]
    fn unknown_names_pass_through() {
        let line = r#"{"v":1,"type":"snapshot","op":{"id":"c","kind":"rebase","state":"paused","phase":"thinking","progress":{"done":1,"total":2,"unit":"bytes"}}}"#;
        let item = parse(line).unwrap().item();
        assert_eq!((item.kind.as_str(), item.state, item.detail.as_str()), ("rebase", State::Running, "thinking"));
        assert_eq!(item.progress, Progress::Steps { done: 1, total: 2, unit: Unit::Other("bytes".into()) });
        assert_eq!(item.source, "fspec:rebase");
    }
}
