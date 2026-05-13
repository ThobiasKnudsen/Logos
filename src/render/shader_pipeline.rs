use std::collections::HashMap;
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
    /// Upper bound on the WGSL loop guard. Set to `MAX_LOOP_ITERATIONS` at
    /// render time. We pass it as a uniform — rather than a literal in the
    /// generated WGSL — to prevent driver shader compilers (notably NVIDIA's)
    /// from fully unrolling for-loops at compile time. With the literal, a
    /// loop body like `sum = sum * x` starting at 0 made the NVIDIA shader
    /// compiler hang for many seconds (or indefinitely) trying to symbolic-
    /// fold a constant-zero unrolled chain.
    pub max_loop_iter: u32,
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

/// Matches `MAX_LOOP_ITERATIONS` in `lang::wgsl_gen`. Written into the
/// `max_loop_iter` uniform every frame.
pub const MAX_LOOP_ITERATIONS: u32 = 10_000;

impl Default for ShaderUniforms {
    fn default() -> Self {
        Self {
            time: 0.0,
            max_loop_iter: MAX_LOOP_ITERATIONS,
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

/// One pipeline per `plot(...)` call. Analytic plots use the
/// fullscreen-triangle vertex shader and rely entirely on the
/// generated fragment shader; vertex plots (issue #28) take a custom
/// vertex+fragment WGSL together with an uploaded vertex buffer and
/// draw a line strip in world coordinates.
struct CellPipeline {
    cell_id: usize,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    /// Color the shader writes for "the cell's plot color" — one per
    /// pipeline so the renderer can use a different `primary_color` for each
    /// cell. Updating this doesn't require recompilation.
    primary_color: [f32; 4],
    /// Stable hash of the WGSL source this pipeline was built from.
    /// Used together with `vertex_hash` to short-circuit rebuilds when
    /// `set_cell_shaders` is called with content identical to what
    /// already exists for the same cell.
    wgsl_hash: u64,
    /// Vertex buffer for vertex plots. `None` for analytic plots — the
    /// renderer draws a fullscreen triangle directly.
    vertex_buffer: Option<wgpu::Buffer>,
    /// Number of vertices in `vertex_buffer`; `0` when not a vertex plot.
    vertex_count: u32,
    /// Stable hash of the uploaded vertex positions. `0` for analytic
    /// plots. The "skip re-upload when unchanged" half of issue #28
    /// is implemented as `existing.vertex_hash == new.vertex_hash` in
    /// `set_cell_shaders` so the buffer survives across replays.
    vertex_hash: u64,
}

/// Input for a single per-plot pipeline (one entry per `plot(...)` call).
/// The renderer's `set_cell_shaders` glue layer fills these in from the
/// notebook's `ShaderSpec` and the cell's plot color.
pub struct CellShaderInput<'a> {
    /// Premultiplied-alpha plot color the cell wants to display in.
    pub primary_color: [f32; 4],
    /// Complete WGSL — the analytic fragment-only path, or the
    /// `notebook::vertex_plot::VERTEX_PLOT_WGSL` pair for vertex plots.
    pub wgsl: &'a str,
    /// `Some` for vertex plots (issue #28). The renderer uploads the
    /// positions into a fresh `vec2<f32>` buffer and draws them as a
    /// line strip.
    pub vertices: Option<&'a [[f32; 2]]>,
    /// Hash of `vertices`. `0` for analytic plots. Used as the
    /// content key for the per-cell pipeline cache.
    pub vertex_hash: u64,
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
    /// Pipelines for non-active tabs, keyed by stable `tab_id` (not index —
    /// indices shift when tabs are closed).
    stashed: HashMap<u64, Vec<CellPipeline>>,
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
                // Vertex plots (issue #28) read `axis_min` / `axis_max`
                // from the same uniform inside their vertex shader, so
                // both stages must see the binding.
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        Self {
            pipelines: Vec::new(),
            stashed: HashMap::new(),
            vertex_shader,
            bind_group_layout,
            pipeline_layout,
            surface_format,
            start_time: Instant::now(),
        }
    }

    /// Replace every pipeline for `cell_id` with one pipeline per
    /// input. A cell with multiple `plot(...)` calls produces several
    /// entries — they all render into the same scissor rect and
    /// overlay via alpha blending.
    ///
    /// Pipelines whose content (WGSL hash + vertex hash) matches an
    /// existing pipeline for the same cell are reused — only their
    /// `primary_color` is touched. That's the "re-upload only on
    /// change" half of issue #28: re-playing a vertex plot with the
    /// same data preserves the GPU buffer rather than re-uploading.
    pub fn set_cell_shaders(
        &mut self,
        device: &wgpu::Device,
        cell_id: usize,
        inputs: &[CellShaderInput<'_>],
    ) -> Result<(), String> {
        // Pull every existing pipeline for this cell out so we can
        // try to match each new input against one of them. Anything
        // that doesn't get reclaimed below drops at end-of-scope and
        // releases its GPU resources.
        let mut available: Vec<CellPipeline> = Vec::new();
        let mut i = 0;
        while i < self.pipelines.len() {
            if self.pipelines[i].cell_id == cell_id {
                available.push(self.pipelines.swap_remove(i));
            } else {
                i += 1;
            }
        }

        let mut new_pipelines = Vec::with_capacity(inputs.len());
        let mut build_err: Option<String> = None;
        for input in inputs {
            if build_err.is_some() {
                break;
            }
            let wgsl_hash = hash_str(input.wgsl);
            let match_idx = available.iter().position(|p| {
                p.wgsl_hash == wgsl_hash && p.vertex_hash == input.vertex_hash
            });
            if let Some(idx) = match_idx {
                let mut reused = available.swap_remove(idx);
                reused.primary_color = input.primary_color;
                new_pipelines.push(reused);
                continue;
            }
            match self.build_pipeline(device, cell_id, input, wgsl_hash) {
                Ok(p) => new_pipelines.push(p),
                Err(e) => build_err = Some(e),
            }
        }

        if let Some(e) = build_err {
            // Partial-failure cleanup: keep the cell in a known-empty
            // state and let unused old pipelines drop with the local.
            return Err(e);
        }
        self.pipelines.extend(new_pipelines);
        Ok(())
    }

    fn build_pipeline(
        &self,
        device: &wgpu::Device,
        cell_id: usize,
        input: &CellShaderInput<'_>,
        wgsl_hash: u64,
    ) -> Result<CellPipeline, String> {
        if let Some(positions) = input.vertices {
            self.build_vertex_pipeline(device, cell_id, input, wgsl_hash, positions)
        } else {
            self.build_fragment_pipeline(device, cell_id, input, wgsl_hash)
        }
    }

    fn build_fragment_pipeline(
        &self,
        device: &wgpu::Device,
        cell_id: usize,
        input: &CellShaderInput<'_>,
        wgsl_hash: u64,
    ) -> Result<CellPipeline, String> {
        let primary_color = input.primary_color;
        let wgsl_source = input.wgsl;
        // Push an error scope so wgpu returns errors instead of panicking.
        let shader_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let fragment_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell_fragment_shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
        });

        // Check for shader compilation errors before proceeding.
        if let Some(err) = pollster::block_on(shader_scope.pop()) {
            return Err(format!("{}", err));
        }

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

        // Push another scope for pipeline creation (catches entry point errors, etc.)
        let pipeline_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cell_shader_pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment_shader,
                entry_point: Some("fs_main"),
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
            multiview_mask: None,
            cache: None,
        });

        if let Some(err) = pollster::block_on(pipeline_scope.pop()) {
            return Err(format!("{}", err));
        }

        Ok(CellPipeline {
            cell_id,
            pipeline,
            bind_group,
            uniform_buffer,
            primary_color,
            wgsl_hash,
            vertex_buffer: None,
            vertex_count: 0,
            vertex_hash: 0,
        })
    }

    fn build_vertex_pipeline(
        &self,
        device: &wgpu::Device,
        cell_id: usize,
        input: &CellShaderInput<'_>,
        wgsl_hash: u64,
        positions: &[[f32; 2]],
    ) -> Result<CellPipeline, String> {
        let primary_color = input.primary_color;
        let wgsl_source = input.wgsl;
        let shader_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        // Vertex+fragment WGSL — a single module that defines both
        // entry points. The fragment-only path can't reuse `vs_main`
        // because vertex plots compute their own clip-space mapping
        // from world coordinates inside the vertex shader.
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell_vertex_plot_shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
        });

        if let Some(err) = pollster::block_on(shader_scope.pop()) {
            return Err(format!("{}", err));
        }

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vertex_plot_uniforms"),
            size: std::mem::size_of::<ShaderUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vertex_plot_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Allocate the vertex buffer with the data pre-mapped so the
        // initial upload is part of buffer creation — no separate
        // `queue.write_buffer` call needed and we don't need the
        // queue handle here.
        let bytes: &[u8] = bytemuck::cast_slice(positions);
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vertex_plot_positions"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        vertex_buffer.get_mapped_range_mut(..).copy_from_slice(bytes);
        vertex_buffer.unmap();

        let pipeline_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            }],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vertex_plot_pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        if let Some(err) = pollster::block_on(pipeline_scope.pop()) {
            return Err(format!("{}", err));
        }

        Ok(CellPipeline {
            cell_id,
            pipeline,
            bind_group,
            uniform_buffer,
            primary_color,
            wgsl_hash,
            vertex_buffer: Some(vertex_buffer),
            vertex_count: positions.len() as u32,
            vertex_hash: input.vertex_hash,
        })
    }

    /// Update the primary color used by every pipeline belonging to
    /// `cell_id`. Cheap — no recompile, just a uniform write next frame.
    pub fn set_cell_primary_color(&mut self, cell_id: usize, primary_color: [f32; 4]) {
        for cp in self.pipelines.iter_mut() {
            if cp.cell_id == cell_id {
                cp.primary_color = primary_color;
            }
        }
        for pipelines in self.stashed.values_mut() {
            for cp in pipelines.iter_mut() {
                if cp.cell_id == cell_id {
                    cp.primary_color = primary_color;
                }
            }
        }
    }

    /// Remove every pipeline for the given `cell_id`.
    pub fn remove(&mut self, cell_id: usize) {
        self.pipelines.retain(|p| p.cell_id != cell_id);
    }

    /// Returns true if any cell has an active shader pipeline.
    pub fn has_active(&self) -> bool {
        !self.pipelines.is_empty()
    }

    /// Stash current pipelines for a tab (pause rendering without destroying
    /// GPU resources). Always clears `pipelines`, even if empty, so the
    /// caller can rely on the active list being empty afterwards.
    pub fn stash(&mut self, tab_id: u64) {
        let pipelines = std::mem::take(&mut self.pipelines);
        if !pipelines.is_empty() {
            self.stashed.insert(tab_id, pipelines);
        }
    }

    /// Restore previously stashed pipelines for a tab. Replaces the active
    /// list — callers must `stash` the outgoing tab first or its pipelines
    /// will leak (and keep rendering).
    pub fn restore(&mut self, tab_id: u64) {
        self.pipelines = self.stashed.remove(&tab_id).unwrap_or_default();
    }

    /// Drop stashed pipelines for a tab (e.g. when closing it).
    pub fn drop_stashed(&mut self, tab_id: u64) {
        self.stashed.remove(&tab_id);
    }

    /// Clear the active pipelines without stashing them — used when closing
    /// the active tab, since its pipelines have nowhere to go.
    pub fn clear_active(&mut self) {
        self.pipelines.clear();
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
                max_loop_iter: MAX_LOOP_ITERATIONS,
                resolution: [right_pane.w, right_pane.h],
                mouse: mouse_uv,
                axis_min: [axis_bounds[0], axis_bounds[1]],
                axis_max: [axis_bounds[2], axis_bounds[3]],
                primary_color: cp.primary_color,
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
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load, // preserve existing UI
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
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
                match &cp.vertex_buffer {
                    Some(vb) => {
                        // Vertex plot: bind the uploaded positions and
                        // draw them as a line strip. The pipeline's
                        // `primary_topology` was set to LineStrip at
                        // build time so adjacent vertices connect.
                        pass.set_vertex_buffer(0, vb.slice(..));
                        pass.draw(0..cp.vertex_count, 0..1);
                    }
                    None => {
                        // Fullscreen triangle for analytic plots.
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
    }
}

/// Stable 64-bit hash over a string. Used as the content key for
/// per-cell pipeline reuse in `set_cell_shaders`.
fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
