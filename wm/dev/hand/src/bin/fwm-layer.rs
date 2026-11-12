//! A layer-shell surface of one colour, for a rig that has no bar or
//! wallpaper of its own:
//!
//!   fwm-layer bar top top 0 36 --exclusive 36 --color cc202020
//!
//! `fwm-layer NAMESPACE LAYER EDGE WIDTH HEIGHT`. LAYER is `background`,
//! `bottom`, `top` or `overlay`. EDGE is the edge it lies along (`top`,
//! `bottom`, `left`, `right`), `fill` for all four or `none` for the middle
//! of the output. A WIDTH or HEIGHT of 0 takes what the compositor gives.
//! `--color` is AARRGGBB, not premultiplied. `--keyboard exclusive` takes
//! the keyboard while it is up. It stays until it is killed.

use std::error::Error;

use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{Anchor, Event, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

#[derive(Default)]
struct Globals {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shell: Option<ZwlrLayerShellV1>,
}

struct Sheet {
    globals: Globals,
    surface: Option<wl_surface::WlSurface>,
    asked: (u32, u32),
    color: u32,
    closed: bool,
    failed: Option<String>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Sheet {
    fn event(
        sheet: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_compositor" => {
                sheet.globals.compositor = Some(registry.bind(name, version.min(4), queue, ()))
            }
            "wl_shm" => sheet.globals.shm = Some(registry.bind(name, 1, queue, ())),
            "zwlr_layer_shell_v1" => {
                sheet.globals.shell = Some(registry.bind(name, version.min(4), queue, ()))
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for Sheet {
    fn event(
        sheet: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                let pick = |given: u32, asked: u32| if given > 0 { given } else { asked.max(1) };
                let size = (pick(width, sheet.asked.0), pick(height, sheet.asked.1));
                if let Err(error) = sheet.paint(size, queue) {
                    sheet.failed = Some(error.to_string());
                }
            }
            Event::Closed => sheet.closed = true,
            _ => {}
        }
    }
}

delegate_noop!(Sheet: wl_compositor::WlCompositor);
delegate_noop!(Sheet: ZwlrLayerShellV1);
delegate_noop!(Sheet: wl_shm_pool::WlShmPool);
delegate_noop!(Sheet: ignore wl_shm::WlShm);
delegate_noop!(Sheet: ignore wl_surface::WlSurface);
delegate_noop!(Sheet: ignore wl_buffer::WlBuffer);

impl Sheet {
    fn paint(&mut self, size: (u32, u32), queue: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        let (Some(shm), Some(surface)) = (&self.globals.shm, &self.surface) else {
            return Ok(());
        };
        let buffer = fwm_hand::solid(shm, queue, size, self.color)?;
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, size.0 as i32, size.1 as i32);
        surface.commit();
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut words = std::env::args().skip(1);
    let usage = "fwm-layer NAMESPACE LAYER EDGE WIDTH HEIGHT [--exclusive N] [--color AARRGGBB] [--keyboard exclusive]";
    let mut word = || words.next().ok_or(usage);
    let namespace = word()?;
    let layer = match word()?.as_str() {
        "background" => Layer::Background,
        "bottom" => Layer::Bottom,
        "top" => Layer::Top,
        "overlay" => Layer::Overlay,
        other => return Err(format!("unknown layer `{other}`").into()),
    };
    let anchor = match word()?.as_str() {
        "top" => Anchor::Top | Anchor::Left | Anchor::Right,
        "bottom" => Anchor::Bottom | Anchor::Left | Anchor::Right,
        "left" => Anchor::Left | Anchor::Top | Anchor::Bottom,
        "right" => Anchor::Right | Anchor::Top | Anchor::Bottom,
        "fill" => Anchor::all(),
        "none" => Anchor::empty(),
        other => return Err(format!("unknown edge `{other}`").into()),
    };
    let asked: (u32, u32) = (word()?.parse()?, word()?.parse()?);
    let (mut exclusive, mut color) = (0i32, 0xff20_2020u32);
    let mut keyboard = KeyboardInteractivity::None;
    while let Some(flag) = words.next() {
        let value = words.next().ok_or(usage)?;
        match flag.as_str() {
            "--exclusive" => exclusive = value.parse()?,
            "--color" => color = u32::from_str_radix(&value, 16)?,
            "--keyboard" if value == "exclusive" => keyboard = KeyboardInteractivity::Exclusive,
            other => return Err(format!("unknown flag `{other}`").into()),
        }
    }

    let connection = Connection::connect_to_env()?;
    let mut events = connection.new_event_queue();
    let queue = events.handle();
    connection.display().get_registry(&queue, ());
    let mut sheet = Sheet {
        globals: Globals::default(),
        surface: None,
        asked,
        color,
        closed: false,
        failed: None,
    };
    events.roundtrip(&mut sheet)?;
    let compositor = sheet.globals.compositor.clone().ok_or("no wl_compositor")?;
    let shell = sheet
        .globals
        .shell
        .clone()
        .ok_or("no zwlr_layer_shell_v1")?;
    if sheet.globals.shm.is_none() {
        return Err("no wl_shm".into());
    }

    let surface = compositor.create_surface(&queue, ());
    let layer = shell.get_layer_surface(&surface, None, layer, namespace, &queue, ());
    layer.set_size(asked.0, asked.1);
    layer.set_anchor(anchor);
    layer.set_exclusive_zone(exclusive);
    layer.set_keyboard_interactivity(keyboard);
    // The first commit carries no buffer; the configure that answers it
    // says how large to paint.
    surface.commit();
    sheet.surface = Some(surface);

    while !sheet.closed {
        events.blocking_dispatch(&mut sheet)?;
        if let Some(error) = sheet.failed.take() {
            return Err(error.into());
        }
    }
    Ok(())
}
