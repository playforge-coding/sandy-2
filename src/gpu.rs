//! All wgpu state: device/surface setup, the grid texture, and the per-frame
//! upload + draw. The physics (in `sim`) is completely GPU-agnostic; this file
//! copies the simulation's RGBA buffer into a texture and renders it with a
//! selective bloom so emissive materials (fire, lava) glow.
//!
//! The draw is three fullscreen passes (see `shader.wgsl`): extract+blur the
//! glowing pixels horizontally into `glow_a`, blur that vertically into
//! `glow_b`, then composite the crisp grid texture plus the blurred glow to the
//! window. The glow buffers are kept at the grid resolution (they're tiny and
//! the halo is soft anyway), so only the surface needs reconfiguring on resize.

use std::sync::Arc;

use winit::window::Window;

use crate::sim::{Simulation, GRID_H, GRID_W};

/// How far the glow spreads, in grid texels per blur tap. Larger = wider halo.
const GLOW_SPREAD: f32 = 1.5;

pub struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    // Bloom pipeline: extract+blur-h → blur-v → composite.
    pipeline_blur_h: wgpu::RenderPipeline,
    pipeline_blur_v: wgpu::RenderPipeline,
    pipeline_composite: wgpu::RenderPipeline,

    bg_blur_h: wgpu::BindGroup,
    bg_blur_h_step: wgpu::BindGroup,
    bg_blur_v: wgpu::BindGroup,
    bg_blur_v_step: wgpu::BindGroup,
    bg_composite: wgpu::BindGroup,

    grid_texture: wgpu::Texture,
    // Offscreen render targets for the two blur passes (grid resolution).
    glow_a_view: wgpu::TextureView,
    glow_b_view: wgpu::TextureView,

    /// CPU-side RGBA scratch buffer, re-filled from the sim every frame.
    pixels: Vec<u8>,

    /// egui's wgpu backend — turns the tessellated UI into draw calls layered
    /// over the composited scene each frame (see [`State::render`]).
    egui_renderer: egui_wgpu::Renderer,

    pub sim: Simulation,
}

/// Pack the 16-byte `Blur` uniform (a per-tap UV offset, padded to vec4).
fn blur_step_bytes(step_x: f32, step_y: f32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&step_x.to_le_bytes());
    b[4..8].copy_from_slice(&step_y.to_le_bytes());
    b
}

impl State {
    pub async fn new(window: Arc<Window>) -> State {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        // Arc<Window> gives us a 'static surface that keeps the window alive.
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("no suitable GPU adapter found");

        // On the web (WebGL) we must stay within the conservative downlevel
        // limits; native can use the full defaults.
        let required_limits = if cfg!(target_arch = "wasm32") {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::default()
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sandy device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                ..Default::default()
            })
            .await
            .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        // Prefer an sRGB surface so colours round-trip cleanly with our sRGB
        // grid texture; otherwise take whatever the surface offers.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        // Match the grid texture's colour space to the surface so the blit is
        // a 1:1 passthrough. The glow buffers use the same format, so the bloom
        // maths happen in the same (linear-light, via sRGB) space.
        let tex_format = if format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo, // vsync, universally supported
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // ---- Textures: the grid (one texel per cell) and two glow buffers ----
        let grid_size = wgpu::Extent3d {
            width: GRID_W as u32,
            height: GRID_H as u32,
            depth_or_array_layers: 1,
        };
        let grid_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grid"),
            size: grid_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: tex_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let grid_view = grid_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // The blur passes ping-pong through these, both sampled and rendered to.
        let make_glow = |label: &str| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: grid_size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: tex_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let glow_a_view = make_glow("glow a");
        let glow_b_view = make_glow("glow b");

        // Nearest for the crisp grid (and the exact glow mask); linear for the
        // glow buffers so the halo upsamples smoothly.
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // ---- Blur step uniforms (one per direction) ----
        let blur_h_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur h step"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_v_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur v step"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &blur_h_buf,
            0,
            &blur_step_bytes(GLOW_SPREAD / GRID_W as f32, 0.0),
        );
        queue.write_buffer(
            &blur_v_buf,
            0,
            &blur_step_bytes(0.0, GLOW_SPREAD / GRID_H as f32),
        );

        // ---- Bind group layouts ----
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };

        // group(0) for the blur passes: one input texture + its sampler.
        let bgl_in = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur input bgl"),
            entries: &[tex_entry(0), sampler_entry(1)],
        });
        // group(1): the blur step uniform.
        let bgl_step = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur step bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        // group(0) for the composite: scene texture + sampler, glow texture + sampler.
        let bgl_composite = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite bgl"),
            entries: &[
                tex_entry(0),
                sampler_entry(1),
                tex_entry(2),
                sampler_entry(3),
            ],
        });

        // ---- Bind groups ----
        let bg_blur_h = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur h input"),
            layout: &bgl_in,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&grid_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&nearest),
                },
            ],
        });
        let bg_blur_v = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur v input"),
            layout: &bgl_in,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&glow_a_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&linear),
                },
            ],
        });
        let bg_blur_h_step = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur h step"),
            layout: &bgl_step,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: blur_h_buf.as_entire_binding(),
            }],
        });
        let bg_blur_v_step = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur v step"),
            layout: &bgl_step,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: blur_v_buf.as_entire_binding(),
            }],
        });
        let bg_composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &bgl_composite,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&grid_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&glow_b_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&linear),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // ---- Pipelines ----
        let blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur layout"),
            bind_group_layouts: &[Some(&bgl_in), Some(&bgl_step)],
            immediate_size: 0,
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite layout"),
            bind_group_layouts: &[Some(&bgl_composite)],
            immediate_size: 0,
        });

        // Small helper so the three pipelines only differ by entry point/target.
        let make_pipeline = |label: &str,
                             layout: &wgpu::PipelineLayout,
                             fs_entry: &str,
                             target: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs_entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        let pipeline_blur_h = make_pipeline("blur h", &blur_layout, "fs_blur_h", tex_format);
        let pipeline_blur_v = make_pipeline("blur v", &blur_layout, "fs_blur_v", tex_format);
        let pipeline_composite = make_pipeline(
            "composite",
            &composite_layout,
            "fs_composite",
            config.format,
        );

        // egui paints into the surface format, in its own pass after the bloom
        // composite. Defaults are fine: no MSAA, no depth, feathered AA on.
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        );

        // Open onto a freshly-generated world rather than an empty grid.
        let mut sim = Simulation::new();
        crate::worldgen::generate(&mut sim, crate::worldgen::DEFAULT_SEED);
        let pixels = vec![0u8; GRID_W * GRID_H * 4];

        State {
            window,
            surface,
            device,
            queue,
            config,
            pipeline_blur_h,
            pipeline_blur_v,
            pipeline_composite,
            bg_blur_h,
            bg_blur_h_step,
            bg_blur_v,
            bg_blur_v_step,
            bg_composite,
            grid_texture,
            glow_a_view,
            glow_b_view,
            pixels,
            egui_renderer,
            sim,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// A cloned handle to the window, for callers (e.g. egui input plumbing in
    /// `app`) that need to hold it past a borrow of `self`.
    pub fn window_arc(&self) -> Arc<Window> {
        self.window.clone()
    }

    /// The device's maximum 2D texture dimension — egui clamps its font/image
    /// atlas to this.
    pub fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Map a physical cursor position (window pixels) to a grid cell.
    pub fn cursor_to_grid(&self, pos: (f64, f64)) -> (i32, i32) {
        let gx = (pos.0 / self.config.width as f64 * self.sim.width as f64) as i32;
        let gy = (pos.1 / self.config.height as f64 * self.sim.height as f64) as i32;
        (gx, gy)
    }

    /// Advance the simulation one tick and upload the rendered grid to the GPU.
    /// The UI is no longer stamped into the pixel buffer — egui draws it as a
    /// separate pass in [`State::render`].
    pub fn update(&mut self) {
        self.sim.step();
        self.sim.render_into(&mut self.pixels);
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.grid_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(GRID_W as u32 * 4),
                rows_per_image: Some(GRID_H as u32),
            },
            wgpu::Extent3d {
                width: GRID_W as u32,
                height: GRID_H as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draw the grid texture to the window, blooming the emissive cells, then
    /// layer the egui interface on top. `paint_jobs`/`textures_delta` come from
    /// `egui::Context::tessellate`/`run` (driven in `app`); `pixels_per_point`
    /// is the display scale egui laid the UI out at.
    pub fn render(
        &mut self,
        paint_jobs: Vec<egui::ClippedPrimitive>,
        textures_delta: egui::TexturesDelta,
        pixels_per_point: f32,
    ) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            // Surface needs reconfiguring; skip this frame and fix it up.
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            // Transient (occluded/timeout/validation): just skip the frame.
            _ => return,
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        // Small helper: a single fullscreen pass into `target` with `pipeline`
        // and the given bind groups (group 1 is optional).
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    label: &str,
                    target: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    bg0: &wgpu::BindGroup,
                    bg1: Option<&wgpu::BindGroup>| {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, bg0, &[]);
            if let Some(bg1) = bg1 {
                rp.set_bind_group(1, bg1, &[]);
            }
            rp.draw(0..3, 0..1);
        };

        // 1. Extract glowing pixels + horizontal blur → glow_a.
        pass(
            &mut encoder,
            "blur h pass",
            &self.glow_a_view,
            &self.pipeline_blur_h,
            &self.bg_blur_h,
            Some(&self.bg_blur_h_step),
        );
        // 2. Vertical blur → glow_b.
        pass(
            &mut encoder,
            "blur v pass",
            &self.glow_b_view,
            &self.pipeline_blur_v,
            &self.bg_blur_v,
            Some(&self.bg_blur_v_step),
        );
        // 3. Composite the crisp scene + blurred glow → window.
        pass(
            &mut encoder,
            "composite pass",
            &view,
            &self.pipeline_composite,
            &self.bg_composite,
            None,
        );

        // 4. egui on top, loading (not clearing) the composited scene.
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        for (id, delta) in &textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let user_cmd_bufs = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );
        {
            // egui's renderer wants a 'static pass; `forget_lifetime` detaches
            // it from the borrow of `view`, which outlives the pass anyway.
            let mut rp = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut rp, &paint_jobs, &screen);
        }

        self.queue.submit(
            user_cmd_bufs
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        frame.present();

        // Free any textures egui retired this frame (after submit, so they
        // aren't dropped while still referenced by in-flight commands).
        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
    }
}
