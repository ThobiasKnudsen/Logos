//! Safe wrapper around the CSL/REDUCE FFI.
//!
//! `ReduceSession` owns a single CSL instance. It must only be used from
//! one thread (CSL is not thread-safe). The background service in
//! `service.rs` ensures this by running all calls on a dedicated thread.

use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_int;

use super::ffi;

/// Thread-local buffers for CSL I/O callbacks.
/// CSL communicates via character-at-a-time callbacks. We use thread-local
/// storage so that the static `extern "C"` callback functions can access
/// the input/output buffers without any synchronization.
thread_local! {
    static INPUT_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    static INPUT_POS: RefCell<usize> = RefCell::new(0);
    static OUTPUT_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(4096));
}

/// Callback: CSL reads one character of input.
extern "C" fn reader_callback() -> c_int {
    INPUT_BUF.with(|buf| {
        INPUT_POS.with(|pos| {
            let buf = buf.borrow();
            let mut pos = pos.borrow_mut();
            if *pos < buf.len() {
                let ch = buf[*pos] as c_int;
                *pos += 1;
                ch
            } else {
                // EOF
                -1
            }
        })
    })
}

/// Callback: CSL writes one character of output.
extern "C" fn writer_callback(ch: c_int) -> c_int {
    if ch >= 0 {
        OUTPUT_BUF.with(|buf| {
            buf.borrow_mut().push(ch as u8);
        });
    }
    0
}

/// A single REDUCE/CSL session. NOT Send or Sync — must stay on one thread.
pub struct ReduceSession {
    _not_send: std::marker::PhantomData<*mut ()>,
}

impl ReduceSession {
    /// Initialize CSL and load the REDUCE image.
    ///
    /// This is expensive (~50-200ms). Call once, reuse for all simplifications.
    pub fn new() -> Result<Self, String> {
        // CSL needs argv[0] to locate things. With BUILTIN_IMAGE the path
        // doesn't matter much, but it must be a valid C string.
        let argv0 = CString::new("logos-reduce").unwrap();
        let quiet = CString::new("-w").unwrap();
        let argv_ptrs: Vec<*const i8> = vec![argv0.as_ptr(), quiet.as_ptr()];

        unsafe {
            ffi::reduce_ffi_cslstart(
                argv_ptrs.len() as c_int,
                argv_ptrs.as_ptr(),
                Some(writer_callback),
            );
        }

        // Drain any startup output
        OUTPUT_BUF.with(|buf| buf.borrow_mut().clear());

        // Prepare the procedural top-level loop
        let rc = unsafe { ffi::PROC_prepare_for_top_level_loop() };
        if rc != 0 {
            return Err(format!("PROC_prepare_for_top_level_loop failed: {}", rc));
        }

        // Set callbacks for future I/O
        unsafe {
            ffi::PROC_set_callbacks(Some(reader_callback), Some(writer_callback));
        }

        // Suppress GC messages
        unsafe {
            ffi::PROC_gc_messages(0);
        }

        // Drain any remaining startup output
        OUTPUT_BUF.with(|buf| buf.borrow_mut().clear());

        Ok(ReduceSession {
            _not_send: std::marker::PhantomData,
        })
    }

    /// Send a REDUCE statement and return the textual output.
    ///
    /// The statement should end with `;` (print result) or `$` (suppress).
    /// Example: `"1+1;"` returns `"2"`.
    pub fn eval(&self, statement: &str) -> Result<String, String> {
        // Clear output buffer
        OUTPUT_BUF.with(|buf| buf.borrow_mut().clear());

        let c_stmt = CString::new(statement)
            .map_err(|e| format!("Invalid statement (contains null): {}", e))?;

        let rc = unsafe { ffi::PROC_process_one_reduce_statement(c_stmt.as_ptr()) };

        let output = OUTPUT_BUF.with(|buf| {
            String::from_utf8_lossy(&buf.borrow()).to_string()
        });

        if rc != 0 {
            return Err(format!(
                "REDUCE error (code {}): {}",
                rc,
                output.trim()
            ));
        }

        // Clean up the output: strip leading/trailing whitespace and
        // any trailing newlines REDUCE adds
        let cleaned = output.trim().to_string();
        Ok(cleaned)
    }

    /// Convenience: wrap an expression in `ws "expr";` to simplify it.
    /// Returns the simplified expression as a string.
    pub fn simplify(&self, expr: &str) -> Result<String, String> {
        // Use the "ws" (write standard) output format for cleaner results.
        // First, tell REDUCE to compute and print in a single statement.
        let stmt = if expr.ends_with(';') || expr.ends_with('$') {
            expr.to_string()
        } else {
            format!("{};", expr)
        };

        self.eval(&stmt)
    }

    /// Set a REDUCE switch (e.g., "expandlogs", "factor").
    pub fn set_switch(&self, name: &str, on: bool) -> Result<(), String> {
        let c_name = CString::new(name)
            .map_err(|e| format!("Invalid switch name: {}", e))?;
        let val = if on { 1 } else { 0 };
        let rc = unsafe { ffi::PROC_set_switch(c_name.as_ptr(), val) };
        if rc != 0 {
            Err(format!("Failed to set switch '{}': {}", name, rc))
        } else {
            Ok(())
        }
    }
}

impl Drop for ReduceSession {
    fn drop(&mut self) {
        unsafe {
            ffi::reduce_ffi_cslfinish(Some(writer_callback));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CSL has global state and can only be initialized once per process.
    // All tests must share a single session instance.
    #[test]
    fn test_reduce_session() {
        let session = ReduceSession::new().expect("Failed to create session");

        // Basic arithmetic
        let result = session.simplify("1+1").expect("1+1 failed");
        assert!(
            result.contains("2"),
            "Expected '2' in result, got: '{}'",
            result
        );

        // Algebraic simplification: (x+1)^2 - x^2 - 2*x = 1
        let result = session
            .simplify("(x+1)^2 - x^2 - 2*x")
            .expect("algebra failed");
        assert!(
            result.contains("1"),
            "Expected '1' in result, got: '{}'",
            result
        );

        // Differentiation: d/dx(x^3) = 3*x^2
        let result = session.simplify("df(x^3, x)").expect("df failed");
        assert!(
            result.contains("3") && result.contains("x"),
            "Expected '3*x^2' or similar in result, got: '{}'",
            result
        );
    }
}
