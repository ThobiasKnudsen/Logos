//! UI Views - full-screen compositions of components

const main_view = @import("main_view.zig");
pub const mainView = main_view.mainView;
pub const deinit = main_view.deinit;
pub const isDialogPending = main_view.isDialogPending;
