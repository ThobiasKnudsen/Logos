const std = @import("std");
const builtin = @import("builtin");

const App = @import("app.zig").App;

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var app = try App.init(allocator);
    defer app.deinit();

    try app.run();
}

// Keep JIT functionality available as a separate entry point
pub fn runJitDemo() !void {
    const jit_mod = @import("jit/jit.zig");
    const allocator = std.heap.page_allocator;

    const zig_exe = "zig-out/tools/zig_compiler/zig";
    const temp_dir = "zig-out/tools/jit_temp";

    const src_code =
        \\const std = @import("std");
        \\
        \\pub export fn add(a: i32, b: i32) i32 {
        \\    return a + b;
        \\}
        \\
        \\pub export fn sub(a: i32, b: i32) i32 {
        \\    return a - b;
        \\}
        \\
        \\pub export fn long_loop() f64 {
        \\    var ret_val: f64 = 0.0;
        \\    var prng = std.Random.DefaultPrng.init(1);
        \\    for (0..10_000_000) |_| {
        \\        ret_val += prng.random().float(f64);
        \\    }
        \\    return ret_val;
        \\}
    ;

    const lib_name = "temp_module_1";

    var jit = try jit_mod.Jit.init(allocator, zig_exe, temp_dir);
    defer jit.deinit();

    var jit_lib = try jit.compileAndLoad(src_code, lib_name);
    defer jit_lib.deinit(allocator);

    const TypeFn1 = *const fn (i32, i32) callconv(.c) i32;
    const add_func = try jit_lib.getSymbol(TypeFn1, "add");
    const sub_func = try jit_lib.getSymbol(TypeFn1, "sub");
    const TypeFn2 = *const fn () callconv(.c) f64;
    const loop_func = try jit_lib.getSymbol(TypeFn2, "long_loop");

    const result_1 = add_func(5, 10);
    std.debug.print("JIT result add: {}\n", .{result_1});
    const result_2 = sub_func(5, 10);
    std.debug.print("JIT result sub: {}\n", .{result_2});
    const result_3 = loop_func();
    std.debug.print("JIT result loop: {}\n", .{result_3});
}
