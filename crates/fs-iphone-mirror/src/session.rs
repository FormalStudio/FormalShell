//! One phone, kept connected: tunnel, developer image, screen stream, RTCP
//! upkeep and HID input, on a tokio runtime of its own. A failure anywhere
//! tears the connection down and tries again with backoff; a quiet stream
//! (the screen asleep) never does, only a tunnel that stops answering.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use idevice::core_device::{
    ButtonState, HevcDepacketizer, IndigoHidClient, MainKeyboardService, ReportBlock, RtpPacket,
    TOUCHSCREEN_STATE_CONTACT, TOUCHSCREEN_STATE_RELEASE, UniversalHidServiceClient,
    build_frame_ack, build_keyframe_request, build_liveness, build_rctl,
};
use idevice::core_device::{KeyboardUsage, PasteboardServiceClient};
use idevice::{ReadWrite, RsdService};
use tokio::sync::mpsc;

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
    Lock,
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
    Paste(String),
    /// Lift every finger and key: the window lost focus or closed.
    Release,
}

const CNAME: &str = "formalshell";

pub fn spawn(sink: impl Fn(Event) + Send + 'static, decoder: Decoder) -> mpsc::UnboundedSender<Cmd> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("mirror-session".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(run(sink, rx, decoder));
        })
        .expect("spawn session");
    tx
}

async fn run(sink: impl Fn(Event), mut cmds: mpsc::UnboundedReceiver<Cmd>, decoder: Decoder) {
    let mut backoff = Duration::from_secs(1);
    let mut last_addr: Option<SocketAddr> = None;
    loop {
        let started = Instant::now();
        let reason = match connect(&sink, &mut cmds, &decoder, &mut last_addr).await {
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
    if !image::mounted(&mut tunnel).await? {
        sink(Event::Status(Status::Preparing));
        image::mount(&mut tunnel).await?;
    }
    let screen = tokio::time::timeout(Duration::from_secs(15), stream::start(&mut tunnel.adapter, &mut tunnel.rsd))
        .await
        .map_err(|_| "screen stream did not start".to_string())??;
    let mut input = Input::default();
    let result = pump(sink, cmds, decoder, &mut tunnel, &screen, &mut input).await;
    input.release(&mut tunnel).await;
    let mut client = screen.client;
    let _ = tokio::time::timeout(Duration::from_secs(2), client.stop_media_stream()).await;
    result
}

async fn pump(
    sink: &impl Fn(Event),
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
    decoder: &Decoder,
    tunnel: &mut tunnel::Tunnel,
    screen: &stream::Screen,
    input: &mut Input,
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
                if let Some(pkt) = RtpPacket::parse(&dg.data) {
                    audio_seq = pkt.sequence_number as u32;
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
}

fn hid_xy(x: f64, y: f64) -> (u16, u16) {
    ((x.clamp(0.0, 1.0) * 65535.0).round() as u16, (y.clamp(0.0, 1.0) * 65535.0).round() as u16)
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
            Cmd::Paste(text) => self.paste(tunnel, &text).await,
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
            Button::Lock => self.press(tunnel, 0x0c, 0x30, Duration::from_millis(60)).await,
        }
    }

    async fn paste(&mut self, tunnel: &mut tunnel::Tunnel, text: &str) -> Result<(), String> {
        let mut board = PasteboardServiceClient::connect_rsd(&mut tunnel.adapter, &mut tunnel.rsd)
            .await
            .map_err(|e| format!("pasteboard: {e:?}"))?;
        tokio::time::timeout(Duration::from_secs(5), board.set_text(text, idevice::core_device::GENERAL_PASTEBOARD))
            .await
            .map_err(|_| "pasteboard timed out".to_string())?
            .map_err(|e| format!("pasteboard: {e:?}"))?;
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
