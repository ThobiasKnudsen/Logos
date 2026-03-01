// reduce_ffi.cpp — Thin C wrappers for CSL functions with C++ linkage.

#include "cslbase/headers.h"
#include "cslbase/proc.h"
#include "reduce_ffi.h"

extern "C" {

void reduce_ffi_cslstart(int argc, const char *argv[],
                         csl_character_writer w)
{
    CSL_LISP::cslstart(argc, argv,
                       reinterpret_cast<CSL_LISP::character_writer *>(w));
}

int reduce_ffi_cslfinish(csl_character_writer w)
{
    return CSL_LISP::cslfinish(
        reinterpret_cast<CSL_LISP::character_writer *>(w));
}

int reduce_ffi_find_program_directory(const char *argv0)
{
    return CSL_LISP::find_program_directory(argv0);
}

int reduce_ffi_execute_lisp_function(const char *fname,
                                     csl_character_reader r,
                                     csl_character_writer w)
{
    return CSL_LISP::execute_lisp_function(
        fname,
        reinterpret_cast<CSL_LISP::character_reader *>(r),
        reinterpret_cast<CSL_LISP::character_writer *>(w));
}

} // extern "C"
