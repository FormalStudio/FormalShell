//! HEVC to NV12 with the system's libavcodec, opened at run time (the
//! package links no codec and the distros ship different sonames):
//! NVDEC through `hevc_cuvid` where it opens, the software `hevc` decoder
//! where it does not. In process, because the ffmpeg CLI holds every frame
//! until the next one arrives, which on a still screen is forever.
//!
//! Only the newest frame is kept, so a slow present drops frames instead of
//! queueing them, and a decoder that falls behind skips to the next key
//! frame and asks the phone for one.

use std::ffi::{CStr, CString, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::av::{Av, AvFrameHead, AvPacketHead, EAGAIN};

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// NV12: `width * height` luma, then interleaved chroma at half size.
    pub data: Vec<u8>,
    /// When its last packet reached the decoder, and when it came out.
    pub received: Instant,
    pub decoded: Instant,
}

/// The newest decoded frame and the hook that tells its reader one landed.
pub struct Latest {
    pub frame: Mutex<Option<Frame>>,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl Latest {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self { frame: Mutex::new(None), notify: Box::new(notify) })
    }

    pub fn take(&self) -> Option<Frame> {
        self.frame.lock().ok()?.take()
    }
}

/// Access units waiting for the decoder; past this it is behind.
const QUEUE: usize = 6;

pub struct Decoder {
    tx: SyncSender<(Vec<u8>, Instant)>,
    /// Set when units were dropped: nothing decodes again until a key frame
    /// arrives, so the session asks the phone for one.
    pub wants_keyframe: Arc<AtomicBool>,
}

impl Decoder {
    pub fn start(latest: Arc<Latest>) -> Self {
        let (tx, rx) = sync_channel(QUEUE);
        let wants_keyframe = Arc::new(AtomicBool::new(false));
        let wants = wants_keyframe.clone();
        std::thread::Builder::new()
            .name("mirror-decode".into())
            .spawn(move || run(rx, latest, wants))
            .expect("spawn decoder");
        Self { tx, wants_keyframe }
    }

    pub fn push(&self, au: Vec<u8>) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send((au, Instant::now())) {
            self.wants_keyframe.store(true, Ordering::Relaxed);
        }
    }
}

fn is_keyframe(au: &[u8]) -> bool {
    au.windows(4)
        .any(|w| w[0] == 0 && w[1] == 0 && w[2] == 1 && (16..=23).contains(&((w[3] >> 1) & 0x3f)))
}

fn run(rx: Receiver<(Vec<u8>, Instant)>, latest: Arc<Latest>, wants: Arc<AtomicBool>) {
    let av = match Av::load() {
        Ok(av) => av,
        Err(e) => {
            tracing::error!("libavcodec: {e}");
            // Drain so the session never blocks on a decoder that is not there.
            while rx.recv().is_ok() {}
            return;
        }
    };
    let names: &[&str] = if std::env::var_os("FS_IPHONE_SOFTWARE_DECODE").is_some() {
        &["hevc"]
    } else {
        &["hevc_cuvid", "hevc"]
    };
    let mut choice = 0;
    let mut codec: Option<Codec> = None;
    let mut decoded_any = false;
    let mut skipping = true;
    let mut stats: Vec<Duration> = Vec::with_capacity(300);
    while let Ok((au, received)) = rx.recv() {
        let key = is_keyframe(&au);
        if wants.load(Ordering::Relaxed) && !key {
            skipping = true;
        }
        if key {
            skipping = false;
            wants.store(false, Ordering::Relaxed);
        }
        if skipping {
            continue;
        }
        if codec.is_none() {
            while choice < names.len() {
                match Codec::open(&av, names[choice]) {
                    Ok(c) => {
                        tracing::info!("decoding with {}", names[choice]);
                        codec = Some(c);
                        decoded_any = false;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("{}: {e}", names[choice]);
                        choice += 1;
                    }
                }
            }
            if codec.is_none() {
                tracing::error!("no HEVC decoder opens");
                while rx.recv().is_ok() {}
                return;
            }
        }
        let c = codec.as_mut().unwrap();
        match c.decode(&au, received) {
            Ok(Some(frame)) => {
                decoded_any = true;
                stats.push(frame.decoded - received);
                if stats.len() == 300 {
                    stats.sort();
                    tracing::debug!("decode median {:?}, p95 {:?}", stats[150], stats[285]);
                    stats.clear();
                }
                if let Ok(mut slot) = latest.frame.lock() {
                    *slot = Some(frame);
                }
                (latest.notify)();
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("{} failed: {e}", names[choice]);
                codec = None;
                // One that never produced a frame gives way to the next.
                if !decoded_any && choice + 1 < names.len() {
                    choice += 1;
                }
                wants.store(true, Ordering::Relaxed);
                skipping = true;
            }
        }
    }
}

const AV_PIX_FMT_YUV420P: c_int = 0;
const AV_PIX_FMT_YUVJ420P: c_int = 12;
const AV_PIX_FMT_NV12: c_int = 23;

struct Codec<'a> {
    av: &'a Av,
    ctx: *mut c_void,
    packet: *mut c_void,
    frame: *mut c_void,
}

impl<'a> Codec<'a> {
    fn open(av: &'a Av, name: &str) -> Result<Self, String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        // SAFETY: plain libavcodec calls on objects this struct owns and frees in Drop.
        unsafe {
            (av.log_set_level)(16); // AV_LOG_ERROR
            let codec = (av.find_decoder_by_name)(cname.as_ptr());
            if codec.is_null() {
                return Err("not built in".into());
            }
            let ctx = (av.alloc_context3)(codec);
            if ctx.is_null() {
                return Err("no context".into());
            }
            let set = |k: &CStr, v: &CStr| (av.opt_set)(ctx, k.as_ptr(), v.as_ptr(), 0);
            // cuvid holds four frames for display order unless low_delay is
            // set; the software decoder holds one per frame thread.
            set(c"flags", c"low_delay");
            if name == "hevc" {
                set(c"threads", c"4");
                set(c"thread_type", c"slice");
            }
            let mut me = Self { av, ctx, packet: (av.packet_alloc)(), frame: (av.frame_alloc)() };
            let r = (av.open2)(ctx, codec, std::ptr::null_mut());
            if r < 0 {
                return Err(format!("open failed ({r})"));
            }
            me.ctx = ctx;
            Ok(me)
        }
    }

    /// Decodes one access unit; the newest picture it yields, if any.
    fn decode(&mut self, au: &[u8], received: Instant) -> Result<Option<Frame>, String> {
        // SAFETY: the packet borrows `au` for the duration of send_packet,
        // which copies a packet that carries no buffer of its own.
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
            let mut out = None;
            loop {
                let r = (self.av.receive_frame)(self.ctx, self.frame);
                if r < 0 {
                    if r == EAGAIN {
                        break;
                    }
                    return Err(format!("receive_frame {r}"));
                }
                out = nv12(&*(self.frame as *const AvFrameHead), received);
            }
            Ok(out)
        }
    }
}

impl Drop for Codec<'_> {
    fn drop(&mut self) {
        // SAFETY: frees what `open` allocated; libav nulls the pointers.
        unsafe {
            (self.av.free_context)(&mut self.ctx);
            (self.av.packet_free)(&mut self.packet);
            (self.av.frame_free)(&mut self.frame);
        }
    }
}

/// Copies a decoded picture out as tight NV12.
///
/// # Safety
/// `f` is a frame libavcodec just returned, its planes valid for its format.
unsafe fn nv12(f: &AvFrameHead, received: Instant) -> Option<Frame> {
    let (w, h) = (f.width as usize, f.height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut data = vec![0u8; w * h + cw * ch * 2];
    let (luma, chroma) = data.split_at_mut(w * h);
    let row = |plane: usize, y: usize, len: usize| -> &[u8] {
        // SAFETY: inside the plane, per the caller's contract.
        unsafe { std::slice::from_raw_parts(f.data[plane].offset(y as isize * f.linesize[plane] as isize), len) }
    };
    for y in 0..h {
        luma[y * w..(y + 1) * w].copy_from_slice(row(0, y, w));
    }
    match f.format {
        AV_PIX_FMT_NV12 => {
            for y in 0..ch {
                chroma[y * cw * 2..(y + 1) * cw * 2].copy_from_slice(row(1, y, cw * 2));
            }
        }
        AV_PIX_FMT_YUV420P | AV_PIX_FMT_YUVJ420P => {
            for y in 0..ch {
                let (u, v) = (row(1, y, cw), row(2, y, cw));
                let out = &mut chroma[y * cw * 2..(y + 1) * cw * 2];
                for x in 0..cw {
                    out[2 * x] = u[x];
                    out[2 * x + 1] = v[x];
                }
            }
        }
        other => {
            tracing::warn!("unhandled pixel format {other}");
            return None;
        }
    }
    Some(Frame { width: w as u32, height: h as u32, data, received, decoded: Instant::now() })
}
