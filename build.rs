use std::env;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let mut build = cc::Build::new();

    build
        .cpp(true)
        .warnings(false)
        // Include paths
        .include("vendor/csl/include")   // config.h
        .include("vendor/csl/cslbase")   // all CSL headers + ops/
        .include("vendor/csl")           // for reduce_ffi.h
        // Preprocessor defines
        .define("HAVE_CONFIG_H", "1")
        .define("EMBEDDED", "1")
        .define("BUILTIN_IMAGE", "1")
        .define("NO_BYTECOUNTS", "1");

    // MSVC uses /std:c++17 (set by cc crate from "c++17")
    // GCC/Clang use -std=gnu++17 for GNU extensions
    if target_env == "msvc" {
        build.std("c++17");
        // CSL code checks `WIN32` (no underscore) but MSVC only defines `_WIN32`
        build.define("WIN32", "1");
        // Prevent windows.h from defining min/max macros (breaks std::min/std::max)
        build.define("NOMINMAX", "1");
        // Enable C++ exception handling (suppresses C4530 warnings)
        build.flag("/EHsc");
    } else {
        build.std("gnu++17");
    }

    // Core CSL source files
    let csl_sources = [
        "vendor/csl/cslbase/newallocate.cpp",
        "vendor/csl/cslbase/arith01.cpp",
        "vendor/csl/cslbase/arith02.cpp",
        "vendor/csl/cslbase/arith03.cpp",
        "vendor/csl/cslbase/arith04.cpp",
        "vendor/csl/cslbase/arith05.cpp",
        "vendor/csl/cslbase/arith06.cpp",
        "vendor/csl/cslbase/arith07.cpp",
        "vendor/csl/cslbase/arith08.cpp",
        "vendor/csl/cslbase/arith09.cpp",
        "vendor/csl/cslbase/arith10.cpp",
        "vendor/csl/cslbase/arith11.cpp",
        "vendor/csl/cslbase/arith12.cpp",
        "vendor/csl/cslbase/arith13.cpp",
        "vendor/csl/cslbase/arith14.cpp",
        "vendor/csl/cslbase/bytes1.cpp",
        "vendor/csl/cslbase/char.cpp",
        "vendor/csl/cslbase/embedcsl.cpp",
        "vendor/csl/cslbase/cslmpi.cpp",
        "vendor/csl/cslbase/cslread.cpp",
        "vendor/csl/cslbase/eval1.cpp",
        "vendor/csl/cslbase/eval2.cpp",
        "vendor/csl/cslbase/eval3.cpp",
        "vendor/csl/cslbase/eval4.cpp",
        "vendor/csl/cslbase/fasl.cpp",
        "vendor/csl/cslbase/fns1.cpp",
        "vendor/csl/cslbase/fns2.cpp",
        "vendor/csl/cslbase/fns3.cpp",
        "vendor/csl/cslbase/fwin.cpp",
        "vendor/csl/cslbase/newcslgc.cpp",
        "vendor/csl/cslbase/lisphash.cpp",
        "vendor/csl/cslbase/isprime.cpp",
        "vendor/csl/cslbase/preserve.cpp",
        "vendor/csl/cslbase/print.cpp",
        "vendor/csl/cslbase/restart.cpp",
        "vendor/csl/cslbase/sysfwin.cpp",
        "vendor/csl/cslbase/termed.cpp",
        "vendor/csl/cslbase/inthash.cpp",
        "vendor/csl/cslbase/serialize.cpp",
        "vendor/csl/cslbase/stubs.cpp",
        "vendor/csl/cslbase/showhdr.cpp",
        "vendor/csl/cslbase/forks.cpp",
        "vendor/csl/cslbase/gc-check.cpp",
        "vendor/csl/cslbase/jit.cpp",
        "vendor/csl/cslbase/qsieve.cpp",
    ];

    for src in &csl_sources {
        build.file(src);
    }

    // Additional files
    build.file("vendor/csl/machineid.cpp");
    build.file("vendor/csl/reduce_ffi.cpp");

    build.compile("csl");

    // Link libraries AFTER compiling CSL (link order matters)
    if target_os == "linux" {
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=stdc++");
    } else if target_os == "macos" {
        println!("cargo:rustc-link-lib=c++");
    }
    // MSVC links the C++ runtime automatically

    if target_env != "msvc" {
        println!("cargo:rustc-link-lib=z");
        println!("cargo:rustc-link-lib=m");
    } else {
        // Windows MSVC: link zlib (installed via vcpkg or similar)
        println!("cargo:rustc-link-lib=zlib");
    }

    // Tell cargo to re-run if vendor files change
    println!("cargo:rerun-if-changed=vendor/csl/");
    println!("cargo:rerun-if-changed=build.rs");
}
