//! `polkit_probe <pkexec> <command>...`: registers the agent for this session,
//! then runs `<pkexec> <command>` twice. The first request is cancelled from the
//! agent side; the second is answered with the wrong password and then the
//! right one, read from stdin (wrong on the first line, right on the second).
//! Prints `cancel rc=<n>` and `auth rc=<n>` with pkexec's exit codes.

use std::io::BufRead;
use std::process::Command;

use fs_auth::polkit::{Agent, AuthFlow, AuthRequest, FlowEvent};
use zeroize::Zeroizing;

fn main() {
    let command: Vec<String> = std::env::args().skip(1).collect();
    if command.len() < 2 {
        eprintln!("usage: polkit_probe <pkexec> <command>...");
        std::process::exit(2);
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let wrong = Zeroizing::new(lines.next().expect("wrong password").expect("read stdin"));
    let right = Zeroizing::new(lines.next().expect("right password").expect("read stdin"));

    let ok = futures_lite::future::block_on(async {
        let connection = zbus::Connection::system().await.expect("system bus");
        let agent = Agent::register(&connection).await.expect("register agent");
        println!("registered");

        let rc = pkexec(&command);
        let mut flow = AuthFlow::start(request(&agent, &rc).await).await;
        println!("request action={} identities={:?}", flow.request().action_id, flow.request().identities);
        loop {
            match flow.next().await.expect("event") {
                FlowEvent::Request { prompt, echo } => {
                    println!("cancel: prompt echo={echo} {prompt:?}");
                    break;
                }
                event => println!("cancel: {event:?}"),
            }
        }
        flow.cancel();
        let cancel_rc = rc.recv().await.expect("pkexec exit");
        println!("cancel rc={cancel_rc}");

        let rc = pkexec(&command);
        let mut flow = AuthFlow::start(request(&agent, &rc).await).await;
        let mut answers = vec![right, wrong];
        let mut failed = 0;
        loop {
            let event = flow.next().await.expect("event");
            println!("auth: {event:?}");
            match event {
                FlowEvent::Request { .. } => {
                    let Some(answer) = answers.pop() else { break };
                    flow.submit(answer).await;
                }
                FlowEvent::Failed => failed += 1,
                FlowEvent::Succeeded | FlowEvent::Cancelled | FlowEvent::Unavailable(_) => break,
                FlowEvent::Info(_) | FlowEvent::Error(_) => {}
            }
        }
        drop(flow);
        let auth_rc = rc.recv().await.expect("pkexec exit");
        println!("auth rc={auth_rc} failed={failed}");

        agent.unregister().await.expect("unregister");
        cancel_rc == 126 && auth_rc == 0 && failed == 1
    });
    std::process::exit(if ok { 0 } else { 1 });
}

async fn request(agent: &Agent, rc: &async_channel::Receiver<i32>) -> AuthRequest {
    let exited = async {
        let code = rc.recv().await.expect("pkexec exit");
        eprintln!("pkexec exited {code} before asking the agent");
        std::process::exit(1);
    };
    futures_lite::future::or(async { agent.requests.recv().await.expect("request") }, exited).await
}

fn pkexec(command: &[String]) -> async_channel::Receiver<i32> {
    let (tx, rx) = async_channel::bounded(1);
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("spawn pkexec");
    std::thread::spawn(move || {
        let status = child.wait().expect("wait pkexec");
        let _ = tx.send_blocking(status.code().unwrap_or(-1));
    });
    rx
}
