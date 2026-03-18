//! Safe wrapper around the CSL/REDUCE FFI.
//!
//! `ReduceSession` owns a single CSL instance. It must only be used from
//! one thread (CSL is not thread-safe). The background service in
//! `service.rs` ensures this by running all calls on a dedicated thread.

use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_int;

use super::ffi;

// Thread-local buffers for CSL I/O callbacks.
// CSL communicates via character-at-a-time callbacks. We use thread-local
// storage so that the static `extern "C"` callback functions can access
// the input/output buffers without any synchronization.
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
        let rc = unsafe { ffi::reduce_ffi_prepare_for_top_level_loop() };
        if rc != 0 {
            return Err(format!("PROC_prepare_for_top_level_loop failed: {}", rc));
        }

        // Set callbacks for future I/O
        unsafe {
            ffi::reduce_ffi_set_callbacks(Some(reader_callback), Some(writer_callback));
        }

        // Suppress GC messages
        unsafe {
            ffi::reduce_ffi_gc_messages(0);
        }

        // We're in Lisp/symbolic mode. The bootstrap image has a broken
        // assgnpri that dumps raw S-expressions. We define our own infix
        // printer using prepsq (SQ → prefix) and a recursive formatter.
        let init_stmts = [
            // Define recursive infix printer for prepsq prefix forms
            concat!(
                "symbolic procedure logos_infix(u); ",
                "if numberp u then prin2 u ",
                "else if atom u then prin2 u ",
                "else begin scalar op; ",
                "op := car u; ",
                "if op eq 'plus then ",
                "<< logos_infix cadr u; ",
                "for each x in cddr u do << prin2 \" + \"; logos_infix x >> >> ",
                "else if op eq 'minus then ",
                "<< prin2 \" - \"; logos_infix cadr u >> ",
                "else if op eq 'difference then ",
                "<< logos_infix cadr u; prin2 \" - \"; logos_infix caddr u >> ",
                "else if op eq 'times then ",
                "<< logos_infix cadr u; ",
                "for each x in cddr u do << prin2 \"*\"; logos_infix x >> >> ",
                "else if op eq 'quotient then ",
                "<< logos_infix cadr u; prin2 \"/\"; logos_infix caddr u >> ",
                "else if op eq 'expt then ",
                "<< logos_infix cadr u; prin2 \"^\"; logos_infix caddr u >> ",
                "else << ",
                "prin2 op; prin2 \"(\"; ",
                "logos_infix cadr u; ",
                "for each x in cddr u do << prin2 \",\"; logos_infix x >>; ",
                "prin2 \")\" >> ",
                "end;",
            ),
            // Redefine assgnpri to use our infix printer
            concat!(
                "symbolic procedure logos_assgnpri(u, v, w); ",
                "if atom u then << prin2 u; terpri() >> ",
                "else if eqcar(u, '!*sq) then << logos_infix prepsq cadr u; terpri() >> ",
                "else << print u; terpri() >>;",
            ),
            "copyd('assgnpri, 'logos_assgnpri);",
            // Switch to algebraic mode
            "algebraic;",
        ];
        for stmt in &init_stmts {
            let c = CString::new(*stmt).unwrap();
            unsafe {
                ffi::reduce_ffi_process_statement(c.as_ptr());
            }
        }

        // Load all compiled packages from the embedded image.
        // These are baked into vendor/csl/image.cpp at build time.
        let packages = ["int", "solve", "ezgcd", "factor", "matrix", "taylor"];
        for pkg in &packages {
            OUTPUT_BUF.with(|buf| buf.borrow_mut().clear());
            let stmt = CString::new(format!("load_package({});", pkg)).unwrap();
            let rc = unsafe { ffi::reduce_ffi_process_statement(stmt.as_ptr()) };
            let output = OUTPUT_BUF.with(|buf| {
                String::from_utf8_lossy(&buf.borrow()).to_string()
            });
            if rc != 0 || output.contains("not found") || output.contains("*****") {
                log::error!("Failed to load REDUCE package '{}': {}", pkg, output.trim());
            } else {
                log::info!("REDUCE package '{}' loaded", pkg);
            }
        }
        OUTPUT_BUF.with(|buf| buf.borrow_mut().clear());

        // Disable echo so output doesn't contain the input statement
        let echo = CString::new("echo").unwrap();
        unsafe {
            ffi::reduce_ffi_set_switch(echo.as_ptr(), 0);
        }

        // Warmup: execute a statement to absorb any remaining init output
        let warmup = CString::new("1;").unwrap();
        unsafe {
            ffi::reduce_ffi_process_statement(warmup.as_ptr());
        }

        // Drain all startup and warmup output
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
        log::trace!("REDUCE eval: {:?}", statement);
        // Clear output buffer
        OUTPUT_BUF.with(|buf| buf.borrow_mut().clear());

        let c_stmt = CString::new(statement)
            .map_err(|e| format!("Invalid statement (contains null): {}", e))?;

        let rc = unsafe { ffi::reduce_ffi_process_statement(c_stmt.as_ptr()) };

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

        // Clean up: the bootstrap output printer wraps results as:
        //   "\n\nBootstrap version of assgnpri:\n{result}\nnil\nonly\n"
        // Strip the marker prefix and "nil"/"only" suffix to extract the
        // actual result. Also handles clean output (no bootstrap noise).
        let lines: Vec<&str> = output.trim().lines().collect();

        // Strip trailing noise lines ("nil", "only", empty)
        let mut end = lines.len();
        while end > 0 {
            let l = lines[end - 1].trim();
            if l.is_empty() || l == "nil" || l == "only" {
                end -= 1;
            } else {
                break;
            }
        }

        // Strip leading noise lines ("Bootstrap version of assgnpri:", empty)
        let mut start = 0;
        while start < end {
            let l = lines[start].trim();
            if l.is_empty() || l.starts_with("Bootstrap version") {
                start += 1;
            } else {
                break;
            }
        }

        // Filter out REDUCE noise/error lines, keeping actual results
        let mut result_lines = Vec::new();
        let mut error_lines = Vec::new();
        for &line in &lines[start..end] {
            let trimmed = line.trim();
            if trimmed.starts_with("*****")
                || trimmed.contains("package not found")
                || trimmed.starts_with("Failed to find")
            {
                error_lines.push(trimmed);
            } else if !trimmed.is_empty() {
                result_lines.push(trimmed);
            }
        }

        if !result_lines.is_empty() {
            // Got a result (possibly with warnings — ignore them)
            Ok(result_lines.join("\n"))
        } else if !error_lines.is_empty() {
            // Only error lines, no result
            Err(error_lines.join("\n"))
        } else {
            Ok(String::new())
        }
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

    /// Toggle a REDUCE switch (e.g., "factor", "echo").
    pub fn set_switch(&self, name: &str, on: bool) -> Result<(), String> {
        let c_name = CString::new(name)
            .map_err(|e| format!("Invalid switch name: {}", e))?;
        let val = if on { 1 } else { 0 };
        let rc = unsafe { ffi::reduce_ffi_set_switch(c_name.as_ptr(), val) };
        if rc != 0 && rc != -1 {
            return Err(format!("set_switch({}, {}) failed: {}", name, on, rc));
        }
        if rc == -1 {
            return Err(format!("set_switch({}, {}): C++ exception caught", name, on));
        }
        Ok(())
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
    use crate::lang::reduce::translate;

    // CSL has global state and can only be initialized once per process.
    // All tests must live in a single #[test] function that shares one session.

    /// Helper: assert that the REDUCE result contains `expected`.
    #[allow(dead_code)]
    fn assert_simplify(session: &ReduceSession, input: &str, expected: &str) {
        let result = session
            .simplify(input)
            .unwrap_or_else(|e| panic!("simplify({:?}) failed: {}", input, e));
        assert!(
            result.contains(expected),
            "simplify({:?}): expected {:?} in {:?}",
            input,
            expected,
            result,
        );
    }

    /// Helper: assert the REDUCE result exactly equals `expected`.
    fn assert_simplify_eq(session: &ReduceSession, input: &str, expected: &str) {
        let result = session
            .simplify(input)
            .unwrap_or_else(|e| panic!("simplify({:?}) failed: {}", input, e));
        assert_eq!(
            result, expected,
            "simplify({:?}): expected {:?}, got {:?}",
            input, expected, result,
        );
    }

    #[test]
    fn test_reduce_session() {
        let session = ReduceSession::new().expect("Failed to create REDUCE session");

        // ═══════════════════════════════════════════════════════════
        // 1. NUMERIC ARITHMETIC
        // ═══════════════════════════════════════════════════════════

        // ── Basic operations ─────────────────────────────────────
        assert_simplify_eq(&session, "1+1", "2");
        assert_simplify_eq(&session, "3*7", "21");
        assert_simplify_eq(&session, "100/4", "25");
        assert_simplify_eq(&session, "2**10", "1024");
        assert_simplify_eq(&session, "17 - 9", "8");
        assert_simplify_eq(&session, "(-3)*(-4)", "12");
        assert_simplify_eq(&session, "2 + 3 * 4", "14");
        assert_simplify_eq(&session, "(2 + 3) * 4", "20");

        // ── Numeric functions (algebraic mode returns exact) ─────
        assert_simplify_eq(&session, "sin(0)", "0");
        assert_simplify_eq(&session, "cos(0)", "1");
        assert_simplify_eq(&session, "log(1)", "0");
        assert_simplify_eq(&session, "sqrt(4)", "2");
        assert_simplify_eq(&session, "sqrt(9)", "3");

        // ── Large numbers (arbitrary precision bignum) ───────────
        assert_simplify_eq(&session, "2**64", "18446744073709551616");
        assert_simplify_eq(&session, "999999 * 1000001", "999999999999");

        // ── Negative exponents → exact rationals ────────────────
        assert_simplify_eq(&session, "2**(-1)", "1/2");
        assert_simplify_eq(&session, "10**(-2)", "1/100");

        // ── Zero handling ───────────────────────────────────────
        assert_simplify_eq(&session, "0 + 0", "0");
        assert_simplify_eq(&session, "0 * 999", "0");
        assert_simplify_eq(&session, "x * 0", "0");
        assert_simplify_eq(&session, "x + 0", "x");
        assert_simplify_eq(&session, "0**5", "0");
        assert_simplify_eq(&session, "1**999", "1");

        // ── Nested operations ───────────────────────────────────
        assert_simplify_eq(&session, "((2+3)*4 - 10) / 2", "5");
        assert_simplify_eq(&session, "2**(2**3)", "256");

        // ── Sign handling ───────────────────────────────────────
        assert_simplify_eq(&session, "(-1)**4", "1");

        // ═══════════════════════════════════════════════════════════
        // 2. SYMBOLIC ALGEBRA
        // ═══════════════════════════════════════════════════════════

        // ── Variable collection ─────────────────────────────────
        assert_simplify_eq(&session, "x + x + x", "3*x");
        assert_simplify_eq(&session, "5*x - 3*x", "2*x");

        // ── Polynomial identity ─────────────────────────────────
        assert_simplify_eq(&session, "(x+1)**2 - x**2 - 2*x", "1");
        assert_simplify_eq(&session, "(a+b)*(a-b) - (a**2 - b**2)", "0");

        // ── Like-term collection (multi-variable) ───────────────
        assert_simplify_eq(&session, "x*y + y*x", "2*x*y");

        // ── Expansion ───────────────────────────────────────────
        {
            let r = session.simplify("(x+1)**2").unwrap();
            assert!(r.contains("x^2") && r.contains("2*x") && r.contains("1"),
                "(x+1)**2 expansion: {}", r);
        }

        // ── Distribution ────────────────────────────────────────
        {
            let r = session.simplify("x*(y+z)").unwrap();
            assert!(r.contains("x*y") && r.contains("x*z"),
                "distribution x*(y+z): {}", r);
        }

        // ── Power laws ──────────────────────────────────────────
        assert_simplify_eq(&session, "x**2 * x**3", "x^5");
        assert_simplify_eq(&session, "(x**2)**3", "x^6");

        // ── Symbolic cancellation ───────────────────────────────
        assert_simplify_eq(&session, "x/x", "1");
        {
            let r = session.simplify("(x**2 - y**2)/(x - y)").unwrap();
            assert!(r.contains("x") && r.contains("y"),
                "(x²-y²)/(x-y) cancellation: {}", r);
        }

        // ── No simplification when already simplified ───────────
        {
            let r = session.simplify("x**2 - 1").unwrap();
            assert!(r.contains("x^2"), "x^2 - 1 stays: {}", r);
        }

        // ═══════════════════════════════════════════════════════════
        // 3. DIFFERENTIATION
        // ═══════════════════════════════════════════════════════════

        // ── Power rule ──────────────────────────────────────────
        assert_simplify_eq(&session, "df(x**3, x)", "3*x^2");
        assert_simplify_eq(&session, "df(x**5, x)", "5*x^4");

        // ── Trig derivatives ────────────────────────────────────
        assert_simplify_eq(&session, "df(sin(x), x)", "cos(x)");
        assert_simplify_eq(&session, "df(cos(x), x)", "- sin(x)");

        // ── Log derivative ──────────────────────────────────────
        assert_simplify_eq(&session, "df(log(x), x)", "1/x");

        // ── Higher-order derivatives ────────────────────────────
        assert_simplify_eq(&session, "df(x**4, x, 2)", "12*x^2");
        assert_simplify_eq(&session, "df(x**3, x, 3)", "6");
        assert_simplify_eq(&session, "df(x**3, x, 4)", "0");
        assert_simplify_eq(&session, "df(x**5, x, 2)", "20*x^3");

        // ── Polynomial differentiation ──────────────────────────
        {
            let r = session.simplify("df(x**2 + 3*x + 1, x)").unwrap();
            assert!(r.contains("2*x") && r.contains("3"),
                "df(x²+3x+1, x): {}", r);
        }

        // ── Reciprocal: d/dx(1/x) = -1/x² ─────────────────────
        {
            let r = session.simplify("df(1/x, x)").unwrap();
            assert!(r.contains("1/x^2"),
                "df(1/x, x): expected -1/x^2, got: {}", r);
        }

        // ── Product rule: d/dx(x·sin(x)) ───────────────────────
        {
            let r = session.simplify("df(x*sin(x), x)").unwrap();
            assert!(r.contains("cos(x)") && r.contains("sin(x)"),
                "product rule df(x*sin(x),x): {}", r);
        }

        // ── Chain rule: d/dx(sin(x²)) = 2x·cos(x²) ────────────
        {
            let r = session.simplify("df(sin(x**2), x)").unwrap();
            assert!(r.contains("cos(x^2)") && r.contains("2"),
                "chain rule df(sin(x²),x): {}", r);
        }

        // ── Partial derivatives ─────────────────────────────────
        {
            let r = session.simplify("df(x**3 * y**2, x)").unwrap();
            assert!(r.contains("3") && r.contains("x^2") && r.contains("y^2"),
                "∂/∂x(x³y²): {}", r);
        }
        {
            let r = session.simplify("df(x**3 * y**2, y)").unwrap();
            assert!(r.contains("2") && r.contains("x^3") && r.contains("y"),
                "∂/∂y(x³y²): {}", r);
        }

        // ── Trig chain: d/dx(sin²(x)) = 2sin(x)cos(x) ────────
        {
            let r = session.simplify("df(sin(x)**2, x)").unwrap();
            assert!(r.contains("sin(x)") && r.contains("cos(x)"),
                "df(sin²(x),x): {}", r);
        }

        // ── 2nd derivative of log: d²/dx²(log(x)) = -1/x² ────
        {
            let r = session.simplify("df(log(x), x, 2)").unwrap();
            assert!(r.contains("1/x^2"),
                "df(log(x),x,2): expected -1/x², got: {}", r);
        }

        // ═══════════════════════════════════════════════════════════
        // 4. OUTPUT FORMATTING (logos_infix printer paths)
        // ═══════════════════════════════════════════════════════════

        // ── Atoms: numbers and symbols ──────────────────────────
        assert_simplify_eq(&session, "42", "42");
        assert_simplify_eq(&session, "x", "x");

        // ── Plus: binary and n-ary sums ─────────────────────────
        assert_simplify_eq(&session, "x + y", "x + y");
        {
            let r = session.simplify("x + y + z").unwrap();
            assert!(r.contains("x") && r.contains("y") && r.contains("z")
                && r.contains("+"), "n-ary sum: {}", r);
        }

        // ── Times ───────────────────────────────────────────────
        assert_simplify_eq(&session, "2*x", "2*x");

        // ── Quotient ────────────────────────────────────────────
        assert_simplify_eq(&session, "1/x", "1/x");
        assert_simplify_eq(&session, "a/b", "a/b");

        // ── Expt with symbolic exponent ─────────────────────────
        assert_simplify_eq(&session, "x**n", "x^n");

        // ── Unary minus ─────────────────────────────────────────
        assert_simplify_eq(&session, "-x", "- x");

        // ── Generic function calls (unevaluated trig/log) ──────
        assert_simplify_eq(&session, "sin(x)", "sin(x)");
        assert_simplify_eq(&session, "cos(x)", "cos(x)");
        assert_simplify_eq(&session, "log(x)", "log(x)");

        // ═══════════════════════════════════════════════════════════
        // 5. FRACTION ARITHMETIC
        // ═══════════════════════════════════════════════════════════

        assert_simplify_eq(&session, "1/3 + 1/6", "1/2");
        assert_simplify_eq(&session, "2/3 * 3/4", "1/2");

        // ── Sums to whole numbers ───────────────────────────────
        assert_simplify_eq(&session, "1/3 + 2/3", "1");
        assert_simplify_eq(&session, "1/2 + 1/3 + 1/6", "1");

        // ── Subtraction ─────────────────────────────────────────
        assert_simplify_eq(&session, "7/12 - 1/4", "1/3");

        // ── Fraction exponentiation ─────────────────────────────
        assert_simplify_eq(&session, "(1/2)**2", "1/4");
        assert_simplify_eq(&session, "(2/3)**3", "8/27");

        // ── Auto-reduction to lowest terms ──────────────────────
        assert_simplify_eq(&session, "100/200", "1/2");
        assert_simplify_eq(&session, "36/48", "3/4");

        // ── Mixed integer and fraction ──────────────────────────
        assert_simplify_eq(&session, "1 + 1/2", "3/2");
        assert_simplify_eq(&session, "5 - 7/3", "8/3");

        // ═══════════════════════════════════════════════════════════
        // 6. ERROR HANDLING
        // Note: CSL throws C++ exceptions on some invalid inputs
        // (e.g., @#$%), which abort across FFI. Only test inputs
        // that CSL handles gracefully.
        // ═══════════════════════════════════════════════════════════

        // ── Malformed input (unbalanced parens) ─────────────────
        {
            let r = session.simplify("(x+1");
            assert!(r.is_ok() || r.is_err(), "unbalanced parens: no panic");
        }

        // ── Empty statement ─────────────────────────────────────
        {
            let r = session.eval(";");
            assert!(r.is_ok() || r.is_err(), "empty statement: no panic");
        }

        // ── Long expression (100 terms) ─────────────────────────
        {
            let long_expr = (0..100).map(|_| "1").collect::<Vec<_>>().join("+");
            let r = session.simplify(&long_expr);
            assert!(r.is_ok(), "100-term sum should evaluate: {:?}", r);
            if let Ok(s) = r {
                assert_eq!(s, "100", "sum of 100 ones");
            }
        }

        // ═══════════════════════════════════════════════════════════
        // 7. UNICODE TRANSLATION ROUND-TRIP
        //    Unicode → to_reduce() → simplify() → from_reduce()
        // ═══════════════════════════════════════════════════════════

        // ── Superscript exponents: x² + 1 ──────────────────────
        {
            let input = "x\u{00B2} + 1"; // x² + 1
            let ascii = translate::to_reduce(input);
            assert_eq!(ascii, "x**2 + 1");
            let result = session.simplify(&ascii).unwrap();
            assert!(result.contains("x^2"), "x²+1 eval: {}", result);
            let output = translate::from_reduce(&result);
            assert!(output.contains("x^2"), "x^2 round-trip: {}", output);
        }

        // ── π constant ──────────────────────────────────────────
        {
            let ascii = translate::to_reduce("\u{03C0}"); // π
            assert_eq!(ascii, "pi");
            let result = session.simplify(&ascii).unwrap();
            assert_eq!(result, "pi");
            let output = translate::from_reduce(&result);
            assert_eq!(output, "\u{03C0}"); // π
        }

        // ── √(4) evaluates to 2 ────────────────────────────────
        {
            let ascii = translate::to_reduce("\u{221A}(4)"); // √(4)
            assert_eq!(ascii, "sqrt(4)");
            let result = session.simplify(&ascii).unwrap();
            assert_eq!(result, "2");
        }

        // ── √(x) stays symbolic, round-trips ───────────────────
        {
            let ascii = translate::to_reduce("\u{221A}(x)"); // √(x)
            assert_eq!(ascii, "sqrt(x)");
            let result = session.simplify(&ascii).unwrap();
            assert_eq!(result, "sqrt(x)");
            let output = translate::from_reduce(&result);
            assert_eq!(output, "\u{221A}(x)"); // √(x)
        }

        // ── × operator: 3 × x² → 3*x**2 → 3*x^2 ──────────────
        {
            let input = "3 \u{00D7} x\u{00B2}"; // 3 × x²
            let ascii = translate::to_reduce(input);
            assert_eq!(ascii, "3 * x**2");
            let result = session.simplify(&ascii).unwrap();
            assert_eq!(result, "3*x^2");
            let output = translate::from_reduce(&result);
            assert_eq!(output, "3*x^2");
        }

        // ── Simplification changes expression: x²+2x+1−x² → 2x+1
        {
            let input = "x\u{00B2} + 2*x + 1 \u{2212} x\u{00B2}"; // x²+2x+1−x²
            let ascii = translate::to_reduce(input);
            assert_eq!(ascii, "x**2 + 2*x + 1 - x**2");
            let result = session.simplify(&ascii).unwrap();
            assert_eq!(result, "2*x + 1");
        }

        // ── Differentiation with superscripts: df(x³,x) → 3x^2 ─
        {
            let ascii = translate::to_reduce("x\u{00B3}"); // x³
            assert_eq!(ascii, "x**3");
            let result = session.simplify(&format!("df({}, x)", ascii)).unwrap();
            assert_eq!(result, "3*x^2");
            let output = translate::from_reduce(&result);
            assert_eq!(output, "3*x^2");
        }

        // ── Greek letters: α+1 (from_reduce doesn't reverse α) ─
        {
            let ascii = translate::to_reduce("\u{03B1} + 1"); // α + 1
            assert_eq!(ascii, "alpha + 1");
            let result = session.simplify(&ascii).unwrap();
            assert_eq!(result, "alpha + 1");
            // NOTE: from_reduce only reverses pi, infinity, sqrt — not alpha
            let output = translate::from_reduce(&result);
            assert_eq!(output, "alpha + 1"); // known gap
        }

        // ═══════════════════════════════════════════════════════════
        // 8. ALGEBRAIC FUNCTIONS (substitution, GCD, deg, num/den, etc.)
        // ═══════════════════════════════════════════════════════════

        // ── Substitution ────────────────────────────────────────
        assert_simplify_eq(&session, "sub(x=2, x**2 + 1)", "5");
        assert_simplify_eq(&session, "sub(x=0, sin(x))", "0");
        {
            let r = session.simplify("sub(x=y+1, x**2)").unwrap();
            assert!(r.contains("y^2") && r.contains("2*y") && r.contains("1"),
                "sub(x=y+1, x²): {}", r);
        }

        // ── GCD ─────────────────────────────────────────────────
        assert_simplify_eq(&session, "gcd(12, 18)", "6");
        assert_simplify_eq(&session, "gcd(100, 75)", "25");

        // ── Degree ──────────────────────────────────────────────
        assert_simplify_eq(&session, "deg(x**3 + x + 1, x)", "3");
        assert_simplify_eq(&session, "deg(x**2*y + y**3, y)", "3");

        // ── Numerator / denominator extraction ──────────────────
        assert_simplify_eq(&session, "num(x/(x+1))", "x");
        {
            let r = session.simplify("den(x/(x+1))").unwrap();
            assert!(r.contains("x") && r.contains("1"),
                "den(x/(x+1)): {}", r);
        }

        // ── Remainder (polynomial division) ─────────────────────
        assert_simplify_eq(&session, "remainder(x**3 + 1, x + 1)", "0");
        assert_simplify_eq(&session, "remainder(x**2 + 1, x)", "1");

        // ── Absolute value ──────────────────────────────────────
        assert_simplify_eq(&session, "abs(-5)", "5");
        assert_simplify_eq(&session, "abs(3)", "3");

        // ── Exponential and logarithm ───────────────────────────
        assert_simplify_eq(&session, "exp(0)", "1");
        {
            let r = session.simplify("exp(log(x))").unwrap();
            // May simplify to x, or stay as exp(log(x))
            assert!(r == "x" || r.contains("exp"),
                "exp(log(x)): {}", r);
        }

        // ── Additional trig functions ───────────────────────────
        assert_simplify_eq(&session, "tan(0)", "0");
        {
            // atan, asin, acos may or may not simplify at special values
            let r = session.simplify("asin(0)").unwrap();
            assert!(r == "0" || r.contains("asin"),
                "asin(0): {}", r);
        }
        {
            let r = session.simplify("acos(1)").unwrap();
            assert!(r == "0" || r.contains("acos"),
                "acos(1): {}", r);
        }

        // ── Hyperbolic functions ────────────────────────────────
        {
            let r = session.simplify("sinh(0)").unwrap();
            assert!(r == "0" || r.contains("sinh"),
                "sinh(0): {}", r);
        }
        {
            let r = session.simplify("cosh(0)").unwrap();
            assert!(r == "1" || r.contains("cosh"),
                "cosh(0): {}", r);
        }

        // ── Symbolic sqrt ───────────────────────────────────────
        {
            let r = session.simplify("sqrt(x**2)").unwrap();
            // REDUCE simplifies to abs(x), x, or leaves as sqrt(x**2)
            assert!(r == "x" || r.contains("abs") || r.contains("sqrt"),
                "sqrt(x²): {}", r);
        }

        // ── For-sum loop ────────────────────────────────────────
        {
            let r = session.eval("for k := 1:5 sum k;");
            assert!(r.is_ok(), "for-sum loop: {:?}", r);
            if let Ok(ref s) = r {
                assert!(s.contains("15"), "for k:=1:5 sum k = 15, got: {}", s);
            }
        }

        // ── Conditional (concrete values) ───────────────────────
        {
            let r = session.eval("if 3 > 2 then 1 else -1;");
            assert!(r.is_ok(), "if-then-else: {:?}", r);
            if let Ok(ref s) = r {
                assert!(s.contains("1"), "if 3>2 then 1 else -1: {}", s);
            }
        }

        // ═══════════════════════════════════════════════════════════
        // 9. PACKAGE-DEPENDENT FEATURES (now guaranteed available)
        // ═══════════════════════════════════════════════════════════

        // ── LCM ─────────────────────────────────────────────────
        {
            let s = session.eval("lcm(12, 18);").expect("lcm failed");
            assert!(s.contains("36"), "lcm(12,18): {}", s);
        }

        // ── Trig identities ─────────────────────────────────────
        {
            let r = session.simplify("sin(x)**2 + cos(x)**2")
                .expect("trig identity eval failed");
            assert!(
                r == "1" || r.contains("sin") && r.contains("cos"),
                "trig identity: {}", r
            );
        }

        // ── Integration ─────────────────────────────────────────
        {
            let r = session.simplify("int(x**2, x)")
                .expect("int(x²,x) failed");
            assert!(r.contains("x^3"), "int(x²,x): {}", r);
        }
        {
            let r = session.eval("int(sin(x), x);")
                .expect("int(sin(x),x) failed");
            assert!(r.contains("cos"), "int(sin(x),x): {}", r);
        }
        {
            let r = session.eval("int(1/x, x);")
                .expect("int(1/x,x) failed");
            assert!(r.contains("log"), "int(1/x,x): {}", r);
        }

        // ── Solve ───────────────────────────────────────────────
        {
            let r = session.eval("solve(x**2 - 1, x);")
                .expect("solve(x²-1,x) failed");
            assert!(r.contains("1"), "solve(x²-1,x): {}", r);
        }

        // ── Taylor series ───────────────────────────────────────
        {
            let r = session.eval("taylor(sin(x), x, 0, 5);")
                .expect("taylor(sin(x),x,0,5) failed");
            assert!(r.contains("x"), "taylor(sin(x),x,0,5): {}", r);
        }

        // ═══════════════════════════════════════════════════════════
        // 10. SET_SWITCH BEHAVIOR (factor mode guaranteed available)
        // ═══════════════════════════════════════════════════════════

        session.set_switch("factor", true).expect("factor on");
        {
            let r = session.simplify("x**2 - 1")
                .expect("factor mode x²-1 failed");
            assert!(r.contains("x"), "factor mode x²-1: {}", r);
            session.set_switch("factor", false).expect("factor off");
            // Verify normal mode restored
            let r2 = session.simplify("x**2 - 1").unwrap();
            assert!(r2.contains("x^2"), "after factor off: {}", r2);
        }

        // ═══════════════════════════════════════════════════════════
        // 10b. COMPLEX UNICODE EXPRESSION ROUND-TRIP
        //      x²*y²+sin(x)³-sin(y²)²  (the `=9` is stripped for eval)
        // ═══════════════════════════════════════════════════════════
        {
            // Unicode → REDUCE ASCII
            let input = "x\u{00B2}*y\u{00B2}+sin(x)\u{00B3}-sin(y\u{00B2})\u{00B2}";
            let ascii = translate::to_reduce(input);
            assert_eq!(ascii, "x**2*y**2+sin(x)**3-sin(y**2)**2");

            // Evaluate in REDUCE — should return with the same terms
            let result = session.simplify(&ascii).unwrap();
            // REDUCE may reorder but key subexpressions must survive
            assert!(result.contains("sin(x)"),
                "complex expr: expected sin(x) in: {}", result);
            assert!(result.contains("sin(y^2)") || result.contains("sin(y^"),
                "complex expr: expected sin(y^2) in: {}", result);
            assert!(result.contains("x^2") || result.contains("x*y"),
                "complex expr: expected x^2 or x*y in: {}", result);

            // Round-trip back via from_reduce (** → ^)
            let back = translate::from_reduce(&result);
            assert!(back.contains("sin(x)"),
                "round-trip: expected sin(x) in: {}", back);
            assert!(back.contains("^"),
                "round-trip: expected ^ in: {}", back);
        }

        // ═══════════════════════════════════════════════════════════
        // 11. STATEFUL OPERATIONS (assignments — test last, clean up)
        // ═══════════════════════════════════════════════════════════

        // ── Variable assignment and use ─────────────────────────
        {
            session.eval("myvar := x + 1;").expect("assignment failed");
            let r = session.simplify("myvar**2").unwrap();
            assert!(r.contains("x^2") && r.contains("2*x") && r.contains("1"),
                "myvar**2 = (x+1)²: {}", r);
            // Clean up
            session.eval("clear myvar;").expect("clear failed");
        }

        // ── Multi-step computation ──────────────────────────────
        {
            session.eval("aa := 3;").expect("aa := 3");
            session.eval("bb := 4;").expect("bb := 4");
            let r = session.simplify("aa + bb").unwrap();
            assert!(r.contains("7"), "aa+bb=7: {}", r);
            session.eval("clear aa, bb;").expect("clear aa,bb");
        }

    }
}
