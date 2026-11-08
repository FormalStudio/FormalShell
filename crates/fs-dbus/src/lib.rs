//! The D-Bus client pieces every daemon crate shares, on zbus and whatever
//! executor drives the connection.
//!
//! zbus's socket reader stops reading once any `MessageStream` on a
//! connection holds 64 unread messages, and method replies come in through
//! that same reader. So whatever awaits a reply while it holds a stream
//! drains that stream meanwhile ([`drain_while`]); [`watch`] does it for
//! every read it makes. Anything that cannot gets a connection of its own
//! ([`private`]), which keeps the wedge from reaching every other user of a
//! shared one but does not prevent it.

mod backoff;
mod drain;
mod owner;
mod prop;
mod watch;

pub use backoff::Backoff;
pub use drain::{drain_while, private};
pub use owner::{NameOwner, Owner, OwnerWatch};
pub use prop::{Props, plain, prop};
pub use watch::{Watched, watch};
