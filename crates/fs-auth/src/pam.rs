//! A PAM conversation for the lock screen: one `pam_authenticate` run on its
//! own thread, its messages and result posted back over a channel, then
//! `pam_acct_mgmt`, so an account expired or locked out since login is
//! refused even with the right password.
//!
//! Straight onto libpam through pam-sys2 rather than a wrapper crate: the
//! answer is copied from the caller's zeroizing buffer into the one malloc'd
//! buffer PAM takes ownership of, and nothing else ever holds it. Every
//! answer that never reaches PAM (an aborted conversation, a refused
//! response) is wiped before it is freed.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::thread;

use async_channel::{Receiver, Sender};
use pam_sys2 as sys;
use zeroize::{Zeroize, Zeroizing};

pub const LOCK_SERVICE: &str = "formalshell-lock";

/// Linux-PAM's extension style (_pam_types.h), not in pam-sys2's root.
const PAM_RADIO_TYPE: c_int = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// A prompt that needs `Conversation::respond`. `echo` is false for a
    /// password.
    Prompt { text: String, echo: bool },
    Info(String),
    Error(String),
}

/// How `pam_authenticate` and the account check after it ended. The strings
/// are PAM's own `pam_strerror`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Success,
    /// `PAM_AUTH_ERR`: the wrong password.
    Failed(String),
    /// `PAM_MAXTRIES`: a module has locked the account for now.
    MaxTries(String),
    /// The password was right but `pam_acct_mgmt` refused the account:
    /// expired, inactive, or shut out by a module (`pam_access`, `pam_nologin`).
    AccountUnavailable(String),
    /// The password was right and is past its age (`PAM_NEW_AUTHTOK_REQD`).
    NewPasswordRequired(String),
    /// `pam_start` failed, usually a service with no file under /etc/pam.d.
    StartFailed(String),
    /// Any other PAM error.
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Message(Message),
    Done(Outcome),
}

/// One authentication attempt. Dropping it aborts a conversation waiting on
/// a response: the pending prompt answers `PAM_CONV_ERR` and the thread ends.
pub struct Conversation {
    events: Receiver<Event>,
    responses: Sender<Zeroizing<String>>,
}

impl Conversation {
    pub fn start(service: &str, user: &str) -> std::io::Result<Self> {
        let (event_tx, events) = async_channel::unbounded();
        let (responses, response_rx) = async_channel::bounded(1);
        let service = service.to_owned();
        let user = user.to_owned();
        thread::Builder::new()
            .name("fs-pam".into())
            .spawn(move || {
                let outcome = authenticate(&service, &user, &event_tx, response_rx);
                let _ = event_tx.send_blocking(Event::Done(outcome));
            })?;
        Ok(Self { events, responses })
    }

    /// The next message, then exactly one `Event::Done`, then `None`.
    pub async fn next(&self) -> Option<Event> {
        self.events.recv().await.ok()
    }

    /// Answers the pending prompt. False when the conversation has ended;
    /// the refused answer is wiped as it drops.
    pub fn respond(&self, response: Zeroizing<String>) -> bool {
        self.responses.try_send(response).is_ok()
    }
}

/// One `pam_authenticate` on the calling thread with one answer ready: the
/// first prompt takes `password`, any later one ends the conversation with
/// `PAM_CONV_ERR`. Messages PAM shows along the way are dropped.
pub fn check(service: &str, user: &str, password: Zeroizing<String>) -> Outcome {
    let (events, _shown) = async_channel::unbounded();
    let (answer, responses) = async_channel::bounded(1);
    let _ = answer.try_send(password);
    drop(answer);
    authenticate(service, user, &events, responses)
}

/// The login name of the user this process runs as.
pub fn current_user() -> Option<String> {
    let mut buf = vec![0 as c_char; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = ptr::null_mut();
    // SAFETY: getpwuid_r writes into `pwd` and `buf`, both sized as passed.
    let rc = unsafe { libc::getpwuid_r(libc::getuid(), &mut pwd, buf.as_mut_ptr(), buf.len(), &mut found) };
    if rc != 0 || found.is_null() || pwd.pw_name.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(pwd.pw_name) }.to_string_lossy().into_owned())
}

struct Handler {
    events: Sender<Event>,
    responses: Receiver<Zeroizing<String>>,
}

fn authenticate(service: &str, user: &str, events: &Sender<Event>, responses: Receiver<Zeroizing<String>>) -> Outcome {
    let (Ok(c_service), Ok(c_user)) = (CString::new(service), CString::new(user)) else {
        return Outcome::StartFailed("invalid service or user name".into());
    };
    let mut handler = Handler { events: events.clone(), responses };
    let conv = sys::pam_conv { conv: Some(converse), appdata_ptr: (&raw mut handler).cast::<c_void>() };
    let mut handle: *mut sys::pam_handle_t = ptr::null_mut();
    // SAFETY: every pointer outlives the handle: `conv` and `handler` live
    // on this frame until pam_end below, and pam_start copies the strings.
    let started = unsafe { sys::pam_start(c_service.as_ptr(), c_user.as_ptr(), &conv, &mut handle) };
    if started != sys::PAM_SUCCESS as c_int || handle.is_null() {
        return Outcome::StartFailed(strerror(handle, started));
    }
    // SAFETY: `handle` came from a successful pam_start and is ended once.
    let status = unsafe { sys::pam_authenticate(handle, 0) };
    let mut outcome = auth_outcome(status, strerror(handle, status));
    let mut last = status;
    if outcome == Outcome::Success {
        // SAFETY: the same live handle, before pam_end.
        last = unsafe { sys::pam_acct_mgmt(handle, 0) };
        outcome = account_outcome(last, strerror(handle, last));
    }
    unsafe { sys::pam_end(handle, last) };
    outcome
}

fn auth_outcome(status: c_int, text: String) -> Outcome {
    match status {
        s if s == sys::PAM_SUCCESS as c_int => Outcome::Success,
        s if s == sys::PAM_AUTH_ERR as c_int => Outcome::Failed(text),
        s if s == sys::PAM_MAXTRIES as c_int => Outcome::MaxTries(text),
        _ => Outcome::Error(text),
    }
}

fn account_outcome(status: c_int, text: String) -> Outcome {
    match status {
        s if s == sys::PAM_SUCCESS as c_int => Outcome::Success,
        s if s == sys::PAM_NEW_AUTHTOK_REQD as c_int => Outcome::NewPasswordRequired(text),
        s if [sys::PAM_ACCT_EXPIRED, sys::PAM_AUTHTOK_EXPIRED, sys::PAM_PERM_DENIED, sys::PAM_USER_UNKNOWN, sys::PAM_AUTH_ERR]
            .iter()
            .any(|c| s == *c as c_int) =>
        {
            Outcome::AccountUnavailable(text)
        }
        _ => Outcome::Error(text),
    }
}

fn strerror(handle: *mut sys::pam_handle_t, status: c_int) -> String {
    // SAFETY: pam_strerror returns a static string and accepts any handle.
    let text = unsafe { sys::pam_strerror(handle, status) };
    if text.is_null() {
        return format!("PAM error {status}");
    }
    unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
}

impl Handler {
    fn ask(&mut self, prompt: &CStr, echo: bool) -> Option<Zeroizing<String>> {
        let text = prompt.to_string_lossy().into_owned();
        self.events.send_blocking(Event::Message(Message::Prompt { text, echo })).ok()?;
        self.responses.recv_blocking().ok()
    }

    fn tell(&mut self, message: Message) {
        let _ = self.events.send_blocking(Event::Message(message));
    }
}

/// A malloc'd, NUL-terminated copy PAM frees itself. None for an answer
/// with an interior NUL, which no module could read whole.
fn to_pam(answer: &[u8]) -> Option<*mut c_char> {
    if answer.contains(&0) {
        return None;
    }
    // SAFETY: a fresh allocation of len + 1 bytes, written in full.
    unsafe {
        let buf = libc::malloc(answer.len() + 1).cast::<u8>();
        if buf.is_null() {
            return None;
        }
        ptr::copy_nonoverlapping(answer.as_ptr(), buf, answer.len());
        *buf.add(answer.len()) = 0;
        Some(buf.cast())
    }
}

/// Wipes and frees replies already handed out, for a conversation that
/// fails part way through.
unsafe fn drop_replies(replies: *mut sys::pam_response, filled: usize) {
    for i in 0..filled {
        let r = unsafe { &mut *replies.add(i) };
        if !r.resp.is_null() {
            let len = unsafe { libc::strlen(r.resp) };
            unsafe { std::slice::from_raw_parts_mut(r.resp.cast::<u8>(), len) }.zeroize();
            unsafe { libc::free(r.resp.cast()) };
            r.resp = ptr::null_mut();
        }
    }
    unsafe { libc::free(replies.cast()) };
}

unsafe extern "C" fn converse(
    count: c_int,
    messages: *mut *const sys::pam_message,
    out: *mut *mut sys::pam_response,
    data: *mut c_void,
) -> c_int {
    let conv_err = sys::PAM_CONV_ERR as c_int;
    if count <= 0 || messages.is_null() || out.is_null() || data.is_null() {
        return conv_err;
    }
    let count = count as usize;
    // SAFETY: `data` is the Handler authenticate() put in the pam_conv.
    let handler = unsafe { &mut *data.cast::<Handler>() };
    let replies = unsafe { libc::calloc(count, std::mem::size_of::<sys::pam_response>()) }.cast::<sys::pam_response>();
    if replies.is_null() {
        return sys::PAM_BUF_ERR as c_int;
    }
    for i in 0..count {
        // Linux-PAM passes an array of pointers to messages.
        let message = unsafe { &**messages.add(i) };
        let text = if message.msg.is_null() { c"" } else { unsafe { CStr::from_ptr(message.msg) } };
        let style = message.msg_style;
        let answer = if style == sys::PAM_PROMPT_ECHO_OFF as c_int {
            handler.ask(text, false)
        } else if style == sys::PAM_PROMPT_ECHO_ON as c_int || style == PAM_RADIO_TYPE {
            handler.ask(text, true)
        } else if style == sys::PAM_ERROR_MSG as c_int {
            handler.tell(Message::Error(text.to_string_lossy().into_owned()));
            continue;
        } else if style == sys::PAM_TEXT_INFO as c_int {
            handler.tell(Message::Info(text.to_string_lossy().into_owned()));
            continue;
        } else {
            None
        };
        let Some(answer) = answer else {
            unsafe { drop_replies(replies, i) };
            return conv_err;
        };
        let Some(buf) = to_pam(answer.as_bytes()) else {
            drop(answer);
            unsafe { drop_replies(replies, i) };
            return conv_err;
        };
        drop(answer);
        unsafe { (*replies.add(i)).resp = buf };
    }
    unsafe { *out = replies };
    sys::PAM_SUCCESS as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interior_nul_is_refused() {
        assert!(to_pam(b"a\0b").is_none());
    }

    #[test]
    fn answer_is_copied_whole_with_its_terminator() {
        let buf = to_pam(b"hunter2").unwrap();
        let copied = unsafe { CStr::from_ptr(buf) }.to_bytes().to_vec();
        unsafe { libc::free(buf.cast()) };
        assert_eq!(copied, b"hunter2");
    }

    #[test]
    fn authentication_maps_its_status() {
        assert_eq!(auth_outcome(sys::PAM_SUCCESS as c_int, String::new()), Outcome::Success);
        assert_eq!(auth_outcome(sys::PAM_AUTH_ERR as c_int, "x".into()), Outcome::Failed("x".into()));
        assert_eq!(auth_outcome(sys::PAM_MAXTRIES as c_int, "x".into()), Outcome::MaxTries("x".into()));
        assert_eq!(auth_outcome(sys::PAM_NEW_AUTHTOK_REQD as c_int, "x".into()), Outcome::Error("x".into()));
    }

    #[test]
    fn the_account_check_keeps_an_expired_or_aged_account_out() {
        assert_eq!(account_outcome(sys::PAM_SUCCESS as c_int, String::new()), Outcome::Success);
        for s in [sys::PAM_ACCT_EXPIRED, sys::PAM_AUTHTOK_EXPIRED, sys::PAM_PERM_DENIED, sys::PAM_USER_UNKNOWN, sys::PAM_AUTH_ERR] {
            assert_eq!(account_outcome(s as c_int, "x".into()), Outcome::AccountUnavailable("x".into()));
        }
        assert_eq!(account_outcome(sys::PAM_NEW_AUTHTOK_REQD as c_int, "x".into()), Outcome::NewPasswordRequired("x".into()));
        assert_eq!(account_outcome(sys::PAM_SYSTEM_ERR as c_int, "x".into()), Outcome::Error("x".into()));
    }

    #[test]
    fn missing_service_fails_to_start_or_errors() {
        let conversation = Conversation::start("fs-auth-no-such-service", "nobody").unwrap();
        let outcome = futures_lite::future::block_on(async {
            loop {
                match conversation.next().await {
                    Some(Event::Done(outcome)) => break outcome,
                    Some(Event::Message(Message::Prompt { .. })) => {
                        conversation.respond(Zeroizing::new(String::new()));
                    }
                    Some(Event::Message(_)) => {}
                    None => panic!("conversation ended without a result"),
                }
            }
        });
        assert_ne!(outcome, Outcome::Success);
    }
}
