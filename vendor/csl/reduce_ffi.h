// reduce_ffi.h — Thin C wrapper for CSL functions with C++ linkage.
// The PROC_* functions in proc.h are already extern "C", so Rust can call
// them directly.  Only cslstart / cslfinish / find_program_directory /
// execute_lisp_function live in namespace CSL_LISP with C++ linkage and
// therefore need these wrappers.

#ifndef REDUCE_FFI_H
#define REDUCE_FFI_H

#ifdef __cplusplus
extern "C" {
#endif

typedef int (*csl_character_reader)(void);
typedef int (*csl_character_writer)(int);

void reduce_ffi_cslstart(int argc, const char *argv[],
                         csl_character_writer w);

int reduce_ffi_cslfinish(csl_character_writer w);

int reduce_ffi_find_program_directory(const char *argv0);

int reduce_ffi_execute_lisp_function(const char *fname,
                                     csl_character_reader r,
                                     csl_character_writer w);

#ifdef __cplusplus
}
#endif

#endif // REDUCE_FFI_H
