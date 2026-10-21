//! greetd-ipc(7): a native-endian u32 length, then that many bytes of JSON,
//! one response for every request. The socket is `$GREETD_SOCK`.
//!
//! [`Conversation`] is the dispatch on greetd's state with the socket
//! taken out: it is fed responses and hands back the next request.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    CreateSession { username: String },
    PostAuthMessageResponse {
        #[serde(serialize_with = "secret")]
        response: Option<Zeroizing<String>>,
    },
    StartSession { cmd: Vec<String>, env: Vec<String> },
    CancelSession,
}

fn secret<S: serde::Serializer>(v: &Option<Zeroizing<String>>, s: S) -> Result<S::Ok, S::Error> {
    v.as_ref().map(|t| t.as_str()).serialize(s)
}

impl Request {
    /// The request's type, the one part of it that is safe to log.
    pub fn name(&self) -> &'static str {
        match self {
            Request::CreateSession { .. } => "create_session",
            Request::PostAuthMessageResponse { .. } => "post_auth_message_response",
            Request::StartSession { .. } => "start_session",
            Request::CancelSession => "cancel_session",
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Visible,
    Secret,
    Info,
    Error,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    AuthError,
    Error,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error { error_type: ErrorType, description: String },
    AuthMessage { auth_message_type: MessageType, auth_message: String },
}

pub fn write<W: Write>(w: &mut W, req: &Request) -> io::Result<()> {
    let body = Zeroizing::new(serde_json::to_vec(req)?);
    let len = u32::try_from(body.len()).map_err(|_| io::Error::other("request too long"))?;
    let mut frame = Zeroizing::new(Vec::with_capacity(4 + body.len()));
    frame.extend_from_slice(&len.to_ne_bytes());
    frame.extend_from_slice(&body);
    w.write_all(&frame)?;
    w.flush()
}

/// One response, and its JSON as greetd sent it.
pub fn read<R: Read>(r: &mut R) -> io::Result<(Response, String)> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let mut body = vec![0u8; u32::from_ne_bytes(len) as usize];
    r.read_exact(&mut body)?;
    let raw = String::from_utf8(body).map_err(|_| io::Error::other("response is not UTF-8"))?;
    let resp = serde_json::from_str(&raw).map_err(|e| io::Error::other(format!("bad response {raw}: {e}")))?;
    Ok((resp, raw))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// No conversation: the field asks for a username.
    Inactive,
    /// A request is out and nothing typeable is being asked.
    Waiting,
    /// greetd asked a question the field answers.
    Asking,
    /// start_session succeeded: the greeter's work is done.
    Launched,
}

/// What a step asks of the caller.
#[derive(Debug, PartialEq)]
pub enum Step {
    Send(Request),
    Exit,
    Nothing,
}

pub struct Conversation {
    pub phase: Phase,
    /// The last auth_message, "" until one arrives.
    pub message: String,
    pub echo: bool,
    /// The field's error caption; "" shows none.
    pub error: String,
    /// The argv a successful login runs, and the environment it gets.
    pub command: Vec<String>,
    pub env: Vec<String>,
    /// The username the conversation was opened for.
    pub user: String,
    /// The request whose response is due.
    pending: Option<&'static str>,
}

impl Conversation {
    pub fn new(command: Vec<String>) -> Self {
        Self { phase: Phase::Inactive, message: String::new(), echo: false, error: String::new(), command, env: Vec::new(), user: String::new(), pending: None }
    }

    /// The label for the current prompt.
    pub fn label(&self) -> &str {
        match self.phase {
            Phase::Asking if !self.message.is_empty() => &self.message,
            Phase::Asking => "Password",
            Phase::Inactive => "User",
            Phase::Waiting | Phase::Launched => "Authenticating",
        }
    }

    pub fn input_enabled(&self) -> bool {
        matches!(self.phase, Phase::Inactive | Phase::Asking)
    }

    pub fn masked(&self) -> bool {
        self.phase == Phase::Asking && !self.echo
    }

    /// The field's Enter.
    pub fn submit(&mut self, text: Zeroizing<String>) -> Step {
        match self.phase {
            Phase::Inactive if !text.is_empty() => {
                self.error.clear();
                self.user = text.to_string();
                self.send(Request::CreateSession { username: text.to_string() })
            }
            Phase::Asking => self.send(Request::PostAuthMessageResponse { response: Some(text) }),
            _ => Step::Nothing,
        }
    }

    fn send(&mut self, req: Request) -> Step {
        self.pending = Some(req.name());
        self.phase = Phase::Waiting;
        Step::Send(req)
    }

    /// The socket failed under a request: nothing more can be said to greetd.
    pub fn broken(&mut self, why: &str) {
        self.pending = None;
        self.phase = Phase::Inactive;
        if self.error.is_empty() {
            self.error = if why.is_empty() { "greetd error".into() } else { why.into() };
        }
    }

    pub fn receive(&mut self, resp: Response) -> Step {
        let after = self.pending.take();
        match resp {
            Response::AuthMessage { auth_message_type, auth_message } => {
                self.message = auth_message;
                match auth_message_type {
                    MessageType::Visible | MessageType::Secret => {
                        self.echo = auth_message_type == MessageType::Visible;
                        self.phase = Phase::Asking;
                        Step::Nothing
                    }
                    // Nothing to type: greetd still waits for an answer.
                    MessageType::Info | MessageType::Error => {
                        if auth_message_type == MessageType::Error {
                            self.error = self.message.clone();
                        }
                        self.send(Request::PostAuthMessageResponse { response: None })
                    }
                }
            }
            Response::Success => match after {
                Some("start_session") => {
                    self.phase = Phase::Launched;
                    Step::Exit
                }
                Some("cancel_session") => {
                    self.phase = Phase::Inactive;
                    Step::Nothing
                }
                _ => {
                    eprintln!("greetd: authentication complete");
                    let (cmd, env) = (self.command.clone(), self.env.clone());
                    self.send(Request::StartSession { cmd, env })
                }
            },
            Response::Error { error_type, description } => {
                // greetd answers a cancel after a finished conversation with
                // an error of its own; the failure already showing stays.
                if after == Some("cancel_session") {
                    self.phase = Phase::Inactive;
                    return Step::Nothing;
                }
                self.error = match (error_type, description.is_empty()) {
                    (_, false) => description,
                    (ErrorType::AuthError, true) => "Authentication failed".into(),
                    (ErrorType::Error, true) => "greetd error".into(),
                };
                self.message.clear();
                self.send(Request::CancelSession)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(s: &str) -> Zeroizing<String> {
        Zeroizing::new(s.into())
    }

    fn parse(raw: &str) -> Response {
        let mut frame = (raw.len() as u32).to_ne_bytes().to_vec();
        frame.extend_from_slice(raw.as_bytes());
        read(&mut frame.as_slice()).unwrap().0
    }

    #[test]
    fn requests_frame_as_greetd_reads_them() {
        let mut out = Vec::new();
        write(&mut out, &Request::CreateSession { username: "test".into() }).unwrap();
        let len = u32::from_ne_bytes(out[..4].try_into().unwrap()) as usize;
        assert_eq!(&out[4..], br#"{"type":"create_session","username":"test"}"#);
        assert_eq!(len, out.len() - 4);
        let mut out = Vec::new();
        write(&mut out, &Request::PostAuthMessageResponse { response: None }).unwrap();
        assert_eq!(&out[4..], br#"{"type":"post_auth_message_response","response":null}"#);
        let mut out = Vec::new();
        write(&mut out, &Request::StartSession { cmd: vec!["Hyprland".into()], env: vec![] }).unwrap();
        assert_eq!(&out[4..], br#"{"type":"start_session","cmd":["Hyprland"],"env":[]}"#);
    }

    #[test]
    fn a_wrong_password_then_the_right_one() {
        let mut c = Conversation::new(vec!["Hyprland".into()]);
        assert_eq!(c.label(), "User");
        assert_eq!(c.submit(z("")), Step::Nothing);
        assert!(matches!(c.submit(z("test")), Step::Send(Request::CreateSession { .. })));
        assert!(!c.input_enabled());
        c.receive(parse(r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#));
        assert_eq!((c.label(), c.masked()), ("Password: ", true));
        assert!(matches!(c.submit(z("nope")), Step::Send(Request::PostAuthMessageResponse { .. })));
        let step = c.receive(parse(r#"{"type":"error","error_type":"auth_error","description":"pam_authenticate: AUTH_ERR"}"#));
        assert_eq!(step, Step::Send(Request::CancelSession));
        let step = c.receive(parse(r#"{"type":"error","error_type":"error","description":"unable to send message: Connection refused"}"#));
        assert_eq!(step, Step::Nothing);
        assert_eq!((c.phase.clone(), c.error.as_str()), (Phase::Inactive, "pam_authenticate: AUTH_ERR"));
        c.submit(z("test"));
        assert!(c.error.is_empty());
        c.receive(parse(r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#));
        c.submit(z("right"));
        let step = c.receive(parse(r#"{"type":"success"}"#));
        assert_eq!(step, Step::Send(Request::StartSession { cmd: vec!["Hyprland".into()], env: vec![] }));
        assert_eq!(c.receive(parse(r#"{"type":"success"}"#)), Step::Exit);
    }

    #[test]
    fn info_messages_are_answered_for_the_user() {
        let mut c = Conversation::new(vec![]);
        c.submit(z("test"));
        let step = c.receive(parse(r#"{"type":"auth_message","auth_message_type":"info","auth_message":"Touch the key"}"#));
        assert_eq!(step, Step::Send(Request::PostAuthMessageResponse { response: None }));
        assert_eq!(c.label(), "Authenticating");
        c.receive(parse(r#"{"type":"auth_message","auth_message_type":"visible","auth_message":"Code: "}"#));
        assert!(!c.masked());
    }
}
