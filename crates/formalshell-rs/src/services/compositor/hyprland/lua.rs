//! The `hl.*` Lua each write sends.
//! Window ids reach this module as the backend's opaque ids; the
//! `address:0x...` selector is built here and nowhere else.

use fs_types::{IRect, WindowId, WorkspaceId};

use crate::services::compositor::ZenKeys;

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

fn selector(id: &WindowId) -> String {
    let id = id.as_str();
    if id.starts_with("0x") { format!("address:{id}") } else { format!("address:0x{id}") }
}

/// One argv word, single-quoted for the shell `exec_cmd` hands it to.
fn quote_arg(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

pub fn focus_workspace(id: &WorkspaceId) -> String {
    format!("hl.dsp.focus({{ workspace = {} }})", value(id.as_str()))
}

pub fn focus_workspace_at(idx: i64) -> String {
    format!("hl.dsp.focus({{ workspace = {idx} }})")
}

pub fn focus_window(id: &WindowId) -> String {
    format!("hl.dsp.focus({{ window = {} }})", string(&selector(id)))
}

/// A bare selector argument is ignored and closes the focused window; only
/// the `window` field picks the target.
pub fn close_window(id: &WindowId) -> String {
    format!("hl.dsp.window.close({{ window = {} }})", string(&selector(id)))
}

pub fn spawn(argv: &[String]) -> String {
    let cmd: Vec<String> = argv.iter().map(|a| quote_arg(a)).collect();
    format!("hl.dsp.exec_cmd({})", string(&cmd.join(" ")))
}

pub fn tag_mac(id: &WindowId) -> String {
    format!("hl.dsp.window.tag({{ tag = \"+{}\", window = {} }})", fs_types::MAC_TAG, string(&selector(id)))
}

pub fn float_window(id: &WindowId) -> String {
    format!("hl.dsp.window.float({{ window = {}, action = \"set\" }})", string(&selector(id)))
}

pub fn toggle_floating(id: &WindowId) -> String {
    format!("hl.dsp.window.float({{ window = {}, action = \"toggle\" }})", string(&selector(id)))
}

pub fn toggle_fullscreen(id: &WindowId) -> String {
    format!("hl.dsp.window.fullscreen({{ window = {}, mode = \"fullscreen\", action = \"toggle\" }})", string(&selector(id)))
}

pub fn move_to_workspace(id: &WindowId, idx: i64) -> String {
    format!("hl.dsp.window.move({{ window = {}, workspace = {idx}, follow = false }})", string(&selector(id)))
}

pub fn swap_windows(a: &WindowId, b: &WindowId) -> String {
    format!("hl.dsp.window.swap({{ window = {}, target = {} }})", string(&selector(a)), string(&selector(b)))
}

/// The scrolling layout has no move-column-to-index (0.56 and 0.57 alike):
/// `swapcol` trades the focused column with a neighbour, so the column is
/// focused and walked there one swap at a time. Columns are told apart by
/// their windows' x on the tape.
pub fn move_column(id: &WindowId, before: Option<&WindowId>) -> String {
    let window = string(&selector(id));
    let before = before.map_or_else(|| "nil".to_owned(), |b| format!("hl.get_window({})", string(&selector(b))));
    format!(
        "local w = hl.get_window({window}) \
         if w and w.workspace and not w.floating then \
         local xs, seen = {{}}, {{}} \
         for _, c in ipairs(hl.get_workspace_windows(w.workspace.id)) do \
         if not c.floating and not seen[c.at.x] then seen[c.at.x] = true; xs[#xs + 1] = c.at.x end end \
         table.sort(xs) \
         local from, rest = nil, {{}} \
         for i, x in ipairs(xs) do if x == w.at.x then from = i else rest[#rest + 1] = x end end \
         local to, b = #rest + 1, {before} \
         if b then for i, x in ipairs(rest) do if x == b.at.x then to = i end end end \
         if from and to ~= from then \
         hl.dispatch(hl.dsp.focus({{ window = {window} }})) \
         for _ = 1, math.abs(to - from) do hl.dispatch(hl.dsp.layout(to > from and \"swapcol r\" or \"swapcol l\")) end \
         end end"
    )
}

pub fn resize_window(id: &WindowId, w: i64, h: i64) -> String {
    format!("hl.dsp.window.resize({{ window = {}, x = {w}, y = {h} }})", string(&selector(id)))
}

pub fn move_window(id: &WindowId, x: i64, y: i64) -> String {
    format!("hl.dsp.window.move({{ window = {}, x = {x}, y = {y} }})", string(&selector(id)))
}

/// The first half of zen under the scrolling layout: the window's column
/// widened to the whole output, so the tape is never narrower than the
/// output when [`zen`] slides it, and the width it had kept in
/// `formalshell_zen` for the way back. A chunk of its own, so the layout
/// has scrolled the wider column into view before [`zen`] reads where the
/// tape ends.
pub fn zen_widen(id: &WindowId) -> String {
    let window = string(&selector(id));
    format!(
        "local w = hl.get_window({window}) \
         if w and w.workspace and w.workspace.tiled_layout == \"scrolling\" and w.layout and w.layout.column then \
         formalshell_zen = formalshell_zen or {{}} \
         formalshell_zen[{window}] = w.layout.column.width \
         hl.dispatch(hl.dsp.focus({{ window = {window} }})) \
         hl.dispatch(hl.dsp.layout(\"colresize 1.0\")) \
         end"
    )
}

/// A window stacked in a column put in a column of its own, left of the
/// column it leaves: floated out of a column it shares, it would leave the
/// rest of the column where the layout fits it, on the output, and as the
/// last column it would be the one the layout never scrolls off. A chunk of
/// its own, ahead of [`zen_widen`], which would otherwise widen the column
/// the window has just left. It comes back from zen as a column of its own
/// all the same.
pub fn zen_expel(id: &WindowId) -> String {
    let window = string(&selector(id));
    format!(
        "local w = hl.get_window({window}) \
         if w and w.workspace and w.workspace.tiled_layout == \"scrolling\" and not w.floating then \
         local mates = 0 \
         for _, c in ipairs(hl.get_workspace_windows(w.workspace.id)) do \
         if not c.floating and c.at.x == w.at.x then mates = mates + 1 end end \
         if mates > 1 then \
         hl.dispatch(hl.dsp.focus({{ window = {window} }})) \
         hl.dispatch(hl.dsp.layout(\"consume_or_expel prev\")) \
         end end"
    )
}

/// A window back from zen, its column just left of `mate`'s, taken into
/// that column right above `below` (at its foot with none). A window taken
/// into a column lands at its foot, so it is swapped up past each window
/// from the foot to `below`, read before it moves.
pub fn zen_restack(id: &WindowId, mate: &WindowId, below: Option<&WindowId>) -> String {
    let window = string(&selector(id));
    let below = below.map_or_else(|| "nil".to_owned(), |b| format!("hl.get_window({})", string(&selector(b))));
    format!(
        "local w, m, b = hl.get_window({window}), hl.get_window({}), {below} \
         if w and m and not w.floating and not m.floating and w.workspace and m.workspace and w.workspace.id == m.workspace.id then \
         local column = {{}} \
         for _, c in ipairs(hl.get_workspace_windows(m.workspace.id)) do \
         if not c.floating and c.at.x == m.at.x then column[#column + 1] = c end end \
         table.sort(column, function(p, q) return p.at.y < q.at.y end) \
         hl.dispatch(hl.dsp.focus({{ window = {window} }})) \
         hl.dispatch(hl.dsp.layout(\"consume_or_expel next\")) \
         if b then for i = #column, 1, -1 do \
         local a = tostring(column[i].address) \
         hl.dispatch(hl.dsp.window.swap({{ window = {window}, target = \"address:\" .. (a:sub(1, 2) == \"0x\" and a or \"0x\" .. a) }})) \
         if column[i].address == b.address then break end \
         end end end",
        string(&selector(mate))
    )
}

/// Under the scrolling layout the tape slides off the output's left edge
/// and is pinned there with `inhibit_scroll` (0.56.2): a tape narrower than
/// the output is centred again on every recalculate, and the inhibitor is
/// the one thing that holds its offset. Then the window floats at `rect`,
/// its column closing up off screen. With the tape `held` off screen
/// already, zen moving to a window of it, the tape stays where it is and
/// only the window's column width is kept for the way back.
pub fn zen(id: &WindowId, rect: IRect, held: bool) -> String {
    let window = string(&selector(id));
    let tape = if held {
        format!(
            "if w.layout and w.layout.column then \
             formalshell_zen = formalshell_zen or {{}} \
             formalshell_zen[{window}] = w.layout.column.width \
             end "
        )
    } else {
        "local x0, right = w.monitor.x, w.monitor.x \
         for _, c in ipairs(hl.get_workspace_windows(w.workspace.id)) do \
         if not c.floating and c.at.x + c.size.x > right then right = c.at.x + c.size.x end end \
         hl.dispatch(hl.dsp.layout(\"inhibit_scroll 0\")) \
         if right > x0 then hl.dispatch(hl.dsp.layout(\"move -\" .. (right - x0))) end \
         hl.dispatch(hl.dsp.layout(\"inhibit_scroll 1\")) "
            .to_owned()
    };
    format!(
        "local w = hl.get_window({window}) \
         if w and w.workspace then \
         if w.workspace.tiled_layout == \"scrolling\" then \
         {tape}\
         end \
         hl.dispatch(hl.dsp.window.float({{ window = {window}, action = \"set\" }})) \
         hl.dispatch(hl.dsp.window.resize({{ window = {window}, x = {}, y = {} }})) \
         hl.dispatch(hl.dsp.window.move({{ window = {window}, x = {}, y = {} }})) \
         hl.dispatch(hl.dsp.focus({{ window = {window} }})) \
         end",
        rect.w.max(1),
        rect.h.max(1),
        rect.x,
        rect.y,
    )
}

/// The window tiled again at the column width it had (it comes back as a
/// column of the default width), or put back at its old rect if it was
/// floating before, and the tape let go unless zen stays `held` on it.
pub fn unzen(id: &WindowId, floating: Option<IRect>, held: bool) -> String {
    let window = string(&selector(id));
    let release = if held { "" } else { "hl.dispatch(hl.dsp.layout(\"inhibit_scroll 0\"))" };
    let back = format!("hl.dispatch(hl.dsp.focus({{ window = {window} }})) {release}");
    match floating {
        Some(r) => format!(
            "{back} hl.dispatch(hl.dsp.window.resize({{ window = {window}, x = {}, y = {} }})) \
             hl.dispatch(hl.dsp.window.move({{ window = {window}, x = {}, y = {} }}))",
            r.w.max(1),
            r.h.max(1),
            r.x,
            r.y,
        ),
        None => format!(
            "{back} hl.dispatch(hl.dsp.window.float({{ window = {window}, action = \"unset\" }})) \
             local width = formalshell_zen and formalshell_zen[{window}] \
             if width then \
             formalshell_zen[{window}] = nil \
             hl.dispatch(hl.dsp.focus({{ window = {window} }})) \
             hl.dispatch(hl.dsp.layout(\"colresize \" .. width)) \
             end"
        ),
    }
}

/// The include's `formalshell_zen_keys`: while `id` is the zen window and
/// in front, each chord the config gave a focus move to a side takes the
/// focus to the window `keys` names for that side. None lets go of them.
/// Nothing under a config whose include predates it.
pub fn zen_keys(id: Option<&WindowId>, keys: &ZenKeys) -> String {
    let Some(id) = id else {
        return "if formalshell_zen_keys then formalshell_zen_keys(nil) end".into();
    };
    let side = |key: &str, target: &Option<WindowId>| target.as_ref().map(|t| format!("{key} = {}", string(&selector(t))));
    let sides: Vec<String> =
        [side("l", &keys.left), side("r", &keys.right), side("u", &keys.up), side("d", &keys.down)].into_iter().flatten().collect();
    format!("if formalshell_zen_keys then formalshell_zen_keys({}, {{ {} }}) end", string(&selector(id)), sides.join(", "))
}

/// Lets go of the tape on the focused workspace, the zen window having
/// closed while it held it, and brings the focused column back in view:
/// the focus the close moved was refused while the tape was held.
pub fn zen_release() -> String {
    "hl.dispatch(hl.dsp.layout(\"inhibit_scroll 0\")) hl.dispatch(hl.dsp.layout(\"fit_into_view\"))".into()
}

pub fn park_window(id: &WindowId) -> String {
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
/// focus, so the bind sees it without taking it from the app. It is made
/// past the include's recorder, which would copy it into the `mac` submap
/// and leave the copy behind, and is live in every submap instead.
pub fn bind_escape(global: &str, argv: &[String]) -> String {
    format!(
        "{} {global} = (formalshell_raw and formalshell_raw.bind or hl.bind)(\"Escape\", {}, {{ non_consuming = true, submap_universal = true }})",
        unbind(global),
        spawn(argv)
    )
}

/// What [`mac_keys`] raises ahead of the count.
const RECORDED: &str = "formalshell recorded ";

/// The include's `formalshell_mac_keys`: the `mac` submap out of the binds
/// it recorded, less the Super chords not in `reserved`. Nothing under a
/// config that does not load the include's MAC APPS block, as
/// [`mac_focus`] is. `hyprctl eval` answers `ok` whatever a chunk returns,
/// so the count the include answers with is raised, for [`mac_recorded`].
pub fn mac_keys(reserved: &[String]) -> String {
    let list: Vec<String> = reserved.iter().map(|c| string(c)).collect();
    format!("if formalshell_mac_keys then error(\"{RECORDED}\" .. tostring(formalshell_mac_keys({{ {} }})), 0) end", list.join(", "))
}

/// How many binds the include recorded, out of what [`mac_keys`] was
/// answered.
pub fn mac_recorded(answer: &str) -> Option<u32> {
    answer.rsplit_once(RECORDED)?.1.trim().parse().ok()
}

/// The include's `formalshell_mac_focus`: whether `window`, the one the
/// shell found in front, is a Mac window. The include drops the answer when
/// another window is in front by the time it arrives.
pub fn mac_focus(on: bool, window: &WindowId) -> String {
    let asked = if window.is_empty() { "nil".to_owned() } else { string(&selector(window)) };
    format!("if formalshell_mac_focus then formalshell_mac_focus({on}, {asked}) end")
}

/// The include's `formalshell_mac_app`: into the `macapp` submap, where each
/// of `chords` runs `appmenu key` on it, or out of it with none.
pub fn mac_app(chords: Option<&[String]>) -> String {
    let Some(chords) = chords else { return "if formalshell_mac_app then formalshell_mac_app(nil) end".into() };
    let binds: Vec<String> = chords
        .iter()
        .map(|c| format!("{{ chord = {}, action = {} }}", string(c), spawn(&crate::services::compositor::ipc_argv(&["appmenu", "key", c]))))
        .collect();
    format!("if formalshell_mac_app then formalshell_mac_app({{ {} }}) end", binds.join(", "))
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

    fn id(s: &str) -> WindowId {
        WindowId::new(s)
    }

    #[test]
    fn worded_as_the_ipc_contract() {
        assert_eq!(focus_workspace(&WorkspaceId::new("2")), "hl.dsp.focus({ workspace = 2 })");
        assert_eq!(focus_workspace(&WorkspaceId::new("name")), "hl.dsp.focus({ workspace = \"name\" })");
        assert_eq!(focus_workspace_at(3), "hl.dsp.focus({ workspace = 3 })");
        assert_eq!(focus_window(&id("55d0c1b0")), "hl.dsp.focus({ window = \"address:0x55d0c1b0\" })");
        assert_eq!(close_window(&id("0xab")), "hl.dsp.window.close({ window = \"address:0xab\" })");
        assert_eq!(
            spawn(&["foot".into(), "-e".into(), "it's".into()]),
            r#"hl.dsp.exec_cmd("'foot' '-e' 'it'\\''s'")"#
        );
        assert_eq!(
            park_window(&id("ab")),
            "hl.dsp.window.move({ window = \"address:0xab\", workspace = \"special:formalshell-console\", follow = false })"
        );
        assert_eq!(toggle_floating(&id("ab")), "hl.dsp.window.float({ window = \"address:0xab\", action = \"toggle\" })");
        assert_eq!(
            toggle_fullscreen(&id("ab")),
            "hl.dsp.window.fullscreen({ window = \"address:0xab\", mode = \"fullscreen\", action = \"toggle\" })"
        );
        assert_eq!(swap_windows(&id("ab"), &id("cd")), "hl.dsp.window.swap({ window = \"address:0xab\", target = \"address:0xcd\" })");
        let col = move_column(&id("ab"), Some(&id("cd")));
        assert!(col.starts_with("local w = hl.get_window(\"address:0xab\") if w"));
        assert!(col.contains("local to, b = #rest + 1, hl.get_window(\"address:0xcd\") if b"));
        assert!(col.contains("hl.dsp.layout(to > from and \"swapcol r\" or \"swapcol l\")"));
        assert!(move_column(&id("ab"), None).contains("#rest + 1, nil if b"));
        let back = zen_restack(&id("ab"), &id("cd"), Some(&id("ef")));
        assert!(back.starts_with("local w, m, b = hl.get_window(\"address:0xab\"), hl.get_window(\"address:0xcd\"), hl.get_window(\"address:0xef\") if"));
        assert!(back.contains("hl.dsp.layout(\"consume_or_expel next\")"));
        assert!(zen_restack(&id("ab"), &id("cd"), None).contains("hl.get_window(\"address:0xcd\"), nil if"));
        assert_eq!(move_to_workspace(&id("ab"), 2), "hl.dsp.window.move({ window = \"address:0xab\", workspace = 2, follow = false })");
        assert_eq!(toggle_special(), "hl.dsp.workspace.toggle_special(\"formalshell-console\")");
        assert_eq!(
            bind_escape("fs_esc", &["ipc".into(), "x".into()]),
            r#"if fs_esc then fs_esc:remove() fs_esc = nil end fs_esc = (formalshell_raw and formalshell_raw.bind or hl.bind)("Escape", hl.dsp.exec_cmd("'ipc' 'x'"), { non_consuming = true, submap_universal = true })"#
        );
        assert_eq!(
            mac_keys(&["SUPER + H".into(), "SUPER + 1".into()]),
            r#"if formalshell_mac_keys then error("formalshell recorded " .. tostring(formalshell_mac_keys({ "SUPER + H", "SUPER + 1" })), 0) end"#
        );
        assert_eq!(mac_keys(&[]), r#"if formalshell_mac_keys then error("formalshell recorded " .. tostring(formalshell_mac_keys({  })), 0) end"#);
        assert_eq!(mac_recorded("error: formalshell recorded 41\n"), Some(41));
        assert_eq!(mac_recorded("error: formalshell recorded nil"), None);
        assert_eq!(mac_recorded("ok"), None);
        assert_eq!(mac_focus(true, &id("ab")), r#"if formalshell_mac_focus then formalshell_mac_focus(true, "address:0xab") end"#);
        assert_eq!(mac_focus(false, &id("")), "if formalshell_mac_focus then formalshell_mac_focus(false, nil) end");
        assert_eq!(cursor(true), "hl.config({ cursor = { invisible = true } })");
        assert_eq!(cursor(false), "if formalshell_arrived then formalshell_arrived() else hl.config({ cursor = { invisible = false } }) end");
        assert_eq!(exit(), "hl.dsp.exit()");
        assert_eq!(value("-3"), "-3");
        assert_eq!(value("+"), "\"+\"");
        assert_eq!(string(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
