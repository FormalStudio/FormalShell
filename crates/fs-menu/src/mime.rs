//! A file's type by its name, off shared-mime-info's `globs2`, and the
//! types above it off `subclasses`: what Open With weighs an app's
//! `MimeType` against. A name alone is read, never the file.

use std::collections::HashMap;

struct Glob {
    weight: u32,
    /// What the name starts or ends with, lowercase unless `exact`.
    part: String,
    /// `*.ext` rather than `name*`.
    suffix: bool,
    /// The glob's `cs` flag: its case counts.
    exact: bool,
    mime: String,
}

#[derive(Default)]
pub struct Types {
    /// Whole names, lowercase.
    names: HashMap<String, (u32, String)>,
    globs: Vec<Glob>,
    parents: HashMap<String, Vec<String>>,
}

impl Types {
    /// `globs2` is `weight:type:glob[:flags]` a line and `subclasses`
    /// `type parent`; several files of either joined read as one. A glob
    /// with a class or a `?`, or a `*` anywhere but one end, is left out.
    pub fn parse(globs2: &str, subclasses: &str) -> Types {
        let mut types = Types::default();
        for line in globs2.lines().filter(|l| !l.starts_with('#')) {
            let mut fields = line.split(':');
            let (Some(weight), Some(mime), Some(glob)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let Ok(weight) = weight.parse::<u32>() else { continue };
            let exact = fields.next().is_some_and(|f| f.split(',').any(|f| f == "cs"));
            if glob.contains(['[', '?']) {
                continue;
            }
            let cased = |part: &str| if exact { part.to_owned() } else { part.to_lowercase() };
            match (glob.strip_prefix('*'), glob.strip_suffix('*')) {
                (Some(rest), _) if !rest.is_empty() && !rest.contains('*') => {
                    types.globs.push(Glob { weight, part: cased(rest), suffix: true, exact, mime: mime.to_owned() });
                }
                (_, Some(rest)) if !rest.is_empty() && !rest.contains('*') => {
                    types.globs.push(Glob { weight, part: cased(rest), suffix: false, exact, mime: mime.to_owned() });
                }
                (None, None) => {
                    let held = types.names.entry(glob.to_lowercase()).or_insert((weight, mime.to_owned()));
                    if weight > held.0 {
                        *held = (weight, mime.to_owned());
                    }
                }
                _ => {}
            }
        }
        for line in subclasses.lines() {
            if let Some((mime, parent)) = line.split_once(' ') {
                types.parents.entry(mime.to_owned()).or_default().push(parent.trim().to_owned());
            }
        }
        types
    }

    /// The type of a file named `name`: a whole name first, then the glob
    /// of the most weight, the longest of those, one whose case counts
    /// ahead of one whose does not.
    pub fn of(&self, name: &str) -> Option<&str> {
        let lower = name.to_lowercase();
        if let Some((_, mime)) = self.names.get(&lower) {
            return Some(mime);
        }
        self.globs
            .iter()
            .filter(|g| {
                let name = if g.exact { name } else { lower.as_str() };
                name.len() > g.part.len() && if g.suffix { name.ends_with(&g.part) } else { name.starts_with(&g.part) }
            })
            .max_by_key(|g| (g.weight, g.part.len(), g.exact))
            .map(|g| g.mime.as_str())
    }

    /// `mime` and every type above it, nearest first. Any `text/` type
    /// stands under `text/plain`, as the spec has it.
    pub fn chain(&self, mime: &str) -> Vec<String> {
        let mut out = vec![mime.to_owned()];
        let mut at = 0;
        while at < out.len() {
            let mut above = self.parents.get(&out[at]).cloned().unwrap_or_default();
            if out[at].starts_with("text/") && out[at] != "text/plain" {
                above.push("text/plain".to_owned());
            }
            for parent in above {
                if !out.contains(&parent) {
                    out.push(parent);
                }
            }
            at += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLOBS: &str = "# comment\n\
        50:text/x-rust:*.rs\n\
        50:application/gzip:*.gz\n\
        50:application/x-compressed-tar:*.tar.gz\n\
        50:text/x-makefile:Makefile\n\
        50:text/x-readme:README*\n\
        50:text/x-c++src:*.C:cs\n\
        50:text/x-csrc:*.c\n\
        60:image/svg+xml:*.svg\n\
        50:text/x-odd:*.[ch]\n";
    const SUBCLASSES: &str = "image/svg+xml application/xml\napplication/xml text/plain\napplication/x-compressed-tar application/gzip\n";

    #[test]
    fn a_name_reads_as_its_longest_glob_and_a_whole_name_first() {
        let t = Types::parse(GLOBS, SUBCLASSES);
        assert_eq!(t.of("main.rs"), Some("text/x-rust"));
        assert_eq!(t.of("MAIN.RS"), Some("text/x-rust"));
        assert_eq!(t.of("a.tar.gz"), Some("application/x-compressed-tar"));
        assert_eq!(t.of("a.gz"), Some("application/gzip"));
        assert_eq!(t.of("makefile"), Some("text/x-makefile"));
        assert_eq!(t.of("README.md"), Some("text/x-readme"));
        assert_eq!(t.of("x.C"), Some("text/x-c++src"));
        assert_eq!(t.of("x.c"), Some("text/x-csrc"));
        assert_eq!(t.of(".rs"), None);
        assert_eq!(t.of("notes"), None);
    }

    #[test]
    fn a_chain_climbs_the_subclasses_and_text_ends_on_plain() {
        let t = Types::parse(GLOBS, SUBCLASSES);
        assert_eq!(t.chain("image/svg+xml"), ["image/svg+xml", "application/xml", "text/plain"]);
        assert_eq!(t.chain("text/x-rust"), ["text/x-rust", "text/plain"]);
        assert_eq!(t.chain("text/plain"), ["text/plain"]);
        assert_eq!(t.chain("image/png"), ["image/png"]);
    }
}
