//! Runs against a private session bus the test starts itself, with a fake
//! player served by zbus.

use std::collections::HashMap;
use std::future::Future;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_io::Timer;
use fs_mpris::{LoopStatus, Mpris, PlaybackStatus};
use futures_util::future::{Either, select};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{Connection, connection, interface};

const PATH: &str = "/org/mpris/MediaPlayer2";
const TRACK: &str = "/fake/track/1";

struct Daemon {
    child: Child,
    address: String,
    config: PathBuf,
}

/// Minimal self-contained config: the distro's session.conf is not reliably
/// found from a build sandbox.
const CONFIG: &str = r#"<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>"#;

impl Daemon {
    fn start() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let config = std::env::temp_dir().join(format!(
            "fs-mpris-bus-{}-{}.conf",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&config, CONFIG).unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg("--config-file")
            .arg(&config)
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon on PATH");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        Daemon { child, address: line.trim().to_owned(), config }
    }

    async fn connect(&self) -> Connection {
        connection::Builder::address(self.address.as_str()).unwrap().build().await.unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.config);
    }
}

type Log = Arc<Mutex<Vec<String>>>;

struct Root {
    log: Log,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.log.lock().unwrap().push("Raise".into());
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        "Fake Player".into()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "fake".into()
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
}

struct Player {
    log: Log,
    status: String,
    title: String,
    with_track_id: bool,
    shuffle: bool,
    loop_status: String,
    volume: f64,
    rate: f64,
    position: i64,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play_pause(&self) {
        self.log.lock().unwrap().push("PlayPause".into());
    }

    fn next(&self) {
        self.log.lock().unwrap().push("Next".into());
    }

    fn previous(&self) {
        self.log.lock().unwrap().push("Previous".into());
    }

    fn seek(&self, offset: i64) {
        self.log.lock().unwrap().push(format!("Seek {offset}"));
    }

    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        self.log.lock().unwrap().push(format!("SetPosition {track} {position}"));
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut m: HashMap<String, OwnedValue> = HashMap::new();
        let mut put = |k: &str, v: Value<'_>| {
            m.insert(k.to_owned(), v.try_to_owned().unwrap());
        };
        if self.with_track_id {
            put("mpris:trackid", Value::from(ObjectPath::try_from(TRACK).unwrap()));
        }
        put("mpris:length", Value::from(200_000_000_i64));
        put("mpris:artUrl", Value::from("file:///cover.png"));
        put("xesam:title", Value::from(self.title.as_str()));
        put("xesam:album", Value::from("Album"));
        put("xesam:artist", Value::from(vec!["A".to_owned(), "B".to_owned()]));
        m
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.rate
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&mut self, v: bool) {
        self.shuffle = v;
        self.log.lock().unwrap().push(format!("Shuffle {v}"));
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        self.loop_status.clone()
    }

    #[zbus(property)]
    fn set_loop_status(&mut self, v: String) {
        self.log.lock().unwrap().push(format!("Loop {v}"));
        self.loop_status = v;
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.volume
    }

    #[zbus(property)]
    fn set_volume(&mut self, v: f64) {
        self.volume = v;
        self.log.lock().unwrap().push(format!("Volume {v}"));
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.position
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

struct Fake {
    conn: Connection,
    log: Log,
}

impl Fake {
    async fn start(d: &Daemon, suffix: &str, status: &str, with_track_id: bool) -> Fake {
        let log: Log = Arc::default();
        let conn = connection::Builder::address(d.address.as_str())
            .unwrap()
            .name(format!("org.mpris.MediaPlayer2.{suffix}"))
            .unwrap()
            .serve_at(PATH, Root { log: log.clone() })
            .unwrap()
            .serve_at(
                PATH,
                Player {
                    log: log.clone(),
                    status: status.into(),
                    title: "Song".into(),
                    with_track_id,
                    shuffle: false,
                    loop_status: "None".into(),
                    volume: 0.5,
                    rate: 1.0,
                    position: 10_000_000,
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        Fake { conn, log }
    }

    async fn player(&self) -> zbus::object_server::InterfaceRef<Player> {
        self.conn.object_server().interface::<_, Player>(PATH).await.unwrap()
    }

    fn calls(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

fn run<F: Future>(f: F) -> F::Output {
    async_io::block_on(async {
        match select(Box::pin(f), Timer::after(Duration::from_secs(20))).await {
            Either::Left((v, _)) => v,
            Either::Right(_) => panic!("test timed out"),
        }
    })
}

async fn wait_for(m: &mut Mpris, pred: impl Fn(&Mpris) -> bool) {
    while !pred(m) {
        m.next().await.expect("bus closed");
    }
}

fn bus(suffix: &str) -> String {
    format!("org.mpris.MediaPlayer2.{suffix}")
}

#[test]
fn discovers_players_already_on_the_bus() {
    let d = Daemon::start();
    run(async {
        let _fake = Fake::start(&d, "one", "Playing", true).await;
        let m = Mpris::connect(&d.connect().await).await.unwrap();
        assert_eq!(m.players().count(), 1);
        let s = m.player(&bus("one")).unwrap();
        assert_eq!(s.identity, "Fake Player");
        assert_eq!(s.desktop_entry.as_deref(), Some("fake"));
        assert_eq!(s.status, PlaybackStatus::Playing);
        assert_eq!(s.metadata.title, "Song");
        assert_eq!(s.metadata.artist(), "A, B");
        assert_eq!(s.metadata.art_url, "file:///cover.png");
        assert_eq!(s.metadata.track_id.as_deref(), Some(TRACK));
        assert_eq!(s.metadata.length_us, Some(200_000_000));
        assert_eq!(s.shuffle, Some(false));
        assert_eq!(s.loop_status, Some(LoopStatus::None));
        assert_eq!(s.volume, Some(0.5));
        assert!(s.can_go_next && !s.can_go_previous && s.can_seek_now() && s.can_raise);
        assert!(s.position_supported);
    });
}

#[test]
fn player_appears_and_vanishes() {
    let d = Daemon::start();
    run(async {
        let mut m = Mpris::connect(&d.connect().await).await.unwrap();
        assert_eq!(m.players().count(), 0);
        let fake = Fake::start(&d, "late", "Paused", true).await;
        wait_for(&mut m, |m| m.player(&bus("late")).is_some()).await;
        assert_eq!(m.players().count(), 1);
        fake.conn.graceful_shutdown().await;
        wait_for(&mut m, |m| m.player(&bus("late")).is_none()).await;
        assert_eq!(m.players().count(), 0);
    });
}

#[test]
fn property_changes_are_reread() {
    let d = Daemon::start();
    run(async {
        let fake = Fake::start(&d, "props", "Playing", true).await;
        let mut m = Mpris::connect(&d.connect().await).await.unwrap();
        wait_for(&mut m, |m| m.players().count() == 1).await;

        let iface = fake.player().await;
        {
            let mut p = iface.get_mut().await;
            p.status = "Paused".into();
            p.title = "Other".into();
            p.position = 77_000_000;
            p.playback_status_changed(iface.signal_emitter()).await.unwrap();
            p.metadata_changed(iface.signal_emitter()).await.unwrap();
        }
        wait_for(&mut m, |m| m.player(&bus("props")).unwrap().metadata.title == "Other").await;
        let s = m.player(&bus("props")).unwrap();
        assert_eq!(s.status, PlaybackStatus::Paused);
        assert_eq!(s.position(Instant::now() + Duration::from_secs(5)), Duration::from_secs(77));
    });
}

#[test]
fn position_interpolates_without_polling_and_seeked_resets_it() {
    let d = Daemon::start();
    run(async {
        let fake = Fake::start(&d, "pos", "Playing", true).await;
        let mut m = Mpris::connect(&d.connect().await).await.unwrap();
        wait_for(&mut m, |m| m.players().count() == 1).await;

        let s = m.player(&bus("pos")).unwrap();
        let later = s.position(Instant::now() + Duration::from_secs(3));
        assert!(later >= Duration::from_secs(13) && later < Duration::from_secs(14), "{later:?}");

        let iface = fake.player().await;
        Player::seeked(iface.signal_emitter(), 42_000_000).await.unwrap();
        wait_for(&mut m, |m| m.player(&bus("pos")).unwrap().position(Instant::now()) >= Duration::from_secs(42)).await;
        let p = m.player(&bus("pos")).unwrap().position(Instant::now());
        assert!(p < Duration::from_secs(43), "{p:?}");
    });
}

#[test]
fn method_calls_land_on_the_fake() {
    let d = Daemon::start();
    run(async {
        let fake = Fake::start(&d, "calls", "Playing", true).await;
        let mut m = Mpris::connect(&d.connect().await).await.unwrap();
        wait_for(&mut m, |m| m.players().count() == 1).await;
        let c = m.controls();
        let n = bus("calls");

        c.play_pause(&n).await.unwrap();
        c.next(&n).await.unwrap();
        c.previous(&n).await.unwrap();
        c.seek(&n, -5_000_000).await.unwrap();
        c.raise(&n).await.unwrap();
        c.set_shuffle(&n, true).await.unwrap();
        c.set_loop_status(&n, LoopStatus::Playlist).await.unwrap();
        c.set_volume(&n, 0.25).await.unwrap();

        assert_eq!(
            fake.calls(),
            [
                "PlayPause",
                "Next",
                "Previous",
                "Seek -5000000",
                "Raise",
                "Shuffle true",
                "Loop Playlist",
                "Volume 0.25",
            ]
        );
    });
}
