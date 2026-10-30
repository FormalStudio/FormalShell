//! Pure logic behind the bar and the screen chrome: the bar layout resolver,
//! the workspace cell model, drawer and frame geometry, the panel cursor, the
//! palette dither, the backdrop grid and sculpture, the window switcher list, hot corner arming and the plugin
//! manifest resolver. Plain data in, plain data out, no IO.

// A NaN input must read as "no value", which `!(x > 0.0)` says and `x <= 0.0` does not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod bar;
pub mod clock_face;
pub mod cursor;
pub mod dither;
pub mod drawer;
pub mod frame;
pub mod grid;
pub mod hot_corners;
pub mod plugins;
pub mod rim;
pub mod shards;
pub mod switcher;
pub mod types;
