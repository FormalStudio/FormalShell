//! The polkit agent: fs-auth's agent registered for
//! this session on the system bus, one request at a time, each run as an
//! `AuthFlow` whose events go to the dialog and whose answers come back
//! from it.

use std::sync::OnceLock;

use fs_auth::polkit::{Agent, AuthFlow, FlowEvent};
use futures_lite::future;
use zeroize::Zeroizing;

use crate::runtime::{Ctx, Msg, Publisher};

/// What the dialog shows, as the flow moves.
#[derive(Debug)]
pub enum Event {
    Begin { request: u64, message: String, identity: String },
    Prompt { prompt: String, echo: bool },
    Failed,
    /// The request is over, however it ended.
    Done,
}

/// What the dialog answers, naming the request from its `Begin`: one that
/// reaches the service after its request ended is not the next one's.
pub enum Cmd {
    Submit(u64, Zeroizing<String>),
    Cancel(u64),
}

impl Cmd {
    fn request(&self) -> u64 {
        match self {
            Cmd::Submit(request, _) | Cmd::Cancel(request) => *request,
        }
    }
}

static COMMANDS: OnceLock<async_channel::Sender<Cmd>> = OnceLock::new();

pub fn send(cmd: Cmd) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.try_send(cmd);
    }
}

pub fn start(ctx: &Ctx) {
    let (cmd_tx, cmds) = async_channel::unbounded();
    if COMMANDS.set(cmd_tx).is_err() {
        return;
    }
    ctx.spawn(run(ctx.publisher().clone(), cmds));
}

async fn run(publisher: Publisher, cmds: async_channel::Receiver<Cmd>) {
    let conn = match crate::services::bus::system().await {
        Ok(c) => c,
        Err(err) => return eprintln!("polkit: system bus: {err}"),
    };
    let session = match crate::services::users::session(&conn).await {
        Ok(s) => s,
        Err(err) => return eprintln!("polkit: no logind session: {err}"),
    };
    let agent = match Agent::register(&conn, &session.id).await {
        Ok(a) => a,
        Err(err) => return eprintln!("polkit: registering the agent: {err}"),
    };
    eprintln!("polkit: agent registered for logind session {}", session.id);
    let ui = |e: Event| publisher.send(Msg::Polkit(e));
    let mut next = 0u64;
    while let Ok(request) = agent.requests.recv().await {
        next += 1;
        let id = next;
        let mut flow = AuthFlow::start(request).await;
        ui(Event::Begin { request: id, message: flow.request().message.clone(), identity: flow.selected().name.clone() });
        loop {
            let step = future::or(async { Ok(flow.next().await) }, async { Err(cmds.recv().await.ok()) }).await;
            match step {
                Ok(Some(FlowEvent::Request { prompt, echo })) => ui(Event::Prompt { prompt, echo }),
                Ok(Some(FlowEvent::Failed)) => ui(Event::Failed),
                Ok(Some(FlowEvent::Info(text))) => eprintln!("polkit: info: {text}"),
                Ok(Some(FlowEvent::Error(text))) => eprintln!("polkit: error: {text}"),
                Ok(Some(FlowEvent::Succeeded | FlowEvent::Cancelled | FlowEvent::Unavailable(_)) | None) => break,
                Err(Some(cmd)) if cmd.request() != id => {}
                Err(Some(Cmd::Submit(_, answer))) => flow.submit(answer).await,
                Err(Some(Cmd::Cancel(_)) | None) => {
                    flow.cancel();
                    break;
                }
            }
        }
        ui(Event::Done);
    }
}
