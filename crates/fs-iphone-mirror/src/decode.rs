//! HEVC to NV12 with the system's libavcodec, opened at run time (the
//! package links no codec and the distros ship different sonames):
//! NVDEC through `hevc_cuvid` where it opens, the software `hevc` decoder
//! where it does not. In process, because the ffmpeg CLI holds every frame
//! until the next one arrives, which on a still screen is forever.
//!
//! Decoded frames wait for the window in a queue of HELD: Wi-Fi hands
//! frames over in twos as often as singly, and showing only the newer of
//! a pair drops every other step of a scroll, where one a refresh shows
//! both and is caught up by the next gap. Past HELD the oldest goes, so a
//! slow present never builds delay. A stall's worth of units arriving at
//! once is all decoded, since each predicts from the last, and only its
//! last pictures are copied out. A decoder that cannot keep up,
//! with units waiting on it for BEHIND without a break, skips to the next
//! key frame and asks the phone for one, which takes the phone seconds to
//! send: the last resort, never the answer to a burst.

use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// Decoded frames a reader has yet to show, at most this many.
const HELD: usize = 2;

/// The decoded frames not yet shown, oldest first, and the hook that tells
/// their reader one landed.
pub struct Latest {
    frames: Mutex<VecDeque<Frame>>,
    /// Shown frames' buffers, for the next pictures: a fresh 4.5 MB one
    /// each frame is zeroed and paged in before it is written.
    spare: Mutex<Vec<Vec<u8>>>,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl Latest {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self { frames: Mutex::default(), spare: Mutex::default(), notify: Box::new(notify) })
    }

    /// Hands back the buffer of a frame that has been shown.
    pub fn recycle(&self, frame: Frame) {
        if let Ok(mut spare) = self.spare.lock()
            && spare.len() <= HELD
        {
            spare.push(frame.data);
        }
    }

    /// Whether a frame is waiting to be shown.
    pub fn more(&self) -> bool {
        self.frames.lock().is_ok_and(|f| !f.is_empty())
    }

    pub fn take(&self) -> Option<Frame> {
        self.frames.lock().ok()?.pop_front()
    }
}

/// Access units the queue holds, a bound on memory alone: about eight
/// seconds of picture.
const QUEUE: usize = 512;
/// A queue that has not been empty for this long means decoding is slower
/// than the stream: a burst, however long the stall before it, is decoded
/// through several times faster than it was sent.
const BEHIND: Duration = Duration::from_secs(2);

pub struct Decoder {
    tx: Option<SyncSender<(Vec<u8>, Instant)>>,
    /// Units pushed and not yet taken up by the decoder.
    queued: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Set when units were dropped: nothing decodes again until a key frame
    /// arrives, so the session asks the phone for one.
    pub wants_keyframe: Arc<AtomicBool>,
}

impl Decoder {
    pub fn start(latest: Arc<Latest>) -> Self {
        let (tx, rx) = sync_channel(QUEUE);
        let wants_keyframe = Arc::new(AtomicBool::new(false));
        let wants = wants_keyframe.clone();
        let queued = Arc::new(AtomicUsize::new(0));
        let left = queued.clone();
        let thread = std::thread::Builder::new()
            .name("mirror-decode".into())
            .spawn(move || run(rx, latest, wants, left))
            .expect("spawn decoder");
        Self { tx: Some(tx), queued, thread: Some(thread), wants_keyframe }
    }

    pub fn push(&self, au: Vec<u8>) {
        match self.tx.as_ref().map(|tx| tx.try_send((au, Instant::now()))) {
            Some(Ok(())) => {
                self.queued.fetch_add(1, Ordering::Relaxed);
            }
            Some(Err(TrySendError::Full(_))) => self.wants_keyframe.store(true, Ordering::Relaxed),
            _ => {}
        }
    }
}

/// The decoder finishes its unit before the process goes: libc's exit
/// handlers tear CUDA down under a decode in flight, which crashes.
impl Drop for Decoder {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn is_keyframe(au: &[u8]) -> bool {
    au.windows(4)
        .any(|w| w[0] == 0 && w[1] == 0 && w[2] == 1 && (16..=23).contains(&((w[3] >> 1) & 0x3f)))
}

fn run(rx: Receiver<(Vec<u8>, Instant)>, latest: Arc<Latest>, wants: Arc<AtomicBool>, queued: Arc<AtomicUsize>) {
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
    let mut waiting: Option<(Vec<u8>, Instant)> = None;
    let mut behind: Option<Instant> = None;
    while let Some((au, received)) = waiting.take().or_else(|| rx.recv().ok()) {
        waiting = rx.try_recv().ok();
        // The units still behind this one.
        let left = queued.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
        let key = is_keyframe(&au);
        if waiting.is_none() {
            behind = None;
        } else if behind.get_or_insert_with(Instant::now).elapsed() > BEHIND {
            wants.store(true, Ordering::Relaxed);
            behind = None;
        }
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
        // A unit with more behind it than the window would show is decoded
        // for the next to predict from; its picture is never shown.
        let show = (left < HELD).then(|| latest.spare.lock().ok().and_then(|mut s| s.pop()).unwrap_or_default());
        match c.decode(&au, received, show) {
            Ok(Some(frame)) => {
                decoded_any = true;
                stats.push(frame.decoded - received);
                if stats.len() == 300 {
                    stats.sort();
                    tracing::debug!("decode median {:?}, p95 {:?}", stats[150], stats[285]);
                    stats.clear();
                }
                let unseen = latest.frames.lock().ok().and_then(|mut frames| {
                    frames.push_back(frame);
                    (frames.len() > HELD).then(|| frames.pop_front()).flatten()
                });
                if let Some(unseen) = unseen {
                    latest.recycle(unseen);
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

    /// Decodes one access unit; the newest picture it yields, if any, copied
    /// into `show`'s buffer where there is one to show it in.
    fn decode(&mut self, au: &[u8], received: Instant, mut show: Option<Vec<u8>>) -> Result<Option<Frame>, String> {
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
                // One unit can yield two pictures; the later one reuses the buffer.
                if let Some(data) = out.take().map(|f: Frame| f.data).or_else(|| show.take()) {
                    out = nv12(&*(self.frame as *const AvFrameHead), received, data);
                }
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

/// Copies a decoded picture out as tight NV12, into `data`.
///
/// # Safety
/// `f` is a frame libavcodec just returned, its planes valid for its format.
unsafe fn nv12(f: &AvFrameHead, received: Instant, mut data: Vec<u8>) -> Option<Frame> {
    let (w, h) = (f.width as usize, f.height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    data.resize(w * h + cw * ch * 2, 0);
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
