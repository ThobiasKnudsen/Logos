//! Code Cell - A single executable unit in the notebook interface
//!
//! Each cell contains code, execution state, and output (text and/or plot).
//! Cells have per-cell play/stop controls and validation state.

const std = @import("std");
const glsl_gen = @import("../lang/glsl_gen.zig");

const Allocator = std.mem.Allocator;

/// Output type for a cell
pub const OutputType = enum {
    none, // No output yet
    text_only, // Scalar result, just print
    plot_only, // Contains axis vars, just plot
    both, // Print value and plot
    err, // Execution error
};

/// Cell output result
pub const CellOutput = struct {
    /// Printed/evaluated result text (stored for reload)
    text: ?[]const u8,
    /// GLSL shader if plottable
    shader: ?glsl_gen.GeneratedShader,
    /// How to display this output
    output_type: OutputType,
    /// Error message if execution failed
    error_msg: ?[]const u8,

    pub fn deinit(self: *CellOutput, allocator: Allocator) void {
        if (self.text) |t| {
            allocator.free(t);
        }
        if (self.shader) |s| {
            allocator.free(s.source);
        }
        if (self.error_msg) |e| {
            allocator.free(e);
        }
    }
};

/// A single code cell in the notebook
pub const CodeCell = struct {
    /// Unique ID for dvui widget IDs
    id: usize,
    /// Code content for this cell
    content: std.ArrayList(u8),
    /// Execution result (null if not executed yet)
    output: ?CellOutput,
    /// RGBA color for this cell's plot (0-255 range)
    color: [4]u8,
    /// Whether this cell is actively playing (rendering its shader)
    is_playing: bool,
    /// Whether this cell has a validation error
    has_validation_error: bool,
    /// Validation error message (if any)
    validation_error: ?[]const u8,
    /// Cursor position within this cell (byte index)
    cursor_index: usize,

    /// Initialize a new empty cell
    pub fn init(_: Allocator, id: usize) CodeCell {
        return .{
            .id = id,
            .content = .{ .items = &.{}, .capacity = 0 },
            .output = null,
            .color = autoAssignColor(id),
            .is_playing = false,
            .has_validation_error = false,
            .validation_error = null,
            .cursor_index = 0,
        };
    }

    /// Initialize a cell with existing content
    pub fn initWithContent(allocator: Allocator, id: usize, initial_content: []const u8) !CodeCell {
        var cell = CodeCell.init(allocator, id);
        try cell.content.appendSlice(allocator, initial_content);
        return cell;
    }

    pub fn deinit(self: *CodeCell, allocator: Allocator) void {
        self.content.deinit(allocator);
        if (self.output) |*out| {
            out.deinit(allocator);
        }
        if (self.validation_error) |err| {
            allocator.free(err);
            self.validation_error = null;
        }
    }

    /// Set custom color for this cell's plot (RGB, keeps existing alpha)
    pub fn setColor(self: *CodeCell, rgb: [3]u8) void {
        self.color[0] = rgb[0];
        self.color[1] = rgb[1];
        self.color[2] = rgb[2];
        // Keep existing alpha
    }

    /// Set full RGBA color for this cell's plot
    pub fn setColorRGBA(self: *CodeCell, rgba: [4]u8) void {
        self.color = rgba;
    }

    /// Set color from normalized float values (0.0-1.0 range)
    pub fn setColorFromFloat(self: *CodeCell, r: f32, g: f32, b: f32, a: f32) void {
        self.color = .{
            @intFromFloat(@max(0.0, @min(1.0, r)) * 255.0),
            @intFromFloat(@max(0.0, @min(1.0, g)) * 255.0),
            @intFromFloat(@max(0.0, @min(1.0, b)) * 255.0),
            @intFromFloat(@max(0.0, @min(1.0, a)) * 255.0),
        };
    }

    /// Get color as normalized vec4 (for shader uniforms)
    pub fn getColorVec4(self: *const CodeCell) [4]f32 {
        return .{
            @as(f32, @floatFromInt(self.color[0])) / 255.0,
            @as(f32, @floatFromInt(self.color[1])) / 255.0,
            @as(f32, @floatFromInt(self.color[2])) / 255.0,
            @as(f32, @floatFromInt(self.color[3])) / 255.0,
        };
    }

    /// Get color as hex string for JSON serialization (includes alpha)
    pub fn getColorHex(self: *const CodeCell, allocator: Allocator) ![]const u8 {
        return std.fmt.allocPrint(allocator, "#{x:0>2}{x:0>2}{x:0>2}{x:0>2}", .{
            self.color[0],
            self.color[1],
            self.color[2],
            self.color[3],
        });
    }

    /// Set color from hex string (supports both #RRGGBB and #RRGGBBAA)
    pub fn setColorFromHex(self: *CodeCell, hex: []const u8) !void {
        if (hex.len != 7 and hex.len != 9) {
            return error.InvalidHexColor;
        }
        if (hex[0] != '#') {
            return error.InvalidHexColor;
        }
        self.color[0] = try std.fmt.parseInt(u8, hex[1..3], 16);
        self.color[1] = try std.fmt.parseInt(u8, hex[3..5], 16);
        self.color[2] = try std.fmt.parseInt(u8, hex[5..7], 16);
        if (hex.len == 9) {
            self.color[3] = try std.fmt.parseInt(u8, hex[7..9], 16);
        } else {
            self.color[3] = 255; // Default to fully opaque
        }
    }

    /// Auto-assign a color from a palette based on cell ID
    fn autoAssignColor(id: usize) [4]u8 {
        const palette = [_][4]u8{
            .{ 255, 85, 0, 255 }, // Orange
            .{ 0, 170, 255, 255 }, // Blue
            .{ 255, 0, 0, 255 }, // Red
            .{ 0, 255, 127, 255 }, // Spring green
            .{ 255, 0, 255, 255 }, // Magenta
            .{ 255, 255, 0, 255 }, // Yellow
            .{ 0, 255, 255, 255 }, // Cyan
            .{ 170, 85, 255, 255 }, // Purple
        };
        return palette[id % palette.len];
    }
};
