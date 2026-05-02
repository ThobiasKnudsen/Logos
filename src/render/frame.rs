use glyphon::{
    Attrs, Buffer as TextBuffer, Color as GlyphonColor, Family, Resolution, Shaping, TextArea,
    TextBounds,
};
use wgpu::{
    CommandEncoderDescriptor, LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor,
    StoreOp, TextureViewDescriptor,
};

use crate::app::{HoverTarget, WindowControlRects};
use crate::ui::layout::{LayoutResult, Rect};
use crate::ui::theme::{self, fonts, spacing};

use super::rects::RectInstance;
use super::{
    clip_bounds_around_dropdown, clip_bounds_for_dropdown, clip_bounds_under_dropdown,
    compute_nice_step, cursor_decimals, format_tick, generate_ticks, menu_item_pad, rect_from,
    rect_rounded, tab_close_size, tab_dot_pad, tab_pad_h, RenderAreaParams, Renderer,
    MAX_AXIS_LABELS,
};

impl Renderer {
    pub fn render(
        &mut self,
        layout: &LayoutResult,
        hover: HoverTarget,
        win_controls: &WindowControlRects,
        is_dragging_split: bool,
        open_menu: Option<usize>,
        render_area: &RenderAreaParams,
    ) {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.surface_config.width,
                height: self.surface_config.height,
            },
        );

        let sw = self.surface_config.width as f32;
        let sh = self.surface_config.height as f32;
        let lp = layout.left_pane;
        let text_pad = spacing::sm();
        let t = theme::theme();

        let pane_left = lp.x as i32;
        let pane_top = lp.y as i32;
        let pane_right = if self.v_track_rect.is_some() {
            (lp.x + lp.w - spacing::scrollbar_width()) as i32
        } else {
            (lp.x + lp.w) as i32
        };
        let pane_bottom = (lp.y + lp.h) as i32;

        let mut ui_rects = vec![
            rect_from(layout.left_pane, t.editor_bg),
            rect_from(layout.right_pane, t.graph_bg),
        ];

        let split_color = if is_dragging_split || hover == HoverTarget::SplitHandle {
            t.split_handle_hover
        } else {
            t.split_handle
        };
        ui_rects.push(rect_from(layout.split_handle, split_color));

        for (i, cl) in self.cell_layouts.iter().enumerate() {
            if cl.container.y + cl.container.h < lp.y || cl.container.y > lp.y + lp.h {
                continue;
            }

            let border_color = if i == self.active_cell_index {
                t.border_focus
            } else {
                t.border
            };
            let cell_radius = 12.0 * fonts::scale();
            ui_rects.push(rect_rounded(
                Rect {
                    x: cl.container.x - 1.0,
                    y: cl.container.y - 1.0,
                    w: cl.container.w + 2.0,
                    h: cl.container.h + 2.0,
                },
                border_color,
                cell_radius + 1.0,
            ));

            ui_rects.push(rect_rounded(cl.container, t.bg_elevated, cell_radius));

            {
                let is_playing = i < self.cell_playing.len() && self.cell_playing[i];
                let is_hovered = hover == HoverTarget::CellPlayButton(i);
                let btn_color = if is_playing {
                    if is_hovered {
                        t.stop_button_hover
                    } else {
                        t.stop_button
                    }
                } else if is_hovered {
                    t.play_button_hover
                } else {
                    t.play_button
                };
                ui_rects.push(rect_rounded(
                    cl.play_button,
                    btn_color,
                    4.0 * fonts::scale(),
                ));
            }

            if hover == HoverTarget::CellCopyButton(i) {
                ui_rects.push(rect_rounded(
                    cl.copy_button,
                    t.bg_hover,
                    cl.copy_button.w / 2.0,
                ));
            }

            if hover == HoverTarget::CellDeleteButton(i) {
                ui_rects.push(rect_rounded(
                    cl.delete_button,
                    t.bg_hover,
                    cl.delete_button.w / 2.0,
                ));
            }

            ui_rects.push(rect_from(cl.separator, t.border));

            if cl.output_toolbar.h > 0.0 {
                ui_rects.push(rect_from(cl.output_separator, t.border));

                if hover == HoverTarget::CellOutputToggle(i) {
                    ui_rects.push(rect_rounded(
                        cl.output_toggle,
                        t.bg_hover,
                        cl.output_toggle.w / 2.0,
                    ));
                }

                if hover == HoverTarget::CellOutputCopyButton(i) {
                    ui_rects.push(rect_rounded(
                        cl.output_copy_button,
                        t.bg_hover,
                        cl.output_copy_button.w / 2.0,
                    ));
                }
            }

            if cl.editor_h_scrollbar_track.h > 0.0 {
                ui_rects.push(rect_from(cl.editor_h_scrollbar_track, t.scrollbar_track));
                let thumb_color = if hover == HoverTarget::CellEditorHScrollThumb(i) {
                    t.scrollbar_thumb_hover
                } else {
                    t.scrollbar_thumb
                };
                ui_rects.push(rect_from(cl.editor_h_scrollbar_thumb, thumb_color));
            }

            if cl.editor_v_scrollbar_track.h > 0.0 {
                ui_rects.push(rect_from(cl.editor_v_scrollbar_track, t.scrollbar_track));
                let thumb_color = if hover == HoverTarget::CellEditorVScrollThumb(i) {
                    t.scrollbar_thumb_hover
                } else {
                    t.scrollbar_thumb
                };
                ui_rects.push(rect_from(cl.editor_v_scrollbar_thumb, thumb_color));
            }
        }

        if let Some(cl) = self.cell_layouts.get(self.active_cell_index) {
            let sel_scroll_x = self
                .cell_editor_scroll_x
                .get(self.active_cell_index)
                .copied()
                .unwrap_or(0.0);
            let sel_scroll_y = self
                .cell_editor_scroll_y
                .get(self.active_cell_index)
                .copied()
                .unwrap_or(0.0);
            let editor_right = if cl.editor_v_scrollbar_track.h > 0.0 {
                cl.editor_v_scrollbar_track.x
            } else {
                cl.editor.x + cl.editor.w
            };
            let editor_bottom = if cl.editor_h_scrollbar_track.h > 0.0 {
                cl.editor_h_scrollbar_track.y
            } else {
                cl.editor.y + cl.editor.h
            };
            for &(sx, sy, sw_, sh_) in &self.selection_content_rects {
                let screen_x = cl.editor.x + text_pad + sx - sel_scroll_x;
                let screen_y = cl.editor.y + text_pad + sy - sel_scroll_y;
                let clipped_x = screen_x.max(cl.editor.x);
                let clipped_y = screen_y.max(cl.editor.y);
                let clipped_w = (screen_x + sw_).min(editor_right) - clipped_x;
                let clipped_h = (screen_y + sh_).min(editor_bottom) - clipped_y;
                if clipped_w > 0.0
                    && clipped_h > 0.0
                    && clipped_x + clipped_w >= lp.x
                    && clipped_x < pane_right as f32
                    && clipped_y + clipped_h > lp.y
                    && clipped_y < pane_bottom as f32
                {
                    ui_rects.push(RectInstance {
                        x: clipped_x,
                        y: clipped_y,
                        w: clipped_w,
                        h: clipped_h,
                        color: t.editor_selection.to_f32_array(),
                        corner_radius: 2.0,
                        _padding: [0.0; 3],
                    });
                }
            }
        }

        if let Some(cl) = self.cell_layouts.get(self.active_cell_index) {
            let cur_scroll_x = self
                .cell_editor_scroll_x
                .get(self.active_cell_index)
                .copied()
                .unwrap_or(0.0);
            let cur_scroll_y = self
                .cell_editor_scroll_y
                .get(self.active_cell_index)
                .copied()
                .unwrap_or(0.0);
            let cursor_screen_x = cl.editor.x + text_pad + self.cursor_content_pos.0 - cur_scroll_x;
            let cursor_screen_y = cl.editor.y + text_pad + self.cursor_content_pos.1 - cur_scroll_y;
            let ch = self.cursor_content_pos.2;

            let cur_editor_right = if cl.editor_v_scrollbar_track.h > 0.0 {
                cl.editor_v_scrollbar_track.x
            } else {
                cl.editor.x + cl.editor.w
            };
            let cur_editor_bottom = if cl.editor_h_scrollbar_track.h > 0.0 {
                cl.editor_h_scrollbar_track.y
            } else {
                cl.editor.y + cl.editor.h
            };
            if cursor_screen_x >= cl.editor.x
                && cursor_screen_x < cur_editor_right
                && cursor_screen_y + ch > cl.editor.y
                && cursor_screen_y < cur_editor_bottom
                && cursor_screen_x >= lp.x
                && cursor_screen_x < pane_right as f32
                && cursor_screen_y + ch > lp.y
                && cursor_screen_y < pane_bottom as f32
            {
                ui_rects.push(RectInstance {
                    x: cursor_screen_x,
                    y: cursor_screen_y,
                    w: fonts::CURSOR_WIDTH,
                    h: ch,
                    color: t.cursor.to_f32_array(),
                    corner_radius: 0.0,
                    _padding: [0.0; 3],
                });
            }
        }

        if self.add_cell_rect.y + self.add_cell_rect.h > lp.y && self.add_cell_rect.y < lp.y + lp.h
        {
            let add_color = if hover == HoverTarget::AddCellButton {
                t.bg_hover
            } else {
                t.bg_elevated
            };
            ui_rects.push(rect_rounded(
                self.add_cell_rect,
                add_color,
                self.add_cell_rect.w / 2.0,
            ));
        }

        if let Some(track) = self.v_track_rect {
            ui_rects.push(rect_from(track, t.scrollbar_track));
        }
        if let Some(thumb) = self.v_thumb_rect {
            let color = if matches!(hover, HoverTarget::VScrollThumb) {
                t.scrollbar_thumb_hover
            } else {
                t.scrollbar_thumb
            };
            ui_rects.push(rect_from(thumb, color));
        }

        ui_rects.push(rect_from(layout.title_bar, t.bg_secondary));
        ui_rects.push(rect_from(layout.tab_bar, t.tab_inactive));
        ui_rects.push(rect_from(layout.status_bar, t.bg_secondary));

        for (idx, rect) in self.menu_item_rects.iter().enumerate() {
            let is_open = open_menu == Some(idx);
            let is_hovered = hover == HoverTarget::MenuItem(idx);
            if is_open || is_hovered {
                ui_rects.push(rect_from(*rect, t.menu_item_hover));
            }
        }

        for (idx, (rect, is_active)) in self.tab_bg_rects.iter().enumerate() {
            let color = if *is_active {
                t.tab_active
            } else if matches!(hover, HoverTarget::Tab(i) | HoverTarget::TabClose(i) if i == idx) {
                t.tab_hover
            } else {
                t.tab_inactive
            };
            ui_rects.push(rect_from(*rect, color));
        }

        for (idx, close_rect) in self.tab_close_rects.iter().enumerate() {
            if hover == HoverTarget::TabClose(idx) {
                ui_rects.push(rect_rounded(*close_rect, t.bg_hover, close_rect.w / 2.0));
            }
        }

        if hover == HoverTarget::PlusButton {
            let size = tab_close_size();
            let cx = self.plus_rect.x + self.plus_rect.w / 2.0;
            let cy = self.plus_rect.y + self.plus_rect.h / 2.0;
            let r = Rect {
                x: cx - size / 2.0,
                y: cy - size / 2.0,
                w: size,
                h: size,
            };
            ui_rects.push(rect_rounded(r, t.bg_hover, size / 2.0));
        }

        ui_rects.push(RectInstance {
            x: 0.0,
            y: layout.tab_bar.y + layout.tab_bar.h - 1.0,
            w: sw,
            h: 1.0,
            color: t.border.to_f32_array(),
            corner_radius: 0.0,
            _padding: [0.0; 3],
        });

        ui_rects.push(RectInstance {
            x: 0.0,
            y: layout.status_bar.y,
            w: sw,
            h: 1.0,
            color: t.border.to_f32_array(),
            corner_radius: 0.0,
            _padding: [0.0; 3],
        });

        ui_rects.push(RectInstance {
            x: layout.left_pane.x,
            y: layout.left_pane.y,
            w: 1.0,
            h: layout.left_pane.h,
            color: t.border.to_f32_array(),
            corner_radius: 0.0,
            _padding: [0.0; 3],
        });

        ui_rects.push(RectInstance {
            x: layout.right_pane.x + layout.right_pane.w - 1.0,
            y: layout.right_pane.y,
            w: 1.0,
            h: layout.right_pane.h,
            color: t.border.to_f32_array(),
            corner_radius: 0.0,
            _padding: [0.0; 3],
        });

        if hover == HoverTarget::WinBtnMinimize {
            ui_rects.push(rect_from(win_controls.minimize, t.bg_hover));
        }
        if hover == HoverTarget::WinBtnMaximize {
            ui_rects.push(rect_from(win_controls.maximize, t.bg_hover));
        }
        if hover == HoverTarget::WinBtnClose {
            ui_rects.push(rect_from(win_controls.close, t.close_button_hover));
        }

        if self.dropdown_active {
            ui_rects.push(rect_from(self.dropdown_bg, t.dropdown_bg));
            let db = self.dropdown_bg;
            ui_rects.push(RectInstance {
                x: db.x,
                y: db.y,
                w: db.w,
                h: 1.0,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0,
                _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x,
                y: db.y + db.h - 1.0,
                w: db.w,
                h: 1.0,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0,
                _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x,
                y: db.y,
                w: 1.0,
                h: db.h,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0,
                _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x + db.w - 1.0,
                y: db.y,
                w: 1.0,
                h: db.h,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0,
                _padding: [0.0; 3],
            });

            for (idx, rect) in self.dropdown_item_rects.iter().enumerate() {
                if hover == HoverTarget::DropdownItem(idx) {
                    ui_rects.push(rect_from(*rect, t.dropdown_hover));
                } else if self.dropdown_active_item == Some(idx) {
                    ui_rects.push(rect_from(*rect, t.bg_elevated));
                }
            }
        }

        if self.ac_active {
            let ac_radius = 6.0 * fonts::scale();
            ui_rects.push(rect_rounded(
                Rect {
                    x: self.ac_bg.x - 1.0,
                    y: self.ac_bg.y - 1.0,
                    w: self.ac_bg.w + 2.0,
                    h: self.ac_bg.h + 2.0,
                },
                t.dropdown_separator,
                ac_radius + 1.0,
            ));
            ui_rects.push(rect_rounded(self.ac_bg, t.dropdown_bg, ac_radius));

            for (idx, rect) in self.ac_item_rects.iter().enumerate() {
                if idx == self.ac_selected_index {
                    ui_rects.push(rect_from(*rect, t.dropdown_hover));
                } else if hover == HoverTarget::AutocompleteItem(idx) {
                    ui_rects.push(rect_from(*rect, t.bg_elevated));
                }
            }
        }

        let mut text_areas: Vec<TextArea> = Vec::new();
        let editor_color = t.text_primary.to_glyphon();

        let dd_clip = if self.dropdown_active {
            Some(self.dropdown_bg)
        } else {
            None
        };

        for (i, cl) in self.cell_layouts.iter().enumerate() {
            if cl.container.y + cl.container.h < lp.y || cl.container.y > lp.y + lp.h {
                continue;
            }

            if i < self.cell_buffers.len() {
                let ed_scroll_x = self.cell_editor_scroll_x.get(i).copied().unwrap_or(0.0);
                let ed_scroll_y = self.cell_editor_scroll_y.get(i).copied().unwrap_or(0.0);

                let clip_left = (cl.editor.x as i32).max(pane_left);
                let clip_top = (cl.editor.y as i32).max(pane_top);
                let v_sb_adjust = if cl.editor_v_scrollbar_track.h > 0.0 {
                    (cl.editor.x + cl.editor.w - cl.editor_v_scrollbar_track.x) as i32
                } else {
                    0
                };
                let clip_right = ((cl.editor.x + cl.editor.w) as i32 - v_sb_adjust).min(pane_right);
                let h_sb_adjust = if cl.editor_h_scrollbar_track.h > 0.0 {
                    (cl.editor.y + cl.editor.h - cl.editor_h_scrollbar_track.y) as i32
                } else {
                    0
                };
                let clip_bottom =
                    ((cl.editor.y + cl.editor.h) as i32 - h_sb_adjust).min(pane_bottom);

                let bounds = TextBounds {
                    left: clip_left,
                    top: clip_top,
                    right: clip_right,
                    bottom: clip_bottom,
                };

                let regions = if let Some(dd) = &dd_clip {
                    clip_bounds_around_dropdown(&bounds, dd)
                } else {
                    vec![bounds]
                };

                for region in regions {
                    if region.left < region.right && region.top < region.bottom {
                        text_areas.push(TextArea {
                            buffer: &self.cell_buffers[i],
                            left: cl.editor.x + text_pad - ed_scroll_x,
                            top: cl.editor.y + text_pad - ed_scroll_y,
                            scale: 1.0,
                            bounds: region,
                            default_color: editor_color,
                            custom_glyphs: &[],
                        });
                    }
                }
            }

            if cl.output_toolbar.h > 0.0 {
                let tb_clip_top = (cl.output_toolbar.y as i32).max(pane_top);
                let tb_clip_bottom =
                    ((cl.output_toolbar.y + cl.output_toolbar.h) as i32).min(pane_bottom);
                if tb_clip_top < tb_clip_bottom {
                    let is_collapsed = cl.output.h <= 0.0;
                    let line_h = fonts::small_size() * 1.4;

                    let chevron_buf = if is_collapsed {
                        &self.cell_chevron_right
                    } else {
                        &self.cell_chevron_down
                    };
                    let chev_w = Self::measure_label_width(chevron_buf);
                    let chev_x = cl.output_toggle.x + (cl.output_toggle.w - chev_w) / 2.0;
                    let chev_y = cl.output_toggle.y + (cl.output_toggle.h - line_h) / 2.0;
                    let mut chev_bounds = TextBounds {
                        left: (cl.output_toggle.x as i32).max(pane_left),
                        top: tb_clip_top,
                        right: ((cl.output_toggle.x + cl.output_toggle.w) as i32).min(pane_right),
                        bottom: tb_clip_bottom,
                    };
                    let chev_visible = dd_clip
                        .as_ref()
                        .is_none_or(|dd| clip_bounds_under_dropdown(&mut chev_bounds, dd));
                    if chev_visible {
                        text_areas.push(TextArea {
                            buffer: chevron_buf,
                            left: chev_x,
                            top: chev_y,
                            scale: 1.0,
                            bounds: chev_bounds,
                            default_color: t.text_muted.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }

                    let label_x = cl.output_toggle.x + cl.output_toggle.w + spacing::xs();
                    let label_y = cl.output_toolbar.y + (cl.output_toolbar.h - line_h) / 2.0;
                    let mut label_bounds = TextBounds {
                        left: (label_x as i32).max(pane_left),
                        top: tb_clip_top,
                        right: ((cl.output_toolbar.x + cl.output_toolbar.w) as i32).min(pane_right),
                        bottom: tb_clip_bottom,
                    };
                    let label_visible = dd_clip
                        .as_ref()
                        .is_none_or(|dd| clip_bounds_under_dropdown(&mut label_bounds, dd));
                    if label_visible {
                        text_areas.push(TextArea {
                            buffer: &self.output_label,
                            left: label_x,
                            top: label_y,
                            scale: 1.0,
                            bounds: label_bounds,
                            default_color: t.text_muted.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }

                    let ocb = &cl.output_copy_button;
                    let mut ocb_bounds = TextBounds {
                        left: (ocb.x as i32).max(pane_left),
                        top: tb_clip_top,
                        right: ((ocb.x + ocb.w) as i32).min(pane_right),
                        bottom: tb_clip_bottom,
                    };
                    let ocb_visible = dd_clip
                        .as_ref()
                        .is_none_or(|dd| clip_bounds_under_dropdown(&mut ocb_bounds, dd));
                    if ocb_visible {
                        let copy_w = Self::measure_label_width(&self.cell_copy_label);
                        let copy_line_h = fonts::ui_size() * 1.4;
                        let cx = ocb.x + (ocb.w - copy_w) / 2.0;
                        let cy = ocb.y + (ocb.h - copy_line_h) / 2.0;
                        text_areas.push(TextArea {
                            buffer: &self.cell_copy_label,
                            left: cx,
                            top: cy,
                            scale: 1.0,
                            bounds: ocb_bounds,
                            default_color: t.text_muted.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }
                }
            }

            if cl.output.h > 0.0 && i < self.cell_output_buffers.len() {
                let scroll_x = self.cell_output_scroll_x.get(i).copied().unwrap_or(0.0);
                let clip_left = (cl.output.x as i32).max(pane_left);
                let clip_top = (cl.output.y as i32).max(pane_top);
                let clip_right = ((cl.output.x + cl.output.w) as i32).min(pane_right);
                let clip_bottom = ((cl.output.y + cl.output.h) as i32).min(pane_bottom);

                let is_err = self.cell_output_is_error.get(i).copied().unwrap_or(false);
                let output_color = if is_err { t.stop_button } else { t.text_muted };

                if clip_left < clip_right && clip_top < clip_bottom {
                    text_areas.push(TextArea {
                        buffer: &self.cell_output_buffers[i],
                        left: cl.output.x + text_pad - scroll_x,
                        top: cl.output.y + text_pad,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: clip_left,
                            top: clip_top,
                            right: clip_right,
                            bottom: clip_bottom,
                        },
                        default_color: output_color.to_glyphon(),
                        custom_glyphs: &[],
                    });
                }
            }

            let play = &cl.play_button;
            let play_clip_top = (play.y as i32).max(pane_top);
            let play_clip_bottom = ((play.y + play.h) as i32).min(pane_bottom);
            if play_clip_top < play_clip_bottom {
                let mut bounds = TextBounds {
                    left: play.x as i32,
                    top: play_clip_top,
                    right: (play.x + play.w) as i32,
                    bottom: play_clip_bottom,
                };
                let visible = dd_clip
                    .as_ref()
                    .is_none_or(|dd| clip_bounds_under_dropdown(&mut bounds, dd));

                if visible {
                    let is_playing = i < self.cell_playing.len() && self.cell_playing[i];
                    let play_buf = if is_playing {
                        &self.cell_stop_label
                    } else {
                        &self.cell_play_label
                    };
                    let label_w = Self::measure_label_width(play_buf);
                    let line_h = fonts::ui_size() * 1.4;
                    let cx = play.x + (play.w - label_w) / 2.0;
                    let cy = play.y + (play.h - line_h) / 2.0;
                    text_areas.push(TextArea {
                        buffer: play_buf,
                        left: cx,
                        top: cy,
                        scale: 1.0,
                        bounds,
                        default_color: GlyphonColor::rgb(255, 255, 255),
                        custom_glyphs: &[],
                    });
                }
            }

            {
                let cb = &cl.copy_button;
                let cb_clip_top = (cb.y as i32).max(pane_top);
                let cb_clip_bottom = ((cb.y + cb.h) as i32).min(pane_bottom);
                if cb_clip_top < cb_clip_bottom {
                    let mut bounds = TextBounds {
                        left: cb.x as i32,
                        top: cb_clip_top,
                        right: (cb.x + cb.w) as i32,
                        bottom: cb_clip_bottom,
                    };
                    let visible = dd_clip
                        .as_ref()
                        .is_none_or(|dd| clip_bounds_under_dropdown(&mut bounds, dd));

                    if visible {
                        let label_w = Self::measure_label_width(&self.cell_copy_label);
                        let line_h = fonts::ui_size() * 1.4;
                        let cx = cb.x + (cb.w - label_w) / 2.0;
                        let cy = cb.y + (cb.h - line_h) / 2.0;
                        text_areas.push(TextArea {
                            buffer: &self.cell_copy_label,
                            left: cx,
                            top: cy,
                            scale: 1.0,
                            bounds,
                            default_color: t.text_secondary.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }
                }
            }

            let del = &cl.delete_button;
            let del_clip_top = (del.y as i32).max(pane_top);
            let del_clip_bottom = ((del.y + del.h) as i32).min(pane_bottom);
            if del_clip_top < del_clip_bottom {
                let mut bounds = TextBounds {
                    left: del.x as i32,
                    top: del_clip_top,
                    right: (del.x + del.w) as i32,
                    bottom: del_clip_bottom,
                };
                let visible = dd_clip
                    .as_ref()
                    .is_none_or(|dd| clip_bounds_under_dropdown(&mut bounds, dd));

                if visible {
                    let label_w = Self::measure_label_width(&self.cell_delete_label);
                    let line_h = fonts::ui_size() * 1.4;
                    let cx = del.x + (del.w - label_w) / 2.0;
                    let cy = del.y + (del.h - line_h) / 2.0;
                    text_areas.push(TextArea {
                        buffer: &self.cell_delete_label,
                        left: cx,
                        top: cy,
                        scale: 1.0,
                        bounds,
                        default_color: t.text_secondary.to_glyphon(),
                        custom_glyphs: &[],
                    });
                }
            }
        }

        for (i, cl) in self.cell_layouts.iter().enumerate() {
            if !matches!(hover, HoverTarget::CellPlayButton(idx) if idx == i) {
                continue;
            }
            let tip_w = Self::measure_label_width(&self.tooltip_label) + spacing::sm() * 2.0;
            let tip_h = fonts::small_size() * 1.4 + spacing::xs() * 2.0;
            let tip_x = cl.play_button.x + cl.play_button.w / 2.0 - tip_w / 2.0;
            let tip_y = cl.play_button.y + cl.play_button.h + spacing::xs();
            let tip_rect = Rect {
                x: tip_x,
                y: tip_y,
                w: tip_w,
                h: tip_h,
            };

            ui_rects.push(rect_rounded(
                Rect {
                    x: tip_rect.x - 1.0,
                    y: tip_rect.y - 1.0,
                    w: tip_rect.w + 2.0,
                    h: tip_rect.h + 2.0,
                },
                t.tooltip_border,
                4.0 * fonts::scale(),
            ));
            ui_rects.push(rect_rounded(tip_rect, t.tooltip_bg, 4.0 * fonts::scale()));

            text_areas.push(TextArea {
                buffer: &self.tooltip_label,
                left: tip_rect.x + spacing::sm(),
                top: tip_rect.y + spacing::xs(),
                scale: 1.0,
                bounds: TextBounds {
                    left: tip_rect.x as i32,
                    top: tip_rect.y as i32,
                    right: (tip_rect.x + tip_rect.w) as i32,
                    bottom: (tip_rect.y + tip_rect.h) as i32,
                },
                default_color: t.text_primary.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        if self.add_cell_rect.y + self.add_cell_rect.h > lp.y && self.add_cell_rect.y < lp.y + lp.h
        {
            let clip_top = (self.add_cell_rect.y as i32).max(pane_top);
            let clip_bottom =
                ((self.add_cell_rect.y + self.add_cell_rect.h) as i32).min(pane_bottom);
            let mut bounds = TextBounds {
                left: self.add_cell_rect.x as i32,
                top: clip_top,
                right: (self.add_cell_rect.x + self.add_cell_rect.w) as i32,
                bottom: clip_bottom,
            };
            let visible = dd_clip
                .as_ref()
                .is_none_or(|dd| clip_bounds_under_dropdown(&mut bounds, dd));

            if visible && bounds.top < bounds.bottom {
                let label_w = Self::measure_label_width(&self.add_cell_label);
                let line_h = fonts::ui_size() * 1.4;
                let cx = self.add_cell_rect.x + (self.add_cell_rect.w - label_w) / 2.0;
                let cy = self.add_cell_rect.y + (self.add_cell_rect.h - line_h) / 2.0;
                text_areas.push(TextArea {
                    buffer: &self.add_cell_label,
                    left: cx,
                    top: cy,
                    scale: 1.0,
                    bounds,
                    default_color: t.text_secondary.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        let menu_right = win_controls.minimize.x;
        for (i, label) in self.menu_item_labels.iter().enumerate() {
            if i >= self.menu_item_rects.len() {
                break;
            }
            let rect = &self.menu_item_rects[i];
            text_areas.push(TextArea {
                buffer: label,
                left: rect.x + menu_item_pad(),
                top: rect.y + spacing::xs(),
                scale: 1.0,
                bounds: TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: menu_right as i32,
                    bottom: (rect.y + rect.h) as i32,
                },
                default_color: t.text_primary.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        let win_ctrl_pairs: [(&TextBuffer, &Rect); 3] = [
            (&self.win_min_label, &win_controls.minimize),
            (&self.win_max_label, &win_controls.maximize),
            (&self.win_close_label, &win_controls.close),
        ];
        for (label, rect) in &win_ctrl_pairs {
            let label_w = Self::measure_label_width(label);
            let cx = rect.x + (rect.w - label_w) / 2.0;
            let cy = rect.y + spacing::xs();
            text_areas.push(TextArea {
                buffer: label,
                left: cx,
                top: cy,
                scale: 1.0,
                bounds: TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: (rect.x + rect.w) as i32,
                    bottom: (rect.y + rect.h) as i32,
                },
                default_color: t.text_primary.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        let tab_bar = layout.tab_bar;
        let dropdown_clip = if self.dropdown_active {
            Some(self.dropdown_bg)
        } else {
            None
        };

        for (i, label) in self.tab_labels.iter().enumerate() {
            let (tab_rect, _) = &self.tab_bg_rects[i];
            let Some(bounds) = clip_bounds_for_dropdown(tab_rect, &tab_bar, dropdown_clip.as_ref())
            else {
                continue;
            };

            let is_modified = i < self.tab_modified.len() && self.tab_modified[i];
            let text_left = if is_modified {
                tab_rect.x
                    + tab_dot_pad()
                    + Self::measure_label_width(&self.dot_label)
                    + tab_dot_pad()
            } else {
                tab_rect.x + tab_pad_h()
            };
            text_areas.push(TextArea {
                buffer: label,
                left: text_left,
                top: tab_rect.y + spacing::sm(),
                scale: 1.0,
                bounds,
                default_color: t.text_primary.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        for (i, (tab_rect, _)) in self.tab_bg_rects.iter().enumerate() {
            if i < self.tab_modified.len() && self.tab_modified[i] {
                let Some(bounds) =
                    clip_bounds_for_dropdown(tab_rect, &tab_bar, dropdown_clip.as_ref())
                else {
                    continue;
                };

                let dot_x = tab_rect.x + tab_dot_pad();
                let dot_y = tab_rect.y + spacing::sm() - 1.0;
                text_areas.push(TextArea {
                    buffer: &self.dot_label,
                    left: dot_x,
                    top: dot_y,
                    scale: 1.0,
                    bounds,
                    default_color: t.text_primary.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        for (i, close_label) in self.tab_close_labels.iter().enumerate() {
            let close_rect = &self.tab_close_rects[i];
            let Some(mut bounds) =
                clip_bounds_for_dropdown(close_rect, &tab_bar, dropdown_clip.as_ref())
            else {
                continue;
            };
            bounds.left = bounds.left.max(close_rect.x as i32);
            bounds.right = bounds.right.min((close_rect.x + close_rect.w) as i32);
            bounds.top = close_rect.y as i32;
            bounds.bottom = (close_rect.y + close_rect.h) as i32;

            let label_w = Self::measure_label_width(close_label);
            let line_h = fonts::ui_size() * 1.4;
            let cx = close_rect.x + (close_rect.w - label_w) / 2.0;
            let cy = close_rect.y + (close_rect.h - line_h) / 2.0;

            text_areas.push(TextArea {
                buffer: close_label,
                left: cx,
                top: cy,
                scale: 1.0,
                bounds,
                default_color: t.text_muted.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        if let Some(bounds) =
            clip_bounds_for_dropdown(&self.plus_rect, &tab_bar, dropdown_clip.as_ref())
        {
            text_areas.push(TextArea {
                buffer: &self.plus_label,
                left: self.plus_rect.x + tab_pad_h(),
                top: self.plus_rect.y + spacing::sm(),
                scale: 1.0,
                bounds,
                default_color: t.text_muted.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        if self.dropdown_active {
            for (i, (label, shortcut)) in self
                .dropdown_item_labels
                .iter()
                .zip(self.dropdown_shortcut_labels.iter())
                .enumerate()
            {
                if i >= self.dropdown_item_rects.len() {
                    break;
                }
                let rect = &self.dropdown_item_rects[i];
                text_areas.push(TextArea {
                    buffer: label,
                    left: rect.x + menu_item_pad(),
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds: TextBounds {
                        left: rect.x as i32,
                        top: rect.y as i32,
                        right: (rect.x + rect.w) as i32,
                        bottom: (rect.y + rect.h) as i32,
                    },
                    default_color: t.text_primary.to_glyphon(),
                    custom_glyphs: &[],
                });
                let shortcut_w = Self::measure_label_width(shortcut);
                text_areas.push(TextArea {
                    buffer: shortcut,
                    left: rect.x + rect.w - menu_item_pad() - shortcut_w,
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds: TextBounds {
                        left: rect.x as i32,
                        top: rect.y as i32,
                        right: (rect.x + rect.w) as i32,
                        bottom: (rect.y + rect.h) as i32,
                    },
                    default_color: t.text_muted.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        if self.ac_active {
            for (i, (label, kind_label)) in self
                .ac_item_labels
                .iter()
                .zip(self.ac_kind_labels.iter())
                .enumerate()
            {
                if i >= self.ac_item_rects.len() {
                    break;
                }
                let rect = &self.ac_item_rects[i];
                let bounds = TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: (rect.x + rect.w) as i32,
                    bottom: (rect.y + rect.h) as i32,
                };
                text_areas.push(TextArea {
                    buffer: kind_label,
                    left: rect.x + spacing::sm(),
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds,
                    default_color: t.text_muted.to_glyphon(),
                    custom_glyphs: &[],
                });
                let badge_w = Self::measure_label_width(kind_label);
                text_areas.push(TextArea {
                    buffer: label,
                    left: rect.x + spacing::sm() + badge_w + spacing::sm(),
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds,
                    default_color: t.text_primary.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        text_areas.push(TextArea {
            buffer: &self.status_label,
            left: layout.status_bar.x + spacing::md(),
            top: layout.status_bar.y + spacing::xs(),
            scale: 1.0,
            bounds: TextBounds {
                left: layout.status_bar.x as i32,
                top: layout.status_bar.y as i32,
                right: (layout.status_bar.x + layout.status_bar.w) as i32,
                bottom: (layout.status_bar.y + layout.status_bar.h) as i32,
            },
            default_color: t.text_secondary.to_glyphon(),
            custom_glyphs: &[],
        });

        let rp = layout.right_pane;
        let scale = fonts::scale();
        let label_pad = 4.0_f32 * scale;

        let x_range = render_area.axis_x_max - render_area.axis_x_min;
        let y_range = render_area.axis_y_max - render_area.axis_y_min;

        let min_px_spacing = 80.0_f32 * scale;
        let max_ticks_x = (rp.w / min_px_spacing).max(2.0) as usize;
        let max_ticks_y = (rp.h / min_px_spacing).max(2.0) as usize;
        let max_ticks_x = max_ticks_x.min(MAX_AXIS_LABELS);
        let max_ticks_y = max_ticks_y.min(MAX_AXIS_LABELS);

        let mut x_step = compute_nice_step(x_range, max_ticks_x);
        let mut y_step = compute_nice_step(y_range, max_ticks_y);

        let x_ticks = loop {
            let xt = generate_ticks(render_area.axis_x_min, render_area.axis_x_max, x_step);
            if xt.len() <= MAX_AXIS_LABELS {
                break xt;
            }
            x_step *= 2.0;
        };
        let y_ticks = loop {
            let yt = generate_ticks(render_area.axis_y_min, render_area.axis_y_max, y_step);
            if yt.len() <= MAX_AXIS_LABELS {
                break yt;
            }
            y_step *= 2.0;
        };

        for (i, tick) in x_ticks.iter().enumerate() {
            let text = format_tick(*tick, x_step);
            self.axis_label_buffers[i].set_text(
                &mut self.font_system,
                &text,
                Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );
            self.axis_label_buffers[i].shape_until_scroll(&mut self.font_system, false);
        }
        for (i, tick) in y_ticks.iter().enumerate() {
            let text = format_tick(*tick, y_step);
            self.axis_label_buffers[MAX_AXIS_LABELS + i].set_text(
                &mut self.font_system,
                &text,
                Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );
            self.axis_label_buffers[MAX_AXIS_LABELS + i]
                .shape_until_scroll(&mut self.font_system, false);
        }

        let grid_color = [1.0_f32, 1.0, 1.0, 0.5];
        let zero_color = [1.0_f32, 1.0, 1.0, 0.5];
        let mut axis_rects: Vec<RectInstance> = Vec::new();
        if x_range > f32::EPSILON {
            for tick in &x_ticks {
                let t_ = (tick - render_area.axis_x_min) / x_range;
                let sx = rp.x + t_ * rp.w;
                if sx >= rp.x && sx <= rp.x + rp.w {
                    let is_zero = tick.abs() < f32::EPSILON;
                    axis_rects.push(RectInstance {
                        x: if is_zero { sx - 0.5 } else { sx },
                        y: rp.y,
                        w: if is_zero { 2.0 } else { 1.0 },
                        h: rp.h,
                        color: if is_zero { zero_color } else { grid_color },
                        corner_radius: 0.0,
                        _padding: [0.0; 3],
                    });
                }
            }
        }
        if y_range > f32::EPSILON {
            for tick in &y_ticks {
                let t_ = (tick - render_area.axis_y_min) / y_range;
                let sy = rp.y + rp.h - t_ * rp.h;
                if sy >= rp.y && sy <= rp.y + rp.h {
                    let is_zero = tick.abs() < f32::EPSILON;
                    axis_rects.push(RectInstance {
                        x: rp.x,
                        y: if is_zero { sy - 0.5 } else { sy },
                        w: rp.w,
                        h: if is_zero { 2.0 } else { 1.0 },
                        color: if is_zero { zero_color } else { grid_color },
                        corner_radius: 0.0,
                        _padding: [0.0; 3],
                    });
                }
            }
        }
        let label_h = fonts::small_size() * 1.4;

        let mut axis_text_areas: Vec<TextArea> = Vec::new();
        let label_color = GlyphonColor::rgba(255, 255, 255, 255);
        let rp_bounds = TextBounds {
            left: rp.x as i32,
            top: rp.y as i32,
            right: (rp.x + rp.w) as i32,
            bottom: (rp.y + rp.h) as i32,
        };

        if x_range > f32::EPSILON {
            for (i, tick) in x_ticks.iter().enumerate() {
                let t_ = (tick - render_area.axis_x_min) / x_range;
                let sx = rp.x + t_ * rp.w;
                let lw = Self::measure_label_width(&self.axis_label_buffers[i]);
                let lx = (sx - lw / 2.0).max(rp.x + label_pad);
                let ly = rp.y + rp.h - label_h - label_pad;
                axis_text_areas.push(TextArea {
                    buffer: &self.axis_label_buffers[i],
                    left: lx,
                    top: ly,
                    scale: 1.0,
                    bounds: rp_bounds,
                    default_color: label_color,
                    custom_glyphs: &[],
                });
            }
        }

        if y_range > f32::EPSILON {
            for (i, tick) in y_ticks.iter().enumerate() {
                let t_ = (tick - render_area.axis_y_min) / y_range;
                let sy = rp.y + rp.h - t_ * rp.h;
                let lx = rp.x + label_pad;
                let ly = (sy - label_h - 1.0).max(rp.y + label_pad);
                axis_text_areas.push(TextArea {
                    buffer: &self.axis_label_buffers[MAX_AXIS_LABELS + i],
                    left: lx,
                    top: ly,
                    scale: 1.0,
                    bounds: rp_bounds,
                    default_color: label_color,
                    custom_glyphs: &[],
                });
            }
        }

        let mut cursor_bg_rects: Vec<RectInstance> = Vec::new();
        let mut cursor_text_areas: Vec<TextArea> = Vec::new();
        if hover == HoverTarget::RenderArea {
            let cursor_color = [1.0_f32, 1.0, 1.0, 0.8];
            let line_thickness = 2.0_f32;
            let [mu, mv] = render_area.mouse_uv;
            let cx = rp.x + mu * rp.w;
            let cy = rp.y + (1.0 - mv) * rp.h;

            axis_rects.push(RectInstance {
                x: cx - line_thickness / 2.0,
                y: rp.y,
                w: line_thickness,
                h: rp.h,
                color: cursor_color,
                corner_radius: 0.0,
                _padding: [0.0; 3],
            });
            axis_rects.push(RectInstance {
                x: rp.x,
                y: cy - line_thickness / 2.0,
                w: rp.w,
                h: line_thickness,
                color: cursor_color,
                corner_radius: 0.0,
                _padding: [0.0; 3],
            });

            let world_x = render_area.axis_x_min + mu * x_range;
            let world_y = render_area.axis_y_min + mv * y_range;
            let cursor_label_h = fonts::ui_size() * 1.4;
            let x_decimals = cursor_decimals(x_range);
            let y_decimals = cursor_decimals(y_range);
            let cursor_text_color = GlyphonColor::rgba(255, 255, 255, 255);

            let cx_text = format!("{:.prec$}", world_x, prec = x_decimals);
            self.cursor_x_label.set_text(
                &mut self.font_system,
                &cx_text,
                Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );
            self.cursor_x_label
                .shape_until_scroll(&mut self.font_system, false);
            let cx_lw = Self::measure_label_width(&self.cursor_x_label);
            let cx_lx = (cx - cx_lw / 2.0).clamp(rp.x + label_pad, rp.x + rp.w - cx_lw - label_pad);
            let cx_ly = rp.y + rp.h - cursor_label_h - label_pad;

            let cy_text = format!("{:.prec$}", world_y, prec = y_decimals);
            self.cursor_y_label.set_text(
                &mut self.font_system,
                &cy_text,
                Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );
            self.cursor_y_label
                .shape_until_scroll(&mut self.font_system, false);
            let cy_lw = Self::measure_label_width(&self.cursor_y_label);
            let cy_ly = (cy - cursor_label_h - 1.0)
                .clamp(rp.y + label_pad, rp.y + rp.h - cursor_label_h - label_pad);
            let cy_lx = rp.x + label_pad;

            let bg_pad = 3.0_f32 * scale;
            let bg_color = [0.0_f32, 0.0, 0.0, 0.9];
            cursor_bg_rects.push(RectInstance {
                x: cx_lx - bg_pad,
                y: cx_ly - bg_pad,
                w: cx_lw + bg_pad * 2.0,
                h: cursor_label_h + bg_pad * 2.0,
                color: bg_color,
                corner_radius: 3.0,
                _padding: [0.0; 3],
            });
            cursor_bg_rects.push(RectInstance {
                x: cy_lx - bg_pad,
                y: cy_ly - bg_pad,
                w: cy_lw + bg_pad * 2.0,
                h: cursor_label_h + bg_pad * 2.0,
                color: bg_color,
                corner_radius: 3.0,
                _padding: [0.0; 3],
            });

            cursor_text_areas.push(TextArea {
                buffer: &self.cursor_x_label,
                left: cx_lx,
                top: cx_ly,
                scale: 1.0,
                bounds: rp_bounds,
                default_color: cursor_text_color,
                custom_glyphs: &[],
            });
            cursor_text_areas.push(TextArea {
                buffer: &self.cursor_y_label,
                left: cy_lx,
                top: cy_ly,
                scale: 1.0,
                bounds: rp_bounds,
                default_color: cursor_text_color,
                custom_glyphs: &[],
            });
        }

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

        self.axis_text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.axis_atlas,
                &self.viewport,
                axis_text_areas,
                &mut self.swash_cache,
            )
            .unwrap();

        let frame = self.surface.get_current_texture().unwrap();
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.scene_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(t.bg_primary.to_wgpu()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.rect_renderer
                .draw(&self.queue, &mut pass, sw, sh, &ui_rects);
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        if self.shader_pipeline.has_active() {
            self.shader_pipeline.render(
                &mut encoder,
                &self.scene_view,
                &self.queue,
                &layout.right_pane,
                (self.surface_config.width, self.surface_config.height),
                [
                    render_area.axis_x_min,
                    render_area.axis_y_max,
                    render_area.axis_x_max,
                    render_area.axis_y_min,
                ],
                render_area.mouse_uv,
            );
        }

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("overlay_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.overlay_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.overlay_rect_renderer
                .draw(&self.queue, &mut pass, sw, sh, &axis_rects);
            self.axis_text_renderer
                .render(&self.axis_atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("composite_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &self.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        if !cursor_bg_rects.is_empty() {
            self.cursor_text_renderer
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut self.font_system,
                    &mut self.cursor_text_atlas,
                    &self.viewport,
                    cursor_text_areas,
                    &mut self.swash_cache,
                )
                .unwrap();

            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("cursor_overlay_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.cursor_rect_renderer
                .draw(&self.queue, &mut pass, sw, sh, &cursor_bg_rects);
            self.cursor_text_renderer
                .render(&self.cursor_text_atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.atlas.trim();
        self.axis_atlas.trim();
        self.cursor_text_atlas.trim();
    }
}
