//! The GPU path behind `render.backend`: one wgpu Vulkan device on the
//! compositor's own render node, vello_hybrid drawing a surface's damage
//! into a canvas texture of its own, and linux-dmabuf buffers exported out
//! of Vulkan that the canvas is copied into for the compositor to read.
//!
//! Resolved once at start ([`resolve`]). Anything it cannot find (no
//! linux-dmabuf v4, a main device with no render node, no Vulkan device on
//! that node but llvmpipe, a driver that cannot export a linear dmabuf)
//! leaves the shell on vello_cpu with the reason logged and in `debug
//! dump`. A device lost later sends every surface back to the CPU on its
//! next present.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use ash::vk;
use fs_theme::theme::Backend;
use smithay_client_toolkit::dmabuf::{DmabufFeedback, DmabufHandler, DmabufState};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::{Connection, EventQueue, Proxy, QueueHandle, backend::ObjectId};
use smithay_client_toolkit::reexports::protocols::wp::linux_dmabuf::zv1::client::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1};
use vello_common_hybrid::filter_effects::{Filter, FilterFunction};
use vello_common_hybrid::paint::{Image, ImageId, ImageSource, Tint, TintMode};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Rect, Shape, Stroke};
use vello_cpu::peniko::{BlendMode, Fill, Gradient, ImageSampler};
use vello_cpu::Pixmap;

use crate::render::Raster;
use crate::scene::IRect;

/// The square every frame's damage is drawn in, a piece at a time: the
/// renderer's depth target is made once at this size and every render
/// pass has to match it.
pub const TILE: i32 = 1024;
/// The atlas small pictures (glyph coverage, icons) are packed into.
const ATLAS: u32 = 2048;
/// Past this side a picture gets a texture of its own instead of atlas room:
/// with only pictures this small in it, the budget below leaves the atlases
/// far from full however they fragment (vello_hybrid panics on a full one).
const BIG: u16 = 256;
/// Atlas bytes kept before the pictures unused longest are dropped: half
/// of two atlases, so the allocator never runs out of room for one more
/// (vello_hybrid panics when it does).
const ATLAS_BUDGET: usize = (ATLAS * ATLAS * 4) as usize;

const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");
const DRM_FORMAT_MOD_LINEAR: u64 = 0;

thread_local! {
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
}

/// Set once the GPU path fails (the device lost, from whatever thread wgpu
/// calls that on, or a buffer it cannot make or finish): every surface goes
/// back to the CPU on its next present.
static LOST: AtomicBool = AtomicBool::new(false);
static FAILED: Mutex<Option<String>> = Mutex::new(None);

fn fail(reason: String) {
    eprintln!("formalshell: render: {reason}; drawing on the CPU");
    FAILED.lock().unwrap().get_or_insert(reason);
    LOST.store(true, Ordering::Relaxed);
}

/// What `debug dump` reports: the backend asked for, the one drawing, why
/// the GPU is not drawing when it is not, and the device when it is.
#[derive(Clone)]
pub struct Status {
    pub asked: Backend,
    pub backend: &'static str,
    pub reason: Option<String>,
    pub device: Option<String>,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);

pub fn status() -> Status {
    let lost = LOST.load(Ordering::Relaxed);
    let mut s = STATUS.lock().unwrap().clone().unwrap_or(Status { asked: Backend::Auto, backend: "cpu", reason: None, device: None });
    if lost && s.backend == "gpu" {
        s.backend = "cpu";
        s.reason = FAILED.lock().unwrap().clone();
    }
    s
}

/// True while surfaces should draw on the GPU.
pub fn active() -> bool {
    !LOST.load(Ordering::Relaxed) && GPU.with(|g| g.borrow().is_some())
}

fn with<T>(f: impl FnOnce(&mut Gpu) -> T) -> Option<T> {
    GPU.with(|g| g.borrow_mut().as_mut().map(f))
}

/// Picks the backend for this run, once, before any surface is made.
pub fn resolve(asked: Backend, conn: &Connection, globals: &GlobalList) {
    let tried = match asked {
        Backend::Cpu => Err("render.backend is cpu".to_string()),
        Backend::Auto | Backend::Gpu => Gpu::new(conn, globals),
    };
    let status = match tried {
        Ok(gpu) => {
            let device = gpu.name.clone();
            eprintln!("formalshell: render: drawing on the GPU ({device})");
            GPU.with(|g| *g.borrow_mut() = Some(gpu));
            Status { asked, backend: "gpu", reason: None, device: Some(device) }
        }
        Err(reason) => {
            if asked != Backend::Cpu {
                eprintln!("formalshell: render: drawing on the CPU: {reason}");
            }
            Status { asked, backend: "cpu", reason: Some(reason), device: None }
        }
    };
    *STATUS.lock().unwrap() = Some(status);
}

/// The linux-dmabuf objects, on a queue of their own so the protocol's
/// handler never has to be the App's: the main loop reads their events off
/// the socket and a present dispatches them.
struct Wl {
    dmabuf: DmabufState,
    feedback: Option<DmabufFeedback>,
    released: HashMap<ObjectId, Rc<Cell<bool>>>,
}

impl DmabufHandler for Wl {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf
    }

    fn dmabuf_feedback(&mut self, _: &Connection, _: &QueueHandle<Self>, proxy: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, feedback: DmabufFeedback) {
        self.feedback = Some(feedback);
        proxy.destroy();
    }

    fn created(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, _: WlBuffer) {}

    fn failed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1) {}

    fn released(&mut self, _: &Connection, _: &QueueHandle<Self>, buffer: &WlBuffer) {
        if let Some(flag) = self.released.get(&buffer.id()) {
            flag.set(true);
        }
    }
}

smithay_client_toolkit::delegate_dispatch2!(Wl);

/// A picture's place: a slot in the atlas, or a texture of its own.
enum Placed {
    Atlas(ImageId),
    Own(vello_hybrid::TextureId),
}

struct Uploaded {
    /// Held so the address the table is keyed by is never reused under it.
    _pixmap: Arc<Pixmap>,
    placed: Placed,
    bytes: usize,
    used: u64,
}

pub struct Gpu {
    name: String,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    raw: ash::Device,
    family: u32,
    fd_fns: ash::khr::external_memory_fd::Device,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello_hybrid::Renderer,
    resources: vello_hybrid::Resources,
    scene: vello_hybrid::Scene,
    tile: wgpu::Texture,
    tile_view: wgpu::TextureView,
    bindings: vello_hybrid::TextureBindings,
    images: HashMap<usize, Uploaded>,
    atlas_bytes: usize,
    next_texture: u64,
    frame: u64,
    uploads: Option<wgpu::CommandEncoder>,
    fade: Option<Fade>,
    wl_queue: EventQueue<Wl>,
    wl: Wl,
}

impl Gpu {
    fn new(conn: &Connection, globals: &GlobalList) -> Result<Self, String> {
        let wl_queue = conn.new_event_queue::<Wl>();
        let qh = wl_queue.handle();
        let mut wl = Wl { dmabuf: DmabufState::new(globals, &qh), feedback: None, released: HashMap::new() };
        let mut wl_queue = wl_queue;
        if wl.dmabuf.version().unwrap_or(0) < 4 {
            return Err("the compositor offers no linux-dmabuf v4".into());
        }
        wl.dmabuf.get_default_feedback(&qh).map_err(|e| format!("linux-dmabuf feedback: {e}"))?;
        for _ in 0..3 {
            if wl.feedback.is_some() {
                break;
            }
            wl_queue.roundtrip(&mut wl).map_err(|e| format!("linux-dmabuf roundtrip: {e}"))?;
        }
        let feedback = wl.feedback.as_ref().ok_or("the compositor sent no dmabuf feedback")?;
        let linear = feedback.tranches().iter().any(|t| {
            t.formats.iter().any(|&i| feedback.format_table().get(i as usize).is_some_and(|f| f.format == DRM_FORMAT_ABGR8888 && f.modifier == DRM_FORMAT_MOD_LINEAR))
        });
        if !linear {
            return Err("the compositor takes no linear ABGR8888 dmabuf".into());
        }
        let node = render_node(feedback.main_device()).ok_or("the compositor's main device has no render node")?;

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let adapters = futures_lite::future::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));
        let found: Vec<String> = adapters.iter().map(|a| a.get_info().name).collect();
        let adapter = adapters
            .into_iter()
            .find(|a| a.get_info().device_type != wgpu::DeviceType::Cpu && unsafe { drm_node(a) } == Some(node.rdev))
            .ok_or_else(|| {
                let found = if found.is_empty() { "none".to_string() } else { found.join(", ") };
                format!("no Vulkan device drives {} (found: {found})", node.path)
            })?;
        let info = adapter.get_info();
        let name = format!("{} on {}", info.name, node.path);

        // SAFETY: the hal adapter is this wgpu adapter's own and outlives
        // the calls; the extensions added are checked as supported first.
        let (hal_device, instance_raw, physical) = unsafe {
            let hal = adapter.as_hal::<wgpu::hal::api::Vulkan>().ok_or("not a Vulkan adapter")?;
            let instance_raw = hal.shared_instance().raw_instance().clone();
            let physical = hal.raw_physical_device();
            let have = instance_raw.enumerate_device_extension_properties(physical).map_err(|e| format!("device extensions: {e}"))?;
            let has = |n: &CStr| have.iter().any(|p| p.extension_name_as_c_str().is_ok_and(|e| e == n));
            for need in [ash::khr::external_memory_fd::NAME, ash::ext::external_memory_dma_buf::NAME, ash::ext::image_drm_format_modifier::NAME, ash::ext::queue_family_foreign::NAME] {
                if !has(need) {
                    return Err(format!("{} lacks {}", info.name, need.to_string_lossy()));
                }
            }
            let open = hal
                .open_with_callback(
                    wgpu::Features::empty(),
                    &adapter.limits(),
                    &wgpu::MemoryHints::MemoryUsage,
                    Some(Box::new(|args| {
                        // wgpu enables neither; the second is what lets a
                        // barrier name VK_QUEUE_FAMILY_FOREIGN_EXT.
                        for ext in [ash::ext::image_drm_format_modifier::NAME, ash::ext::queue_family_foreign::NAME] {
                            if !args.extensions.contains(&ext) {
                                args.extensions.push(ext);
                            }
                        }
                    })),
                )
                .map_err(|e| format!("opening {}: {e}", info.name))?;
            (open, instance_raw, physical)
        };
        let raw = hal_device.device.raw_device().clone();
        let family = hal_device.device.queue_family_index();
        let fd_fns = ash::khr::external_memory_fd::Device::new(&instance_raw, &raw);
        // SAFETY: the device was opened on this adapter with no features.
        let (device, queue) = unsafe {
            adapter.create_device_from_hal(
                hal_device,
                &wgpu::DeviceDescriptor { label: Some("formalshell"), required_limits: adapter.limits(), memory_hints: wgpu::MemoryHints::MemoryUsage, ..Default::default() },
            )
        }
        .map_err(|e| format!("wgpu device: {e}"))?;
        device.on_uncaptured_error(Arc::new(|e| eprintln!("formalshell: render: wgpu: {e}")));
        device.set_device_lost_callback(|reason, message| fail(format!("the GPU device was lost ({reason:?}): {message}")));

        let settings = vello_hybrid::RenderSettings {
            memory_settings: vello_hybrid::MemorySettings {
                image_atlas_config: vello_hybrid::AtlasConfig { initial_atlas_count: 0, max_atlases: 4, atlas_size: (ATLAS, ATLAS), auto_grow: true, ..Default::default() },
                ..Default::default()
            },
            ..Default::default()
        };
        let target = vello_hybrid::RenderTargetConfig { format: wgpu::TextureFormat::Rgba8Unorm, width: TILE as u32, height: TILE as u32 };
        let (renderer, resources) = vello_hybrid::Renderer::new_with(&device, &target, settings);
        let tile = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("formalshell tile"),
            size: wgpu::Extent3d { width: TILE as u32, height: TILE as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let tile_view = tile.create_view(&wgpu::TextureViewDescriptor::default());
        let mut gpu = Self {
            name,
            instance: instance_raw,
            physical,
            raw,
            family,
            fd_fns,
            device,
            queue,
            renderer,
            resources,
            scene: vello_hybrid::Scene::new(TILE as u16, TILE as u16),
            tile,
            tile_view,
            bindings: vello_hybrid::TextureBindings::new(),
            images: HashMap::new(),
            atlas_bytes: 0,
            next_texture: 1,
            frame: 0,
            uploads: None,
            fade: None,
            wl_queue,
            wl,
        };
        // A driver that cannot export one says so here, not on the first frame.
        gpu.export(16, 16).map_err(|e| format!("exporting a dmabuf: {e}"))?;
        Ok(gpu)
    }

    /// A linear RGBA image the compositor can read, its memory exported as
    /// a dmabuf, and the wl_buffer and wgpu texture over it.
    fn export(&mut self, w: u32, h: u32) -> Result<Buffer, String> {
        let raw = &self.raw;
        let modifiers = [DRM_FORMAT_MOD_LINEAR];
        let mut list = vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
        let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .extent(vk::Extent3D { width: w, height: h, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external)
            .push_next(&mut list);
        // SAFETY: every handle is this device's; on any failure what was
        // made is destroyed before returning.
        unsafe {
            let image = raw.create_image(&info, None).map_err(|e| format!("vkCreateImage: {e}"))?;
            let req = raw.get_image_memory_requirements(image);
            let props = self.instance.get_physical_device_memory_properties(self.physical);
            let types = props.memory_types_as_slice();
            let pick = |flags: vk::MemoryPropertyFlags| (0..types.len()).find(|&i| req.memory_type_bits & (1 << i) != 0 && types[i].property_flags.contains(flags));
            let Some(index) = pick(vk::MemoryPropertyFlags::DEVICE_LOCAL).or_else(|| pick(vk::MemoryPropertyFlags::empty())) else {
                raw.destroy_image(image, None);
                return Err("no memory type for the image".into());
            };
            let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let alloc = vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(index as u32).push_next(&mut export).push_next(&mut dedicated);
            let memory = match raw.allocate_memory(&alloc, None) {
                Ok(m) => m,
                Err(e) => {
                    raw.destroy_image(image, None);
                    return Err(format!("vkAllocateMemory: {e}"));
                }
            };
            let fail = |what: String| {
                raw.destroy_image(image, None);
                raw.free_memory(memory, None);
                what
            };
            raw.bind_image_memory(image, memory, 0).map_err(|e| fail(format!("vkBindImageMemory: {e}")))?;
            let fd = self
                .fd_fns
                .get_memory_fd(&vk::MemoryGetFdInfoKHR::default().memory(memory).handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT))
                .map_err(|e| fail(format!("vkGetMemoryFdKHR: {e}")))?;
            let fd = OwnedFd::from_raw_fd(fd);
            let plane = raw.get_image_subresource_layout(image, vk::ImageSubresource { aspect_mask: vk::ImageAspectFlags::MEMORY_PLANE_0_EXT, mip_level: 0, array_layer: 0 });

            let params = self.wl.dmabuf.create_params(&self.wl_queue.handle()).map_err(|e| fail(format!("linux-dmabuf params: {e}")))?;
            params.add(fd.as_fd(), 0, plane.offset as u32, plane.row_pitch as u32, DRM_FORMAT_MOD_LINEAR);
            let (wl_buffer, params) = params.create_immed(w as i32, h as i32, DRM_FORMAT_ABGR8888, zwp_linux_buffer_params_v1::Flags::empty(), &self.wl_queue.handle());
            params.destroy();
            drop(fd);

            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let hal_desc = wgpu::hal::TextureDescriptor {
                label: Some("formalshell dmabuf"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUses::COPY_DST | wgpu::TextureUses::COLOR_TARGET,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: Vec::new(),
            };
            let hal_device = self.device.as_hal::<wgpu::hal::api::Vulkan>().ok_or_else(|| fail("not a Vulkan device".into()))?;
            let hal_texture = hal_device.texture_from_raw(image, &hal_desc, None, wgpu::hal::vulkan::TextureMemory::Dedicated(memory));
            drop(hal_device);
            let texture = self.device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("formalshell dmabuf"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
            );
            let released = Rc::new(Cell::new(true));
            self.wl.released.insert(wl_buffer.id(), released.clone());
            Ok(Buffer { texture, image, wl_buffer, released, held: Cell::new(None), stale: Vec::new() })
        }
    }

    /// The picture's place on the GPU, uploaded on its first use.
    fn place(&mut self, pixmap: &Arc<Pixmap>) -> Option<&Placed> {
        let key = Arc::as_ptr(pixmap) as usize;
        let frame = self.frame;
        if !self.images.contains_key(&key) {
            let (w, h) = (pixmap.width(), pixmap.height());
            if w == 0 || h == 0 {
                return None;
            }
            let bytes = usize::from(w) * usize::from(h) * 4;
            let big = w > BIG || h > BIG;
            if !big && self.atlas_bytes + bytes > ATLAS_BUDGET {
                self.evict(bytes);
            }
            let placed = if big {
                let size = wgpu::Extent3d { width: u32::from(w), height: u32::from(h), depth_or_array_layers: 1 };
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("formalshell picture"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                self.queue.write_texture(
                    texture.as_image_copy(),
                    pixmap.data_as_u8_slice(),
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(u32::from(w) * 4), rows_per_image: None },
                    size,
                );
                let id = vello_hybrid::TextureId(self.next_texture);
                self.next_texture += 1;
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.bindings.insert(id, view);
                Placed::Own(id)
            } else {
                let encoder = self.uploads.get_or_insert_with(|| self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("formalshell uploads") }));
                let id = self.renderer.upload_image(&mut self.resources, &self.device, &self.queue, encoder, &Pixels(pixmap));
                self.atlas_bytes += bytes;
                Placed::Atlas(id)
            };
            self.images.insert(key, Uploaded { _pixmap: pixmap.clone(), placed, bytes, used: frame });
        }
        let entry = self.images.get_mut(&key)?;
        entry.used = frame;
        Some(&entry.placed)
    }

    /// Drops the atlas pictures unused longest until `want` more bytes fit
    /// the budget; nothing this frame drew is dropped.
    fn evict(&mut self, want: usize) {
        let mut old: Vec<(u64, usize)> = self.images.iter().filter(|(_, u)| matches!(u.placed, Placed::Atlas(_)) && u.used < self.frame).map(|(k, u)| (u.used, *k)).collect();
        old.sort_unstable();
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("formalshell evict") });
        for (_, key) in old {
            if self.atlas_bytes + want <= ATLAS_BUDGET / 2 {
                break;
            }
            if let Some(Uploaded { placed: Placed::Atlas(id), bytes, .. }) = self.images.remove(&key) {
                self.renderer.destroy_image(&mut self.resources, &mut encoder, id);
                self.atlas_bytes -= bytes;
            }
        }
        self.queue.submit([encoder.finish()]);
        // Pictures with textures of their own go when nothing drew them
        // for a while, so a closed wallpaper picker frees its memory.
        let frame = self.frame;
        let gone: Vec<usize> = self.images.iter().filter(|(_, u)| matches!(u.placed, Placed::Own(..)) && u.used + 600 < frame).map(|(k, _)| *k).collect();
        for key in gone {
            if let Some(Uploaded { placed: Placed::Own(id), .. }) = self.images.remove(&key) {
                self.bindings.remove(id);
            }
        }
    }

    /// Draws what `paint` puts in the tile's top-left `w` by `h` and copies
    /// it to `at` on `canvas`.
    fn draw(&mut self, canvas: &wgpu::Texture, at: (u32, u32), (w, h): (u32, u32), paint: impl FnOnce(&mut Frame)) {
        self.scene.reset();
        let mut frame = Frame { gpu: self, paint: Paint::Plain, paint_at: Affine::IDENTITY };
        paint(&mut frame);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("formalshell tile") });
        let size = vello_hybrid::RenderSize { width: TILE as u32, height: TILE as u32 };
        if let Err(e) = self.renderer.render(&self.scene, &mut self.resources, &self.device, &self.queue, &mut encoder, &size, &self.tile_view, &self.bindings) {
            eprintln!("formalshell: render: vello_hybrid: {e}");
        }
        encoder.copy_texture_to_texture(
            self.tile.as_image_copy(),
            wgpu::TexelCopyTextureInfo { texture: canvas, mip_level: 0, origin: wgpu::Origin3d { x: at.0, y: at.1, z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let uploads = self.uploads.take().map(|e| e.finish());
        self.queue.submit(uploads.into_iter().chain([encoder.finish()]));
    }
}

impl Gpu {
    /// A command buffer handing `image` to the compositor (`release`) from
    /// `layout`, or taking it back into `layout`: VK_QUEUE_FAMILY_FOREIGN_EXT
    /// in GENERAL is how an exported dmabuf is shared.
    fn ownership(&self, image: vk::Image, release: bool, layout: vk::ImageLayout) -> wgpu::CommandBuffer {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("formalshell ownership") });
        let writes = vk::AccessFlags::TRANSFER_WRITE | vk::AccessFlags::COLOR_ATTACHMENT_WRITE;
        let barrier = vk::ImageMemoryBarrier::default()
            .image(image)
            .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 });
        let barrier = if release {
            barrier
                .src_access_mask(writes)
                .dst_access_mask(vk::AccessFlags::empty())
                .old_layout(layout)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(self.family)
                .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
        } else {
            barrier
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(writes)
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                .dst_queue_family_index(self.family)
        };
        // SAFETY: the encoder is fresh and only recorded through hal here;
        // the image is a live export of this device's.
        unsafe {
            encoder.as_hal_mut::<wgpu::hal::api::Vulkan, _, _>(|e| {
                if let Some(e) = e {
                    let stages = vk::PipelineStageFlags::ALL_COMMANDS;
                    self.raw.cmd_pipeline_barrier(e.raw_handle(), stages, stages, vk::DependencyFlags::empty(), &[], &[], &[barrier]);
                }
            });
        }
        encoder.finish()
    }
}

/// A Pixmap from vello_cpu written into vello_hybrid's atlas: the same
/// premultiplied RGBA8 bytes under another crate version's type.
struct Pixels<'a>(&'a Arc<Pixmap>);

impl vello_hybrid::AtlasWriter for Pixels<'_> {
    fn width(&self) -> u32 {
        u32::from(self.0.width())
    }

    fn height(&self) -> u32 {
        u32::from(self.0.height())
    }

    /// Through the encoder rather than `queue.write_texture`, which runs
    /// ahead of every command buffer: an atlas grown in the same encoder
    /// copies its old layers over after, and would overwrite the upload.
    fn write_to_atlas_layer(&self, device: &wgpu::Device, _: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, atlas: &wgpu::Texture, layer: u32, offset: [u32; 2], width: u32, height: u32) {
        let row = 4 * width as usize;
        let stride = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let mut padded = vec![0; stride * height as usize];
        for (src, dst) in self.0.data_as_u8_slice().chunks_exact(row).zip(padded.chunks_exact_mut(stride)) {
            dst[..row].copy_from_slice(src);
        }
        let staging = wgpu::util::DeviceExt::create_buffer_init(device, &wgpu::util::BufferInitDescriptor {
            label: Some("formalshell upload"),
            contents: &padded,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo { buffer: &staging, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride as u32), rows_per_image: Some(height) } },
            wgpu::TexelCopyTextureInfo { texture: atlas, mip_level: 0, origin: wgpu::Origin3d { x: offset[0], y: offset[1], z: layer }, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }
}

/// The render node behind `dev`, a primary or a render node's device number.
struct Node {
    path: String,
    rdev: (u32, u32),
}

fn render_node(dev: libc::dev_t) -> Option<Node> {
    let (major, minor) = (libc::major(dev), libc::minor(dev));
    let drm = std::fs::read_dir(format!("/sys/dev/char/{major}:{minor}/device/drm")).ok()?;
    for entry in drm.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("renderD") {
            continue;
        }
        let numbers = std::fs::read_to_string(entry.path().join("dev")).ok()?;
        let (a, b) = numbers.trim().split_once(':')?;
        return Some(Node { path: format!("/dev/dri/{name}"), rdev: (a.parse().ok()?, b.parse().ok()?) });
    }
    None
}

/// The render node a Vulkan adapter drives, off VK_EXT_physical_device_drm.
///
/// # Safety
/// `adapter` must be a live Vulkan adapter.
unsafe fn drm_node(adapter: &wgpu::Adapter) -> Option<(u32, u32)> {
    // SAFETY: the hal adapter is the caller's live adapter.
    unsafe {
        let hal = adapter.as_hal::<wgpu::hal::api::Vulkan>()?;
        let instance = hal.shared_instance().raw_instance();
        let physical = hal.raw_physical_device();
        let have = instance.enumerate_device_extension_properties(physical).ok()?;
        if !have.iter().any(|p| p.extension_name_as_c_str().is_ok_and(|e| e == ash::ext::physical_device_drm::NAME)) {
            return None;
        }
        let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
        let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
        instance.get_physical_device_properties2(physical, &mut props);
        (drm.has_render == vk::TRUE).then_some((drm.render_major as u32, drm.render_minor as u32))
    }
}

/// What the next fill draws with.
enum Paint {
    Plain,
    /// A picture with a texture of its own: drawn as texture rects under a
    /// clip of the filled shape, faded by an opacity layer.
    Own { id: vello_hybrid::TextureId, size: (u16, u16), alpha: f32, quality: vello_cpu::peniko::ImageQuality },
}

/// One tile's scene, with pictures resolved to their GPU places.
pub struct Frame<'a> {
    gpu: &'a mut Gpu,
    paint: Paint,
    paint_at: Affine,
}

impl Frame<'_> {
    fn own(&mut self, shape: Option<&BezPath>, rect: Rect) {
        let Paint::Own { id, size, alpha, quality } = self.paint else { return };
        let paint_at = self.paint_at;
        let scene = &mut self.gpu.scene;
        if let Some(path) = shape {
            scene.push_clip_layer(path);
        } else {
            scene.push_clip_layer(&rect.to_path(0.1));
        }
        if alpha < 1.0 {
            scene.push_opacity_layer(alpha);
        }
        let source = vello_common_hybrid::geometry::RectU16 { x0: 0, y0: 0, x1: size.0, y1: size.1 };
        scene.draw_texture_rects(id, quality, [vello_hybrid::SampleRect { source_region: source, transform: paint_at }]);
        if alpha < 1.0 {
            scene.pop_layer();
        }
        scene.pop_layer();
    }
}

impl Raster for Frame<'_> {
    fn set_transform(&mut self, t: Affine) {
        self.gpu.scene.set_transform(t);
    }

    fn set_color(&mut self, c: AlphaColor<Srgb>) {
        self.paint = Paint::Plain;
        self.gpu.scene.set_paint(c);
    }

    fn set_gradient(&mut self, g: Gradient) {
        self.paint = Paint::Plain;
        self.gpu.scene.set_paint(g);
    }

    fn set_image(&mut self, pixmap: Arc<Pixmap>, sampler: ImageSampler) {
        let size = (pixmap.width(), pixmap.height());
        match self.gpu.place(&pixmap) {
            Some(Placed::Atlas(id)) => {
                // vello_hybrid takes no sampler alpha on a picture; a
                // multiply tint by premultiplied white at that alpha is the
                // same fade.
                let tint = (sampler.alpha < 1.0).then(|| Tint { color: AlphaColor::new([1.0, 1.0, 1.0, sampler.alpha]), mode: TintMode::Multiply });
                let image = Image { image: ImageSource::opaque_id(*id), sampler: sampler.with_alpha(1.0) };
                self.paint = Paint::Plain;
                self.gpu.scene.set_tint(tint);
                self.gpu.scene.set_paint(image);
            }
            Some(Placed::Own(id)) => {
                self.paint = Paint::Own { id: *id, size, alpha: sampler.alpha, quality: sampler.quality };
                self.gpu.scene.set_tint(None);
            }
            None => {
                self.paint = Paint::Plain;
                self.gpu.scene.set_paint(AlphaColor::<Srgb>::TRANSPARENT);
            }
        }
    }

    fn set_paint_transform(&mut self, t: Affine) {
        self.paint_at = t;
        self.gpu.scene.set_paint_transform(t);
    }

    fn reset_paint_transform(&mut self) {
        self.paint_at = Affine::IDENTITY;
        self.gpu.scene.reset_paint_transform();
    }

    fn set_stroke(&mut self, s: Stroke) {
        self.gpu.scene.set_stroke(s);
    }

    fn set_fill_rule(&mut self, f: Fill) {
        self.gpu.scene.set_fill_rule(f);
    }

    fn fill_path(&mut self, p: &BezPath) {
        match self.paint {
            Paint::Plain => self.gpu.scene.fill_path(p),
            Paint::Own { .. } => self.own(Some(p), p.bounding_box()),
        }
    }

    fn fill_rect(&mut self, r: &Rect) {
        match self.paint {
            Paint::Plain => self.gpu.scene.fill_rect(r),
            Paint::Own { .. } => self.own(None, *r),
        }
    }

    fn stroke_path(&mut self, p: &BezPath) {
        self.gpu.scene.stroke_path(p);
    }

    fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32) {
        self.gpu.scene.fill_blurred_rounded_rect(r, radius, std_dev, false);
    }

    fn push_clip_layer(&mut self, p: &BezPath) {
        self.gpu.scene.push_clip_layer(p);
    }

    fn push_group(&mut self) {
        self.gpu.scene.push_layer(None, None, None, None, None);
    }

    fn push_blend_layer(&mut self, b: BlendMode) {
        self.gpu.scene.push_blend_layer(b);
    }

    fn push_blur_layer(&mut self, radius: f32) {
        self.gpu.scene.push_filter_layer(Filter::from_function(FilterFunction::Blur { radius }));
    }

    fn pop_layer(&mut self) {
        self.gpu.scene.pop_layer();
    }
}

/// A surface's canvas: everything drawn since it was made, which the
/// buffers are copied out of.
pub struct Canvas {
    texture: wgpu::Texture,
    size: (u32, u32),
    /// The opacity copy's bind group over this canvas, made on first use.
    fade: Option<wgpu::BindGroup>,
}

impl Canvas {
    /// None once the device is gone.
    pub fn new(w: u32, h: u32) -> Option<Self> {
        let (w, h) = (w.max(1), h.max(1));
        with(|g| {
            let texture = g.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("formalshell canvas"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            Self { texture, size: (w, h), fade: None }
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// One tile's worth of `device` pixels drawn by `paint`, its origin at
    /// the tile's top-left.
    pub fn draw(&mut self, device: IRect, paint: impl FnOnce(&mut Frame)) {
        let texture = &self.texture;
        with(|g| g.draw(texture, (device.x as u32, device.y as u32), (device.w as u32, device.h as u32), paint));
    }

    /// `device` back to transparent.
    pub fn clear(&mut self, device: IRect) {
        if device.is_empty() {
            return;
        }
        let texture = &self.texture;
        with(|g| {
            let zero = vec![0u8; device.w as usize * device.h as usize * 4];
            g.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x: device.x as u32, y: device.y as u32, z: 0 }, aspect: wgpu::TextureAspect::All },
                &zero,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(device.w as u32 * 4), rows_per_image: None },
                wgpu::Extent3d { width: device.w as u32, height: device.h as u32, depth_or_array_layers: 1 },
            );
        });
    }
}

/// Splits a rect of canvas pixels into tiles.
pub fn tiles(r: IRect) -> impl Iterator<Item = IRect> {
    (r.y..r.bottom()).step_by(TILE as usize).flat_map(move |y| {
        (r.x..r.right()).step_by(TILE as usize).map(move |x| IRect::new(x, y, TILE.min(r.right() - x), TILE.min(r.bottom() - y)))
    })
}

/// A dmabuf the compositor reads, and what it still lacks of the canvas.
pub struct Buffer {
    texture: wgpu::Texture,
    image: vk::Image,
    wl_buffer: WlBuffer,
    /// Handed to the compositor (queue family FOREIGN, layout GENERAL),
    /// and the layout wgpu still believes it is in, to take it back into.
    held: Cell<Option<vk::ImageLayout>>,
    released: Rc<Cell<bool>>,
    pub stale: Vec<IRect>,
}

impl Buffer {
    /// None, and the GPU path given up, when the driver cannot make one.
    pub fn new(w: u32, h: u32) -> Option<Self> {
        with(|g| g.export(w, h)).and_then(|r| r.map_err(|e| fail(format!("exporting a {w}x{h} dmabuf: {e}"))).ok())
    }

    pub fn free(&self) -> bool {
        self.released.get()
    }

    pub fn wl_buffer(&self) -> &WlBuffer {
        &self.wl_buffer
    }

    /// Attached: the compositor holds it until it says otherwise.
    pub fn attached(&self) {
        self.released.set(false);
    }

    /// `rects` (canvas pixels) copied from `canvas`, at `opacity` out of
    /// 256, handed over to the compositor (Vulkan's external ownership
    /// release) and waited on, so it never reads a half-drawn buffer. False,
    /// and the GPU path given up, when the GPU did not finish.
    pub fn fill(&self, canvas: &mut Canvas, rects: &[IRect], opacity: u16) -> bool {
        with(|g| {
            let mut encoder = g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("formalshell present") });
            let layout = if opacity >= 256 {
                for r in rects {
                    let origin = wgpu::Origin3d { x: r.x as u32, y: r.y as u32, z: 0 };
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo { texture: &canvas.texture, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                        wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                        wgpu::Extent3d { width: r.w as u32, height: r.h as u32, depth_or_array_layers: 1 },
                    );
                }
                vk::ImageLayout::TRANSFER_DST_OPTIMAL
            } else {
                let fade = g.fade.get_or_insert_with(|| Fade::new(&g.device));
                g.queue.write_buffer(&fade.uniform, 0, &(f32::from(opacity) / 256.0).to_le_bytes());
                let group = canvas.fade.get_or_insert_with(|| fade.group(&g.device, &canvas.texture));
                let view = self.texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("formalshell fade"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&fade.pipeline);
                pass.set_bind_group(0, &*group, &[]);
                for r in rects {
                    pass.set_scissor_rect(r.x as u32, r.y as u32, r.w as u32, r.h as u32);
                    pass.draw(0..3, 0..1);
                }
                drop(pass);
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL
            };
            // wgpu cannot hand an image to another queue family, so the
            // ownership transfers go in command buffers of their own, around
            // the copy: the acquire puts the image back in the layout wgpu
            // last left it in, which is where its own tracking starts from.
            let acquire = self.held.take().map(|was| g.ownership(self.image, false, was));
            let release = g.ownership(self.image, true, layout);
            self.held.set(Some(layout));
            let index = g.queue.submit(acquire.into_iter().chain([encoder.finish(), release]));
            match g.device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(std::time::Duration::from_secs(1)) }) {
                Ok(_) => true,
                Err(e) => {
                    fail(format!("waiting on the GPU: {e}"));
                    false
                }
            }
        })
        .unwrap_or(false)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let id = self.wl_buffer.id();
        with(|g| g.wl.released.remove(&id));
        self.wl_buffer.destroy();
    }
}

/// Learns which buffers the compositor gave back since the last present.
pub fn dispatch() {
    with(|g| {
        if let Err(e) = g.wl_queue.dispatch_pending(&mut g.wl) {
            eprintln!("formalshell: render: linux-dmabuf: {e}");
        }
        g.frame += 1;
    });
}

/// The copy a faded subsurface takes on its way into a buffer: every
/// channel of the premultiplied canvas times one factor.
struct Fade {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
}

const FADE_WGSL: &str = r"
@group(0) @binding(0) var canvas: texture_2d<f32>;
@group(0) @binding(1) var<uniform> k: vec4<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(canvas, vec2<i32>(at.xy), 0) * k.x;
}
";

impl Fade {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("formalshell fade"), source: wgpu::ShaderSource::Wgsl(FADE_WGSL.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("formalshell fade"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("formalshell fade"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("formalshell fade"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("formalshell fade"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { pipeline, layout, uniform }
    }

    fn group(&self, device: &wgpu::Device, canvas: &wgpu::Texture) -> wgpu::BindGroup {
        let view = canvas.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("formalshell fade"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.uniform.as_entire_binding() },
            ],
        })
    }
}
