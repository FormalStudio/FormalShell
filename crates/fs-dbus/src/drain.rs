use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::{Context, Poll};

use zbus::Connection;
use zbus::connection::Builder;

/// Awaits `fut` while every input `poll_input` has ready goes to `keep`, so
/// the streams behind it never sit full while `fut` waits on a reply that
/// would arrive behind them.
pub async fn drain_while<T, O>(
    mut poll_input: impl FnMut(&mut Context<'_>) -> Poll<Option<T>>,
    mut keep: impl FnMut(T),
    fut: impl Future<Output = O>,
) -> O {
    let mut fut = pin!(fut);
    poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(out);
        }
        while let Poll::Ready(Some(input)) = poll_input(cx) {
            keep(input);
        }
        Poll::Pending
    })
    .await
}

/// A connection of the caller's own, for one that awaits replies while a
/// stream of its sits undrained. Its messages move only while the returned
/// future is polled, on the caller's executor rather than a zbus thread.
pub async fn private(builder: Builder<'static>) -> zbus::Result<(Connection, impl Future<Output = ()> + use<>)> {
    let conn = builder.internal_executor(false).build().await?;
    let executor = conn.executor().clone();
    Ok((conn, async move {
        loop {
            executor.tick().await;
        }
    }))
}
