//! The service thread: one `LocalExecutor` driven by async-io's reactor,
//! the same reactor zbus 5 runs on by default.

use std::future::Future;
use std::rc::Rc;

use async_executor::LocalExecutor;

use super::{Pool, Publisher};
use crate::store::Diff;

pub type Request = Box<dyn FnOnce(&Ctx) + Send>;

/// What a service task holds: a way to spawn more tasks, publish diffs and
/// reach the pool.
#[derive(Clone)]
pub struct Ctx {
    executor: Rc<LocalExecutor<'static>>,
    publisher: Publisher,
    pool: Pool,
}

impl Ctx {
    pub fn spawn(&self, task: impl Future<Output = ()> + 'static) {
        self.executor.spawn(task).detach();
    }

    pub fn publish(&self, diff: Diff) {
        self.publisher.publish(diff);
    }

    pub fn publisher(&self) -> &Publisher {
        &self.publisher
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }
}

/// The thread ends when the UI's [`super::Runtime`] is dropped. What was
/// sent on `rx` before it started runs after `services`.
pub fn spawn(
    rx: async_channel::Receiver<Request>,
    publisher: Publisher,
    pool: Pool,
    services: impl FnOnce(&Ctx) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("fs-service".into())
        .spawn(move || {
            super::lower_priority();
            let ctx = Ctx { executor: Rc::new(LocalExecutor::new()), publisher, pool };
            services(&ctx);
            let executor = ctx.executor.clone();
            async_io::block_on(executor.run(async move {
                while let Ok(request) = rx.recv().await {
                    request(&ctx);
                }
            }));
        })
        .expect("spawn the service thread");
}
