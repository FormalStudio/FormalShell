//! What argv asks `formalshell` to be: the shell itself (no arguments), or
//! one of its subcommands. Anything else is a usage error, never a second
//! shell taking over the first.

#[derive(Debug, PartialEq, Eq)]
pub enum Command<'a> {
    Shell,
    Greeter(&'a [String]),
    Install(&'a str, &'a [String]),
    /// `formalshell ipc [qs flags] <call|show|...> ...`, the words after `ipc`.
    Ipc(&'a [String]),
    /// `formalshell theme <fn> [args...]`, the words after `theme`: an IPC
    /// call on the `theme` target.
    Theme(&'a [String]),
    Usage,
}

pub fn parse(args: &[String]) -> Command<'_> {
    let Some(first) = args.get(1) else { return Command::Shell };
    let rest = &args[2..];
    match first.as_str() {
        "greeter" => Command::Greeter(rest),
        "install" | "update" | "uninstall" => Command::Install(first, rest),
        "ipc" => Command::Ipc(rest),
        "theme" => Command::Theme(rest),
        _ => Command::Usage,
    }
}

/// The IPC words `formalshell theme ...` stands for.
pub fn theme_call(rest: &[String]) -> Vec<String> {
    let mut words = vec!["call".to_owned(), "theme".to_owned()];
    words.extend_from_slice(rest);
    words
}

pub const USAGE: &str = "usage: formalshell                         run the shell
       formalshell ipc [--any-display] <call|show|wait|listen|prop> ...
       formalshell theme <function> [args...]   same as: ipc call theme ...
       formalshell greeter | install | update | uninstall
";

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        std::iter::once("formalshell").chain(words.iter().copied()).map(str::to_owned).collect()
    }

    #[test]
    fn bare_start_is_the_shell() {
        assert_eq!(parse(&argv(&[])), Command::Shell);
    }

    #[test]
    fn host_ipc_form() {
        let a = argv(&["ipc", "--any-display", "call", "menu", "toggle"]);
        assert_eq!(parse(&a), Command::Ipc(&a[2..]));
    }

    #[test]
    fn subcommands_keep_their_words() {
        let a = argv(&["install", "--now"]);
        assert_eq!(parse(&a), Command::Install("install", &a[2..]));
        let g = argv(&["greeter"]);
        assert_eq!(parse(&g), Command::Greeter(&g[2..]));
    }

    #[test]
    fn unknown_words_never_start_a_shell() {
        for words in [&["--debug"][..], &["bogus"], &["-h"], &["call", "menu", "toggle"]] {
            assert_eq!(parse(&argv(words)), Command::Usage);
        }
    }
}
