pub mod rects;

use std::sync::Arc;

use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{
    CommandEncoderDescriptor, DeviceDescriptor, Instance, InstanceDescriptor, LoadOp,
    MultisampleState, Operations, PresentMode, RenderPassColorAttachment, RenderPassDescriptor,
    RequestAdapterOptions, StoreOp, SurfaceConfiguration, TextureUsages, TextureViewDescriptor,
};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::ui::layout::LayoutResult;
use crate::ui::theme::{colors, fonts, spacing, Rgba};
use rects::{RectInstance, RectRenderer};

/// Handles all GPU rendering: wgpu setup, text via glyphon, rects via instanced draw.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,

    // Editor text buffer (left pane)
    editor_buffer: TextBuffer,
    /// (x, y, height) in pixels, relative to surface origin.
    cursor_pos: (f32, f32, f32),

    // UI label buffers
    menu_label: TextBuffer,
    tab_label: TextBuffer,
    status_label: TextBuffer,
    graph_label: TextBuffer,

    // Batched rect renderer
    rect_renderer: RectRenderer,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let scale_factor = window.scale_factor();

        // GPU setup
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default(), None)
            .await
            .unwrap();

        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let caps = surface.get_capabilities(&adapter);
        let swapchain_format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode: PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Text rendering setup
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, swapchain_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, MultisampleState::default(), None);

        let mut editor_buffer = TextBuffer::new(
            &mut font_system,
            Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
        );

        let physical_width = (physical_size.width as f64 * scale_factor) as f32;
        let physical_height = (physical_size.height as f64 * scale_factor) as f32;
        editor_buffer.set_size(&mut font_system, Some(physical_width), Some(physical_height));
        editor_buffer.shape_until_scroll(&mut font_system, false);

        // UI label buffers
        let menu_label = Self::create_label(
            &mut font_system,
            fonts::menu_size(),
            "File  Edit  View  Help",
        );
        let tab_label =
            Self::create_label(&mut font_system, fonts::ui_size(), "Session 1");
        let status_label = Self::create_label(
            &mut font_system,
            fonts::status_size(),
            "Ready \u{2502} 7 lines \u{2502} Ln 1, Col 1",
        );
        let graph_label =
            Self::create_label(&mut font_system, fonts::ui_size(), "Graph");

        // Rect renderer
        let rect_renderer = RectRenderer::new(&device, swapchain_format);

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
            editor_buffer,
            cursor_pos: (spacing::TEXT_PADDING, spacing::TEXT_PADDING, fonts::editor_line_height()),
            menu_label,
            tab_label,
            status_label,
            graph_label,
            rect_renderer,
        }
    }

    fn create_label(font_system: &mut FontSystem, size: f32, text: &str) -> TextBuffer {
        let mut buf = TextBuffer::new(font_system, Metrics::new(size, size * 1.4));
        buf.set_size(font_system, Some(2000.0), Some(size * 2.0));
        buf.set_text(
            font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(font_system, false);
        buf
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Update editor text and recompute cursor position within the given pane rect.
    pub fn update_text(
        &mut self,
        text: &str,
        cursor_byte: usize,
        pane_x: f32,
        pane_y: f32,
        pane_w: f32,
        pane_h: f32,
    ) {
        // Size the editor buffer to the left pane
        self.editor_buffer.set_size(
            &mut self.font_system,
            Some(pane_w - spacing::TEXT_PADDING * 2.0),
            Some(pane_h - spacing::TEXT_PADDING * 2.0),
        );
        self.editor_buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        self.editor_buffer
            .shape_until_scroll(&mut self.font_system, false);

        self.cursor_pos =
            Self::compute_cursor_pos(&self.editor_buffer, text, cursor_byte, pane_x, pane_y);
    }

    fn compute_cursor_pos(
        text_buffer: &TextBuffer,
        text: &str,
        cursor_byte: usize,
        pane_x: f32,
        pane_y: f32,
    ) -> (f32, f32, f32) {
        let pad = spacing::TEXT_PADDING;
        let clamped = cursor_byte.min(text.len());
        let before = &text[..clamped];
        let line_idx = before.matches('\n').count();
        let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_byte = clamped - line_start;

        for run in text_buffer.layout_runs() {
            if run.line_i == line_idx {
                for glyph in run.glyphs.iter() {
                    if glyph.start >= col_byte {
                        return (
                            pane_x + pad + glyph.x,
                            pane_y + pad + run.line_top,
                            run.line_height,
                        );
                    }
                }
                let x = run
                    .glyphs
                    .last()
                    .map(|g| g.x + g.w)
                    .unwrap_or(0.0);
                return (pane_x + pad + x, pane_y + pad + run.line_top, run.line_height);
            }
        }

        let mut last_top = 0.0_f32;
        let mut last_height = fonts::editor_line_height();
        let mut last_line_i = 0;
        for run in text_buffer.layout_runs() {
            last_top = run.line_top;
            last_height = run.line_height;
            last_line_i = run.line_i;
        }
        let extra = (line_idx.saturating_sub(last_line_i)) as f32;
        (
            pane_x + pad,
            pane_y + pad + last_top + last_height * extra,
            last_height,
        )
    }

    /// Update the status bar label text.
    pub fn update_status(&mut self, text: &str) {
        self.status_label.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        self.status_label
            .shape_until_scroll(&mut self.font_system, false);
    }

    pub fn render(&mut self, layout: &LayoutResult) {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.surface_config.width,
                height: self.surface_config.height,
            },
        );

        let sw = self.surface_config.width as f32;
        let sh = self.surface_config.height as f32;

        // -- Collect UI background rects --
        let ui_rects = [
            // Menu bar
            rect_from(layout.menu_bar, colors::BG_SECONDARY),
            // Tab bar
            rect_from(layout.tab_bar, colors::TAB_INACTIVE),
            // Left pane (editor)
            rect_from(layout.left_pane, colors::EDITOR_BG),
            // Split handle
            rect_from(layout.split_handle, colors::SPLIT_HANDLE),
            // Right pane (graph)
            rect_from(layout.right_pane, colors::GRAPH_BG),
            // Status bar
            rect_from(layout.status_bar, colors::BG_SECONDARY),
            // Cursor
            RectInstance {
                x: self.cursor_pos.0,
                y: self.cursor_pos.1,
                w: fonts::CURSOR_WIDTH,
                h: self.cursor_pos.2,
                color: colors::CURSOR.to_f32_array(),
            },
        ];

        // -- Prepare text areas --
        let pad = spacing::TEXT_PADDING;
        let lp = layout.left_pane;
        let text_areas = [
            // Editor text in left pane
            TextArea {
                buffer: &self.editor_buffer,
                left: lp.x + pad,
                top: lp.y + pad,
                scale: 1.0,
                bounds: TextBounds {
                    left: lp.x as i32,
                    top: lp.y as i32,
                    right: (lp.x + lp.w) as i32,
                    bottom: (lp.y + lp.h) as i32,
                },
                default_color: colors::TEXT_PRIMARY.to_glyphon(),
                custom_glyphs: &[],
            },
            // Menu label
            TextArea {
                buffer: &self.menu_label,
                left: layout.menu_bar.x + spacing::SM,
                top: layout.menu_bar.y + spacing::XS,
                scale: 1.0,
                bounds: TextBounds {
                    left: layout.menu_bar.x as i32,
                    top: layout.menu_bar.y as i32,
                    right: (layout.menu_bar.x + layout.menu_bar.w) as i32,
                    bottom: (layout.menu_bar.y + layout.menu_bar.h) as i32,
                },
                default_color: colors::TEXT_PRIMARY.to_glyphon(),
                custom_glyphs: &[],
            },
            // Tab label
            TextArea {
                buffer: &self.tab_label,
                left: layout.tab_bar.x + spacing::MD,
                top: layout.tab_bar.y + spacing::SM,
                scale: 1.0,
                bounds: TextBounds {
                    left: layout.tab_bar.x as i32,
                    top: layout.tab_bar.y as i32,
                    right: (layout.tab_bar.x + layout.tab_bar.w) as i32,
                    bottom: (layout.tab_bar.y + layout.tab_bar.h) as i32,
                },
                default_color: colors::TEXT_PRIMARY.to_glyphon(),
                custom_glyphs: &[],
            },
            // Status label
            TextArea {
                buffer: &self.status_label,
                left: layout.status_bar.x + spacing::MD,
                top: layout.status_bar.y + spacing::XS,
                scale: 1.0,
                bounds: TextBounds {
                    left: layout.status_bar.x as i32,
                    top: layout.status_bar.y as i32,
                    right: (layout.status_bar.x + layout.status_bar.w) as i32,
                    bottom: (layout.status_bar.y + layout.status_bar.h) as i32,
                },
                default_color: colors::TEXT_SECONDARY.to_glyphon(),
                custom_glyphs: &[],
            },
            // Graph placeholder
            TextArea {
                buffer: &self.graph_label,
                left: layout.right_pane.x + layout.right_pane.w / 2.0 - 20.0,
                top: layout.right_pane.y + layout.right_pane.h / 2.0 - 10.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: layout.right_pane.x as i32,
                    top: layout.right_pane.y as i32,
                    right: (layout.right_pane.x + layout.right_pane.w) as i32,
                    bottom: (layout.right_pane.y + layout.right_pane.h) as i32,
                },
                default_color: colors::TEXT_MUTED.to_glyphon(),
                custom_glyphs: &[],
            },
        ];

        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .unwrap();

        let frame = self.surface.get_current_texture().unwrap();
        let view = frame
            .texture
            .create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(colors::BG_PRIMARY.to_wgpu()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw UI background rects + cursor
            self.rect_renderer
                .draw(&self.queue, &mut pass, sw, sh, &ui_rects);

            // Draw text on top
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.atlas.trim();
    }
}

fn rect_from(r: crate::ui::layout::Rect, color: Rgba) -> RectInstance {
    RectInstance {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        color: color.to_f32_array(),
    }
}
