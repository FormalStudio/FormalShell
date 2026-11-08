//! `watch` against a fake daemon on a dbus-daemon of the test's own
//! (`dev/check.sh fs-dbus`).

use std::future::Future;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use async_io::{Timer, block_on};
use fs_dbus::{Watched, watch};
use futures_lite::{Stream, StreamExt, future};
use zbus::message::Type;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, connection, interface};

const SERVICE: &str = "org.example.Fake";
const PATH: &str = "/org/example/Fake";
const IFACE: &str = "org.example.Fake";

/// Distro session configs live under a sysconfdir the build sandbox lacks.
const BUS_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=TMPDIR</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

struct Bus {
    child: Child,
    address: String,
    config: PathBuf,
}

impl Bus {
    fn start(test: &str) -> Bus {
        let tmp = std::env::temp_dir();
        let config = tmp.join(format!("fs-dbus-bus-{}-{test}.conf", std::process::id()));
        std::fs::write(&config, BUS_CONFIG.replace("TMPDIR", tmp.to_str().unwrap())).unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon on PATH");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        Bus { child, address: line.trim().to_owned(), config }
    }

    async fn connect(&self) -> Connection {
        connection::Builder::address(self.address.as_str()).unwrap().build().await.unwrap()
    }

    async fn daemon(&self, level: u32) -> Connection {
        connection::Builder::address(self.address.as_str())
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(PATH, Fake { level })
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.config);
    }
}

struct Fake {
    level: u32,
}

#[interface(name = "org.example.Fake")]
impl Fake {
    #[zbus(property)]
    fn level(&self) -> u32 {
        self.level
    }
}

async fn set_level(daemon: &Connection, level: u32) {
    let iface = daemon.object_server().interface::<_, Fake>(PATH).await.unwrap();
    iface.get_mut().await.level = level;
    iface.get().await.level_changed(iface.signal_emitter()).await.unwrap();
}

/// The fake's `Level`, read whole on every settled burst.
struct Level<const QUIET_MS: u64> {
    conn: Connection,
}

/// A restart's two owner changes land in one burst.
type Settled = Level<500>;
/// A read after every signal, while the flood is still arriving.
type Eager = Level<0>;

impl<const QUIET_MS: u64> Watched for Level<QUIET_MS> {
    type State = u32;
    type View = u32;
    const SERVICE: &'static str = SERVICE;
    const QUIET: Duration = Duration::from_millis(QUIET_MS);

    fn rule(&self) -> zbus::Result<MatchRule<'static>> {
        Ok(MatchRule::builder().msg_type(Type::Signal).sender(SERVICE)?.build())
    }

    async fn read(&self) -> zbus::Result<u32> {
        let reply = self
            .conn
            .call_method(Some(SERVICE), PATH, Some("org.freedesktop.DBus.Properties"), "Get", &(IFACE, "Level"))
            .await?;
        let value: OwnedValue = reply.body().deserialize()?;
        Ok(u32::try_from(value)?)
    }

    fn view(&self, state: &u32) -> u32 {
        *state
    }
}

async fn within<T>(what: &str, fut: impl Future<Output = T>) -> T {
    future::or(async { Some(fut.await) }, async {
        Timer::after(Duration::from_secs(10)).await;
        None
    })
    .await
    .unwrap_or_else(|| panic!("timed out waiting for {what}"))
}

async fn next(what: &str, stream: &mut (impl Stream<Item = Option<u32>> + Unpin)) -> Option<u32> {
    within(what, stream.next()).await.unwrap_or_else(|| panic!("stream ended waiting for {what}"))
}

#[test]
fn a_daemon_absent_then_up_then_restarted_reads_whole_each_time() {
    let bus = Bus::start("restart");
    block_on(async {
        let client = bus.connect().await;
        let mut levels = Box::pin(watch(client.clone(), Settled { conn: client }));
        assert_eq!(next("the absent daemon", &mut levels).await, None);

        let daemon = bus.daemon(3).await;
        assert_eq!(next("the daemon's arrival", &mut levels).await, Some(3));
        set_level(&daemon, 4).await;
        assert_eq!(next("the change", &mut levels).await, Some(4));

        daemon.close().await.unwrap();
        assert_eq!(next("the daemon leaving", &mut levels).await, None);

        let daemon = bus.daemon(7).await;
        assert_eq!(next("the new daemon", &mut levels).await, Some(7));
        drop(daemon);
    });
}

#[test]
fn a_restart_inside_one_burst_with_the_same_state_hands_out_nothing() {
    let bus = Bus::start("same");
    block_on(async {
        let client = bus.connect().await;
        let daemon = bus.daemon(5).await;
        let mut levels = Box::pin(watch(client.clone(), Settled { conn: client }));
        assert_eq!(next("the first read", &mut levels).await, Some(5));

        daemon.close().await.unwrap();
        let daemon = bus.daemon(5).await;
        set_level(&daemon, 6).await;
        assert_eq!(next("the change after the restart", &mut levels).await, Some(6));
    });
}

#[test]
fn a_flood_of_signals_during_reads_does_not_wedge_the_connection() {
    let bus = Bus::start("flood");
    block_on(async {
        let client = bus.connect().await;
        let daemon = bus.daemon(0).await;
        let mut levels = Box::pin(watch(client.clone(), Eager { conn: client }));
        assert_eq!(next("the first read", &mut levels).await, Some(0));
        for level in 1..=300 {
            set_level(&daemon, level).await;
        }
        let mut last = None;
        while last != Some(300) {
            last = next("the last level", &mut levels).await;
        }
    });
}
