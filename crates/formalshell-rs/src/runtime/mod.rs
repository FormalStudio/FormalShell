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

/// What reaches the UI thread. Nearly every message is a diff, so boxing
/// it to shrink the rare others would cost an allocation per message.
#[allow(clippy::large_enum_variant)]
pub enum Msg {
    Diff(Diff),
    /// logind's word for the lock, which holds no slice.
    Lock(crate::services::session::Event),
    /// The polkit agent's requests, for its dialog.
    Polkit(crate::services::polkit::Event),
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
    /// The service thread's end of `service`, until `start` hands it over.
    queued: Option<async_channel::Receiver<service::Request>>,
    pool: Pool,
}

/// The pool, and a service queue with no thread behind it yet.
impl Default for Runtime {
    fn default() -> Self {
        let (service, queued) = async_channel::unbounded();
        Self { service, queued: Some(queued), pool: Pool::new(POOL_THREADS, POOL_IDLE) }
    }
}

impl Runtime {
    /// Starts the service thread and runs `services` on it first, which
    /// spawns every service's task, then what `service` queued before.
    pub fn start(&mut self, publisher: Publisher, services: impl FnOnce(&Ctx) + Send + 'static) {
        if let Some(rx) = self.queued.take() {
            service::spawn(rx, publisher, self.pool.clone(), services);
        }
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
    use std::sync::mpsc;
    use std::time::Duration;

    use calloop::EventLoop;
    use calloop::channel::{self, Event};
    use calloop::timer::{TimeoutAction, Timer};

    use super::*;
    use crate::services::clock;

    const FRAME: Duration = Duration::from_millis(8);
    /// Frames the UI loop has to draw while both jobs are held.
    const HELD_FRAMES: u32 = 3;
    /// Only there so a UI loop the jobs did block fails instead of hanging.
    const STUCK: Duration = Duration::from_secs(5);

    struct Ui {
        started: mpsc::Receiver<()>,
        running: usize,
        held_frames: u32,
        release: Vec<mpsc::Sender<()>>,
        results: Vec<String>,
    }

    fn result(text: &str) -> Diff {
        Diff::Clock(clock::Diff::Text(text.into()))
    }

    /// Blocks until the UI loop lets it go, then says whether it did.
    fn held(started: &mpsc::Sender<()>, release: &mpsc::Receiver<()>) -> &'static str {
        let _ = started.send(());
        if release.recv_timeout(STUCK).is_ok() { "released" } else { "stuck" }
    }

    /// A blocking job queued from the UI thread and one awaited by a
    /// service task each hold until the UI loop, with a timer standing in
    /// for the frame clock, has drawn frames while both run. Neither can be
    /// released unless the loop keeps turning, so both results reading
    /// "released" is the proof.
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
                ui.running += ui.started.try_iter().count();
                if ui.running == 2 && !ui.release.is_empty() {
                    ui.held_frames += 1;
                    if ui.held_frames == HELD_FRAMES {
                        for release in ui.release.drain(..) {
                            let _ = release.send(());
                        }
                    }
                }
                TimeoutAction::ToDuration(FRAME)
            })
            .unwrap();

        let (started, started_rx) = mpsc::channel();
        let (release_ui, hold_ui) = mpsc::channel();
        let (release_service, hold_service) = mpsc::channel();
        let publisher = Publisher::new(sender);
        let mut runtime = Runtime::default();
        runtime.start(publisher.clone(), |_| {});
        let started_ui = started.clone();
        runtime.pool().submit(move || publisher.publish(result(&format!("ui {}", held(&started_ui, &hold_ui)))));
        runtime.service(move |ctx| {
            let ctx2 = ctx.clone();
            ctx.spawn(async move {
                let text = ctx2.pool().run(move || held(&started, &hold_service)).await.unwrap();
                ctx2.publish(result(&format!("service {text}")));
            });
        });

        let mut ui = Ui {
            started: started_rx,
            running: 0,
            held_frames: 0,
            release: vec![release_ui, release_service],
            results: Vec::new(),
        };
        while ui.results.len() < 2 {
            event_loop.dispatch(Some(FRAME), &mut ui).unwrap();
        }
        ui.results.sort();
        assert_eq!(ui.results, ["service released", "ui released"]);
        assert_eq!(ui.held_frames, HELD_FRAMES);
    }
}
