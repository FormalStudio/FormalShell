//! One single-line text field's editing, shared by every input the shell
//! draws: the text, the caret and the selection's other end, both byte
//! offsets on char boundaries.
//!
//! Generic over the string's holder so a password field keeps its
//! `Zeroizing<String>`: every edit works in place on that one buffer.

use zeroize::Zeroizing;

use crate::nav::Modifiers;

/// What a field keeps its text in.
pub trait Buffer {
    fn buf(&self) -> &String;
    fn buf_mut(&mut self) -> &mut String;
}

impl Buffer for String {
    fn buf(&self) -> &String {
        self
    }
    fn buf_mut(&mut self) -> &mut String {
        self
    }
}

impl Buffer for Zeroizing<String> {
    fn buf(&self) -> &String {
        self
    }
    fn buf_mut(&mut self) -> &mut String {
        self
    }
}

/// The keys a field reads, apart from the text a key types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
    /// A letter key, lowercased, for the Ctrl shortcuts: the text a key
    /// types under Ctrl is a control character, not the letter.
    Letter(char),
    Other,
}

/// What one key did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not the field's: the caller's own binding, if any, runs.
    Pass,
    /// The field took it; the caret or selection may have moved.
    Held,
    /// The text changed.
    Edited,
    /// Put this on the clipboard.
    Copy(String),
    /// The text changed, and this goes on the clipboard.
    Cut(String),
    /// Read the clipboard and hand it to [`Field::paste`].
    Paste,
}

#[derive(Clone, Debug, Default)]
pub struct Field<S = String> {
    text: S,
    caret: usize,
    anchor: Option<usize>,
    /// Shown masked: nothing leaves it through copy or cut.
    pub secret: bool,
    /// A secret drawn in the clear (a saved Wi-Fi password asked for), which
    /// still never leaves through copy or cut.
    pub shown: bool,
}

impl<S: Buffer> Field<S> {
    /// The caret at the end of `text`.
    pub fn new(text: S) -> Self {
        let caret = text.buf().len();
        Self { text, caret, anchor: None, secret: false, shown: false }
    }

    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    pub fn text(&self) -> &str {
        self.text.buf()
    }

    pub fn is_empty(&self) -> bool {
        self.text.buf().is_empty()
    }

    /// The whole text replaced, the caret at its end.
    pub fn set(&mut self, text: &str) {
        self.text.buf_mut().clear();
        self.text.buf_mut().push_str(text);
        self.caret = self.text.buf().len();
        self.anchor = None;
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    /// The holder swapped for `fresh`, which starts empty.
    pub fn replace(&mut self, fresh: S) -> S {
        let old = std::mem::replace(&mut self.text, fresh);
        self.caret = self.text.buf().len();
        self.anchor = None;
        old
    }

    /// Byte offset.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected byte range, start before end, when it covers anything.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret).then(|| (a.min(self.caret), a.max(self.caret)))
    }

    /// The caret and the selection's other end (the caret again with none)
    /// as char counts, which is what a masked field's one dot per char
    /// needs to place them.
    pub fn caret_chars(&self) -> (usize, usize) {
        let at = |b: usize| self.text.buf()[..b].chars().count();
        (at(self.caret), at(self.anchor.unwrap_or(self.caret)))
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.buf().len();
    }

    /// `s` typed at the caret over any selection, control characters
    /// dropped.
    pub fn insert(&mut self, s: &str) -> bool {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return false;
        }
        self.delete_selection();
        self.text.buf_mut().insert_str(self.caret, &clean);
        self.caret += clean.len();
        true
    }

    /// The clipboard's text into a one-line field: line breaks and tabs
    /// become spaces, a trailing newline goes.
    pub fn paste(&mut self, s: &str) -> bool {
        let s = s.trim_end_matches(['\n', '\r']);
        let flat: String = s.replace("\r\n", " ").chars().map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c }).collect();
        self.insert(&flat)
    }

    /// One key. `text` is what it types, if anything.
    pub fn key(&mut self, key: Key, mods: Modifiers, text: Option<&str>) -> Outcome {
        if mods.alt {
            return Outcome::Pass;
        }
        let shift = mods.shift;
        match key {
            Key::Left | Key::Right => {
                let back = key == Key::Left;
                let to = match self.selection() {
                    Some((a, b)) if !shift && !mods.ctrl => if back { a } else { b },
                    _ if mods.ctrl => if back { self.word_start(self.caret) } else { self.word_end(self.caret) },
                    _ if back => self.prev(self.caret),
                    _ => self.next(self.caret),
                };
                self.go(to, shift);
                Outcome::Held
            }
            Key::Home => {
                self.go(0, shift);
                Outcome::Held
            }
            Key::End => {
                self.go(self.text.buf().len(), shift);
                Outcome::Held
            }
            Key::Backspace | Key::Delete => {
                if self.delete_selection() {
                    return Outcome::Edited;
                }
                let (from, to) = match (key, mods.ctrl) {
                    (Key::Backspace, true) => (self.word_start(self.caret), self.caret),
                    (Key::Backspace, false) => (self.prev(self.caret), self.caret),
                    (_, true) => (self.caret, self.word_end(self.caret)),
                    (_, false) => (self.caret, self.next(self.caret)),
                };
                if from == to {
                    return Outcome::Held;
                }
                self.text.buf_mut().replace_range(from..to, "");
                self.caret = from;
                Outcome::Edited
            }
            Key::Letter(c) if mods.ctrl => match c {
                'a' => {
                    self.select_all();
                    Outcome::Held
                }
                'c' => match self.selection().filter(|_| !self.secret) {
                    Some((a, b)) => Outcome::Copy(self.text.buf()[a..b].to_owned()),
                    None => Outcome::Held,
                },
                'x' => match self.selection().filter(|_| !self.secret) {
                    Some((a, b)) => {
                        let cut = self.text.buf()[a..b].to_owned();
                        self.delete_selection();
                        Outcome::Cut(cut)
                    }
                    None => Outcome::Held,
                },
                'v' => Outcome::Paste,
                'u' => {
                    if self.text.buf().is_empty() {
                        return Outcome::Held;
                    }
                    self.clear();
                    Outcome::Edited
                }
                _ => Outcome::Pass,
            },
            _ if mods.ctrl => Outcome::Pass,
            _ => match text {
                Some(t) if self.insert(t) => Outcome::Edited,
                _ => Outcome::Pass,
            },
        }
    }

    fn go(&mut self, to: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = to;
    }

    fn delete_selection(&mut self) -> bool {
        let sel = self.selection();
        self.anchor = None;
        let Some((a, b)) = sel else { return false };
        self.text.buf_mut().replace_range(a..b, "");
        self.caret = a;
        true
    }

    fn prev(&self, at: usize) -> usize {
        self.text.buf()[..at].chars().next_back().map_or(at, |c| at - c.len_utf8())
    }

    fn next(&self, at: usize) -> usize {
        self.text.buf()[at..].chars().next().map_or(at, |c| at + c.len_utf8())
    }

    /// Back over any whitespace, then over the word before it.
    fn word_start(&self, at: usize) -> usize {
        let head = self.text.buf()[..at].trim_end();
        head.rfind(char::is_whitespace).map_or(0, |i| i + head[i..].chars().next().map_or(1, char::len_utf8))
    }

    /// On over any whitespace, then over the word after it.
    fn word_end(&self, at: usize) -> usize {
        let tail = &self.text.buf()[at..];
        let skip = tail.len() - tail.trim_start().len();
        let rest = &tail[skip..];
        at + skip + rest.find(char::is_whitespace).unwrap_or(rest.len())
    }
}
