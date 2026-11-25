//! The phone's sound: one AAC-ELD access unit per RTP packet (480 stereo
//! samples at 48 kHz), decoded with libavcodec's `aac` and played through a
//! PipeWire stream of our own.
//!
//! The picture is shown the moment it decodes, so the sound keeps as little
//! as Wi-Fi allows: a cycle's worth of the graph plus a margin as long as
//! the longest wait between packets, or what a run dry was short, easing
//! back while neither comes. The phone's radio dozes, and its packets can
//! come in bursts up to 150 ms apart. The ring reads its samples a little
//! faster or slower to hold the buffer there, which takes up the drift
//! between the phone's clock and ours; a backlog past that goes at once,
//! crossfaded. Never a sample dropped or a hard stop: either clicks.

use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pipewire as pw;
use pw::spa;

use crate::av::{Av, AvFrameHead, AvPacketHead, AvParametersHead, EAGAIN};

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
/// Kept beyond a cycle's worth, in frames, to start with and at least.
const MARGIN: f64 = 960.0;
const MARGIN_MOST: f64 = 7200.0;
/// A wait between packets longer than this is the phone going quiet, not
/// Wi-Fi.
const QUIET: f64 = 24_000.0;
/// Past the target by this many frames, the backlog goes at once.
const BACKLOG: usize = 2400;
/// The most the ring's reading leans to bring the buffer to its target: a
/// percent, too little to hear.
const LEAN: f64 = 0.01;
/// Frames a start, a run dry or a cut fades over.
const FADE: usize = 96;
/// AudioSpecificConfig for the phone's stream: ER AAC ELD, 48 kHz, stereo,
/// 480-sample frames, LD SBR off. The `f8e64000` cookie AudioToolbox takes
/// sets the resilience flags libavcodec then reads the units with, and
/// almost none decode.
const ASC: [u8; 4] = [0xf8, 0xe6, 0x50, 0x00];
const AV_CODEC_ID_AAC: c_int = 0x15002;
const AVMEDIA_TYPE_AUDIO: c_int = 1;
const AV_SAMPLE_FMT_FLT: c_int = 3;
const AV_SAMPLE_FMT_FLTP: c_int = 8;

struct Ring {
    samples: VecDeque<f32>,
    playing: bool,
    margin: f64,
    /// Frames the graph takes a cycle, as of the last.
    cycle: usize,
    /// Frames buffered at the start of a cycle, smoothed over about a second.
    level: Option<f64>,
    pushed: Option<Instant>,
    /// Input frames read per frame played, less one.
    lean: f64,
    /// Where between its first two frames the next read falls.
    phase: f64,
}

impl Default for Ring {
    fn default() -> Self {
        Self { samples: VecDeque::new(), playing: false, margin: MARGIN, cycle: 1024, level: None, pushed: None, lean: 0.0, phase: 0.0 }
    }
}

impl Ring {
    fn frames(&self) -> usize {
        self.samples.len() / CHANNELS
    }

    fn target(&self) -> usize {
        self.cycle + self.margin as usize
    }

    fn push(&mut self, pcm: &[f32]) {
        let now = Instant::now();
        if let Some(then) = self.pushed.replace(now) {
            let wait = (now - then).as_secs_f64() * RATE as f64;
            if wait < QUIET {
                self.margin = self.margin.max(wait.min(MARGIN_MOST));
            }
        }
        self.samples.extend(pcm);
        let target = self.target();
        if self.frames() > target + BACKLOG {
            self.cut(self.frames() - target);
        }
    }

    /// Drops the oldest `n` frames, crossfading into what follows.
    fn cut(&mut self, n: usize) {
        let fade = FADE.min(self.frames() - n);
        for i in 0..fade * CHANNELS {
            let w = (i / CHANNELS) as f32 / fade as f32;
            let j = n * CHANNELS + i;
            self.samples[j] = self.samples[i] * (1.0 - w) + self.samples[j] * w;
        }
        self.samples.drain(..n * CHANNELS);
        self.level = None;
    }

    /// Fills one cycle, reading the buffer `1 + lean` frames a frame,
    /// each played frame a line between the two it falls between.
    fn fill(&mut self, out: &mut [f32]) {
        let frames = out.len() / CHANNELS;
        self.cycle = frames;
        let target = self.target();
        let starting = !self.playing;
        if starting {
            if self.frames() < target {
                out.fill(0.0);
                return;
            }
            self.playing = true;
            self.level = None;
            self.lean = 0.0;
            self.phase = 0.0;
        }
        let level = self.frames();
        let ratio = 1.0 + self.lean;
        let end = self.phase + frames as f64 * ratio;
        // The last frame played may lie between the frame `end` falls in
        // and the one after.
        if level < end.floor() as usize + 2 {
            let have = level.min(frames);
            for (o, s) in out.iter_mut().zip(self.samples.drain(..have * CHANNELS)) {
                *o = s;
            }
            out[have * CHANNELS..].fill(0.0);
            fade(&mut out[..have * CHANNELS], false);
            self.playing = false;
            self.margin = (self.margin + (frames - have) as f64).min(MARGIN_MOST);
            return;
        }
        for (i, frame) in out.chunks_exact_mut(CHANNELS).enumerate() {
            let at = self.phase + i as f64 * ratio;
            let j = at as usize;
            let t = (at - j as f64) as f32;
            for (c, o) in frame.iter_mut().enumerate() {
                let a = self.samples[j * CHANNELS + c];
                *o = if t == 0.0 { a } else { a + (self.samples[(j + 1) * CHANNELS + c] - a) * t };
            }
        }
        let used = end.floor() as usize;
        self.phase = end - used as f64;
        self.samples.drain(..used * CHANNELS);
        if starting {
            fade(out, true);
        }
        // A millisecond a second.
        self.margin = (self.margin - frames as f64 / 1000.0).max(MARGIN);
        let level = self.level.map_or(level as f64, |l| l + (level as f64 - l) * 0.02);
        self.level = Some(level);
        self.lean = ((level - target as f64) / RATE as f64).clamp(-LEAN, LEAN);
    }
}

/// Ramps the start of `out` up from silence, or its end down to it.
fn fade(out: &mut [f32], up: bool) {
    let n = (out.len() / CHANNELS).min(FADE);
    let len = out.len();
    for i in 0..n {
        let g = i as f32 / n as f32;
        for c in 0..CHANNELS {
            let j = if up { i * CHANNELS + c } else { len - (i + 1) * CHANNELS + c };
            out[j] *= g;
        }
    }
}

pub struct Audio {
    tx: SyncSender<Vec<u8>>,
    ring: Arc<Mutex<Ring>>,
    pub muted: Arc<AtomicBool>,
    /// Device-side delay of the stream in frames, from its last cycle.
    device_delay: Arc<AtomicU32>,
}

impl Audio {
    pub fn start() -> Self {
        let (tx, rx) = sync_channel::<Vec<u8>>(32);
        let ring = Arc::new(Mutex::new(Ring::default()));
        let muted = Arc::new(AtomicBool::new(false));
        let device_delay = Arc::new(AtomicU32::new(0));
        let r = ring.clone();
        std::thread::Builder::new()
            .name("mirror-audio-decode".into())
            .spawn(move || decode(rx, r))
            .expect("spawn audio decoder");
        let (r, m, d) = (ring.clone(), muted.clone(), device_delay.clone());
        std::thread::Builder::new()
            .name("mirror-audio".into())
            .spawn(move || {
                if let Err(e) = play(r, m, d) {
                    tracing::error!("pipewire: {e}");
                }
            })
            .expect("spawn audio");
        Self { tx, ring, muted, device_delay }
    }

    pub fn push(&self, au: &[u8]) {
        let _ = self.tx.try_send(au.to_vec());
    }

    /// A new connection: whatever was buffered belongs to the old one.
    pub fn reset(&self) {
        if let Ok(mut ring) = self.ring.lock() {
            *ring = Ring::default();
        }
    }

    /// Buffered plus the device's own delay: how long a sample that arrives
    /// now waits before it is heard.
    pub fn playout_ms(&self) -> f64 {
        let buffered = self.ring.lock().map(|r| r.frames()).unwrap_or(0);
        (buffered + self.device_delay.load(Ordering::Relaxed) as usize) as f64 * 1000.0 / RATE as f64
    }
}

fn decode(rx: Receiver<Vec<u8>>, ring: Arc<Mutex<Ring>>) {
    let av = match Av::load() {
        Ok(av) => av,
        Err(e) => {
            tracing::error!("libavcodec: {e}");
            while rx.recv().is_ok() {}
            return;
        }
    };
    let mut codec = None;
    let mut pcm = Vec::with_capacity(480 * CHANNELS);
    while let Ok(au) = rx.recv() {
        if codec.is_none() {
            match AacEld::open(&av, &ASC) {
                Ok(c) => codec = Some(c),
                Err(e) => {
                    tracing::error!("aac: {e}");
                    while rx.recv().is_ok() {}
                    return;
                }
            }
        }
        pcm.clear();
        if let Err(e) = codec.as_mut().unwrap().decode(&au, &mut pcm) {
            // One bad unit (a volume change can produce one) is dropped; a
            // decoder that keeps failing is opened again.
            tracing::debug!("aac: {e}");
            codec = None;
            continue;
        }
        let Ok(mut ring) = ring.lock() else { return };
        ring.push(&pcm);
    }
}

struct AacEld<'a> {
    av: &'a Av,
    ctx: *mut c_void,
    packet: *mut c_void,
    frame: *mut c_void,
}

impl<'a> AacEld<'a> {
    fn open(av: &'a Av, asc: &[u8]) -> Result<Self, String> {
        // SAFETY: libavcodec calls on objects this struct owns and frees in
        // Drop; the parameters' extradata is av_mallocz'd with the padding
        // libavcodec reads past its end, and freed with the parameters.
        unsafe {
            (av.log_set_level)(16);
            let codec = (av.find_decoder_by_name)(c"aac".as_ptr());
            if codec.is_null() {
                return Err("aac not built in".into());
            }
            let ctx = (av.alloc_context3)(codec);
            if ctx.is_null() {
                return Err("no context".into());
            }
            let me = Self { av, ctx, packet: (av.packet_alloc)(), frame: (av.frame_alloc)() };
            let mut par = (av.parameters_alloc)();
            let head = par as *mut AvParametersHead;
            (*head).codec_type = AVMEDIA_TYPE_AUDIO;
            (*head).codec_id = AV_CODEC_ID_AAC;
            let extra = (av.mallocz)(asc.len() + 64) as *mut u8;
            std::ptr::copy_nonoverlapping(asc.as_ptr(), extra, asc.len());
            (*head).extradata = extra;
            (*head).extradata_size = asc.len() as c_int;
            let r = (av.parameters_to_context)(ctx, par);
            (av.parameters_free)(&mut par);
            if r < 0 {
                return Err(format!("parameters ({r})"));
            }
            // The ELD config does not fill these in, and a frame without
            // them is thrown away as invalid.
            (av.opt_set)(ctx, c"ch_layout".as_ptr(), c"stereo".as_ptr(), 0);
            (av.opt_set)(ctx, c"ar".as_ptr(), c"48000".as_ptr(), 0);
            let r = (av.open2)(ctx, codec, std::ptr::null_mut());
            if r < 0 {
                return Err(format!("open failed ({r})"));
            }
            Ok(me)
        }
    }

    /// Decodes one unit, appending interleaved stereo f32 to `out`.
    fn decode(&mut self, au: &[u8], out: &mut Vec<f32>) -> Result<(), String> {
        // SAFETY: as in the video decoder: the packet borrows `au` for
        // send_packet only, and the frame's planes are read per its format.
        unsafe {
            let pkt = self.packet as *mut AvPacketHead;
            (*pkt).data = au.as_ptr() as *mut u8;
            (*pkt).size = au.len() as c_int;
            let r = (self.av.send_packet)(self.ctx, self.packet);
            (*pkt).data = std::ptr::null_mut();
            (*pkt).size = 0;
            if r < 0 && r != EAGAIN {
                return Err(format!("send_packet {r}"));
            }
            loop {
                let r = (self.av.receive_frame)(self.ctx, self.frame);
                if r == EAGAIN {
                    return Ok(());
                }
                if r < 0 {
                    return Err(format!("receive_frame {r}"));
                }
                let f = &*(self.frame as *const AvFrameHead);
                let n = f.nb_samples as usize;
                match f.format {
                    AV_SAMPLE_FMT_FLTP => {
                        let l = std::slice::from_raw_parts(f.data[0] as *const f32, n);
                        let r = if f.data[1].is_null() { l } else { std::slice::from_raw_parts(f.data[1] as *const f32, n) };
                        for i in 0..n {
                            out.push(l[i]);
                            out.push(r[i]);
                        }
                    }
                    AV_SAMPLE_FMT_FLT => {
                        out.extend_from_slice(std::slice::from_raw_parts(f.data[0] as *const f32, n * CHANNELS));
                    }
                    other => return Err(format!("sample format {other}")),
                }
            }
        }
    }
}

impl Drop for AacEld<'_> {
    fn drop(&mut self) {
        // SAFETY: frees what `open` allocated; libav ignores null pointers.
        unsafe {
            (self.av.free_context)(&mut self.ctx);
            (self.av.packet_free)(&mut self.packet);
            (self.av.frame_free)(&mut self.frame);
        }
    }
}

fn play(ring: Arc<Mutex<Ring>>, muted: Arc<AtomicBool>, device_delay: Arc<AtomicU32>) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "iPhone",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Game",
            *pw::keys::APP_NAME => "iPhone Mirroring",
            *pw::keys::APP_ICON_NAME => "phone",
            *pw::keys::NODE_LATENCY => "480/48000",
        },
    )?;
    let stride = CHANNELS * std::mem::size_of::<f32>();
    let _listener = stream
        .add_local_listener_with_user_data(())
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let requested = buffer.requested() as usize;
            if let Ok(t) = stream.time() {
                device_delay.store(t.delay().max(0) as u32, Ordering::Relaxed);
            }
            let datas = buffer.datas_mut();
            let data = &mut datas[0];
            let Some(slice) = data.data() else { return };
            let room = slice.len() / stride;
            let frames = if requested == 0 { room.min(480) } else { requested.min(room) };
            let out: &mut [f32] = bytemuck_f32(&mut slice[..frames * stride]);
            match ring.lock() {
                Ok(mut ring) => ring.fill(out),
                Err(_) => out.fill(0.0),
            }
            if muted.load(Ordering::Relaxed) {
                out.fill(0.0);
            }
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = stride as _;
            *chunk.size_mut() = (frames * stride) as _;
        })
        .register()?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(RATE);
    info.set_channels(CHANNELS as u32);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .map_err(|_| pw::Error::CreationFailed)?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&values).ok_or(pw::Error::CreationFailed)?];
    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    mainloop.run();
    Ok(())
}

fn bytemuck_f32(bytes: &mut [u8]) -> &mut [f32] {
    // SAFETY: any bit pattern is an f32; PipeWire's mapped buffers are
    // page aligned, which the assert checks.
    let (head, body, _) = unsafe { bytes.align_to_mut::<f32>() };
    debug_assert!(head.is_empty());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 440 Hz tone from a clock 0.5% fast or slow, in packets that come
    /// in bursts, played a cycle at a time: no step between frames the tone
    /// cannot make, and the buffer held near its target.
    #[test]
    fn a_drifting_bursty_phone_plays_without_a_step() {
        for drift in [1.005, 0.995] {
            drifting(drift);
        }
    }

    fn drifting(drift: f64) {
        let mut ring = Ring::default();
        let tone = |n: usize| (n as f64 * 440.0 * std::f64::consts::TAU / RATE as f64).sin() as f32 * 0.5;
        let (mut made, mut owed, mut last) = (0usize, 0.0f64, None::<f32>);
        let (mut steps, mut most) = (0, 0usize);
        let mut out = vec![0.0f32; 1024 * CHANNELS];
        for cycle in 0..3000 {
            // Every fourth cycle's packets come late, with the next's.
            owed += 1024.0 * drift;
            if cycle % 4 != 3 {
                while owed >= 480.0 {
                    let pcm: Vec<f32> = (made..made + 480).flat_map(|n| [tone(n), tone(n)]).collect();
                    ring.push(&pcm);
                    made += 480;
                    owed -= 480.0;
                }
            }
            ring.fill(&mut out);
            for frame in out.chunks_exact(CHANNELS) {
                if let Some(prev) = last
                    && ring.playing
                    && (frame[0] - prev).abs() > 0.03
                {
                    steps += 1;
                }
                last = Some(frame[0]);
            }
            if cycle > 500 {
                most = most.max(ring.frames());
            }
        }
        assert_eq!(steps, 0, "at {drift}");
        assert!(most < ring.target() + BACKLOG, "{most} buffered");
    }

    /// A second of 440 Hz through libfdk's AAC-ELD at 480 samples a frame,
    /// one access unit at a time as the phone sends them. Needs an ffmpeg
    /// built with libfdk_aac, so it runs on request only.
    #[test]
    #[ignore]
    fn decodes_eld_one_unit_at_a_time() {
        let dir = std::env::temp_dir().join(format!("fs-eld-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("eld.m4a");
        let ok = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=f=440:r=48000:d=1", "-ac", "2"])
            .args(["-c:a", "libfdk_aac", "-profile:a", "aac_eld", "-frame_length", "480", "-b:a", "128k"])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(ok.success());
        let probe = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-show_packets", "-show_streams", "-show_data", "-of", "flat"])
            .arg(&clip)
            .output()
            .unwrap();
        let text = String::from_utf8(probe.stdout).unwrap();
        // `flat` prints each hex dump as one quoted string of "offset: hex  ascii" lines.
        let hex = |dump: &str| -> Vec<u8> {
            dump.split("\\n")
                .flat_map(|line| line.split_once(": ").map(|(_, r)| r.split("  ").next().unwrap_or("").replace(' ', "")))
                .flat_map(|h| (0..h.len() / 2).map(move |i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect::<Vec<_>>())
                .collect()
        };
        let mut asc = Vec::new();
        let mut units = Vec::new();
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("streams.stream.0.extradata=") {
                asc = hex(v.trim_matches('"'));
            } else if line.starts_with("packets.packet.") && line.contains(".data=") {
                units.push(hex(line.split_once('=').unwrap().1.trim_matches('"')));
            }
        }
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(asc.len(), 4, "extradata from ffprobe");
        assert!(units.len() > 90, "{} units", units.len());
        let av = Av::load().unwrap();
        let mut codec = AacEld::open(&av, &asc).unwrap();
        let mut pcm = Vec::new();
        for u in &units {
            codec.decode(u, &mut pcm).unwrap();
        }
        assert!(pcm.len() >= (units.len() - 2) * 480 * CHANNELS, "{} samples", pcm.len());
        let peak = pcm.iter().fold(0f32, |m, s| m.max(s.abs()));
        // lavfi's sine is at an eighth of full scale.
        assert!(peak > 0.05 && peak < 0.2, "peak {peak}");
        AacEld::open(&av, &ASC).expect("the phone's config opens");
    }
}

