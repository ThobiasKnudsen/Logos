const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // ── Executable ─────────────────────────────────────────────────────
    const exe = b.addExecutable(.{
        .name = "Logos",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
        }),
    });

    exe.linkLibC();

    // ── DVUI with SDL3GPU backend ──────────────────────────────────────────
    const dvui_dep = b.dependency("dvui", .{
        .target = target,
        .optimize = optimize,
        .backend = .sdl3gpu, // Use SDL3 GPU API backend
    });
    exe.root_module.addImport("dvui", dvui_dep.module("dvui_sdl3gpu"));
    exe.root_module.addImport("sdl3gpu", dvui_dep.module("sdl3"));

    // ── PCREz (Zig wrapper for PCRE2) ─────────────────────────────────
    const pcrez_dep = b.lazyDependency("pcrez", .{
        .target = target,
        .optimize = optimize,
    }) orelse @panic("PCREz dependency failed");
    const pcrez_mod = b.addModule("pcrez", .{
        .root_source_file = pcrez_dep.path("src/regex.zig"),
        .target = target,
        .optimize = optimize,
    });
    exe.root_module.addImport("pcrez", pcrez_mod);

    // ── Run step ───────────────────────────────────────────────────────
    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());
    if (b.args) |args| run_cmd.addArgs(args);

    const run_step = b.step("run", "Run the app");
    run_step.dependOn(&run_cmd.step);

    // ── Bundled Zig compiler (unchanged) ───────────────────────────────
    const bundled_zig_dir = b.getInstallPath(.prefix, "tools/zig_compiler");
    const script_path = b.path("scripts/download_zig_compiler.sh");
    const download = b.addSystemCommand(&.{
        "/usr/bin/env",
        "bash",
        script_path.getPath(b),
        bundled_zig_dir,
    });
    b.getInstallStep().dependOn(&download.step);

    const zig_compiler_step = b.step("zig-compiler", "Download/extract bundled Zig master compiler");
    zig_compiler_step.dependOn(&download.step);

    // ── Tests ─────────────────────────────────────────────────────────────
    const regex_trie_test_mod = b.createModule(.{
        .root_source_file = b.path("src/ast/regex_trie_test.zig"),
        .target = target,
        .optimize = optimize,
    });
    regex_trie_test_mod.addImport("pcrez", pcrez_mod);
    regex_trie_test_mod.addImport("regex_splitting", b.createModule(.{
        .root_source_file = b.path("src/ast/regex_splitting.zig"),
        .target = target,
        .optimize = optimize,
    }));

    const regex_trie_tests = b.addTest(.{
        .root_module = regex_trie_test_mod,
    });
    regex_trie_tests.linkLibC();

    // Link PCRE2 library (required by PCREz)
    const pcre2_dep = b.dependency("pcre2", .{
        .target = target,
        .optimize = optimize,
    });
    regex_trie_tests.linkLibrary(pcre2_dep.artifact("pcre2-8"));

    const run_regex_trie_tests = b.addRunArtifact(regex_trie_tests);

    const test_step = b.step("test", "Run tests");
    test_step.dependOn(&run_regex_trie_tests.step);

    const run_test_live = b.addRunArtifact(regex_trie_tests);
    const test_live_step = b.step("test-live", "Run the test with live output");
    test_live_step.dependOn(&run_test_live.step);
}
