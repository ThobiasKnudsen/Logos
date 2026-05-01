# Vendored third-party code

This directory contains source code from third-party projects compiled into
the Logos binary via `build.rs`. Each subdirectory's license is summarised
below; full headers live inside the source files themselves.

## csl/

CSL (Codemist Standard Lisp) — the runtime that hosts the REDUCE computer
algebra system. Authored by Arthur C. Norman / Codemist.

License: **2-clause BSD** ("Codemist BSD"). See file headers, e.g.
`csl/cslbase/acnutil.h`. Copyright notice must be retained in source and
binary distributions.

`reduce_ffi.cpp` / `reduce_ffi.h` are Logos-authored thin wrappers that catch
C++ exceptions and expose a small C ABI to the Rust side; they're licensed
under the project's main LICENSE.

## reduce-packages/

REDUCE algebra system packages (factor, int, limit, matrix, misc, …) loaded
at runtime by CSL.

License: **2-clause BSD** (same Codemist BSD as CSL).

---

When adding a new vendored dependency, append an entry above with the
project name, attribution, and a one-line license summary, and ensure the
upstream license headers are preserved in the vendored files.
