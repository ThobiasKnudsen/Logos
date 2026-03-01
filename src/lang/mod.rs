pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod wgsl_gen;

/// Compile source code through the full pipeline: lex → parse → WGSL gen.
/// Returns the complete WGSL shader source string.
pub fn compile(source: &str) -> Result<String, String> {
    let mut lex = lexer::Lexer::new(source);
    let tokens = lex.tokenize()?;
    let mut parser = parser::Parser::new(tokens);
    let ast = parser.parse()?;
    wgsl_gen::generate(&ast)
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_empty_input_compiles() {
        let result = compile("");
        assert!(result.is_ok());
        let shader = result.unwrap();
        assert!(shader.contains("fn fs_main"));
    }

    #[test]
    fn test_single_var_compiles() {
        let shader = compile("x").unwrap();
        assert!(shader.contains("let _result = x;"));
    }

    #[test]
    fn test_simple_math_compiles() {
        let shader = compile("x * x + y * y").unwrap();
        assert!(shader.contains("((x * x) + (y * y))"));
    }

    #[test]
    fn test_paraboloid() {
        // The "type x*x + y*y → see paraboloid" scenario from the plan
        let shader = compile("x * x + y * y").unwrap();
        assert!(shader.contains("@fragment"));
        assert!(shader.contains("let x = world.x"));
        assert!(shader.contains("let y = world.y"));
    }

    #[test]
    fn test_with_bindings_and_result() {
        let shader = compile("r: sqrt(x*x + y*y)\nsin(r * 10) / r").unwrap();
        assert!(shader.contains("let r = sqrt("));
        assert!(shader.contains("(sin((r * 10.0)) / r)"));
    }

    #[test]
    fn test_function_def_and_call() {
        let shader = compile("dist(a, b): sqrt(a*a + b*b)\ndist(x, y)").unwrap();
        assert!(shader.contains("fn dist(a: f32, b: f32) -> f32"));
        assert!(shader.contains("dist(x, y)"));
    }

    #[test]
    fn test_conditional_coloring() {
        let shader = compile("if (x*x + y*y < 1) 1 else 0").unwrap();
        assert!(shader.contains("select("));
    }

    #[test]
    fn test_trig_composition() {
        let shader = compile("sin(x * 3) * cos(y * 3)").unwrap();
        assert!(shader.contains("sin((x * 3.0))"));
        assert!(shader.contains("cos((y * 3.0))"));
    }

    #[test]
    fn test_time_animation() {
        let shader = compile("sin(x + time)").unwrap();
        assert!(shader.contains("let time = u.time"));
        assert!(shader.contains("sin((x + time))"));
    }

    #[test]
    fn test_nested_blocks() {
        let shader = compile("f(a): (b: a * 2, b + 1)\nf(x)").unwrap();
        assert!(shader.contains("fn f("));
    }

    #[test]
    fn test_equality_operator() {
        // `=` in Logos means equality, should compile to `==` in WGSL
        let shader = compile("x = 0").unwrap();
        assert!(shader.contains("(x == 0.0)"));
    }

    #[test]
    fn test_complex_mandelbrot_style() {
        let source = "r: x*x + y*y\nif (r < 4) (1 - r/4) else 0";
        let shader = compile(source).unwrap();
        assert!(shader.contains("let r ="));
        assert!(shader.contains("select("));
    }

    #[test]
    fn test_multiple_builtins() {
        let shader = compile("clamp(abs(sin(x * 6) - 0.5), 0, 1)").unwrap();
        assert!(shader.contains("clamp("));
        assert!(shader.contains("abs("));
        assert!(shader.contains("sin("));
    }

    #[test]
    fn test_whitespace_resilience() {
        // Various whitespace should compile identically
        let s1 = compile("x+y").unwrap();
        let s2 = compile("x + y").unwrap();
        let s3 = compile("  x  +  y  ").unwrap();
        // They should all contain the same expression
        assert!(s1.contains("(x + y)"));
        assert!(s2.contains("(x + y)"));
        assert!(s3.contains("(x + y)"));
    }
}
