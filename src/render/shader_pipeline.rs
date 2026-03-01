use std::time::Instant;

use bytemuck::{Pod, Zeroable};

use crate::ui::layout::Rect;

// ---------------------------------------------------------------------------
// Uniforms
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ShaderUniforms {
    pub time: f32,
    pub _pad0: f32,
    pub resolution: [f32; 2],
    pub mouse: [f32; 2],
    pub zoom: f32,
    pub _pad1: f32,
    pub pan: [f32; 2],
    pub axis_min: [f32; 2],
    pub axis_max: [f32; 2],
    pub _pad2: [f32; 2],
    pub primary_color: [f32; 4],
    pub secondary_color: [f32; 4],
    pub background_color: [f32; 4],
}

impl Default for ShaderUniforms {
    fn default() -> Self {
        Self {
            time: 0.0,
            _pad0: 0.0,
            resolution: [800.0, 600.0],
            mouse: [0.0, 0.0],
            zoom: 1.0,
            _pad1: 0.0,
            pan: [0.0, 0.0],
            axis_min: [-5.0, -5.0],
            axis_max: [5.0, 5.0],
            _pad2: [0.0; 2],
            primary_color: [0.925, 0.655, 0.420, 1.0], // warm orange
            secondary_color: [0.506, 0.780, 0.518, 1.0], // soft green
            background_color: [0.063, 0.063, 0.063, 1.0], // dark bg
        }
    }
}

// ---------------------------------------------------------------------------
// Per-cell pipeline
// ---------------------------------------------------------------------------

struct CellPipeline {
    cell_id: usize,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
}

// ---------------------------------------------------------------------------
// Fullscreen triangle vertex shader (shared)
// ---------------------------------------------------------------------------

const VERTEX_SHADER_WGSL: &str = r#"
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Fullscreen triangle: 3 vertices cover the entire screen
    var pos: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = pos[vi];
    var out: VsOut;
    out.position = vec4<f32>(p.x, p.y, 0.0, 1.0);
    // Map clip coords to UV [0,1] with y flipped for conventional orientation
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return out;
}
"#;

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

pub struct ShaderPipelineManager {
    pipelines: Vec<CellPipeline>,
    vertex_shader: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    surface_format: wgpu::TextureFormat,
    start_time: Instant,
}

impl ShaderPipelineManager {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let vertex_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader_pipeline_vertex"),
            source: wgpu::ShaderSource::Wgsl(VERTEX_SHADER_WGSL.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader_pipeline_bgl"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shader_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        Self {
            pipelines: Vec::new(),
            vertex_shader,
            bind_group_layout,
            pipeline_layout,
            surface_format,
            start_time: Instant::now(),
        }
    }

    /// Compile a WGSL fragment shader and add a pipeline for the given cell.
    /// The `wgsl_source` should contain the full fragment shader module including
    /// the uniform struct and `fs_main` entry point.
    pub fn compile_and_add(
        &mut self,
        device: &wgpu::Device,
        cell_id: usize,
        wgsl_source: &str,
    ) -> Result<(), String> {
        // Remove any existing pipeline for this cell
        self.remove(cell_id);

        // Validate by trying to create the shader module
        // wgpu will panic or return an error for invalid WGSL
        let fragment_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell_fragment_shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader_uniforms"),
            size: std::mem::size_of::<ShaderUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cell_shader_pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex_shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
            multiview: None,
            cache: None,
        });

        self.pipelines.push(CellPipeline {
            cell_id,
            pipeline,
            bind_group,
            uniform_buffer,
        });

        Ok(())
    }

    /// Remove the pipeline for a given cell_id.
    pub fn remove(&mut self, cell_id: usize) {
        self.pipelines.retain(|p| p.cell_id != cell_id);
    }

    /// Returns true if any cell has an active shader pipeline.
    pub fn has_active(&self) -> bool {
        !self.pipelines.is_empty()
    }

    /// Returns the count of active pipelines.
    pub fn active_count(&self) -> usize {
        self.pipelines.len()
    }

    /// Render all active shader pipelines into the given surface view.
    /// All pipelines render to the full right pane, overlaid via alpha blending.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        right_pane: &Rect,
        screen_size: (u32, u32),
        axis_bounds: [f32; 4], // [x_min, y_min, x_max, y_max]
        mouse_uv: [f32; 2],
    ) {
        if self.pipelines.is_empty() {
            return;
        }

        let elapsed = self.start_time.elapsed().as_secs_f32();

        // Compute scissor rect once (same for all pipelines)
        let sx = (right_pane.x as u32).min(screen_size.0);
        let sy = (right_pane.y as u32).min(screen_size.1);
        let sw = (right_pane.w as u32).min(screen_size.0.saturating_sub(sx));
        let sh = (right_pane.h as u32).min(screen_size.1.saturating_sub(sy));

        for cp in self.pipelines.iter() {
            // Update uniforms with actual axis bounds from user interaction
            let uniforms = ShaderUniforms {
                time: elapsed,
                resolution: [right_pane.w, right_pane.h],
                mouse: mouse_uv,
                axis_min: [axis_bounds[0], axis_bounds[1]],
                axis_max: [axis_bounds[2], axis_bounds[3]],
                ..Default::default()
            };
            queue.write_buffer(&cp.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

            if sw == 0 || sh == 0 {
                continue;
            }

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shader_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: surface_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load, // preserve existing UI
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

                pass.set_scissor_rect(sx, sy, sw, sh);
                pass.set_viewport(
                    right_pane.x,
                    right_pane.y,
                    right_pane.w,
                    right_pane.h,
                    0.0,
                    1.0,
                );
                pass.set_pipeline(&cp.pipeline);
                pass.set_bind_group(0, &cp.bind_group, &[]);
                pass.draw(0..3, 0..1); // fullscreen triangle
            }
        }
    }
}
