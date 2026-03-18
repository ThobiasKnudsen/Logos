//! Translation between Logos Unicode math notation and REDUCE ASCII syntax.
//!
//! Logos uses Unicode symbols (π, ², √, etc.) while REDUCE uses ASCII
//! (pi, **2, sqrt(), etc.). This module converts between the two.

/// Word-level replacements: Logos keyword → REDUCE keyword.
const WORD_REPLACEMENTS: &[(&str, &str)] = &[
    ("integral", "int"),
    ("derivative", "df"),
];

/// Convert Logos Unicode math notation to REDUCE-compatible ASCII.
pub fn to_reduce(input: &str) -> String {
    // First pass: word-level replacements
    let mut working = input.to_string();
    for &(from, to) in WORD_REPLACEMENTS {
        working = replace_word(&working, from, to);
    }

    // Second pass: character-level Unicode → ASCII
    let mut output = String::with_capacity(working.len() * 2);
    let mut chars = working.chars().peekable();

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

            // Superscript digits → **N (REDUCE exponentiation)
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

            // Caret → REDUCE exponentiation
            '^' => output.push_str("**"),

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

/// Replace whole words only (surrounded by non-alphanumeric or string boundaries).
fn replace_word(input: &str, from: &str, to: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;
    while let Some(pos) = remaining.find(from) {
        let before_ok = pos == 0
            || !remaining[..pos]
                .chars()
                .last()
                .unwrap()
                .is_alphanumeric();
        let after = &remaining[pos + from.len()..];
        let after_ok = after.is_empty()
            || !after.chars().next().unwrap().is_alphanumeric();
        if before_ok && after_ok {
            result.push_str(&remaining[..pos]);
            result.push_str(to);
            remaining = after;
        } else {
            result.push_str(&remaining[..pos + from.len()]);
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

/// Convert REDUCE ASCII output back to Logos notation.
///
/// Converts `**` → `^` for exponentiation, and REDUCE keywords back to
/// Unicode symbols where appropriate.
pub fn from_reduce(input: &str) -> String {
    let mut output = input.to_string();

    // Word-level replacements (whole words only to avoid mangling substrings)
    output = replace_word(&output, "int", "integral");
    output = replace_word(&output, "df", "derivative");
    output = replace_word(&output, "pi", "\u{03C0}");
    output = replace_word(&output, "infinity", "\u{221E}");
    output = replace_word(&output, "sqrt", "\u{221A}");

    // ** → ^ (REDUCE exponentiation → Logos caret)
    output = output.replace("**", "^");

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
    }

    // ── to_reduce: Caret → ** ───────────────────────────────────

    #[test]
    fn test_caret_to_reduce() {
        assert_eq!(to_reduce("x^2"), "x**2");
        assert_eq!(to_reduce("x^n"), "x**n");
        assert_eq!(to_reduce("(a+b)^3"), "(a+b)**3");
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

    // ── from_reduce: Exponents (** → ^) ────────────────────────

    #[test]
    fn test_from_reduce_exponents() {
        assert_eq!(from_reduce("x**2"), "x^2");
        assert_eq!(from_reduce("x**3"), "x^3");
        assert_eq!(from_reduce("x**n"), "x^n");
        assert_eq!(from_reduce("x**10"), "x^10");
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
            "3*x^2 + \u{03C0}"
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
    fn test_roundtrip_caret() {
        // x^2 → x**2 (to_reduce) → x^2 (from_reduce)
        assert_eq!(to_reduce("x^2"), "x**2");
        assert_eq!(from_reduce("x**2"), "x^2");
    }

    #[test]
    fn test_roundtrip_superscript() {
        // x² → x**2 (to_reduce) → x^2 (from_reduce)
        // Note: superscripts convert to ** for REDUCE, and back to ^ (not ²)
        let ascii = to_reduce("x\u{00B2}");
        assert_eq!(ascii, "x**2");
        let back = from_reduce(&ascii);
        assert_eq!(back, "x^2");
    }

    // ── Edge cases ───────────────────────────────────────────────

    #[test]
    fn test_empty_input() {
        assert_eq!(to_reduce(""), "");
        assert_eq!(from_reduce(""), "");
    }
}
