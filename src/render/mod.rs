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

use crate::ui::layout::{LayoutResult, Rect};
use crate::ui::theme::{colors, fonts, spacing, Rgba};
use rects::{RectInstance, RectRenderer};

/// Info about a single tab, passed from AppState to the renderer.
pub struct TabInfo {
    pub name: String,
    pub is_active: bool,
    pub is_modified: bool,
}

/// Hit-test rectangles returned by update_tab_bar for mouse handling.
#[derive(Debug, Clone, Copy)]
pub struct TabHitRect {
    pub full: Rect,
    pub close: Rect,
}

const TAB_PAD_H: f32 = 12.0;
const TAB_CLOSE_SIZE: f32 = 14.0;
const TAB_CLOSE_PAD: f32 = 6.0;
const TAB_GAP: f32 = 2.0;

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
    status_label: TextBuffer,
    graph_label: TextBuffer,

    // Dynamic tab bar
    tab_labels: Vec<TextBuffer>,
    tab_close_labels: Vec<TextBuffer>,
    tab_bg_rects: Vec<(Rect, bool)>, // (rect, is_active)
    tab_close_rects: Vec<Rect>,
    plus_label: TextBuffer,
    plus_rect: Rect,

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
        let status_label = Self::create_label(
            &mut font_system,
            fonts::status_size(),
            "Ready \u{2502} Ln 1, Col 1",
        );
        let graph_label =
            Self::create_label(&mut font_system, fonts::ui_size(), "Graph");
        let plus_label =
            Self::create_label(&mut font_system, fonts::ui_size(), "+");

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
            status_label,
            graph_label,
            tab_labels: Vec::new(),
            tab_close_labels: Vec::new(),
            tab_bg_rects: Vec::new(),
            tab_close_rects: Vec::new(),
            plus_label,
            plus_rect: Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
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

    /// Measure the pixel width of a text buffer's first layout run.
    fn measure_label_width(buf: &TextBuffer) -> f32 {
        let mut max_x = 0.0_f32;
        for run in buf.layout_runs() {
            if let Some(last) = run.glyphs.last() {
                max_x = max_x.max(last.x + last.w);
            }
        }
        max_x
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

    /// Rebuild the tab bar labels and rects from the given tab infos.
    /// Returns hit rects for each tab and the plus button rect.
    pub fn update_tab_bar(
        &mut self,
        tabs: &[TabInfo],
        tab_bar_rect: Rect,
    ) -> (Vec<TabHitRect>, Rect) {
        self.tab_labels.clear();
        self.tab_close_labels.clear();
        self.tab_bg_rects.clear();
        self.tab_close_rects.clear();

        let tab_h = tab_bar_rect.h;
        let mut x = tab_bar_rect.x + TAB_GAP;
        let y = tab_bar_rect.y;

        let mut hit_rects = Vec::with_capacity(tabs.len());

        for tab in tabs {
            // Build label text: "name" or "name \u{2022}" (bullet for modified)
            let label_text = if tab.is_modified {
                format!("{} \u{2022}", tab.name)
            } else {
                tab.name.clone()
            };

            let label = Self::create_label(&mut self.font_system, fonts::ui_size(), &label_text);
            let text_w = Self::measure_label_width(&label);

            // close button "x"
            let close_label = Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{00d7}");

            let tab_w = TAB_PAD_H + text_w + TAB_CLOSE_PAD + TAB_CLOSE_SIZE + TAB_PAD_H;

            let tab_rect = Rect { x, y, w: tab_w, h: tab_h };
            let close_rect = Rect {
                x: x + tab_w - TAB_PAD_H - TAB_CLOSE_SIZE,
                y: y + (tab_h - TAB_CLOSE_SIZE) / 2.0,
                w: TAB_CLOSE_SIZE,
                h: TAB_CLOSE_SIZE,
            };

            self.tab_bg_rects.push((tab_rect, tab.is_active));
            self.tab_close_rects.push(close_rect);
            self.tab_labels.push(label);
            self.tab_close_labels.push(close_label);

            hit_rects.push(TabHitRect {
                full: tab_rect,
                close: close_rect,
            });

            x += tab_w + TAB_GAP;
        }

        // Plus button
        let plus_w = TAB_PAD_H * 2.0 + 10.0;
        self.plus_rect = Rect { x, y, w: plus_w, h: tab_h };

        (hit_rects, self.plus_rect)
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

        // -- Collect UI background rects dynamically --
        let mut ui_rects = vec![
            // Menu bar
            rect_from(layout.menu_bar, colors::BG_SECONDARY),
            // Tab bar background
            rect_from(layout.tab_bar, colors::TAB_INACTIVE),
        ];

        // Per-tab background rects
        for (rect, is_active) in &self.tab_bg_rects {
            let color = if *is_active {
                colors::TAB_ACTIVE
            } else {
                colors::TAB_INACTIVE
            };
            ui_rects.push(rect_from(*rect, color));
        }

        // Plus button background
        ui_rects.push(rect_from(self.plus_rect, colors::TAB_INACTIVE));

        ui_rects.extend_from_slice(&[
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
        ]);

        // -- Prepare text areas dynamically --
        let pad = spacing::TEXT_PADDING;
        let lp = layout.left_pane;

        let mut text_areas: Vec<TextArea> = Vec::new();

        // Editor text in left pane
        text_areas.push(TextArea {
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
        });

        // Menu label
        text_areas.push(TextArea {
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
        });

        // Tab labels
        let tab_bar = layout.tab_bar;
        for (i, label) in self.tab_labels.iter().enumerate() {
            let (tab_rect, _) = &self.tab_bg_rects[i];
            text_areas.push(TextArea {
                buffer: label,
                left: tab_rect.x + TAB_PAD_H,
                top: tab_rect.y + spacing::SM,
                scale: 1.0,
                bounds: TextBounds {
                    left: tab_bar.x as i32,
                    top: tab_bar.y as i32,
                    right: (tab_bar.x + tab_bar.w) as i32,
                    bottom: (tab_bar.y + tab_bar.h) as i32,
                },
                default_color: colors::TEXT_PRIMARY.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Tab close labels (x buttons)
        for (i, close_label) in self.tab_close_labels.iter().enumerate() {
            let close_rect = &self.tab_close_rects[i];
            text_areas.push(TextArea {
                buffer: close_label,
                left: close_rect.x,
                top: close_rect.y,
                scale: 1.0,
                bounds: TextBounds {
                    left: tab_bar.x as i32,
                    top: tab_bar.y as i32,
                    right: (tab_bar.x + tab_bar.w) as i32,
                    bottom: (tab_bar.y + tab_bar.h) as i32,
                },
                default_color: colors::TEXT_MUTED.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Plus button label
        text_areas.push(TextArea {
            buffer: &self.plus_label,
            left: self.plus_rect.x + TAB_PAD_H,
            top: self.plus_rect.y + spacing::SM,
            scale: 1.0,
            bounds: TextBounds {
                left: tab_bar.x as i32,
                top: tab_bar.y as i32,
                right: (tab_bar.x + tab_bar.w) as i32,
                bottom: (tab_bar.y + tab_bar.h) as i32,
            },
            default_color: colors::TEXT_MUTED.to_glyphon(),
            custom_glyphs: &[],
        });

        // Status label
        text_areas.push(TextArea {
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
        });

        // Graph placeholder
        text_areas.push(TextArea {
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
        });

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

fn rect_from(r: Rect, color: Rgba) -> RectInstance {
    RectInstance {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        color: color.to_f32_array(),
    }
}
