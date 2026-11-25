mod audio;
mod av;
mod decode;
mod device;
mod model;
mod session;
mod ui;

use std::collections::BTreeSet;
use std::io::BufRead;
use std::sync::Arc;
use std::time::{Duration, Instant};

use idevice::core_device::RotationDirection;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("drive") => drive(),
        None => {
            if let Err(e) = ui::run() {
                tracing::error!("{e}");
                std::process::exit(1);
            }
        }
        _ => eprintln!("usage: formalshell-iphone-mirror [drive]"),
    }
}

/// Luma sampled on a coarse grid, to tell one picture from the next.
fn grid(frame: &decode::Frame) -> Vec<u8> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut out = Vec::with_capacity(48 * 96);
    for gy in 0..96 {
        for gx in 0..48 {
            out.push(frame.data[(gy * h / 96) * w + gx * w / 48]);
        }
    }
    out
}

fn differs(a: &[u8], b: &[u8]) -> bool {
    a.iter().zip(b).filter(|(x, y)| x.abs_diff(**y) > 12).count() > 8
}

/// Input to decoded picture: taps the home screen's Search pill and presses
/// Home in turn, timing each from the HID send to the first decoded frame
/// that differs from the one before it.
fn measure(latest: &decode::Latest, send: &dyn Fn(session::Cmd), rounds: usize) {
    let wait_still = || {
        // A frame at least 300ms after the screen stopped changing.
        let mut last = None;
        let mut still = Instant::now();
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if let Some(f) = latest.take() {
                let g = grid(&f);
                if last.as_ref().is_some_and(|l: &Vec<u8>| differs(l, &g)) {
                    still = Instant::now();
                }
                last = Some(g);
            }
            if still.elapsed() > Duration::from_millis(400) && last.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        last
    };
    let mut lat = Vec::new();
    for i in 0..rounds * 2 {
        let Some(base) = wait_still() else {
            eprintln!("measure: no frames");
            return;
        };
        let t0 = Instant::now();
        if i % 2 == 0 {
            send(session::Cmd::Touch(session::Phase::Down, 0.5, 0.42));
            send(session::Cmd::Touch(session::Phase::Up, 0.5, 0.42));
        } else {
            send(session::Cmd::Button(session::Button::Home));
        }
        let deadline = t0 + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(f) = latest.take()
                && differs(&base, &grid(&f))
            {
                eprintln!("{} {:?}", if i % 2 == 0 { "tap" } else { "home" }, f.received - t0);
                lat.push((f.decoded - t0, f.received - t0));
                break;
            }
            std::thread::sleep(Duration::from_micros(500));
        }
    }
    if lat.is_empty() {
        eprintln!("measure: the screen never changed");
        return;
    }
    let mut decoded: Vec<_> = lat.iter().map(|l| l.0).collect();
    let mut received: Vec<_> = lat.iter().map(|l| l.1).collect();
    decoded.sort();
    received.sort();
    let p = |v: &[Duration], q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    eprintln!(
        "input to decoded frame over {} changes: p50 {:?} p95 {:?} (received p50 {:?} p95 {:?})",
        decoded.len(),
        p(&decoded, 0.5),
        p(&decoded, 0.95),
        p(&received, 0.5),
        p(&received, 0.95)
    );
}

/// The phone with no window: commands on stdin, one per line (`tap x y`,
/// `drag x1 y1 x2 y2`, `down|move|up x y`, `home`, `switcher`, `lock`, `key
/// usage...`, `hold usage...`, `paste text`, `wait ms`, `shot path.png`,
/// `measure rounds`, `quit`), x and y in 0..1 of the portrait screen. For
/// checking the device layer on a real phone without putting anything on
/// screen.
fn drive() {
    let frames = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let count = frames.clone();
    let latest = decode::Latest::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    let decoder = decode::Decoder::start(latest.clone());
    let audio = Arc::new(audio::Audio::start());
    let session = session::spawn(|e| eprintln!("event: {e:?}"), decoder, audio);
    let start = Instant::now();
    let send = |c| {
        session.send(c);
    };
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let w: Vec<&str> = line.split_whitespace().collect();
        let f = |i: usize| w.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.5);
        match w.first().copied() {
            Some("tap") => {
                send(session::Cmd::Touch(session::Phase::Down, f(1), f(2)));
                std::thread::sleep(Duration::from_millis(60));
                send(session::Cmd::Touch(session::Phase::Up, f(1), f(2)));
            }
            Some("drag") => {
                let (x1, y1, x2, y2) = (f(1), f(2), f(3), f(4));
                send(session::Cmd::Touch(session::Phase::Down, x1, y1));
                for i in 1..=12 {
                    let t = i as f64 / 12.0;
                    std::thread::sleep(Duration::from_millis(16));
                    send(session::Cmd::Touch(session::Phase::Move, x1 + (x2 - x1) * t, y1 + (y2 - y1) * t));
                }
                send(session::Cmd::Touch(session::Phase::Up, x2, y2));
            }
            Some("down") => send(session::Cmd::Touch(session::Phase::Down, f(1), f(2))),
            Some("move") => send(session::Cmd::Touch(session::Phase::Move, f(1), f(2))),
            Some("up") => send(session::Cmd::Touch(session::Phase::Up, f(1), f(2))),
            Some("home") => send(session::Cmd::Button(session::Button::Home)),
            Some("switcher") => send(session::Cmd::Button(session::Button::AppSwitcher)),
            Some("rotate") => send(session::Cmd::Rotate(if w.get(1) == Some(&"left") { RotationDirection::Left } else { RotationDirection::Right })),
            Some("lock") => send(session::Cmd::Button(session::Button::Lock)),
            Some("key") => {
                let keys: BTreeSet<u8> = w[1..].iter().filter_map(|k| k.parse().ok()).collect();
                send(session::Cmd::Type(vec![keys]));
            }
            Some("hold") => send(session::Cmd::Keys(w[1..].iter().filter_map(|k| k.parse().ok()).collect())),
            Some("paste") => send(session::Cmd::Paste(line[6.min(line.len())..].to_string())),
            Some("wait") => std::thread::sleep(Duration::from_millis(f(1) as u64)),
            Some("shot") => {
                if let Some(frame) = latest.take() {
                    let path = w.get(1).copied().unwrap_or("shot.png");
                    let raw = format!("{path}.nv12");
                    let _ = std::fs::write(&raw, &frame.data);
                    let _ = std::process::Command::new("ffmpeg")
                        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "nv12", "-s"])
                        .arg(format!("{}x{}", frame.width, frame.height))
                        .args(["-i", &raw, "-vf", "scale=iw/2:ih/2", path])
                        .status();
                    let _ = std::fs::remove_file(&raw);
                    eprintln!("shot {path} ({}x{}, {:?} old)", frame.width, frame.height, frame.decoded.elapsed());
                } else {
                    eprintln!("shot: no new frame since the last one");
                }
            }
            Some("measure") => measure(&latest, &send, w.get(1).and_then(|n| n.parse().ok()).unwrap_or(10)),
            Some("quit") => break,
            _ => {}
        }
        eprintln!(
            "[{:.1}s] {line} (frames {})",
            start.elapsed().as_secs_f64(),
            frames.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
    send(session::Cmd::Release);
    session.close(Duration::from_secs(2));
}
