//! The argv for a one-off command in the quake console.

/// The argv for a one-off drop-down: the console's own command with its app id
/// swapped for `run_app_id` and `-e <script>` appended. Every emulator spells
/// the class flag differently, so the id is substituted inside the argv the user
/// already wrote rather than appended as a flag this function would have to
/// guess. `None` when that argv never names `app_id`: the one-off would then
/// answer to the console's own id and the console's toggle could pick it up
/// instead of the console, which is worse than not running it.
pub fn one_off_argv(command: Option<&[String]>, app_id: &str, run_app_id: &str, script: &str) -> Option<Vec<String>> {
    let command = command.filter(|c| !c.is_empty())?;
    if app_id.is_empty() || run_app_id.is_empty() || app_id == run_app_id {
        return None;
    }
    let mut named = false;
    let mut out: Vec<String> = Vec::with_capacity(command.len() + 4);
    for arg in command {
        if arg.contains(app_id) {
            named = true;
            out.push(arg.replace(app_id, run_app_id));
        } else {
            out.push(arg.clone());
        }
    }
    if !named {
        return None;
    }
    out.extend(["-e", "sh", "-c", script].map(String::from));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    /// The console's own command, its app id swapped so the standing console
    /// keeps its identity, and the command appended the one way every emulator
    /// the config note names accepts.
    #[test]
    fn one_off_argv_swaps_the_app_id_and_appends_the_command() {
        let argv = one_off_argv(
            Some(&s(&["ghostty", "--class=dev.formalshell.console"])),
            "dev.formalshell.console",
            "dev.formalshell.console.run",
            "nix run nixpkgs#hello; read",
        )
        .unwrap();
        assert_eq!(
            argv,
            s(&["ghostty", "--class=dev.formalshell.console.run", "-e", "sh", "-c", "nix run nixpkgs#hello; read"])
        );
    }

    #[test]
    fn one_off_argv_handles_the_id_in_its_own_argument() {
        let argv = one_off_argv(Some(&s(&["foot", "--app-id", "con"])), "con", "con.run", "true").unwrap();
        assert_eq!(argv.join(" "), "foot --app-id con.run -e sh -c true");
    }

    /// An argv that never names the app id would spawn a second window answering
    /// to the console's own id, which is worse than not running.
    #[test]
    fn one_off_argv_refuses_an_argv_that_never_names_the_app_id() {
        assert_eq!(one_off_argv(Some(&s(&["ghostty"])), "con", "con.run", "true"), None);
        assert_eq!(one_off_argv(Some(&[]), "con", "con.run", "true"), None);
        assert_eq!(one_off_argv(None, "con", "con.run", "true"), None);
        assert_eq!(one_off_argv(Some(&s(&["ghostty", "--class=con"])), "con", "con", "true"), None);
    }
}
