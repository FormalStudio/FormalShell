//! Logic behind the two capture-side IPC targets: `capture` (pull something
//! off the screen into the clipboard) and `record` (video). No clock reads and
//! no IO: every function takes its inputs and returns a value.
//!
//! The argv builders return plain vectors that go straight onto a child
//! process with no shell in between, so a recording directory carrying a space
//! or a quote can never splice a command.

use fs_js as js;
use chrono::{Datelike, NaiveDateTime, Timelike};
use regex::Regex;
use std::sync::LazyLock;

/// Transient PipeWire/Pulse sink the desktop+mic mode mixes both sources into.
/// Fixed name, created and torn down inside one recording's lifetime; never
/// written to state.json.
pub const MIX_SINK: &str = "formalshell-mix";

/// "YYYYMMDD-HHMMSS", every field zero-padded.
pub fn timestamp(date: &NaiveDateTime) -> String {
    format!(
        "{}{:02}{:02}-{:02}{:02}{:02}",
        date.year(),
        date.month(),
        date.day(),
        date.hour(),
        date.minute(),
        date.second()
    )
}

pub fn output_path(dir: &str, prefix: &str, ext: &str, date: &NaiveDateTime) -> String {
    let base = dir.trim_end_matches('/');
    format!("{base}/{prefix}-{}.{ext}", timestamp(date))
}

/// The path without its extension. A leading dot after the last slash is a
/// dotfile, not an extension, so it survives.
fn stem(source: &str) -> &str {
    let slash = source.rfind('/').map_or(-1, |i| i as isize);
    match source.rfind('.') {
        Some(dot) if dot as isize > slash + 1 => &source[..dot],
        _ => source,
    }
}

/// The GIF lands next to its source rather than in recording.directory: the
/// daily case is an mp4 someone sent you sitting in ~/Downloads, and moving the
/// result somewhere else is friction rather than less of it.
pub fn gif_output_path(source: &str) -> String {
    if source.is_empty() {
        return String::new();
    }
    format!("{}.gif", stem(source))
}

static GEOMETRY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(-?[0-9]+),(-?[0-9]+)\s+([0-9]+)x([0-9]+)$").unwrap());

/// slurp's own default output format ("%x,%y %wx%h"), the same string grim and
/// wf-recorder both take for -g. Negative origins are legal on a multi-output
/// layout; a zero-width or zero-height box is never a geometry.
pub fn parse_geometry(text: &str) -> String {
    let Some(m) = GEOMETRY_RE.captures(js::trim(text)) else {
        return String::new();
    };
    if js::parse_number(&m[3]) <= 0.0 || js::parse_number(&m[4]) <= 0.0 {
        return String::new();
    }
    format!("{},{} {}x{}", &m[1], &m[2], &m[3], &m[4])
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `od -An -tu1` over the three trailing bytes of a 1x1 `grim -t ppm`. grim's
/// P6 writer emits "P6\n<W> <H>\n255\n" followed by W*H*3 native RGB bytes and
/// no trailing newline, so the last three bytes are the last pixel, in R,G,B
/// order and already alpha-free. On a fractionally scaled output a 1x1 logical
/// region renders as more than one device pixel, and the last one is still
/// inside the region the user picked, so the trailing-bytes read stays correct
/// there too.
pub fn hex_from_ppm_bytes(text: &str) -> String {
    let raw = js::trim(text);
    if raw.is_empty() {
        return String::new();
    }
    let parts = js::split_ws(raw);
    if parts.len() != 3 {
        return String::new();
    }
    let mut out = String::from("#");
    for part in parts {
        if !all_digits(part) {
            return String::new();
        }
        let v = js::parse_number(part);
        if v > 255.0 {
            return String::new();
        }
        out.push_str(&format!("{:02X}", v as u8));
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSetup {
    pub modules: Vec<f64>,
    pub device: String,
}

/// The audio-setup script's two-line answer: the pactl module ids it loaded
/// (blank when it loaded none) on the first line, the device wf-recorder should
/// record on the second. Anything else is a failed setup, reported as an empty
/// device so the caller never starts a recording that silently captures
/// nothing.
pub fn parse_audio_setup(text: &str) -> AudioSetup {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut ids = Vec::new();
    let raw = js::trim(lines.first().copied().unwrap_or(""));
    if !raw.is_empty() {
        for part in js::split_ws(raw) {
            if !all_digits(part) {
                return AudioSetup { modules: Vec::new(), device: String::new() };
            }
            ids.push(js::parse_number(part));
        }
    }
    let device = js::trim(lines.get(1).copied().unwrap_or("")).to_string();
    AudioSetup { modules: ids, device }
}

#[derive(Debug, Clone, Default)]
pub struct RecorderOpts {
    pub path: String,
    pub framerate: f64,
    pub output: String,
    pub geometry: String,
    pub codec: String,
    pub pixel_format: String,
    pub max_height: Option<f64>,
    pub no_dmabuf: bool,
    pub audio_device: String,
    pub audio_backend: String,
}

/// wf-recorder v0.6 argv, read off manpage/wf-recorder.1 and src/main.cpp's
/// getopt table.
///
/// `-y` is mandatory on every single invocation, not a convenience: without it
/// main.cpp's user_specified_overwrite() does a blocking std::getline(std::cin)
/// the moment the target file already exists, and a child spawned by the shell
/// gets a stdin pipe that never reaches EOF.
///
/// `--audio` is declared optional_argument, so the device has to ride the same
/// argv element. A separate element would be left as a stray positional and
/// the device silently dropped, recording the default source instead.
/// `--audio-backend` is required_argument and long-form only, so it takes the
/// ordinary two-element form.
pub fn recorder_argv(opts: &RecorderOpts) -> Vec<String> {
    let mut argv: Vec<String> = vec!["wf-recorder".into(), "-y".into(), "-f".into(), opts.path.clone()];
    if opts.framerate > 0.0 {
        argv.extend(["-r".to_string(), js::num_str(opts.framerate)]);
    }
    if !opts.output.is_empty() {
        argv.extend(["-o".to_string(), opts.output.clone()]);
    }
    if !opts.geometry.is_empty() {
        argv.extend(["-g".to_string(), opts.geometry.clone()]);
    }
    if !opts.codec.is_empty() {
        argv.extend(["-c".to_string(), opts.codec.clone()]);
    }
    if !opts.pixel_format.is_empty() {
        argv.extend(["-x".to_string(), opts.pixel_format.clone()]);
    }
    // MPEG-4 part 2's default 200 kb/s smears text.
    if opts.codec == "mpeg4" {
        argv.extend(["-p".to_string(), "b=8M".into()]);
    }
    let filter = scale_cap_filter(opts.max_height.unwrap_or(f64::NAN));
    if !filter.is_empty() {
        argv.extend(["-F".to_string(), filter]);
    }
    if opts.no_dmabuf {
        argv.push("--no-dmabuf".into());
    }
    if !opts.audio_device.is_empty() {
        argv.push(format!("--audio={}", opts.audio_device));
        if !opts.audio_backend.is_empty() {
            argv.extend(["--audio-backend".to_string(), opts.audio_backend.clone()]);
        }
    }
    argv
}

/// wf-recorder's -F/--filter passes straight into ffmpeg's own filtergraph
/// parser, so an ordinary scale expression works with no shell in between to
/// escape it. -2 asks ffmpeg to compute the width from the chosen height,
/// rounded to an even number (most encoders require one); min(ih,H) leaves the
/// height untouched under the cap and clamps it to H otherwise, so nothing here
/// has to know the captured height in advance. Single-quoted per ffmpeg's own
/// filtergraph escaping, since the comma inside min(...) would otherwise read
/// as the end of the -F value.
pub fn scale_cap_filter(max_height: f64) -> String {
    if max_height.is_nan() || max_height <= 0.0 {
        return String::new();
    }
    format!("scale=-2:'min(ih,{})'", js::num_str(max_height.floor()))
}

/// The record IPC's own maxHeight override: "" defers to recording.maxHeight,
/// and 0 is itself the legal "uncapped, regardless of what config says" answer.
/// Anything else must be a plain non-negative integer; NaN signals "not a
/// number", which the caller turns into an IPC error string rather than
/// silently falling back to the config default and hiding a typo.
pub fn resolve_max_height(arg: Option<&str>, config_default: f64) -> f64 {
    let raw = js::trim(arg.unwrap_or(""));
    if raw.is_empty() {
        return config_default;
    }
    if !all_digits(raw) {
        return f64::NAN;
    }
    js::parse_number(raw)
}

#[derive(Debug, Clone, Copy)]
pub struct GifOpts {
    pub fps: f64,
    pub width: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GifArgv {
    pub pass1: Vec<String>,
    pub pass2: Vec<String>,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Two-pass palettegen/paletteuse. `-f image2 -update 1` is what makes pass one
/// write a single-frame palette PNG instead of a numbered sequence.
/// dither=bayer is the shell's one sanctioned texture (DESIGN.md), and bayer_scale
/// sits at 3 rather than 0 for the chunky look.
pub fn gif_argv(in_path: &str, palette_path: &str, out_path: &str, opts: GifOpts) -> GifArgv {
    let chain = format!("fps={},scale={}:-1:flags=lanczos", js::num_str(opts.fps), js::num_str(opts.width));
    GifArgv {
        pass1: strings(&[
            "ffmpeg",
            "-y",
            "-loglevel",
            "error",
            "-i",
            in_path,
            "-vf",
            &format!("{chain},palettegen=stats_mode=diff"),
            "-f",
            "image2",
            "-update",
            "1",
            palette_path,
        ]),
        pass2: strings(&[
            "ffmpeg",
            "-y",
            "-loglevel",
            "error",
            "-i",
            in_path,
            "-i",
            palette_path,
            "-lavfi",
            &format!("{chain} [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle"),
            "-loop",
            "0",
            out_path,
        ]),
    }
}

/// "MM:SS" under an hour, "H:MM:SS" past it.
pub fn elapsed_label(ms: f64) -> String {
    let total = (ms / 1000.0).floor().max(0.0);
    let s = total % 60.0;
    let m = (total / 60.0).floor() % 60.0;
    let h = (total / 3600.0).floor();
    let (s, m, h) = (s as u64, m as u64, h as u64);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrOutcome {
    Ok,
    Empty,
    Failed,
}

impl OcrOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            OcrOutcome::Ok => "ok",
            OcrOutcome::Empty => "empty",
            OcrOutcome::Failed => "failed",
        }
    }
}

/// The OCR pipeline's own exit codes, not tesseract's. 3 means the region held
/// no text (a real outcome, with no clipboard write and no error to report),
/// while 2 means one of the four commands in the chain actually failed.
pub fn ocr_outcome(exit_code: i32) -> OcrOutcome {
    match exit_code {
        0 => OcrOutcome::Ok,
        3 => OcrOutcome::Empty,
        _ => OcrOutcome::Failed,
    }
}

// finalize_recording's own trim (omarchy's bin/omarchy-capture-screenrecording,
// MIT): PipeWire emits a click at the start of every captured stream, so the
// first 0.1s is always cut. Pure functions rather than one shell script so the
// reencode/audio decisions are directly testable.

/// ffprobe -select_streams v:0 -read_intervals %+0.2 -show_entries packet=flags
/// -of csv=p=0: one flag string per packet in the first 0.2s. A "D" (discard)
/// flag is what a stream copy can't trim past (it rewinds to the keyframe
/// instead), so its presence is what forces a re-encode.
pub fn finalize_needs_reencode(packet_flags_text: &str) -> bool {
    packet_flags_text.contains('D')
}

/// ffprobe -select_streams a -show_entries stream=codec_type -of csv=p=0: one
/// "audio" line per audio stream, nothing at all when there is none.
pub fn finalize_has_audio(stream_types_text: &str) -> bool {
    stream_types_text.contains("audio")
}

/// Sits next to its source, the same dotfile-safe stem logic as
/// `gif_output_path`.
/// A script printing the first of x264, OpenH264 and MPEG-4 part 2 this
/// ffmpeg can open, all of which mp4 holds. The `-encoders` list is not
/// enough: openSUSE ships noopenh264, a stub that is listed and fails to
/// open. Printing nothing leaves wf-recorder its own default, which openSUSE
/// patches to utvideo in gbrp, which mp4 cannot hold either.
pub const CODEC_PROBE: &str = r#"for c in libx264 libopenh264 mpeg4; do ffmpeg -v quiet -f lavfi -i color=s=64x64 -frames:v 1 -pix_fmt yuv420p -c:v "$c" -f null - && { echo "$c"; exit 0; }; done"#;

pub fn finalize_output_path(source: &str) -> String {
    if source.is_empty() {
        return String::new();
    }
    format!("{}-processed.mp4", stem(source))
}

/// `-ss` before `-i` trims frame-accurately on the reencode path and at the
/// nearest keyframe on the copy path, both upstream's own command. The audio
/// filter chain hard-mutes the first 400ms (the PipeWire capture-open pop a
/// fade alone can't attenuate enough), fades the next 50ms, then normalizes the
/// rest to -14 LUFS.
pub fn finalize_argv(in_path: &str, out_path: &str, reencode: Option<&str>, has_audio: bool) -> Vec<String> {
    let mut argv = strings(&["ffmpeg", "-y", "-ss", "0.1", "-i", in_path]);
    match reencode {
        Some("libx264" | "") => argv.extend(strings(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"])),
        Some(codec) => argv.extend(strings(&["-c:v", codec, "-b:v", "8M"])),
        None => argv.extend(strings(&["-c:v", "copy"])),
    }
    if has_audio {
        argv.extend(strings(&[
            "-af",
            "volume=enable='lt(t,0.4)':volume=0,afade=t=in:st=0.4:d=0.05,loudnorm=I=-14:TP=-1.5:LRA=11",
        ]));
    }
    argv.push(out_path.to_string());
    argv
}

/// The SAVED toast's own thumbnail. Same dotfile-safe stem logic as
/// `gif_output_path`/`finalize_output_path`.
pub fn preview_frame_path(source: &str) -> String {
    if source.is_empty() {
        return String::new();
    }
    format!("{}-preview.png", stem(source))
}

/// One frame at 0.1s in, the same offset `finalize_argv` trims to, so the
/// thumbnail matches what the finalized file actually opens on, at upstream's
/// own -q:v 2.
pub fn preview_frame_argv(source: &str, out_path: &str) -> Vec<String> {
    strings(&["ffmpeg", "-y", "-loglevel", "error", "-ss", "0.1", "-i", source, "-vframes", "1", "-q:v", "2", out_path])
}

/// Webcam overlay: mpv's own low-latency profile and 8:9 portrait crop, given
/// its own title/app-id so the compositor placement (and the map poll that
/// precedes it) can find exactly this window and nothing else mpv might already
/// have open. No forced `--demuxer-lavf-o video_size`: the crop filter derives
/// its width from whatever height the device actually opens at, so nothing here
/// needs a v4l2-ctl format probe first.
pub fn webcam_argv(device: &str, app_id: &str) -> Vec<String> {
    vec![
        "mpv".into(),
        format!("av://v4l2:{device}"),
        "--profile=low-latency".into(),
        "--untimed".into(),
        "--no-cache".into(),
        "--vf=lavfi=[crop=ih*8/9:ih]".into(),
        format!("--title={app_id}"),
        format!("--wayland-app-id={app_id}"),
        "--no-border".into(),
        "--no-audio".into(),
        "--no-osc".into(),
        "--osd-level=0".into(),
        "--really-quiet".into(),
    ]
}

/// `for f in /dev/video*; do [ -e "$f" ] && echo "$f"; done` output: one path
/// per line, in the shell's own glob order, empty when the glob matched
/// nothing. The auto-detect pick is the first line: no v4l2-ctl capability
/// filter, since that would pull in a second CLI dependency.
pub fn parse_webcam_devices(text: &str) -> Vec<String> {
    text.split('\n').map(js::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// 50ms x 100 = 5s to wait for mpv's window to map before giving up honestly:
/// a 2s budget was too tight for real v4l2 init on some USB cameras, and mpv is
/// already spawned by the time the code gives up, so a late map became an
/// untracked window tiled across the recording forever. `give_up_attempts` is
/// when the WEBCAM UNAVAILABLE notification fires and the recording launches
/// without the camera; `reap_attempts` is how much further the same poll keeps
/// watching afterward so a straggler window that maps just past the timeout
/// gets closed the instant it appears instead of leaking.
pub const WEBCAM_MAP_GIVEUP_ATTEMPTS: u32 = 100;
pub const WEBCAM_MAP_REAP_ATTEMPTS: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapPollAction {
    Reap,
    Place,
    GiveUp,
    Stop,
    Wait,
}

impl MapPollAction {
    pub fn as_str(self) -> &'static str {
        match self {
            MapPollAction::Reap => "reap",
            MapPollAction::Place => "place",
            MapPollAction::GiveUp => "give-up",
            MapPollAction::Stop => "stop",
            MapPollAction::Wait => "wait",
        }
    }
}

pub fn webcam_map_poll_action(
    found: bool,
    attempts: u32,
    gave_up: bool,
    give_up_attempts: u32,
    reap_attempts: u32,
) -> MapPollAction {
    if found {
        return if gave_up { MapPollAction::Reap } else { MapPollAction::Place };
    }
    if !gave_up && attempts >= give_up_attempts {
        return MapPollAction::GiveUp;
    }
    if gave_up && attempts >= reap_attempts {
        return MapPollAction::Stop;
    }
    MapPollAction::Wait
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WebcamGeometry {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

const HEIGHT_FRACTION_SMALL: f64 = 9.0 / 50.0;
const HEIGHT_FRACTION_MEDIUM: f64 = 1.0 / 4.0;
const HEIGHT_FRACTION_LARGE: f64 = 27.0 / 80.0;

/// The 8:9 portrait presets, scaled from the captured region's own height so
/// the camera occupies the same share of the frame at 1080p as at a scaled 6K
/// capture (omarchy's omarchy-capture-webcam-resize, MIT, ported as intent, not
/// its integer-bash arithmetic). `large`'s width is what a tall, narrow region
/// can't fit at full scale, so the height every preset scales from is capped to
/// what the region's own width (minus two margins) allows the widest preset to
/// reach.
pub fn webcam_geometry(size_name: &str, region: Region, margin: f64) -> WebcamGeometry {
    let frac = match size_name {
        "small" => HEIGHT_FRACTION_SMALL,
        "large" => HEIGHT_FRACTION_LARGE,
        _ => HEIGHT_FRACTION_MEDIUM,
    };
    let mut scale_height = region.height;
    let available_width = region.width - 2.0 * margin;
    let large_width = scale_height * HEIGHT_FRACTION_LARGE * 8.0 / 9.0;
    if available_width > 0.0 && large_width > available_width {
        scale_height = available_width * 10.0 / 3.0;
    }
    let height = js::round(scale_height * frac).max(1.0);
    let width = js::round(height * 8.0 / 9.0).max(1.0);
    let mut x = region.x + region.width - width - margin;
    let mut y = region.y + region.height - height - margin;
    if x < region.x + margin {
        x = region.x + margin;
    }
    if y < region.y + margin {
        y = region.y + margin;
    }
    WebcamGeometry { width, height, x: js::round(x), y: js::round(y) }
}

/// The "X,Y WxH" geometry a region/window recording already resolved, back into
/// the region `webcam_geometry` needs: `parse_geometry`'s own shape, minus the
/// round trip through a string.
pub fn region_from_geometry(geometry: &str) -> Option<Region> {
    let m = GEOMETRY_RE.captures(js::trim(geometry))?;
    Some(Region { x: js::parse_number(&m[1]), y: js::parse_number(&m[2]), width: js::parse_number(&m[3]), height: js::parse_number(&m[4]) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, s).unwrap()
    }

    fn at(argv: &[String], flag: &str) -> String {
        let i = argv.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} missing"));
        argv[i + 1].clone()
    }

    fn has(argv: &[String], flag: &str) -> bool {
        argv.iter().any(|a| a == flag)
    }

    #[test]
    fn timestamp_zero_pads_every_field() {
        assert_eq!(timestamp(&date(2026, 8, 11, 6, 5, 4)), "20260811-060504");
        assert_eq!(timestamp(&date(2026, 12, 31, 23, 59, 59)), "20261231-235959");
    }

    #[test]
    fn output_path_composes_one_slash_and_the_extension() {
        let d = date(2026, 8, 11, 6, 5, 4);
        assert_eq!(output_path("/home/k/Videos", "screenrecording", "mp4", &d), "/home/k/Videos/screenrecording-20260811-060504.mp4");
        assert_eq!(output_path("/home/k/Videos/", "screenrecording", "mp4", &d), "/home/k/Videos/screenrecording-20260811-060504.mp4");
    }

    #[test]
    fn gif_output_path_sits_next_to_its_source() {
        assert_eq!(gif_output_path("/a/b/clip.mp4"), "/a/b/clip.gif");
        assert_eq!(gif_output_path("/a/b/clip"), "/a/b/clip.gif");
        assert_eq!(gif_output_path("/a/b.d/clip.mp4"), "/a/b.d/clip.gif");
        assert_eq!(gif_output_path("/a/b.d/clip"), "/a/b.d/clip.gif");
        assert_eq!(gif_output_path(""), "");
    }

    #[test]
    fn parse_geometry_accepts_slurps_own_format() {
        assert_eq!(parse_geometry("10,-5 800x600"), "10,-5 800x600");
        assert_eq!(parse_geometry("  0,0 1920x1080\n"), "0,0 1920x1080");
        assert_eq!(parse_geometry("640,360 1x1"), "640,360 1x1");
    }

    #[test]
    fn parse_geometry_rejects_degenerate_and_malformed_boxes() {
        assert_eq!(parse_geometry("0,0 0x10"), "");
        assert_eq!(parse_geometry("0,0 10x0"), "");
        assert_eq!(parse_geometry(""), "");
        assert_eq!(parse_geometry("not a geometry"), "");
        assert_eq!(parse_geometry("0,0 800x600 extra"), "");
    }

    #[test]
    fn hex_from_ppm_bytes_formats_uppercase_rrggbb() {
        assert_eq!(hex_from_ppm_bytes("  67 133 190\n"), "#4385BE");
        assert_eq!(hex_from_ppm_bytes("0 0 0"), "#000000");
        assert_eq!(hex_from_ppm_bytes("255 255 255"), "#FFFFFF");
        assert_eq!(hex_from_ppm_bytes("1 2 3"), "#010203");
    }

    #[test]
    fn hex_from_ppm_bytes_rejects_anything_that_is_not_three_bytes() {
        assert_eq!(hex_from_ppm_bytes(""), "");
        assert_eq!(hex_from_ppm_bytes("67 133"), "");
        assert_eq!(hex_from_ppm_bytes("67 133 190 255"), "");
        assert_eq!(hex_from_ppm_bytes("67 133 999"), "");
        assert_eq!(hex_from_ppm_bytes("grim: failed to capture"), "");
    }

    #[test]
    fn parse_audio_setup_reads_modules_then_device() {
        let mic = parse_audio_setup("41 42 43\nformalshell-mix.monitor\n");
        assert_eq!(mic.modules.len(), 3);
        assert_eq!(mic.modules[0], 41.0);
        assert_eq!(mic.modules[2], 43.0);
        assert_eq!(mic.device, "formalshell-mix.monitor");

        let desktop = parse_audio_setup("\nalsa_output.pci-0000_00_1f.3.analog-stereo.monitor\n");
        assert_eq!(desktop.modules.len(), 0);
        assert_eq!(desktop.device, "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor");
    }

    #[test]
    fn parse_audio_setup_reports_no_device_on_a_failed_setup() {
        assert_eq!(parse_audio_setup("").device, "");
        assert_eq!(parse_audio_setup("\n").device, "");
        assert_eq!(parse_audio_setup("41 42 43\n").device, "");
        let broken = parse_audio_setup("41 oops\nformalshell-mix.monitor\n");
        assert_eq!(broken.modules.len(), 0);
        assert_eq!(broken.device, "");
    }

    fn opts(f: impl FnOnce(&mut RecorderOpts)) -> RecorderOpts {
        let mut o = RecorderOpts { path: "/v/a.mp4".into(), framerate: 30.0, output: "DP-1".into(), ..Default::default() };
        f(&mut o);
        o
    }

    #[test]
    fn recorder_argv_always_forces_overwrite() {
        let argv = recorder_argv(&opts(|_| {}));
        assert!(has(&argv, "-y"));
        assert_eq!(argv[0], "wf-recorder");
        assert_eq!(at(&argv, "-f"), "/v/a.mp4");
        assert_eq!(at(&argv, "-r"), "30");
        assert_eq!(at(&argv, "-o"), "DP-1");
    }

    #[test]
    fn recorder_argv_attaches_the_audio_device_to_one_element() {
        let argv = recorder_argv(&opts(|o| {
            o.audio_device = "formalshell-mix.monitor".into();
            o.audio_backend = "pulse".into();
        }));
        assert!(!has(&argv, "-a"));
        assert!(!has(&argv, "formalshell-mix.monitor"));
        assert!(has(&argv, "--audio=formalshell-mix.monitor"));
        assert_eq!(at(&argv, "--audio-backend"), "pulse");
    }

    #[test]
    fn recorder_argv_emits_no_audio_flag_without_a_device() {
        let argv = recorder_argv(&opts(|_| {}));
        assert!(argv.iter().all(|a| !a.starts_with("--audio")));
        assert!(!has(&argv, "--audio-backend"));
    }

    #[test]
    fn recorder_argv_emits_optional_flags_only_when_asked() {
        let bare = recorder_argv(&opts(|o| {
            o.framerate = 0.0;
            o.output = String::new();
        }));
        for flag in ["-g", "-o", "-r", "-c", "--no-dmabuf"] {
            assert!(!has(&bare, flag), "{flag}");
        }
        let full = recorder_argv(&opts(|o| {
            o.framerate = 15.0;
            o.output = "winit-0".into();
            o.geometry = "0,0 800x600".into();
            o.codec = "libx264".into();
            o.no_dmabuf = true;
        }));
        assert_eq!(at(&full, "-g"), "0,0 800x600");
        assert_eq!(at(&full, "-c"), "libx264");
        assert!(has(&full, "--no-dmabuf"));
    }

    #[test]
    fn scale_cap_filter_is_empty_without_a_cap() {
        assert_eq!(scale_cap_filter(0.0), "");
        assert_eq!(scale_cap_filter(-5.0), "");
        assert_eq!(scale_cap_filter(f64::NAN), "");
    }

    #[test]
    fn scale_cap_filter_clamps_height_and_keeps_width_even() {
        assert_eq!(scale_cap_filter(720.0), "scale=-2:'min(ih,720)'");
        assert_eq!(scale_cap_filter(js::parse_number("1080")), "scale=-2:'min(ih,1080)'");
        assert_eq!(scale_cap_filter(480.9), "scale=-2:'min(ih,480)'");
    }

    #[test]
    fn recorder_argv_adds_the_scale_filter_only_when_capped() {
        assert!(!has(&recorder_argv(&opts(|_| {})), "-F"));
        let capped = recorder_argv(&opts(|o| o.max_height = Some(720.0)));
        assert_eq!(at(&capped, "-F"), "scale=-2:'min(ih,720)'");
    }

    #[test]
    fn resolve_max_height_defers_to_the_config_default_on_empty() {
        assert_eq!(resolve_max_height(Some(""), 720.0), 720.0);
        assert_eq!(resolve_max_height(None, 720.0), 720.0);
        assert_eq!(resolve_max_height(None, 0.0), 0.0);
        assert_eq!(resolve_max_height(Some("  "), 720.0), 720.0);
    }

    #[test]
    fn resolve_max_height_accepts_a_plain_non_negative_integer() {
        assert_eq!(resolve_max_height(Some("480"), 720.0), 480.0);
        assert_eq!(resolve_max_height(Some("0"), 720.0), 0.0);
    }

    #[test]
    fn resolve_max_height_rejects_anything_that_is_not_a_number() {
        for bad in ["tall", "-5", "720p", "7.5"] {
            assert!(resolve_max_height(Some(bad), 0.0).is_nan(), "{bad}");
        }
    }

    #[test]
    fn gif_argv_pass_one_writes_a_single_frame_palette() {
        let a = gif_argv("/v/a.mp4", "/run/pal.png", "/v/a.gif", GifOpts { fps: 12.0, width: 640.0 });
        assert_eq!(at(&a.pass1, "-f"), "image2");
        assert_eq!(at(&a.pass1, "-update"), "1");
        assert_eq!(a.pass1.last().unwrap(), "/run/pal.png");
        let vf = at(&a.pass1, "-vf");
        assert!(vf.contains("fps=12"));
        assert!(vf.contains("scale=640:-1"));
        assert!(vf.contains("palettegen=stats_mode=diff"));
    }

    #[test]
    fn gif_argv_pass_two_reads_the_palette_as_its_second_input() {
        let a = gif_argv("/v/a.mp4", "/run/pal.png", "/v/a.gif", GifOpts { fps: 8.0, width: 320.0 });
        let inputs: Vec<&String> = a.pass2.iter().enumerate().filter(|(_, v)| *v == "-i").map(|(i, _)| &a.pass2[i + 1]).collect();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0], "/v/a.mp4");
        assert_eq!(inputs[1], "/run/pal.png");
        assert_eq!(a.pass2.last().unwrap(), "/v/a.gif");
        let lavfi = at(&a.pass2, "-lavfi");
        assert!(lavfi.contains("[x][1:v] paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle"));
        assert!(lavfi.contains("fps=8"));
    }

    #[test]
    fn elapsed_label_grows_an_hour_field_only_when_needed() {
        assert_eq!(elapsed_label(0.0), "00:00");
        assert_eq!(elapsed_label(42000.0), "00:42");
        assert_eq!(elapsed_label(599000.0), "09:59");
        assert_eq!(elapsed_label(3600000.0), "1:00:00");
        assert_eq!(elapsed_label(3661000.0), "1:01:01");
        assert_eq!(elapsed_label(-500.0), "00:00");
    }

    #[test]
    fn ocr_outcome_separates_empty_from_failed() {
        assert_eq!(ocr_outcome(0).as_str(), "ok");
        assert_eq!(ocr_outcome(3).as_str(), "empty");
        assert_eq!(ocr_outcome(2).as_str(), "failed");
        assert_eq!(ocr_outcome(1).as_str(), "failed");
        assert_eq!(ocr_outcome(127).as_str(), "failed");
    }

    #[test]
    fn finalize_needs_reencode_reads_the_discard_flag() {
        assert!(finalize_needs_reencode("K_\nD_\n__\n"));
        assert!(!finalize_needs_reencode("K_\n__\n__\n"));
        assert!(!finalize_needs_reencode(""));
    }

    #[test]
    fn finalize_has_audio_reads_the_stream_list() {
        assert!(finalize_has_audio("audio\n"));
        assert!(!finalize_has_audio(""));
        assert!(!finalize_has_audio("\n"));
    }

    #[test]
    fn finalize_output_path_sits_next_to_its_source() {
        assert_eq!(finalize_output_path("/v/a.mp4"), "/v/a-processed.mp4");
        assert_eq!(finalize_output_path("/v/b.d/a.mp4"), "/v/b.d/a-processed.mp4");
        assert_eq!(finalize_output_path(""), "");
    }

    #[test]
    fn finalize_argv_always_trims_the_first_tenth_of_a_second() {
        let argv = finalize_argv("/v/a.mp4", "/v/a-processed.mp4", None, false);
        assert_eq!(at(&argv, "-ss"), "0.1");
        assert!(!has(&argv, "-af"));
        assert_eq!(at(&argv, "-c:v"), "copy");
        assert_eq!(argv.last().unwrap(), "/v/a-processed.mp4");
    }

    #[test]
    fn finalize_argv_reencodes_only_when_the_gop_needs_it() {
        let argv = finalize_argv("/v/a.mp4", "/v/out.mp4", Some("libx264"), false);
        assert_eq!(at(&argv, "-c:v"), "libx264");
        assert!(has(&argv, "-preset"));
        assert!(has(&argv, "-crf"));
        let argv = finalize_argv("/v/a.mp4", "/v/out.mp4", Some("libopenh264"), false);
        assert_eq!(at(&argv, "-c:v"), "libopenh264");
        assert!(!has(&argv, "-crf"));
    }

    #[test]
    fn recorder_argv_gives_mpeg4_a_bitrate() {
        let argv = recorder_argv(&opts(|o| {
            o.codec = "mpeg4".into();
            o.pixel_format = "yuv420p".into();
        }));
        assert_eq!(at(&argv, "-c"), "mpeg4");
        assert_eq!(at(&argv, "-x"), "yuv420p");
        assert_eq!(at(&argv, "-p"), "b=8M");
        assert!(!has(&recorder_argv(&opts(|o| o.codec = "libx264".into())), "-p"));
    }

    #[test]
    fn finalize_argv_skips_the_audio_filter_without_a_track() {
        assert!(!has(&finalize_argv("/v/a.mp4", "/v/out.mp4", None, false), "-af"));
    }

    #[test]
    fn finalize_argv_mutes_fades_and_normalizes_when_there_is_audio() {
        let argv = finalize_argv("/v/a.mp4", "/v/out.mp4", None, true);
        let af = at(&argv, "-af");
        assert!(af.contains("volume=enable='lt(t,0.4)':volume=0"));
        assert!(af.contains("afade=t=in:st=0.4:d=0.05"));
        assert!(af.contains("loudnorm=I=-14:TP=-1.5:LRA=11"));
    }

    #[test]
    fn preview_frame_path_sits_next_to_its_source() {
        assert_eq!(preview_frame_path("/v/a.mp4"), "/v/a-preview.png");
        assert_eq!(preview_frame_path("/v/b.d/a.mp4"), "/v/b.d/a-preview.png");
        assert_eq!(preview_frame_path(""), "");
    }

    #[test]
    fn preview_frame_argv_pulls_one_frame_near_the_start() {
        let argv = preview_frame_argv("/v/a.mp4", "/v/a-preview.png");
        assert_eq!(argv[0], "ffmpeg");
        assert_eq!(at(&argv, "-ss"), "0.1");
        assert_eq!(at(&argv, "-i"), "/v/a.mp4");
        assert_eq!(at(&argv, "-vframes"), "1");
        assert_eq!(at(&argv, "-q:v"), "2");
        assert_eq!(argv.last().unwrap(), "/v/a-preview.png");
    }

    #[test]
    fn webcam_argv_targets_the_device_and_carries_the_app_id() {
        let argv = webcam_argv("/dev/video0", "formalshell-webcam");
        assert_eq!(argv[0], "mpv");
        assert_eq!(argv[1], "av://v4l2:/dev/video0");
        assert!(has(&argv, "--title=formalshell-webcam"));
        assert!(has(&argv, "--wayland-app-id=formalshell-webcam"));
        assert!(has(&argv, "--vf=lavfi=[crop=ih*8/9:ih]"));
    }

    #[test]
    fn parse_webcam_devices_reads_one_path_per_line() {
        assert_eq!(parse_webcam_devices("/dev/video0\n/dev/video1\n"), ["/dev/video0", "/dev/video1"]);
        assert!(parse_webcam_devices("").is_empty());
        assert!(parse_webcam_devices("\n\n").is_empty());
    }

    fn region(x: f64, y: f64, width: f64, height: f64) -> Region {
        Region { x, y, width, height }
    }

    fn geo(width: f64, height: f64, x: f64, y: f64) -> WebcamGeometry {
        WebcamGeometry { width, height, x, y }
    }

    #[test]
    fn webcam_geometry_scales_the_medium_preset_from_region_height() {
        assert_eq!(webcam_geometry("medium", region(100.0, 200.0, 1920.0, 1080.0), 18.0), geo(240.0, 270.0, 1762.0, 992.0));
    }

    #[test]
    fn webcam_geometry_falls_back_to_medium_for_an_unknown_size() {
        let r = region(0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(webcam_geometry("bogus", r, 18.0), webcam_geometry("medium", r, 18.0));
    }

    #[test]
    fn webcam_geometry_caps_the_scale_height_for_a_narrow_region() {
        assert_eq!(webcam_geometry("large", region(0.0, 0.0, 300.0, 1000.0), 18.0), geo(264.0, 297.0, 18.0, 685.0));
    }

    #[test]
    fn webcam_geometry_clamps_to_the_region_when_margins_overwhelm_it() {
        assert_eq!(webcam_geometry("medium", region(0.0, 0.0, 10.0, 10.0), 18.0), geo(3.0, 3.0, 18.0, 18.0));
    }

    fn poll(found: bool, attempts: u32, gave_up: bool) -> &'static str {
        webcam_map_poll_action(found, attempts, gave_up, 100, 200).as_str()
    }

    #[test]
    fn webcam_map_poll_waits_while_unfound_and_under_the_giveup_bound() {
        assert_eq!(poll(false, 1, false), "wait");
        assert_eq!(poll(false, 99, false), "wait");
    }

    #[test]
    fn webcam_map_poll_places_the_window_the_first_time_it_is_found() {
        assert_eq!(poll(true, 3, false), "place");
    }

    #[test]
    fn webcam_map_poll_gives_up_honestly_at_the_bound_but_keeps_polling() {
        assert_eq!(poll(false, 100, false), "give-up");
    }

    #[test]
    fn webcam_map_poll_reaps_a_straggler_window_found_after_giveup() {
        assert_eq!(poll(true, 150, true), "reap");
    }

    #[test]
    fn webcam_map_poll_stops_once_the_reap_bound_is_hit_with_nothing_found() {
        assert_eq!(poll(false, 199, true), "wait");
        assert_eq!(poll(false, 200, true), "stop");
    }

    #[test]
    fn region_from_geometry_parses_the_slurp_shape() {
        assert_eq!(region_from_geometry("100,200 800x600"), Some(region(100.0, 200.0, 800.0, 600.0)));
        assert_eq!(region_from_geometry("-10,-20 100x50"), Some(region(-10.0, -20.0, 100.0, 50.0)));
        assert_eq!(region_from_geometry(""), None);
        assert_eq!(region_from_geometry("bogus"), None);
    }
}
