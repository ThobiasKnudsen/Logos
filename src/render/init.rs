use std::sync::Arc;

use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Family, FontSystem, Metrics, Shaping, Style, SwashCache,
    TextAtlas, TextRenderer,
};
use wgpu::{
    DeviceDescriptor, Extent3d, Instance, InstanceDescriptor, MultisampleState, PowerPreference,
    PresentMode, RequestAdapterOptions, SurfaceConfiguration, TextureDimension, TextureUsages,
    TextureViewDescriptor,
};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::app;
use crate::ui::layout::Rect;
use crate::ui::theme::{font_family, fonts};

use super::rects::RectRenderer;
use super::shader_pipeline::ShaderPipelineManager;
use super::{Renderer, COMPOSITE_SHADER_WGSL, MAX_AXIS_LABELS};

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();

        let instance = Instance::new(InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        log::info!("GPU: {}", adapter.get_info().name);
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default(), None)
            .await
            .unwrap();
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let caps = surface.get_capabilities(&adapter);
        let swapchain_format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .or_else(|| caps.formats.first().copied())
            .expect("adapter returned no surface formats");
        let present_mode = if caps.present_modes.contains(&PresentMode::Immediate) {
            PresentMode::Immediate
        } else {
            PresentMode::Fifo
        };
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 3,
        };
        surface.configure(&device, &surface_config);

        let mut font_system = FontSystem::new();
        for family in font_family::FAMILIES {
            for &bytes in family.font_data {
                font_system.db_mut().load_font_data(bytes.to_vec());
            }
        }
        // Chrome-only fonts (the "Λ" logo and the italic face used for
        // untitled tab labels). Registered once but never selected through
        // the Fonts menu.
        font_system
            .db_mut()
            .load_font_data(font_family::LOGO_FONT_DATA.to_vec());
        font_system
            .db_mut()
            .load_font_data(font_family::ITALIC_CHROME_FONT_DATA.to_vec());
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = glyphon::Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, swapchain_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, MultisampleState::default(), None);

        let menu_item_labels: Vec<TextBuffer> = app::MENU_NAMES
            .iter()
            .map(|name| Self::create_label(&mut font_system, fonts::menu_size(), name))
            .collect();

        let logo_label = Self::create_label_with_family(
            &mut font_system,
            fonts::logo_size(),
            "\u{039B}",
            font_family::LOGO_FONT_FAMILY,
        );

        let status_label = Self::create_label(
            &mut font_system,
            fonts::status_size(),
            "Ready \u{2502} Ln 1, Col 1",
        );
        let plus_label = Self::create_label(&mut font_system, fonts::ui_size(), "+");
        let dot_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{25CF}");

        let win_min_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{2500}");
        let win_max_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{25A1}");
        let win_close_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{00D7}");

        let feedback_label =
            Self::create_label(&mut font_system, fonts::menu_size(), "Help us improve!");

        let color_picker_labels = [
            Self::create_label(&mut font_system, fonts::small_size(), "R"),
            Self::create_label(&mut font_system, fonts::small_size(), "G"),
            Self::create_label(&mut font_system, fonts::small_size(), "B"),
            Self::create_label(&mut font_system, fonts::small_size(), "A"),
        ];

        let rect_renderer = RectRenderer::new(&device, swapchain_format);
        let shader_pipeline = ShaderPipelineManager::new(&device, swapchain_format);

        let axis_cache = Cache::new(&device);
        let mut axis_atlas = TextAtlas::new(&device, &queue, &axis_cache, swapchain_format);
        let axis_text_renderer =
            TextRenderer::new(&mut axis_atlas, &device, MultisampleState::default(), None);
        let overlay_rect_renderer = RectRenderer::new(&device, swapchain_format);
        let axis_label_buffers: Vec<TextBuffer> = (0..MAX_AXIS_LABELS * 2)
            .map(|_| Self::create_label(&mut font_system, fonts::small_size(), "0"))
            .collect();
        let cursor_x_label = Self::create_label(&mut font_system, fonts::ui_size(), "0");
        let cursor_y_label = Self::create_label(&mut font_system, fonts::ui_size(), "0");
        let cursor_rect_renderer = RectRenderer::new(&device, swapchain_format);
        let cursor_text_cache = Cache::new(&device);
        let mut cursor_text_atlas =
            TextAtlas::new(&device, &queue, &cursor_text_cache, swapchain_format);
        let cursor_text_renderer = TextRenderer::new(
            &mut cursor_text_atlas,
            &device,
            MultisampleState::default(),
            None,
        );

        let (scene_texture, scene_view) = Self::create_offscreen_texture(
            &device,
            physical_size.width,
            physical_size.height,
            swapchain_format,
            "scene_texture",
        );
        let (overlay_texture, overlay_view) = Self::create_offscreen_texture(
            &device,
            physical_size.width,
            physical_size.height,
            swapchain_format,
            "overlay_texture",
        );
        let composite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("composite_sampler"),
            // Linear instead of Nearest: scene/overlay textures should
            // match the surface 1:1, but on fractional-DPI scale factors
            // (e.g. 1.25, 1.5) the composite UV→texel mapping can land
            // a hair off-center, which Nearest snaps to a single texel
            // (visible "pixelation" on glyph edges). Linear is cheap
            // and gives a clean blend across the half-texel boundary.
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("composite_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite_shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SHADER_WGSL.into()),
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("composite_pl"),
                bind_group_layouts: &[&composite_bind_group_layout],
                push_constant_ranges: &[],
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite_pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: swapchain_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let composite_bind_group = Self::create_composite_bind_group(
            &device,
            &composite_bind_group_layout,
            &scene_view,
            &overlay_view,
            &composite_sampler,
        );

        let add_cell_label = Self::create_label(&mut font_system, fonts::ui_size(), "+");
        let cell_delete_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{2715}");
        let cell_copy_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{2398}");
        let cell_play_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{25B6}");
        let cell_stop_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{25A0}");
        let cell_color_label = Self::create_label(&mut font_system, fonts::small_size(), "color");
        let cell_chevron_right =
            Self::create_label(&mut font_system, fonts::small_size(), "\u{25B6}");
        let cell_chevron_down =
            Self::create_label(&mut font_system, fonts::small_size(), "\u{25BC}");
        let output_label = Self::create_label(&mut font_system, fonts::small_size(), "Output");
        let tooltip_label = Self::create_label(&mut font_system, fonts::small_size(), "Ctrl+Enter");

        let zero_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };

        Self {
            device,
            queue,
            surface,
            surface_config,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            cell_buffers: Vec::new(),
            cell_texts: Vec::new(),
            cell_output_buffers: Vec::new(),
            cell_output_texts: Vec::new(),
            cell_output_is_error: Vec::new(),
            cell_layouts: Vec::new(),
            active_cell_index: 0,
            cursor_content_pos: (0.0, 0.0, fonts::editor_line_height()),
            selection_content_rects: Vec::new(),
            cell_scroll_y: 0.0,
            cells_total_height: 0.0,
            cached_editor_pane: zero_rect,
            add_cell_label,
            add_cell_rect: zero_rect,
            cell_delete_label,
            cell_copy_label,
            cell_play_label,
            cell_stop_label,
            cell_color_label,
            cell_chevron_right,
            cell_chevron_down,
            output_label,
            tooltip_label,
            cell_playing: Vec::new(),
            cell_output_scroll_x: Vec::new(),
            cell_editor_scroll_x: Vec::new(),
            cell_editor_scroll_y: Vec::new(),
            cell_content_heights: Vec::new(),
            prev_active_cell: usize::MAX,
            prev_cursor_byte: usize::MAX,
            v_track_rect: None,
            v_thumb_rect: None,
            status_label,
            cached_status_text: String::new(),
            menu_item_labels,
            menu_item_rects: Vec::new(),
            logo_label,
            logo_rect: zero_rect,
            dropdown_item_labels: Vec::new(),
            dropdown_shortcut_labels: Vec::new(),
            dropdown_bg: zero_rect,
            dropdown_item_rects: Vec::new(),
            dropdown_active: false,
            dropdown_active_item: None,
            cached_tab_info: Vec::new(),
            tab_labels: Vec::new(),
            tab_close_labels: Vec::new(),
            tab_modified: Vec::new(),
            dot_label,
            tab_bg_rects: Vec::new(),
            tab_close_rects: Vec::new(),
            plus_label,
            plus_rect: zero_rect,
            win_min_label,
            win_max_label,
            win_close_label,
            feedback_label,
            feedback_button_rect: zero_rect,
            color_picker_bg: zero_rect,
            color_picker_sliders: [zero_rect, zero_rect, zero_rect, zero_rect],
            color_picker_labels,
            ac_active: false,
            ac_bg: zero_rect,
            ac_item_rects: Vec::new(),
            ac_item_labels: Vec::new(),
            ac_kind_labels: Vec::new(),
            ac_selected_index: 0,
            rect_renderer,
            shader_pipeline,
            overlay_rect_renderer,
            axis_atlas,
            axis_text_renderer,
            axis_label_buffers,
            cursor_x_label,
            cursor_y_label,
            cursor_rect_renderer,
            cursor_text_atlas,
            cursor_text_renderer,
            scene_texture,
            scene_view,
            overlay_texture,
            overlay_view,
            composite_pipeline,
            composite_bind_group_layout,
            composite_bind_group,
            composite_sampler,
        }
    }

    fn create_offscreen_texture(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        (texture, view)
    }

    fn create_composite_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        scene_view: &wgpu::TextureView,
        overlay_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(scene_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(overlay_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);

        let (scene_texture, scene_view) = Self::create_offscreen_texture(
            &self.device,
            new_size.width,
            new_size.height,
            self.surface_config.format,
            "scene_texture",
        );
        let (overlay_texture, overlay_view) = Self::create_offscreen_texture(
            &self.device,
            new_size.width,
            new_size.height,
            self.surface_config.format,
            "overlay_texture",
        );
        self.composite_bind_group = Self::create_composite_bind_group(
            &self.device,
            &self.composite_bind_group_layout,
            &scene_view,
            &overlay_view,
            &self.composite_sampler,
        );
        self.scene_texture = scene_texture;
        self.scene_view = scene_view;
        self.overlay_texture = overlay_texture;
        self.overlay_view = overlay_view;
    }

    /// Recreate all label buffers at current font sizes (after zoom change).
    pub fn rebuild_labels(&mut self) {
        self.menu_item_labels = app::MENU_NAMES
            .iter()
            .map(|name| Self::create_label(&mut self.font_system, fonts::menu_size(), name))
            .collect();
        self.status_label = Self::create_label(&mut self.font_system, fonts::status_size(), "");
        self.plus_label = Self::create_label(&mut self.font_system, fonts::ui_size(), "+");
        self.dot_label = Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{25CF}");
        self.win_min_label =
            Self::create_label(&mut self.font_system, fonts::menu_size(), "\u{2500}");
        self.win_max_label =
            Self::create_label(&mut self.font_system, fonts::menu_size(), "\u{25A1}");
        self.win_close_label =
            Self::create_label(&mut self.font_system, fonts::menu_size(), "\u{00D7}");
        self.feedback_label =
            Self::create_label(&mut self.font_system, fonts::menu_size(), "Help us improve!");
        self.logo_label = Self::create_label_with_family(
            &mut self.font_system,
            fonts::logo_size(),
            "\u{039B}",
            font_family::LOGO_FONT_FAMILY,
        );
        self.color_picker_labels = [
            Self::create_label(&mut self.font_system, fonts::small_size(), "R"),
            Self::create_label(&mut self.font_system, fonts::small_size(), "G"),
            Self::create_label(&mut self.font_system, fonts::small_size(), "B"),
            Self::create_label(&mut self.font_system, fonts::small_size(), "A"),
        ];

        for buf in &mut self.cell_buffers {
            buf.set_metrics(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            buf.shape_until_scroll(&mut self.font_system, false);
        }
        for buf in &mut self.cell_output_buffers {
            buf.set_metrics(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            buf.shape_until_scroll(&mut self.font_system, false);
        }

        self.add_cell_label = Self::create_label(&mut self.font_system, fonts::ui_size(), "+");
        self.cell_delete_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2715}");
        self.cell_copy_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2398}");
        self.cell_play_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{25B6}");
        self.cell_stop_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{25A0}");
        self.cell_color_label =
            Self::create_label(&mut self.font_system, fonts::small_size(), "color");
        self.cell_chevron_right =
            Self::create_label(&mut self.font_system, fonts::small_size(), "\u{25B6}");
        self.cell_chevron_down =
            Self::create_label(&mut self.font_system, fonts::small_size(), "\u{25BC}");
        self.output_label =
            Self::create_label(&mut self.font_system, fonts::small_size(), "Output");
        self.tooltip_label =
            Self::create_label(&mut self.font_system, fonts::small_size(), "Ctrl+Enter");

        let axis_size = fonts::small_size();
        let axis_metrics = Metrics::new(axis_size, axis_size * 1.4);
        for buf in &mut self.axis_label_buffers {
            buf.set_metrics(&mut self.font_system, axis_metrics);
            buf.set_size(&mut self.font_system, Some(2000.0), Some(axis_size * 2.0));
            buf.shape_until_scroll(&mut self.font_system, false);
        }
        let cursor_size = fonts::ui_size();
        let cursor_metrics = Metrics::new(cursor_size, cursor_size * 1.4);
        for buf in [&mut self.cursor_x_label, &mut self.cursor_y_label] {
            buf.set_metrics(&mut self.font_system, cursor_metrics);
            buf.set_size(&mut self.font_system, Some(2000.0), Some(cursor_size * 2.0));
            buf.shape_until_scroll(&mut self.font_system, false);
        }

        self.cached_tab_info.clear();
        self.cached_status_text.clear();
        self.cell_texts.clear();
        self.cell_output_texts.clear();
        self.cell_output_is_error.clear();

        self.close_dropdown();
        self.close_autocomplete();
    }

    pub(super) fn create_label(font_system: &mut FontSystem, size: f32, text: &str) -> TextBuffer {
        let family = font_family::active_family();
        Self::create_label_with_family(font_system, size, text, family)
    }

    /// Build a label using the dedicated italic chrome family. Used for
    /// untitled tab names so they read as "draft / unsaved" at a glance
    /// versus saved files which display upright in the user's code font.
    /// The italic family ships only an italic face — there is no upright
    /// fallback to accidentally select.
    pub(super) fn create_label_italic(
        font_system: &mut FontSystem,
        size: f32,
        text: &str,
    ) -> TextBuffer {
        let mut buf = TextBuffer::new(font_system, Metrics::new(size, size * 1.4));
        buf.set_size(font_system, Some(2000.0), Some(size * 2.0));
        buf.set_text(
            font_system,
            text,
            Attrs::new()
                .family(Family::Name(font_family::ITALIC_CHROME_FONT_FAMILY))
                .style(Style::Italic),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(font_system, false);
        buf
    }

    /// Build a label that bypasses the user's selected font family —
    /// needed for chrome glyphs that always want a specific face (e.g. the
    /// Greek-display "Λ" logo).
    pub(super) fn create_label_with_family(
        font_system: &mut FontSystem,
        size: f32,
        text: &str,
        family: &str,
    ) -> TextBuffer {
        let mut buf = TextBuffer::new(font_system, Metrics::new(size, size * 1.4));
        buf.set_size(font_system, Some(2000.0), Some(size * 2.0));
        buf.set_text(
            font_system,
            text,
            Attrs::new().family(Family::Name(family)),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(font_system, false);
        buf
    }

    pub(super) fn measure_label_width(buf: &TextBuffer) -> f32 {
        let mut max_x = 0.0_f32;
        for run in buf.layout_runs() {
            if let Some(last) = run.glyphs.last() {
                max_x = max_x.max(last.x + last.w);
            }
        }
        max_x
    }
}
