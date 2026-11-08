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

/// One well-known name's owner changes, off the bus's `NameOwnerChanged`.
pub struct OwnerWatch {
    messages: MessageStream,
}

impl OwnerWatch {
    /// Subscribed once this resolves, so a change right after is not lost.
    pub async fn new(conn: &Connection, name: &str) -> zbus::Result<Self> {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg(0, name.to_owned())?
            .build();
        Ok(Self { messages: MessageStream::for_match_rule(rule, conn, None).await? })
    }
}

impl Stream for OwnerWatch {
    type Item = Owner;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Owner>> {
        loop {
            let message = match Pin::new(&mut self.messages).poll_next(cx) {
                Poll::Ready(Some(Ok(message))) => message,
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            };
            if let Ok((_, _, new)) = message.body().deserialize::<(String, String, String)>() {
                return Poll::Ready(Some(if new.is_empty() { Owner::Gone } else { Owner::Back }));
            }
        }
    }
}
