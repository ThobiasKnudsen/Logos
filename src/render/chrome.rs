use glyphon::{Attrs, Family, Shaping};

use crate::ui::layout::Rect;
use crate::ui::theme::{font_family, fonts};

use super::{
    tab_close_pad, tab_close_size, tab_dot_pad, tab_pad_h, Renderer, TabHitRect, TabInfo,
};

impl Renderer {
    pub fn update_status(&mut self, text: &str) {
        if self.cached_status_text == text {
            return;
        }
        self.cached_status_text = text.to_string();
        let family = font_family::active_family();
        self.status_label.set_text(
            &mut self.font_system,
            text,
            &Attrs::new().family(Family::Name(family)),
            Shaping::Advanced,
            None,
        );
        self.status_label
            .shape_until_scroll(&mut self.font_system, false);
    }

    pub fn update_tab_bar(
        &mut self,
        tabs: &[TabInfo],
        tab_bar_rect: Rect,
    ) -> Option<(Vec<TabHitRect>, Rect)> {
        let new_info: Vec<(String, bool, bool, bool)> = tabs
            .iter()
            .map(|t| (t.name.clone(), t.is_active, t.is_modified, t.is_untitled))
            .collect();
        if new_info == self.cached_tab_info {
            return None;
        }
        self.cached_tab_info = new_info;

        self.tab_labels.clear();
        self.tab_close_labels.clear();
        self.tab_modified.clear();
        self.tab_bg_rects.clear();
        self.tab_close_rects.clear();

        let tab_h = tab_bar_rect.h;
        let mut x = tab_bar_rect.x;
        let y = tab_bar_rect.y;
        let mut hit_rects = Vec::with_capacity(tabs.len());

        let dot_w = Self::measure_label_width(&self.dot_label);
        let dot_area = tab_dot_pad() + dot_w + tab_dot_pad();

        for tab in tabs {
            let label = if tab.is_untitled {
                Self::create_label_italic(&mut self.font_system, fonts::ui_size(), &tab.name)
            } else {
                Self::create_label(&mut self.font_system, fonts::ui_size(), &tab.name)
            };
            let text_w = Self::measure_label_width(&label);
            let close_label =
                Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2715}");
            let left_pad = if tab.is_modified {
                dot_area
            } else {
                tab_pad_h()
            };
            let tab_w = left_pad + text_w + tab_close_pad() + tab_close_size() + tab_pad_h();
            let tab_rect = Rect {
                x,
                y,
                w: tab_w,
                h: tab_h,
            };
            let close_rect = Rect {
                x: x + tab_w - tab_pad_h() - tab_close_size(),
                y: y + (tab_h - tab_close_size()) / 2.0,
                w: tab_close_size(),
                h: tab_close_size(),
            };

            self.tab_bg_rects.push((tab_rect, tab.is_active));
            self.tab_close_rects.push(close_rect);
            self.tab_labels.push(label);
            self.tab_close_labels.push(close_label);
            self.tab_modified.push(tab.is_modified);
            hit_rects.push(TabHitRect {
                full: tab_rect,
                close: close_rect,
            });
            x += tab_w;
        }

        let plus_w = tab_pad_h() * 2.0 + Self::measure_label_width(&self.plus_label);
        self.plus_rect = Rect {
            x,
            y,
            w: plus_w,
            h: tab_h,
        };
        Some((hit_rects, self.plus_rect))
    }
}
