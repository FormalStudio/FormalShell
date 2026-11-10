//! The fspec command lines the shell runs (FormalSpec `docs/shell-install.md`).

/// What `fspec app` does to one app, and the word fspec spells it with on
/// the command line, in its operation kinds and in its commit subjects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppVerb {
    Install,
    Uninstall,
}

impl AppVerb {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "install" => Some(AppVerb::Install),
            "uninstall" => Some(AppVerb::Uninstall),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AppVerb::Install => "install",
            AppVerb::Uninstall => "uninstall",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// A package started once without installing it.
    Run(String),
    /// The spec's inputs updated, run in the spec's directory.
    Update,
    /// An app installed or uninstalled by its search id.
    App(AppVerb, String),
    /// The newest app change taken back.
    Undo,
    /// Which spec, if any, owns a desktop file.
    Owner(String),
    /// One package's details.
    Info(String),
    Search { query: String, limit: u32 },
    Facts,
    /// The checkout's HEAD applied to this machine.
    ApplyHead,
}

impl Command {
    pub fn argv(&self) -> Vec<String> {
        let limit;
        let words: Vec<&str> = match self {
            Command::Run(package) => vec!["run", "--detach", package],
            Command::Update => vec!["update"],
            Command::App(verb, id) => vec!["app", verb.as_str(), id, "--detach"],
            Command::Undo => vec!["app", "undo", "--detach"],
            Command::Owner(desktop) => vec!["app", "owner", desktop, "--json"],
            Command::Info(id) => vec!["info", id, "--json"],
            Command::Search { query, limit: n } => {
                limit = n.to_string();
                vec!["search", "--json", "--offline", "--limit", &limit, query]
            }
            Command::Facts => vec!["facts", "--json"],
            Command::ApplyHead => vec!["apply", "--rev", "HEAD"],
        };
        std::iter::once("fspec").chain(words).map(String::from).collect()
    }
}
