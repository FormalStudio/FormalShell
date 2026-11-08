use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_io::Timer;
use futures_lite::{Stream, future, stream};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::{Backoff, Owner, OwnerWatch};

/// Retrying a read that failed with the daemon up, or a subscription that
/// failed.
const RETRY_FLOOR: Duration = Duration::from_secs(1);
const RETRY_CEILING: Duration = Duration::from_secs(30);

/// A daemon read as one state: whole once, then patched from the signals
/// that carry their whole change and read again on any other.
pub trait Watched {
    /// Kept between bursts.
    type State;
    /// What the watch hands out; a burst that leaves it equal hands nothing.
    type View: Clone + PartialEq;
    /// The daemon's well-known name, whose owner the watch follows.
    const SERVICE: &'static str;
    /// How long the daemon stays silent before a burst counts as settled.
    /// Zero takes the signals already queued and no more.
    const QUIET: Duration = Duration::ZERO;

    /// The signals that mean a change.
    fn rule(&self) -> zbus::Result<MatchRule<'static>>;
    fn read(&self) -> impl Future<Output = zbus::Result<Self::State>>;
    /// One signal applied in place; false when the state cannot take it,
    /// and the daemon is read again.
    fn apply(&self, _state: &mut Self::State, _signal: &Message) -> bool {
        false
    }
    fn view(&self, state: &Self::State) -> Self::View;
}

/// The daemon's view, then a new one after every settled burst of signals
/// that changed it: None while the daemon has no owner on the bus, and a
/// whole read again whenever a new one takes the name. The rule and the
/// owner watch are installed before the first read, so nothing slips
/// between the two. A read that fails with the daemon up, and a
/// subscription that fails, are tried again on a backoff. Ends with the
/// connection.
///
/// Poll it for as long as it lives: between polls its signals queue on the
/// connection, and a full queue stops the connection reading.
pub fn watch<W: Watched>(conn: Connection, watched: W) -> impl Stream<Item = Option<W::View>> {
    let watcher = Watcher {
        conn,
        watched,
        inputs: None,
        backlog: VecDeque::new(),
        state: None,
        stale: true,
        shown: None,
        retry: Backoff::new(RETRY_FLOOR, RETRY_CEILING),
    };
    stream::unfold(watcher, |mut w| async move {
        let view = w.next().await?;
        Some((view, w))
    })
}

enum Input {
    Signal(Message),
    Unreadable,
    Owner(Owner),
}

struct Inputs {
    owner: OwnerWatch,
    signals: MessageStream,
}

impl Inputs {
    async fn subscribe<W: Watched>(conn: &Connection, watched: &W) -> zbus::Result<Self> {
        Ok(Self {
            owner: OwnerWatch::new(conn, W::SERVICE).await?,
            signals: MessageStream::for_match_rule(watched.rule()?, conn, None).await?,
        })
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<Input>> {
        match Pin::new(&mut self.owner).poll_next(cx) {
            Poll::Ready(Some(owner)) => return Poll::Ready(Some(Input::Owner(owner.owner))),
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => {}
        }
        Pin::new(&mut self.signals)
            .poll_next(cx)
            .map(|next| next.map(|signal| signal.map_or(Input::Unreadable, Input::Signal)))
    }

    async fn drain_while<O>(&mut self, backlog: &mut VecDeque<Input>, fut: impl Future<Output = O>) -> O {
        crate::drain_while(|cx| self.poll(cx), |input| backlog.push_back(input), fut).await
    }
}

struct Watcher<W: Watched> {
    conn: Connection,
    watched: W,
    inputs: Option<Inputs>,
    /// Inputs that arrived while a read or a retry was waiting.
    backlog: VecDeque<Input>,
    /// None while the daemon has no owner.
    state: Option<W::State>,
    /// The daemon has to be read again.
    stale: bool,
    /// What was last handed out, None before the first.
    shown: Option<Option<W::View>>,
    retry: Backoff,
}

impl<W: Watched> Watcher<W> {
    async fn next(&mut self) -> Option<Option<W::View>> {
        loop {
            if self.inputs.is_none() {
                match Inputs::subscribe(&self.conn, &self.watched).await {
                    Ok(inputs) => self.inputs = Some(inputs),
                    Err(zbus::Error::InputOutput(_)) => return None,
                    Err(_) => {
                        self.retry.wait().await;
                        continue;
                    }
                }
            }
            if self.stale {
                self.read().await;
                if self.stale {
                    continue;
                }
            } else if !self.burst().await {
                return None;
            }
            if !self.stale
                && let Some(view) = self.offer()
            {
                return Some(view);
            }
        }
    }

    /// The daemon read whole, the inputs drained meanwhile. A failure with
    /// the daemon up waits out the backoff and leaves the state stale; with
    /// no owner the state goes until one appears.
    async fn read(&mut self) {
        let Self { conn, watched, inputs, backlog, state, stale, retry, .. } = self;
        let Some(subscribed) = inputs.as_mut() else { return };
        match subscribed.drain_while(backlog, watched.read()).await {
            Ok(read) => {
                *state = Some(read);
                *stale = false;
                retry.reset();
            }
            // The connection is gone; subscribing again says so.
            Err(zbus::Error::InputOutput(_)) => *inputs = None,
            Err(_) if subscribed.drain_while(backlog, owned(conn, W::SERVICE)).await => {
                subscribed.drain_while(backlog, retry.wait()).await;
            }
            Err(_) => {
                *state = None;
                *stale = false;
            }
        }
    }

    /// Takes the next input and every one behind it until the daemon has
    /// been quiet for `QUIET`. False once the inputs end.
    async fn burst(&mut self) -> bool {
        let Some(first) = self.input().await else { return false };
        self.take(first);
        loop {
            let next = if W::QUIET.is_zero() {
                future::poll_once(self.input()).await
            } else {
                future::or(async { Some(self.input().await) }, async {
                    Timer::after(W::QUIET).await;
                    None
                })
                .await
            };
            match next {
                Some(Some(input)) => self.take(input),
                Some(None) => return false,
                None => return true,
            }
        }
    }

    async fn input(&mut self) -> Option<Input> {
        if let Some(input) = self.backlog.pop_front() {
            return Some(input);
        }
        let inputs = self.inputs.as_mut()?;
        poll_fn(|cx| inputs.poll(cx)).await
    }

    fn take(&mut self, input: Input) {
        match input {
            Input::Owner(Owner::Gone) => {
                self.state = None;
                self.stale = false;
            }
            Input::Owner(Owner::Back) => self.stale = true,
            Input::Unreadable => self.stale |= self.state.is_some(),
            Input::Signal(signal) => {
                if let Some(state) = &mut self.state {
                    self.stale |= !self.watched.apply(state, &signal);
                }
            }
        }
    }

    fn offer(&mut self) -> Option<Option<W::View>> {
        let view = self.state.as_ref().map(|s| self.watched.view(s));
        if self.shown.as_ref() == Some(&view) {
            return None;
        }
        self.shown = Some(view.clone());
        Some(view)
    }
}

/// Whether anything owns `name`; true when the bus does not answer, so the
/// read is tried again.
async fn owned(conn: &Connection, name: &'static str) -> bool {
    let Ok(dbus) = DBusProxy::builder(conn).cache_properties(CacheProperties::No).build().await else { return true };
    let Ok(name) = BusName::try_from(name) else { return true };
    dbus.name_has_owner(name).await.unwrap_or(true)
}
