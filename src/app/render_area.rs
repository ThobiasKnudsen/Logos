use crate::ui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MouseZone {
    Center,
    XAxisEdge,
    YAxisEdge,
}

/// Which render-area visual element a toggle button controls. Used as
/// the payload of `HoverTarget::RenderAreaToggle` and the entry point
/// for click → state mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToggleKind {
    /// Background major/minor gridlines.
    Grid,
    /// Mouse-tracking crosshair lines.
    Cursor,
    /// Tick labels along the axes.
    AxisNumbers,
    /// 2D vs 3D projection mode (3D camera is not yet wired; toggling
    /// it stashes the user's preference so it picks up when 3D lands).
    ViewMode,
}

impl ToggleKind {
    /// Stable ordering for layout (left → right): grid, cursor, axis
    /// numbers, then view mode. Used by the renderer to place the four
    /// buttons in a deterministic row.
    pub fn all() -> [Self; 4] {
        [Self::Grid, Self::Cursor, Self::AxisNumbers, Self::ViewMode]
    }
}

/// Persistent visibility toggles for the render area. Saved per-tab on
/// `NotebookView` so each notebook keeps its own preferences when the
/// user flips between tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewToggles {
    pub show_grid: bool,
    pub show_cursor: bool,
    pub show_axis_numbers: bool,
    /// `true` ⇒ 3D projection. The 2D/3D camera split is pending; the
    /// toggle is stored so user intent survives the upcoming work.
    pub is_3d: bool,
}

impl Default for ViewToggles {
    fn default() -> Self {
        Self {
            show_grid: true,
            show_cursor: true,
            show_axis_numbers: true,
            is_3d: false,
        }
    }
}

impl ViewToggles {
    pub fn get(&self, kind: ToggleKind) -> bool {
        match kind {
            ToggleKind::Grid => self.show_grid,
            ToggleKind::Cursor => self.show_cursor,
            ToggleKind::AxisNumbers => self.show_axis_numbers,
            ToggleKind::ViewMode => self.is_3d,
        }
    }

    pub fn toggle(&mut self, kind: ToggleKind) {
        match kind {
            ToggleKind::Grid => self.show_grid = !self.show_grid,
            ToggleKind::Cursor => self.show_cursor = !self.show_cursor,
            ToggleKind::AxisNumbers => self.show_axis_numbers = !self.show_axis_numbers,
            ToggleKind::ViewMode => self.is_3d = !self.is_3d,
        }
    }
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
    /// Visibility toggles. Mutated by clicks on the render-area chrome
    /// buttons and persisted to the active `NotebookView` on tab switch.
    pub toggles: ViewToggles,
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
            toggles: ViewToggles::default(),
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

#[cfg(test)]
mod toggle_tests {
    use super::*;

    /// Default toggle state: grid/cursor/axis-numbers on, 3D off.
    /// Locks in the issue #27 "default-friendly" UX so a regression
    /// silently hiding the grid would surface immediately in tests.
    #[test]
    fn default_view_toggles_show_everything_in_2d() {
        let v = ViewToggles::default();
        assert!(v.show_grid);
        assert!(v.show_cursor);
        assert!(v.show_axis_numbers);
        assert!(!v.is_3d);
    }

    /// Each toggle flips independently — flipping one must not bleed
    /// into another, which is the explicit acceptance criterion in
    /// issue #27 ("Each toggle is independent").
    #[test]
    fn toggles_are_independent() {
        for target in ToggleKind::all() {
            let mut v = ViewToggles::default();
            let originals = [
                v.show_grid,
                v.show_cursor,
                v.show_axis_numbers,
                v.is_3d,
            ];
            v.toggle(target);
            let after = [v.show_grid, v.show_cursor, v.show_axis_numbers, v.is_3d];
            for (i, kind) in ToggleKind::all().iter().enumerate() {
                if *kind == target {
                    assert_ne!(originals[i], after[i], "toggling {:?} did not flip it", target);
                } else {
                    assert_eq!(
                        originals[i], after[i],
                        "toggling {:?} unexpectedly flipped {:?}",
                        target, kind
                    );
                }
            }
        }
    }

    /// Two toggles return to the original state — guards against a
    /// future refactor that accidentally mutates extra state on toggle.
    #[test]
    fn double_toggle_is_identity() {
        let mut v = ViewToggles::default();
        for kind in ToggleKind::all() {
            v.toggle(kind);
            v.toggle(kind);
        }
        assert_eq!(v, ViewToggles::default());
    }
}
