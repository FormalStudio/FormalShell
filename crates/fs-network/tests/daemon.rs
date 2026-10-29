//! Needs NetworkManager and the hwsim radios, so the test is ignored under a
//! plain `cargo test`. The VM check (`dev/check.sh fs-network`) runs it
//! with `--ignored`: wlan0 is NetworkManager's station, wlan1 and wlan2 are the
//! hostapd radios serving FORMALTEST (WPA2-PSK) and FORMALTEST-EAP (PEAP with
//! MSCHAPv2), the same topology dev/smoke.d/wifi.sh drives through the shell.
//!
//! One test, not several: the steps share one station and one NetworkManager
//! profile store, and the order is the point (wrong password, connect,
//! forget, enterprise).

use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use fs_network::{
    ConnectError, ConnectionState, DeviceKind, FailReason, NetworkManager, Security, Snapshot,
    WifiNetwork,
};
use futures_lite::{Stream, StreamExt, future};
use zbus::Connection;

const PSK_SSID: &str = "FORMALTEST";
const EAP_SSID: &str = "FORMALTEST-EAP";

async fn timed<T>(what: &str, limit: Duration, fut: impl Future<Output = T>) -> T {
    future::or(async { Some(fut.await) }, async {
        Timer::after(limit).await;
        None
    })
    .await
    .unwrap_or_else(|| panic!("timed out waiting for {what}"))
}

fn network<'a>(snapshot: &'a Snapshot, ssid: &str) -> Option<&'a WifiNetwork> {
    snapshot.networks.iter().find(|n| n.ssid == ssid)
}

/// Polls until `done` holds, asking for a scan on every round.
async fn until(
    nm: &NetworkManager,
    what: &str,
    limit: Duration,
    done: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    timed(what, limit, async {
        loop {
            let _ = nm.request_scan().await;
            let snapshot = nm.snapshot().await.unwrap();
            if done(&snapshot) {
                return snapshot;
            }
            Timer::after(Duration::from_millis(500)).await;
        }
    })
    .await
}

/// What the change stream has delivered, drained on its own thread: zbus
/// stops dispatching to the whole connection once a stream's queue is full,
/// so a stream nobody polls would freeze the calls the test makes.
struct Seen(std::sync::mpsc::Receiver<Snapshot>);

impl Seen {
    fn spawn(mut changes: impl Stream<Item = Snapshot> + Unpin + Send + 'static) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            block_on(async {
                while let Some(snapshot) = changes.next().await {
                    if tx.send(snapshot).is_err() {
                        break;
                    }
                }
            })
        });
        Self(rx)
    }

    fn saw(&self, what: &str, pred: impl Fn(&Snapshot) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            match self.0.recv_timeout(left) {
                Ok(snapshot) if pred(&snapshot) => return,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        panic!("timed out waiting for {what}");
    }
}

#[test]
#[ignore = "needs NetworkManager and hwsim radios"]
fn wifi_round_trip_against_real_networkmanager() {
    block_on(async {
        let conn = Connection::system().await.unwrap();
        let nm = NetworkManager::connect(&conn).await.unwrap();
        let mut changes = Box::pin(nm.changes().await.unwrap());

        let first = changes.next().await.expect("the initial snapshot");
        let seen = Seen::spawn(changes);
        assert!(first.wifi_enabled);
        let station = first
            .devices
            .iter()
            .find(|d| d.interface == "wlan0")
            .expect("wlan0");
        assert_eq!(station.kind, DeviceKind::Wifi);
        assert!(station.managed());
        for ap_radio in ["wlan1", "wlan2"] {
            let d = first.devices.iter().find(|d| d.interface == ap_radio);
            assert!(d.is_none_or(|d| !d.managed()), "{ap_radio} must stay unmanaged");
        }

        // Scan: both SSIDs, with the unit the type promises.
        eprintln!("step: Scan: both SSIDs, with the unit the type promises.");
        let scanned = until(&nm, "both SSIDs in a scan", Duration::from_secs(60), |s| {
            network(s, PSK_SSID).is_some() && network(s, EAP_SSID).is_some()
        })
        .await;
        let psk = network(&scanned, PSK_SSID).unwrap();
        eprintln!("{psk:?}");
        assert!(psk.visible && !psk.known && !psk.connected());
        assert!(matches!(psk.security, Security::Wpa2Psk | Security::Sae));
        assert!((1..=100).contains(&psk.signal.as_percent()));
        assert!(psk.signal.as_fraction() <= 1.0);
        let eap = network(&scanned, EAP_SSID).unwrap();
        assert!(eap.security.is_enterprise(), "{:?}", eap.security);

        // No profile and no passphrase.
        eprintln!("step: No profile and no passphrase.");
        assert!(matches!(
            nm.connect_saved(PSK_SSID).await,
            Err(ConnectError::SecretsRequired)
        ));
        assert!(matches!(
            nm.connect_psk("no-such-ssid", "whatever").await,
            Err(ConnectError::SsidNotFound)
        ));

        // Wrong passphrase: a typed error, and nothing left saved.
        eprintln!("step: Wrong passphrase: a typed error, and nothing left saved.");
        let wrong = nm.connect_psk(PSK_SSID, "wrong-formaltest-psk").await;
        eprintln!("wrong password: {wrong:?}");
        assert!(matches!(wrong, Err(ConnectError::WrongPassword)), "{wrong:?}");
        let after_wrong = until(&nm, "the failed attempt to settle", Duration::from_secs(45), |s| {
            network(s, PSK_SSID).is_some_and(|n| !n.state.is_changing())
        })
        .await;
        let n = network(&after_wrong, PSK_SSID).unwrap();
        assert!(!n.connected() && !n.known, "{n:?}");

        // The right one.
        eprintln!("step: The right one.");
        nm.connect_psk(PSK_SSID, "formaltest-psk").await.unwrap();
        let joined = nm.snapshot().await.unwrap();
        let n = network(&joined, PSK_SSID).unwrap();
        assert!(n.known && n.connected(), "{n:?}");
        assert!(
            joined
                .devices
                .iter()
                .any(|d| d.interface == "wlan0" && d.connected())
        );
        seen.saw("the stream to report FORMALTEST connected", |s| {
            network(s, PSK_SSID).is_some_and(WifiNetwork::connected)
        });

        // Disconnect keeps the profile; connect_saved uses it.
        eprintln!("step: Disconnect keeps the profile; connect_saved uses it.");
        assert!(nm.disconnect(PSK_SSID).await.unwrap());
        assert!(!nm.disconnect("no-such-ssid").await.unwrap());
        let idle = until(&nm, "FORMALTEST to disconnect", Duration::from_secs(30), |s| {
            network(s, PSK_SSID).is_some_and(|n| n.known && n.state == ConnectionState::Disconnected)
        })
        .await;
        assert!(network(&idle, PSK_SSID).unwrap().known);
        nm.connect_saved(PSK_SSID).await.unwrap();
        assert!(network(&nm.snapshot().await.unwrap(), PSK_SSID).unwrap().connected());

        // Forget.
        eprintln!("step: Forget.");
        assert!(nm.forget(PSK_SSID).await.unwrap());
        assert!(!nm.forget(PSK_SSID).await.unwrap());
        let forgotten = until(&nm, "FORMALTEST to be forgotten", Duration::from_secs(30), |s| {
            network(s, PSK_SSID).is_some_and(|n| !n.known && !n.state.is_changing())
        })
        .await;
        assert!(!network(&forgotten, PSK_SSID).unwrap().connected());

        // Enterprise: a wrong password first, then the real one.
        eprintln!("step: Enterprise: a wrong password first, then the real one.");
        let wrong_eap = nm.connect_eap(EAP_SSID, "formaltest", "wrong-eap-pw").await;
        eprintln!("wrong enterprise password: {wrong_eap:?}");
        assert!(
            matches!(
                wrong_eap,
                Err(ConnectError::WrongPassword
                    | ConnectError::Failed(FailReason::SupplicantFailed))
            ),
            "{wrong_eap:?}"
        );
        nm.forget(EAP_SSID).await.unwrap();
        until(&nm, "the failed enterprise attempt to settle", Duration::from_secs(45), |s| {
            network(s, EAP_SSID).is_some_and(|n| n.visible && !n.state.is_changing())
        })
        .await;

        // wpa_supplicant temp-disables an SSID after an auth failure and the
        // access point drops out of the scan list for about ten seconds.
        // Sometimes its scan then stays pending for good and it never takes
        // the access point off its ignore list (CI, 2026-10-08: every
        // activation for 90s failed with no auth attempt). Cycling the radio
        // drops the supplicant interface and both with it.
        timed("the enterprise connect", Duration::from_secs(120), async {
            loop {
                match nm
                    .connect_eap(EAP_SSID, "formaltest", "formaltest-eap-pw")
                    .await
                {
                    Ok(()) => return,
                    Err(ConnectError::SsidNotFound | ConnectError::Failed(_)) => {
                        let _ = nm.forget(EAP_SSID).await;
                        let _ = nm.set_wifi_enabled(false).await;
                        Timer::after(Duration::from_secs(1)).await;
                        let _ = nm.set_wifi_enabled(true).await;
                        until(&nm, "the enterprise SSID after the radio", Duration::from_secs(30), |s| {
                            s.wifi_enabled && network(s, EAP_SSID).is_some_and(|n| n.visible)
                        })
                        .await;
                    }
                    Err(e) => panic!("enterprise connect: {e:?}"),
                }
            }
        })
        .await;
        let joined = nm.snapshot().await.unwrap();
        let n = network(&joined, EAP_SSID).unwrap();
        assert!(n.known && n.connected(), "{n:?}");
        assert!(nm.forget(EAP_SSID).await.unwrap());
        let gone = until(&nm, "the enterprise profile to go", Duration::from_secs(30), |s| {
            network(s, EAP_SSID).is_some_and(|n| !n.known && !n.state.is_changing())
        })
        .await;
        assert!(!network(&gone, EAP_SSID).unwrap().connected());

        // The radio switch.
        eprintln!("step: The radio switch.");
        nm.set_wifi_enabled(false).await.unwrap();
        seen.saw("wifi_enabled to drop", |s| !s.wifi_enabled);
        assert!(nm.snapshot().await.unwrap().networks.is_empty());
        nm.set_wifi_enabled(true).await.unwrap();
        until(&nm, "networks after the radio came back", Duration::from_secs(60), |s| {
            s.wifi_enabled && network(s, PSK_SSID).is_some()
        })
        .await;
    });
}
