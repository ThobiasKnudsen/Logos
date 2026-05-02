//! Lower-level integration tests for the Logos compile pipeline:
//! lex → parse → wgsl_gen → naga validate. These tests bypass the
//! `Notebook` engine on purpose so they can exercise bare expressions
//! (the fixtures pre-date `plot()`/`print()` actions and rely on
//! `lang::compile`'s "wrap any expression as a fragment shader" default).
//!
//! For full notebook-pipeline integration tests (action routing, REDUCE
//! round-trips, sync), see `src/notebook/tests.rs`.

use serde::Deserialize;

#[derive(Deserialize)]
struct Notebook {
    cells: Vec<Cell>,
}

#[derive(Deserialize)]
struct Cell {
    code: String,
    #[allow(dead_code)]
    color: String,
}

/// Load a notebook fixture and return its cells.
fn load_fixture(name: &str) -> Notebook {
    let path = format!("tests/fixtures/{}", name);
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path, e));
    serde_json::from_str(&contents)
        .unwrap_or_else(|e| panic!("Failed to parse fixture {}: {}", path, e))
}

/// Concatenate cell code from 0..=cell_index (with newlines between cells).
fn concat_cells(cells: &[Cell], cell_index: usize) -> String {
    let mut source = String::new();
    for (i, cell) in cells.iter().enumerate() {
        if i > cell_index {
            break;
        }
        if !source.is_empty() {
            source.push('\n');
        }
        source.push_str(&cell.code);
    }
    source
}

/// Validate a WGSL shader string using naga's WGSL frontend.
/// This is the same validation wgpu performs before sending to the GPU.
fn validate_wgsl(wgsl: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(wgsl).map_err(|e| {
        format!(
            "naga WGSL parse error: {}\n\n--- Generated WGSL ---\n{}",
            e, wgsl
        )
    })?;

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator.validate(&module).map_err(|e| {
        format!(
            "naga validation error: {}\n\n--- Generated WGSL ---\n{}",
            e, wgsl
        )
    })?;

    Ok(())
}

/// Full pipeline: compile + validate. Panics with the generated WGSL on failure.
fn compile_and_validate(cells: &[Cell], cell_index: usize) -> String {
    let source = concat_cells(cells, cell_index);
    let wgsl = crate::lang::compile(&source).unwrap_or_else(|e| {
        panic!(
            "Logos compile failed for cells 0..={}:\n  source: {:?}\n  error: {}",
            cell_index, source, e
        )
    });
    validate_wgsl(&wgsl).unwrap_or_else(|e| {
        panic!(
            "WGSL validation failed for cells 0..={}:\n  source: {:?}\n  {}",
            cell_index, source, e
        )
    });
    wgsl
}

/// Compile + validate the full notebook (all cells). Returns the WGSL shader.
fn compile_notebook(name: &str) -> String {
    let notebook = load_fixture(name);
    assert!(!notebook.cells.is_empty(), "Notebook {} has no cells", name);
    let last = notebook.cells.len() - 1;
    compile_and_validate(&notebook.cells, last)
}

/// Run lex → parse → WGSL gen → naga validate as separate stages, printing
/// intermediate output on failure to pinpoint which stage broke.
fn compile_stages(source: &str) -> String {
    // Stage 1: Lex
    let mut lexer = crate::lang::lexer::Lexer::new(source);
    let tokens = lexer
        .tokenize()
        .unwrap_or_else(|e| panic!("LEXER FAILED\n  source: {:?}\n  error: {}", source, e));
    eprintln!("  lex: {} tokens", tokens.len());

    // Stage 2: Parse
    let mut parser = crate::lang::parser::Parser::new(tokens, source.to_string());
    let ir = parser
        .parse()
        .unwrap_or_else(|e| panic!("PARSER FAILED\n  source: {:?}\n  error: {}", source, e));
    eprintln!("  parse: ok (IR: {:?})", std::mem::discriminant(&ir));

    // Stage 3: WGSL codegen
    let wgsl = crate::lang::wgsl_gen::generate(&ir)
        .unwrap_or_else(|e| panic!("WGSL GEN FAILED\n  source: {:?}\n  error: {}", source, e));
    eprintln!("  wgsl gen: {} bytes", wgsl.len());

    // Stage 4: naga WGSL validation
    validate_wgsl(&wgsl)
        .unwrap_or_else(|e| panic!("NAGA VALIDATION FAILED\n  source: {:?}\n  {}", source, e));
    eprintln!("  naga validate: ok");

    wgsl
}

// ===========================================================================
// Single-cell tests
// ===========================================================================

#[test]
fn test_circle_equation() {
    let shader = compile_notebook("circle.json");
    assert!(shader.contains("x_m"), "circle should use corner checking");
    assert!(shader.contains("!("), "equality uses straddle check");
    assert!(shader.contains("select(0.0, 1.0, _result)"));
    assert!(
        !shader.contains("\u{00B2}"),
        "Unicode ² should be compiled away"
    );
}

#[test]
fn test_simple_numeric() {
    let shader = compile_notebook("simple_numeric.json");
    assert!(shader.contains("sin(x)"));
    assert!(shader.contains("cos(y)"));
    assert!(
        shader.contains("clamp(_result, 0.0, 1.0)"),
        "numeric → clamp"
    );
}

#[test]
fn test_paraboloid() {
    let shader = compile_notebook("paraboloid.json");
    assert!(shader.contains("((x * x) + (y * y))"));
    assert!(shader.contains("@fragment"));
    assert!(shader.contains("let x = world.x"));
    assert!(shader.contains("let y = world.y"));
}

#[test]
fn test_binding_and_trig() {
    let shader = compile_notebook("binding_and_trig.json");
    assert!(shader.contains("let r = sqrt("));
    assert!(shader.contains("sin((r * 10.0))"));
}

#[test]
fn test_conditional_coloring() {
    let shader = compile_notebook("conditional_coloring.json");
    assert!(shader.contains("let r = sqrt("));
    assert!(shader.contains("select("), "if/else compiles to select");
}

#[test]
fn test_time_animation() {
    let shader = compile_notebook("time_animation.json");
    assert!(shader.contains("sin("));
    assert!(shader.contains("cos("));
}

#[test]
fn test_inequality_region() {
    let shader = compile_notebook("inequality_region.json");
    assert!(shader.contains("x_m"), "boolean should use corner checking");
    assert!(shader.contains("&&"), "and + inequality → ANDed corners");
    assert!(shader.contains("select(0.0, 1.0, _result)"));
}

#[test]
fn test_multi_binding_chain() {
    let shader = compile_notebook("multi_binding_chain.json");
    assert!(shader.contains("let a = (x * 2.0)"));
    assert!(shader.contains("let b = (y * 3.0)"));
    assert!(shader.contains("let c = (a + b)"));
    assert!(shader.contains("sin(c)"));
}

#[test]
fn test_tuple_color_output() {
    let shader = compile_notebook("tuple_color_output.json");
    assert!(shader.contains("vec3<f32>(sin(x), cos(y), 0.5)"));
}

#[test]
fn test_ellipse() {
    let shader = compile_notebook("ellipse.json");
    // Pure numeric bindings are emitted as module-scope constants
    assert!(shader.contains("const a = 3.0"));
    assert!(shader.contains("const b = 2.0"));
    assert!(shader.contains("x_m"), "equality should trigger corners");
    assert!(shader.contains("!("), "straddle check");
}

// ===========================================================================
// Multi-cell tests
// ===========================================================================

#[test]
fn test_func_def_then_call() {
    let notebook = load_fixture("func_def_call.json");

    // Cell 0 alone: f(x): x² — function def compiles on its own
    let shader0 = compile_and_validate(&notebook.cells, 0);
    assert!(
        shader0.contains("fn f(x: f32) -> f32"),
        "cell 0 should define f"
    );

    // Cell 1 with cell 0: y = f(x) — boolean equality with corner checking
    let shader1 = compile_and_validate(&notebook.cells, 1);
    assert!(
        shader1.contains("fn f(x: f32) -> f32"),
        "f should still be defined"
    );
    assert!(shader1.contains("x_m"), "y=f(x) is boolean → corners");
    assert!(
        shader1.contains("f(x_m)") || shader1.contains("f(x_p)"),
        "f should be called with corner values"
    );
}

#[test]
fn test_func_compose_and_plot() {
    let notebook = load_fixture("func_compose_plot.json");

    let shader = compile_and_validate(&notebook.cells, 2);
    assert!(shader.contains("fn f("), "f should be defined");
    assert!(shader.contains("fn F("), "F should be defined");
    assert!(shader.contains("x_m"), "y=F(x) is boolean → corners");
}

#[test]
fn test_nested_functions() {
    let notebook = load_fixture("nested_functions.json");

    let shader = compile_and_validate(&notebook.cells, 2);
    assert!(shader.contains("fn square(a: f32) -> f32"));
    assert!(shader.contains("fn dist(a: f32, b: f32) -> f32"));
    assert!(shader.contains("dist(x, y)"));
    assert!(
        shader.contains("clamp(_result, 0.0, 1.0)"),
        "numeric result"
    );
}

#[test]
fn test_multiple_independent_plots() {
    let notebook = load_fixture("multiple_plots.json");

    // Cell 0: y = sin(x)
    let shader0 = compile_and_validate(&notebook.cells, 0);
    assert!(shader0.contains("x_m"), "y=sin(x) is boolean");
    assert!(shader0.contains("sin(x_m)") || shader0.contains("sin(x_p)"));

    // Cell 1: y = cos(x) — accumulated with cell 0
    let shader1 = compile_and_validate(&notebook.cells, 1);
    assert!(shader1.contains("x_m"));
}

#[test]
fn test_complex_multi_cell_glow() {
    let notebook = load_fixture("complex_multi_cell.json");

    let shader = compile_and_validate(&notebook.cells, 2);
    assert!(shader.contains("fn glow("));
    assert!(shader.contains("let r = sqrt("));
    assert!(shader.contains("glow((r - 2.0))"));
}

// ===========================================================================
// Incremental compilation tests
// ===========================================================================

#[test]
fn test_incremental_func_def_call() {
    let notebook = load_fixture("func_def_call.json");

    // Cell 0: "f(x): x²" — single function def compiles and validates
    let shader0 = compile_and_validate(&notebook.cells, 0);
    assert!(shader0.contains("fn f("));

    // Cell 1: adds "y = f(x)"
    let shader1 = compile_and_validate(&notebook.cells, 1);
    assert!(shader1.contains("fn f("));
    assert!(shader1.contains("x_m"), "y=f(x) is boolean → corners");
}

#[test]
fn test_incremental_nested_functions() {
    let notebook = load_fixture("nested_functions.json");

    // Each progressive accumulation should at least lex and parse correctly
    for i in 0..notebook.cells.len() {
        let source = concat_cells(&notebook.cells, i);
        let mut lexer = crate::lang::lexer::Lexer::new(&source);
        let tokens = lexer.tokenize();
        assert!(
            tokens.is_ok(),
            "Lex failed at cell {}: {:?}",
            i,
            tokens.err()
        );
        let mut parser = crate::lang::parser::Parser::new(tokens.unwrap(), source.clone());
        let ast = parser.parse();
        assert!(ast.is_ok(), "Parse failed at cell {}: {:?}", i, ast.err());
    }

    // Final: full compile + naga validate
    let shader = compile_and_validate(&notebook.cells, 2);
    assert!(shader.contains("fn square("));
    assert!(shader.contains("fn dist("));
    assert!(shader.contains("dist(x, y)"));
}

// ===========================================================================
// Stage-by-stage tests (print each stage's result for diagnostics)
// ===========================================================================

#[test]
fn test_stages_circle() {
    let notebook = load_fixture("circle.json");
    eprintln!("--- circle.json stage-by-stage ---");
    let wgsl = compile_stages(&notebook.cells[0].code);
    assert!(wgsl.contains("x_m"));
    assert!(wgsl.contains("!("));
}

#[test]
fn test_stages_func_compose() {
    let notebook = load_fixture("func_compose_plot.json");
    let source = concat_cells(&notebook.cells, notebook.cells.len() - 1);
    eprintln!("--- func_compose_plot.json stage-by-stage ---");
    let wgsl = compile_stages(&source);
    assert!(wgsl.contains("fn f("));
    assert!(wgsl.contains("fn F("));
}

// ===========================================================================
// Catch-all: every fixture compiles and produces valid WGSL
// ===========================================================================

/// Snapshot of fixtures that are expected to compile cleanly. If you add a
/// fixture, add it here so removals/renames are caught instead of silently
/// passing `count >= N`.
const EXPECTED_FIXTURES: &[&str] = &[
    "binding_and_trig.json",
    "circle.json",
    "complex_multi_cell.json",
    "conditional_coloring.json",
    "ellipse.json",
    "func_compose_plot.json",
    "func_def_call.json",
    "inequality_region.json",
    "monte_carlo_pi.json",
    "multi_binding_chain.json",
    "multiple_plots.json",
    "nested_functions.json",
    "paraboloid.json",
    "simple_numeric.json",
    "time_animation.json",
    "tuple_color_output.json",
];

#[test]
fn test_all_fixtures_compile_and_validate() {
    let fixtures_dir =
        std::fs::read_dir("tests/fixtures").expect("Failed to read tests/fixtures directory");

    let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in fixtures_dir {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            let notebook = load_fixture(&name);
            let last = notebook.cells.len() - 1;

            // Full pipeline: Logos compile
            let source = concat_cells(&notebook.cells, last);
            let wgsl = crate::lang::compile(&source).unwrap_or_else(|e| {
                panic!(
                    "Fixture {} Logos compile failed:\n  source: {:?}\n  error: {}",
                    name, source, e
                )
            });

            // Structural checks
            assert!(wgsl.contains("@fragment"), "{} missing @fragment", name);
            assert!(wgsl.contains("fn fs_main"), "{} missing fs_main", name);
            assert!(
                wgsl.contains("struct Uniforms"),
                "{} missing Uniforms struct",
                name
            );
            assert!(
                wgsl.contains("var<uniform> u: Uniforms"),
                "{} missing uniform binding",
                name
            );

            // naga WGSL validation — proves the shader would compile on the GPU
            validate_wgsl(&wgsl)
                .unwrap_or_else(|e| panic!("Fixture {} naga validation failed:\n  {}", name, e));

            eprintln!(
                "  {} ok ({} cells, {} bytes WGSL)",
                name,
                notebook.cells.len(),
                wgsl.len()
            );
            found.insert(name);
        }
    }

    let expected: std::collections::BTreeSet<String> =
        EXPECTED_FIXTURES.iter().map(|s| s.to_string()).collect();
    let missing: Vec<&String> = expected.difference(&found).collect();
    let extra: Vec<&String> = found.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "fixture set drift — missing: {:?}, extra: {:?}\n\
         If you intentionally added/removed a fixture, update EXPECTED_FIXTURES.",
        missing,
        extra
    );
}
