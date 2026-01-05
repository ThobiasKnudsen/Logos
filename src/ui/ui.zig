//! UI module - exports all UI components and views
//!
//! Structure:
//!   ui/
//!   ├── components/   Reusable UI widgets (menu, tabs, editor, split)
//!   ├── views/        Full-screen views composed of components
//!   └── theme.zig     Visual styling constants

pub const components = @import("components/components.zig");
pub const views = @import("views/views.zig");
pub const theme = @import("theme.zig");
