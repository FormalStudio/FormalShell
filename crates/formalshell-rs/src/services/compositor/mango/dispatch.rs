//! The `dispatch` lines each write sends over mango's socket
//! (`handle_command` in src/ipc/ipc.c at 0.18.0). Window ids reach this
//! module as the backend's opaque strings; the `client,<id>` selector is
//! built here and nowhere else.
//!
//! The line is split on every comma before the dispatcher sees it, ends at
//! the first newline, and any `client,<digits>` in it is taken as the
//! target, so nothing caller-supplied may carry a comma or a newline.

fn client(id: &str) -> String {
    format!(" client,{id}")
}

/// A monitor spec that matches `name` alone: mango matches a spec without a
/// colon as an unanchored PCRE2 pattern, so "DP-1" would also take "eDP-1".
pub fn monitor(name: &str) -> String {
    let mut out = String::from("^");
    for c in name.chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('$');
    out
}

pub const QUIT: &str = "quit";

pub fn view(monitor_name: &str, tag: i64) -> String {
    format!("viewcrossmon,{tag},{}", monitor(monitor_name))
}

pub fn focus_window(id: &str) -> String {
    format!("focusid{}", client(id))
}

pub fn close_window(id: &str) -> String {
    format!("killclient{}", client(id))
}

pub fn float_window(id: &str) -> String {
    format!("togglefloating{}", client(id))
}

/// `movewin` and `resizewin` read a leading `-` or `+` as a step from where
/// the window is, so an absolute value can only be written from 0 up.
pub fn resize_window(id: &str, w: i64, h: i64) -> String {
    format!("resizewin,{},{}{}", w.max(1), h.max(1), client(id))
}

pub fn move_window(id: &str, x: i64, y: i64) -> String {
    format!("movewin,{},{}{}", x.max(0), y.max(0), client(id))
}

/// Onto the special tag (0), off every view until it is brought back.
/// `tag_special_silent` toggles, so it is only sent for a window not there.
pub fn park_window(id: &str) -> String {
    format!("tag_special_silent{}", client(id))
}

pub fn move_to(id: &str, monitor_name: &str, tag: i64) -> String {
    format!("tagcrossmon,{tag},{}{}", monitor(monitor_name), client(id))
}

pub fn enable_output(name: &str, on: bool) -> String {
    format!("{},{}", if on { "enable_monitor" } else { "disable_monitor" }, monitor(name))
}

pub const RELOAD: &str = "reload_config";

/// One argv word, single-quoted for `sh -c`.
fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// `spawn_shell` runs the line under `sh -c` as a child of mango. A comma
/// or a newline in an argument cannot cross the socket, so the line spells
/// them as shell variables set by a preamble that holds neither.
pub fn spawn(argv: &[String]) -> String {
    let joined = argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let needs = (joined.contains(','), joined.contains('\n'));
    let body = joined.replace(',', "'\"$fs_c\"'").replace('\n', "'\"$fs_n\"'");
    let mut pre = String::new();
    if needs.0 {
        pre.push_str("fs_c=$(printf '\\054'); ");
    }
    if needs.1 {
        pre.push_str("fs_n=$(printf '\\012x'); fs_n=${fs_n%x}; ");
    }
    format!("spawn_shell,{pre}exec {body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worded_as_mango_parses_them() {
        assert_eq!(view("HDMI-A-1", 3), "viewcrossmon,3,^HDMI-A-1$");
        assert_eq!(monitor("a.b(1)"), "^a\\.b\\(1\\)$");
        assert_eq!(focus_window("12"), "focusid client,12");
        assert_eq!(close_window("12"), "killclient client,12");
        assert_eq!(float_window("12"), "togglefloating client,12");
        assert_eq!(resize_window("12", 800, 0), "resizewin,800,1 client,12");
        assert_eq!(move_window("12", -20, 40), "movewin,0,40 client,12");
        assert_eq!(park_window("12"), "tag_special_silent client,12");
        assert_eq!(move_to("12", "eDP-1", 2), "tagcrossmon,2,^eDP-1$ client,12");
        assert_eq!(enable_output("DP-1", false), "disable_monitor,^DP-1$");
    }

    #[test]
    fn spawn_keeps_commas_and_newlines_off_the_wire() {
        assert_eq!(spawn(&["foot".into(), "-e".into(), "it's".into()]), r#"spawn_shell,exec 'foot' '-e' 'it'\''s'"#);
        let line = spawn(&["sh".into(), "-c".into(), "echo a,b\nc".into()]);
        assert!(!line.trim_start_matches("spawn_shell,").contains(',') && !line.contains('\n'), "{line}");
        let out = std::process::Command::new("sh").arg("-c").arg(line.trim_start_matches("spawn_shell,").replacen("exec ", "printf '%s|' ", 1)).output().unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "sh|-c|echo a,b\nc|");
    }
}
