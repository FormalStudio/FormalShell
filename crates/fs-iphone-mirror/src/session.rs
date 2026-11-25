//! One phone, kept connected: tunnel, developer image, screen stream, RTCP
//! upkeep and HID input, on a tokio runtime of its own. A failure anywhere
//! tears the connection down and tries again with backoff; a quiet stream
//! (the screen asleep) never does, only a tunnel that stops answering.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use idevice::core_device::{
    ButtonState, HevcDepacketizer, IndigoHidClient, MainKeyboardService, ReportBlock, RtpPacket,
    TOUCHSCREEN_STATE_CONTACT, TOUCHSCREEN_STATE_RELEASE, UniversalHidServiceClient,
    build_frame_ack, build_keyframe_request, build_liveness, build_rctl,
};
use idevice::core_device::{
    DataInclusionPolicy, GENERAL_PASTEBOARD, KeyboardUsage, Orientation, OrientationServiceClient, PasteboardServiceClient,
    RotationDirection, UTI_JPEG, UTI_PNG,
};
use idevice::diagnostics_relay::DiagnosticsRelayClient;
use idevice::springboardservices::{InterfaceOrientation, SpringBoardServicesClient};
use idevice::{ReadWrite, RsdService};
use tokio::sync::mpsc;

use crate::audio::Audio;
use crate::decode::Decoder;
use crate::device::{image, pairing, stream, tunnel};

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Searching,
    Connecting,
    Preparing,
    Streaming,
    /// Lost, with the reason and when the next attempt starts.
    Reconnecting(String),
    /// Nothing to connect with until the user acts (no pairing record).
    Unpaired,
}

#[derive(Clone, Debug)]
pub enum Event {
    Status(Status),
    Phone { product_type: String },
    /// The phone turned: quarter turns clockwise that stand its picture
    /// up. The stream is always the portrait panel, whichever way it faces.
    Turn(u8),
    /// The phone's screen went dark (it sleeps locked) or lit up again.
    Asleep(bool),
    /// Something was copied on the phone.
    Clip(Clip),
}

/// What a pasteboard holds, as far as the two sides share it.
#[derive(Clone, PartialEq)]
pub enum Clip {
    Text(String),
    Image { mime: &'static str, bytes: Vec<u8> },
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Clip::Text(t) => write!(f, "Text({t:?})"),
            Clip::Image { mime, bytes } => write!(f, "Image({mime}, {} bytes)", bytes.len()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Home,
    AppSwitcher,
    /// Locks the phone, or wakes it while its screen is dark.
    Side,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    /// A finger on the screen, in 0..1 of the phone's portrait screen.
    Touch(Phase, f64, f64),
    /// The keys held, as HID keyboard usages; the phone's report follows it.
    Keys(BTreeSet<u8>),
    /// Chords pressed and released in turn: typing one character, dead key
    /// first where the phone's layout needs one.
    Type(Vec<BTreeSet<u8>>),
    Button(Button),
    /// Fill the phone's pasteboard, unless it holds this already.
    Clip(Clip),
    /// The phone's Command+V.
    Paste,
    /// Turn the phone a quarter, as if it were rotated in the hand.
    Rotate(RotationDirection),
    /// Lift every finger and key: the window lost focus or closed.
    Release,
}

const CNAME: &str = "formalshell";

pub struct Session {
    cmds: mpsc::UnboundedSender<Cmd>,
    done: std::sync::mpsc::Receiver<()>,
}

impl Session {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.cmds.send(cmd);
    }

    /// Lets the phone have what is queued, then waits up to `wait` for the
    /// session and the decoder to stop.
    pub fn close(self, wait: Duration) {
        drop(self.cmds);
        let _ = self.done.recv_timeout(wait);
    }
}

pub fn spawn(sink: impl Fn(Event) + Send + 'static, decoder: Decoder, audio: Arc<Audio>) -> Session {
    let (tx, rx) = mpsc::unbounded_channel();
    let (done_tx, done) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("mirror-session".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(run(sink, rx, decoder, audio));
            drop(rt);
            let _ = done_tx.send(());
        })
        .expect("spawn session");
    Session { cmds: tx, done }
}

async fn run(sink: impl Fn(Event), mut cmds: mpsc::UnboundedReceiver<Cmd>, decoder: Decoder, audio: Arc<Audio>) {
    let mut backoff = Duration::from_secs(1);
    let mut last_addr: Option<SocketAddr> = None;
    loop {
        let started = Instant::now();
        let reason = match connect(&sink, &mut cmds, &decoder, &audio, &mut last_addr).await {
            Ok(()) => return,
            Err(e) => e,
        };
        if started.elapsed() > Duration::from_secs(30) {
            backoff = Duration::from_secs(1);
        }
        tracing::warn!("session ended: {reason}; retrying in {backoff:?}");
        if reason == UNPAIRED {
            sink(Event::Status(Status::Unpaired));
        } else {
            sink(Event::Status(Status::Reconnecting(reason)));
        }
        // Input that arrives while there is no phone goes nowhere.
        let wait = tokio::time::sleep(backoff);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                c = cmds.recv() => if c.is_none() { return },
            }
        }
        backoff = (backoff * 2).min(Duration::from_secs(15));
    }
}

const UNPAIRED: &str = "unpaired";

async fn connect(
    sink: &impl Fn(Event),
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
    decoder: &Decoder,
    audio: &Audio,
    last_addr: &mut Option<SocketAddr>,
) -> Result<(), String> {
    let mut record = pairing::all().into_iter().next().ok_or(UNPAIRED)?;
    sink(Event::Status(Status::Searching));
    let mut tunnel = None;
    if let Some(addr) = *last_addr {
        sink(Event::Status(Status::Connecting));
        tunnel = tokio::time::timeout(Duration::from_secs(10), tunnel::open(&mut record, addr))
            .await
            .ok()
            .and_then(Result::ok);
    }
    let mut tunnel = match tunnel {
        Some(t) => t,
        None => {
            let addrs = tunnel::discover(&record, Duration::from_secs(6)).await.map_err(|e| e.to_string())?;
            sink(Event::Status(Status::Connecting));
            let mut opened = Err("no address".to_string());
            for addr in addrs {
                opened = match tokio::time::timeout(Duration::from_secs(10), tunnel::open(&mut record, addr)).await {
                    Ok(Ok(t)) => Ok(t),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err(format!("{addr}: timed out")),
                };
                if opened.is_ok() {
                    break;
                }
            }
            opened?
        }
    };
    *last_addr = Some(tunnel.phone);
    if let Some(pt) = tunnel.rsd.properties.get("ProductType").and_then(|v| v.as_string()) {
        sink(Event::Phone { product_type: pt.to_string() });
    }
    // A phone a session left turned stays turned. An interface in
    // landscape gives that away; one in portrait reads as upright.
    match interface_turn(&mut tunnel).await {
        Ok(turn) => sink(Event::Turn(turn)),
        Err(e) => tracing::debug!("orientation: {e}"),
    }
    if !image::mounted(&mut tunnel).await? {
        sink(Event::Status(Status::Preparing));
        image::mount(&mut tunnel).await?;
    }
    let screen = tokio::time::timeout(Duration::from_secs(15), stream::start(&mut tunnel.adapter, &mut tunnel.rsd))
        .await
        .map_err(|_| "screen stream did not start".to_string())??;
    let mut lit = backlight(&mut tunnel).await;
    let mut copied = pasteboard(&mut tunnel).await;
    let mut input = Input::default();
    audio.reset();
    let result = pump(sink, cmds, decoder, audio, &mut tunnel, &screen, &mut input, &mut lit, &mut copied).await;
    input.release(&mut tunnel).await;
    let mut client = screen.client;
    let _ = tokio::time::timeout(Duration::from_secs(2), client.stop_media_stream()).await;
    result
}

async fn pump(
    sink: &impl Fn(Event),
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
    decoder: &Decoder,
    audio_out: &Audio,
    tunnel: &mut tunnel::Tunnel,
    screen: &stream::Screen,
    input: &mut Input,
    lit: &mut mpsc::UnboundedReceiver<bool>,
    copied: &mut mpsc::UnboundedReceiver<Clip>,
) -> Result<(), String> {
    let video = screen.video_rtcp;
    let audio = screen.audio_rtcp;
    let mut depack = HevcDepacketizer::new();
    let mut au: Vec<u8> = Vec::new();
    let mut closed_with_aud = false;
    let (mut base_seq, mut highest_seq, mut frames) = (None::<u16>, 0u32, 0u16);
    let mut audio_seq = 0u32;
    let mut last_packet = Instant::now();
    let mut last_keyframe_ask = Instant::now() - Duration::from_secs(10);
    let mut fir_seq = 0u8;
    let epoch = Instant::now();
    let mut rctl = tokio::time::interval(Duration::from_millis(50));
    let mut report = tokio::time::interval(Duration::from_secs(1));
    let mut streaming = false;
    let mut clock: Option<(u32, u32)> = None;
    let mut lags: Vec<i64> = Vec::new();
    let mut audio_clock: Option<(u32, u32)> = None;
    let mut audio_lags: Vec<i64> = Vec::new();
    loop {
        tokio::select! {
            dg = screen.video.recv() => {
                let dg = dg.map_err(|e| format!("video: {e}"))?;
                if idevice::core_device::is_rtcp(&dg.data) {
                    if let Some(sr) = idevice::core_device::rtcp::SenderReport::parse_first(&dg.data) {
                        tracing::debug!("video SR: phone clock behind ours by {} ms (transit included)", ntp_lag_ms(sr.ntp_middle));
                        if dg.data.len() >= 20 && dg.data[1] == 200 {
                            let rtp = u32::from_be_bytes([dg.data[16], dg.data[17], dg.data[18], dg.data[19]]);
                            clock = Some((sr.ntp_middle, rtp));
                        }
                    }
                    continue;
                }
                let Some(pkt) = RtpPacket::parse(&dg.data) else { continue };
                last_packet = Instant::now();
                if !streaming {
                    streaming = true;
                    sink(Event::Status(Status::Streaming));
                }
                let base = *base_seq.get_or_insert(pkt.sequence_number);
                highest_seq = highest_seq.max(pkt.sequence_number.wrapping_sub(base) as u32);
                depack.push(pkt.sequence_number, pkt.timestamp, pkt.payload);
                let mut out = depack.take_output();
                // The depacketizer opens the next unit with an AUD; this one
                // was already closed by its own.
                if closed_with_aud && out.starts_with(&AUD) {
                    out.drain(..AUD.len());
                }
                closed_with_aud = false;
                au.extend_from_slice(&out);
                if pkt.marker {
                    if let Some((ntp, rtp)) = clock {
                        // Capture time of this frame on the phone's clock; the
                        // video RTP clock runs at 24 kHz.
                        let ticks = pkt.timestamp.wrapping_sub(rtp) as i32 as i64;
                        let at = ntp.wrapping_add((ticks * 65536 / 24000) as u32);
                        lags.push(ntp_lag_ms(at));
                        if lags.len() == 120 {
                            lags.sort();
                            tracing::debug!("capture to received: p50 {} ms p95 {} ms", lags[60], lags[114]);
                            lags.clear();
                        }
                    }
                    au.extend_from_slice(&AUD);
                    closed_with_aud = true;
                    frames = frames.wrapping_add(1);
                    decoder.push(std::mem::take(&mut au));
                    send(tunnel, &screen.video, video.port, build_frame_ack(video.ours, pkt.timestamp)).await;
                }
                if decoder.wants_keyframe.load(std::sync::atomic::Ordering::Relaxed)
                    && last_keyframe_ask.elapsed() > Duration::from_millis(500)
                {
                    last_keyframe_ask = Instant::now();
                    fir_seq = fir_seq.wrapping_add(1);
                    let blocks = [block(video.phone, highest_seq)];
                    send(tunnel, &screen.video, video.port, build_keyframe_request(video.ours, CNAME, video.phone, &blocks, fir_seq)).await;
                }
            }
            dg = screen.audio.recv() => {
                let dg = dg.map_err(|e| format!("audio: {e}"))?;
                if idevice::core_device::is_rtcp(&dg.data) {
                    if dg.data.len() >= 20 && dg.data[1] == 200 {
                        let ntp = u32::from_be_bytes([dg.data[10], dg.data[11], dg.data[12], dg.data[13]]);
                        let rtp = u32::from_be_bytes([dg.data[16], dg.data[17], dg.data[18], dg.data[19]]);
                        audio_clock = Some((ntp, rtp));
                    }
                    continue;
                }
                if let Some(pkt) = RtpPacket::parse(&dg.data) {
                    audio_seq = pkt.sequence_number as u32;
                    audio_out.push(pkt.payload);
                    if let Some((ntp, rtp)) = audio_clock {
                        let ticks = pkt.timestamp.wrapping_sub(rtp) as i32 as i64;
                        audio_lags.push(ntp_lag_ms(ntp.wrapping_add((ticks * 65536 / 48000) as u32)));
                        if audio_lags.len() == 200 {
                            audio_lags.sort();
                            tracing::debug!(
                                "audio capture to received: p50 {} ms p95 {} ms; then {:.0} ms to the speaker",
                                audio_lags[100], audio_lags[190], audio_out.playout_ms()
                            );
                            audio_lags.clear();
                        }
                    }
                }
            }
            Some(on) = lit.recv() => sink(Event::Asleep(!on)),
            Some(clip) = copied.recv() => {
                // Our own copy coming back is not news.
                if input.clip.as_ref() != Some(&clip) {
                    input.clip = Some(clip.clone());
                    sink(Event::Clip(clip));
                }
            }
            _ = rctl.tick() => {
                let clock = epoch.elapsed().as_millis() as u16;
                send(tunnel, &screen.video, video.port, build_rctl(video.ours, clock, frames, highest_seq as u16)).await;
            }
            _ = report.tick() => {
                send(tunnel, &screen.video, video.port, build_liveness(video.ours, CNAME, &[block(video.phone, highest_seq)])).await;
                if audio.port != 0 {
                    send(tunnel, &screen.audio, audio.port, build_liveness(audio.ours, CNAME, &[block(audio.phone, audio_seq)])).await;
                }
                // The screen asleep sends nothing; the tunnel going away
                // shows as the display service not answering.
                if last_packet.elapsed() > Duration::from_secs(5) {
                    alive(tunnel).await?;
                    last_packet = Instant::now();
                }
            }
            cmd = cmds.recv() => {
                let Some(mut cmd) = cmd else { return Ok(()) };
                if let Cmd::Rotate(direction) = cmd {
                    let mut turned = rotate(tunnel, direction).await;
                    // An iPhone has no upside-down interface: on to landscape.
                    if let Ok(Some(2)) = turned {
                        turned = rotate(tunnel, direction).await;
                    }
                    match turned {
                        Ok(Some(turn)) => sink(Event::Turn(turn)),
                        Ok(None) => {}
                        Err(e) => tracing::warn!("{e}"),
                    }
                    continue;
                }
                // Only the newest of a run of moves matters.
                if matches!(cmd, Cmd::Touch(Phase::Move, ..)) {
                    while let Ok(next) = cmds.try_recv() {
                        if matches!(next, Cmd::Touch(Phase::Move, ..)) {
                            cmd = next;
                        } else {
                            input.handle(tunnel, cmd).await;
                            cmd = next;
                            break;
                        }
                    }
                }
                input.handle(tunnel, cmd).await;
            }
        }
    }
}

/// Our wall clock minus an NTP short-format stamp, in ms.
fn ntp_lag_ms(ntp_middle: u32) -> i64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() + 2_208_988_800;
    let frac = ((now.subsec_nanos() as u64) << 32) / 1_000_000_000;
    let ours = (((secs << 32) | frac) >> 16) as u32;
    let diff = ours.wrapping_sub(ntp_middle) as i32 as i64;
    diff * 1000 / 65536
}

const AUD: [u8; 7] = [0, 0, 0, 1, 0x46, 0x01, 0x50];

fn block(ssrc: u32, highest_seq: u32) -> ReportBlock {
    ReportBlock { source_ssrc: ssrc, highest_seq, ..Default::default() }
}

async fn send(tunnel: &tunnel::Tunnel, socket: &idevice::tcp::handle::UdpSocketHandle, port: u16, data: Vec<u8>) {
    if port == 0 {
        return;
    }
    let _ = tunnel;
    if let Err(e) = socket.send_to(port, data).await {
        tracing::debug!("rtcp: {e}");
    }
}

async fn alive(tunnel: &mut tunnel::Tunnel) -> Result<(), String> {
    let probe = async {
        let mut client = idevice::core_device::DisplayServiceClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
            .await
            .map_err(|e| format!("{e:?}"))?;
        client.get_media_stream_server_status().await.map_err(|e| format!("{e:?}"))
    };
    match tokio::time::timeout(Duration::from_secs(6), probe).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("phone stopped answering: {e}")),
        Err(_) => Err("phone stopped answering".into()),
    }
}

#[derive(Default)]
struct Input {
    hid: Option<UniversalHidServiceClient<Box<dyn ReadWrite>>>,
    keyboard: Option<MainKeyboardService>,
    indigo: Option<IndigoHidClient<Box<dyn ReadWrite>>>,
    touch: Option<(u16, u16)>,
    keys: BTreeSet<u8>,
    board: Option<PasteboardServiceClient<Box<dyn ReadWrite>>>,
    /// What both pasteboards hold, whichever side it came from.
    clip: Option<Clip>,
}

fn hid_xy(x: f64, y: f64) -> (u16, u16) {
    ((x.clamp(0.0, 1.0) * 65535.0).round() as u16, (y.clamp(0.0, 1.0) * 65535.0).round() as u16)
}

/// The phone's pasteboard, each time something is copied there.
async fn pasteboard(tunnel: &mut tunnel::Tunnel) -> mpsc::UnboundedReceiver<Clip> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut board = match PasteboardServiceClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd).await {
        Ok(board) => board,
        Err(e) => {
            tracing::debug!("pasteboard: {e:?}");
            return rx;
        }
    };
    if let Err(e) = board.set_change_notifications(true, GENERAL_PASTEBOARD, Some(DataInclusionPolicy::AllResolved)).await {
        tracing::debug!("pasteboard: {e:?}");
        return rx;
    }
    tokio::spawn(async move {
        while let Ok(snapshot) = board.recv_push().await {
            let image = || {
                let (uti, bytes) = snapshot.image()?;
                let mime = match uti {
                    UTI_PNG => "image/png",
                    UTI_JPEG => "image/jpeg",
                    _ => return None,
                };
                Some(Clip::Image { mime, bytes })
            };
            let Some(clip) = snapshot.text().map(Clip::Text).or_else(image) else { continue };
            if tx.send(clip).is_err() {
                return;
            }
        }
    });
    rx
}

/// Whether the screen is lit, each time that changes. A dark screen sends
/// no picture at all, and a still one hardly any, so the stream cannot tell
/// them apart; the backlight's brightness is 0 only while the screen is off.
async fn backlight(tunnel: &mut tunnel::Tunnel) -> mpsc::UnboundedReceiver<bool> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut relay = match DiagnosticsRelayClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd).await {
        Ok(relay) => relay,
        Err(e) => {
            tracing::debug!("backlight: {e:?}");
            return rx;
        }
    };
    tokio::spawn(async move {
        let mut was = None;
        loop {
            let tree = tokio::time::timeout(Duration::from_secs(3), relay.ioregistry(None, None, Some("AppleARMBacklight"))).await;
            let Ok(Ok(Some(tree))) = tree else { return };
            let brightness = tree
                .get("IODisplayParameters")
                .and_then(|p| p.as_dictionary())
                .and_then(|p| p.get("brightness"))
                .and_then(|b| b.as_dictionary())
                .and_then(|b| b.get("value"))
                .and_then(|v| v.as_unsigned_integer());
            let Some(brightness) = brightness else { return };
            let on = brightness > 0;
            if was != Some(on) {
                was = Some(on);
                if tx.send(on).is_err() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    rx
}

async fn interface_turn(tunnel: &mut tunnel::Tunnel) -> Result<u8, String> {
    let mut sb = SpringBoardServicesClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
        .await
        .map_err(|e| format!("springboard: {e:?}"))?;
    let o = tokio::time::timeout(Duration::from_secs(2), sb.get_interface_orientation())
        .await
        .map_err(|_| "springboard timed out".to_string())?
        .map_err(|e| format!("{e:?}"))?;
    // The interface turns against the phone: landscape left is the phone
    // turned clockwise.
    Ok(match o {
        InterfaceOrientation::LandscapeLeft => 1,
        InterfaceOrientation::LandscapeRight => 3,
        _ => 0,
    })
}

/// Turns the phone as if in the hand and returns the quarter turns
/// clockwise that stand its picture up, or none while it reads as flat.
async fn rotate(tunnel: &mut tunnel::Tunnel, direction: RotationDirection) -> Result<Option<u8>, String> {
    let mut control = OrientationServiceClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
        .await
        .map_err(|e| format!("device control: {e:?}"))?;
    let state = tokio::time::timeout(Duration::from_secs(3), control.rotate(direction))
        .await
        .map_err(|_| "rotate timed out".to_string())?
        .map_err(|e| format!("rotate: {e:?}"))?;
    tracing::debug!("orientation now {:?}", state.orientation);
    // UIKit's device landscape right is the phone turned clockwise: its
    // panel's left edge is now the top.
    Ok(match state.orientation {
        Orientation::Portrait => Some(0),
        Orientation::LandscapeRight => Some(1),
        Orientation::PortraitUpsideDown => Some(2),
        Orientation::LandscapeLeft => Some(3),
        _ => None,
    })
}

impl Input {
    async fn handle(&mut self, tunnel: &mut tunnel::Tunnel, cmd: Cmd) {
        let r = match cmd {
            Cmd::Touch(phase, x, y) => self.touch(tunnel, phase, hid_xy(x, y)).await,
            Cmd::Keys(keys) => self.keys(tunnel, keys).await,
            Cmd::Type(chords) => {
                let mut r = Ok(());
                for chord in chords {
                    r = self.keys(tunnel, chord).await;
                    if r.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(8)).await;
                    r = self.keys(tunnel, BTreeSet::new()).await;
                    if r.is_err() {
                        break;
                    }
                }
                r
            }
            Cmd::Button(b) => self.button(tunnel, b).await,
            Cmd::Clip(clip) => self.set_clip(tunnel, clip).await,
            Cmd::Paste => self.paste(tunnel).await,
            // The stream loop turns the phone: it reports where it landed.
            Cmd::Rotate(_) => Ok(()),
            Cmd::Release => {
                self.release(tunnel).await;
                Ok(())
            }
        };
        if let Err(e) = r {
            tracing::warn!("input: {e}");
            // A broken service connection is opened again on next use.
            self.hid = None;
            self.keyboard = None;
            self.indigo = None;
            self.keys.clear();
            self.touch = None;
        }
    }

    async fn hid(&mut self, tunnel: &mut tunnel::Tunnel) -> Result<&mut UniversalHidServiceClient<Box<dyn ReadWrite>>, String> {
        if self.hid.is_none() {
            self.hid = Some(
                UniversalHidServiceClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
                    .await
                    .map_err(|e| format!("hid: {e:?}"))?,
            );
        }
        Ok(self.hid.as_mut().unwrap())
    }

    async fn touch(&mut self, tunnel: &mut tunnel::Tunnel, phase: Phase, at: (u16, u16)) -> Result<(), String> {
        let down = self.touch.is_some();
        let hid = self.hid(tunnel).await?;
        match phase {
            Phase::Down | Phase::Move => {
                hid.send_touchscreen(TOUCHSCREEN_STATE_CONTACT, at.0, at.1, None).await.map_err(|e| format!("{e:?}"))?;
                self.touch = Some(at);
            }
            Phase::Up if down => {
                hid.send_touchscreen(TOUCHSCREEN_STATE_RELEASE, at.0, at.1, None).await.map_err(|e| format!("{e:?}"))?;
                self.touch = None;
            }
            Phase::Up => {}
        }
        Ok(())
    }

    /// Modifiers change in a report of their own before the keys that need
    /// them, and keys let go before modifiers do: a receiver reading a
    /// letter and Shift in one report can take the letter first.
    async fn keys(&mut self, tunnel: &mut tunnel::Tunnel, want: BTreeSet<u8>) -> Result<(), String> {
        if want == self.keys {
            return Ok(());
        }
        self.hid(tunnel).await?;
        if self.keyboard.is_none() {
            let hid = self.hid.as_mut().unwrap();
            self.keyboard = Some(hid.create_main_keyboard().await.map_err(|e| format!("keyboard: {e:?}"))?);
        }
        let is_mod = |u: &u8| (0xe0..=0xe7).contains(u);
        let kept: BTreeSet<u8> = self.keys.intersection(&want).copied().filter(|u| !is_mod(u)).collect();
        let old_mods: BTreeSet<u8> = self.keys.iter().copied().filter(is_mod).collect();
        let new_mods: BTreeSet<u8> = want.iter().copied().filter(is_mod).collect();
        let steps = [&kept | &old_mods, &kept | &new_mods, want.clone()];
        for step in steps {
            if step == self.keys {
                continue;
            }
            let usages: Vec<KeyboardUsage> = step.iter().filter_map(|u| KeyboardUsage::new(*u as u16).ok()).collect();
            let hid = self.hid.as_mut().unwrap();
            hid.set_main_keyboard_usages(self.keyboard.as_mut().unwrap(), usages)
                .await
                .map_err(|e| format!("keys: {e:?}"))?;
            self.keys = step;
        }
        Ok(())
    }

    async fn indigo(&mut self, tunnel: &mut tunnel::Tunnel) -> Result<&mut IndigoHidClient<Box<dyn ReadWrite>>, String> {
        if self.indigo.is_none() {
            self.indigo = Some(
                IndigoHidClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
                    .await
                    .map_err(|e| format!("indigo: {e:?}"))?,
            );
        }
        Ok(self.indigo.as_mut().unwrap())
    }

    async fn press(&mut self, tunnel: &mut tunnel::Tunnel, page: u64, usage: u64, hold: Duration) -> Result<(), String> {
        let indigo = self.indigo(tunnel).await?;
        indigo.send_button(page, usage, ButtonState::Down).await.map_err(|e| format!("{e:?}"))?;
        tokio::time::sleep(hold).await;
        indigo.send_button(page, usage, ButtonState::Up).await.map_err(|e| format!("{e:?}"))
    }

    async fn button(&mut self, tunnel: &mut tunnel::Tunnel, button: Button) -> Result<(), String> {
        match button {
            // The consumer-page Menu button waits out the double-press window
            // (about 300 ms) before acting; the keyboard's Command+H does not.
            Button::Home => {
                let held = self.keys.clone();
                self.keys(tunnel, BTreeSet::from([0xe3, 0x0b])).await?;
                tokio::time::sleep(Duration::from_millis(30)).await;
                self.keys(tunnel, held).await
            }
            // Apple's vendor keyboard page, Expose All. A double Home press
            // and edge swipes over the HID touchscreen do not reach SpringBoard.
            Button::AppSwitcher => self.press(tunnel, 0xff01, 0x10, Duration::from_millis(40)).await,
            Button::Side => self.press(tunnel, 0x0c, 0x30, Duration::from_millis(60)).await,
        }
    }

    async fn set_clip(&mut self, tunnel: &mut tunnel::Tunnel, clip: Clip) -> Result<(), String> {
        if self.clip.as_ref() == Some(&clip) {
            return Ok(());
        }
        if self.board.is_none() {
            self.board = Some(
                PasteboardServiceClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
                    .await
                    .map_err(|e| format!("pasteboard: {e:?}"))?,
            );
        }
        let board = self.board.as_mut().unwrap();
        let set = async {
            match &clip {
                Clip::Text(text) => board.set_text(text, GENERAL_PASTEBOARD).await,
                Clip::Image { mime, bytes } => board.set_image(bytes, if *mime == "image/jpeg" { UTI_JPEG } else { UTI_PNG }, GENERAL_PASTEBOARD).await,
            }
        };
        let r = tokio::time::timeout(Duration::from_secs(5), set).await;
        if !matches!(r, Ok(Ok(()))) {
            self.board = None;
        }
        r.map_err(|_| "pasteboard timed out".to_string())?.map_err(|e| format!("pasteboard: {e:?}"))?;
        self.clip = Some(clip);
        Ok(())
    }

    async fn paste(&mut self, tunnel: &mut tunnel::Tunnel) -> Result<(), String> {
        // The phone's Command+V, not Control+V.
        let held = self.keys.clone();
        self.keys(tunnel, BTreeSet::from([0xe3])).await?;
        self.keys(tunnel, BTreeSet::from([0xe3, 0x19])).await?;
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.keys(tunnel, BTreeSet::from([0xe3])).await?;
        self.keys(tunnel, held).await
    }

    async fn release(&mut self, tunnel: &mut tunnel::Tunnel) {
        if let (Some(at), Some(hid)) = (self.touch.take(), self.hid.as_mut()) {
            let _ = tokio::time::timeout(Duration::from_secs(1), hid.send_touchscreen(TOUCHSCREEN_STATE_RELEASE, at.0, at.1, None)).await;
        }
        if !self.keys.is_empty() {
            let _ = tokio::time::timeout(Duration::from_secs(1), self.keys(tunnel, BTreeSet::new())).await;
        }
    }
}
