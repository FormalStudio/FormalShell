//! The client against the system bus's real bluetoothd. Run by `dev/check.sh`
//! in a VM rig guest, in two phases: first with
//! bluetoothd up and no controller, then after btvirt has created two virtual
//! ones that find, pair with and forget each other.

use std::time::Duration;

use async_channel::Receiver;
use async_executor::LocalExecutor;
use async_io::Timer;
use fs_bluez::agent::{AgentRequest, Capability};
use fs_bluez::state::Event;
use fs_bluez::{Bluez, Error};
use futures_lite::future::{block_on, or};
use zbus::Connection;

async fn within<T>(what: &str, fut: impl Future<Output = T>) -> T {
    or(async { Some(fut.await) }, async {
        Timer::after(Duration::from_secs(60)).await;
        None
    })
    .await
    .unwrap_or_else(|| panic!("timed out waiting for {what}"))
}

async fn expect(rx: &Receiver<Event>, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
    within(what, async {
        loop {
            let e = rx.recv().await.expect("event channel closed");
            println!("event: {e:?}");
            if pred(&e) {
                return e;
            }
        }
    })
    .await
}

fn run<F: AsyncFnOnce(&LocalExecutor<'_>)>(f: F) {
    let ex = LocalExecutor::new();
    block_on(ex.run(f(&ex)));
}

#[test]
#[ignore = "needs bluetoothd; run by dev/check.sh"]
fn bluetoothd_without_a_controller_is_the_no_adapter_state() {
    run(async |ex| {
        let conn = Connection::system().await.unwrap();
        let (bluez, _rx, monitor) = Bluez::connect(&conn).await.unwrap();
        ex.spawn(monitor.run()).detach();
        let s = bluez.state();
        assert!(s.default_adapter().is_none());
        assert!(s.devices().is_empty());
        assert!(matches!(bluez.start_discovery("/org/bluez/hci0").await, Err(Error::Bus(_))));
        assert!(matches!(bluez.pair("AA:BB:CC:DD:EE:FF".parse().unwrap()).await, Err(Error::NoAdapter)));
    });
}

#[test]
#[ignore = "needs bluetoothd and two btvirt controllers; run by dev/check.sh"]
fn two_virtual_controllers_discover_pair_and_forget() {
    run(async |ex| {
        let conn = Connection::system().await.unwrap();
        let (bluez, rx, monitor) = Bluez::connect(&conn).await.unwrap();
        ex.spawn(monitor.run()).detach();

        within("two adapters", async {
            while bluez.state().adapters().len() < 2 {
                Timer::after(Duration::from_millis(200)).await;
            }
        })
        .await;
        let adapters = bluez.state().adapters();
        let (local, remote) = (adapters[0].clone(), adapters[1].clone());
        assert_eq!(local.path, "/org/bluez/hci0");
        assert_ne!(local.address, remote.address);
        assert_eq!(bluez.state().default_adapter().unwrap().path, local.path);

        let session = bluez.register_agent(Capability::DisplayYesNo).await.unwrap();
        let requests = session.requests().clone();
        ex.spawn(async move {
            while let Ok(request) = requests.recv().await {
                println!("agent: {request:?}");
                match request {
                    AgentRequest::Confirm { reply, .. } | AgentRequest::Authorize { reply, .. } | AgentRequest::AuthorizeService { reply, .. } => {
                        let _ = reply.send(true).await;
                    }
                    AgentRequest::PinCode { reply, .. } => {
                        let _ = reply.send(Some("0000".into())).await;
                    }
                    AgentRequest::Passkey { reply, .. } => {
                        let _ = reply.send(Some(0)).await;
                    }
                    _ => {}
                }
            }
        })
        .detach();

        for a in [&local, &remote] {
            bluez.set_powered(&a.path, true).await.unwrap();
        }
        within("both powered", async {
            loop {
                let s = bluez.state();
                if s.adapters().iter().all(|a| a.powered) {
                    return;
                }
                Timer::after(Duration::from_millis(200)).await;
            }
        })
        .await;
        bluez.set_pairable(&remote.path, true).await.unwrap();
        bluez.set_discoverable(&remote.path, true).await.unwrap();
        bluez.start_discovery(&local.path).await.unwrap();
        assert!(bluez.start_discovery(&local.path).await.unwrap_err().is_in_progress());
        expect(&rx, "local adapter discovering", |e| matches!(e, Event::AdapterChanged(a) if a.path == local.path && a.discovering)).await;
        let found = expect(&rx, "remote controller found", |e| matches!(e, Event::DeviceAdded(d) if d.address == remote.address && d.adapter == local.path)).await;
        let Event::DeviceAdded(found) = found else { unreachable!() };
        assert!(!found.paired);

        bluez.stop_discovery(&local.path).await.unwrap();
        expect(&rx, "scan stopped", |e| matches!(e, Event::AdapterChanged(a) if a.path == local.path && !a.discovering)).await;

        let paired = bluez.pair(remote.address).await;
        println!("pair: {paired:?}");
        paired.unwrap();
        within("paired in state", async {
            while !bluez.state().find_device(&local.path, remote.address).is_some_and(|d| d.paired) {
                Timer::after(Duration::from_millis(200)).await;
            }
        })
        .await;
        assert!(!bluez.state().find_device(&local.path, remote.address).unwrap().pairing);

        bluez.set_trusted(remote.address, true).await.unwrap();
        within("trusted in state", async {
            while !bluez.state().find_device(&local.path, remote.address).is_some_and(|d| d.trusted) {
                Timer::after(Duration::from_millis(200)).await;
            }
        })
        .await;

        bluez.remove(remote.address).await.unwrap();
        expect(&rx, "forgotten", |e| matches!(e, Event::DeviceRemoved { address, .. } if *address == remote.address)).await;
        drop(session);
    });
}
