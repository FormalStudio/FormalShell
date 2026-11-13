//! `FORMALWM_FRAMES=<file>`: every frame the engine presents, as a line of
//! JSON with the wall clock's milliseconds. It is how motion is checked on
//! a rig nobody is watching: a jump is a step no neighbouring frame has.

use std::{
    cell::RefCell,
    fs::File,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use formalwm_core::scene::Visual;
use serde_json::json;

use crate::engine::Engine;

thread_local! {
    static OUT: RefCell<Option<File>> = RefCell::new(
        std::env::var_os("FORMALWM_FRAMES").and_then(|path| File::create(path).ok()),
    );
}

pub fn frame(engine: &Engine, frame: &[Visual]) {
    OUT.with(|out| {
        let mut out = out.borrow_mut();
        let Some(file) = out.as_mut() else {
            return;
        };
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
        let windows: Vec<_> = frame
            .iter()
            .map(|visual| {
                json!({
                    "app": engine.model.window(&visual.id).map(|window| window.app_id.as_str()),
                    "x": visual.rect.x, "y": visual.rect.y,
                    "w": visual.rect.w, "h": visual.rect.h,
                    "o": visual.opacity, "lane": visual.lane,
                })
            })
            .collect();
        let line = json!({
            "t": t,
            "overview": engine.overview.progress.value(),
            "windows": windows,
        });
        let _ = writeln!(file, "{line}");
    });
}
