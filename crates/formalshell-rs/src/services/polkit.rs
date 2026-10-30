//! The polkit agent: fs-auth's agent registered for
//! this session on the system bus, one request at a time, each run as an
//! `AuthFlow` whose events go to the dialog and whose answers come back
//! from it.

use std::sync::OnceLock;

use calloop::channel::Sender;
use fs_auth::polkit::{AGENT_PATH, Agent, AuthFlow, FlowEvent};
use futures_lite::future;
use zeroize::Zeroizing;

use crate::runtime::Ctx;
use crate::wayland::lock::LockMsg;

/// What the dialog shows, as the flow moves.
#[derive(Debug)]
pub enum Event {
    Begin { message: String, identity: String },
    Prompt { prompt: String, echo: bool },
    Failed,
    /// The request is over, however it ended.
    Done,
}

pub enum Cmd {
    Submit(Zeroizing<String>),
    Cancel,
}

static COMMANDS: OnceLock<async_channel::Sender<Cmd>> = OnceLock::new();

pub fn send(cmd: Cmd) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.try_send(cmd);
    }
}

pub fn start(ctx: &Ctx, tx: Sender<LockMsg>) {
    let (cmd_tx, cmds) = async_channel::unbounded();
    if COMMANDS.set(cmd_tx).is_err() {
        return;
    }
    ctx.spawn(run(tx, cmds));
}

async fn run(tx: Sender<LockMsg>, cmds: async_channel::Receiver<Cmd>) {
    let conn = match crate::services::bus::system().await {
        Ok(c) => c,
        Err(err) => return eprintln!("polkit: system bus: {err}"),
    };
    let agent = match Agent::register(&conn, AGENT_PATH).await {
        Ok(a) => a,
        Err(err) => return eprintln!("polkit: registering the agent: {err}"),
    };
    eprintln!("polkit: agent registered");
    let ui = |e: Event| {
        let _ = tx.send(LockMsg::Polkit(e));
    };
    while let Ok(request) = agent.requests.recv().await {
        // Answers typed for an earlier request are not this one's.
        while cmds.try_recv().is_ok() {}
        let mut flow = AuthFlow::start(request).await;
        ui(Event::Begin { message: flow.request().message.clone(), identity: flow.selected().name.clone() });
        loop {
            let step = future::or(async { Ok(flow.next().await) }, async { Err(cmds.recv().await.ok()) }).await;
            match step {
                Ok(Some(FlowEvent::Request { prompt, echo })) => ui(Event::Prompt { prompt, echo }),
                Ok(Some(FlowEvent::Failed)) => ui(Event::Failed),
                Ok(Some(FlowEvent::Info(_) | FlowEvent::Error(_))) => {}
                Ok(Some(FlowEvent::Succeeded | FlowEvent::Cancelled | FlowEvent::Unavailable(_)) | None) => break,
                Err(Some(Cmd::Submit(answer))) => flow.submit(answer).await,
                Err(Some(Cmd::Cancel) | None) => {
                    flow.cancel();
                    break;
                }
            }
        }
        ui(Event::Done);
    }
}
