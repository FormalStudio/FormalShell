//! The geoclue path: a GeoClue2 client under the desktop id the NixOS module
//! allowlists, left started so a later fix replaces an early inaccurate one
//! (`updateInterval` 60 s). A machine with no GeoClue, or one that will not talk to this
//! client, ends the task and leaves the beaconDB lookup to place it.

use futures_lite::StreamExt;
use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

/// `AccuracyLevel` EXACT, what QtPositioning asks of every method.
const EXACT: u32 = 8;
/// Seconds between fixes the client reports.
const TIME_THRESHOLD: u32 = 60;

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

    #[zbus(property)]
    fn set_desktop_id(&self, id: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_requested_accuracy_level(&self, level: u32) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_time_threshold(&self, seconds: u32) -> zbus::Result<()>;
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
}

async fn fix(conn: &zbus::Connection, path: &OwnedObjectPath) -> Option<(f64, f64)> {
    if path.as_str() == "/" {
        return None;
    }
    let location = LocationProxy::builder(conn).path(path.clone()).ok()?.build().await.ok()?;
    let (lat, lon) = (location.latitude().await.ok()?, location.longitude().await.ok()?);
    (lat.is_finite() && lon.is_finite()).then_some((lat, lon))
}

/// Sends each fix GeoClue reports until the bus goes away or the receiver does.
pub async fn run(fixes: async_channel::Sender<(f64, f64)>) {
    let Ok(conn) = crate::services::bus::system().await else { return };
    let Ok(manager) = ManagerProxy::new(&conn).await else { return };
    let Ok(path) = manager.get_client().await else { return };
    let Ok(builder) = ClientProxy::builder(&conn).path(path) else { return };
    let Ok(client) = builder.build().await else { return };
    // GeoClue refuses a client with no desktop id, so these come first.
    if client.set_desktop_id("formalshell").await.is_err() {
        return;
    }
    let _ = client.set_requested_accuracy_level(EXACT).await;
    let _ = client.set_time_threshold(TIME_THRESHOLD).await;
    let Ok(mut updates) = client.receive_location_updated().await else { return };
    if client.start().await.is_err() {
        return;
    }
    if let Some(at) = client.location().await.ok().as_ref().map(|p| fix(&conn, p)) {
        if let Some(at) = at.await {
            let _ = fixes.send(at).await;
        }
    }
    while let Some(update) = updates.next().await {
        let Ok(args) = update.args() else { continue };
        if let Some(at) = fix(&conn, &args.new).await {
            if fixes.send(at).await.is_err() {
                return;
            }
        }
    }
}
