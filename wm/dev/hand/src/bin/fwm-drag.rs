//! A window that starts a drag of a line of text when it is pressed, for a
//! rig with nothing else to drag from:
//!
//!   fwm-drag source > "$SHOJI_RUN_DIR/drag.log"
//!
//! `fwm-drag APP_ID [--title TITLE] [--size WxH] [--fixed] [--min WxH] [--max WxH] [--child] [--maximized] [--tag TAG] [--content photo|video|game]`. It prints what
//! becomes of each drag, which carries an orange square for its icon: `sent` when the target asks for the text,
//! `dropped`, `finished`, `cancelled`, `enter` when the pointer comes over
//! it, `key N` for each key pressed on it by its evdev code, and `configure WxH` for each size the compositor asks of it. An empty APP_ID or TITLE sets none,
//! and `--fixed` allows no other size, which is how a dialog looks to the
//! compositor. `--min` and `--max` bound the sizes it takes, and it draws at
//! the nearest it allows to the size it is asked for, as a toolkit does.
//! `--child` maps a second, resizable toplevel whose parent is the first, a
//! transient dialog, and prints `child configure WxH` for it; `--maximized`
//! asks to be maximized before it maps, as Electron apps do; `--late MS`
//! draws its first frame MS after the first configure, as GTK 4 apps do;
//! `--tag` gives it an xdg-toplevel-tag and `--content` a
//! wp-content-type: enough to stand in for the windows the rules name.

use std::{error::Error, fs::File, io::Write};

use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop, event_created_child,
    protocol::{
        wl_buffer, wl_compositor, wl_data_device, wl_data_device_manager, wl_data_offer,
        wl_data_source, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
        wl_surface,
    },
};
use wayland_protocols::{
    wp::content_type::v1::client::{
        wp_content_type_manager_v1::WpContentTypeManagerV1,
        wp_content_type_v1::{Type, WpContentTypeV1},
    },
    xdg::{
        shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base},
        toplevel_tag::v1::client::xdg_toplevel_tag_manager_v1::XdgToplevelTagManagerV1,
    },
};

const TEXT: &str = "dropped-by-fwm-drag";

#[derive(Default)]
struct Source {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    shell: Option<xdg_wm_base::XdgWmBase>,
    tags: Option<XdgToplevelTagManagerV1>,
    contents: Option<WpContentTypeManagerV1>,
    data: Option<wl_data_device_manager::WlDataDeviceManager>,
    device: Option<wl_data_device::WlDataDevice>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    surface: Option<wl_surface::WlSurface>,
    size: (u32, u32),
    min: (u32, u32),
    max: (u32, u32),
    child: Option<wl_surface::WlSurface>,
    child_size: (u32, u32),
    closed: bool,
    failed: Option<String>,
    /// Milliseconds the first frame waits after the first configure.
    late: u64,
}

/// The side of the drag's icon, in pixels.
const ICON: u32 = 32;

impl Dispatch<wl_registry::WlRegistry, ()> for Source {
    fn event(
        source: &mut Self,
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
                source.compositor = Some(registry.bind(name, version.min(4), queue, ()))
            }
            "wl_shm" => source.shm = Some(registry.bind(name, 1, queue, ())),
            "wl_seat" => source.seat = Some(registry.bind(name, version.min(5), queue, ())),
            "xdg_wm_base" => source.shell = Some(registry.bind(name, 1, queue, ())),
            "xdg_toplevel_tag_manager_v1" => source.tags = Some(registry.bind(name, 1, queue, ())),
            "wp_content_type_manager_v1" => {
                source.contents = Some(registry.bind(name, 1, queue, ()))
            }
            "wl_data_device_manager" => {
                source.data = Some(registry.bind(name, version.min(3), queue, ()))
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Source {
    fn event(
        _: &mut Self,
        shell: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            shell.pong(serial);
        }
    }
}

/// Which of the client's toplevels an object belongs to.
const MAIN: bool = false;
const CHILD: bool = true;

impl Dispatch<xdg_toplevel::XdgToplevel, bool> for Source {
    fn event(
        source: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        child: &bool,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } if *child => {
                println!("child configure {width}x{height}");
                if width > 0 && height > 0 {
                    source.child_size = (width as u32, height as u32);
                }
            }
            xdg_toplevel::Event::Configure { width, height, .. } => {
                println!("configure {width}x{height}");
                if width > 0 && height > 0 {
                    let bound = |asked: i32, min: u32, max: u32| {
                        let max = if max == 0 { u32::MAX } else { max };
                        (asked as u32).clamp(min.min(max), max)
                    };
                    source.size = (
                        bound(width, source.min.0, source.max.0),
                        bound(height, source.min.1, source.max.1),
                    );
                }
            }
            xdg_toplevel::Event::Close if !*child => source.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, bool> for Source {
    fn event(
        source: &mut Self,
        shell_surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        child: &bool,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let xdg_surface::Event::Configure { serial } = event else {
            return;
        };
        shell_surface.ack_configure(serial);
        let (surface, size, color) = if *child {
            (&source.child, source.child_size, 0xff6a_4a2a)
        } else {
            (&source.surface, source.size, 0xff2a_4a6a)
        };
        let (Some(shm), Some(surface)) = (&source.shm, surface) else {
            return;
        };
        if !*child && source.late > 0 {
            std::thread::sleep(std::time::Duration::from_millis(std::mem::take(
                &mut source.late,
            )));
        }
        let mark =
            (!*child && (source.min != (0, 0) || source.max != (0, 0))).then_some(0xffc0_6040);
        match fwm_hand::marked(shm, queue, size, color, mark) {
            Ok(buffer) => {
                surface.attach(Some(&buffer), 0, 0);
                surface.damage_buffer(0, 0, size.0 as i32, size.1 as i32);
                surface.commit();
            }
            Err(error) => source.failed = Some(error.to_string()),
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Source {
    fn event(
        source: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Pointer) && source.pointer.is_none() {
            source.pointer = Some(seat.get_pointer(queue, ()));
        }
        if capabilities.contains(wl_seat::Capability::Keyboard) && source.keyboard.is_none() {
            source.keyboard = Some(seat.get_keyboard(queue, ()));
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Source {
    fn event(
        _: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            println!("key {key}");
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Source {
    fn event(
        source: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let serial = match event {
            wl_pointer::Event::Enter { .. } => return println!("enter"),
            wl_pointer::Event::Button {
                serial,
                state: WEnum::Value(wl_pointer::ButtonState::Pressed),
                ..
            } => serial,
            _ => return,
        };
        let (Some(data), Some(device), Some(surface), Some(compositor), Some(shm)) = (
            &source.data,
            &source.device,
            &source.surface,
            &source.compositor,
            &source.shm,
        ) else {
            return;
        };
        let offer = data.create_data_source(queue, ());
        offer.offer("text/plain;charset=utf-8".to_owned());
        offer.offer("text/plain".to_owned());
        offer.set_actions(
            wl_data_device_manager::DndAction::Copy | wl_data_device_manager::DndAction::Move,
        );
        let icon = compositor.create_surface(queue, ());
        device.start_drag(Some(&offer), surface, Some(&icon), serial);
        match fwm_hand::solid(shm, queue, (ICON, ICON), 0xffe0_a020) {
            Ok(buffer) => {
                icon.attach(Some(&buffer), 0, 0);
                icon.damage_buffer(0, 0, ICON as i32, ICON as i32);
                icon.commit();
            }
            Err(error) => source.failed = Some(error.to_string()),
        }
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for Source {
    fn event(
        _: &mut Self,
        offer: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { fd, .. } => {
                let _ = File::from(fd).write_all(TEXT.as_bytes());
                println!("sent");
            }
            wl_data_source::Event::DndDropPerformed => println!("dropped"),
            wl_data_source::Event::DndFinished => {
                println!("finished");
                offer.destroy();
            }
            wl_data_source::Event::Cancelled => {
                println!("cancelled");
                offer.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for Source {
    fn event(
        _: &mut Self,
        _: &wl_data_device::WlDataDevice,
        _: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(Source, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
    ]);
}

delegate_noop!(Source: wl_compositor::WlCompositor);
delegate_noop!(Source: wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(Source: wl_shm_pool::WlShmPool);
delegate_noop!(Source: ignore wl_shm::WlShm);
delegate_noop!(Source: ignore wl_surface::WlSurface);
delegate_noop!(Source: ignore wl_buffer::WlBuffer);
delegate_noop!(Source: ignore XdgToplevelTagManagerV1);
delegate_noop!(Source: ignore WpContentTypeManagerV1);
delegate_noop!(Source: ignore WpContentTypeV1);
delegate_noop!(Source: ignore wl_data_offer::WlDataOffer);

fn main() -> Result<(), Box<dyn Error>> {
    const USAGE: &str = "fwm-drag APP_ID [--title TITLE] [--size WxH] [--fixed] [--min WxH] [--max WxH] [--child] [--maximized] [--late MS] [--tag TAG] [--content photo|video|game]";
    let mut args = std::env::args().skip(1);
    let app_id = args.next().ok_or(USAGE)?;
    let (mut title, mut size, mut fixed) = (app_id.clone(), (400, 300), false);
    let (mut tag, mut content, mut late) = (None, None, 0);
    let (mut min, mut max, mut child, mut maximized) = ((0, 0), (0, 0), false, false);
    let dims = |text: Option<String>| -> Result<(u32, u32), Box<dyn Error>> {
        let text = text.ok_or(USAGE)?;
        let (w, h) = text.split_once('x').ok_or(USAGE)?;
        Ok((w.parse()?, h.parse()?))
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--title" => title = args.next().ok_or(USAGE)?,
            "--size" => size = dims(args.next())?,
            "--min" => min = dims(args.next())?,
            "--max" => max = dims(args.next())?,
            "--child" => child = true,
            "--maximized" => maximized = true,
            "--fixed" => fixed = true,
            "--late" => late = args.next().ok_or(USAGE)?.parse()?,
            "--tag" => tag = Some(args.next().ok_or(USAGE)?),
            "--content" => {
                content = Some(match args.next().ok_or(USAGE)?.as_str() {
                    "photo" => Type::Photo,
                    "video" => Type::Video,
                    "game" => Type::Game,
                    _ => return Err(USAGE.into()),
                })
            }
            _ => return Err(USAGE.into()),
        }
    }
    let connection = Connection::connect_to_env()?;
    let mut events = connection.new_event_queue();
    let queue = events.handle();
    connection.display().get_registry(&queue, ());
    if fixed {
        (min, max) = (size, size);
    }
    let mut source = Source {
        size,
        min,
        max,
        child_size: (300, 200),
        late,
        ..Source::default()
    };
    events.roundtrip(&mut source)?;

    let compositor = source.compositor.clone().ok_or("no wl_compositor")?;
    let shell = source.shell.clone().ok_or("no xdg_wm_base")?;
    let seat = source.seat.clone().ok_or("no wl_seat")?;
    let data = source.data.clone().ok_or("no wl_data_device_manager")?;
    if source.shm.is_none() {
        return Err("no wl_shm".into());
    }
    source.device = Some(data.get_data_device(&seat, &queue, ()));

    let surface = compositor.create_surface(&queue, ());
    let shell_surface = shell.get_xdg_surface(&surface, &queue, MAIN);
    let toplevel = shell_surface.get_toplevel(&queue, MAIN);
    let (app_id_for_child, title_for_child) = (app_id.clone(), title.clone());
    if !app_id.is_empty() {
        toplevel.set_app_id(app_id);
    }
    if !title.is_empty() {
        toplevel.set_title(title);
    }
    if let Some(tag) = tag {
        let tags = source
            .tags
            .clone()
            .ok_or("no xdg_toplevel_tag_manager_v1")?;
        tags.set_toplevel_tag(&toplevel, tag);
    }
    if let Some(content) = content {
        let contents = source
            .contents
            .clone()
            .ok_or("no wp_content_type_manager_v1")?;
        contents
            .get_surface_content_type(&surface, &queue, ())
            .set_content_type(content);
    }
    if min != (0, 0) {
        toplevel.set_min_size(min.0 as i32, min.1 as i32);
    }
    if max != (0, 0) {
        toplevel.set_max_size(max.0 as i32, max.1 as i32);
    }
    if maximized {
        toplevel.set_maximized();
    }
    surface.commit();
    source.surface = Some(surface);
    if child {
        // Mapped first, so the dialog has a parent the compositor knows.
        events.roundtrip(&mut source)?;
        let surface = compositor.create_surface(&queue, ());
        let child = shell
            .get_xdg_surface(&surface, &queue, CHILD)
            .get_toplevel(&queue, CHILD);
        child.set_parent(Some(&toplevel));
        child.set_app_id(format!("{}-child", app_id_for_child));
        child.set_title(format!("{} child", title_for_child));
        surface.commit();
        source.child = Some(surface);
    }

    while !source.closed {
        events.blocking_dispatch(&mut source)?;
        std::io::stdout().flush()?;
        if let Some(error) = source.failed.take() {
            return Err(error.into());
        }
    }
    Ok(())
}
