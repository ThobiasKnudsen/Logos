use crate::ui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MouseZone {
    Center,
    XAxisEdge,
    YAxisEdge,
}

pub(crate) struct RenderAreaState {
    pub axis_x_min: f32,
    pub axis_x_max: f32,
    pub axis_y_min: f32,
    pub axis_y_max: f32,
    /// World units per screen pixel — kept equal for X and Y (square pixels in world space).
    pub world_per_pixel: f32,
    pub prev_pane: Option<Rect>,
    pub prev_win_pos: (f32, f32),
    pub prev_win_size: (u32, u32),
    pub is_dragging: bool,
    pub last_drag_pos: (f32, f32),
    pub mouse_zone: MouseZone,
}

impl Default for RenderAreaState {
    fn default() -> Self {
        Self {
            axis_x_min: -5.0,
            axis_x_max: 5.0,
            axis_y_min: -5.0,
            axis_y_max: 5.0,
            world_per_pixel: 0.0,
            prev_pane: None,
            prev_win_pos: (0.0, 0.0),
            prev_win_size: (0, 0),
            is_dragging: false,
            last_drag_pos: (0.0, 0.0),
            mouse_zone: MouseZone::Center,
        }
    }
}

impl RenderAreaState {
    pub fn adjust_for_pane_change(
        &mut self,
        new_pane: &Rect,
        win_pos: (f32, f32),
        win_size: (u32, u32),
    ) {
        if new_pane.w <= 0.0 || new_pane.h <= 0.0 {
            return;
        }

        if let Some(old) = self.prev_pane {
            let wpp_x = if old.w > 0.0 {
                (self.axis_x_max - self.axis_x_min) / old.w
            } else {
                self.world_per_pixel
            };
            let wpp_y = if old.h > 0.0 {
                (self.axis_y_max - self.axis_y_min) / old.h
            } else {
                self.world_per_pixel
            };

            let old_screen_left = self.prev_win_pos.0 + old.x;
            let new_screen_left = win_pos.0 + new_pane.x;
            self.axis_x_min += (new_screen_left - old_screen_left) * wpp_x;
            self.axis_x_max = self.axis_x_min + new_pane.w * wpp_x;

            let old_screen_bottom = self.prev_win_pos.1 + old.y + old.h;
            let new_screen_bottom = win_pos.1 + new_pane.y + new_pane.h;
            self.axis_y_min -= (new_screen_bottom - old_screen_bottom) * wpp_y;
            self.axis_y_max = self.axis_y_min + new_pane.h * wpp_y;
        } else {
            let wpp_x = (self.axis_x_max - self.axis_x_min) / new_pane.w;
            let wpp_y = (self.axis_y_max - self.axis_y_min) / new_pane.h;
            self.world_per_pixel = wpp_x.max(wpp_y);

            let cx = (self.axis_x_min + self.axis_x_max) / 2.0;
            let cy = (self.axis_y_min + self.axis_y_max) / 2.0;
            let half_w = new_pane.w * self.world_per_pixel / 2.0;
            let half_h = new_pane.h * self.world_per_pixel / 2.0;
            self.axis_x_min = cx - half_w;
            self.axis_x_max = cx + half_w;
            self.axis_y_min = cy - half_h;
            self.axis_y_max = cy + half_h;
        }

        self.prev_pane = Some(*new_pane);
        self.prev_win_pos = win_pos;
        self.prev_win_size = win_size;
    }

    pub fn zoom_uniform(&mut self, factor: f32, cursor_screen: (f32, f32), pane: &Rect) {
        if pane.w <= 0.0 || pane.h <= 0.0 {
            return;
        }

        let uv_x = ((cursor_screen.0 - pane.x) / pane.w).clamp(0.0, 1.0);
        let t_y = ((pane.y + pane.h - cursor_screen.1) / pane.h).clamp(0.0, 1.0);

        let wx = self.axis_x_min + uv_x * (self.axis_x_max - self.axis_x_min);
        let wy = self.axis_y_min + t_y * (self.axis_y_max - self.axis_y_min);

        let x_range = (self.axis_x_max - self.axis_x_min) * factor;
        let y_range = (self.axis_y_max - self.axis_y_min) * factor;

        self.axis_x_min = wx - uv_x * x_range;
        self.axis_x_max = self.axis_x_min + x_range;
        self.axis_y_min = wy - t_y * y_range;
        self.axis_y_max = self.axis_y_min + y_range;
    }

    pub fn zoom_x(&mut self, factor: f32, cursor_screen: (f32, f32), pane: &Rect) {
        if pane.w <= 0.0 {
            return;
        }
        let uv_x = ((cursor_screen.0 - pane.x) / pane.w).clamp(0.0, 1.0);
        let wx = self.axis_x_min + uv_x * (self.axis_x_max - self.axis_x_min);
        let range = (self.axis_x_max - self.axis_x_min) * factor;
        self.axis_x_min = wx - uv_x * range;
        self.axis_x_max = self.axis_x_min + range;
    }

    pub fn zoom_y(&mut self, factor: f32, cursor_screen: (f32, f32), pane: &Rect) {
        if pane.h <= 0.0 {
            return;
        }
        let t_y = ((pane.y + pane.h - cursor_screen.1) / pane.h).clamp(0.0, 1.0);
        let wy = self.axis_y_min + t_y * (self.axis_y_max - self.axis_y_min);
        let range = (self.axis_y_max - self.axis_y_min) * factor;
        self.axis_y_min = wy - t_y * range;
        self.axis_y_max = self.axis_y_min + range;
    }
}
