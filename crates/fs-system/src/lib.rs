//! Pure logic for the system-facing parts of FormalShell: monitor parsers,
//! power, display, lights, capture, audio, camera, controller, app icon,
//! keyboard layout and overnight models. Text and plain data in, plain data
//! out. No IO, no UI.

pub mod appicon;
pub mod audio;
pub mod camera;
pub mod capture;
pub mod console;
pub mod display;
pub mod dualsense;
pub mod exec;
pub mod keyboard;
pub mod lights;
pub mod monitor;
pub mod overnight;
pub mod power;
