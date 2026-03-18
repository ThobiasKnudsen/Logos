// reduce_ffi.cpp — Thin C wrappers for CSL functions with C++ linkage.
// All wrappers use try/catch to prevent C++ exceptions from propagating
// through extern "C" into Rust (which would be a fatal abort).

#include "cslbase/headers.h"
#include "cslbase/proc.h"
#include "reduce_ffi.h"
#include <cstdio>

extern "C" {

void reduce_ffi_cslstart(int argc, const char *argv[],
                         csl_character_writer w)
{
    try {
        CSL_LISP::cslstart(argc, argv,
                           reinterpret_cast<CSL_LISP::character_writer *>(w));
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in cslstart\n");
    }
}

int reduce_ffi_cslfinish(csl_character_writer w)
{
    try {
        return CSL_LISP::cslfinish(
            reinterpret_cast<CSL_LISP::character_writer *>(w));
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in cslfinish\n");
        return -1;
    }
}

int reduce_ffi_find_program_directory(const char *argv0)
{
    try {
        return CSL_LISP::find_program_directory(argv0);
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in find_program_directory\n");
        return -1;
    }
}

int reduce_ffi_execute_lisp_function(const char *fname,
                                     csl_character_reader r,
                                     csl_character_writer w)
{
    try {
        return CSL_LISP::execute_lisp_function(
            fname,
            reinterpret_cast<CSL_LISP::character_reader *>(r),
            reinterpret_cast<CSL_LISP::character_writer *>(w));
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in execute_lisp_function\n");
        return -1;
    }
}

int reduce_ffi_process_statement(const char *stmt)
{
    try {
        return CSL_LISP::PROC_process_one_reduce_statement(stmt);
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in process_statement\n");
        return -1;
    }
}

int reduce_ffi_set_switch(const char *name, int val)
{
    try {
        return CSL_LISP::PROC_set_switch(name, val);
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in set_switch\n");
        return -1;
    }
}

int reduce_ffi_gc_messages(int n)
{
    try {
        return CSL_LISP::PROC_gc_messages(n);
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in gc_messages\n");
        return -1;
    }
}

int reduce_ffi_prepare_for_top_level_loop(void)
{
    try {
        return CSL_LISP::PROC_prepare_for_top_level_loop();
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in prepare_for_top_level_loop\n");
        return -1;
    }
}

int reduce_ffi_set_callbacks(csl_character_reader r, csl_character_writer w)
{
    try {
        return CSL_LISP::PROC_set_callbacks(
            reinterpret_cast<CSL_LISP::character_reader *>(r),
            reinterpret_cast<CSL_LISP::character_writer *>(w));
    } catch (...) {
        std::fprintf(stderr, "reduce_ffi: C++ exception in set_callbacks\n");
        return -1;
    }
}

} // extern "C"
