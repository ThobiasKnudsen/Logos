//! Translation between Logos Unicode math notation and REDUCE ASCII syntax.
//!
//! Logos uses Unicode symbols (π, ², √, etc.) while REDUCE uses ASCII
//! (pi, **2, sqrt(), etc.). This module converts between the two.

/// Convert Logos Unicode math notation to REDUCE-compatible ASCII.
pub fn to_reduce(input: &str) -> String {
    let mut output = String::with_capacity(input.len() * 2);
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Greek letters
            '\u{03B1}' => output.push_str("alpha"),
            '\u{03B2}' => output.push_str("beta"),
            '\u{03B3}' => output.push_str("gamma"),
            '\u{03B4}' => output.push_str("delta"),
            '\u{03B5}' => output.push_str("epsilon"),
            '\u{03B8}' => output.push_str("theta"),
            '\u{03BB}' => output.push_str("lambda"),
            '\u{03BC}' => output.push_str("mu"),
            '\u{03C0}' => output.push_str("pi"),
            '\u{03C3}' => output.push_str("sigma"),
            '\u{03C6}' => output.push_str("phi"),
            '\u{03C9}' => output.push_str("omega"),

            // Superscript digits → **N
            '\u{00B2}' => output.push_str("**2"),
            '\u{00B3}' => output.push_str("**3"),
            '\u{2070}' => output.push_str("**0"),
            '\u{00B9}' => output.push_str("**1"),
            '\u{2074}' => output.push_str("**4"),
            '\u{2075}' => output.push_str("**5"),
            '\u{2076}' => output.push_str("**6"),
            '\u{2077}' => output.push_str("**7"),
            '\u{2078}' => output.push_str("**8"),
            '\u{2079}' => output.push_str("**9"),

            // Math operators
            '\u{00D7}' => output.push('*'),   // ×
            '\u{00F7}' => output.push('/'),   // ÷
            '\u{2212}' => output.push('-'),   // − (minus sign)
            '\u{221A}' => output.push_str("sqrt"), // √

            // Math functions / operators
            '\u{222B}' => output.push_str("int"),   // ∫ (integral)
            '\u{2202}' => output.push_str("df"),    // ∂ (partial derivative)
            '\u{2207}' => output.push_str("nabla"), // ∇ (nabla/gradient)

            // Summation / product (these need context, basic stubs)
            '\u{2211}' => output.push_str("sum"),  // ∑
            '\u{220F}' => output.push_str("prod"), // ∏

            // Infinity
            '\u{221E}' => output.push_str("infinity"),

            // Additional Greek letters (uppercase)
            '\u{0393}' => output.push_str("Gamma"),   // Γ
            '\u{0394}' => output.push_str("Delta"),   // Δ
            '\u{0398}' => output.push_str("Theta"),   // Θ
            '\u{039B}' => output.push_str("Lambda"),  // Λ
            '\u{039E}' => output.push_str("Xi"),      // Ξ
            '\u{03A0}' => output.push_str("Pi"),      // Π (uppercase Pi)
            '\u{03A3}' => output.push_str("Sigma"),   // Σ
            '\u{03A6}' => output.push_str("Phi"),     // Φ
            '\u{03A8}' => output.push_str("Psi"),     // Ψ
            '\u{03A9}' => output.push_str("Omega"),   // Ω

            // Additional lowercase Greek letters
            '\u{03B6}' => output.push_str("zeta"),    // ζ
            '\u{03B7}' => output.push_str("eta"),     // η
            '\u{03B9}' => output.push_str("iota"),    // ι
            '\u{03BA}' => output.push_str("kappa"),   // κ
            '\u{03BD}' => output.push_str("nu"),      // ν
            '\u{03BE}' => output.push_str("xi"),      // ξ
            '\u{03C1}' => output.push_str("rho"),     // ρ
            '\u{03C5}' => output.push_str("upsilon"), // υ
            '\u{03C7}' => output.push_str("chi"),     // χ
            '\u{03C8}' => output.push_str("psi"),     // ψ

            // Relation symbols
            '\u{2264}' => output.push_str("<="),  // ≤
            '\u{2265}' => output.push_str(">="),  // ≥
            '\u{2260}' => output.push_str("!="),  // ≠

            // Arrows (pass as identifiers for now)
            '\u{2192}' => output.push_str("arrow_right"), // →
            '\u{2190}' => output.push_str("arrow_left"),  // ←

            // Only pass through REDUCE-safe ASCII characters.
            // Strip backslashes and unmapped non-ASCII to prevent C++ exceptions.
            _ => {
                if ch.is_ascii_alphanumeric()
                    || matches!(ch,
                        ' ' | '+' | '-' | '*' | '/' | '^' | '(' | ')' | ','
                        | '.' | ';' | '$' | ':' | '=' | '<' | '>' | '!'
                        | '_' | '\'' | '"' | '\n' | '\t')
                {
                    output.push(ch);
                }
                // else: silently drop (backslash, @, #, non-ASCII not in map, etc.)
            }
        }
    }

    output
}

/// Convert REDUCE ASCII output back to Unicode math notation.
pub fn from_reduce(input: &str) -> String {
    let mut output = input.to_string();

    // Simple word-boundary replacements
    output = output.replace("pi", "\u{03C0}");
    output = output.replace("infinity", "\u{221E}");
    output = output.replace("sqrt", "\u{221A}");

    // **N → superscript (common cases)
    output = output.replace("**2", "\u{00B2}");
    output = output.replace("**3", "\u{00B3}");
    output = output.replace("**4", "\u{2074}");
    output = output.replace("**5", "\u{2075}");
    output = output.replace("**6", "\u{2076}");
    output = output.replace("**7", "\u{2077}");
    output = output.replace("**8", "\u{2078}");
    output = output.replace("**9", "\u{2079}");

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── to_reduce: Greek letters ─────────────────────────────────

    #[test]
    fn test_greek_to_reduce() {
        assert_eq!(to_reduce("\u{03B1}"), "alpha");
        assert_eq!(to_reduce("\u{03B2}"), "beta");
        assert_eq!(to_reduce("\u{03B3}"), "gamma");
        assert_eq!(to_reduce("\u{03B4}"), "delta");
        assert_eq!(to_reduce("\u{03B5}"), "epsilon");
        assert_eq!(to_reduce("\u{03B8}"), "theta");
        assert_eq!(to_reduce("\u{03BB}"), "lambda");
        assert_eq!(to_reduce("\u{03BC}"), "mu");
        assert_eq!(to_reduce("\u{03C0}"), "pi");
        assert_eq!(to_reduce("\u{03C3}"), "sigma");
        assert_eq!(to_reduce("\u{03C6}"), "phi");
        assert_eq!(to_reduce("\u{03C9}"), "omega");
    }

    // ── to_reduce: Superscript digits ────────────────────────────

    #[test]
    fn test_superscripts_to_reduce() {
        assert_eq!(to_reduce("x\u{2070}"), "x**0");
        assert_eq!(to_reduce("x\u{00B9}"), "x**1");
        assert_eq!(to_reduce("x\u{00B2}"), "x**2");
        assert_eq!(to_reduce("x\u{00B3}"), "x**3");
        assert_eq!(to_reduce("x\u{2074}"), "x**4");
        assert_eq!(to_reduce("x\u{2075}"), "x**5");
        assert_eq!(to_reduce("x\u{2076}"), "x**6");
        assert_eq!(to_reduce("x\u{2077}"), "x**7");
        assert_eq!(to_reduce("x\u{2078}"), "x**8");
        assert_eq!(to_reduce("x\u{2079}"), "x**9");
    }

    // ── to_reduce: Math operators ────────────────────────────────

    #[test]
    fn test_operators_to_reduce() {
        assert_eq!(to_reduce("x \u{00D7} y"), "x * y");   // ×
        assert_eq!(to_reduce("x \u{00F7} y"), "x / y");   // ÷
        assert_eq!(to_reduce("x \u{2212} y"), "x - y");   // − (minus sign)
        assert_eq!(to_reduce("\u{221A}(x)"), "sqrt(x)");   // √
    }

    // ── to_reduce: Special symbols ───────────────────────────────

    #[test]
    fn test_special_symbols_to_reduce() {
        assert_eq!(to_reduce("\u{2211}"), "sum");    // ∑
        assert_eq!(to_reduce("\u{220F}"), "prod");   // ∏
        assert_eq!(to_reduce("\u{221E}"), "infinity");
    }

    // ── to_reduce: Passthrough ───────────────────────────────────

    #[test]
    fn test_ascii_passthrough() {
        assert_eq!(to_reduce("x + y * 2"), "x + y * 2");
        assert_eq!(to_reduce("sin(x)"), "sin(x)");
        assert_eq!(to_reduce("df(x**2, x)"), "df(x**2, x)");
    }

    // ── to_reduce: Combined expressions ──────────────────────────

    #[test]
    fn test_combined_expression_to_reduce() {
        // π × r²
        assert_eq!(
            to_reduce("\u{03C0} \u{00D7} r\u{00B2}"),
            "pi * r**2"
        );
        // α + β × γ
        assert_eq!(
            to_reduce("\u{03B1} + \u{03B2} \u{00D7} \u{03B3}"),
            "alpha + beta * gamma"
        );
        // √(x² + y²)
        assert_eq!(
            to_reduce("\u{221A}(x\u{00B2} + y\u{00B2})"),
            "sqrt(x**2 + y**2)"
        );
    }

    // ── from_reduce: Exponents ───────────────────────────────────

    #[test]
    fn test_from_reduce_exponents() {
        assert_eq!(from_reduce("x**2"), "x\u{00B2}");
        assert_eq!(from_reduce("x**3"), "x\u{00B3}");
        assert_eq!(from_reduce("x**4"), "x\u{2074}");
        assert_eq!(from_reduce("x**5"), "x\u{2075}");
        assert_eq!(from_reduce("x**6"), "x\u{2076}");
        assert_eq!(from_reduce("x**7"), "x\u{2077}");
        assert_eq!(from_reduce("x**8"), "x\u{2078}");
        assert_eq!(from_reduce("x**9"), "x\u{2079}");
    }

    // ── from_reduce: Symbols ─────────────────────────────────────

    #[test]
    fn test_from_reduce_symbols() {
        assert_eq!(from_reduce("pi"), "\u{03C0}");
        assert_eq!(from_reduce("infinity"), "\u{221E}");
        assert_eq!(from_reduce("sqrt(x)"), "\u{221A}(x)");
    }

    // ── from_reduce: Combined output ─────────────────────────────

    #[test]
    fn test_from_reduce_combined() {
        assert_eq!(
            from_reduce("3*x**2 + pi"),
            "3*x\u{00B2} + \u{03C0}"
        );
    }

    // ── Round-trip tests ─────────────────────────────────────────

    #[test]
    fn test_roundtrip_pi() {
        let unicode = "\u{03C0}";
        let ascii = to_reduce(unicode);
        assert_eq!(ascii, "pi");
        let back = from_reduce(&ascii);
        assert_eq!(back, unicode);
    }

    #[test]
    fn test_roundtrip_exponents() {
        for (uni, exp) in [
            ("x\u{00B2}", "x**2"),
            ("x\u{00B3}", "x**3"),
            ("x\u{2074}", "x**4"),
        ] {
            let ascii = to_reduce(uni);
            assert_eq!(ascii, exp);
            let back = from_reduce(&ascii);
            assert_eq!(back, uni);
        }
    }

    // ── Edge cases ───────────────────────────────────────────────

    #[test]
    fn test_empty_input() {
        assert_eq!(to_reduce(""), "");
        assert_eq!(from_reduce(""), "");
    }

    // ── Complex expression: x²*y²+sin(x)³-sin(y²)²=9 ──────────

    #[test]
    fn test_complex_expression_to_reduce() {
        // x²*y²+sin(x)³-sin(y²)²=9
        let input = "x\u{00B2}*y\u{00B2}+sin(x)\u{00B3}-sin(y\u{00B2})\u{00B2}=9";
        let ascii = to_reduce(input);
        assert_eq!(ascii, "x**2*y**2+sin(x)**3-sin(y**2)**2=9");
    }

    #[test]
    fn test_complex_expression_from_reduce() {
        // REDUCE output → Unicode
        let reduce_out = "x**2*y**2 + sin(x)**3 - sin(y**2)**2";
        let unicode = from_reduce(reduce_out);
        assert_eq!(unicode, "x\u{00B2}*y\u{00B2} + sin(x)\u{00B3} - sin(y\u{00B2})\u{00B2}");
    }

    #[test]
    fn test_complex_expression_roundtrip() {
        // x²*y²+sin(x)³-sin(y²)²
        let input = "x\u{00B2}*y\u{00B2}+sin(x)\u{00B3}-sin(y\u{00B2})\u{00B2}";
        let ascii = to_reduce(input);
        assert_eq!(ascii, "x**2*y**2+sin(x)**3-sin(y**2)**2");
        let back = from_reduce(&ascii);
        assert_eq!(back, input, "round-trip failed: {} -> {} -> {}", input, ascii, back);
    }
}
