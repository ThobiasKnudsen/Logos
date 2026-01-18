const std = @import("std");

// Simulate the shader on CPU for diagnostic purposes
pub fn main() !void {
    // Use debug.print instead of stdout for simplicity
    const print = std.debug.print;

    const sep = "=" ** 60;

    print("\n{s}\n", .{sep});
    print("CPU SHADER SIMULATION TEST\n", .{});
    print("Expression: x < y\n", .{});
    print("{s}\n\n", .{sep});

    // Shader uniforms (matching actual values from logs)
    const axis_min = [2]f32{ -5.0, -5.0 };
    const axis_max = [2]f32{ 5.0, 5.0 };
    const resolution = [2]f32{ 716.0, 762.0 };

    print("Uniforms:\n", .{});
    print("  axis_min = ({d:.3}, {d:.3})\n", .{ axis_min[0], axis_min[1] });
    print("  axis_max = ({d:.3}, {d:.3})\n", .{ axis_max[0], axis_max[1] });
    print("  resolution = ({d:.1}, {d:.1})\n\n", .{ resolution[0], resolution[1] });

    // Calculate pixel size in world coordinates
    const pixel_size = [2]f32{
        (axis_max[0] - axis_min[0]) / resolution[0],
        (axis_max[1] - axis_min[1]) / resolution[1],
    };
    const half_px = pixel_size[0] * 0.5;
    const half_py = pixel_size[1] * 0.5;

    print("Pixel size:\n", .{});
    print("  pixel_size = ({d:.6}, {d:.6})\n", .{ pixel_size[0], pixel_size[1] });
    print("  half_px = {d:.6}\n", .{half_px});
    print("  half_py = {d:.6}\n\n", .{half_py});

    // Test grid size
    const grid_size = 20;

    print("Testing {0}x{0} grid:\n\n", .{grid_size});

    // Sample detailed output for a few pixels
    const dash_sep2 = "-" ** 60;
    print("Detailed output for sample pixels:\n", .{});
    print("{s}\n", .{dash_sep2});

    const sample_pixels = [_][2]usize{
        .{ 0, 0 },           // Bottom-left
        .{ grid_size - 1, 0 }, // Bottom-right
        .{ 0, grid_size - 1 }, // Top-left
        .{ grid_size - 1, grid_size - 1 }, // Top-right
        .{ grid_size / 2, grid_size / 2 }, // Center
    };

    for (sample_pixels) |pixel| {
        const px = pixel[0];
        const py = pixel[1];

        // Convert pixel to UV (0 to 1)
        const u = @as(f32, @floatFromInt(px)) / @as(f32, @floatFromInt(grid_size - 1));
        const v = @as(f32, @floatFromInt(py)) / @as(f32, @floatFromInt(grid_size - 1));

        // Map UV to world coordinates
        const x = axis_min[0] + u * (axis_max[0] - axis_min[0]);
        const y = axis_min[1] + v * (axis_max[1] - axis_min[1]);

        // Calculate corner coordinates
        const x_m = x - half_px;
        const x_p = x + half_px;
        const y_m = y - half_py;
        const y_p = y + half_py;

        // Evaluate at corners (x < y)
        const c1 = x_m < y_m;
        const c2 = x_m < y_p;
        const c3 = x_p < y_m;
        const c4 = x_p < y_p;

        // Final result (all corners must agree for inequality)
        const result = c1 and c2 and c3 and c4;

        print("\nPixel ({}, {}) - uv=({d:.3}, {d:.3}):\n", .{ px, py, u, v });
        print("  world = ({d:.3}, {d:.3})\n", .{ x, y });
        print("  corners: x_m={d:.6}, x_p={d:.6}, y_m={d:.6}, y_p={d:.6}\n", .{ x_m, x_p, y_m, y_p });
        print("  corner evals: c1={}, c2={}, c3={}, c4={}\n", .{ c1, c2, c3, c4 });
        print("  RESULT = {} ({s})\n", .{ result, if (result) "GREEN" else "RED" });
    }

    const dash_sep = "-" ** 60;
    print("\n{s}\n\n", .{dash_sep});

    // Visual grid output
    print("Visual grid ({0}x{0}):\n", .{grid_size});
    print("  G = GREEN (x < y is true)\n", .{});
    print("  R = RED (x < y is false)\n\n", .{});

    // Count stats
    var green_count: usize = 0;
    var red_count: usize = 0;

    // Draw grid (top to bottom, matching screen coordinates)
    var row: usize = grid_size;
    while (row > 0) {
        row -= 1;
        const py = row;

        var px: usize = 0;
        while (px < grid_size) : (px += 1) {
            // Convert pixel to UV
            const u = @as(f32, @floatFromInt(px)) / @as(f32, @floatFromInt(grid_size - 1));
            const v = @as(f32, @floatFromInt(py)) / @as(f32, @floatFromInt(grid_size - 1));

            // Map UV to world coordinates
            const x = axis_min[0] + u * (axis_max[0] - axis_min[0]);
            const y = axis_min[1] + v * (axis_max[1] - axis_min[1]);

            // Calculate corners
            const x_m = x - half_px;
            const x_p = x + half_px;
            const y_m = y - half_py;
            const y_p = y + half_py;

            // Evaluate
            const result = (x_m < y_m) and (x_m < y_p) and (x_p < y_m) and (x_p < y_p);

            if (result) {
                print("G", .{});
                green_count += 1;
            } else {
                print("R", .{});
                red_count += 1;
            }
        }
        print("\n", .{});
    }

    print("\nStatistics:\n", .{});
    print("  GREEN pixels: {}\n", .{green_count});
    print("  RED pixels: {}\n", .{red_count});
    print("  Total: {}\n", .{green_count + red_count});

    const total = @as(f32, @floatFromInt(green_count + red_count));
    print("  GREEN percentage: {d:.1}%\n", .{@as(f32, @floatFromInt(green_count)) / total * 100.0});

    const sep_end = "=" ** 60;
    print("\n{s}\n", .{sep_end});
}
