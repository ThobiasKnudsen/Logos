const std = @import("std");
const builtin = @import("builtin");

// Custom error set for JIT operations
const JitError = error{
    WriteSourceFailed,
    CompileFailed,
    LibraryOpenFailed,
    SymbolNotFound,
    OutOfMemory,
    SourceAlreadyExists,
    SourceNotFound,
};

/// A loaded dynamic library produced by the JIT.
/// Owns the DynLib + copies of the source so you can safely overwrite/delete files later.
pub const JitDynLib = struct {
    dyn_lib: std.DynLib,
    src_code_copy: []const u8, // owned
    src_code_name: []const u8, // owned

    pub fn deinit(self: *JitDynLib, allocator: std.mem.Allocator) void {
        self.dyn_lib.close();
        allocator.free(self.src_code_copy);
        allocator.free(self.src_code_name);
    }

    /// Safe symbol lookup that ties the symbol lifetime to the JitDynLib
    pub fn getSymbol(self: *JitDynLib, comptime T: type, name: [:0]const u8) !T {
        return self.dyn_lib.lookup(T, name) orelse {
            std.debug.print("Error: symbol '{s}' not found in JIT module '{s}'\n", .{ name, self.src_code_name });
            return error.SymbolNotFound;
        };
    }

    pub fn reload(self: *JitDynLib, jit: *Jit, new_src: []const u8) !void {
        const new_lib = try jit.recompileAndLoad(new_src, self.src_code_name);
        self.dyn_lib.close();
        self.* = new_lib; // replaces everything safely
    }
};

pub const Jit = struct {
    allocator: std.mem.Allocator,
    zig_compiler_path: []const u8, // owned – can be "zig" (uses PATH) or full path
    temp_dir_path: []const u8, // owned, absolute path
    optimize: std.builtin.OptimizeMode = .ReleaseFast,

    pub fn init(allocator: std.mem.Allocator, zig_compiler_path: []const u8, temp_dir_path: []const u8) !Jit {

        // Resolve the Zig compiler path — this one MUST exist
        const abs_zig_compiler_path = try std.fs.cwd().realpathAlloc(allocator, zig_compiler_path);
        errdefer allocator.free(abs_zig_compiler_path);

        // Don't resolve temp_dir_path yet — it probably doesn't exist
        // Just duplicate the string as-is (or make it absolute manually if you want)
        const owned_temp_dir_path = try allocator.dupe(u8, temp_dir_path);
        errdefer allocator.free(owned_temp_dir_path);

        // Now clean + create the directory
        std.fs.cwd().deleteTree(owned_temp_dir_path) catch |err| {
            if (err != error.FileNotFound) return err;
        };
        try std.fs.cwd().makeDir(owned_temp_dir_path);

        // Now that it exists, you CAN resolve it if you really need the absolute path
        const abs_temp_dir_path = try std.fs.cwd().realpathAlloc(allocator, owned_temp_dir_path);
        errdefer allocator.free(abs_temp_dir_path);

        return Jit{
            .allocator = allocator,
            .zig_compiler_path = abs_zig_compiler_path,
            .temp_dir_path = abs_temp_dir_path, // now truly absolute and guaranteed to exist
            .optimize = .ReleaseFast,
        };
    }

    pub fn deinit(self: *Jit) void {
        // delete the temp folder. It should be created again on every new Jit.init function
        std.fs.cwd().deleteTree(self.temp_dir_path) catch {};
        self.allocator.free(self.zig_compiler_path);
        self.allocator.free(self.temp_dir_path);
    }

    // Public API
    pub fn compileAndLoad(self: *Jit, src_code: []const u8, src_name: []const u8) !JitDynLib {
        const src_path = try self.constructSourcePath(src_name);
        defer self.allocator.free(src_path);

        // Fail if source already exists (prevents accidental overwrites)
        if (std.fs.cwd().access(src_path, .{})) |_| {
            return error.SourceAlreadyExists;
        } else |err| switch (err) {
            error.FileNotFound => {}, // expected
            else => |e| return e,
        }

        return self.compileAndLoadInternal(src_code, src_name, src_path);
    }

    pub fn recompileAndLoad(self: *Jit, src_code: []const u8, src_name: []const u8) !JitDynLib {
        const src_path = try self.constructSourcePath(src_name);
        defer self.allocator.free(src_path);

        // For recompile we *require* the source file to already exist
        std.fs.cwd().access(src_path, .{}) catch |err| switch (err) {
            error.FileNotFound => return error.SourceNotFound,
            else => return err,
        };

        return self.compileAndLoadInternal(src_code, src_name, src_path);
    }

    // Internal helpers
    fn compileAndLoadInternal(
        self: *Jit,
        src_code: []const u8,
        src_name: []const u8,
        src_path: []const u8,
    ) !JitDynLib {
        // Delete any previous library (prevents loading stale .so on compile error)
        const lib_path = try self.constructLibraryPath(src_name);
        defer self.allocator.free(lib_path);

        std.fs.cwd().deleteFile(lib_path) catch |err| switch (err) {
            error.FileNotFound => {},
            else => |e| return e,
        };

        // Write source file
        self.writeFile(src_path, src_code) catch |e| {
            std.debug.print("Failed to write source '{s}': {}\n", .{ src_path, e });
            return error.WriteSourceFailed;
        };

        // Compile
        try self.runZigCompiler(src_path, lib_path, src_name);

        // Load
        const dyn_lib = std.DynLib.open(lib_path) catch |e| {
            std.debug.print("Failed to open JIT library '{s}': {}\n", .{ lib_path, e });
            return error.LibraryOpenFailed;
        };

        return JitDynLib{
            .dyn_lib = dyn_lib,
            .src_code_copy = try self.allocator.dupe(u8, src_code),
            .src_code_name = try self.allocator.dupe(u8, src_name),
        };
    }

    fn runZigCompiler(self: *Jit, src_path: []const u8, lib_path: []const u8, name: []const u8) !void {
        var arena = std.heap.ArenaAllocator.init(self.allocator);
        defer arena.deinit();
        const aa = arena.allocator();

        var args = try std.ArrayList([]const u8).initCapacity(aa, 20); // Pre-allocate ~20 slots; adjust if needed, or use .empty for 0
        errdefer args.deinit(aa); // Use errdefer since deinit now needs allocator

        try args.append(aa, self.zig_compiler_path);
        try args.append(aa, "build-lib");
        try args.append(aa, src_path);
        try args.append(aa, "-dynamic");
        try args.append(aa, switch (self.optimize) {
            .Debug => "-ODebug",
            .ReleaseSafe => "-OReleaseSafe",
            .ReleaseFast => "-OReleaseFast",
            .ReleaseSmall => "-OReleaseSmall",
        });
        try args.append(aa, "--name");
        try args.append(aa, name);
        try args.append(aa, "-fno-llvm");
        const emit_bin_lib_path = try std.fmt.allocPrint(aa, "-femit-bin={s}", .{lib_path});
        try args.append(aa, emit_bin_lib_path);
        try args.append(aa, "--cache-dir");
        try args.append(aa, self.temp_dir_path);

        var child = std.process.Child.init(args.items, aa);
        child.stderr_behavior = .Pipe;
        child.stdout_behavior = .Pipe;

        child.spawn() catch |spawn_err| {
            std.debug.print("Failed to spawn Zig compiler: {}\n", .{spawn_err});
            std.debug.print("Executing:", .{});
            for (args.items) |arg| {
                std.debug.print(" ", .{});
                // Quote arguments that contain spaces (optional but nice)
                if (std.mem.indexOf(u8, arg, " ")) |_| {
                    std.debug.print("\"{s}\"", .{arg});
                } else {
                    std.debug.print("{s}", .{arg});
                }
            }
            std.debug.print("\n", .{});
            return error.CompileFailed;
        };

        // Read output from both stderr and stdout before waiting
        var stdout_list = std.ArrayList(u8).empty;
        defer stdout_list.deinit(aa);
        var stderr_list = std.ArrayList(u8).empty;
        defer stderr_list.deinit(aa);

        // Use collectOutput to read from both pipes
        child.collectOutput(aa, &stdout_list, &stderr_list, 1024 * 1024) catch |err| {
            std.debug.print("Warning: Failed to collect output: {}\n", .{err});
        };

        // Extract the messages (memory is owned by arena, will be freed when arena is deinitialized)
        const stdout_msg = try stdout_list.toOwnedSlice(aa);
        const stderr_msg = try stderr_list.toOwnedSlice(aa);

        const term = child.wait() catch |wait_err| {
            std.debug.print("Failed to wait for Zig compiler: {}\n", .{wait_err});
            return error.CompileFailed;
        };

        if (term != .Exited or term.Exited != 0) {
            std.debug.print("Zig compiler failed with exit code: {}\n", .{if (term == .Exited) term.Exited else @as(i32, -1)});

            if (stderr_msg.len > 0) {
                std.debug.print("Zig compiler stderr:\n{s}\n", .{stderr_msg});
            }

            if (stdout_msg.len > 0) {
                std.debug.print("Zig compiler stdout:\n{s}\n", .{stdout_msg});
            }

            return error.CompileFailed;
        }
    }

    fn constructSourcePath(self: *Jit, name: []const u8) ![]u8 {
        const filename = try std.fmt.allocPrint(self.allocator, "{s}.zig", .{name});
        errdefer self.allocator.free(filename);
        return try std.fs.path.join(self.allocator, &.{ self.temp_dir_path, filename });
    }

    fn constructLibraryPath(self: *Jit, name: []const u8) ![]u8 {
        const prefix = if (builtin.os.tag == .windows) "" else "lib";
        const ext = switch (builtin.os.tag) {
            .windows => ".dll",
            .macos => ".dylib",
            else => ".so",
        };
        const filename = try std.fmt.allocPrint(self.allocator, "{s}{s}{s}", .{ prefix, name, ext });
        errdefer self.allocator.free(filename);
        return try std.fs.path.join(self.allocator, &.{ self.temp_dir_path, filename });
    }

    fn writeFile(self: *Jit, path: []const u8, data: []const u8) !void {
        _ = self;
        const dir = std.fs.path.dirname(path) orelse ".";
        try std.fs.cwd().makePath(dir);
        var file = try std.fs.cwd().createFile(path, .{ .truncate = true });
        defer file.close();
        try file.writeAll(data);
    }
};
