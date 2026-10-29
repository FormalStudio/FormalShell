use std::collections::HashMap;
use std::future::Future;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use async_channel::Receiver;
use fs_notifd::{CloseReason, Config, Error, Event, Image, Server, Urgency};
use futures_lite::StreamExt;
use zbus::connection::Builder;
use zbus::zvariant::Value;
use zbus::{Connection, proxy};

/// A private session bus; the host's is never touched.
struct Bus {
    child: Child,
    address: String,
}

impl Bus {
    fn start() -> Bus {
        // Not `--session`: that reads a config path baked into the package,
        // which a sandboxed build may not have.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir();
        let conf = dir.join(format!("fs-notifd-bus-{}-{}.conf", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::write(
            &conf,
            format!(
                "<busconfig><type>session</type><listen>unix:tmpdir={}</listen><auth>EXTERNAL</auth>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
                dir.display()
            ),
        )
        .unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg("--config-file")
            .arg(&conf)
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon on PATH");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
        let _ = std::fs::remove_file(&conf);
        if !address.starts_with("unix:") {
            let status = child.wait().unwrap();
            panic!("dbus-daemon printed {address:?} and exited with {status}");
        }
        Bus { child, address: address.trim().to_owned() }
    }

    fn builder(&self) -> Builder<'static> {
        Builder::address(self.address.as_str()).unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Client {
    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;
    fn get_server_information(&self) -> zbus::Result<(String, String, String, String)>;
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
    fn close_notification(&self, id: u32) -> zbus::Result<()>;
    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn activation_token(&self, id: u32, activation_token: &str) -> zbus::Result<()>;
}

struct Rig {
    _bus: Bus,
    server: Server,
    events: Receiver<Event>,
    client: Connection,
}

impl Rig {
    async fn new() -> Rig {
        let bus = Bus::start();
        let (server, events) = Server::start_on(bus.builder(), Config::default()).await.unwrap();
        let client = bus.builder().build().await.unwrap();
        Rig { _bus: bus, server, events, client }
    }

    async fn proxy(&self) -> ClientProxy<'_> {
        ClientProxy::new(&self.client).await.unwrap()
    }
}

fn run<T>(f: impl Future<Output = T>) -> T {
    async_io::block_on(f)
}

async fn within<T>(f: impl Future<Output = T>) -> T {
    futures_lite::future::or(f, async {
        async_io::Timer::after(Duration::from_secs(5)).await;
        panic!("timed out");
    })
    .await
}

async fn quiet<T>(f: impl Future<Output = T>) -> bool {
    futures_lite::future::or(async { f.await; false }, async {
        async_io::Timer::after(Duration::from_millis(300)).await;
        true
    })
    .await
}

async fn notified(events: &Receiver<Event>) -> std::sync::Arc<fs_notifd::Notification> {
    match within(events.recv()).await.unwrap() {
        Event::Notified(n) => n,
        other => panic!("expected Notified, got {other:?}"),
    }
}

async fn replaced(events: &Receiver<Event>) -> std::sync::Arc<fs_notifd::Notification> {
    match within(events.recv()).await.unwrap() {
        Event::Replaced(n) => n,
        other => panic!("expected Replaced, got {other:?}"),
    }
}

fn hints<'a>(pairs: Vec<(&'a str, Value<'a>)>) -> HashMap<&'a str, Value<'a>> {
    pairs.into_iter().collect()
}

#[test]
fn identity_and_capabilities_hold() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        assert_eq!(
            p.get_server_information().await.unwrap(),
            ("formalshell".into(), "FormalShell".into(), env!("CARGO_PKG_VERSION").into(), "1.2".into())
        );
        assert_eq!(p.get_capabilities().await.unwrap(), ["persistence", "body", "actions", "icon-static"]);
    });
}

#[test]
fn notify_send_shaped_call() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        let id = p
            .notify(
                "notify-send",
                0,
                "dialog-information",
                "Summary",
                "Body & <b>text</b>",
                &["default", "Open", "mute", "Mute"],
                hints(vec![
                    ("urgency", Value::U8(2)),
                    ("category", Value::from("email.arrived")),
                    ("desktop-entry", Value::from("thunderbird")),
                    ("transient", Value::from(true)),
                    ("resident", Value::from(true)),
                    ("sound-name", Value::from("message-new-email")),
                    ("sound-file", Value::from("/tmp/ding.oga")),
                    ("suppress-sound", Value::from(false)),
                    ("x-canonical-private-synchronous", Value::from("vol")),
                    ("x-custom", Value::from(7i32)),
                    ("value", Value::from(140i32)),
                ]),
                -1,
            )
            .await
            .unwrap();
        assert_eq!(id, 1);
        let n = notified(&rig.events).await;
        assert_eq!((n.id, n.app_name.as_str(), n.app_icon.as_str()), (1, "notify-send", "dialog-information"));
        assert_eq!((n.summary.as_str(), n.body.as_str()), ("Summary", "Body & <b>text</b>"));
        assert_eq!(n.urgency, Urgency::Critical);
        assert_eq!(n.category.as_deref(), Some("email.arrived"));
        assert_eq!(n.desktop_entry, "thunderbird");
        assert!(n.transient && n.resident && !n.suppress_sound);
        assert_eq!(n.sound_name.as_deref(), Some("message-new-email"));
        assert_eq!(n.sound_file.as_deref(), Some("/tmp/ding.oga"));
        assert_eq!(n.synchronous.as_deref(), Some("vol"));
        assert_eq!(n.expire_timeout, -1);
        assert_eq!(n.actions.len(), 2);
        assert_eq!((n.actions[1].key.as_str(), n.actions[1].label.as_str()), ("mute", "Mute"));
        assert!(n.hints.contains_key("x-custom"));
        assert_eq!(n.value.map(|v| v.get()), Some(100));

        let id2 = p.notify("a", 0, "", "s", "", &[], hints(vec![]), 0).await.unwrap();
        assert_eq!(id2, 2);
        let n2 = notified(&rig.events).await;
        assert_eq!(n2.urgency, Urgency::Normal);
        assert!(n2.category.is_none() && n2.image.is_none() && n2.value.is_none());
        assert_eq!(rig.server.notifications().len(), 2);
    });
}

#[test]
fn odd_action_list_is_dropped() {
    run(async {
        let rig = Rig::new().await;
        rig.proxy().await.notify("a", 0, "", "s", "", &["only-key"], hints(vec![]), 0).await.unwrap();
        assert!(notified(&rig.events).await.actions.is_empty());
    });
}

#[test]
fn replaces_id_updates_in_place() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        let id = p.notify("app", 0, "", "Song A", "", &[], hints(vec![]), 0).await.unwrap();
        notified(&rig.events).await;

        let again = p.notify("app", id, "", "Song B", "", &["k", "K"], hints(vec![]), 0).await.unwrap();
        assert_eq!(again, id);
        let n = replaced(&rig.events).await;
        assert_eq!((n.id, n.summary.as_str(), n.actions.len()), (id, "Song B", 1));
        assert_eq!(rig.server.notifications().len(), 1);
        assert_eq!(rig.server.get(id).unwrap().summary, "Song B");

        // An id that is no longer live is not reused.
        let stale = p.notify("app", 99, "", "new", "", &[], hints(vec![]), 0).await.unwrap();
        assert_ne!(stale, 99);
        assert_ne!(stale, id);
        notified(&rig.events).await;
    });
}

#[test]
fn synchronous_tag_replaces_same_app_only() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        let tag = || hints(vec![("x-canonical-private-synchronous", Value::from("volume"))]);
        let id = p.notify("osd", 0, "", "10%", "", &[], tag(), 0).await.unwrap();
        notified(&rig.events).await;
        assert_eq!(p.notify("osd", 0, "", "20%", "", &[], tag(), 0).await.unwrap(), id);
        assert_eq!(replaced(&rig.events).await.summary, "20%");
        let other = p.notify("other", 0, "", "x", "", &[], tag(), 0).await.unwrap();
        assert_ne!(other, id);
        notified(&rig.events).await;
    });
}

#[test]
fn close_reasons() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        let mut closed = p.receive_notification_closed().await.unwrap();
        let mk = |s: &'static str| p.notify("a", 0, "", s, "", &[], HashMap::new(), 0);

        let (a, b, c) = (mk("a").await.unwrap(), mk("b").await.unwrap(), mk("c").await.unwrap());
        for _ in 0..3 {
            notified(&rig.events).await;
        }

        p.close_notification(a).await.unwrap();
        let args = closed_of(within(closed.next()).await.unwrap());
        assert_eq!(args, (a, 3));
        assert!(matches!(within(rig.events.recv()).await.unwrap(), Event::Closed { id } if id == a));

        assert!(rig.server.close(b, CloseReason::Expired).await.unwrap());
        let args = closed_of(within(closed.next()).await.unwrap());
        assert_eq!(args, (b, 1));

        assert!(rig.server.close(c, CloseReason::Dismissed).await.unwrap());
        let args = closed_of(within(closed.next()).await.unwrap());
        assert_eq!(args, (c, 2));

        // Owner-initiated closes are not repeated as events, and a dead id is silent.
        assert!(!rig.server.close(c, CloseReason::Dismissed).await.unwrap());
        p.close_notification(c).await.unwrap();
        assert!(quiet(closed.next()).await);
        assert!(quiet(rig.events.recv()).await);
        assert!(rig.server.notifications().is_empty());
    });
}

#[test]
fn actions_round_trip() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        let mut invoked = p.receive_action_invoked().await.unwrap();
        let mut token = p.receive_activation_token().await.unwrap();
        let mut closed = p.receive_notification_closed().await.unwrap();

        let id = p.notify("a", 0, "", "s", "", &["default", "Open", "x", "X"], hints(vec![]), 0).await.unwrap();
        notified(&rig.events).await;

        assert!(matches!(
            rig.server.invoke_action(id, "nope", None).await,
            Err(Error::NoSuchAction { .. })
        ));
        assert!(matches!(
            rig.server.invoke_action(id + 5, "default", None).await,
            Err(Error::NoSuchNotification(_))
        ));

        assert!(rig.server.invoke_action(id, "default", Some("tok-1")).await.unwrap());
        let t = token_of(within(token.next()).await.unwrap());
        assert_eq!(t, (id, "tok-1".to_owned()));
        let a = invoked_of(within(invoked.next()).await.unwrap());
        assert_eq!(a, (id, "default".to_owned()));
        let c = closed_of(within(closed.next()).await.unwrap());
        assert_eq!(c, (id, 2));
        assert!(rig.server.get(id).is_none());
    });
}

#[test]
fn resident_notification_survives_its_action() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        let mut invoked = p.receive_action_invoked().await.unwrap();
        let mut closed = p.receive_notification_closed().await.unwrap();
        let id = p
            .notify("a", 0, "", "s", "", &["k", "K"], hints(vec![("resident", Value::from(true))]), 0)
            .await
            .unwrap();
        notified(&rig.events).await;

        assert!(!rig.server.invoke_action(id, "k", None).await.unwrap());
        within(invoked.next()).await.unwrap();
        assert!(quiet(closed.next()).await);
        assert!(rig.server.get(id).is_some());
    });
}

fn pixels(w: i32, h: i32, stride: i32, alpha: bool, data: Vec<u8>) -> Value<'static> {
    Value::from((w, h, stride, alpha, 8i32, if alpha { 4i32 } else { 3 }, data))
}

#[test]
fn image_hints() {
    run(async {
        let rig = Rig::new().await;
        let p = rig.proxy().await;
        let send = |h: Vec<(&'static str, Value<'static>)>| async {
            p.notify("a", 0, "", "s", "", &[], hints(h), 0).await.unwrap();
            notified(&rig.events).await
        };

        // 2x2 RGB with two bytes of row padding becomes tightly packed opaque RGBA.
        let rgb = vec![1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10, 11, 12, 0, 0];
        let n = send(vec![("image-data", pixels(2, 2, 8, false, rgb))]).await;
        let Some(Image::Data(d)) = &n.image else { panic!("{:?}", n.image) };
        assert_eq!((d.width, d.height), (2, 2));
        assert_eq!(d.rgba, [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]);
        assert!(!n.hints.contains_key("image-data"));

        let n = send(vec![("image_data", pixels(1, 1, 4, true, vec![9, 8, 7, 6]))]).await;
        assert_eq!(n.image, Some(Image::Data(fs_notifd::ImageData { width: 1, height: 1, rgba: vec![9, 8, 7, 6] })));

        let n = send(vec![("icon_data", pixels(1, 1, 3, false, vec![1, 2, 3]))]).await;
        assert!(matches!(n.image, Some(Image::Data(_))));

        // Short buffer: the pixels are refused and image-path is used instead.
        let n = send(vec![
            ("image-data", pixels(4, 4, 12, false, vec![0; 10])),
            ("image-path", Value::from("file:///tmp/x.png")),
        ])
        .await;
        assert_eq!(n.image, Some(Image::Path("/tmp/x.png".into())));

        let n = send(vec![("image_path", Value::from("firefox"))]).await;
        assert_eq!(n.image, Some(Image::Path("firefox".into())));

        // Data wins over a path.
        let n = send(vec![
            ("image-path", Value::from("firefox")),
            ("image-data", pixels(1, 1, 3, false, vec![1, 2, 3])),
        ])
        .await;
        assert!(matches!(n.image, Some(Image::Data(_))));
    });
}

#[test]
fn name_already_owned_is_an_error_and_not_stolen() {
    run(async {
        let rig = Rig::new().await;
        let other = Config { name: "second".into(), ..Config::default() };
        let err = Server::start_on(rig._bus.builder(), other).await.err().expect("second server must fail");
        assert!(matches!(err, Error::NameTaken), "{err:?}");

        let p = rig.proxy().await;
        assert_eq!(p.get_server_information().await.unwrap().0, "formalshell");
        p.notify("a", 0, "", "s", "", &[], hints(vec![]), 0).await.unwrap();
        notified(&rig.events).await;
    });
}

#[test]
fn name_owned_by_a_plain_connection() {
    run(async {
        let bus = Bus::start();
        let squatter = bus.builder().name("org.freedesktop.Notifications").unwrap().build().await.unwrap();
        let err = Server::start_on(bus.builder(), Config::default()).await.err().expect("must fail");
        assert!(matches!(err, Error::NameTaken), "{err:?}");
        drop(squatter);
    });
}

#[test]
fn real_notify_send() {
    if Command::new("notify-send").arg("--version").output().is_err() {
        eprintln!("notify-send not on PATH, skipping");
        return;
    }
    run(async {
        let rig = Rig::new().await;
        let out = Command::new("notify-send")
            .env("DBUS_SESSION_BUS_ADDRESS", &rig._bus.address)
            .args(["-a", "myapp", "-u", "low", "-i", "mail-unread", "-c", "email.arrived", "-t", "4000"])
            .args(["-h", "string:desktop-entry:org.example.app", "-h", "string:x-canonical-private-synchronous:vol"])
            .args(["-h", "boolean:transient:true", "-h", "int:value:40", "-A", "reply=Reply", "Hello", "World"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let n = notified(&rig.events).await;
        assert_eq!(n.app_name, "myapp");
        // libnotify 0.8 sends -i as the image-path hint, older ones as app_icon.
        assert!(n.app_icon == "mail-unread" || n.image == Some(Image::Path("mail-unread".into())));
        assert_eq!((n.summary.as_str(), n.body.as_str()), ("Hello", "World"));
        assert_eq!(n.urgency, Urgency::Low);
        assert_eq!(n.category.as_deref(), Some("email.arrived"));
        assert_eq!(n.desktop_entry, "org.example.app");
        assert!(n.transient);
        assert_eq!(n.expire_timeout, 4000);
        assert_eq!(n.synchronous.as_deref(), Some("vol"));
        assert_eq!(n.value.map(|v| v.get()), Some(40));
    });
}

fn closed_of(s: NotificationClosed) -> (u32, u32) {
    let a = s.args().unwrap();
    (a.id, a.reason)
}

fn token_of(s: ActivationToken) -> (u32, String) {
    let a = s.args().unwrap();
    (a.id, a.activation_token.to_owned())
}

fn invoked_of(s: ActionInvoked) -> (u32, String) {
    let a = s.args().unwrap();
    (a.id, a.action_key.to_owned())
}
