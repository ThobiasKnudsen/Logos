use glyphon::{Attrs, Family, Shaping};

use crate::app::{self, MenuItemDef};
use crate::editor::autocomplete::CandidateKind;
use crate::ui::layout::Rect;
use crate::ui::theme::{font_family, fonts, spacing};

use super::{menu_item_pad, Renderer};

impl Renderer {
    pub fn set_maximized_icon(&mut self, is_maximized: bool) {
        let icon = if is_maximized { "\u{274F}" } else { "\u{25A1}" };
        let family = font_family::active_family();
        self.win_max_label.set_text(
            &mut self.font_system,
            icon,
            Attrs::new().family(Family::Name(family)),
            Shaping::Advanced,
        );
        self.win_max_label
            .shape_until_scroll(&mut self.font_system, false);
    }

    /// Update menu item positions from title bar rect. Returns hit rects.
    pub fn update_menu_items(&mut self, title_bar: Rect, win_ctrl_start_x: f32) -> Vec<Rect> {
        let mut rects = Vec::with_capacity(app::MENU_NAMES.len());
        let mut x = title_bar.x + spacing::sm();
        let y = title_bar.y;
        let h = title_bar.h;

        for label in &self.menu_item_labels {
            let text_w = Self::measure_label_width(label);
            let item_w = menu_item_pad() * 2.0 + text_w;
            if x + item_w > win_ctrl_start_x {
                break;
            }
            rects.push(Rect { x, y, w: item_w, h });
            x += item_w;
        }

        self.menu_item_rects = rects.clone();
        rects
    }

    /// Open a dropdown menu. `active_item` highlights one item (e.g. current theme).
    /// Returns item rects for hit testing.
    pub fn open_dropdown(
        &mut self,
        menu_index: usize,
        menu_rect: Rect,
        active_item: Option<usize>,
    ) -> Vec<Rect> {
        let static_items: &[MenuItemDef] = app::menu_items(menu_index);
        let dynamic_count = app::dynamic_menu_count(menu_index);
        let item_count = dynamic_count.unwrap_or(static_items.len());
        if item_count == 0 {
            self.dropdown_active = false;
            return Vec::new();
        }

        let item_h = spacing::dropdown_item_height();
        let pad = spacing::dropdown_padding();

        self.dropdown_item_labels.clear();
        self.dropdown_shortcut_labels.clear();
        self.dropdown_active_item = active_item;
        let mut max_label_w = 0.0_f32;
        let mut max_shortcut_w = 0.0_f32;

        #[allow(clippy::needless_range_loop)]
        for i in 0..item_count {
            let (item_label, item_shortcut) = if dynamic_count.is_some() {
                (app::dynamic_menu_label(menu_index, i), "")
            } else {
                (static_items[i].label.to_string(), static_items[i].shortcut)
            };
            let label_text = if active_item == Some(i) {
                format!("\u{2713} {}", item_label)
            } else {
                format!("   {}", item_label)
            };
            let label = Self::create_label(&mut self.font_system, fonts::menu_size(), &label_text);
            let shortcut =
                Self::create_label(&mut self.font_system, fonts::small_size(), item_shortcut);
            max_label_w = max_label_w.max(Self::measure_label_width(&label));
            max_shortcut_w = max_shortcut_w.max(Self::measure_label_width(&shortcut));
            self.dropdown_item_labels.push(label);
            self.dropdown_shortcut_labels.push(shortcut);
        }

        let dropdown_w = (max_label_w + max_shortcut_w + menu_item_pad() * 4.0)
            .max(spacing::dropdown_min_width());
        let dropdown_h = item_count as f32 * item_h + pad * 2.0;

        let x = menu_rect.x;
        let y = menu_rect.y + menu_rect.h;

        self.dropdown_bg = Rect {
            x,
            y,
            w: dropdown_w,
            h: dropdown_h,
        };

        let mut item_rects = Vec::with_capacity(item_count);
        for i in 0..item_count {
            item_rects.push(Rect {
                x,
                y: y + pad + i as f32 * item_h,
                w: dropdown_w,
                h: item_h,
            });
        }

        self.dropdown_item_rects = item_rects.clone();
        self.dropdown_active = true;
        item_rects
    }

    pub fn close_dropdown(&mut self) {
        self.dropdown_active = false;
        self.dropdown_item_labels.clear();
        self.dropdown_shortcut_labels.clear();
        self.dropdown_item_rects.clear();
    }

    /// Open the autocomplete popup at the given position with candidates.
    /// Returns item rects for hit-testing.
    pub fn open_autocomplete(
        &mut self,
        x: f32,
        y: f32,
        candidates: &[(String, CandidateKind)],
        selected_index: usize,
        pane: Rect,
    ) -> Vec<Rect> {
        self.ac_item_labels.clear();
        self.ac_kind_labels.clear();
        self.ac_item_rects.clear();

        if candidates.is_empty() {
            self.ac_active = false;
            return Vec::new();
        }

        let item_h = spacing::dropdown_item_height();
        let pad = spacing::dropdown_padding();

        let mut max_label_w = 0.0_f32;
        let mut max_kind_w = 0.0_f32;

        for (label, kind) in candidates {
            let lbl = Self::create_label(&mut self.font_system, fonts::editor_size(), label);
            let badge =
                Self::create_label(&mut self.font_system, fonts::small_size(), kind.badge());
            max_label_w = max_label_w.max(Self::measure_label_width(&lbl));
            max_kind_w = max_kind_w.max(Self::measure_label_width(&badge));
            self.ac_item_labels.push(lbl);
            self.ac_kind_labels.push(badge);
        }

        let popup_w = (max_kind_w + spacing::sm() + max_label_w + menu_item_pad() * 2.0)
            .max(120.0 * fonts::scale());
        let popup_h = candidates.len() as f32 * item_h + pad * 2.0;

        let mut popup_x = x;
        let mut popup_y = y;

        if popup_y + popup_h > pane.y + pane.h {
            popup_y = y - fonts::editor_line_height() - popup_h;
        }

        if popup_x + popup_w > pane.x + pane.w {
            popup_x = (pane.x + pane.w - popup_w).max(pane.x);
        }

        self.ac_bg = Rect {
            x: popup_x,
            y: popup_y,
            w: popup_w,
            h: popup_h,
        };

        let mut item_rects = Vec::with_capacity(candidates.len());
        for i in 0..candidates.len() {
            item_rects.push(Rect {
                x: popup_x,
                y: popup_y + pad + i as f32 * item_h,
                w: popup_w,
                h: item_h,
            });
        }

        self.ac_item_rects = item_rects.clone();
        self.ac_selected_index = selected_index;
        self.ac_active = true;
        item_rects
    }

    pub fn close_autocomplete(&mut self) {
        self.ac_active = false;
        self.ac_item_labels.clear();
        self.ac_kind_labels.clear();
        self.ac_item_rects.clear();
    }

    pub fn update_autocomplete_selection(&mut self, index: usize) {
        self.ac_selected_index = index;
    }

    pub fn autocomplete_bg_rect(&self) -> Option<Rect> {
        if self.ac_active {
            Some(self.ac_bg)
        } else {
            None
        }
    }

    /// Returns the cursor position (content-relative x, y, line_height) for the active cell.
    pub fn cursor_content_pos(&self) -> (f32, f32, f32) {
        self.cursor_content_pos
    }

    /// Returns the dropdown background rect if a dropdown is active.
    pub fn dropdown_bg_rect(&self) -> Option<Rect> {
        if self.dropdown_active {
            Some(self.dropdown_bg)
        } else {
            None
        }
    }
}
