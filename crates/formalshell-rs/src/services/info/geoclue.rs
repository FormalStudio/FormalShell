//! The geoclue path: a GeoClue2 client under the desktop id the system's GeoClue
//! config allowlists. A machine with no GeoClue, or one that will not talk to this
//! client, ends the task and leaves the beaconDB lookup to place it.
//!
//! The client is started only long enough to be placed, then stopped until
//! AGAIN has passed or the machine has slept. A started client keeps
//! GeoClue scanning Wi-Fi: 10 s after each scan at STREET accuracy or finer
//! and 300 s at CITY (gclue-wifi.c's `WIFI_SCAN_TIMEOUT_*`), and each scan
//! takes the radio off its channel in 110 ms hops for about 3 s, which
//! anything live over that Wi-Fi shows as a stall.

use std::time::{Duration, Instant, SystemTime};

use fs_info::location::COARSE_METRES;
use futures_lite::StreamExt;
use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

/// `AccuracyLevel` CITY: a forecast needs no finer, and GeoClue adds 3 km
/// to what it reports at this level.
const CITY: u32 = 4;
/// How long one start waits for a fix finer than COARSE_METRES.
const LISTEN: Duration = Duration::from_secs(60);
/// A placed machine is placed again this often.
const AGAIN: Duration = Duration::from_secs(3600);

#[proxy(
    interface = "org.freedesktop.GeoClue2.Manager",
    default_service = "org.freedesktop.GeoClue2",
    default_path = "/org/freedesktop/GeoClue2/Manager",
    gen_blocking = false
)]
trait Manager {
    fn get_client(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(interface = "org.freedesktop.GeoClue2.Client", default_service = "org.freedesktop.GeoClue2", gen_blocking = false)]
trait Client {
    fn start(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn set_desktop_id(&self, id: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_requested_accuracy_level(&self, level: u32) -> zbus::Result<()>;
    #[zbus(property)]
    fn location(&self) -> zbus::Result<OwnedObjectPath>;

    #[zbus(signal)]
    fn location_updated(&self, old: OwnedObjectPath, new: OwnedObjectPath) -> zbus::Result<()>;
}

#[proxy(interface = "org.freedesktop.GeoClue2.Location", default_service = "org.freedesktop.GeoClue2", gen_blocking = false)]
trait Location {
    #[zbus(property)]
    fn latitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn longitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn accuracy(&self) -> zbus::Result<f64>;
}

/// Where, and how many metres either way.
pub type GeoFix = ((f64, f64), f64);

async fn fix(conn: &zbus::Connection, path: &OwnedObjectPath) -> Option<GeoFix> {
    if path.as_str() == "/" {
        return None;
    }
    let location = LocationProxy::builder(conn).path(path.clone()).ok()?.build().await.ok()?;
    let (lat, lon) = (location.latitude().await.ok()?, location.longitude().await.ok()?);
    // A source that never says how close it is counts as coarse.
    let accuracy = location.accuracy().await.ok().filter(|a| a.is_finite()).unwrap_or(f64::INFINITY);
    (lat.is_finite() && lon.is_finite()).then_some(((lat, lon), accuracy))
}

/// Until AGAIN has passed, or the wall clock has run ahead of the
/// monotonic one, which stops while the machine sleeps: it may have woken
/// somewhere else.
async fn pause() {
    let (began, wall) = (Instant::now(), SystemTime::now());
    loop {
        async_io::Timer::after(Duration::from_secs(300)).await;
        let asleep = wall.elapsed().unwrap_or_default().saturating_sub(began.elapsed());
        if began.elapsed() >= AGAIN || asleep > Duration::from_secs(120) {
            return;
        }
    }
}

/// Sends each fix GeoClue reports until the bus goes away or the receiver does.
pub async fn run(fixes: async_channel::Sender<GeoFix>) {
    let Ok(conn) = crate::services::bus::system().await else { return };
    let Ok(manager) = ManagerProxy::new(&conn).await else { return };
    let Ok(path) = manager.get_client().await else { return };
    let Ok(builder) = ClientProxy::builder(&conn).path(path) else { return };
    let Ok(client) = builder.build().await else { return };
    // GeoClue refuses a client with no desktop id, so these come first.
    if client.set_desktop_id("formalshell").await.is_err() {
        return;
    }
    let _ = client.set_requested_accuracy_level(CITY).await;
    let Ok(mut updates) = client.receive_location_updated().await else { return };
    loop {
        if client.start().await.is_err() {
            return;
        }
        // Whether the receiver is still there once the listening ends.
        let listen = async {
            let mut at = match client.location().await {
                Ok(path) => fix(&conn, &path).await,
                Err(_) => None,
            };
            loop {
                if let Some(at) = at {
                    if fixes.send(at).await.is_err() {
                        return false;
                    }
                    if at.1 < COARSE_METRES {
                        return true;
                    }
                }
                let Some(update) = updates.next().await else { return false };
                at = match update.args() {
                    Ok(args) => fix(&conn, &args.new).await,
                    Err(_) => None,
                };
            }
        };
        let wanted = futures_lite::future::or(listen, async {
            async_io::Timer::after(LISTEN).await;
            true
        })
        .await;
        let _ = client.stop().await;
        if !wanted {
            return;
        }
        pause().await;
    }
}
