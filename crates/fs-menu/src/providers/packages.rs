//! Install mode's rows off fspec's answers (FormalSpec
//! `docs/shell-install.md`): `fspec search --json` lines, the
//! `{"op":"<id>"}` an `fspec app ... --detach` prints, and `fspec app
//! owner --json`. The launcher never runs a package manager itself.

use serde::Deserialize;

use super::id_part;
use crate::node::{Kind, Node};

/// A package row's kind: its `path` is the search id.
pub const PACKAGE_KIND: &str = "package";

/// The Install route's id, the level its rows are searched in.
pub const INSTALL_ROUTE: &str = "install";

/// One `fspec search --json` line.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Package {
    /// `<source>:<ref>`, what install, uninstall and run take.
    pub id: String,
    pub name: String,
    pub source: String,
    pub version: Option<String>,
    /// An absolute path to the catalog's icon.
    pub icon: Option<String>,
    pub description: Option<String>,
    pub installed: bool,
    pub installable: bool,
}

/// The packages in the order fspec ranked them; a line that is not one is
/// skipped.
pub fn parse_search(stdout: &str) -> Vec<Package> {
    stdout.lines().filter_map(|l| serde_json::from_str::<Package>(l.trim()).ok()).filter(|p| !p.id.is_empty()).collect()
}

/// The operation id `--detach` prints once the work is on the stream.
pub fn parse_op(stdout: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Started {
        op: String,
    }
    stdout.lines().rev().find_map(|l| serde_json::from_str::<Started>(l.trim()).ok()).map(|s| s.op).filter(|o| !o.is_empty())
}

/// What `fspec app owner --json` says of one desktop file.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Owner {
    pub id: Option<String>,
    /// Only an entry in this host's `apps.star`: what `fspec app
    /// uninstall` takes out.
    pub removable: bool,
}

impl Owner {
    pub fn parse(stdout: &str) -> Owner {
        stdout.lines().rev().find_map(|l| serde_json::from_str::<Owner>(l.trim()).ok()).unwrap_or_default()
    }

    /// The search id an Uninstall takes, when there is one to offer.
    pub fn removable_id(&self) -> Option<&str> {
        self.id.as_deref().filter(|_| self.removable)
    }
}

/// The line a failed fspec call leaves for a row: its last stderr line
/// without fspec's own prefix, else its exit status.
pub fn failure(stderr: &str, code: i32) -> String {
    let line = stderr.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.strip_prefix("fspec: ").unwrap_or(line);
    let line = line.strip_prefix("error: ").or_else(|| line.strip_prefix("Error: ")).unwrap_or(line);
    if line.is_empty() { format!("fspec exited {code}") } else { line.to_owned() }
}

/// What a package row reads on its right at rest: its source and version,
/// or `Installed`.
pub fn package_value(p: &Package) -> String {
    if p.installed {
        return "Installed".into();
    }
    match p.version.as_deref().filter(|v| !v.is_empty()) {
        Some(v) => format!("{} {v}", p.source),
        None => p.source.clone(),
    }
}

/// One row per package. Enter installs and leaves the launcher open on the
/// row; an installed package or one fspec cannot write for this machine
/// runs nothing.
pub fn package_rows(found: &[Package]) -> Vec<Node> {
    found
        .iter()
        .map(|p| {
            let installable = p.installable && !p.installed;
            Node {
                path: p.id.clone(),
                meta: Some(package_value(p)),
                full_text: p.description.clone().unwrap_or_default(),
                icon_source: p.icon.clone().unwrap_or_default(),
                action: installable.then(|| format!("@ipc:package.install:{}", p.id)),
                verb: installable.then(|| "Install".to_owned()),
                checked: p.installed.then(|| PACKAGE_INSTALLED.to_owned()),
                keep_open: Some(true),
                local_only: true,
                ..Node::new(format!("install.pkg.{}", id_part(&p.id)), p.name.clone(), Kind::Other(PACKAGE_KIND.into()))
            }
        })
        .collect()
}

/// The `checked` an installed package's row carries; the launcher ticks
/// it off the row alone.
pub const PACKAGE_INSTALLED: &str = "@package:installed";

pub fn is_package(n: &Node) -> bool {
    matches!(&n.kind, Kind::Other(k) if k == PACKAGE_KIND)
}

pub fn install_searching_row() -> Node {
    Node::note("install.searching", "Searching\u{2026}")
}

pub fn install_no_packages_row(q: &str) -> Node {
    Node::note("install.none", format!("No packages for \u{201C}{q}\u{201D}"))
}

pub fn install_failed_row(why: &str) -> Node {
    Node::note("install.failed", format!("Search failed: {why}"))
}

pub fn install_no_fspec_row() -> Node {
    Node::note("install.nofspec", "Installing apps needs fspec")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = r#"{"id":"flatpak:org.mozilla.firefox","name":"Firefox","source":"flatpak","version":"159.0","icon":"/c/firefox.png","description":"Fast, Private & Safe Web Browser","installed":false,"owner":null,"installable":true,"run":true,"spec":"pkg()"}"#;

    #[test]
    fn reads_search_lines_in_order() {
        let out = format!("{FIREFOX}\nnot json\n{}\n", r#"{"id":"zypper:btop","name":"btop","source":"zypper","version":null,"icon":null,"description":null,"installed":true,"owner":"distro","installable":true}"#);
        let found = parse_search(&out);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "Firefox");
        assert_eq!(package_value(&found[0]), "flatpak 159.0");
        assert_eq!(package_value(&found[1]), "Installed");
    }

    #[test]
    fn rows_install_only_what_fspec_can_write() {
        let mut found = parse_search(FIREFOX);
        found.push(Package { id: "zypper:btop".into(), name: "btop".into(), installed: true, installable: true, ..Package::default() });
        found.push(Package { id: "cask:x".into(), name: "x".into(), installable: false, ..Package::default() });
        let rows = package_rows(&found);
        assert_eq!(rows[0].action.as_deref(), Some("@ipc:package.install:flatpak:org.mozilla.firefox"));
        assert_eq!(rows[0].path, "flatpak:org.mozilla.firefox");
        assert_eq!(rows[0].icon_source, "/c/firefox.png");
        assert!(is_package(&rows[0]));
        assert_eq!(rows[1].action, None);
        assert_eq!(rows[1].checked.as_deref(), Some(PACKAGE_INSTALLED));
        assert_eq!(rows[2].action, None);
        assert_eq!(rows[0].id, "install.pkg.flatpak%3Aorg%2Emozilla%2Efirefox");
    }

    #[test]
    fn enter_installs_and_keeps_the_launcher_open() {
        let rows = package_rows(&parse_search(FIREFOX));
        let s = crate::actions::Subject { node: &rows[0], windows: 0, discrete_gpu: false, picker_select: false, abbrev: None, pinned: false, removable: None };
        let first = crate::actions::first_action(&s).expect("an install");
        assert_eq!(first.id, "install");
        assert!(first.keep_open);
        assert_eq!(first.run, crate::actions::Run::Ipc("package.install:flatpak:org.mozilla.firefox".into()));
        let installed = package_rows(&[Package { id: "zypper:btop".into(), installed: true, installable: true, ..Package::default() }]);
        let s = crate::actions::Subject { node: &installed[0], ..s };
        assert_eq!(crate::actions::first_action(&s), None);
    }

    #[test]
    fn reads_the_detached_op_and_the_owner() {
        assert_eq!(parse_op("{\"op\":\"1a2-3\"}\n").as_deref(), Some("1a2-3"));
        assert_eq!(parse_op("nothing"), None);
        let o = Owner::parse(r#"{"path":"/x.desktop","id":"flatpak:org.mozilla.firefox","pkg":"org.mozilla.firefox","source":"flatpak","owner":"spec","removable":true}"#);
        assert_eq!(o.removable_id(), Some("flatpak:org.mozilla.firefox"));
        let none = Owner::parse(r#"{"path":"/x.desktop","id":null,"pkg":null,"source":null,"owner":null,"removable":false}"#);
        assert_eq!(none.removable_id(), None);
        let distro = Owner::parse(r#"{"path":"/x.desktop","id":"zypper:gimp","owner":"distro","removable":false}"#);
        assert_eq!(distro.removable_id(), None);
    }

    #[test]
    fn a_failure_reads_its_last_line() {
        assert_eq!(failure("warning\nfspec: error: set FSPEC_SPEC\n", 1), "set FSPEC_SPEC");
        assert_eq!(failure("", 2), "fspec exited 2");
    }
}
