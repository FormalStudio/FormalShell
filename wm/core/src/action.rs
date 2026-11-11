//! What a bind does: the engine's named actions and their arguments, as the
//! settings write them.

use crate::tape::Side;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Width {
    Set(f64),
    Add(f64),
    /// The next preset.
    Cycle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceRef {
    /// A named workspace by its number, from 1.
    Number(usize),
    Previous,
    /// A neighbour in the focused output's stack.
    Relative(isize),
    /// Hyprland's `e+1`: the next workspace of the output that holds a
    /// window, round from the last to the first.
    Occupied(isize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// An argv.
    Spawn(Vec<String>),
    /// A line for `sh -c`.
    Shell(String),
    Terminal,
    Close,
    Fullscreen,
    Maximize,
    Float,
    FocusColumn(Side),
    FocusWindow(Side),
    SwapColumn(Side),
    SwapWindow(Side),
    ColumnWidth(Width),
    Fit,
    ConsumeOrExpel(Side),
    Workspace(WorkspaceRef),
    /// Sends the focused window there and follows it.
    MoveToWorkspace(usize),
    /// Open, close, or toggle without an argument.
    Overview(Option<bool>),
    Zen,
    /// Drags the window under the pointer, on a pointer button.
    MoveWindow,
    ResizeWindow,
    Exit,
}

pub const NAMES: &[&str] = &[
    "spawn",
    "sh",
    "terminal",
    "close",
    "fullscreen",
    "maximize",
    "float",
    "focus-column",
    "focus-window",
    "swap-column",
    "swap-window",
    "column-width",
    "fit",
    "consume-or-expel",
    "workspace",
    "move-to-workspace",
    "overview",
    "zen",
    "move-window",
    "resize-window",
    "exit",
];

fn side(action: &str, arg: Option<&String>, prev: &[&str], next: &[&str]) -> Result<Side, String> {
    match arg.map(String::as_str) {
        Some(arg) if prev.contains(&arg) => Ok(Side::Prev),
        Some(arg) if next.contains(&arg) => Ok(Side::Next),
        other => Err(format!(
            "`{action}` takes {} or {}, not {other:?}",
            prev[0], next[0]
        )),
    }
}

impl Action {
    pub fn parse(name: &str, args: &[String]) -> Result<Self, String> {
        let first = args.first();
        let no_args = |action: Action| {
            if args.is_empty() {
                Ok(action)
            } else {
                Err(format!("`{name}` takes no arguments"))
            }
        };
        match name {
            "spawn" if !args.is_empty() => Ok(Action::Spawn(args.to_vec())),
            "spawn" => Err("`spawn` needs a command".to_owned()),
            "sh" if args.len() == 1 => Ok(Action::Shell(args[0].clone())),
            "sh" => Err("`sh` takes one line".to_owned()),
            "terminal" => no_args(Action::Terminal),
            "close" => no_args(Action::Close),
            "fullscreen" => no_args(Action::Fullscreen),
            "maximize" => no_args(Action::Maximize),
            "float" => no_args(Action::Float),
            "fit" => no_args(Action::Fit),
            "zen" => no_args(Action::Zen),
            "move-window" => no_args(Action::MoveWindow),
            "resize-window" => no_args(Action::ResizeWindow),
            "exit" => no_args(Action::Exit),
            "focus-column" => side(name, first, &["left"], &["right"]).map(Action::FocusColumn),
            "swap-column" => side(name, first, &["left"], &["right"]).map(Action::SwapColumn),
            "focus-window" => side(name, first, &["up"], &["down"]).map(Action::FocusWindow),
            "swap-window" => side(name, first, &["up"], &["down"]).map(Action::SwapWindow),
            "consume-or-expel" => {
                side(name, first, &["prev", "left"], &["next", "right"]).map(Action::ConsumeOrExpel)
            }
            "column-width" => {
                let arg = first.ok_or("`column-width` needs a width, a step or `cycle`")?;
                if arg == "cycle" {
                    return Ok(Action::ColumnWidth(Width::Cycle));
                }
                let value: f64 = arg
                    .parse()
                    .map_err(|_| format!("`column-width`: bad width `{arg}`"))?;
                Ok(Action::ColumnWidth(
                    if arg.starts_with('+') || arg.starts_with('-') {
                        Width::Add(value)
                    } else {
                        Width::Set(value)
                    },
                ))
            }
            "workspace" => {
                let arg = first
                    .ok_or("`workspace` needs a number, `previous`, `+1`, `-1`, `e+1` or `e-1`")?;
                if arg == "previous" {
                    return Ok(Action::Workspace(WorkspaceRef::Previous));
                }
                if let Some(step) = arg.strip_prefix('e') {
                    return step
                        .parse()
                        .ok()
                        .filter(|_| step.starts_with('+') || step.starts_with('-'))
                        .map(|step| Action::Workspace(WorkspaceRef::Occupied(step)))
                        .ok_or_else(|| format!("`workspace`: bad workspace `{arg}`"));
                }
                let value: isize = arg
                    .parse()
                    .map_err(|_| format!("`workspace`: bad workspace `{arg}`"))?;
                Ok(Action::Workspace(
                    if arg.starts_with('+') || arg.starts_with('-') {
                        WorkspaceRef::Relative(value)
                    } else if value >= 1 {
                        WorkspaceRef::Number(value as usize)
                    } else {
                        return Err(format!("`workspace`: bad workspace `{arg}`"));
                    },
                ))
            }
            "move-to-workspace" => first
                .and_then(|arg| arg.parse::<usize>().ok())
                .filter(|number| *number >= 1)
                .map(Action::MoveToWorkspace)
                .ok_or_else(|| "`move-to-workspace` needs a workspace number".to_owned()),
            "overview" => match first.map(String::as_str) {
                None | Some("toggle") => Ok(Action::Overview(None)),
                Some("open") => Ok(Action::Overview(Some(true))),
                Some("close") => Ok(Action::Overview(Some(false))),
                Some(other) => Err(format!(
                    "`overview` takes open, close or toggle, not `{other}`"
                )),
            },
            other => Err(format!("unknown action `{other}`")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(name: &str, args: &[&str]) -> Result<Action, String> {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        Action::parse(name, &args)
    }

    #[test]
    fn every_listed_name_parses_with_some_argument() {
        let samples: &[(&str, &[&str])] = &[
            ("spawn", &["ghostty"]),
            ("sh", &["a && b"]),
            ("focus-column", &["left"]),
            ("focus-window", &["down"]),
            ("swap-column", &["right"]),
            ("swap-window", &["up"]),
            ("column-width", &["cycle"]),
            ("consume-or-expel", &["prev"]),
            ("workspace", &["3"]),
            ("move-to-workspace", &["3"]),
        ];
        for name in NAMES {
            let args = samples
                .iter()
                .find(|(sample, _)| sample == name)
                .map_or(&[][..], |(_, args)| args);
            assert!(parse(name, args).is_ok(), "{name} {args:?}");
        }
    }

    #[test]
    fn widths_set_step_and_cycle() {
        assert_eq!(
            parse("column-width", &["1.0"]),
            Ok(Action::ColumnWidth(Width::Set(1.0)))
        );
        assert_eq!(
            parse("column-width", &["+0.1"]),
            Ok(Action::ColumnWidth(Width::Add(0.1)))
        );
        assert_eq!(
            parse("column-width", &["-0.1"]),
            Ok(Action::ColumnWidth(Width::Add(-0.1)))
        );
        assert!(parse("column-width", &["wide"]).is_err());
    }

    #[test]
    fn workspaces_by_number_neighbour_and_previous() {
        assert_eq!(
            parse("workspace", &["4"]),
            Ok(Action::Workspace(WorkspaceRef::Number(4)))
        );
        assert_eq!(
            parse("workspace", &["+1"]),
            Ok(Action::Workspace(WorkspaceRef::Relative(1)))
        );
        assert_eq!(
            parse("workspace", &["-1"]),
            Ok(Action::Workspace(WorkspaceRef::Relative(-1)))
        );
        assert_eq!(
            parse("workspace", &["previous"]),
            Ok(Action::Workspace(WorkspaceRef::Previous))
        );
        assert_eq!(
            parse("workspace", &["e+1"]),
            Ok(Action::Workspace(WorkspaceRef::Occupied(1)))
        );
        assert_eq!(
            parse("workspace", &["e-1"]),
            Ok(Action::Workspace(WorkspaceRef::Occupied(-1)))
        );
        assert!(parse("workspace", &["e1"]).is_err());
        assert!(parse("workspace", &["0"]).is_err());
    }

    #[test]
    fn what_is_refused() {
        assert!(parse("spawn", &[]).is_err());
        assert!(parse("close", &["now"]).is_err());
        assert!(parse("focus-column", &["up"]).is_err());
        assert!(parse("launch", &[]).is_err());
    }
}
