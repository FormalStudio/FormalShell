//! What the rig's clients share: a buffer of one colour.

use std::{
    fs::{self, File},
    io::{self, Write},
    os::fd::AsFd,
};

use wayland_client::{
    Dispatch, QueueHandle,
    protocol::{wl_buffer::WlBuffer, wl_shm, wl_shm_pool::WlShmPool},
};

/// A `w` by `h` buffer filled with `color`, AARRGGBB and not premultiplied.
pub fn solid<D>(
    shm: &wl_shm::WlShm,
    queue: &QueueHandle<D>,
    size: (u32, u32),
    color: u32,
) -> io::Result<WlBuffer>
where
    D: Dispatch<WlShmPool, ()> + Dispatch<WlBuffer, ()> + 'static,
{
    marked(shm, queue, size, color, None)
}

/// `solid` with a band of 40 pixels of `mark` down its left and right
/// edges, so a screenshot shows whether the buffer was scaled to its rect,
/// cut from its top left, cut around its middle, or let out of it.
pub fn marked<D>(
    shm: &wl_shm::WlShm,
    queue: &QueueHandle<D>,
    (w, h): (u32, u32),
    color: u32,
    mark: Option<u32>,
) -> io::Result<WlBuffer>
where
    D: Dispatch<WlShmPool, ()> + Dispatch<WlBuffer, ()> + 'static,
{
    // ARGB8888 in memory order, premultiplied.
    let pixel = |color: u32| {
        let [a, r, g, b] = color.to_be_bytes();
        let scale = |channel: u8| (u32::from(channel) * u32::from(a) / 255) as u8;
        [scale(b), scale(g), scale(r), a]
    };
    let (fill, band) = (pixel(color), mark.map(pixel));
    let row: Vec<u8> = (0..w)
        .flat_map(|x| match band {
            Some(band) if x < 40 || x + 40 >= w => band,
            _ => fill,
        })
        .collect();

    let dir = std::env::var("XDG_RUNTIME_DIR").map_err(io::Error::other)?;
    let path = format!("{dir}/fwm-buffer-{}", std::process::id());
    let mut file = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    fs::remove_file(&path)?;
    file.write_all(&row.repeat(h as usize))?;
    file.flush()?;

    let pool = shm.create_pool(file.as_fd(), (w * h * 4) as i32, queue, ());
    let buffer = pool.create_buffer(
        0,
        w as i32,
        h as i32,
        (w * 4) as i32,
        wl_shm::Format::Argb8888,
        queue,
        (),
    );
    pool.destroy();
    Ok(buffer)
}
