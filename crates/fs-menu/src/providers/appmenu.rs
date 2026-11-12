//! The focused Mac app's menu commands as launcher rows: the Mac's Help
//! search, with the keyboard first.

use crate::actions::IpcCall;
use crate::node::{Kind, Node};
use crate::toggles::Checked;

pub const APP_MENU_ROUTE: &str = "appmenu";

/// One command of the app's menu: a leaf, with the titles above it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuCommand {
    /// Its dbusmenu id, which `appmenu run` takes.
    pub id: i32,
    pub label: String,
    pub path: Vec<String>,
    pub enabled: bool,
    pub checked: bool,
    /// Its key equivalent as the bus names it: modifiers, then the key.
    pub shortcut: Vec<String>,
}

/// Whether `node` is a row of the route, for the root query's type order.
pub fn is_menu_command(node: &Node) -> bool {
    node.parent_id.as_deref() == Some(APP_MENU_ROUTE)
}

/// `None` is no Mac app in front. A row's id is its path, not its dbusmenu
/// id: the picks ledger then learns "File/Export/PDF" across runs, where
/// the ids are the app's to renumber. A disabled command is listed inside
/// the route alone, dim and inert.
pub fn app_menu_provider(commands: Option<&[MenuCommand]>) -> Vec<Node> {
    let Some(commands) = commands else {
        return vec![Node { local_only: true, ..Node::note("appmenu.none", "No Mac app in front") }];
    };
    let mut seen = std::collections::HashSet::new();
    commands
        .iter()
        .map(|c| {
            let mut id = format!("{APP_MENU_ROUTE}.{}", c.path.iter().chain([&c.label]).map(String::as_str).collect::<Vec<_>>().join("/"));
            if !seen.insert(id.clone()) {
                id = format!("{id}#{}", c.id);
            }
            let row = Node {
                desc: Some(c.path.join(" \u{203A} ")),
                keys: c.shortcut.clone(),
                checked: c.checked.then_some(Checked::Always),
                ..Node::default()
            };
            if c.enabled {
                Node { action: Some(IpcCall::new("appmenu", "run").arg(c.id.to_string()).into()), ..Node { id, label: c.label.clone(), kind: Kind::Action, ..row } }
            } else {
                Node { dim: Some(true), local_only: true, ..Node { id, label: c.label.clone(), kind: Kind::Note, ..row } }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Run;

    fn command(id: i32, path: &[&str], label: &str) -> MenuCommand {
        MenuCommand { id, label: label.into(), path: path.iter().map(|p| p.to_string()).collect(), enabled: true, ..MenuCommand::default() }
    }

    #[test]
    fn a_command_runs_by_its_id_under_an_id_made_of_its_path() {
        let save = MenuCommand { shortcut: vec!["Super".into(), "S".into()], checked: true, ..command(7, &["File"], "Save") };
        let rows = app_menu_provider(Some(&[save]));
        assert_eq!(rows[0].id, "appmenu.File/Save");
        assert_eq!(rows[0].desc.as_deref(), Some("File"));
        assert_eq!(rows[0].keys, ["Super", "S"]);
        assert_eq!(rows[0].checked, Some(Checked::Always));
        assert_eq!(rows[0].action, Some(Run::Ipc(IpcCall::new("appmenu", "run").arg("7"))));
    }

    #[test]
    fn a_disabled_command_is_an_inert_row_of_the_route_alone() {
        let undo = MenuCommand { enabled: false, ..command(3, &["Edit"], "Undo") };
        let rows = app_menu_provider(Some(&[undo]));
        assert_eq!((rows[0].kind.clone(), rows[0].dim, rows[0].local_only), (Kind::Note, Some(true), true));
        assert_eq!(rows[0].action, None);
    }

    #[test]
    fn two_commands_of_one_path_keep_ids_apart() {
        let rows = app_menu_provider(Some(&[command(4, &["Window"], "Untitled"), command(5, &["Window"], "Untitled")]));
        assert_eq!(rows[0].id, "appmenu.Window/Untitled");
        assert_eq!(rows[1].id, "appmenu.Window/Untitled#5");
    }

    #[test]
    fn no_mac_app_is_one_note_the_root_query_never_ranks() {
        let rows = app_menu_provider(None);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].local_only);
    }
}
