const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // ── DVUI with SDL3GPU backend ──────────────────────────────────────────
    const dvui_dep = b.dependency("dvui", .{
        .target = target,
        .optimize = optimize,
        .backend = .sdl3gpu,
    });

    // ── PCREz (Zig wrapper for PCRE2) ─────────────────────────────────
    const pcrez_dep = b.lazyDependency("pcrez", .{
        .target = target,
        .optimize = optimize,
    }) orelse @panic("PCREz dependency failed");

    const pcre2_dep = b.dependency("pcre2", .{
        .target = target,
        .optimize = optimize,
    });

    const pcrez_mod = b.addModule("pcrez", .{
        .root_source_file = pcrez_dep.path("src/regex.zig"),
        .target = target,
        .optimize = optimize,
    });
    pcrez_mod.linkLibrary(pcre2_dep.artifact("pcre2-8"));

    // ── Regex modules for lexer ──────────────────────────────────────────
    const regex_splitting_mod = b.createModule(.{
        .root_source_file = b.path("src/lang/regex_splitting.zig"),
        .target = target,
        .optimize = optimize,
    });
    regex_splitting_mod.addImport("pcrez", pcrez_mod);

    // ── Shader Compiler Dependencies ─────────────────────────────────────
    // Get shaderc from Zig package (builds from source)
    const shaderc_dep = b.dependency("shaderc_zig", .{
        .target = target,
        .optimize = optimize,
    });

    // Get SDL3 dependency for headers (lazy, from dvui's dependencies)
    const sdl3_dep = b.lazyDependency("sdl3", .{
        .target = target,
        .optimize = optimize,
    });

    // ── Main Executable ─────────────────────────────────────────────────────
    const exe = b.addExecutable(.{
        .name = "Logos",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
        }),
    });

    exe.linkLibC();
    exe.linkLibCpp(); // Required for shaderc C++ runtime

    // Link shaderc
    exe.linkLibrary(shaderc_dep.artifact("shaderc"));

    // Build and link SDL_shadercross with SDL3 headers
    if (sdl3_dep) |sdl3| {
        const sdl3_include_path = sdl3.path("include").getPath(b);

        const shadercross_dep = b.dependency("SDL_shadercross_zig", .{
            .target = target,
            .optimize = optimize,
            .shared = false,
            .dxc = false, // Disable DXC to simplify build
            .cli = false,
            .sdl_include_dir = @as([]const u8, sdl3_include_path),
        });
        exe.linkLibrary(shadercross_dep.artifact("SDL_shadercross"));

        // Add SDL3 headers for shader_compiler.zig @cImport
        exe.root_module.addIncludePath(sdl3.path("include"));
    }

    // DVUI imports
    exe.root_module.addImport("dvui", dvui_dep.module("dvui_sdl3gpu"));
    exe.root_module.addImport("sdl3gpu", dvui_dep.module("sdl3"));

    // PCREz imports
    exe.root_module.addImport("pcrez", pcrez_mod);
    exe.linkLibrary(pcre2_dep.artifact("pcre2-8"));

    // Regex module
    exe.root_module.addImport("regex_splitting", regex_splitting_mod);

    b.installArtifact(exe);

    // ── Run step ───────────────────────────────────────────────────────
    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());
    if (b.args) |args| run_cmd.addArgs(args);

    const run_step = b.step("run", "Run the app");
    run_step.dependOn(&run_cmd.step);

    // ── Bundled Zig compiler ───────────────────────────────────────────
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
    // Create regex_trie module
    const regex_trie_mod = b.createModule(.{
        .root_source_file = b.path("src/lang/regex_trie.zig"),
        .target = target,
        .optimize = optimize,
    });
    regex_trie_mod.addImport("pcrez", pcrez_mod);
    regex_trie_mod.addImport("regex_splitting", regex_splitting_mod);
    regex_trie_mod.linkLibrary(pcre2_dep.artifact("pcre2-8"));

    const regex_trie_test_mod = b.createModule(.{
        .root_source_file = b.path("src/lang/regex_trie_test.zig"),
        .target = target,
        .optimize = optimize,
    });
    regex_trie_test_mod.addImport("pcrez", pcrez_mod);
    regex_trie_test_mod.addImport("regex_splitting", regex_splitting_mod);
    regex_trie_test_mod.addImport("regex_trie", regex_trie_mod);

    const regex_trie_tests = b.addTest(.{
        .root_module = regex_trie_test_mod,
    });
    regex_trie_tests.linkLibC();
    regex_trie_tests.linkLibrary(pcre2_dep.artifact("pcre2-8"));

    const run_regex_trie_tests = b.addRunArtifact(regex_trie_tests);

    // Main test step - runs all tests
    const test_step = b.step("test", "Run all tests");
    test_step.dependOn(&run_regex_trie_tests.step);

    const run_test_live = b.addRunArtifact(regex_trie_tests);
    const test_live_step = b.step("test-live", "Run the test with live output");
    test_live_step.dependOn(&run_test_live.step);

    // ── Shader Generation Tests ──────────────────────────────────────────
    // Tests the shader pipeline: Logos expression → Tokens → AST → GLSL → SPIRV
    const shader_gen_test_mod = b.createModule(.{
        .root_source_file = b.path("src/shader_gen_test.zig"),
        .target = target,
        .optimize = optimize,
    });
    shader_gen_test_mod.addImport("pcrez", pcrez_mod);
    shader_gen_test_mod.addImport("regex_splitting", regex_splitting_mod);

    const shader_gen_test = b.addTest(.{
        .root_module = shader_gen_test_mod,
    });
    shader_gen_test.linkLibC();
    shader_gen_test.linkLibCpp();
    shader_gen_test.linkLibrary(pcre2_dep.artifact("pcre2-8"));
    shader_gen_test.linkLibrary(shaderc_dep.artifact("shaderc"));

    const run_shader_gen_test = b.addRunArtifact(shader_gen_test);

    // Add to main test step
    test_step.dependOn(&run_shader_gen_test.step);

    // Keep dedicated step for running just shader gen tests
    const shader_gen_test_step = b.step("test-shader-gen", "Run shader generation pipeline tests");
    shader_gen_test_step.dependOn(&run_shader_gen_test.step);

    // Also create an executable version for more detailed debugging
    const shader_gen_exe = b.addExecutable(.{
        .name = "shader_gen_test",
        .root_module = shader_gen_test_mod,
    });
    shader_gen_exe.linkLibC();
    shader_gen_exe.linkLibCpp();
    shader_gen_exe.linkLibrary(pcre2_dep.artifact("pcre2-8"));
    shader_gen_exe.linkLibrary(shaderc_dep.artifact("shaderc"));

    b.installArtifact(shader_gen_exe);

    const run_shader_gen_exe = b.addRunArtifact(shader_gen_exe);
    const run_shader_gen_step = b.step("run-shader-gen-test", "Run shader generation test executable");
    run_shader_gen_step.dependOn(&run_shader_gen_exe.step);

    // ── GPU Pipeline Test ────────────────────────────────────────────────
    // Tests actual GPU pipeline creation
    // Pipeline: Logos expression → Tokens → AST → GLSL → SPIRV → SDL Shader → GPU Pipeline
    const gpu_pipeline_test_mod = b.createModule(.{
        .root_source_file = b.path("src/gpu_pipeline_test.zig"),
        .target = target,
        .optimize = optimize,
    });
    gpu_pipeline_test_mod.addImport("pcrez", pcrez_mod);
    gpu_pipeline_test_mod.addImport("regex_splitting", regex_splitting_mod);

    const gpu_pipeline_exe = b.addExecutable(.{
        .name = "gpu_pipeline_test",
        .root_module = gpu_pipeline_test_mod,
    });
    gpu_pipeline_exe.linkLibC();
    gpu_pipeline_exe.linkLibCpp();
    gpu_pipeline_exe.linkLibrary(pcre2_dep.artifact("pcre2-8"));
    gpu_pipeline_exe.linkLibrary(shaderc_dep.artifact("shaderc"));

    // Link SDL_shadercross and SDL3 for GPU pipeline test
    if (sdl3_dep) |sdl3| {
        const sdl3_include_path = sdl3.path("include").getPath(b);

        const shadercross_dep = b.dependency("SDL_shadercross_zig", .{
            .target = target,
            .optimize = optimize,
            .shared = false,
            .dxc = false,
            .cli = false,
            .sdl_include_dir = @as([]const u8, sdl3_include_path),
        });
        gpu_pipeline_exe.linkLibrary(shadercross_dep.artifact("SDL_shadercross"));
        gpu_pipeline_exe.linkLibrary(sdl3.artifact("SDL3"));
        gpu_pipeline_exe.root_module.addIncludePath(sdl3.path("include"));
    }

    b.installArtifact(gpu_pipeline_exe);

    // Create test artifact (not just executable)
    const gpu_pipeline_test = b.addTest(.{
        .root_module = gpu_pipeline_test_mod,
    });
    gpu_pipeline_test.linkLibC();
    gpu_pipeline_test.linkLibCpp();
    gpu_pipeline_test.linkLibrary(pcre2_dep.artifact("pcre2-8"));
    gpu_pipeline_test.linkLibrary(shaderc_dep.artifact("shaderc"));

    if (sdl3_dep) |sdl3| {
        const sdl3_include_path = sdl3.path("include").getPath(b);

        const shadercross_dep = b.dependency("SDL_shadercross_zig", .{
            .target = target,
            .optimize = optimize,
            .shared = false,
            .dxc = false,
            .cli = false,
            .sdl_include_dir = @as([]const u8, sdl3_include_path),
        });
        gpu_pipeline_test.linkLibrary(shadercross_dep.artifact("SDL_shadercross"));
        gpu_pipeline_test.linkLibrary(sdl3.artifact("SDL3"));
        gpu_pipeline_test.root_module.addIncludePath(sdl3.path("include"));
    }

    const run_gpu_pipeline_test = b.addRunArtifact(gpu_pipeline_test);

    // Add to main test step
    test_step.dependOn(&run_gpu_pipeline_test.step);

    // Keep dedicated step and executable for manual testing
    const run_gpu_pipeline_exe = b.addRunArtifact(gpu_pipeline_exe);
    const gpu_pipeline_test_step = b.step("test-gpu-pipeline", "Run GPU pipeline test executable (requires display)");
    gpu_pipeline_test_step.dependOn(&run_gpu_pipeline_exe.step);
}
