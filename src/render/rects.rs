//! Batched instanced rect renderer with optional rounded corners.
//!
//! Draws N solid-color rectangles in a single instanced draw call.
//! Used for UI backgrounds, borders, separators, and the cursor.
//! Supports per-rect corner_radius for smooth rounded corners via SDF.

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RectInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
    pub corner_radius: f32,
    pub _padding: [f32; 3],
}

const MAX_RECTS: usize = 1024;

const RECT_SHADER: &str = r#"
struct ScreenUniform {
    size: vec2<f32>,
};

struct RectData {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: vec4<f32>,
    corner_radius: f32,
    _pad1: f32,
    _pad2: f32,
    _pad3: f32,
};

@group(0) @binding(0) var<uniform> screen: ScreenUniform;
@group(0) @binding(1) var<storage, read> rects: array<RectData>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_pos: vec2<f32>,
    @location(2) rect_size: vec2<f32>,
    @location(3) radius: f32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    let r = rects[ii];
    let uv = corners[vi];
    let pixel = vec2(r.x, r.y) + uv * vec2(r.w, r.h);
    let ndc = pixel / screen.size * 2.0 - 1.0;
    var out: VsOut;
    out.pos = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    out.color = r.color;
    out.local_pos = uv * vec2(r.w, r.h);
    out.rect_size = vec2(r.w, r.h);
    out.radius = r.corner_radius;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // Fast path: no rounding
    if in.radius <= 0.0 {
        return in.color;
    }

    // SDF for rounded rectangle
    let half_size = in.rect_size * 0.5;
    let p = in.local_pos - half_size;
    let q = abs(p) - half_size + vec2(in.radius);
    let d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - in.radius;

    // Anti-aliased edge (smooth over ~1px)
    let alpha = 1.0 - smoothstep(-0.5, 0.5, d);
    if alpha <= 0.001 {
        discard;
    }
    return vec4(in.color.rgb, in.color.a * alpha);
}
"#;

pub struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    screen_buf: wgpu::Buffer,
    storage_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl RectRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });

        // Uniform: screen size (vec2<f32> = 8 bytes, padded to 16)
        let screen_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect_screen_uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Storage buffer for rect instances
        let storage_size = (MAX_RECTS * std::mem::size_of::<RectInstance>()) as u64;
        let storage_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect_storage"),
            size: storage_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect_bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: storage_buf.as_entire_binding(),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect_pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            screen_buf,
            storage_buf,
            bind_group,
        }
    }

    /// Upload rects and record draw commands into an existing render pass.
    pub fn draw<'a>(
        &'a self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'a>,
        screen_width: f32,
        screen_height: f32,
        rects: &[RectInstance],
    ) {
        if rects.is_empty() {
            return;
        }
        if rects.len() > MAX_RECTS {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::warn!(
                    "rect renderer: {} rects exceeds MAX_RECTS ({}), truncating (this will not be logged again)",
                    rects.len(), MAX_RECTS
                );
            }
        }
        let count = rects.len().min(MAX_RECTS);

        // Upload screen size
        let screen_data: [f32; 4] = [screen_width, screen_height, 0.0, 0.0];
        queue.write_buffer(&self.screen_buf, 0, bytemuck::cast_slice(&screen_data));

        // Upload rect data
        queue.write_buffer(&self.storage_buf, 0, bytemuck::cast_slice(&rects[..count]));

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..6, 0..count as u32);
    }
}
