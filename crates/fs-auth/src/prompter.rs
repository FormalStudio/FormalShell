//! The session's system prompter (gcr's `org.gnome.keyring.internal.Prompter`
//! under `org.gnome.keyring.SystemPrompter`) for as long as the boot's lock
//! is up, so gnome-keyring's unlock prompt is answered with the password the
//! lock accepts instead of waiting under it.
//!
//! An autologin starts gnome-keyring-daemon with its login keyring locked.
//! An app asking for a secret through the Secret portal before the password
//! is typed has the daemon prompt for it synchronously on its main thread
//! (gnome-keyring's `daemon/dbus/gkd-secret-portal.c` `unlock_collection`,
//! through `pkcs11/wrap-layer/gkm-wrap-prompt.c`'s `gcr_prompt_password`),
//! and until that prompt is answered the daemon serves nothing else, its
//! control socket included: neither pam_gnome_keyring nor
//! [`crate::keyring`] gets through, and the prompt is on the desktop once
//! the lock lifts. Held here, each password prompt gnome-keyring-daemon
//! raises is answered once with
//! [`Prompter::answer`]'s password and anything else is refused; once
//! nothing is prompting the name is given up, and gcr-prompter is activated
//! for any later prompt.

use std::collections::HashMap;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use futures_lite::future;
use zbus::fdo::RequestNameFlags;
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, OwnedValue};
use zbus::{Connection, interface};
use zeroize::Zeroizing;

const NAME: &str = "org.gnome.keyring.SystemPrompter";
const PATH: &str = "/org/gnome/keyring/Prompter";
const CALLBACK: &str = "org.gnome.keyring.internal.Prompter.Callback";
/// How long prompts still open after the answer are held before they are
/// refused and the name given up.
const LINGER: Duration = Duration::from_secs(10);

type Properties = HashMap<String, OwnedValue>;

/// One prompt, named by its caller's bus name and callback path.
type Key = (String, String);

enum Msg {
    Begin(Key),
    Perform { key: Key, kind: String, properties: Properties, exchange: String },
    Stop(Key),
    Answer(Zeroizing<String>),
    Release,
}

struct PrompterIface {
    tx: Sender<Msg>,
}

fn key(header: &Header<'_>, callback: &ObjectPath<'_>) -> Key {
    (header.sender().map(|s| s.to_string()).unwrap_or_default(), callback.to_string())
}

#[interface(name = "org.gnome.keyring.internal.Prompter")]
impl PrompterIface {
    async fn begin_prompting(&self, #[zbus(header)] header: Header<'_>, callback: ObjectPath<'_>) {
        let _ = self.tx.send(Msg::Begin(key(&header, &callback))).await;
    }

    async fn perform_prompt(&self, #[zbus(header)] header: Header<'_>, callback: ObjectPath<'_>, kind: String, properties: Properties, exchange: String) {
        let _ = self.tx.send(Msg::Perform { key: key(&header, &callback), kind, properties, exchange }).await;
    }

    async fn stop_prompting(&self, #[zbus(header)] header: Header<'_>, callback: ObjectPath<'_>) {
        let _ = self.tx.send(Msg::Stop(key(&header, &callback))).await;
    }
}

/// The prompter while it holds the name. Dropping it unanswered refuses
/// whatever is still waiting and gives the name up.
pub struct Prompter {
    tx: Sender<Msg>,
    answered: bool,
}

impl Prompter {
    /// Takes the name on a thread of its own, returning at once; a name
    /// already owned (gcr-prompter came first) leaves prompts with it.
    pub fn claim() -> Self {
        let (tx, rx) = async_channel::unbounded();
        let iface = PrompterIface { tx: tx.clone() };
        let spawned = std::thread::Builder::new().name("fs-prompter".into()).spawn(move || future::block_on(run(iface, rx)));
        if let Err(e) = spawned {
            eprintln!("keyring prompter: {e}");
        }
        Self { tx, answered: false }
    }

    /// The password the lock accepted: every password prompt held or still
    /// to come is answered with it, once.
    pub fn answer(mut self, password: Zeroizing<String>) {
        self.answered = self.tx.try_send(Msg::Answer(password)).is_ok();
    }
}

impl Drop for Prompter {
    fn drop(&mut self) {
        if !self.answered {
            let _ = self.tx.try_send(Msg::Release);
        }
    }
}

#[derive(Default)]
struct Prompt {
    exchange: exchange::Exchange,
    /// `password-new`: a keyring being made, whose password is not ours to pick.
    new_password: bool,
    /// A password prompt waiting for the answer.
    waiting: bool,
    answered: bool,
}

async fn run(iface: PrompterIface, rx: Receiver<Msg>) {
    let connection = match claim(iface).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            eprintln!("keyring prompter: {NAME} is already owned, prompts stay with it");
            return;
        }
        Err(e) => {
            eprintln!("keyring prompter: {e}");
            return;
        }
    };
    eprintln!("keyring prompter: holding {NAME}");
    let mut prompts: HashMap<Key, Prompt> = HashMap::new();
    let mut password: Option<Zeroizing<String>> = None;
    loop {
        let msg = if password.is_some() {
            match future::or(async { rx.recv().await.ok() }, async {
                async_io::Timer::after(LINGER).await;
                Some(Msg::Release)
            })
            .await
            {
                Some(m) => m,
                None => Msg::Release,
            }
        } else {
            rx.recv().await.unwrap_or(Msg::Release)
        };
        match msg {
            Msg::Begin(key) => {
                if !from_keyring(&connection, &key.0).await {
                    eprintln!("keyring prompter: refused a prompt from {}, not gnome-keyring-daemon", key.0);
                    done(&connection, &key).await;
                    continue;
                }
                eprintln!("keyring prompter: a prompt from {}", key.0);
                let prompt = prompts.entry(key.clone()).or_default();
                let begin = prompt.exchange.begin();
                ready(&connection, &key, "", begin).await;
            }
            Msg::Perform { key, kind, properties, exchange } => {
                let Some(prompt) = prompts.get_mut(&key) else { continue };
                if let Some(v) = properties.get("password-new").and_then(|v| bool::try_from(v).ok()) {
                    prompt.new_password = v;
                }
                if !prompt.exchange.receive(&exchange) {
                    eprintln!("keyring prompter: an invalid secret exchange from {}", key.0);
                    let sent = prompt.exchange.begin();
                    ready(&connection, &key, "no", sent).await;
                } else if kind == "password" && !prompt.new_password && !prompt.answered {
                    prompt.waiting = true;
                    if let Some(pw) = &password {
                        reply(&connection, &key, prompt, pw).await;
                    }
                } else {
                    if prompt.answered {
                        eprintln!("keyring prompter: the keyring refused the login password");
                    }
                    let sent = prompt.exchange.begin();
                    ready(&connection, &key, "no", sent).await;
                }
            }
            Msg::Stop(key) => {
                if prompts.remove(&key).is_some() {
                    done(&connection, &key).await;
                }
            }
            Msg::Answer(pw) => {
                for (key, prompt) in prompts.iter_mut().filter(|(_, p)| p.waiting) {
                    reply(&connection, key, prompt, &pw).await;
                }
                password = Some(pw);
            }
            Msg::Release => {
                for (key, prompt) in prompts.iter_mut().filter(|(_, p)| p.waiting) {
                    let sent = prompt.exchange.begin();
                    ready(&connection, key, "no", sent).await;
                }
                for (key, _) in prompts.drain() {
                    done(&connection, &key).await;
                }
                let _ = connection.release_name(NAME).await;
                eprintln!("keyring prompter: released {NAME}");
                return;
            }
        }
        if password.is_some() && prompts.is_empty() {
            let _ = connection.release_name(NAME).await;
            eprintln!("keyring prompter: answered, released {NAME}");
            return;
        }
    }
}

/// The installed daemon, where every distro install.sh covers puts it.
const DAEMON: &str = "/usr/bin/gnome-keyring-daemon";

/// The password goes to gnome-keyring alone: the sender has to be the
/// owner of `org.gnome.keyring` and run the root-owned installed daemon,
/// since the secret exchange encrypts the answer for whoever asked without
/// saying who.
async fn from_keyring(connection: &Connection, sender: &str) -> bool {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(connection).await else { return false };
    let Ok(name) = zbus::names::BusName::try_from(sender) else { return false };
    let owner = dbus.get_name_owner(zbus::names::BusName::from_static_str("org.gnome.keyring").expect("a bus name")).await;
    if owner.ok().is_none_or(|o| o.as_str() != sender) {
        return false;
    }
    let Ok(pid) = dbus.get_connection_unix_process_id(name).await else { return false };
    let Ok(daemon) = std::fs::canonicalize(DAEMON) else { return false };
    let root_owned = std::fs::metadata(&daemon).is_ok_and(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0);
    root_owned && std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| exe == daemon)
}

async fn claim(iface: PrompterIface) -> zbus::Result<Option<Connection>> {
    let connection = zbus::connection::Builder::session()?.serve_at(PATH, iface)?.build().await?;
    let reply = connection.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into()).await;
    match reply {
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner) => Ok(Some(connection)),
        Ok(_) | Err(zbus::Error::NameTaken) => Ok(None),
        Err(e) => Err(e),
    }
}

async fn reply(connection: &Connection, key: &Key, prompt: &mut Prompt, password: &str) {
    prompt.waiting = false;
    prompt.answered = true;
    match prompt.exchange.send(password.as_bytes()) {
        Some(sent) => ready(connection, key, "yes", sent).await,
        None => {
            let sent = prompt.exchange.begin();
            ready(connection, key, "no", sent).await;
        }
    }
}

async fn ready(connection: &Connection, (name, path): &Key, reply: &str, exchange: String) {
    let exchange = Zeroizing::new(exchange);
    let body = (reply, Properties::new(), exchange.as_str());
    if let Err(e) = connection.call_method(Some(name.as_str()), path.as_str(), Some(CALLBACK), "PromptReady", &body).await {
        eprintln!("keyring prompter: PromptReady to {name}: {e}");
    }
}

/// Sent with no reply waited on: a caller that never answers (one refused
/// above) would otherwise hold up every other prompt.
async fn done(connection: &Connection, (name, path): &Key) {
    let msg = zbus::Message::method_call(path.as_str(), "PromptDone")
        .and_then(|b| b.destination(name.as_str()))
        .and_then(|b| b.interface(CALLBACK))
        .and_then(|b| b.with_flags(zbus::message::Flags::NoReplyExpected))
        .and_then(|b| b.build(&()));
    if let Ok(msg) = msg {
        let _ = connection.send(&msg).await;
    }
}

/// gcr's secret exchange, `sx-aes-1` (`gcr/gcr-secret-exchange.c`): a
/// Diffie-Hellman pair in RFC 3526's 1536-bit group, the shared value padded
/// to the prime's length through HKDF-SHA256 (no salt, no info) into an
/// AES-128 key, and the secret sent CBC with PKCS#7 under a random IV. Each
/// message is a key file under `[sx-aes-1]` carrying base64 `public`, and
/// with a secret `secret` and `iv`.
mod exchange {
    use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use num_bigint::BigUint;
    use zeroize::Zeroizing;

    const GROUP: &str = "sx-aes-1";
    /// RFC 3526 group 5, egg-dh.c's `ietf-ike-grp-modp-1536`; the generator is 2.
    const PRIME: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA237327FFFFFFFFFFFFFFFF";
    const PRIME_BYTES: usize = 192;

    fn prime() -> BigUint {
        BigUint::parse_bytes(PRIME.as_bytes(), 16).expect("the group's prime")
    }

    fn random(buf: &mut [u8]) -> bool {
        use std::io::Read;
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).is_ok()
    }

    pub struct Exchange {
        private: BigUint,
        public: Vec<u8>,
        key: Option<Zeroizing<[u8; 16]>>,
    }

    impl Default for Exchange {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Exchange {
        pub fn new() -> Self {
            let p = prime();
            let mut bytes = Zeroizing::new([0u8; PRIME_BYTES]);
            // Under half the prime, never zero, as egg_dh_gen_pair makes it.
            while !random(&mut bytes[..]) || bytes.iter().all(|b| *b == 0) {}
            bytes[0] &= 0x7f;
            let private = BigUint::from_bytes_be(&bytes[..]);
            let public = BigUint::from(2u32).modpow(&private, &p).to_bytes_be();
            Self { private, public, key: None }
        }

        /// The first message: this side's public key alone.
        pub fn begin(&self) -> String {
            format!("[{GROUP}]\npublic={}\n", STANDARD.encode(&self.public))
        }

        /// Takes the peer's message, deriving the key off its public value.
        pub fn receive(&mut self, input: &str) -> bool {
            let fields = parse(input);
            let Some(peer) = fields.get("public").and_then(|v| STANDARD.decode(v).ok()) else { return false };
            let p = prime();
            let y = BigUint::from_bytes_be(&peer);
            if y <= BigUint::from(1u32) || y >= &p - 1u32 {
                return false;
            }
            let shared = Zeroizing::new(y.modpow(&self.private, &p).to_bytes_be());
            let mut ikm = Zeroizing::new(vec![0u8; PRIME_BYTES - shared.len()]);
            ikm.extend_from_slice(&shared);
            let mut key = Zeroizing::new([0u8; 16]);
            if hkdf::Hkdf::<sha2::Sha256>::new(None, &ikm).expand(&[], &mut key[..]).is_err() {
                return false;
            }
            self.key = Some(key);
            true
        }

        /// `secret` sealed for the peer, with this side's public key.
        pub fn send(&self, secret: &[u8]) -> Option<String> {
            let key = self.key.as_ref()?;
            let mut iv = [0u8; 16];
            if !random(&mut iv) {
                return None;
            }
            let sealed = cbc::Encryptor::<aes::Aes128>::new(key[..].into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(secret);
            Some(format!("[{GROUP}]\npublic={}\nsecret={}\niv={}\n", STANDARD.encode(&self.public), STANDARD.encode(sealed), STANDARD.encode(iv)))
        }

        /// The secret a peer's message carries, for the tests' client side.
        #[cfg(test)]
        pub fn open(&self, input: &str) -> Option<Vec<u8>> {
            use aes::cipher::BlockDecryptMut;
            let fields = parse(input);
            let (secret, iv) = (STANDARD.decode(fields.get("secret")?).ok()?, STANDARD.decode(fields.get("iv")?).ok()?);
            let iv: [u8; 16] = iv.try_into().ok()?;
            cbc::Decryptor::<aes::Aes128>::new(self.key.as_ref()?[..].into(), &iv.into()).decrypt_padded_vec_mut::<Pkcs7>(&secret).ok()
        }
    }

    /// The `[sx-aes-1]` group's keys, as GKeyFile reads them.
    fn parse(input: &str) -> std::collections::HashMap<&str, &str> {
        let mut inside = false;
        let mut out = std::collections::HashMap::new();
        for line in input.lines().map(str::trim) {
            if line.starts_with('[') {
                inside = line == format!("[{GROUP}]");
            } else if inside && let Some((k, v)) = line.split_once('=') {
                out.insert(k.trim(), v.trim());
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn both_sides_agree_and_the_secret_comes_through() {
            let (mut prompter, mut client) = (Exchange::new(), Exchange::new());
            assert!(client.receive(&prompter.begin()));
            assert!(prompter.receive(&client.begin()));
            assert_eq!(prompter.key.as_deref(), client.key.as_deref());
            let sent = prompter.send(b"hunter2").unwrap();
            assert!(sent.starts_with("[sx-aes-1]\npublic="));
            assert!(client.receive(&sent));
            assert_eq!(client.open(&sent).unwrap(), b"hunter2");
        }

        #[test]
        fn a_public_value_out_of_range_or_missing_is_refused() {
            let mut e = Exchange::new();
            assert!(!e.receive("[sx-aes-1]\npublic=AQ==\n"));
            assert!(!e.receive("[sx-aes-1]\n"));
            assert!(!e.receive("[other]\npublic=Ag==\n"));
            assert!(e.send(b"x").is_none());
        }

        #[test]
        fn the_public_value_is_two_to_the_private() {
            let e = Exchange::new();
            assert_eq!(BigUint::from_bytes_be(&e.public), BigUint::from(2u32).modpow(&e.private, &prime()));
            assert!(e.private < prime() >> 1);
        }
    }
}
