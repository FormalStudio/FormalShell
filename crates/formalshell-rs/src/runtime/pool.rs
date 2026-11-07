//! Blocking work (desktop entry scans, icon and image decode, file IO) off
//! both loops: at most `max` threads, started on demand, each one exiting
//! once it has sat `idle` with nothing queued.

use std::collections::VecDeque;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

type Job = Box<dyn FnOnce() + Send>;

#[derive(Clone)]
pub struct Pool {
    inner: Arc<Inner>,
}

struct Inner {
    queue: Mutex<Queue>,
    wake: Condvar,
    max: usize,
    idle: Duration,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    threads: usize,
    waiting: usize,
}

impl Pool {
    pub fn new(max: usize, idle: Duration) -> Self {
        let inner = Inner { queue: Mutex::new(Queue::default()), wake: Condvar::new(), max, idle };
        Self { inner: Arc::new(inner) }
    }

    /// Queues `job`; its result reaches whoever needs it through a
    /// channel it carries, usually a [`super::Publisher`].
    pub fn submit(&self, job: impl FnOnce() + Send + 'static) {
        let mut queue = self.inner.queue.lock().unwrap();
        queue.jobs.push_back(Box::new(job));
        if queue.waiting > 0 {
            self.inner.wake.notify_one();
        } else if queue.threads < self.inner.max {
            queue.threads += 1;
            let inner = self.inner.clone();
            std::thread::Builder::new()
                .name("fs-pool".into())
                .spawn(move || worker(inner))
                .expect("spawn a pool thread");
        }
    }

    /// `job`'s result as a future for the service thread to await. `None`
    /// when the job panicked.
    pub fn run<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(
        &self,
        job: F,
    ) -> impl Future<Output = Option<T>> + use<T, F> {
        let (tx, rx) = async_channel::bounded(1);
        self.submit(move || {
            let _ = tx.send_blocking(job());
        });
        async move { rx.recv().await.ok() }
    }

    #[cfg(test)]
    pub fn threads(&self) -> usize {
        self.inner.queue.lock().unwrap().threads
    }
}

fn worker(inner: Arc<Inner>) {
    super::lower_priority();
    let mut queue = inner.queue.lock().unwrap();
    loop {
        if let Some(job) = queue.jobs.pop_front() {
            drop(queue);
            // A panicking job takes only itself down; the thread and its
            // count in `threads` stay right.
            let _ = catch_unwind(AssertUnwindSafe(job));
            queue = inner.queue.lock().unwrap();
            continue;
        }
        queue.waiting += 1;
        let (guard, wait) = inner.wake.wait_timeout(queue, inner.idle).unwrap();
        queue = guard;
        queue.waiting -= 1;
        if wait.timed_out() && queue.jobs.is_empty() {
            queue.threads -= 1;
            return;
        }
    }
}
