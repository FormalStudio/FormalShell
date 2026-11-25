//! The system's libavcodec, opened at run time: the package links no codec
//! and the distros ship different sonames. Only the leading fields of
//! libav's structs are touched, the ones its ABI has kept still.

use std::ffi::{c_char, c_int, c_void};

use libloading::Library;

pub const EAGAIN: c_int = -11;

/// The leading fields of `AVFrame`, unchanged since libavutil 50.
#[repr(C)]
pub struct AvFrameHead {
    pub data: [*mut u8; 8],
    pub linesize: [c_int; 8],
    pub extended_data: *mut *mut u8,
    pub width: c_int,
    pub height: c_int,
    pub nb_samples: c_int,
    pub format: c_int,
}

/// The leading fields of `AVPacket`, unchanged since libavcodec 57.
#[repr(C)]
pub struct AvPacketHead {
    pub buf: *mut c_void,
    pub pts: i64,
    pub dts: i64,
    pub data: *mut u8,
    pub size: c_int,
}

pub struct Av {
    _codec: Library,
    _util: Library,
    pub find_decoder_by_name: unsafe extern "C" fn(*const c_char) -> *const c_void,
    pub alloc_context3: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    pub open2: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> c_int,
    pub free_context: unsafe extern "C" fn(*mut *mut c_void),
    pub send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub packet_free: unsafe extern "C" fn(*mut *mut c_void),
    pub frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub frame_free: unsafe extern "C" fn(*mut *mut c_void),
    pub parameters_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub parameters_free: unsafe extern "C" fn(*mut *mut c_void),
    pub parameters_to_context: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub mallocz: unsafe extern "C" fn(usize) -> *mut c_void,
    pub opt_set: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, c_int) -> c_int,
    pub log_set_level: unsafe extern "C" fn(c_int),
}

fn open_any(names: &[&str]) -> Result<Library, String> {
    for n in names {
        // SAFETY: loading a system library by soname; its initialisers are libav's own.
        if let Ok(lib) = unsafe { Library::new(n) } {
            return Ok(lib);
        }
    }
    Err(format!("none of {names:?} found"))
}

impl Av {
    pub fn load() -> Result<Self, String> {
        let util = open_any(&["libavutil.so.60", "libavutil.so.59", "libavutil.so.58", "libavutil.so.57"])?;
        let codec = open_any(&["libavcodec.so.62", "libavcodec.so.61", "libavcodec.so.60", "libavcodec.so.59"])?;
        macro_rules! sym {
            ($lib:expr, $name:literal) => {
                // SAFETY: the symbol's signature is libav's public C API.
                *unsafe { $lib.get(concat!($name, "\0").as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
            };
        }
        Ok(Self {
            find_decoder_by_name: sym!(codec, "avcodec_find_decoder_by_name"),
            alloc_context3: sym!(codec, "avcodec_alloc_context3"),
            open2: sym!(codec, "avcodec_open2"),
            free_context: sym!(codec, "avcodec_free_context"),
            send_packet: sym!(codec, "avcodec_send_packet"),
            receive_frame: sym!(codec, "avcodec_receive_frame"),
            packet_alloc: sym!(codec, "av_packet_alloc"),
            packet_free: sym!(codec, "av_packet_free"),
            frame_alloc: sym!(util, "av_frame_alloc"),
            frame_free: sym!(util, "av_frame_free"),
            parameters_alloc: sym!(codec, "avcodec_parameters_alloc"),
            parameters_free: sym!(codec, "avcodec_parameters_free"),
            parameters_to_context: sym!(codec, "avcodec_parameters_to_context"),
            mallocz: sym!(util, "av_mallocz"),
            opt_set: sym!(util, "av_opt_set"),
            log_set_level: sym!(util, "av_log_set_level"),
            _codec: codec,
            _util: util,
        })
    }
}

/// The leading fields of `AVCodecParameters`, unchanged since libavcodec 57.
#[repr(C)]
pub struct AvParametersHead {
    pub codec_type: c_int,
    pub codec_id: c_int,
    pub codec_tag: u32,
    pub extradata: *mut u8,
    pub extradata_size: c_int,
}
