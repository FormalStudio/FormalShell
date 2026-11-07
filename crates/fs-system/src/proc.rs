//! Long-lived children of the shell, wrapped so the kernel kills them when
//! the shell dies.
//!
//! `systemctl --user restart formalshell` kills the shell outright, and every
//! child it owned is reparented to systemd and left running. systemd does not
//! clean them up either, because the unit sets KillMode=process deliberately:
//! outside a uwsm session, apps launched from the launcher share the shell's
//! cgroup and have to survive a restart.
//!
//! PR_SET_PDEATHSIG closes every exit path. setpriv sets it and execs in
//! place, so the wrapped command is still the shell's own direct child and
//! its pid is the one a signal has to reach, which the night light's SIGUSR1
//! handshake depends on.
//!
//! Only for a child that outlives its own call: a one-shot exits long before
//! a restart can orphan it.


pub fn die_with_parent(argv: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ["setpriv", "--pdeathsig", "TERM", "--"].map(String::from).to_vec();
    out.extend(argv.iter().cloned());
    out
}

/// The parts of a desktop entry `app_launch` reads.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub id: String,
    pub command: Vec<String>,
    pub run_in_terminal: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Action {
    pub command: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_wrapped_command_survives_verbatim() {
        let argv = die_with_parent(&s(&["wl-paste", "--type", "text", "--watch", "sh", "-c", "cat"]));
        assert_eq!(argv[4..], s(&["wl-paste", "--type", "text", "--watch", "sh", "-c", "cat"])[..]);
    }

    #[test]
    fn setpriv_options_end_before_the_command() {
        let argv = die_with_parent(&s(&["cava", "-p", "/tmp/cfg"]));
        assert_eq!(argv[..4], s(&["setpriv", "--pdeathsig", "TERM", "--"])[..]);
        assert_eq!(argv.iter().position(|a| a == "--"), Some(3));
    }

    #[test]
    fn an_empty_command_is_not_an_error() {
        assert_eq!(die_with_parent(&[]), s(&["setpriv", "--pdeathsig", "TERM", "--"]));
    }

    #[test]
    fn the_callers_array_is_left_alone() {
        let argv = s(&["ttfx", "--effect", "rain"]);
        let _ = die_with_parent(&argv);
        assert_eq!(argv, s(&["ttfx", "--effect", "rain"]));
    }

}
