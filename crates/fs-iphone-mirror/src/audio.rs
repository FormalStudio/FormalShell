//! The phone's sound: one AAC-ELD access unit per RTP packet (480 stereo
//! samples at 48 kHz), decoded with libavcodec's `aac` and played through a
//! PipeWire stream of our own.
//!
//! The picture is shown the moment it decodes, so the sound keeps as little
//! as Wi-Fi allows: playback starts once `TARGET` is buffered, a run dry
//! waits for `TARGET` again, and a backlog (the phone's clock running ahead
//! of ours, or a burst after a stall) is shed a sample at a time, or at once
//! past `CEILING`.

use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use pipewire as pw;
use pw::spa;

use crate::av::{Av, AvFrameHead, AvPacketHead, AvParametersHead, EAGAIN};

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
/// Buffered before playing, in frames: two packets and a half.
const TARGET: usize = 1200;
/// Past this the oldest sound goes at once.
const CEILING: usize = 7200;
/// AudioSpecificConfig the phone's encoder advertises: ER AAC ELD, 48 kHz,
/// stereo, 480-sample frames.
const ASC: [u8; 4] = [0xf8, 0xe6, 0x40, 0x00];
const AV_CODEC_ID_AAC: c_int = 0x15002;
const AVMEDIA_TYPE_AUDIO: c_int = 1;
const AV_SAMPLE_FMT_FLT: c_int = 3;
const AV_SAMPLE_FMT_FLTP: c_int = 8;

#[derive(Default)]
struct Ring {
    samples: VecDeque<f32>,
    playing: bool,
}

impl Ring {
    fn frames(&self) -> usize {
        self.samples.len() / CHANNELS
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
        ring.samples.extend(&pcm);
        let frames = ring.frames();
        if frames > CEILING {
            let shed = (frames - TARGET) * CHANNELS;
            ring.samples.drain(..shed);
        }
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
        .add_local_listener_with_user_data(0u64)
        .process(move |stream, trimmed| {
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
            let mute = muted.load(Ordering::Relaxed);
            if let Ok(mut ring) = ring.lock() {
                if !ring.playing && ring.frames() >= TARGET {
                    ring.playing = true;
                }
                if ring.playing {
                    // Running long: drop one frame in a hundred, below hearing.
                    if ring.frames() > TARGET + frames + 960 {
                        let drop = (frames / 100).max(1) * CHANNELS;
                        ring.samples.drain(..drop);
                        *trimmed += drop as u64 / CHANNELS as u64;
                    }
                    let have = (ring.frames().min(frames)) * CHANNELS;
                    for (o, s) in out.iter_mut().zip(ring.samples.drain(..have)) {
                        *o = if mute { 0.0 } else { s };
                    }
                    out[have..].fill(0.0);
                    if have < frames * CHANNELS {
                        ring.playing = false;
                    }
                } else {
                    out.fill(0.0);
                }
            } else {
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
