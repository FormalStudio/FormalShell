//! One full-window pass: the NV12 picture converted and filtered down to
//! the window, cut to the display's rounded shape with an antialiased
//! edge, a soft shadow under it, and the overlay over both.

use std::ptr::NonNull;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};

use crate::decode::Frame;

const SHADER: &str = r#"
struct U {
    origin: vec2f,
    size: vec2f,
    crop: vec2f,
    radius: f32,
    video: f32,
    scale: f32,
    turn: f32,
    angle: f32,
    zoom: f32,
};
@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var chroma: texture_2d<f32>;
@group(0) @binding(2) var over: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var<uniform> u: U;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

// Apple's continuous corner: each corner a superellipse that leaves the
// straight edge 1.528 radii from the corner (where iOS's continuous curve
// starts) and crosses the diagonal where the circle would, so curvature
// rises from zero instead of jumping. Must match overlay.rs's CORNER_*.
const CORNER_EXTENT: f32 = 1.52866;
const CORNER_EXPONENT: f32 = 3.26;

fn rrect(p: vec2f, half: vec2f, r: f32) -> f32 {
    let l = r * CORNER_EXTENT;
    let q = abs(p) - half + vec2f(l);
    let m = max(q, vec2f(0.0));
    let n = pow(pow(m.x, CORNER_EXPONENT) + pow(m.y, CORNER_EXPONENT), 1.0 / CORNER_EXPONENT);
    return n + min(max(q.x, q.y), 0.0) - l;
}

// A point of the picture as shown, turned `turn` quarters clockwise, back on
// the portrait panel. Must match ui/mod.rs's `unturn`.
fn unturn(q: vec2f) -> vec2f {
    switch (u32(u.turn) & 3u) {
        case 1u: { return vec2f(q.y, 1.0 - q.x); }
        case 2u: { return vec2f(1.0) - q; }
        case 3u: { return vec2f(1.0 - q.y, q.x); }
        default: { return q; }
    }
}

fn rgb(t: vec2f) -> vec3f {
    let y = textureSampleLevel(luma, samp, t, 0.0).r;
    let c = textureSampleLevel(chroma, samp, t, 0.0).rg - vec2f(0.5);
    // BT.709, full range: the phone tags nothing and sends 0..255.
    return vec3f(y + 1.5748 * c.y, y - 0.1873 * c.x - 0.4681 * c.y, y + 1.8556 * c.x);
}

// A window pixel, seen from the phone while it spins: turned back by the
// spin's angle about the phone's centre and grown back by its shrink.
fn unspin(p: vec2f, centre: vec2f) -> vec2f {
    let c = cos(u.angle);
    let s = sin(u.angle);
    let d = p - centre;
    return centre + vec2f(c * d.x + s * d.y, c * d.y - s * d.x) / u.zoom;
}

@fragment fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let half = u.size * 0.5;
    let centre = u.origin + half;
    let p = unspin(pos.xy, centre);
    // Distances back in window pixels for the edge's antialiasing.
    let d = rrect(p - centre, half, u.radius) * u.zoom;
    let cover = clamp(0.5 - d, 0.0, 1.0);
    // The shadow falls down the window, not down the turning phone.
    let ds = rrect(unspin(pos.xy - vec2f(0.0, 10.0 * u.scale), centre) - centre, half - vec2f(4.0 * u.scale), u.radius) * u.zoom;
    let soft = 26.0 * u.scale;
    let shadow = 0.42 * (1.0 - smoothstep(-soft, soft, ds));
    var base = vec4f(0.0, 0.0, 0.0, shadow);
    if (u.video > 0.5 && cover > 0.0) {
        // A box of 4x4 bilinear taps over the pixel's footprint: the
        // picture is shrunk three to four times, and one tap shimmers.
        let dims = vec2f(textureDimensions(luma));
        let t = unturn((p - u.origin) / u.size) * u.crop;
        // The window's pixel on the panel's axes, which a quarter turn swaps.
        let span = select(u.size, u.size.yx, (u32(u.turn) & 1u) == 1u);
        let step = u.crop / (span * u.zoom) / 4.0;
        var acc = vec3f(0.0);
        for (var j = 0; j < 4; j++) {
            for (var i = 0; i < 4; i++) {
                acc += rgb(t + (vec2f(f32(i), f32(j)) - vec2f(1.5)) * step);
            }
        }
        let c = clamp(acc / 16.0, vec3f(0.0), vec3f(1.0));
        base = vec4f(c * cover, cover) + base * (1.0 - cover);
    }
    let o = textureLoad(over, vec2i(pos.xy), 0);
    return o + base * (1.0 - o.a);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct Uniforms {
    origin: [f32; 2],
    size: [f32; 2],
    crop: [f32; 2],
    radius: f32,
    video: f32,
    scale: f32,
    turn: f32,
    angle: f32,
    zoom: f32,
}

/// Where the picture goes, in device pixels, and how much of the decoded
/// frame is screen (the rest is the encoder's padding).
pub struct Placement {
    pub origin: (f32, f32),
    pub size: (f32, f32),
    pub radius: f32,
    pub scale: f32,
    pub content: (u32, u32),
    /// Quarter turns clockwise to stand the picture up.
    pub turn: u8,
    /// While it spins: its extra turn clockwise about its centre, in
    /// radians, and the shrink that keeps it inside the window.
    pub angle: f32,
    pub zoom: f32,
}

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    video: Option<(wgpu::Texture, wgpu::Texture, (u32, u32))>,
    overlay: wgpu::Texture,
    pub has_video: bool,
}

impl Gpu {
    /// # Safety
    /// `display` and `surface` are live Wayland objects that outlive the `Gpu`.
    pub unsafe fn new(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void, size: (u32, u32)) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(NonNull::new(display).ok_or("no display")?));
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(NonNull::new(surface).ok_or("no surface")?));
        // SAFETY: the caller keeps both handles alive past the Gpu.
        let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(raw_display_handle), raw_window_handle }) }
            .map_err(|e| e.to_string())?;
        let adapter = futures_lite::future::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| e.to_string())?;
        tracing::info!("gpu: {}", adapter.get_info().name);
        let (device, queue) = futures_lite::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("iphone-mirror"), ..Default::default() }))
            .map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .unwrap_or(caps.formats[0]);
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else {
            caps.alpha_modes[0]
        };
        // Never block in present: frame callbacks pace the drawing, and the
        // compositor takes the newest buffer at its next repaint.
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) { wgpu::PresentMode::Mailbox } else { wgpu::PresentMode::Fifo };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0.max(1),
            height: size.1.max(1),
            present_mode,
            desired_maximum_frame_latency: 1,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        tracing::info!("surface: {format:?}, {alpha_mode:?}, {present_mode:?}");

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mirror"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mirror"),
            entries: &[
                texture(0),
                texture(1),
                texture(2),
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mirror"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mirror"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mirror"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mirror"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let overlay = Self::overlay_texture(&device, size);
        Ok(Self { surface, device, queue, config, pipeline, layout, sampler, uniforms, video: None, overlay, has_video: false })
    }

    fn overlay_texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay"),
            size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    pub fn resize(&mut self, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if (self.config.width, self.config.height) == size {
            return;
        }
        self.config.width = size.0;
        self.config.height = size.1;
        self.surface.configure(&self.device, &self.config);
        self.overlay = Self::overlay_texture(&self.device, size);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Premultiplied RGBA at the window's device size.
    pub fn set_overlay(&mut self, rgba: &[u8], size: (u32, u32)) {
        if size != self.size() {
            return;
        }
        self.queue.write_texture(
            self.overlay.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * 4), rows_per_image: None },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
    }

    pub fn set_frame(&mut self, frame: &Frame) {
        let (w, h) = (frame.width, frame.height);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        if self.video.as_ref().is_none_or(|v| v.2 != (w, h)) {
            let make = |label, width, height, format| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            self.video = Some((make("luma", w, h, wgpu::TextureFormat::R8Unorm), make("chroma", cw, ch, wgpu::TextureFormat::Rg8Unorm), (w, h)));
        }
        let (luma, chroma, _) = self.video.as_ref().unwrap();
        let split = (w * h) as usize;
        self.queue.write_texture(
            luma.as_image_copy(),
            &frame.data[..split],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: None },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.write_texture(
            chroma.as_image_copy(),
            &frame.data[split..],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(cw * 2), rows_per_image: None },
            wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        );
        self.has_video = true;
    }

    /// The decoded frame's size, while there is one.
    pub fn frame_size(&self) -> Option<(u32, u32)> {
        self.video.as_ref().filter(|_| self.has_video).map(|v| v.2)
    }

    pub fn render(&mut self, at: &Placement) -> Result<(), String> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                self.surface.configure(&self.device, &self.config);
                return Err(format!("surface: {other:?}"));
            }
        };
        let (fw, fh) = self.frame_size().unwrap_or(at.content);
        let u = Uniforms {
            origin: [at.origin.0, at.origin.1],
            size: [at.size.0, at.size.1],
            crop: [at.content.0 as f32 / fw as f32, at.content.1 as f32 / fh as f32],
            radius: at.radius,
            video: if self.has_video { 1.0 } else { 0.0 },
            scale: at.scale,
            turn: at.turn as f32,
            angle: at.angle,
            zoom: at.zoom,
        };
        // SAFETY: Uniforms is plain f32s, repr(C), no padding beyond its own fields.
        let bytes = unsafe { std::slice::from_raw_parts(&u as *const Uniforms as *const u8, std::mem::size_of::<Uniforms>()) };
        self.queue.write_buffer(&self.uniforms, 0, bytes);
        if self.video.is_none() {
            // Bind something until the first frame lands.
            let blank = Frame { width: 2, height: 2, data: vec![0, 0, 0, 0, 128, 128], received: std::time::Instant::now(), decoded: std::time::Instant::now() };
            self.set_frame(&blank);
            self.has_video = false;
        }
        let (luma, chroma, _) = self.video.as_ref().unwrap();
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (lv, cv, ov) = (view(luma), view(chroma), view(&self.overlay));
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mirror"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&lv) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&cv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&ov) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: self.uniforms.as_entire_binding() },
            ],
        });
        let target = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mirror"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}
