//! Raw extern "C" declarations for the CSL / REDUCE procedural interface.
//!
//! All functions go through try/catch wrappers in reduce_ffi.cpp to prevent
//! C++ exceptions from propagating into Rust (which would be a fatal abort).
//!
//! Safety: These functions are NOT thread-safe. They must only be called
//! from a single thread (the REDUCE worker thread).

#![allow(non_snake_case, dead_code)]

use std::os::raw::{c_char, c_int};

/// Function pointer types matching CSL's character_reader / character_writer.
pub type CharacterReader = Option<extern "C" fn() -> c_int>;
pub type CharacterWriter = Option<extern "C" fn(c_int) -> c_int>;

extern "C" {
    // --- All calls go through try/catch wrappers in reduce_ffi.cpp ---

    pub fn reduce_ffi_cslstart(argc: c_int, argv: *const *const c_char, w: CharacterWriter);

    pub fn reduce_ffi_cslfinish(w: CharacterWriter) -> c_int;

    pub fn reduce_ffi_find_program_directory(argv0: *const c_char) -> c_int;

    pub fn reduce_ffi_execute_lisp_function(
        fname: *const c_char,
        r: CharacterReader,
        w: CharacterWriter,
    ) -> c_int;

    pub fn reduce_ffi_process_statement(stmt: *const c_char) -> c_int;

    pub fn reduce_ffi_set_switch(name: *const c_char, val: c_int) -> c_int;

    pub fn reduce_ffi_gc_messages(n: c_int) -> c_int;

    pub fn reduce_ffi_prepare_for_top_level_loop() -> c_int;

    pub fn reduce_ffi_set_callbacks(r: CharacterReader, w: CharacterWriter) -> c_int;

    // --- PROC_* functions kept for stack-based API (already extern "C") ---

    pub fn PROC_clear_stack() -> c_int;
    pub fn PROC_push_symbol(name: *const c_char) -> c_int;
    pub fn PROC_push_string(data: *const c_char) -> c_int;
    pub fn PROC_push_small_integer(n: i32) -> c_int;
    pub fn PROC_push_big_integer(n: *const c_char) -> c_int;
    pub fn PROC_push_floating(n: f64) -> c_int;
    pub fn PROC_make_function_call(name: *const c_char, n: c_int) -> c_int;
    pub fn PROC_simplify() -> c_int;
    pub fn PROC_make_printable() -> c_int;
}
