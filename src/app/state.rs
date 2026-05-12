use std::time::Instant;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::CursorIcon;

use crate::editor::autocomplete;
use crate::file_dialog::{self, DialogKind, FileDialog};
use crate::notebook::{CellMessage, CellState, Severity};
use crate::render::{CellInfo, TabInfo};
use crate::ui::theme::{self, font_family, fonts};

pub(super) const FEEDBACK_URL: &str = "https://example.com";

use super::cas::format_shader_error;
use super::menus::{active_font_index, active_theme_index, example_path};
use super::{FONTS_MENU_INDEX, HELP_MENU_INDEX, THEME_MENU_INDEX};
use super::render_area::{MouseZone, RenderAreaState};
use super::{
    compute_win_control_rects, detect_mouse_zone, edge_resize_direction, line_col_from,
    point_in_rect, win_pos, AppState, HoverTarget,
};

impl AppState {
    /// Sync renderer with the active tab's cells and all tab infos.
    pub(super) fn sync_active_tab(&mut self) {
        static SYNC_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SYNC_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let sync_start = Instant::now();

        let lp = self.cached_layout.left_pane;
        let tab = self.session.active_tab();
        let cell_count = tab.cells().len();
        self.last_submitted_texts.resize(cell_count, String::new());

        let t0 = Instant::now();
        let cell_infos: Vec<CellInfo> = tab
            .cells()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let text = c.buffer.text().to_string();
                if self.last_submitted_texts[i] != text {
                    self.lang_service.submit(i, text.clone());
                    self.last_submitted_texts[i].clone_from(&text);
                }
                CellInfo {
                    text,
                    cursor_byte: c.buffer.cursor_byte_offset(),
                    is_playing: matches!(c.state, CellState::Playing),
                    selection: c.buffer.selection_range(),
                    output_text: c
                        .outcome
                        .message
                        .as_ref()
                        .map(|m| m.display_text().to_string()),
                    is_error: c
                        .outcome
                        .message
                        .as_ref()
                        .is_some_and(|m| m.is_error()),
                    output_collapsed: c.output_collapsed,
                    contracted_editor_h: c.contracted_editor_h,
                    plot_color: c.plot_color,
                }
            })
            .collect();
        let build_cell_infos_us = t0.elapsed().as_micros();

        let t1 = Instant::now();
        self.cell_layouts = self
            .renderer
            .update_cells(&cell_infos, tab.active_cell_index, lp);
        let update_cells_us = t1.elapsed().as_micros();

        let t2 = Instant::now();
        let tab_infos: Vec<TabInfo> = self
            .session
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| TabInfo {
                name: t.name.clone(),
                is_active: i == self.session.active_index,
                is_modified: t.is_modified,
                is_untitled: t.file_path.is_none(),
            })
            .collect();

        if let Some((hit_rects, plus_rect)) = self
            .renderer
            .update_tab_bar(&tab_infos, self.cached_layout.tab_bar)
        {
            self.tab_hit_rects = hit_rects;
            self.plus_button_rect = plus_rect;
        }

        self.win_control_rects = compute_win_control_rects(&self.cached_layout.title_bar);

        let title_bar = self.cached_layout.title_bar;
        let win_min_x = self.win_control_rects.minimize.x;

        // Logo first so the menu bar knows where it can start.
        self.renderer.update_logo(title_bar);

        // Reserve horizontal space for the feedback button when capping
        // the menu bar, so menus + button + win-controls all coexist.
        let feedback_w = self.renderer.feedback_button_intrinsic_width();
        let gap = crate::ui::theme::spacing::xs();
        let menus_cap = (win_min_x - feedback_w - gap * 2.0).max(title_bar.x);

        self.menu_item_rects = self.renderer.update_menu_items(title_bar, menus_cap);

        let menus_right = self
            .menu_item_rects
            .last()
            .map(|r| r.x + r.w)
            .unwrap_or(title_bar.x + crate::ui::theme::spacing::sm());

        self.feedback_button_rect =
            self.renderer
                .update_feedback_button(title_bar, menus_right, win_min_x);

        // Color-picker geometry: anchored under the open cell's color
        // button, follows the cell as scroll / resize moves it. Closes
        // automatically if the source cell has gone away.
        if let Some(idx) = self.open_color_picker {
            let anchor = self
                .cell_layouts
                .iter()
                .find(|cl| cl.cell_index == idx)
                .map(|cl| cl.color_button);
            match anchor {
                Some(anchor_rect) => {
                    let viewport_w = self.cached_layout.title_bar.w
                        + self.cached_layout.title_bar.x;
                    self.renderer.update_color_picker(anchor_rect, viewport_w);
                }
                None => {
                    self.open_color_picker = None;
                    self.color_picker_drag = None;
                    self.renderer.clear_color_picker();
                }
            }
        } else {
            self.renderer.clear_color_picker();
        }
        let ui_chrome_us = t2.elapsed().as_micros();

        let t3 = Instant::now();
        let tab = self.session.active_tab();
        let cell = tab.active_cell();
        let (line, col) = line_col_from(cell.buffer.text(), cell.buffer.cursor_byte_offset());
        let line_count = cell.buffer.text().lines().count().max(1);
        let name = tab
            .file_path
            .as_ref()
            .map(|p| crate::session::display_name_for_path(p))
            .unwrap_or_else(|| tab.name.clone());
        let modified = if tab.is_modified { " [modified]" } else { "" };
        self.renderer.update_status(&format!(
            "{}{} \u{2502} {} lines \u{2502} Ln {}, Col {} \u{2502} {}",
            name,
            modified,
            line_count,
            line + 1,
            col + 1,
            theme::active_theme_name(),
        ));
        let status_bar_us = t3.elapsed().as_micros();

        let total_us = sync_start.elapsed().as_micros();
        log::debug!(
            "[perf] sync_active_tab[#{}]: {}us total | build_cell_infos={}us update_cells={}us ui_chrome={}us status_bar={}us",
            seq, total_us, build_cell_infos_us, update_cells_us, ui_chrome_us, status_bar_us
        );

        self.window.request_redraw();
    }

    /// Determine what the cursor is hovering over and set the cursor icon.
    pub(super) fn recompute_hover(&mut self) {
        let (mx, my) = self.cursor_position;
        let wc = self.win_control_rects;

        if !self.is_maximized {
            let size = self.window.inner_size();
            if let Some(dir) = edge_resize_direction(mx, my, size.width as f32, size.height as f32)
            {
                self.set_hover(HoverTarget::WindowEdge(dir));
                return;
            }
        }

        if self.open_color_picker.is_some() {
            let slider_rects = self.renderer.color_picker_slider_rects();
            for (idx, r) in slider_rects.iter().enumerate() {
                if r.w > 0.0 && point_in_rect(mx, my, r) {
                    self.set_hover(HoverTarget::ColorPickerSlider(idx as u8));
                    return;
                }
            }
            if let Some(bg) = self.renderer.color_picker_bg_rect() {
                if point_in_rect(mx, my, &bg) {
                    self.set_hover(HoverTarget::ColorPickerArea);
                    return;
                }
            }
        }

        if self.autocomplete.active {
            for (i, rect) in self.autocomplete_item_rects.iter().enumerate() {
                if point_in_rect(mx, my, rect) {
                    self.set_hover(HoverTarget::AutocompleteItem(i));
                    return;
                }
            }
            if let Some(bg) = self.renderer.autocomplete_bg_rect() {
                if point_in_rect(mx, my, &bg) {
                    self.set_hover(HoverTarget::None);
                    return;
                }
            }
        }

        if self.open_menu.is_some() {
            for (i, rect) in self.dropdown_item_rects.iter().enumerate() {
                if point_in_rect(mx, my, rect) {
                    self.set_hover(HoverTarget::DropdownItem(i));
                    return;
                }
            }
            if let Some(bg) = self.renderer.dropdown_bg_rect() {
                if point_in_rect(mx, my, &bg) {
                    self.set_hover(HoverTarget::None);
                    return;
                }
            }
            let on_menu_item = self
                .menu_item_rects
                .iter()
                .any(|r| point_in_rect(mx, my, r));
            if !on_menu_item {
                self.close_menu();
            }
        }

        for (i, rect) in self.menu_item_rects.iter().enumerate() {
            if point_in_rect(mx, my, rect) {
                self.set_hover(HoverTarget::MenuItem(i));
                return;
            }
        }

        if point_in_rect(mx, my, &wc.close) {
            self.set_hover(HoverTarget::WinBtnClose);
            return;
        }
        if point_in_rect(mx, my, &wc.maximize) {
            self.set_hover(HoverTarget::WinBtnMaximize);
            return;
        }
        if point_in_rect(mx, my, &wc.minimize) {
            self.set_hover(HoverTarget::WinBtnMinimize);
            return;
        }

        if self.feedback_button_rect.w > 0.0
            && point_in_rect(mx, my, &self.feedback_button_rect)
        {
            self.set_hover(HoverTarget::FeedbackButton);
            return;
        }

        if point_in_rect(mx, my, &self.cached_layout.title_bar) {
            self.set_hover(HoverTarget::TitleBar);
            return;
        }

        for (i, hit) in self.tab_hit_rects.iter().enumerate() {
            if point_in_rect(mx, my, &hit.close) {
                self.set_hover(HoverTarget::TabClose(i));
                return;
            }
        }

        for (i, hit) in self.tab_hit_rects.iter().enumerate() {
            if point_in_rect(mx, my, &hit.full) {
                self.set_hover(HoverTarget::Tab(i));
                return;
            }
        }

        if point_in_rect(mx, my, &self.plus_button_rect) {
            self.set_hover(HoverTarget::PlusButton);
            return;
        }

        if point_in_rect(mx, my, &self.cached_layout.split_handle) {
            self.set_hover(HoverTarget::SplitHandle);
            return;
        }

        if let Some(thumb) = self.renderer.v_thumb_rect() {
            if point_in_rect(mx, my, &thumb) {
                self.set_hover(HoverTarget::VScrollThumb);
                return;
            }
        }

        let lp = self.cached_layout.left_pane;
        if point_in_rect(mx, my, &lp) {
            let add_rect = self.renderer.add_cell_button_rect();
            if point_in_rect(mx, my, &add_rect) {
                self.set_hover(HoverTarget::AddCellButton);
                return;
            }

            for cl in &self.cell_layouts {
                if point_in_rect(mx, my, &cl.play_button) {
                    self.set_hover(HoverTarget::CellPlayButton(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.color_button) {
                    self.set_hover(HoverTarget::CellColorButton(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.copy_button) {
                    self.set_hover(HoverTarget::CellCopyButton(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.delete_button) {
                    self.set_hover(HoverTarget::CellDeleteButton(cl.cell_index));
                    return;
                }
                if cl.editor_h_scrollbar_thumb.h > 0.0
                    && point_in_rect(mx, my, &cl.editor_h_scrollbar_thumb)
                {
                    self.set_hover(HoverTarget::CellEditorHScrollThumb(cl.cell_index));
                    return;
                }
                if cl.editor_v_scrollbar_thumb.h > 0.0
                    && point_in_rect(mx, my, &cl.editor_v_scrollbar_thumb)
                {
                    self.set_hover(HoverTarget::CellEditorVScrollThumb(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.editor) {
                    self.set_hover(HoverTarget::CellEditor(cl.cell_index));
                    return;
                }
                if cl.output_toolbar.h > 0.0 && point_in_rect(mx, my, &cl.output_copy_button) {
                    self.set_hover(HoverTarget::CellOutputCopyButton(cl.cell_index));
                    return;
                }
                if cl.output_toggle.h > 0.0 && point_in_rect(mx, my, &cl.output_toggle) {
                    self.set_hover(HoverTarget::CellOutputToggle(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.resize_handle) {
                    self.set_hover(HoverTarget::CellResizeHandle(cl.cell_index));
                    return;
                }
            }
        }

        let rp = self.cached_layout.right_pane;
        if point_in_rect(mx, my, &rp) {
            self.render_area.mouse_zone = detect_mouse_zone(mx, my, &rp);
            self.set_hover(HoverTarget::RenderArea);
            return;
        }

        self.set_hover(HoverTarget::None);
    }

    pub(super) fn set_hover(&mut self, target: HoverTarget) {
        let zone_changed =
            target == HoverTarget::RenderArea && self.hover_target == HoverTarget::RenderArea;

        if self.hover_target != target || zone_changed {
            self.hover_target = target;
            let icon = match target {
                HoverTarget::SplitHandle => CursorIcon::ColResize,
                HoverTarget::CellResizeHandle(_) => CursorIcon::NsResize,
                HoverTarget::CellEditor(_) => CursorIcon::Text,
                HoverTarget::CellPlayButton(_)
                | HoverTarget::CellColorButton(_)
                | HoverTarget::CellCopyButton(_)
                | HoverTarget::CellOutputCopyButton(_)
                | HoverTarget::CellOutputToggle(_)
                | HoverTarget::AutocompleteItem(_)
                | HoverTarget::FeedbackButton
                | HoverTarget::ColorPickerSlider(_) => CursorIcon::Pointer,
                HoverTarget::RenderArea => match self.render_area.mouse_zone {
                    MouseZone::Center => {
                        if self.render_area.is_dragging {
                            CursorIcon::Grabbing
                        } else {
                            CursorIcon::Crosshair
                        }
                    }
                    MouseZone::XAxisEdge => CursorIcon::EwResize,
                    MouseZone::YAxisEdge => CursorIcon::NsResize,
                },
                HoverTarget::WindowEdge(dir) => CursorIcon::from(dir),
                _ => CursorIcon::Default,
            };
            self.window.set_cursor(icon);
            self.window.request_redraw();

            if let HoverTarget::MenuItem(i) = target {
                if self.open_menu != Some(i) {
                    self.open_menu(i);
                }
            }
        }
    }

    pub(super) fn open_menu(&mut self, index: usize) {
        self.dismiss_autocomplete();
        if self.open_menu == Some(index) {
            self.close_menu();
            return;
        }
        let menu_rect = self.menu_item_rects[index];
        let active_item = match index {
            THEME_MENU_INDEX => Some(active_theme_index()),
            FONTS_MENU_INDEX => Some(active_font_index()),
            _ => None,
        };
        self.dropdown_item_rects = self.renderer.open_dropdown(index, menu_rect, active_item);
        self.open_menu = Some(index);
        self.window.request_redraw();
    }

    pub(super) fn close_menu(&mut self) {
        if self.open_menu.is_some() {
            self.open_menu = None;
            self.dropdown_item_rects.clear();
            self.renderer.close_dropdown();
            self.window.request_redraw();
        }
    }

    /// Close the tab at `idx`. Handles GPU shader cleanup so a closed tab's
    /// pipelines can't keep rendering, and pending REDUCE work is discarded
    /// so we don't keep redrawing for a tab that no longer exists.
    pub(super) fn close_tab_at(&mut self, idx: usize) {
        if idx >= self.session.tabs.len() {
            return;
        }
        let was_active = idx == self.session.active_index;
        let closing_id = self.session.tabs[idx].tab_id;
        if was_active {
            // The active tab's pipelines live in the renderer's active list,
            // not in `stashed`, so dropping `stashed[id]` is not enough.
            self.renderer.clear_active_shaders();
            // Discard in-flight REDUCE responses too — they were destined
            // for cells that are about to be dropped.
            self.reduce_service.borrow_mut().clear_pending();
        } else {
            self.renderer.drop_stashed_tab_shaders(closing_id);
        }
        self.session.close_tab(idx);
        if was_active {
            let new_idx = self.session.active_index;
            let new_id = self.session.tabs[new_idx].tab_id;
            self.renderer.restore_tab_shaders(new_id);
            if let Some(tab) = self.session.tabs.get(new_idx) {
                if let Some([xmin, xmax, ymin, ymax]) = tab.axis_bounds {
                    self.render_area.axis_x_min = xmin;
                    self.render_area.axis_x_max = xmax;
                    self.render_area.axis_y_min = ymin;
                    self.render_area.axis_y_max = ymax;
                }
            }
            self.invalidate_lang_cache();
        }
        self.sync_active_tab();
    }

    /// Convenience for paths that always close the currently-active tab
    /// (Ctrl+W, File → Close Tab).
    pub(super) fn close_active_tab(&mut self) {
        self.close_tab_at(self.session.active_index);
    }

    /// Save current axis bounds to old tab, restore from new tab (or reset to default).
    /// Switch the active tab, persisting the previous tab's axis bounds so
    /// flipping back restores the same view. Also dismisses any color
    /// picker since its anchor cell belongs to the outgoing tab.
    pub(super) fn switch_tab_axis(&mut self, old_tab: usize, new_tab: usize) {
        self.close_color_picker();
        if let Some(tab) = self.session.tabs.get_mut(old_tab) {
            tab.axis_bounds = Some([
                self.render_area.axis_x_min,
                self.render_area.axis_x_max,
                self.render_area.axis_y_min,
                self.render_area.axis_y_max,
            ]);
        }
        if let Some(tab) = self.session.tabs.get(new_tab) {
            if let Some([xmin, xmax, ymin, ymax]) = tab.axis_bounds {
                self.render_area.axis_x_min = xmin;
                self.render_area.axis_x_max = xmax;
                self.render_area.axis_y_min = ymin;
                self.render_area.axis_y_max = ymax;
            } else {
                let default = RenderAreaState::default();
                self.render_area.axis_x_min = default.axis_x_min;
                self.render_area.axis_x_max = default.axis_x_max;
                self.render_area.axis_y_min = default.axis_y_min;
                self.render_area.axis_y_max = default.axis_y_max;
            }
        }
    }

    pub(super) fn dismiss_autocomplete(&mut self) {
        self.autocomplete.dismiss();
        self.autocomplete_item_rects.clear();
        self.renderer.close_autocomplete();
    }

    pub(super) fn update_autocomplete(&mut self) {
        let tab = self.session.active_tab();
        let cell = tab.active_cell();
        let text = cell.buffer.text();
        let cursor = cell.buffer.cursor_byte_offset();

        if let Some((prefix, prefix_start)) = autocomplete::prefix_at_cursor(text, cursor) {
            let min_len = if prefix.starts_with('\\') { 1 } else { 2 };
            if prefix.len() < min_len {
                self.dismiss_autocomplete();
                return;
            }

            let mut all = autocomplete::static_candidates();

            if prefix.starts_with('\\') {
                all.extend(autocomplete::symbol_candidates());
            }

            all.extend(self.cached_user_symbols.clone());

            self.autocomplete.update(prefix, prefix_start, &all);

            if self.autocomplete.active {
                let (cx, cy, ch) = self.renderer.cursor_content_pos();
                let active_idx = tab.active_cell_index;
                if let Some(cl) = self.cell_layouts.get(active_idx) {
                    let text_pad = crate::ui::theme::spacing::sm();
                    let popup_x = cl.editor.x + text_pad + cx;
                    let popup_y = cl.editor.y + text_pad + cy + ch;
                    let pane = self.cached_layout.left_pane;

                    let candidates: Vec<(String, crate::editor::autocomplete::CandidateKind)> =
                        self.autocomplete
                            .candidates
                            .iter()
                            .map(|c| {
                                let text = c.display.as_deref().unwrap_or(&c.label);
                                (text.to_string(), c.kind)
                            })
                            .collect();

                    self.autocomplete_item_rects = self.renderer.open_autocomplete(
                        popup_x,
                        popup_y,
                        &candidates,
                        self.autocomplete.selected_index,
                        pane,
                    );
                } else {
                    self.dismiss_autocomplete();
                }
            } else {
                self.dismiss_autocomplete();
            }
        } else {
            self.dismiss_autocomplete();
        }
    }

    pub(super) fn accept_autocomplete(&mut self) {
        if let Some((label, prefix_start)) = self.autocomplete.accept() {
            let label = label.to_string();
            let cursor = self
                .session
                .active_tab()
                .active_cell()
                .buffer
                .cursor_byte_offset();
            self.session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .replace_range(prefix_start, cursor, &label);
            self.session.active_tab_mut().mark_modified();
        }
        self.dismiss_autocomplete();
        self.sync_active_tab();
    }

    /// If the user just typed a non-identifier delimiter character and the
    /// LaTeX-style prefix immediately before it exactly matches a
    /// `LATEX_SYMBOLS` entry, replace that prefix with its Unicode
    /// substitution — so typing `\integral(` directly produces `∫(`. The
    /// delimiter character is preserved at the same logical cursor
    /// position. The actual matching logic lives in
    /// `autocomplete::latex_auto_substitute` so it's unit-testable as a
    /// pure function.
    pub(super) fn try_auto_complete_latex_command(&mut self) -> bool {
        let cell = self.session.active_tab().active_cell();
        let text = cell.buffer.text();
        let cursor = cell.buffer.cursor_byte_offset();
        let Some(sub) = autocomplete::latex_auto_substitute(text, cursor) else {
            return false;
        };
        self.session
            .active_tab_mut()
            .active_cell_mut()
            .buffer
            .replace_range(sub.start, sub.end, &sub.replacement);
        self.session.active_tab_mut().mark_modified();
        true
    }

    pub(super) fn handle_menu_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        menu_idx: usize,
        item_idx: usize,
    ) {
        self.close_menu();
        match (menu_idx, item_idx) {
            (0, 0) => {
                let old = self.session.active_index;
                let old_id = self.session.tabs[old].tab_id;
                self.renderer.stash_tab_shaders(old_id);
                let new_idx = self.session.new_tab();
                self.switch_tab_axis(old, new_idx);
                self.sync_active_tab();
            }
            (0, 1) => {
                if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Open));
                }
            }
            (0, 2) => {
                if self.session.active_tab().file_path.is_some() {
                    if let Err(e) = self.session.active_tab_mut().save() {
                        log::error!("Save failed: {}", e);
                    }
                    self.sync_active_tab();
                } else if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                }
            }
            (0, 3) => {
                if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                }
            }
            (0, 4) => {
                self.close_active_tab();
            }
            (0, 5) => {
                event_loop.exit();
            }
            (1, 0) => {
                self.do_cut();
            }
            (1, 1) => {
                self.do_copy();
            }
            (1, 2) => {
                self.do_paste();
            }
            (1, 3) => {
                self.do_select_all();
            }
            (2, 0) => {
                self.do_zoom(|_| fonts::zoom_in());
            }
            (2, 1) => {
                self.do_zoom(|_| fonts::zoom_out());
            }
            (2, 2) => {
                self.do_zoom(|_| fonts::reset_zoom());
            }
            (3, i) => {
                let Some(path) = example_path(i) else {
                    log::error!("Example index {i} out of range");
                    return;
                };
                let old = self.session.active_index;
                let old_id = self.session.tabs[old].tab_id;
                self.renderer.stash_tab_shaders(old_id);
                match self.session.open_file(&path) {
                    Ok(new_idx) => self.switch_tab_axis(old, new_idx),
                    Err(e) => {
                        log::error!("Failed to load example {}: {}", path.display(), e);
                        file_dialog::show_error(
                            "Cannot load example",
                            &format!("{}\n\n{}", path.display(), e),
                        );
                        return;
                    }
                }
                self.sync_active_tab();
            }
            (m, i) if m == THEME_MENU_INDEX => {
                self.select_theme(i);
            }
            (m, i) if m == FONTS_MENU_INDEX => {
                self.select_font(i);
            }
            (m, 0) if m == HELP_MENU_INDEX => {
                self.do_copy_diagnostics();
            }
            _ => {}
        }
    }

    /// Aggregate every cell diagnostic across every open tab into a plain-text
    /// report and place it on the system clipboard.
    pub(super) fn do_copy_diagnostics(&mut self) {
        let report = format_diagnostics_report(&self.session);
        if let Some(cb) = self.clipboard.as_mut() {
            if let Err(e) = cb.set_text(&report) {
                log::error!("Failed to copy diagnostics to clipboard: {e}");
            }
        }
    }

    /// Open the feedback URL in the user's default browser.
    pub(super) fn do_open_feedback_url(&self) {
        if let Err(e) = open_url(FEEDBACK_URL) {
            log::error!("Failed to open feedback URL {FEEDBACK_URL}: {e}");
        }
    }

    pub(super) fn do_cut(&mut self) {
        if let Some(text) = self
            .session
            .active_tab()
            .active_cell()
            .buffer
            .selected_text()
        {
            let owned = text.to_string();
            if let Some(cb) = self.clipboard.as_mut() {
                let _ = cb.set_text(&owned);
            }
            self.session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .backspace();
            self.session.active_tab_mut().mark_modified();
            self.sync_active_tab();
        }
    }

    pub(super) fn do_copy(&mut self) {
        if let Some(text) = self
            .session
            .active_tab()
            .active_cell()
            .buffer
            .selected_text()
        {
            if let Some(cb) = self.clipboard.as_mut() {
                let _ = cb.set_text(text);
            }
        }
    }

    pub(super) fn do_paste(&mut self) {
        if let Some(cb) = self.clipboard.as_mut() {
            if let Ok(text) = cb.get_text() {
                self.session
                    .active_tab_mut()
                    .active_cell_mut()
                    .buffer
                    .insert_text(&text);
                self.session.active_tab_mut().mark_modified();
                self.sync_active_tab();
            }
        }
    }

    pub(super) fn do_select_all(&mut self) {
        self.session
            .active_tab_mut()
            .active_cell_mut()
            .buffer
            .select_all();
        self.sync_active_tab();
    }

    pub(super) fn do_zoom(&mut self, f: impl FnOnce(&mut Self)) {
        f(self);
        self.renderer.rebuild_labels();
        self.layout.apply_scale();
        let size = self.window.inner_size();
        self.cached_layout = self.layout.compute(size.width as f32, size.height as f32);
        let wp = win_pos(&self.window);
        self.render_area
            .adjust_for_pane_change(&self.cached_layout.right_pane, wp, size.into());
        self.sync_active_tab();
    }

    pub(super) fn cycle_theme(&mut self) {
        let name = theme::cycle_theme();
        log::info!("Switched theme to: {}", name);
        self.renderer.invalidate_cell_texts();
        self.invalidate_lang_cache();
        self.sync_active_tab();
    }

    pub(super) fn select_theme(&mut self, index: usize) {
        theme::set_theme(index);
        log::info!("Selected theme: {}", theme::active_theme_name());
        self.renderer.invalidate_cell_texts();
        self.invalidate_lang_cache();
        self.sync_active_tab();
    }

    pub(super) fn select_font(&mut self, index: usize) {
        font_family::set(index);
        log::info!("Selected font: {}", font_family::active_name());
        self.renderer.invalidate_cell_fonts();
        self.renderer.rebuild_labels();
        self.window.request_redraw();
        self.sync_active_tab();
    }

    /// Clear cached user symbols so they are re-computed by the
    /// background lang service on the next sync.
    pub(super) fn invalidate_lang_cache(&mut self) {
        self.cached_user_symbols.clear();
        self.last_submitted_texts.clear();
        self.lang_service.clear_pending();
    }

    pub(super) fn handle_shortcut(&mut self, event_loop: &ActiveEventLoop, key: &Key) -> bool {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        if !ctrl {
            return false;
        }

        match key {
            Key::Character(c) if c.as_str() == "n" => {
                self.close_menu();
                let old = self.session.active_index;
                let old_id = self.session.tabs[old].tab_id;
                self.renderer.stash_tab_shaders(old_id);
                let new_idx = self.session.new_tab();
                self.switch_tab_axis(old, new_idx);
                self.sync_active_tab();
                true
            }
            Key::Character(c) if c.as_str() == "o" => {
                self.close_menu();
                if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Open));
                }
                true
            }
            Key::Character(c) if c.as_str() == "s" || c.as_str() == "S" => {
                self.close_menu();
                if shift {
                    if self.pending_dialog.is_none() {
                        self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                    }
                } else if self.session.active_tab().file_path.is_some() {
                    if let Err(e) = self.session.active_tab_mut().save() {
                        log::error!("Save failed: {}", e);
                        let path = self
                            .session
                            .active_tab()
                            .file_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        file_dialog::show_error(
                            "Cannot save file",
                            &format!("{}\n\n{}", path, e),
                        );
                    }
                    self.sync_active_tab();
                } else if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                }
                true
            }
            Key::Character(c) if c.as_str() == "w" => {
                self.close_menu();
                self.close_active_tab();
                true
            }
            Key::Character(c) if c.as_str() == "q" => {
                event_loop.exit();
                true
            }
            Key::Character(c) if c.as_str() == "a" => {
                self.close_menu();
                self.do_select_all();
                true
            }
            Key::Character(c) if c.as_str() == "c" => {
                self.do_copy();
                true
            }
            Key::Character(c) if c.as_str() == "x" => {
                self.do_cut();
                true
            }
            Key::Character(c) if c.as_str() == "v" => {
                self.do_paste();
                true
            }
            Key::Character(c) if c.as_str() == "=" || c.as_str() == "+" => {
                self.close_menu();
                self.do_zoom(|_| fonts::zoom_in());
                true
            }
            Key::Character(c) if c.as_str() == "-" => {
                self.close_menu();
                self.do_zoom(|_| fonts::zoom_out());
                true
            }
            Key::Character(c) if c.as_str() == "0" => {
                self.close_menu();
                self.do_zoom(|_| fonts::reset_zoom());
                true
            }
            Key::Character(c) if c.as_str() == "t" => {
                self.close_menu();
                self.cycle_theme();
                true
            }
            Key::Named(NamedKey::Enter) => {
                self.close_menu();
                let active = self.session.active_tab().active_cell_index;
                let is_playing =
                    self.session.active_tab().cell(active).state == CellState::Playing;
                if is_playing {
                    self.trigger_cell_stop(active);
                } else {
                    self.trigger_cell_play(active);
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn toggle_maximize(&mut self) {
        self.is_maximized = !self.is_maximized;
        self.window.set_maximized(self.is_maximized);
        self.renderer.set_maximized_icon(self.is_maximized);
    }

    /// Returns true if any drag operation is in progress.
    pub(super) fn is_any_drag_active(&self) -> bool {
        self.is_dragging_split
            || self.is_dragging_v_scroll
            || self.render_area.is_dragging
            || self.is_dragging_editor
            || self.is_dragging_cell_resize
            || self.is_dragging_cell_h_scroll
            || self.is_dragging_cell_v_scroll
    }

    /// Drive cell execution through the headless `Notebook` engine, then
    /// GPU-compile any emitted shader.
    pub(super) fn trigger_cell_play(&mut self, cell_index: usize) {
        self.session.active_tab_mut().notebook.play(cell_index);
        self.sync_cell_from_notebook(cell_index);
        self.sync_active_tab();
    }

    /// GPU-compile every shader the notebook just emitted for this cell
    /// (one per `plot(...)` call). The notebook owns `state` and
    /// `outcome.message`; this function only touches GPU side-effects and
    /// overrides the cell's state/message when shader compilation fails.
    pub(super) fn sync_cell_from_notebook(&mut self, cell_index: usize) {
        let cell = self.session.active_tab().cell(cell_index);
        if cell.outcome.shaders.is_empty() {
            return;
        }
        let cell_id = cell.id;
        let primary_color = cell.plot_color.to_f32_array();
        let sources: Vec<([f32; 4], &str)> = cell
            .outcome
            .shaders
            .iter()
            .map(|s| (primary_color, s.wgsl.as_str()))
            .collect();

        if let Err(e) = self.renderer.set_cell_shaders(cell_id, &sources) {
            log::error!("Shader compilation failed: {}", e);
            let source = self
                .session
                .active_tab()
                .notebook
                .combined_source(cell_index);
            let formatted = format_shader_error(&e.to_string(), &source);
            let cell = self.session.active_tab_mut().cell_mut(cell_index);
            cell.state = CellState::Idle;
            cell.outcome.message = Some(CellMessage::Error(formatted));
            cell.output_collapsed = false;
        }
    }

    /// Update the hover-out grace timer. While hover is on the picker or
    /// any color button, the timer is cleared. The first frame after hover
    /// leaves both, the timer is started; `tick_color_picker_timer` then
    /// closes the picker once `COLOR_PICKER_HOVER_GRACE` has elapsed.
    /// Drag-in-progress always cancels the timer so the popup stays.
    pub(super) fn maybe_close_color_picker_on_hover(&mut self) {
        if self.open_color_picker.is_none() {
            self.color_picker_hover_left_at = None;
            return;
        }
        if self.color_picker_drag.is_some() {
            self.color_picker_hover_left_at = None;
            return;
        }
        let inside_picker = matches!(
            self.hover_target,
            HoverTarget::ColorPickerArea | HoverTarget::ColorPickerSlider(_)
        );
        let on_color_button = matches!(self.hover_target, HoverTarget::CellColorButton(_));
        if inside_picker || on_color_button {
            self.color_picker_hover_left_at = None;
        } else if self.color_picker_hover_left_at.is_none() {
            self.color_picker_hover_left_at = Some(Instant::now());
        }
    }

    /// Drive the hover-out grace timer; close the picker once the grace
    /// period has elapsed. Returns `Some(deadline)` if the timer is still
    /// pending so the event loop can schedule a wakeup.
    pub(super) fn tick_color_picker_timer(&mut self) -> Option<Instant> {
        let started = self.color_picker_hover_left_at?;
        let deadline = started + super::COLOR_PICKER_HOVER_GRACE;
        if Instant::now() >= deadline {
            self.close_color_picker();
            self.color_picker_hover_left_at = None;
            None
        } else {
            Some(deadline)
        }
    }

    pub(super) fn close_color_picker(&mut self) {
        if self.open_color_picker.is_none() {
            return;
        }
        self.open_color_picker = None;
        self.color_picker_drag = None;
        self.color_picker_hover_left_at = None;
        self.renderer.clear_color_picker();
        self.window.request_redraw();
    }

    /// Set the indicated channel of the open picker's cell color from a
    /// vertical mouse position inside that channel's slider track. No-op
    /// if the picker is closed or the channel is out of range.
    pub(super) fn update_color_picker_value_at(&mut self, channel: u8, mouse_y: f32) {
        let Some(cell_index) = self.open_color_picker else {
            return;
        };
        let tracks = self.renderer.color_picker_slider_rects();
        let Some(track) = tracks.get(channel as usize).copied() else {
            return;
        };
        if track.h <= 0.0 {
            return;
        }
        // Sliders fill from the bottom: y = track.bottom → 0, y = track.top → 255.
        let t = ((track.y + track.h - mouse_y) / track.h).clamp(0.0, 1.0);
        let value = (t * 255.0).round() as u8;

        let tab = self.session.active_tab_mut();
        if cell_index >= tab.notebook.len() {
            return;
        }
        let cell = tab.notebook.cell_mut(cell_index);
        match channel {
            0 => cell.plot_color.r = value,
            1 => cell.plot_color.g = value,
            2 => cell.plot_color.b = value,
            3 => cell.plot_color.a = value,
            _ => return,
        }
        let cell_id = cell.id;
        let new_color = cell.plot_color.to_f32_array();
        tab.mark_modified();
        self.renderer.set_cell_primary_color(cell_id, new_color);
        self.sync_active_tab();
    }

    /// Step the cell's color seed by `delta` (positive = next, negative =
    /// previous), recompute its plot color, and push the new color to any
    /// already-running shaders without recompiling.
    pub(super) fn cycle_cell_color(&mut self, cell_index: usize, delta: i32) {
        let tab = self.session.active_tab_mut();
        if cell_index >= tab.notebook.len() {
            return;
        }
        let cell = tab.notebook.cell_mut(cell_index);
        cell.step_color(delta);
        let cell_id = cell.id;
        let new_color = cell.plot_color.to_f32_array();
        tab.mark_modified();
        self.renderer.set_cell_primary_color(cell_id, new_color);
        self.sync_active_tab();
    }

    /// Stop playing a cell's shader.
    pub(super) fn trigger_cell_stop(&mut self, cell_index: usize) {
        let cell_id = self.session.active_tab().cell(cell_index).id;
        self.renderer.remove_cell_shader(cell_id);
        self.session.active_tab_mut().notebook.stop(cell_index);
        let cell = self.session.active_tab_mut().cell_mut(cell_index);
        cell.outcome.message = None;
        self.sync_active_tab();
    }
}

fn severity_label(s: &Severity) -> &'static str {
    match s {
        Severity::Error => "error",
    }
}

fn format_diagnostics_report(session: &crate::session::Session) -> String {
    let mut total = 0usize;
    let mut body = String::new();
    for tab in &session.tabs {
        let diags = tab.notebook.diagnostics();
        if diags.is_empty() {
            continue;
        }
        let path_suffix = tab
            .file_path
            .as_ref()
            .map(|p| format!(" ({})", p.display()))
            .unwrap_or_default();
        body.push_str(&format!("[Tab: {}{}]\n", tab.name, path_suffix));
        for (cell_idx, d) in diags {
            total += 1;
            body.push_str(&format!(
                "  Cell {}, line {}:{} \u{2014} {}: {}\n",
                cell_idx + 1,
                d.span.start_line + 1,
                d.span.start_col + 1,
                severity_label(&d.severity),
                d.message,
            ));
        }
        body.push('\n');
    }

    let mut report = String::new();
    report.push_str("Logos diagnostics\n");
    report.push_str("=================\n");
    if total == 0 {
        report.push_str("No diagnostics. Everything looks clean!\n");
    } else {
        report.push_str(&format!(
            "Total: {} diagnostic(s) across {} tab(s)\n\n",
            total,
            session.tabs.len(),
        ));
        report.push_str(&body);
    }
    report
}

/// Open `url` in the user's default browser. Best-effort: errors on
/// unsupported platforms or missing handlers are returned to the caller.
fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = url;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "open_url not supported on this platform",
        ))
    }
}
