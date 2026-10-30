//! The wingpanel band's one reading of the wallpaper: the wallpaper decoded
//! small and cover-cropped to
//! the main output, the band's rows read back as a profile along it, three
//! numbers for the band and three for any stretch of it. Sampled on a
//! wallpaper or band change and never on a clock; the decode runs on the
//! pool.

use std::sync::{Mutex, OnceLock};

use fs_theme::barpaint::{self, Profile, SAMPLE_WIDTH, Stats};

use crate::runtime::Ctx;
use crate::store;

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub wallpaper: String,
    pub edge: &'static str,
    pub thickness: f64,
    pub screen: (f64, f64),
    pub output: String,
}

#[derive(Default)]
pub struct State {
    /// The output the reading was taken on, empty until one was.
    pub source: String,
    pub profile: Option<Profile>,
}

pub struct Diff(pub String, pub Option<Profile>);

impl State {
    pub fn apply(&mut self, Diff(source, profile): Diff) -> bool {
        if self.source == source && self.profile == profile {
            return false;
        }
        (self.source, self.profile) = (source, profile);
        true
    }

    /// The whole band's numbers, none until it was read.
    pub fn stats(&self) -> Option<Stats> {
        self.profile.as_ref().map(Profile::stats)
    }

    /// The numbers as `bar paint` reports them: an unread band is all zeroes.
    pub fn numbers(&self) -> Stats {
        self.stats().unwrap_or(Stats { mean: 0.0, std: 0.0, acutance: 0.0, sampled: false })
    }
}

static REQUESTS: OnceLock<async_channel::Sender<Request>> = OnceLock::new();
static SENT: Mutex<Option<Request>> = Mutex::new(None);

/// Dropped here when it repeats the last one sent: the bar asks on every
/// store topic, the minute's clock included, and a send wakes the service
/// thread whether or not there is anything to read.
pub fn sample(request: Request) {
    let Some(tx) = REQUESTS.get() else { return };
    let mut sent = SENT.lock().unwrap_or_else(|e| e.into_inner());
    if sent.as_ref() != Some(&request) && tx.try_send(request.clone()).is_ok() {
        *sent = Some(request);
    }
}

/// The band's samples off the decoded wallpaper, drawn the way
/// the desktop crops it: cover, centred, nearest-neighbour.
fn read(request: &Request) -> Option<Profile> {
    // A drawn field at the screen's own shape, so the cover crop below
    // keeps all of a sky's rows.
    let img = match super::wallpaper::generated(&request.wallpaper) {
        true => {
            let (sw, sh) = request.screen;
            let h = (SAMPLE_WIDTH as f64 * sh / sw.max(1.0)).round().clamp(1.0, 4096.0) as u32;
            super::wallpaper::image(&request.wallpaper, SAMPLE_WIDTH, h)?
        }
        false => image::open(&request.wallpaper).ok()?.to_rgba8(),
    };
    let (iw, ih) = (img.width() as f64, img.height() as f64);
    let (sw, sh) = request.screen;
    if iw <= 0.0 || ih <= 0.0 || sw <= 0.0 || sh <= 0.0 {
        return None;
    }
    let cw = SAMPLE_WIDTH as f64;
    let ch = (cw * sh / sw).round().max(1.0);
    let cover = (cw / iw).max(ch / ih);
    let (dw, dh) = (iw * cover, ih * cover);
    let (ox, oy) = ((cw - dw) / 2.0, (ch - dh) / 2.0);
    let band = barpaint::band_rect(request.edge, request.thickness, sw, sh, cw, ch);
    let (bw, bh) = (band.width as usize, band.height as usize);
    let mut data = Vec::with_capacity(bw * bh * 4);
    for y in 0..bh {
        for x in 0..bw {
            let cx = band.x + x as f64 + 0.5;
            let cy = band.y + y as f64 + 0.5;
            let px = (((cx - ox) / cover).floor() as i64).clamp(0, img.width() as i64 - 1) as u32;
            let py = (((cy - oy) / cover).floor() as i64).clamp(0, img.height() as i64 - 1) as u32;
            data.extend_from_slice(&img.get_pixel(px, py).0);
        }
    }
    let along_x = !matches!(request.edge, "left" | "right");
    Some(Profile::read(&data, bw, bh, along_x))
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded::<Request>();
    let _ = REQUESTS.set(tx);
    while let Ok(mut request) = rx.recv().await {
        while let Ok(newer) = rx.try_recv() {
            request = newer;
        }
        let output = request.output.clone();
        let profile = if request.wallpaper.is_empty() {
            None
        } else {
            ctx.pool().run(move || read(&request)).await.flatten()
        };
        ctx.publish(store::Diff::BarPaint(Diff(output, profile)));
    }
}
