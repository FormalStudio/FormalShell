//! The thread model. The UI thread owns Wayland, the scene and the
//! [`Store`](crate::store::Store), and only ever drains [`Msg`]s off a
//! calloop channel. One service thread runs every service on a
//! single-threaded executor, and blocking work goes to the [`Pool`]. Nothing
//! on the UI thread awaits.

pub mod pool;
mod service;

use std::time::Duration;

pub use pool::Pool;
pub use service::Ctx;

use crate::store::Diff;

/// At most two blocking threads, each gone after this long with no work.
const POOL_THREADS: usize = 2;
const POOL_IDLE: Duration = Duration::from_secs(5);

/// Every thread but the UI's runs at nice 10, and the threads it starts
/// (zbus, PipeWire, `blocking`) inherit it: on a starved CPU the UI loop's
/// frame goes first and the rest takes the slack.
pub fn lower_priority() {
    // Linux applies PRIO_PROCESS with who 0 to the calling thread alone.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

/// Wall-clock milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// What reaches the UI thread.
pub enum Msg {
    Diff(Diff),
}

/// The sending half of the UI thread's channel. Cheap to clone and `Send`,
/// so services, pool jobs and the IPC server each hold one.
#[derive(Clone)]
pub struct Publisher(calloop::channel::Sender<Msg>);

impl Publisher {
    pub fn new(sender: calloop::channel::Sender<Msg>) -> Self {
        Self(sender)
    }

    /// A send after the UI thread is gone is dropped: the process is
    /// exiting.
    pub fn send(&self, msg: Msg) {
        let _ = self.0.send(msg);
    }

    pub fn publish(&self, diff: Diff) {
        self.send(Msg::Diff(diff));
    }
}

/// The UI thread's handle on the other two.
pub struct Runtime {
    service: async_channel::Sender<service::Request>,
    pool: Pool,
}

impl Runtime {
    /// Starts the service thread and runs `services` on it first, which
    /// spawns every service's task.
    pub fn start(publisher: Publisher, services: impl FnOnce(&Ctx) + Send + 'static) -> Self {
        let pool = Pool::new(POOL_THREADS, POOL_IDLE);
        let service = service::spawn(publisher, pool.clone(), services);
        Self { service, pool }
    }

    /// Runs `f` on the service thread, where it can spawn tasks.
    pub fn service(&self, f: impl FnOnce(&Ctx) + Send + 'static) {
        let _ = self.service.try_send(Box::new(f));
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use calloop::EventLoop;
    use calloop::channel::{self, Event};
    use calloop::timer::{TimeoutAction, Timer};

    use super::*;
    use crate::services::clock;

    const FRAME: Duration = Duration::from_millis(8);
    const BLOCK: Duration = Duration::from_millis(600);

    #[derive(Default)]
    struct Ui {
        last: Option<Instant>,
        worst: Duration,
        frames: u32,
        results: Vec<String>,
    }

    fn result(text: &str) -> Diff {
        Diff::Clock(clock::Diff::Text(text.into()))
    }

    /// A blocking job queued from the UI thread and one awaited by a
    /// service task, both far longer than a frame, while a timer stands in
    /// for the frame clock: no gap between frames comes near the job's
    /// length, and both results land as diffs.
    #[test]
    fn blocking_job_never_delays_a_frame() {
        let mut event_loop: EventLoop<Ui> = EventLoop::try_new().unwrap();
        let handle = event_loop.handle();
        let (sender, inbox) = channel::channel::<Msg>();
        handle
            .insert_source(inbox, |event, _, ui: &mut Ui| {
                if let Event::Msg(Msg::Diff(Diff::Clock(clock::Diff::Text(text)))) = event {
                    ui.results.push(text);
                }
            })
            .unwrap();
        handle
            .insert_source(Timer::from_duration(FRAME), |_, _, ui: &mut Ui| {
                let now = Instant::now();
                if let Some(last) = ui.last {
                    ui.worst = ui.worst.max(now - last);
                }
                ui.last = Some(now);
                ui.frames += 1;
                TimeoutAction::ToDuration(FRAME)
            })
            .unwrap();

        let publisher = Publisher::new(sender);
        let runtime = Runtime::start(publisher.clone(), |_| {});
        let from_ui = publisher.clone();
        runtime.pool().submit(move || {
            std::thread::sleep(BLOCK);
            from_ui.publish(result("ui"));
        });
        runtime.service(|ctx| {
            let ctx2 = ctx.clone();
            ctx.spawn(async move {
                let text = ctx2.pool().run(|| {
                    std::thread::sleep(BLOCK);
                    "service".to_string()
                });
                let text = text.await.unwrap();
                ctx2.publish(result(&text));
            });
        });

        let mut ui = Ui::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while ui.results.len() < 2 && Instant::now() < deadline {
            event_loop.dispatch(Some(FRAME), &mut ui).unwrap();
        }
        ui.results.sort();
        assert_eq!(ui.results, ["service", "ui"]);
        assert!(ui.frames >= 30, "only {} frames in {BLOCK:?}", ui.frames);
        assert!(ui.worst < Duration::from_millis(100), "a frame waited {:?}", ui.worst);
    }

    #[test]
    fn pool_holds_two_threads_and_lets_them_go() {
        let pool = Pool::new(2, Duration::from_millis(50));
        let (tx, rx) = std::sync::mpsc::channel();
        for _ in 0..4 {
            let tx = tx.clone();
            pool.submit(move || {
                std::thread::sleep(Duration::from_millis(100));
                tx.send(()).unwrap();
            });
        }
        assert_eq!(pool.threads(), 2);
        for _ in 0..4 {
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(pool.threads(), 0);
    }
}
