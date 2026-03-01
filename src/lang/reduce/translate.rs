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

    #[test]
    fn test_superscript_to_reduce() {
        assert_eq!(to_reduce("x\u{00B2}"), "x**2");
        assert_eq!(to_reduce("x\u{00B3}"), "x**3");
    }

    #[test]
    fn test_pi_to_reduce() {
        assert_eq!(to_reduce("\u{03C0}"), "pi");
    }

    #[test]
    fn test_passthrough() {
        assert_eq!(to_reduce("x + y * 2"), "x + y * 2");
    }

    #[test]
    fn test_from_reduce_exponents() {
        assert_eq!(from_reduce("x**2"), "x\u{00B2}");
        assert_eq!(from_reduce("x**3"), "x\u{00B3}");
    }
}
