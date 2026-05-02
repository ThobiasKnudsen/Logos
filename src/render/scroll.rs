use crate::ui::layout::Rect;
use crate::ui::theme::spacing;

use super::{shift_cell_layouts, Renderer};

impl Renderer {
    pub fn v_thumb_rect(&self) -> Option<Rect> {
        self.v_thumb_rect
    }

    /// Scroll the cell container by pixel deltas. Clamps to valid range.
    /// Also repositions cell layouts and scrollbar to match the new scroll.
    pub fn scroll_by(&mut self, _dx: f32, dy: f32) {
        let pane = self.cached_editor_pane;
        let visible_h = pane.h;

        let old_scroll = self.cell_scroll_y;
        self.cell_scroll_y += dy;
        let max_sy = (self.cells_total_height - visible_h).max(0.0);
        self.cell_scroll_y = self.cell_scroll_y.clamp(0.0, max_sy);

        let delta = self.cell_scroll_y - old_scroll;
        if delta.abs() > 0.001 {
            shift_cell_layouts(&mut self.cell_layouts, &mut self.add_cell_rect, delta);
        }

        self.update_scrollbar_rects(pane);
    }

    /// Scroll a cell's output text horizontally.
    pub fn scroll_output_x(&mut self, cell_index: usize, delta: f32) {
        if cell_index >= self.cell_output_scroll_x.len() {
            return;
        }
        let content_w = Self::measure_output_content_width(&self.cell_output_buffers[cell_index]);
        let visible_w = self
            .cell_layouts
            .iter()
            .find(|cl| cl.cell_index == cell_index)
            .map(|cl| cl.output.w - spacing::sm() * 2.0)
            .unwrap_or(0.0);
        let max_scroll = (content_w - visible_w).max(0.0);
        self.cell_output_scroll_x[cell_index] =
            (self.cell_output_scroll_x[cell_index] + delta).clamp(0.0, max_scroll);
    }

    /// Scroll a cell's editor text horizontally.
    pub fn scroll_editor_x(&mut self, cell_index: usize, delta: f32) {
        if cell_index >= self.cell_editor_scroll_x.len() {
            return;
        }
        if cell_index >= self.cell_buffers.len() {
            return;
        }
        let content_w = Self::measure_editor_content_width(&self.cell_buffers[cell_index]);
        let cl = self
            .cell_layouts
            .iter()
            .find(|cl| cl.cell_index == cell_index);
        let v_sb_inset = cl
            .map(|cl| {
                if cl.editor_v_scrollbar_track.h > 0.0 {
                    cl.editor.x + cl.editor.w - cl.editor_v_scrollbar_track.x
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);
        let visible_w = cl
            .map(|cl| cl.editor.w - spacing::sm() * 2.0 - v_sb_inset)
            .unwrap_or(0.0);
        let max_scroll = (content_w - visible_w).max(0.0);
        self.cell_editor_scroll_x[cell_index] =
            (self.cell_editor_scroll_x[cell_index] + delta).clamp(0.0, max_scroll);
    }

    /// Scroll a cell's editor text vertically (for contracted cells).
    /// Returns the unconsumed scroll delta (for passthrough to notebook scroll).
    pub fn scroll_editor_y(&mut self, cell_index: usize, delta: f32) -> f32 {
        if cell_index >= self.cell_editor_scroll_y.len() {
            return delta;
        }
        let content_h = self
            .cell_content_heights
            .get(cell_index)
            .copied()
            .unwrap_or(0.0);
        let visible_h = self
            .cell_layouts
            .iter()
            .find(|cl| cl.cell_index == cell_index)
            .map(|cl| cl.editor.h - spacing::sm() * 2.0)
            .unwrap_or(0.0);
        let max_scroll = (content_h - visible_h).max(0.0);
        if max_scroll <= 0.0 {
            return delta;
        }

        let old = self.cell_editor_scroll_y[cell_index];
        let new = (old + delta).clamp(0.0, max_scroll);
        self.cell_editor_scroll_y[cell_index] = new;
        let consumed = new - old;
        delta - consumed
    }

    /// Set editor horizontal scroll from scrollbar thumb drag.
    pub fn set_editor_h_scroll_from_drag(
        &mut self,
        cell_index: usize,
        mouse_x: f32,
        drag_offset: f32,
    ) {
        let cl = match self
            .cell_layouts
            .iter()
            .find(|c| c.cell_index == cell_index)
        {
            Some(c) => *c,
            None => return,
        };
        let track = cl.editor_h_scrollbar_track;
        let thumb = cl.editor_h_scrollbar_thumb;
        if track.w <= 0.0 || thumb.w <= 0.0 {
            return;
        }
        if cell_index >= self.cell_buffers.len() || cell_index >= self.cell_editor_scroll_x.len() {
            return;
        }
        let content_w = Self::measure_editor_content_width(&self.cell_buffers[cell_index]);
        let v_sb_inset = if cl.editor_v_scrollbar_track.h > 0.0 {
            cl.editor.x + cl.editor.w - cl.editor_v_scrollbar_track.x
        } else {
            0.0
        };
        let visible_w = cl.editor.w - spacing::sm() * 2.0 - v_sb_inset;
        let max_scroll = (content_w - visible_w).max(0.0);
        let new_thumb_x = mouse_x - drag_offset;
        let range = track.w - thumb.w;
        if range > 0.0 {
            let ratio = ((new_thumb_x - track.x) / range).clamp(0.0, 1.0);
            self.cell_editor_scroll_x[cell_index] = ratio * max_scroll;
        }
    }

    /// Set editor vertical scroll from scrollbar thumb drag.
    pub fn set_editor_v_scroll_from_drag(
        &mut self,
        cell_index: usize,
        mouse_y: f32,
        drag_offset: f32,
    ) {
        let cl = match self
            .cell_layouts
            .iter()
            .find(|c| c.cell_index == cell_index)
        {
            Some(c) => *c,
            None => return,
        };
        let track = cl.editor_v_scrollbar_track;
        let thumb = cl.editor_v_scrollbar_thumb;
        if track.h <= 0.0 || thumb.h <= 0.0 {
            return;
        }
        if cell_index >= self.cell_editor_scroll_y.len() {
            return;
        }
        let content_h = self
            .cell_content_heights
            .get(cell_index)
            .copied()
            .unwrap_or(0.0);
        let visible_h = cl.editor.h - spacing::sm() * 2.0;
        let max_scroll = (content_h - visible_h).max(0.0);
        let new_thumb_y = mouse_y - drag_offset;
        let range = track.h - thumb.h;
        if range > 0.0 {
            let ratio = ((new_thumb_y - track.y) / range).clamp(0.0, 1.0);
            self.cell_editor_scroll_y[cell_index] = ratio * max_scroll;
        }
    }

    /// Check if a cell is contracted (has vertical scroll capacity).
    pub fn cell_is_contracted(&self, cell_index: usize) -> bool {
        let content_h = self
            .cell_content_heights
            .get(cell_index)
            .copied()
            .unwrap_or(0.0);
        let visible_h = self
            .cell_layouts
            .iter()
            .find(|cl| cl.cell_index == cell_index)
            .map(|cl| cl.editor.h - spacing::sm() * 2.0)
            .unwrap_or(0.0);
        content_h > visible_h + 1.0
    }

    /// Set vertical scroll position from scrollbar thumb drag.
    pub fn set_v_scroll_from_drag(&mut self, mouse_y: f32, drag_offset: f32) {
        if let (Some(track), Some(thumb)) = (self.v_track_rect, self.v_thumb_rect) {
            let pane = self.cached_editor_pane;
            let visible_h = pane.h;
            let max_scroll = (self.cells_total_height - visible_h).max(0.0);

            let new_thumb_y = mouse_y - drag_offset;
            let range = track.h - thumb.h;
            if range > 0.0 {
                let ratio = ((new_thumb_y - track.y) / range).clamp(0.0, 1.0);
                self.cell_scroll_y = ratio * max_scroll;
            }

            self.update_scrollbar_rects(pane);
        }
    }

    /// Recompute vertical scrollbar from cell container state.
    pub(super) fn update_scrollbar_rects(&mut self, pane: Rect) {
        let visible_h = pane.h;
        let need_v = self.cells_total_height > visible_h;

        if need_v && visible_h > 0.0 && self.cells_total_height > 0.0 {
            let sb_w = spacing::scrollbar_width();
            let sb_gap = 4.0;
            let track_h = pane.h;
            let sb_x = pane.x + pane.w - sb_w - sb_gap;
            self.v_track_rect = Some(Rect {
                x: sb_x,
                y: pane.y,
                w: sb_w,
                h: track_h,
            });

            let ratio = visible_h / self.cells_total_height;
            let thumb_h = (track_h * ratio).max(spacing::scrollbar_thumb_min_h());
            let max_scroll = (self.cells_total_height - visible_h).max(0.0);
            let thumb_y = if max_scroll > 0.0 {
                pane.y + (self.cell_scroll_y / max_scroll) * (track_h - thumb_h)
            } else {
                pane.y
            };
            self.v_thumb_rect = Some(Rect {
                x: sb_x,
                y: thumb_y,
                w: sb_w,
                h: thumb_h,
            });
        } else {
            self.v_track_rect = None;
            self.v_thumb_rect = None;
        }
    }
}
