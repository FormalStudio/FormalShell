//! The `hl.*` Lua each write sends.
//! Window ids reach this module as the backend's opaque strings; the
//! `address:0x...` selector is built here and nowhere else.

pub const PARK_WORKSPACE: &str = "special:formalshell-console";
const PARK_NAME: &str = "formalshell-console";

/// DMS's `luaString`: a double-quoted Lua string.
pub fn string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// DMS's `luaValue`: an integer stays bare, anything else is a string.
pub fn value(value: &str) -> String {
    let digits = value.strip_prefix(['-', '+']).unwrap_or(value);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) { value.to_owned() } else { string(value) }
}

pub fn selector(id: &str) -> String {
    if id.starts_with("0x") { format!("address:{id}") } else { format!("address:0x{id}") }
}

/// One argv word, single-quoted for the shell `exec_cmd` hands it to.
fn quote_arg(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

pub fn focus_workspace(id: &str) -> String {
    format!("hl.dsp.focus({{ workspace = {} }})", value(id))
}

pub fn focus_window(id: &str) -> String {
    format!("hl.dsp.focus({{ window = {} }})", string(&selector(id)))
}

/// A bare selector argument is ignored and closes the focused window; only
/// the `window` field picks the target.
pub fn close_window(id: &str) -> String {
    format!("hl.dsp.window.close({{ window = {} }})", string(&selector(id)))
}

pub fn spawn(argv: &[String]) -> String {
    let cmd: Vec<String> = argv.iter().map(|a| quote_arg(a)).collect();
    format!("hl.dsp.exec_cmd({})", string(&cmd.join(" ")))
}

pub fn float_window(id: &str) -> String {
    format!("hl.dsp.window.float({{ window = {}, action = \"set\" }})", string(&selector(id)))
}

pub fn toggle_floating(id: &str) -> String {
    format!("hl.dsp.window.float({{ window = {}, action = \"toggle\" }})", string(&selector(id)))
}

pub fn toggle_fullscreen(id: &str) -> String {
    format!("hl.dsp.window.fullscreen({{ window = {}, mode = \"fullscreen\", action = \"toggle\" }})", string(&selector(id)))
}

pub fn move_to_workspace(id: &str, idx: i64) -> String {
    format!("hl.dsp.window.move({{ window = {}, workspace = {idx}, follow = false }})", string(&selector(id)))
}

pub fn resize_window(id: &str, w: i64, h: i64) -> String {
    format!("hl.dsp.window.resize({{ window = {}, x = {w}, y = {h} }})", string(&selector(id)))
}

pub fn move_window(id: &str, x: i64, y: i64) -> String {
    format!("hl.dsp.window.move({{ window = {}, x = {x}, y = {y} }})", string(&selector(id)))
}

pub fn park_window(id: &str) -> String {
    format!(
        "hl.dsp.window.move({{ window = {}, workspace = {}, follow = false }})",
        string(&selector(id)),
        string(PARK_WORKSPACE)
    )
}

pub fn toggle_special() -> String {
    format!("hl.dsp.workspace.toggle_special({})", string(PARK_NAME))
}

/// A plain Escape bind running `argv`, kept in a Lua global so a later eval
/// removes that one bind (`hl.unbind("Escape")` would take the user's own
/// Escape binds with it). `non_consuming` passes the key on to whatever has
/// focus, so the bind sees it without taking it from the app.
pub fn bind_escape(global: &str, argv: &[String]) -> String {
    format!("{} {global} = hl.bind(\"Escape\", {}, {{ non_consuming = true }})", unbind(global), spawn(argv))
}

/// Showing it goes through the include's `formalshell_arrived` where the
/// config defines it, which also takes the arrival's blank cursor theme off.
pub fn cursor(hidden: bool) -> String {
    let set = format!("hl.config({{ cursor = {{ invisible = {hidden} }} }})");
    if hidden { set } else { format!("if formalshell_arrived then formalshell_arrived() else {set} end") }
}

pub fn exit() -> String {
    "hl.dsp.exit()".into()
}

pub fn unbind(global: &str) -> String {
    format!("if {global} then {global}:remove() {global} = nil end")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worded_as_the_ipc_contract() {
        assert_eq!(focus_workspace("2"), "hl.dsp.focus({ workspace = 2 })");
        assert_eq!(focus_workspace("name"), "hl.dsp.focus({ workspace = \"name\" })");
        assert_eq!(focus_window("55d0c1b0"), "hl.dsp.focus({ window = \"address:0x55d0c1b0\" })");
        assert_eq!(close_window("0xab"), "hl.dsp.window.close({ window = \"address:0xab\" })");
        assert_eq!(
            spawn(&["foot".into(), "-e".into(), "it's".into()]),
            r#"hl.dsp.exec_cmd("'foot' '-e' 'it'\\''s'")"#
        );
        assert_eq!(
            park_window("ab"),
            "hl.dsp.window.move({ window = \"address:0xab\", workspace = \"special:formalshell-console\", follow = false })"
        );
        assert_eq!(toggle_floating("ab"), "hl.dsp.window.float({ window = \"address:0xab\", action = \"toggle\" })");
        assert_eq!(
            toggle_fullscreen("ab"),
            "hl.dsp.window.fullscreen({ window = \"address:0xab\", mode = \"fullscreen\", action = \"toggle\" })"
        );
        assert_eq!(move_to_workspace("ab", 2), "hl.dsp.window.move({ window = \"address:0xab\", workspace = 2, follow = false })");
        assert_eq!(toggle_special(), "hl.dsp.workspace.toggle_special(\"formalshell-console\")");
        assert_eq!(
            bind_escape("fs_esc", &["ipc".into(), "x".into()]),
            r#"if fs_esc then fs_esc:remove() fs_esc = nil end fs_esc = hl.bind("Escape", hl.dsp.exec_cmd("'ipc' 'x'"), { non_consuming = true })"#
        );
        assert_eq!(cursor(true), "hl.config({ cursor = { invisible = true } })");
        assert_eq!(cursor(false), "if formalshell_arrived then formalshell_arrived() else hl.config({ cursor = { invisible = false } }) end");
        assert_eq!(exit(), "hl.dsp.exit()");
        assert_eq!(value("-3"), "-3");
        assert_eq!(value("+"), "\"+\"");
        assert_eq!(string(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
