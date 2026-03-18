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

            // Summation / product (these need context, basic stubs)
            '\u{2211}' => output.push_str("sum"),  // ∑
            '\u{220F}' => output.push_str("prod"), // ∏

            // Infinity
            '\u{221E}' => output.push_str("infinity"),

            // Everything else passes through
            _ => output.push(ch),
        }
    }

    output
}

/// Check whether the character at the given position is an ASCII alphanumeric
/// or underscore — i.e. part of an identifier.
fn is_word_char(s: &str, byte_pos: usize) -> bool {
    s.as_bytes()
        .get(byte_pos)
        .map_or(false, |&b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Replace `word` with `replacement` only at word boundaries (not inside
/// longer identifiers like "spin" when replacing "pi").
fn replace_word(input: &str, word: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut start = 0;
    while let Some(idx) = input[start..].find(word) {
        let abs = start + idx;
        let before_ok = abs == 0 || !is_word_char(input, abs - 1);
        let after_ok = !is_word_char(input, abs + word.len());
        if before_ok && after_ok {
            result.push_str(&input[start..abs]);
            result.push_str(replacement);
        } else {
            result.push_str(&input[start..abs + word.len()]);
        }
        start = abs + word.len();
    }
    result.push_str(&input[start..]);
    result
}

/// Replace `**N` with a superscript, but only when the character after the
/// digit is NOT another ASCII digit (so `**29` stays as `**29`, not `²9`).
fn replace_exponent(input: &str, digit: char, superscript: &str) -> String {
    let pattern: String = format!("**{}", digit);
    let mut result = String::with_capacity(input.len());
    let mut start = 0;
    while let Some(idx) = input[start..].find(&pattern) {
        let abs = start + idx;
        let after_pos = abs + pattern.len();
        let after_is_digit = input
            .as_bytes()
            .get(after_pos)
            .map_or(false, |&b| b.is_ascii_digit());
        if after_is_digit {
            // Part of a larger exponent like **29 — don't replace
            result.push_str(&input[start..abs + pattern.len()]);
        } else {
            result.push_str(&input[start..abs]);
            result.push_str(superscript);
        }
        start = abs + pattern.len();
    }
    result.push_str(&input[start..]);
    result
}

/// Convert REDUCE ASCII output back to Unicode math notation.
pub fn from_reduce(input: &str) -> String {
    let mut output = input.to_string();

    // Word-boundary-safe replacements (longest first to avoid "pi" matching
    // inside "infinity" — though word-boundary checks handle it anyway).
    output = replace_word(&output, "infinity", "\u{221E}");
    output = replace_word(&output, "sqrt", "\u{221A}");
    output = replace_word(&output, "pi", "\u{03C0}");

    // **N → superscript (only when NOT followed by another digit)
    output = replace_exponent(&output, '2', "\u{00B2}");
    output = replace_exponent(&output, '3', "\u{00B3}");
    output = replace_exponent(&output, '4', "\u{2074}");
    output = replace_exponent(&output, '5', "\u{2075}");
    output = replace_exponent(&output, '6', "\u{2076}");
    output = replace_exponent(&output, '7', "\u{2077}");
    output = replace_exponent(&output, '8', "\u{2078}");
    output = replace_exponent(&output, '9', "\u{2079}");

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

    // ── from_reduce: Word boundary safety ─────────────────────────

    #[test]
    fn test_from_reduce_word_boundaries() {
        // "pi" inside "spin" must NOT be replaced
        assert_eq!(from_reduce("spin"), "spin");
        // "pi" inside "pineapple" must NOT be replaced
        assert_eq!(from_reduce("pineapple"), "pineapple");
        // "pi" inside "api" must NOT be replaced
        assert_eq!(from_reduce("api"), "api");
        // standalone "pi" should be replaced
        assert_eq!(from_reduce("2*pi"), "2*\u{03C0}");
        // "sqrt" inside "isqrt" must NOT be replaced
        assert_eq!(from_reduce("isqrt"), "isqrt");
    }

    #[test]
    fn test_from_reduce_exponent_boundaries() {
        // **29 must NOT become ²9
        assert_eq!(from_reduce("x**29"), "x**29");
        // **2 at end of string should be replaced
        assert_eq!(from_reduce("x**2"), "x\u{00B2}");
        // **2 followed by non-digit should be replaced
        assert_eq!(from_reduce("x**2 + 1"), "x\u{00B2} + 1");
        // **20 must NOT be replaced
        assert_eq!(from_reduce("x**20"), "x**20");
    }
}
