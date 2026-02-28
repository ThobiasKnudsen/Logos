/// Taffy-based UI layout for the application shell.
///
/// Computes pixel-perfect rectangles each frame for:
///   menu_bar, tab_bar, left_pane (editor), split_handle, right_pane (graph), status_bar
///
/// Layout structure (vertical flex):
/// ```text
/// root (column)
/// +-- menu_bar     (height: 28px)
/// +-- tab_bar      (height: 36px)
/// +-- content_area (flex: 1, row)
/// |   +-- left_pane    (width: 45%)
/// |   +-- split_handle (width: 6px)
/// |   +-- right_pane   (flex: 1)
/// +-- status_bar   (height: 24px)
/// ```

use crate::ui::theme::spacing;
use taffy::prelude::*;

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutResult {
    pub menu_bar: Rect,
    pub tab_bar: Rect,
    pub left_pane: Rect,
    pub split_handle: Rect,
    pub right_pane: Rect,
    pub status_bar: Rect,
}

pub struct UiLayout {
    tree: TaffyTree,
    root: NodeId,
    menu_bar: NodeId,
    tab_bar: NodeId,
    content_area: NodeId,
    left_pane: NodeId,
    split_handle: NodeId,
    right_pane: NodeId,
    status_bar: NodeId,
}

impl UiLayout {
    pub fn new() -> Self {
        let mut tree = TaffyTree::new();

        let menu_bar = tree
            .new_leaf(Style {
                size: Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(spacing::MENU_HEIGHT),
                },
                ..Default::default()
            })
            .unwrap();

        let tab_bar = tree
            .new_leaf(Style {
                size: Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(spacing::TAB_HEIGHT),
                },
                ..Default::default()
            })
            .unwrap();

        let left_pane = tree
            .new_leaf(Style {
                size: Size {
                    width: Dimension::Percent(0.45),
                    height: auto(),
                },
                flex_shrink: 0.0,
                ..Default::default()
            })
            .unwrap();

        let split_handle = tree
            .new_leaf(Style {
                size: Size {
                    width: Dimension::Length(spacing::SPLIT_HANDLE_WIDTH),
                    height: auto(),
                },
                flex_shrink: 0.0,
                ..Default::default()
            })
            .unwrap();

        let right_pane = tree
            .new_leaf(Style {
                flex_grow: 1.0,
                ..Default::default()
            })
            .unwrap();

        let content_area = tree
            .new_with_children(
                Style {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    size: Size {
                        width: Dimension::Percent(1.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                &[left_pane, split_handle, right_pane],
            )
            .unwrap();

        let status_bar = tree
            .new_leaf(Style {
                size: Size {
                    width: Dimension::Percent(1.0),
                    height: Dimension::Length(spacing::STATUS_HEIGHT),
                },
                ..Default::default()
            })
            .unwrap();

        let root = tree
            .new_with_children(
                Style {
                    flex_direction: FlexDirection::Column,
                    size: Size {
                        width: Dimension::Percent(1.0),
                        height: Dimension::Percent(1.0),
                    },
                    ..Default::default()
                },
                &[menu_bar, tab_bar, content_area, status_bar],
            )
            .unwrap();

        Self {
            tree,
            root,
            menu_bar,
            tab_bar,
            content_area,
            left_pane,
            split_handle,
            right_pane,
            status_bar,
        }
    }

    /// Recompute layout for the given viewport size and return pixel rects.
    pub fn compute(&mut self, width: f32, height: f32) -> LayoutResult {
        self.tree
            .compute_layout(
                self.root,
                Size {
                    width: AvailableSpace::Definite(width),
                    height: AvailableSpace::Definite(height),
                },
            )
            .unwrap();

        let root_layout = self.tree.layout(self.root).unwrap();
        let root_x = root_layout.location.x;
        let root_y = root_layout.location.y;

        let resolve = |node: NodeId, parent_x: f32, parent_y: f32| -> Rect {
            let l = self.tree.layout(node).unwrap();
            Rect {
                x: parent_x + l.location.x,
                y: parent_y + l.location.y,
                w: l.size.width,
                h: l.size.height,
            }
        };

        let menu_bar = resolve(self.menu_bar, root_x, root_y);
        let tab_bar = resolve(self.tab_bar, root_x, root_y);
        let content = resolve(self.content_area, root_x, root_y);
        let left_pane = resolve(self.left_pane, content.x, content.y);
        let split_handle = resolve(self.split_handle, content.x, content.y);
        let right_pane = resolve(self.right_pane, content.x, content.y);
        let status_bar = resolve(self.status_bar, root_x, root_y);

        LayoutResult {
            menu_bar,
            tab_bar,
            left_pane,
            split_handle,
            right_pane,
            status_bar,
        }
    }
}
