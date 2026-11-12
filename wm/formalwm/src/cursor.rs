//! The dynamic cursor's clock: while the pointer has somewhere to be, each
//! frame gives the compositor the shape `formalwm_core::cursor` makes of its
//! motion.

use std::{
    cell::RefCell,
    fs::File,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use formalwm_core::cursor::{Cursor, Shape};
use serde_json::json;
use shojiwm_rs::prelude::*;

use crate::engine::{Engine, enter};

#[derive(Default)]
pub struct Dynamic {
    /// Where the pointer was last seen.
    at: (f64, f64),
    motion: Cursor,
    /// What the compositor was last told to draw.
    shape: Option<Shape>,
    clock: Option<PollHandle>,
    ticked: Option<f64>,
}

thread_local! {
    /// `FORMALWM_CURSOR=<file>`: every shape the compositor is told, as a
    /// line of JSON with the wall clock's milliseconds, `null` for the image
    /// as it is. A rig nobody watches reads the cursor from it.
    static OUT: RefCell<Option<File>> = RefCell::new(
        std::env::var_os("FORMALWM_CURSOR").and_then(|path| File::create(path).ok()),
    );
}

fn trace(shape: Option<Shape>) {
    OUT.with(|out| {
        let mut out = out.borrow_mut();
        let Some(file) = out.as_mut() else {
            return;
        };
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
        let shape = shape.map(|shape| {
            json!({
                "rotate": shape.rotate, "along": shape.along,
                "across": shape.across, "axis": shape.axis,
            })
        });
        let _ = writeln!(file, "{}", json!({ "t": t, "shape": shape }));
    });
}

impl Engine {
    /// Not on the lock screen, and not on a cursor nobody sees.
    fn cursor_still(&self) -> bool {
        !self.settings.cursor.dynamic.enabled()
            || self.cursor_hidden
            || self.cursor_idle
            || COMPOSITOR.session.is_locked()
    }

    pub fn cursor_moved(&mut self, at: (f64, f64)) {
        self.cursor.at = at;
        if self.cursor.clock.is_some() || self.cursor_still() {
            return;
        }
        self.cursor.ticked = None;
        self.cursor.clock = Some(create_poll_for_each_output(1.0, |handle, _| {
            let now = handle.now_ms();
            enter(move |engine| engine.cursor_tick(now));
        }));
    }

    fn cursor_tick(&mut self, now: f64) {
        // The outputs' clocks interleave; one that is behind adds nothing.
        if self.cursor.ticked.is_some_and(|last| now <= last) {
            return;
        }
        self.cursor.ticked = Some(now);
        let still = self.cursor_still();
        let shape = if still {
            self.cursor.motion.reset();
            None
        } else {
            let dynamic = self.settings.cursor.dynamic;
            self.cursor.motion.step(&dynamic, now, self.cursor.at)
        };
        if shape != self.cursor.shape {
            self.cursor.shape = shape;
            trace(shape);
            // No transform at rest: the compositor draws a transformed
            // cursor into the frame and an untouched one on its plane.
            let transform = shape.map(|shape| CursorTransform {
                rotate: shape.rotate,
                scale_x: shape.along,
                scale_y: shape.across,
                scale_axis: shape.axis,
            });
            if let Err(error) = COMPOSITOR.cursor.set_transform(transform) {
                tracing::warn!(%error, "the cursor could not be shaped");
            }
        }
        if (still || self.cursor.motion.resting())
            && let Some(clock) = self.cursor.clock.take()
        {
            clock.cancel();
        }
    }
}
