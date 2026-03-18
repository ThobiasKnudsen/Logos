// reduce_ffi.h — Safe C wrappers for CSL/REDUCE functions.
// All functions catch C++ exceptions and return -1 on failure.

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

// Safe wrappers for PROC_* functions (catch C++ exceptions)
int reduce_ffi_process_statement(const char *stmt);
int reduce_ffi_set_switch(const char *name, int val);
int reduce_ffi_gc_messages(int n);
int reduce_ffi_prepare_for_top_level_loop(void);
int reduce_ffi_set_callbacks(csl_character_reader r, csl_character_writer w);

#ifdef __cplusplus
}
#endif

#endif // REDUCE_FFI_H
