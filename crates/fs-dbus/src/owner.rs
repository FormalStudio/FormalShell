use std::pin::Pin;
use std::task::{Context, Poll};

use futures_lite::Stream;
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream};

/// What became of a well-known name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Nothing owns it.
    Gone,
    /// A new owner took it, with or without one before: whatever was read
    /// off the old one is stale.
    Back,
}

/// A name that changed hands, and what became of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameOwner {
    pub name: String,
    pub owner: Owner,
}

/// Owner changes off the bus's `NameOwnerChanged`: one well-known name's, or
/// every name's.
pub struct OwnerWatch {
    messages: MessageStream,
}

impl OwnerWatch {
    /// Subscribed once this resolves, so a change right after is not lost.
    pub async fn new(conn: &Connection, name: &str) -> zbus::Result<Self> {
        Self::subscribe(conn, Some(name)).await
    }

    /// Every name on the bus, unique ones included.
    pub async fn any(conn: &Connection) -> zbus::Result<Self> {
        Self::subscribe(conn, None).await
    }

    async fn subscribe(conn: &Connection, name: Option<&str>) -> zbus::Result<Self> {
        let mut rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?;
        if let Some(name) = name {
            rule = rule.arg(0, name.to_owned())?;
        }
        Ok(Self { messages: MessageStream::for_match_rule(rule.build(), conn, None).await? })
    }
}

impl Stream for OwnerWatch {
    type Item = NameOwner;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<NameOwner>> {
        loop {
            let message = match Pin::new(&mut self.messages).poll_next(cx) {
                Poll::Ready(Some(Ok(message))) => message,
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            };
            if let Ok((name, _, new)) = message.body().deserialize::<(String, String, String)>() {
                let owner = if new.is_empty() { Owner::Gone } else { Owner::Back };
                return Poll::Ready(Some(NameOwner { name, owner }));
            }
        }
    }
}
