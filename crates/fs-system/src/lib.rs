//! Pure logic for the system-facing parts of FormalShell: monitor parsers,
//! power, display, lights, capture, audio, camera, controller
//! and overnight models. Text and plain data in, plain data out. No IO, no UI.

pub mod audio;
pub mod camera;
pub mod capture;
pub mod compositor;
pub mod console;
pub mod display;
pub mod dualsense;
pub mod exec;
pub mod lights;
pub mod monitor;
pub mod overnight;
pub mod power;
pub mod proc;
