//! What a press on an element names. The words are what `menu status` and
//! `sidebar status` report and what most surfaces match on; a surface that
//! addresses the items of a list gives its own [`HitId`] value instead,
//! and reads that value back off the event rather than parsing the words.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

/// A surface's own press value. Its `Display` is the words the status
/// replies report for it.
pub trait HitId: fmt::Display + 'static {}

#[derive(Clone)]
pub struct On {
    words: String,
    hit: Option<Rc<dyn Any>>,
}

impl On {
    pub fn as_str(&self) -> &str {
        &self.words
    }

    /// The surface's press value, when the element was given one of type `H`.
    pub fn get<H: HitId>(&self) -> Option<&H> {
        self.hit.as_deref()?.downcast_ref()
    }
}

impl<H: HitId> From<H> for On {
    fn from(hit: H) -> Self {
        Self { words: hit.to_string(), hit: Some(Rc::new(hit)) }
    }
}

impl From<&str> for On {
    fn from(words: &str) -> Self {
        Self { words: words.to_owned(), hit: None }
    }
}

impl From<String> for On {
    fn from(words: String) -> Self {
        Self { words, hit: None }
    }
}

impl From<&String> for On {
    fn from(words: &String) -> Self {
        Self { words: words.clone(), hit: None }
    }
}

impl From<On> for String {
    fn from(on: On) -> Self {
        on.words
    }
}

impl std::ops::Deref for On {
    type Target = str;

    fn deref(&self) -> &str {
        &self.words
    }
}

/// Two hits with the same words are the same press: a typed hit's words
/// are its whole identity.
impl PartialEq for On {
    fn eq(&self, other: &Self) -> bool {
        self.words == other.words
    }
}

impl PartialEq<str> for On {
    fn eq(&self, other: &str) -> bool {
        self.words == other
    }
}

impl PartialEq<String> for On {
    fn eq(&self, other: &String) -> bool {
        self.words == *other
    }
}

impl PartialEq<&str> for On {
    fn eq(&self, other: &&str) -> bool {
        self.words == *other
    }
}

impl fmt::Debug for On {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.words, f)
    }
}

impl fmt::Display for On {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.words)
    }
}

impl serde::Serialize for On {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.words)
    }
}
