use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Where a notification card came from, and so who has to be told when it
/// closes. The printed forms (`12`, `local-3`, `iphone-77-4012`) are what
/// `notifications status`, `activity status` and the logs show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NotificationId {
    /// Issued by the freedesktop server; the sender hears about its close.
    Server(u32),
    /// A toast the shell wrote for itself.
    Local(u64),
    /// Mirrored off the iPhone, by connection session and ANCS uid.
    Phone { session: i64, id: i64 },
}

impl Default for NotificationId {
    fn default() -> Self {
        NotificationId::Server(0)
    }
}

impl fmt::Display for NotificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotificationId::Server(id) => write!(f, "{id}"),
            NotificationId::Local(id) => write!(f, "local-{id}"),
            NotificationId::Phone { session, id } => write!(f, "iphone-{session}-{id}"),
        }
    }
}

impl FromStr for NotificationId {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        if let Some(rest) = s.strip_prefix("local-") {
            return rest.parse().map(NotificationId::Local).map_err(drop);
        }
        if let Some(rest) = s.strip_prefix("iphone-") {
            let (session, id) = rest.split_once('-').ok_or(())?;
            return Ok(NotificationId::Phone { session: session.parse().map_err(drop)?, id: id.parse().map_err(drop)? });
        }
        s.parse().map(NotificationId::Server).map_err(drop)
    }
}

impl Serialize for NotificationId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for NotificationId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(|()| serde::de::Error::custom(format!("not a notification id: {text}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_prints_and_parses_back() {
        for (id, text) in [
            (NotificationId::Server(12), "12"),
            (NotificationId::Local(3), "local-3"),
            (NotificationId::Phone { session: 77, id: 4012 }, "iphone-77-4012"),
        ] {
            assert_eq!(id.to_string(), text);
            assert_eq!(text.parse(), Ok(id));
        }
        assert_eq!("notification".parse::<NotificationId>(), Err(()));
    }
}
