//! A polkit authentication agent: `org.freedesktop.PolicyKit1.AuthenticationAgent`
//! served on the system bus and registered for the caller's logind session.
//! Each `BeginAuthentication` reaches the caller as an `AuthRequest`; an
//! `AuthFlow` then runs the conversation through polkit's agent helper.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use async_channel::{Receiver, Sender};
use async_io::Async;
use futures_lite::future;
use futures_lite::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, interface};
use zeroize::Zeroizing;

const AGENT_PATH: &str = "/org/formalshell/PolkitAgent";

const AUTHORITY_NAME: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY_IFACE: &str = "org.freedesktop.PolicyKit1.Authority";
const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";
// Where distributions install the setuid helper polkit before 127 spawns.
const HELPER_PATHS: &[&str] = &[
    "/run/wrappers/bin/polkit-agent-helper-1",
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
    "/usr/libexec/polkit-1/polkit-agent-helper-1",
    "/usr/lib/policykit-1/polkit-agent-helper-1",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub uid: u32,
    pub name: String,
}

/// One `BeginAuthentication` call, held open until its `AuthFlow` ends or is
/// dropped. Dropping it unanswered dismisses the request.
#[derive(Debug)]
pub struct AuthRequest {
    pub action_id: String,
    pub message: String,
    pub icon_name: String,
    pub details: HashMap<String, String>,
    pub cookie: String,
    pub identities: Vec<Identity>,
    reply: Sender<bool>,
    cancelled: Receiver<()>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowEvent {
    /// The helper wants a response; `echo` is false for a password.
    Request { prompt: String, echo: bool },
    Info(String),
    Error(String),
    /// A wrong answer. A fresh conversation has already started for a retry.
    Failed,
    Succeeded,
    /// polkitd withdrew the request.
    Cancelled,
    /// No helper could be reached for the selected identity.
    Unavailable(String),
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    Cancelled(String),
}

struct AgentIface {
    requests: Sender<AuthRequest>,
    pending: Arc<Mutex<HashMap<String, Sender<()>>>>,
}

#[interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl AgentIface {
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> Result<(), AgentError> {
        let identities = user_identities(&identities);
        if identities.is_empty() {
            return Err(AgentError::Failed("No user identity to authenticate as.".into()));
        }
        let (reply, replied) = async_channel::bounded(1);
        let (cancel, cancelled) = async_channel::bounded(1);
        self.pending.lock().unwrap().insert(cookie.clone(), cancel);
        let request = AuthRequest {
            action_id,
            message,
            icon_name,
            details,
            cookie: cookie.clone(),
            identities,
            reply,
            cancelled,
        };
        let result = match self.requests.send(request).await {
            Ok(()) => replied.recv().await.unwrap_or(false),
            Err(_) => false,
        };
        self.pending.lock().unwrap().remove(&cookie);
        if result {
            Ok(())
        } else {
            Err(AgentError::Cancelled("Authentication request dismissed.".into()))
        }
    }

    async fn cancel_authentication(&self, cookie: String) {
        if let Some(cancel) = self.pending.lock().unwrap().remove(&cookie) {
            let _ = cancel.try_send(());
        }
    }
}

/// The registered agent. Requests arrive on `requests`.
pub struct Agent {
    connection: Connection,
    subject: Subject,
    path: String,
    pub requests: Receiver<AuthRequest>,
}

type Subject = (String, HashMap<String, Value<'static>>);

impl Agent {
    /// Serves the agent at [`AGENT_PATH`] on `connection` (the system bus) and
    /// registers it for the logind session `session_id`
    /// ([`crate::user::session`]).
    pub async fn register(connection: &Connection, session_id: &str) -> zbus::Result<Self> {
        let path = AGENT_PATH;
        let (tx, requests) = async_channel::unbounded();
        let iface = AgentIface { requests: tx, pending: Arc::default() };
        connection.object_server().at(path, iface).await?;
        let subject: Subject = (
            "unix-session".into(),
            HashMap::from([("session-id".to_owned(), Value::from(session_id.to_owned()))]),
        );
        let locale = std::env::var("LANG").unwrap_or_else(|_| "C".into());
        let registered = connection
            .call_method(
                Some(AUTHORITY_NAME),
                AUTHORITY_PATH,
                Some(AUTHORITY_IFACE),
                "RegisterAuthenticationAgent",
                &(&subject, locale.as_str(), path),
            )
            .await;
        if let Err(err) = registered {
            let _ = connection.object_server().remove::<AgentIface, _>(path).await;
            return Err(err);
        }
        Ok(Self { connection: connection.clone(), subject, path: path.to_owned(), requests })
    }

    pub async fn unregister(self) -> zbus::Result<()> {
        self.connection
            .call_method(
                Some(AUTHORITY_NAME),
                AUTHORITY_PATH,
                Some(AUTHORITY_IFACE),
                "UnregisterAuthenticationAgent",
                &(&self.subject, self.path.as_str()),
            )
            .await?;
        self.connection.object_server().remove::<AgentIface, _>(self.path.as_str()).await?;
        Ok(())
    }
}

fn user_identities(identities: &[(String, HashMap<String, OwnedValue>)]) -> Vec<Identity> {
    let mut out: Vec<Identity> = Vec::new();
    for (kind, props) in identities {
        if kind != "unix-user" {
            continue;
        }
        let Some(uid) = props.get("uid").and_then(|v| v.downcast_ref::<u32>().ok()) else {
            continue;
        };
        if out.iter().any(|i| i.uid == uid) {
            continue;
        }
        if let Some(account) = crate::user::account(uid) {
            out.push(Identity { uid, name: account.login });
        }
    }
    out
}

/// The conversation for one `AuthRequest`: a wrong answer restarts the helper for a retry, a right
/// one completes the request, and dropping the flow dismisses it.
pub struct AuthFlow {
    request: AuthRequest,
    session: Option<HelperSession>,
    finished: bool,
}

impl AuthFlow {
    pub async fn start(request: AuthRequest) -> Self {
        let mut flow = Self { request, session: None, finished: false };
        flow.restart().await;
        flow
    }

    pub fn request(&self) -> &AuthRequest {
        &self.request
    }

    pub fn selected(&self) -> &Identity {
        &self.request.identities[0]
    }

    pub async fn submit(&mut self, response: Zeroizing<String>) {
        if let Some(session) = self.session.as_mut()
            && session.respond(response).await.is_err()
        {
            self.session = None;
        }
    }

    /// Dismisses the request: pkexec reports "Request dismissed".
    pub fn cancel(mut self) {
        self.finish(false);
    }

    /// The next event, or `None` once the flow has finished.
    pub async fn next(&mut self) -> Option<FlowEvent> {
        if self.finished {
            return None;
        }
        let Some(session) = self.session.as_mut() else {
            self.finish(false);
            return Some(FlowEvent::Unavailable("The polkit agent helper is not available.".into()));
        };
        let cancelled = self.request.cancelled.clone();
        let line = future::or(async { Some(session.read().await) }, async {
            let _ = cancelled.recv().await;
            None
        })
        .await;
        let Some(line) = line else {
            self.finish(false);
            return Some(FlowEvent::Cancelled);
        };
        let event = match line {
            HelperLine::Request { prompt, echo } => {
                session.prompted = true;
                FlowEvent::Request { prompt, echo }
            }
            HelperLine::Info(text) => FlowEvent::Info(text),
            HelperLine::Error(text) => FlowEvent::Error(text),
            HelperLine::Success => {
                self.finish(true);
                FlowEvent::Succeeded
            }
            HelperLine::Failure | HelperLine::Closed if session.prompted => {
                self.restart().await;
                FlowEvent::Failed
            }
            HelperLine::Failure | HelperLine::Closed => {
                self.finish(false);
                FlowEvent::Unavailable("The polkit agent helper ended before it asked anything.".into())
            }
        };
        Some(event)
    }

    async fn restart(&mut self) {
        self.session = None;
        self.session = HelperSession::start(&self.selected().name, &self.request.cookie).await.ok();
    }

    fn finish(&mut self, success: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.session = None;
        let _ = self.request.reply.try_send(success);
    }
}

impl Drop for AuthFlow {
    fn drop(&mut self) {
        self.finish(false);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum HelperLine {
    Request { prompt: String, echo: bool },
    Info(String),
    Error(String),
    Success,
    Failure,
    Closed,
}

type Reader = BufReader<Box<dyn AsyncRead + Unpin + Send>>;
type Writer = Box<dyn AsyncWrite + Unpin + Send>;

struct HelperSession {
    reader: Reader,
    writer: Writer,
    child: Option<Child>,
    prompted: bool,
}

impl HelperSession {
    async fn start(user: &str, cookie: &str) -> std::io::Result<Self> {
        let mut session = match Self::connect(user).await {
            Ok(session) => session,
            Err(_) => Self::spawn(user)?,
        };
        let mut hello = cookie.as_bytes().to_vec();
        hello.push(b'\n');
        session.writer.write_all(&hello).await?;
        session.writer.flush().await?;
        Ok(session)
    }

    async fn connect(user: &str) -> std::io::Result<Self> {
        if !Path::new(HELPER_SOCKET).exists() {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        let stream = Async::<UnixStream>::connect(HELPER_SOCKET).await?;
        let read = Async::new(stream.get_ref().try_clone()?)?;
        let mut session = Self {
            reader: BufReader::new(Box::new(read)),
            writer: Box::new(stream),
            child: None,
            prompted: false,
        };
        let mut hello = user.as_bytes().to_vec();
        hello.push(b'\n');
        session.writer.write_all(&hello).await?;
        Ok(session)
    }

    fn spawn(user: &str) -> std::io::Result<Self> {
        let helper = HELPER_PATHS
            .iter()
            .find(|p| Path::new(p).exists())
            .ok_or(std::io::ErrorKind::NotFound)?;
        let mut child = Command::new(helper)
            .arg(user)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = Async::new(child.stdin.take().ok_or(std::io::ErrorKind::BrokenPipe)?)?;
        let stdout = Async::new(child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?)?;
        Ok(Self {
            reader: BufReader::new(Box::new(stdout)),
            writer: Box::new(stdin),
            child: Some(child),
            prompted: false,
        })
    }

    async fn read(&mut self) -> HelperLine {
        let mut line = Vec::new();
        match self.reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => HelperLine::Closed,
            Ok(_) => parse_line(&line),
        }
    }

    async fn respond(&mut self, response: Zeroizing<String>) -> std::io::Result<()> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(response.len() + 1));
        bytes.extend_from_slice(response.as_bytes());
        drop(response);
        if bytes.last() != Some(&b'\n') {
            bytes.push(b'\n');
        }
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await
    }
}

impl Drop for HelperSession {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || child.wait());
        }
    }
}

fn parse_line(raw: &[u8]) -> HelperLine {
    let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
    let line = String::from_utf8_lossy(&strcompress(raw)).into_owned();
    let rest = |prefix: &str| line.strip_prefix(prefix).map(str::to_owned);
    if let Some(prompt) = rest("PAM_PROMPT_ECHO_OFF ") {
        HelperLine::Request { prompt, echo: false }
    } else if let Some(prompt) = rest("PAM_PROMPT_ECHO_ON ") {
        HelperLine::Request { prompt, echo: true }
    } else if let Some(text) = rest("PAM_ERROR_MSG ") {
        HelperLine::Error(text)
    } else if let Some(text) = rest("PAM_TEXT_INFO ") {
        HelperLine::Info(text)
    } else if line.starts_with("SUCCESS") {
        HelperLine::Success
    } else {
        HelperLine::Failure
    }
}

/// GLib's `g_strcompress`, which the helper's `g_strescape` output needs.
fn strcompress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let c = input[i];
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&next) = input.get(i) else {
            break;
        };
        i += 1;
        match next {
            b'0'..=b'7' => {
                let mut value = u32::from(next - b'0');
                let mut digits = 1;
                while digits < 3 {
                    match input.get(i) {
                        Some(&d @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(d - b'0');
                            i += 1;
                            digits += 1;
                        }
                        _ => break,
                    }
                }
                out.push((value & 0xff) as u8);
            }
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_lines() {
        assert_eq!(
            parse_line(b"PAM_PROMPT_ECHO_OFF Password: \n"),
            HelperLine::Request { prompt: "Password: ".into(), echo: false }
        );
        assert_eq!(
            parse_line(b"PAM_PROMPT_ECHO_ON Login:"),
            HelperLine::Request { prompt: "Login:".into(), echo: true }
        );
        assert_eq!(
            parse_line(b"PAM_ERROR_MSG Authentication\\040failure\\nagain"),
            HelperLine::Error("Authentication failure\nagain".into())
        );
        assert_eq!(parse_line(b"PAM_TEXT_INFO caps\\tlock"), HelperLine::Info("caps\tlock".into()));
        assert_eq!(parse_line(b"SUCCESS\n"), HelperLine::Success);
        assert_eq!(parse_line(b"FAILURE\n"), HelperLine::Failure);
        assert_eq!(parse_line(b"garbage"), HelperLine::Failure);
    }

    #[test]
    fn strcompress_matches_glib() {
        assert_eq!(strcompress(b"a\\\\b\\\"c"), b"a\\b\"c");
        assert_eq!(strcompress(b"\\303\\251"), "é".as_bytes());
        assert_eq!(strcompress(b"trailing\\"), b"trailing");
        assert_eq!(strcompress(b"\\1234"), b"S4");
    }

    #[test]
    fn identities_keep_users_once_in_order() {
        let user = |uid: u32| {
            ("unix-user".to_owned(), HashMap::from([("uid".to_owned(), OwnedValue::from(uid))]))
        };
        let group =
            ("unix-group".to_owned(), HashMap::from([("gid".to_owned(), OwnedValue::from(0u32))]));
        let list = user_identities(&[user(0), group, user(0)]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].uid, 0);
    }
}
