// The apps provider's row shape: labels are always the entry's display name
// (id only when the name is genuinely empty), the icon-theme name never leaks
// into the text `icon` slot, and `icon_source` carries the resolver's answer
// verbatim, "" on a failed lookup. The resolver is injected; tests pass a stub
// so the shape stays covered headlessly.

use fs_menu::node::{DesktopEntry, Kind};
use fs_menu::providers::apps_provider;

fn stub_entry(id: &str, name: &str, icon: &str) -> DesktopEntry {
    DesktopEntry { id: id.into(), name: name.into(), icon: icon.into(), ..DesktopEntry::default() }
}

fn resolver(name: &str) -> String {
    if name == "resolvable" { "image://icon/resolvable".to_string() } else { String::new() }
}

#[test]
fn label_is_display_name_never_id() {
    let rows = apps_provider(&[stub_entry("firefox", "Firefox Web Browser", "firefox")], Some(&resolver), &[], 0.0);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "apps.firefox");
    assert_eq!(rows[0].label, "Firefox Web Browser");
    assert_eq!(rows[0].kind, Kind::App);
}

#[test]
fn label_falls_back_to_id_only_when_name_empty() {
    let rows = apps_provider(&[stub_entry("bare-id", "", "")], Some(&resolver), &[], 0.0);
    assert_eq!(rows[0].label, "bare-id");
}

#[test]
fn icon_theme_name_never_rendered_as_text() {
    let rows = apps_provider(&[stub_entry("mpv", "mpv Media Player", "mpv")], Some(&resolver), &[], 0.0);
    assert_eq!(rows[0].icon, "");
}

#[test]
fn icon_source_resolved_through_injected_lookup() {
    let rows = apps_provider(
        &[stub_entry("a", "App A", "resolvable"), stub_entry("b", "App B", "no-such-icon"), stub_entry("c", "App C", "")],
        Some(&resolver),
        &[],
        0.0,
    );
    assert_eq!(rows[0].icon_source, "image://icon/resolvable");
    assert_eq!(rows[1].icon_source, "");
    assert_eq!(rows[2].icon_source, "");
}

#[test]
fn no_resolver_degrades_to_empty_icon_source() {
    let rows = apps_provider(&[stub_entry("a", "App A", "resolvable")], None, &[], 0.0);
    assert_eq!(rows[0].icon_source, "");
}
