//! The Wi-Fi CoreDevice tunnel: find the phone's `_remotepairing._tcp`
//! service, pair-verify with the stored record (never pair-setup, which would
//! raise a prompt on the phone), open a TLS-PSK CDTunnel and run the
//! userspace TCP stack over it.

use std::net::{IpAddr, SocketAddr};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::time::Duration;

use idevice::IdeviceError;
use idevice::remote_pairing::{
    PeerDevice, RemotePairingClient, RpPairingSocket, connect_tls_psk_tunnel_native,
};
use idevice::rsd::RsdHandshake;
use idevice::tcp::{adapter::Adapter, handle::AdapterHandle};
use tokio::net::TcpStream;

use super::pairing::Record;

const SERVICE: &str = "_remotepairing._tcp.local.";

pub struct Tunnel {
    pub adapter: AdapterHandle,
    pub rsd: RsdHandshake,
    pub phone: SocketAddr,
    pub acks: QuickAck,
    /// The phone drops the tunnel listener when this connection closes.
    _control: RemotePairingClient<RpPairingSocket<TcpStream>>,
}

/// The tunnel's TCP socket, for acknowledging what the phone sent at once.
/// The phone sends with Nagle on: a short segment (a small frame, a frame's
/// tail, a sound packet) waits there until everything before it is
/// acknowledged, and Linux holds an ACK up to 40 ms once we have sent data
/// of our own, which the stream's upkeep always has. Measured on the
/// socket, the picture's packets arrived 44 ms apart or right behind our
/// own 50 ms report.
pub struct QuickAck(Option<OwnedFd>);

impl QuickAck {
    fn of(stream: &TcpStream) -> Self {
        Self(stream.as_fd().try_clone_to_owned().ok())
    }

    /// Sends the ACK the kernel is holding. The option does not stick: the
    /// next data we send puts the socket back to delaying, so this is called
    /// for every datagram the tunnel delivers.
    pub fn now(&self) {
        let Some(fd) = &self.0 else { return };
        let on: libc::c_int = 1;
        // SAFETY: a plain setsockopt on a socket this struct keeps open.
        unsafe {
            libc::setsockopt(fd.as_raw_fd(), libc::IPPROTO_TCP, libc::TCP_QUICKACK, (&raw const on).cast(), std::mem::size_of::<libc::c_int>() as libc::socklen_t);
        }
    }
}

#[derive(Debug)]
pub enum Error {
    NotFound,
    Verify(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => f.write_str("iPhone not found on the network"),
            Error::Verify(e) => write!(f, "pair-verify failed: {e}"),
            Error::Io(e) => f.write_str(e),
        }
    }
}

impl From<IdeviceError> for Error {
    fn from(e: IdeviceError) -> Self {
        Error::Io(format!("{e:?}"))
    }
}

/// Browse for the phone the record belongs to, matching the advertised
/// `authTag` against the record's alt IRK. IPv4 first: a link-local IPv6
/// answer needs a scope and gains nothing on one LAN.
pub async fn discover(record: &Record, wait: Duration) -> Result<Vec<SocketAddr>, Error> {
    let alt_irk = record.alt_irk;
    tokio::task::spawn_blocking(move || {
        let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| Error::Io(e.to_string()))?;
        let rx = mdns.browse(SERVICE).map_err(|e| Error::Io(e.to_string()))?;
        let deadline = std::time::Instant::now() + wait;
        let mut out = Vec::new();
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(event) = rx.recv_timeout(left) else { break };
            let mdns_sd::ServiceEvent::ServiceResolved(info) = event else { continue };
            let id = info.get_property_val_str("identifier").unwrap_or_default();
            let tag = info.get_property_val_str("authTag").unwrap_or_default();
            if !PeerDevice::validate_auth_tag(&alt_irk, id, tag) {
                tracing::debug!("mdns: {} is not this phone", info.get_fullname());
                continue;
            }
            let mut addrs: Vec<IpAddr> = info.get_addresses().iter().copied().collect();
            addrs.sort_by_key(|a| a.is_ipv6());
            out.extend(addrs.into_iter().filter(|a| !is_link_local(a)).map(|a| SocketAddr::new(a, info.get_port())));
            if !out.is_empty() {
                break;
            }
        }
        let _ = mdns.shutdown();
        if out.is_empty() { Err(Error::NotFound) } else { Ok(out) }
    })
    .await
    .map_err(|e| Error::Io(e.to_string()))?
}

fn is_link_local(a: &IpAddr) -> bool {
    match a {
        IpAddr::V4(v) => v.is_link_local(),
        IpAddr::V6(v) => v.is_unicast_link_local(),
    }
}

pub async fn open(record: &mut Record, phone: SocketAddr) -> Result<Tunnel, Error> {
    let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(phone))
        .await
        .map_err(|_| Error::Io(format!("connect {phone}: timed out")))?
        .map_err(|e| Error::Io(format!("connect {phone}: {e}")))?;
    let _ = stream.set_nodelay(true);
    let host = record.file.identifier.clone();
    let mut rpc = RemotePairingClient::new(RpPairingSocket::new(stream), &host);
    rpc.attempt_pair_verify().await?;
    rpc.validate_pairing(&mut record.file)
        .await
        .map_err(|e| Error::Verify(format!("{e:?}")))?;
    let port = rpc.create_tcp_listener().await?;
    let tunnel_stream = TcpStream::connect(SocketAddr::new(phone.ip(), port))
        .await
        .map_err(|e| Error::Io(format!("tunnel port {port}: {e}")))?;
    let _ = tunnel_stream.set_nodelay(true);
    let acks = QuickAck::of(&tunnel_stream);
    acks.now();
    let tunnel = connect_tls_psk_tunnel_native(tunnel_stream, rpc.encryption_key()).await?;
    let client_ip: IpAddr = tunnel.info.client_address.parse().map_err(|_| Error::Io("bad tunnel address".into()))?;
    let server_ip: IpAddr = tunnel.info.server_address.parse().map_err(|_| Error::Io("bad tunnel address".into()))?;
    let rsd_port = tunnel.info.server_rsd_port;
    tracing::info!(
        "tunnel {} -> {} mtu {} rsd {}",
        tunnel.info.client_address,
        tunnel.info.server_address,
        tunnel.info.mtu,
        rsd_port
    );
    let mut adapter = Adapter::new(Box::new(tunnel.into_inner()), client_ip, server_ip).to_async_handle();
    let rsd_stream = adapter.connect(rsd_port).await.map_err(|e| Error::Io(format!("rsd: {e}")))?;
    let rsd = RsdHandshake::new(rsd_stream).await?;
    Ok(Tunnel { adapter, rsd, phone, acks, _control: rpc })
}
