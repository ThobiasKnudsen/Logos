//! Session management module
//!
//! Handles user sessions (tabs) - each session contains:
//! - Text content the user has written
//! - Parsed/analyzed data from that content
//! - Render state for the graph visualization

pub const TabSession = @import("tab_session.zig").TabSession;
pub const SessionManager = @import("session_manager.zig").SessionManager;
