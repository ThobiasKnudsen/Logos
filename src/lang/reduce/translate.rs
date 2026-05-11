//! Translation between Logos Unicode math notation and REDUCE ASCII syntax.
//!
//! Logos uses Unicode symbols (π, ², √, etc.) while REDUCE uses ASCII
//! (pi, **2, sqrt(), etc.). This module converts between the two.

use crate::lang::ir::Ir;

/// Word-level replacements: Logos keyword → REDUCE keyword.
const WORD_REPLACEMENTS: &[(&str, &str)] = &[("integral", "int"), ("derivative", "df")];

/// Convert Logos IR to REDUCE-compatible ASCII syntax.
///
/// Pretty-prints the IR back to Logos source via `Ir::to_source`, then runs
/// it through `to_reduce` to convert Unicode operators (π, ², √, ^) into
/// the REDUCE ASCII forms (pi, **2, sqrt(), **). Used by `ReduceSimplifier`
/// to send IR-shaped requests to the underlying REDUCE worker.
pub fn ir_to_reduce(ir: &Ir) -> String {
    to_reduce(&ir.to_source())
}

/// Convert Logos Unicode math notation to REDUCE-compatible ASCII.
pub fn to_reduce(input: &str) -> String {
    // First pass: word-level replacements
    let mut working = input.to_string();
    for &(from, to) in WORD_REPLACEMENTS {
        working = replace_word(&working, from, to);
    }

    // Second pass: character-level Unicode → ASCII
    let mut output = String::with_capacity(working.len() * 2);
    let chars = working.chars().peekable();

    for ch in chars {
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
            '\u{03C4}' => output.push_str("tau"), // τ
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

            // Euler's number
            '\u{212F}' => output.push('e'), // ℯ

            // Math operators
            '\u{00D7}' => output.push('*'),        // ×
            '\u{00F7}' => output.push('/'),        // ÷
            '\u{2212}' => output.push('-'),        // − (minus sign)
            '\u{221A}' => output.push_str("sqrt"), // √

            // Math functions / operators
            '\u{222B}' => output.push_str("int"), // ∫ (integral)
            '\u{2202}' => output.push_str("df"),  // ∂ (partial derivative)
            '\u{2146}' => output.push_str("df"),  // ⅆ (derivative)
            '\u{2207}' => output.push_str("nabla"), // ∇ (nabla/gradient)

            // Summation / product (these need context, basic stubs)
            '\u{2211}' => output.push_str("sum"),  // ∑
            '\u{220F}' => output.push_str("prod"), // ∏

            // Infinity
            '\u{221E}' => output.push_str("infinity"),

            // Additional Greek letters (uppercase)
            '\u{0393}' => output.push_str("Gamma"),  // Γ
            '\u{0394}' => output.push_str("Delta"),  // Δ
            '\u{0398}' => output.push_str("Theta"),  // Θ
            '\u{039B}' => output.push_str("Lambda"), // Λ
            '\u{039E}' => output.push_str("Xi"),     // Ξ
            '\u{03A0}' => output.push_str("Pi"),     // Π (uppercase Pi)
            '\u{03A3}' => output.push_str("Sigma"),  // Σ
            '\u{03A6}' => output.push_str("Phi"),    // Φ
            '\u{03A8}' => output.push_str("Psi"),    // Ψ
            '\u{03A9}' => output.push_str("Omega"),  // Ω

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
            '\u{2264}' => output.push_str("<="), // ≤
            '\u{2265}' => output.push_str(">="), // ≥
            '\u{2260}' => output.push_str("!="), // ≠

            // Arrows (pass as identifiers for now)
            '\u{2192}' => output.push_str("arrow_right"), // →
            '\u{2190}' => output.push_str("arrow_left"),  // ←

            // Only pass through REDUCE-safe ASCII characters.
            // Strip backslashes and unmapped non-ASCII to prevent C++ exceptions.
            _ => {
                if ch.is_ascii_alphanumeric()
                    || matches!(
                        ch,
                        ' ' | '+'
                            | '-'
                            | '*'
                            | '/'
                            | '^'
                            | '('
                            | ')'
                            | ','
                            | '.'
                            | ';'
                            | '$'
                            | ':'
                            | '='
                            | '<'
                            | '>'
                            | '!'
                            | '_'
                            | '\''
                            | '"'
                            | '\n'
                            | '\t'
                    )
                {
                    output.push(ch);
                }
                // else: silently drop (backslash, @, #, non-ASCII not in map, etc.)
            }
        }
    }

    output
}

/// Check whether the character at the given position is an ASCII alphanumeric
/// or underscore — i.e. part of an identifier.
fn is_word_char(s: &str, byte_pos: usize) -> bool {
    s.as_bytes()
        .get(byte_pos)
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
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

/// Special functions that REDUCE may return but Logos cannot evaluate.
/// Each entry is (REDUCE name, human-readable description).
const SPECIAL_FUNCTIONS: &[(&str, &str)] = &[
    // Fresnel integrals
    ("fresnel_s", "Fresnel S"),
    ("fresnel_c", "Fresnel C"),
    // Error functions
    ("erf", "error function"),
    ("erfc", "complementary error function"),
    ("erfi", "imaginary error function"),
    // Integral functions
    ("si", "sine integral"),
    ("ci", "cosine integral"),
    ("ei", "exponential integral"),
    ("li", "logarithmic integral"),
    ("dilog", "dilogarithm"),
    ("polylog", "polylogarithm"),
    // Bessel functions
    ("besseli", "Bessel I"),
    ("besselj", "Bessel J"),
    ("besselk", "Bessel K"),
    ("bessely", "Bessel Y"),
    ("hankel1", "Hankel H₁"),
    ("hankel2", "Hankel H₂"),
    // Airy functions
    ("airy_ai", "Airy Ai"),
    ("airy_bi", "Airy Bi"),
    // Gamma / beta
    ("gamma", "gamma function"),
    ("beta", "beta function"),
    ("polygamma", "polygamma"),
    ("psi", "digamma"),
    // Hypergeometric
    ("hypergeometric", "hypergeometric"),
    // Elliptic integrals
    ("elliptice", "elliptic E"),
    ("elliptick", "elliptic K"),
    ("ellipticf", "elliptic F"),
    // Zeta
    ("zeta", "Riemann zeta"),
    // Lambert W
    ("lambert_w", "Lambert W"),
    // Whittaker
    ("whittakerm", "Whittaker M"),
    ("whittakerw", "Whittaker W"),
];

/// Unevaluated CAS operations that REDUCE returns when it cannot solve
/// a problem. Each entry is (REDUCE keyword, human-readable operation).
const UNEVALUATED_OPS: &[(&str, &str)] = &[("int", "integral"), ("df", "derivative")];

/// CAS function names users can write in Logos source. Mirrors the prior
/// text-level CAS-call list so the IR-walking detector finds the same set
/// of subtrees `find_cas_call` (text) used to locate.
const CAS_CALLEES: &[&str] = &[
    "int",
    "df",
    "integral",
    "derivative",
    "limit",
    "solve",
    "diff",
];

/// True if `callee` is a `User`-typed CAS function — i.e. something that
/// should be sent to the symbolic simplifier rather than evaluated by the
/// interpreter or lowered to WGSL.
pub fn is_cas_callee(callee: &crate::lang::ir::Callee) -> bool {
    if let crate::lang::ir::Callee::User(name) = callee {
        CAS_CALLEES.contains(&name.as_str())
    } else {
        false
    }
}

/// Find the path to the first CAS-call subtree in `ir`, or `None` if none.
/// CAS calls are `Apply { callee: User("int"|"df"|"integral"|"derivative"), … }`.
pub fn find_cas_path(ir: &Ir) -> Option<Vec<usize>> {
    ir.find_path(&mut |node| {
        matches!(node, Ir::Apply { callee, .. } if is_cas_callee(callee))
    })
}

/// IR-level analogue of `detect_unevaluated_cas`: returns the human-readable
/// name of the first residual CAS subtree (`int(...)` or `df(...)`) in the
/// simplifier's response IR, indicating REDUCE couldn't solve it.
pub fn detect_unevaluated_cas_ir(ir: &Ir) -> Option<&'static str> {
    let path = ir.find_path(&mut |node| {
        if let Ir::Apply { callee, .. } = node {
            if let crate::lang::ir::Callee::User(name) = callee {
                return matches!(name.as_str(), "int" | "df");
            }
        }
        false
    })?;
    let node = ir.get_at_path(&path)?;
    if let Ir::Apply { callee: crate::lang::ir::Callee::User(name), .. } = node {
        for &(keyword, description) in UNEVALUATED_OPS {
            if name == keyword {
                return Some(description);
            }
        }
    }
    None
}

/// IR-level analogue of `detect_special_functions`: walks the simplifier's
/// response IR for `Apply { callee: User(name), … }` where `name` matches
/// any entry in `SPECIAL_FUNCTIONS`. Returns the matched (name, description)
/// pairs.
pub fn detect_special_functions_ir(ir: &Ir) -> Vec<(&'static str, &'static str)> {
    let mut found: Vec<(&'static str, &'static str)> = Vec::new();
    visit_idents_and_callees(ir, &mut |name| {
        let lower = name.to_ascii_lowercase();
        for &(sf_name, sf_desc) in SPECIAL_FUNCTIONS {
            if lower == sf_name && !found.iter().any(|(n, _)| *n == sf_name) {
                found.push((sf_name, sf_desc));
            }
        }
    });
    found
}

fn visit_idents_and_callees(ir: &Ir, f: &mut dyn FnMut(&str)) {
    match ir {
        Ir::Identifier { name, .. } => f(name),
        Ir::Apply { callee, args, .. } => {
            f(callee.name());
            for a in args {
                visit_idents_and_callees(a, f);
            }
        }
        _ => {
            for c in ir.children() {
                visit_idents_and_callees(c, f);
            }
        }
    }
}

/// Convert REDUCE ASCII output back to Logos notation.
///
/// Converts `**` → `^` for exponentiation (Logos parser uses `^`),
/// and REDUCE keywords back to Unicode symbols where appropriate.
pub fn from_reduce(input: &str) -> String {
    let mut output = input.to_string();

    // Word-boundary-safe replacements (longest words first to avoid partial matches)
    output = replace_word(&output, "infinity", "\u{221E}");
    output = replace_word(&output, "sqrt", "\u{221A}");
    output = replace_word(&output, "tau", "\u{03C4}");
    output = replace_word(&output, "pi", "\u{03C0}");
    output = replace_word(&output, "e", "\u{212F}");

    // ** → ^ (REDUCE exponentiation → Logos caret)
    output = output.replace("**", "^");

    // REDUCE's infix printer emits "a + -b" / "a +  - b" for "a - b".
    // Collapse "+ <whitespace> -" into "-" so output reads naturally.
    output = collapse_plus_minus(&output);

    output
}

/// REDUCE's printer always emits "+" before the next operand; when that operand
/// is negative we get clunky "a + -b" or "a +  - b" sequences. Rewrite them as
/// "a - b" while preserving everything else.
fn collapse_plus_minus(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '+' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            if j < chars.len() && chars[j] == '-' {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push('-');
                let mut k = j + 1;
                while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t') {
                    k += 1;
                }
                if k < chars.len() {
                    out.push(' ');
                }
                i = k;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapse_plus_minus_basic() {
        assert_eq!(collapse_plus_minus("a + -b"), "a - b");
        assert_eq!(collapse_plus_minus("a +  - b"), "a - b");
        assert_eq!(collapse_plus_minus("3*x +  - y^2"), "3*x - y^2");
        assert_eq!(collapse_plus_minus("a + b"), "a + b"); // unchanged
        assert_eq!(collapse_plus_minus("a - b"), "a - b"); // unchanged
    }

    #[test]
    fn test_from_reduce_collapses_plus_minus() {
        let out = from_reduce("3*x +  - y**2");
        assert_eq!(out, "3*x - y^2");
    }

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
        assert_eq!(to_reduce("x \u{00D7} y"), "x * y"); // ×
        assert_eq!(to_reduce("x \u{00F7} y"), "x / y"); // ÷
        assert_eq!(to_reduce("x \u{2212} y"), "x - y"); // − (minus sign)
        assert_eq!(to_reduce("\u{221A}(x)"), "sqrt(x)"); // √
    }

    // ── to_reduce: Special symbols ───────────────────────────────

    #[test]
    fn test_special_symbols_to_reduce() {
        assert_eq!(to_reduce("\u{2211}"), "sum"); // ∑
        assert_eq!(to_reduce("\u{220F}"), "prod"); // ∏
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
        assert_eq!(to_reduce("\u{03C0} \u{00D7} r\u{00B2}"), "pi * r**2");
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
        assert_eq!(from_reduce("3*x**2 + pi"), "3*x^2 + \u{03C0}");
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
    fn test_from_reduce_exponents_to_caret() {
        // ** → ^ for all exponents
        assert_eq!(from_reduce("x**2"), "x^2");
        assert_eq!(from_reduce("x**29"), "x^29");
        assert_eq!(from_reduce("x**2 + 1"), "x^2 + 1");
    }

    // ── Complex expression: x²*y²+sin(x)³-sin(y²)²=9 ──────────

    #[test]
    fn test_complex_expression_to_reduce() {
        let input = "x\u{00B2}*y\u{00B2}+sin(x)\u{00B3}-sin(y\u{00B2})\u{00B2}=9";
        let ascii = to_reduce(input);
        assert_eq!(ascii, "x**2*y**2+sin(x)**3-sin(y**2)**2=9");
    }

    #[test]
    fn test_complex_expression_from_reduce() {
        let reduce_out = "x**2*y**2 + sin(x)**3 - sin(y**2)**2";
        let result = from_reduce(reduce_out);
        assert_eq!(result, "x^2*y^2 + sin(x)^3 - sin(y^2)^2");
    }

    #[test]
    fn test_complex_expression_roundtrip() {
        // x²*y² → x**2*y**2 → x^2*y^2 (superscripts → ** → ^)
        let input = "x\u{00B2}*y\u{00B2}+sin(x)\u{00B3}-sin(y\u{00B2})\u{00B2}";
        let ascii = to_reduce(input);
        assert_eq!(ascii, "x**2*y**2+sin(x)**3-sin(y**2)**2");
        let back = from_reduce(&ascii);
        assert_eq!(back, "x^2*y^2+sin(x)^3-sin(y^2)^2");
    }

    fn parse_ir(s: &str) -> Ir {
        crate::lang::parse(s).expect("parse")
    }

    // ── detect_special_functions_ir ──────────────────────────────

    #[test]
    fn test_detect_fresnel() {
        let ir = parse_ir("fresnel_s(\u{221A}(2)*x)");
        let result = detect_special_functions_ir(&ir);
        assert!(result.iter().any(|(n, _)| *n == "fresnel_s"));
    }

    #[test]
    fn test_detect_erf() {
        let ir = parse_ir("erf(x)");
        let result = detect_special_functions_ir(&ir);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "erf");
    }

    #[test]
    fn test_detect_none_for_elementary() {
        let ir = parse_ir("sin(x)^2 + cos(x)^2");
        let result = detect_special_functions_ir(&ir);
        assert!(result.is_empty());
    }

    #[test]
    fn test_detect_no_false_positive_on_substrings() {
        // "erf" must not match the identifier names "serf" or "erfurt".
        let ir = parse_ir("serf(x) + erfurt");
        let result = detect_special_functions_ir(&ir);
        assert!(result.is_empty());
    }

    #[test]
    fn test_detect_multiple() {
        let ir = parse_ir("fresnel_s(x) + erf(y) + besselj(0, z)");
        let result = detect_special_functions_ir(&ir);
        let names: Vec<&str> = result.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"fresnel_s"));
        assert!(names.contains(&"erf"));
        assert!(names.contains(&"besselj"));
    }

    // ── detect_unevaluated_cas_ir ────────────────────────────────

    #[test]
    fn test_detect_unevaluated_int() {
        let ir = parse_ir("int(sin(cos(x)),x)");
        assert_eq!(detect_unevaluated_cas_ir(&ir), Some("integral"));
    }

    #[test]
    fn test_detect_unevaluated_df() {
        let ir = parse_ir("df(foo(x),x)");
        assert_eq!(detect_unevaluated_cas_ir(&ir), Some("derivative"));
    }

    #[test]
    fn test_detect_unevaluated_none_for_solved() {
        let ir = parse_ir("sin(x)^2 + cos(x)");
        assert_eq!(detect_unevaluated_cas_ir(&ir), None);
    }

    #[test]
    fn test_detect_unevaluated_no_false_positive() {
        // `interval` / `mint` are identifiers, not `int(...)` calls.
        let ir = parse_ir("interval + mint");
        assert_eq!(detect_unevaluated_cas_ir(&ir), None);
        // `pdf` / `adf` — same.
        let ir = parse_ir("pdf + adf");
        assert_eq!(detect_unevaluated_cas_ir(&ir), None);
    }

    // ── User-function inlining before REDUCE submission ──────────
    //
    // When the user writes
    //     e := 2.718
    //     f(x) := x*e^(x²)
    //     F(x) := ∫(f(x), x)
    //     plot(y = F(x))
    // the notebook's `submit_cas_at` calls `substitute_idents` on the
    // CAS subtree with `e` and `f` in scope, then ships the result to
    // REDUCE. Real REDUCE rejects an `f(x)` reference with
    //     "Declare f operator? (Y or N)"
    // because `substitute_idents` (src/lang/ir.rs:617) only replaces
    // `Ir::Identifier` nodes — it does NOT beta-reduce
    // `Ir::Apply { callee: User("f"), … }` against `f`'s FunctionDef.
    //
    // This test pins the expected post-substitution shape: the
    // submitted REDUCE text must not contain a bare `f(` call. It
    // currently FAILS — see issue #16 for the fix (either beta-reduce
    // user-function calls during substitution, or have the translator
    // emit `operator f, F;` declarations in the context).

    #[test]
    fn user_function_call_is_inlined_before_reduce_submission() {
        use crate::lang::ir::Ir;

        // Build the CAS subtree and the bindings the way `submit_cas_at`
        // does — without going through the notebook plumbing.
        let cas_subtree = parse_ir("int(f(x), x)");
        let f_def = Ir::FunctionDef {
            name: "f".to_string(),
            params: vec!["x".to_string()],
            body: Box::new(parse_ir("x*e^(x^2)")),
            span: (0, 0),
        };
        let bindings = vec![
            (
                "e".to_string(),
                Ir::Number {
                    value: 2.718,
                    span: (0, 0),
                },
            ),
            ("f".to_string(), f_def),
        ];

        let submitted_ir = cas_subtree.substitute_idents(&bindings);
        let submitted_text = submitted_ir.to_source();
        let reduce_text = to_reduce(&submitted_text);

        // What REDUCE actually sees:
        //   bug-present: "int(f(x), x)"  → REDUCE asks to declare `f` an operator
        //   bug-fixed:   "int(x*2.718**(x**2), x)"  → REDUCE integrates.
        assert!(
            !reduce_text.contains("f("),
            "user-defined function `f` must be inlined before REDUCE submission;\n\
             got submitted text: {}",
            reduce_text,
        );
    }
}
