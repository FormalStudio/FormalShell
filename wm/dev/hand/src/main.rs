//! The rig's hand on the session's pointer, through wlr-virtual-pointer, for
//! the drags wlrctl cannot make: it clicks, it never holds.
//!
//!   fwm-hand at 300 200 press glide 700 200 400 wait 200 release
//!
//! `at X Y` jumps there, `glide X Y MS` travels there in a straight line
//! over MS milliseconds, `press` and `release` take a button (`left`,
//! `right`, `middle`; left when the next word is not one), `wait MS` holds
//! still. Positions are logical pixels of the layout, 1024x768 unless
//! `--size WxH` comes first.

use std::{
    error::Error,
    thread::sleep,
    time::{Duration, Instant},
};

use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{
        wl_pointer::{Axis, AxisSource, ButtonState},
        wl_registry,
    },
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

/// Between two motions of a glide: a 125 Hz mouse.
const STEP: Duration = Duration::from_millis(8);

struct Hand {
    manager: Option<ZwlrVirtualPointerManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Hand {
    fn event(
        hand: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "zwlr_virtual_pointer_manager_v1"
        {
            hand.manager = Some(registry.bind(name, version.min(2), queue, ()));
        }
    }
}

delegate_noop!(Hand: ZwlrVirtualPointerManagerV1);
delegate_noop!(Hand: ZwlrVirtualPointerV1);

fn button(name: &str) -> Option<u32> {
    match name {
        "left" => Some(0x110),
        "right" => Some(0x111),
        "middle" => Some(0x112),
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut words = std::env::args().skip(1).peekable();
    let mut size = (1024u32, 768u32);
    if words.peek().map(String::as_str) == Some("--size") {
        words.next();
        let text = words.next().ok_or("--size needs WxH")?;
        let (w, h) = text.split_once('x').ok_or("--size needs WxH")?;
        size = (w.parse()?, h.parse()?);
    }

    let connection = Connection::connect_to_env()?;
    let mut events = connection.new_event_queue();
    let queue = events.handle();
    connection.display().get_registry(&queue, ());
    let mut hand = Hand { manager: None };
    events.roundtrip(&mut hand)?;
    let manager = hand
        .manager
        .take()
        .ok_or("the compositor has no zwlr_virtual_pointer_manager_v1")?;
    let pointer = manager.create_virtual_pointer(None, &queue, ());

    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u32;
    let put = |(x, y): (f64, f64)| {
        let clamp = |v: f64, max: u32| v.round().clamp(0.0, f64::from(max)) as u32;
        pointer.motion_absolute(now(), clamp(x, size.0), clamp(y, size.1), size.0, size.1);
        pointer.frame();
    };
    let mut at = (0.0, 0.0);
    let number = |words: &mut dyn Iterator<Item = String>, what: &str| {
        words
            .next()
            .and_then(|word| word.parse::<f64>().ok())
            .ok_or(format!("{what} needs a number"))
    };

    while let Some(word) = words.next() {
        match word.as_str() {
            "at" => {
                at = (number(&mut words, "at")?, number(&mut words, "at")?);
                put(at);
            }
            "glide" => {
                let to = (number(&mut words, "glide")?, number(&mut words, "glide")?);
                let ms = number(&mut words, "glide")?;
                let steps = (ms / STEP.as_millis() as f64).ceil().max(1.0) as u32;
                for step in 1..=steps {
                    let t = f64::from(step) / f64::from(steps);
                    put((at.0 + (to.0 - at.0) * t, at.1 + (to.1 - at.1) * t));
                    events.roundtrip(&mut hand)?;
                    sleep(STEP);
                }
                at = to;
            }
            "press" | "release" => {
                let code = match words.peek().and_then(|name| button(name)) {
                    Some(code) => {
                        words.next();
                        code
                    }
                    None => 0x110,
                };
                let state = if word == "press" {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                };
                pointer.button(now(), code, state);
                pointer.frame();
            }
            "wheel" => {
                let notches = number(&mut words, "wheel")?;
                pointer.axis_source(AxisSource::Wheel);
                pointer.axis_discrete(now(), Axis::VerticalScroll, notches * 15.0, notches as i32);
                pointer.frame();
            }
            "wait" => {
                events.roundtrip(&mut hand)?;
                sleep(Duration::from_millis(number(&mut words, "wait")? as u64));
            }
            other => return Err(format!("unknown word `{other}`").into()),
        }
        events.roundtrip(&mut hand)?;
    }
    Ok(())
}
