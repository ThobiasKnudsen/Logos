//! SDL3 texture render target wrapper
//!
//! Wraps dvui's TextureTarget for custom rendering.
//! The texture can be drawn to and then displayed in dvui widgets.

const std = @import("std");
const dvui = @import("dvui");

pub const TextureTarget = struct {
    /// The dvui texture target (wraps SDL3 texture internally)
    texture: ?dvui.TextureTarget,

    /// dvui backend reference
    backend: dvui.Backend,

    /// Current dimensions
    width: u32,
    height: u32,

    pub fn init(backend: dvui.Backend) TextureTarget {
        return .{
            .texture = null,
            .backend = backend,
            .width = 0,
            .height = 0,
        };
    }

    pub fn deinit(self: *TextureTarget) void {
        if (self.texture) |tex| {
            self.backend.textureDestroy(tex.texture());
        }
        self.texture = null;
    }

    /// Ensure texture exists and is the correct size
    pub fn ensureSize(self: *TextureTarget, width: u32, height: u32) !void {
        if (width == 0 or height == 0) return;

        // Recreate texture if size changed
        if (self.texture == null or self.width != width or self.height != height) {
            if (self.texture) |tex| {
                self.backend.textureDestroy(tex.texture());
            }

            self.texture = try self.backend.textureCreateTarget(
                width,
                height,
                .linear,
            );

            self.width = width;
            self.height = height;
        }
    }

    /// Begin rendering to this texture
    pub fn beginRender(self: *TextureTarget) !void {
        if (self.texture) |tex| {
            try self.backend.renderTarget(tex);
        } else {
            return error.NoTexture;
        }
    }

    /// End rendering to this texture, restore default target
    pub fn endRender(self: *TextureTarget) void {
        self.backend.renderTarget(null) catch {};
    }

    /// Get the texture for display in dvui
    pub fn getTexture(self: *TextureTarget) ?dvui.Texture {
        if (self.texture) |tex| {
            return self.backend.textureFromTarget(tex) catch null;
        }
        return null;
    }
};
