//! The login keyring unlocked with the password the lock just accepted,
//! spoken to gnome-keyring-daemon's control socket the way pam_gnome_keyring
//! does (gnome-keyring's `pam/gkr-pam-client.c`), but after PAM has said yes
//! and under a deadline.
//!
//! The lock's PAM stack carries no pam_gnome_keyring: a session logged in
//! without a password starts the daemon with its login keyring locked, the
//! first Secret Service client then has it raise an unlock prompt, and while
//! that prompt is up the daemon accepts nothing on this socket. The prompt
//! draws under the session lock, so a PAM step waiting on the daemon never
//! returns and the lock never lifts. Here a daemon that does not answer in
//! time is left alone; its prompt is on the desktop once the lock is gone.

use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `GKD_CONTROL_OP_UNLOCK` in gnome-keyring's `gkd-control-codes.h`.
const OP_UNLOCK: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum Unlock {
    Unlocked,
    /// The daemon answered `DENIED`: the keyring's password is not this one.
    Denied,
    /// No daemon listening for this user.
    NoDaemon,
    /// The daemon answered something else, or nothing in time.
    Failed(String),
}

/// `$XDG_RUNTIME_DIR/keyring/control`, where the daemon listens.
pub fn control_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()).map(|d| Path::new(&d).join("keyring/control"))
}

pub fn unlock_login(password: &str, deadline: Duration) -> Unlock {
    match control_path() {
        Some(path) => unlock_at(&path, password, deadline),
        None => Unlock::NoDaemon,
    }
}

pub fn unlock_at(path: &Path, password: &str, deadline: Duration) -> Unlock {
    let uid = unsafe { libc::getuid() };
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Unlock::NoDaemon,
        Err(e) => return Unlock::Failed(format!("{}: {e}", path.display())),
    };
    if !meta.file_type().is_socket() || meta.uid() != uid {
        return Unlock::Failed(format!("{} is not this user's socket", path.display()));
    }
    let mut sock = match UnixStream::connect(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => return Unlock::NoDaemon,
        Err(e) => return Unlock::Failed(format!("connect: {e}")),
    };
    match peer_uid(&sock) {
        Ok(peer) if peer == uid => {}
        Ok(peer) => return Unlock::Failed(format!("the daemon runs as uid {peer}")),
        Err(e) => return Unlock::Failed(format!("peer credentials: {e}")),
    }
    if let Err(e) = sock.set_read_timeout(Some(deadline)).and_then(|_| sock.set_write_timeout(Some(deadline))) {
        return Unlock::Failed(format!("timeouts: {e}"));
    }
    let mut packet = request(password);
    let sent = sock.write_all(&[0]).and_then(|_| sock.write_all(&packet));
    zeroize::Zeroize::zeroize(&mut packet);
    if let Err(e) = sent {
        return Unlock::Failed(format!("write: {e}"));
    }
    let mut reply = [0u8; 8];
    if let Err(e) = sock.read_exact(&mut reply) {
        return Unlock::Failed(match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => format!("no answer in {}s", deadline.as_secs_f32()),
            _ => format!("read: {e}"),
        });
    }
    response(reply)
}

/// The packet's length, the op, then the password as a length and its bytes,
/// every number big-endian (`egg_buffer_encode_uint32`).
fn request(password: &str) -> Vec<u8> {
    let len = 8 + 4 + password.len() as u32;
    let mut out = Vec::with_capacity(len as usize);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&OP_UNLOCK.to_be_bytes());
    out.extend_from_slice(&(password.len() as u32).to_be_bytes());
    out.extend_from_slice(password.as_bytes());
    out
}

/// A length of 8 and one result code: `OK`, `DENIED`, `FAILED`, `NO_DAEMON`.
fn response(reply: [u8; 8]) -> Unlock {
    let len = u32::from_be_bytes(reply[..4].try_into().unwrap());
    let code = u32::from_be_bytes(reply[4..].try_into().unwrap());
    match (len, code) {
        (8, 0) => Unlock::Unlocked,
        (8, 1) => Unlock::Denied,
        (8, 3) => Unlock::NoDaemon,
        (8, c) => Unlock::Failed(format!("the daemon answered {c}")),
        (l, _) => Unlock::Failed(format!("a reply {l} bytes long")),
    }
}

fn peer_uid(sock: &UnixStream) -> io::Result<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe { libc::getsockopt(sock.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) };
    if rc == 0 { Ok(cred.uid) } else { Err(io::Error::last_os_error()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn socket() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control");
        (dir, path)
    }

    #[test]
    fn request_matches_gkr_pam_client() {
        assert_eq!(request("pw"), [0, 0, 0, 14, 0, 0, 0, 1, 0, 0, 0, 2, b'p', b'w']);
    }

    #[test]
    fn unlocks_against_a_daemon_that_answers() {
        let (_dir, path) = socket();
        let listener = UnixListener::bind(&path).unwrap();
        let daemon = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = [0u8; 1 + 14];
            s.read_exact(&mut got).unwrap();
            s.write_all(&[0, 0, 0, 8, 0, 0, 0, 0]).unwrap();
            got
        });
        assert_eq!(unlock_at(&path, "pw", Duration::from_secs(2)), Unlock::Unlocked);
        assert_eq!(daemon.join().unwrap(), [0, 0, 0, 0, 14, 0, 0, 0, 1, 0, 0, 0, 2, b'p', b'w']);
    }

    #[test]
    fn a_daemon_that_never_accepts_times_out() {
        let (_dir, path) = socket();
        let _listener = UnixListener::bind(&path).unwrap();
        let t0 = std::time::Instant::now();
        assert!(matches!(unlock_at(&path, "pw", Duration::from_millis(200)), Unlock::Failed(_)));
        assert!(t0.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn no_socket_is_no_daemon() {
        let (_dir, path) = socket();
        assert_eq!(unlock_at(&path, "pw", Duration::from_millis(200)), Unlock::NoDaemon);
    }

    #[test]
    fn denied_and_failed_replies() {
        assert_eq!(response([0, 0, 0, 8, 0, 0, 0, 1]), Unlock::Denied);
        assert!(matches!(response([0, 0, 0, 8, 0, 0, 0, 2]), Unlock::Failed(_)));
        assert!(matches!(response([0, 0, 0, 9, 0, 0, 0, 0]), Unlock::Failed(_)));
    }
}
