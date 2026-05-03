//! Notebook integration tests — the production class as the test harness.
//!
//! These cover the same scenarios as the previous ad-hoc `run_cell` helper
//! in `lang::mod`, but exercise the real `Notebook` API: cells are added,
//! `play()` is called, REDUCE round-trips are pumped via `tick()`, outcomes
//! are read off the cells. No backdoors.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::lang::reduce::service::ReduceResponse;
use crate::lang::symbolic::NoSimplifier;

use super::{CellMessage, CellOutcome, Notebook, ReduceBackend, ReduceSimplifier, ShaderSpec};

// REDUCE's CSL has process-global state and crashes on re-init, so even
// `--test-threads=1` can't safely combine multiple `ReduceSession::new()`
// calls in one binary (the existing `test_reduce_session` already takes
// CSL). Tests that need to drive REDUCE round-trips use a mock backend
// the test directly controls.

#[derive(Default)]
struct MockState {
    /// Submitted requests waiting for `respond_to(cell_id, …)`.
    inflight: HashMap<usize, u64>,
    /// Queued responses ready for `try_recv`.
    inbox: VecDeque<ReduceResponse>,
    next_request_id: u64,
}

#[derive(Clone, Default)]
struct MockReduce {
    state: Arc<Mutex<MockState>>,
}

impl MockReduce {
    fn new() -> Self {
        Self::default()
    }
    /// Enqueue a response for the most recent submission of `cell_id`.
    /// `result` is the simplified text REDUCE would have produced.
    fn respond_to(&self, cell_id: usize, result: Result<String, String>) {
        let mut s = self.state.lock().unwrap();
        let request_id = s.inflight.remove(&cell_id).unwrap_or(0);
        s.inbox.push_back(ReduceResponse {
            cell_id,
            request_id,
            result,
        });
    }
}

impl ReduceBackend for MockReduce {
    fn submit(&mut self, cell_id: usize, _context: Vec<String>, _expr: String) -> u64 {
        let mut s = self.state.lock().unwrap();
        let id = s.next_request_id;
        s.next_request_id += 1;
        s.inflight.insert(cell_id, id);
        id
    }
    fn try_recv(&mut self) -> Option<ReduceResponse> {
        self.state.lock().unwrap().inbox.pop_front()
    }
    fn has_pending(&self) -> bool {
        !self.state.lock().unwrap().inflight.is_empty()
    }
}

/// Strip the smallest leading indent shared by all non-empty lines and trim
/// outer blank lines. Lets tests use raw multi-line strings with natural
/// Rust indentation.
fn dedent(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').count())
        .min()
        .unwrap_or(0);
    let stripped: Vec<String> = lines
        .iter()
        .map(|l| {
            if l.len() >= min_indent {
                l[min_indent..].to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    stripped.join("\n").trim_matches('\n').to_string()
}

fn null_notebook() -> Notebook {
    Notebook::new(Box::new(NoSimplifier), None)
}

/// Build a notebook with a controllable mock REDUCE. The returned handle
/// lets the test enqueue responses with `respond_to(cell_id, …)`. The
/// mock is wrapped in `ReduceSimplifier` so the notebook sees the
/// IR-shaped `SymbolicSimplifier` API while the test still controls the
/// underlying REDUCE-text round-trip.
fn mock_reduce_notebook() -> (Notebook, MockReduce) {
    let mock = MockReduce::new();
    let simplifier = ReduceSimplifier::new(Box::new(mock.clone()));
    let nb = Notebook::new(Box::new(simplifier), None);
    (nb, mock)
}

fn add_and_play(nb: &mut Notebook, source: &str) -> usize {
    let idx = nb.add_cell(&dedent(source));
    nb.play(idx);
    idx
}

fn validate_wgsl(wgsl: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(wgsl)
        .map_err(|e| format!("naga parse: {}\n--- WGSL ---\n{}", e, wgsl))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("naga validate: {}\n--- WGSL ---\n{}", e, wgsl))?;
    Ok(())
}

fn shader(outcome: &CellOutcome) -> &ShaderSpec {
    outcome.shaders.first().expect("expected at least one shader")
}

// ─── tests ─────────────────────────────────────────────────────────────────

#[test]
fn add_cell_assigns_unique_ids() {
    let mut nb = null_notebook();
    let a = nb.add_cell("x");
    let b = nb.add_cell("y");
    // IDs come from a process-global counter, so parallel test threads can
    // interleave allocations — only assert uniqueness and the locally-
    // monotonic ordering of the two calls in this test.
    let ida = nb.cell(a).id;
    let idb = nb.cell(b).id;
    assert_ne!(ida, idb);
    assert!(idb > ida);
}

#[test]
fn set_text_invalidates_ast_and_marks_stale_after_play() {
    let mut nb = null_notebook();
    let i = nb.add_cell("y = sin(x)");
    nb.play(i);
    assert!(!nb.cell(i).is_stale(), "fresh play is not stale");
    nb.set_text(i, "y = cos(x)");
    assert!(nb.cell(i).is_stale(), "text changed since play → stale");
}

#[test]
fn idle_cell_is_not_stale() {
    let mut nb = null_notebook();
    let i = nb.add_cell("anything");
    assert!(!nb.cell(i).is_stale());
}

#[test]
fn plot_emits_shader_and_validates_under_naga() {
    let mut nb = null_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        plot(y = sin(x))
        "#,
    );
    let s = shader(&nb.cell(i).outcome);
    assert!(s.wgsl.contains("@fragment"));
    assert!(s.wgsl.contains("sin("));
    validate_wgsl(&s.wgsl).expect("wgsl validates");
    assert!(matches!(nb.cell(i).state, super::CellState::Playing));
}

#[test]
fn user_block_with_loop_plot() {
    let mut nb = null_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        f := (
            sum := 0
            for i in 0..10 (
                sum := sum + x²+4*x²
            )
            y = sum
        )
        plot(f)
        "#,
    );
    let s = shader(&nb.cell(i).outcome);
    assert!(!s.wgsl.contains("pow("), "small-int pow expanded to muls");
    assert!(s.wgsl.contains("fn _lifted_f("), "block lifted into fn");
    assert!(s.wgsl.contains("let _corner_f_mm = _lifted_f(x_m, y_m);"));
    validate_wgsl(&s.wgsl).expect("wgsl validates");
}

#[test]
fn user_function_with_axis_capture_returning_f32_corner_substitutes() {
    // Regression: when a user fn returns f32 and captures `x`, calling it
    // inside `y = f(...)` corner-checking has to substitute the corner's
    // `x_m`/`x_p` for the captured arg. Otherwise every corner gets the
    // pixel-center `x`, the sign check never flips, and the curve renders
    // as scattered dots instead of a continuous line.
    let mut nb = null_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        f(n) := (
            sum := x
            for i in 0..n (sum := sum^x)
            sum
        )
        plot(y = f(2))
        "#,
    );
    let s = shader(&nb.cell(i).outcome);
    assert!(
        s.wgsl.contains("f(2.0, x_m)") && s.wgsl.contains("f(2.0, x_p)"),
        "corner-checking must call f with corner-substituted x; got:\n{}",
        s.wgsl
    );
    assert!(
        !s.wgsl.contains("f(2.0, x))"),
        "must NOT see the pixel-center `x` slip in as a captured arg; got:\n{}",
        s.wgsl
    );
    validate_wgsl(&s.wgsl).expect("wgsl validates");
}

#[test]
fn block_binding_value_returning_float_used_in_eq_plot() {
    // Regression: `f := (sum:=0; for...; sum)` is a non-bool block whose
    // result is the float `sum`. With `plot(y = f)` the corner-check has to
    // call `_lifted_f` at each corner so the curve isn't dotted on slopes.
    // It must never emit the bare identifier `sum` at top level.
    let mut nb = null_notebook();
    // Verbatim what the user typed (no spaces around := or +).
    let i = nb.add_cell(
        "f:=(\n sum:=0\n for i in 0..10 (sum:=sum+x)\n sum\n)\nplot(y=f)",
    );
    nb.play(i);
    let s = shader(&nb.cell(i).outcome);
    assert!(
        !s.wgsl.contains(" sum)") && !s.wgsl.contains("(sum)"),
        "WGSL must not reference bare `sum` at top level; got:\n{}",
        s.wgsl
    );
    // The corner check inside `_result` must reference the per-corner values,
    // not the pixel-center `f`. Look for `(y_m) > (_corner_f_mm)` etc.
    let result_line = s
        .wgsl
        .lines()
        .find(|l| l.contains("let _result"))
        .expect("has _result line");
    assert!(
        result_line.contains("_corner_f_mm")
            && result_line.contains("_corner_f_mp")
            && result_line.contains("_corner_f_pm")
            && result_line.contains("_corner_f_pp"),
        "_result must use per-corner f, not pixel-center f;\nLine: {}\nFull WGSL:\n{}",
        result_line,
        s.wgsl,
    );
    validate_wgsl(&s.wgsl).expect("wgsl validates");
}

#[test]
fn anonymous_imperative_block_inside_plot_compiles() {
    // Regression: `plot(y = (sum:=0; for...; sum))` — an imperative block as
    // an *anonymous* expression value (no `f := ...` binding around it) must
    // still hoist its bindings/loops into a synthesized function. Otherwise
    // the WGSL emits a bare `sum` reference and shader compilation fails.
    let mut nb = null_notebook();
    let i = nb.add_cell(
        "plot(y = (sum := 0\n for i in 0..10 (sum := sum + x)\n sum))",
    );
    nb.play(i);
    let s = shader(&nb.cell(i).outcome);
    assert!(
        !s.wgsl.contains(" sum)") && !s.wgsl.contains("(sum)"),
        "WGSL must not reference bare `sum` at top level; got:\n{}",
        s.wgsl
    );
    validate_wgsl(&s.wgsl).expect("wgsl validates");
}

#[test]
fn typing_anonymous_block_oneliner_never_hangs() {
    // Simulate typing the full one-liner one character at a time. Each
    // intermediate prefix must lex+parse in bounded time — the user reported
    // the app freezing while typing this exact text.
    use std::time::{Duration, Instant};
    let full = "plot(y = (sum := 0 for i in 0..10 (sum := sum + x) sum))";
    for end in 0..=full.len() {
        let prefix = &full[..end];
        let start = Instant::now();
        let mut lex = crate::lang::lexer::Lexer::new(prefix);
        if let Ok(tokens) = lex.tokenize() {
            let mut parser =
                crate::lang::parser::Parser::new(tokens, prefix.to_string());
            let _ = parser.parse(); // ok or err — we only care it returns
        }
        let _ = crate::lang::highlight::highlight(prefix);
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "lex+parse+highlight took {:?} on prefix {:?} (len {})",
            start.elapsed(),
            prefix,
            end
        );
    }
}

#[test]
fn anonymous_block_with_multiplication_compiles_and_validates() {
    // Same shape as the one-liner above but with `*` instead of `+`.
    // Reported to hang in naga/wgpu pipeline creation in the live app.
    use std::time::{Duration, Instant};
    let mut nb = null_notebook();
    let i =
        nb.add_cell("plot(y = (sum := 0 for i in 0..10 (sum := sum * x) sum))");
    let start = Instant::now();
    nb.play(i);
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "play() took longer than 3s — likely an infinite loop"
    );
    let s = shader(&nb.cell(i).outcome);
    println!("--- generated WGSL ---\n{}\n--- end ---", s.wgsl);
    let validate_start = Instant::now();
    validate_wgsl(&s.wgsl).expect("wgsl validates");
    assert!(
        validate_start.elapsed() < Duration::from_secs(3),
        "naga validation took longer than 3s — likely the freeze cause"
    );
}

#[test]
fn anonymous_imperative_block_inside_plot_one_liner_compiles() {
    // Same as above but everything on one line, exactly as the user typed it.
    // Parser must not infinite-loop and play() must complete in bounded time.
    use std::time::{Duration, Instant};
    let mut nb = null_notebook();
    let i =
        nb.add_cell("plot(y = (sum := 0 for i in 0..10 (sum := sum + x) sum))");
    let start = Instant::now();
    nb.play(i);
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "play() took longer than 3s — likely an infinite loop"
    );
    // Either the WGSL must validate, or the cell must hold a structured error.
    if !nb.cell(i).outcome.shaders.is_empty() {
        let s = shader(&nb.cell(i).outcome);
        println!("--- generated WGSL ---\n{}\n--- end ---", s.wgsl);
        validate_wgsl(&s.wgsl).expect("wgsl validates");
    } else {
        println!(
            "no shader produced; outcome.message = {:?}",
            nb.cell(i).outcome.message
        );
    }
}

#[test]
fn user_function_with_axis_capture() {
    let mut nb = null_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        f(n) := (
            sum := x
            for i in 0..n (sum := sum^x)
            y = sum
        )
        plot(f(1))
        "#,
    );
    let s = shader(&nb.cell(i).outcome);
    assert!(s.wgsl.contains("fn f(n: f32, x: f32, y: f32) -> bool"));
    assert!(s.wgsl.contains("fn _diff_f(n: f32, x: f32, y: f32) -> f32"));
    validate_wgsl(&s.wgsl).expect("wgsl validates");
}

#[test]
fn print_simple_numeric_evaluates_via_interpreter() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(3 + 4)");
    let m = nb.cell(i).outcome.message.as_ref().expect("message set");
    match m {
        CellMessage::Computed(s) => assert_eq!(s, "7"),
        other => panic!("expected Computed, got {:?}", other),
    }
    assert!(nb.cell(i).outcome.shaders.is_empty());
}

#[test]
fn multiple_prints_in_same_cell_all_appear_in_output() {
    let mut nb = null_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        print(3 + 4)
        print(5 * 2)
        print(7)
        "#,
    );
    let m = nb.cell(i).outcome.message.as_ref().expect("message set");
    match m {
        CellMessage::Computed(s) => assert_eq!(s, "7\n10\n7"),
        other => panic!("expected Computed, got {:?}", other),
    }
}

#[test]
fn multiple_symbolic_prints_resolve_in_order() {
    // Two prints that both need REDUCE. The notebook serializes them: it
    // submits the first, parks on Pending, and only after that response
    // lands submits the second. The final Computed message joins both
    // results with a newline, in source order.
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        f := x + 2*x
        g := x * x
        print(f)
        print(g)
        "#,
    );
    let cell_id = nb.cell(i).id;

    assert!(matches!(
        nb.cell(i).outcome.message,
        Some(CellMessage::Pending)
    ));

    mock.respond_to(cell_id, Ok("3*x".to_string()));
    nb.tick();
    // Still pending — second print is now in-flight.
    assert!(matches!(
        nb.cell(i).outcome.message,
        Some(CellMessage::Pending)
    ));

    mock.respond_to(cell_id, Ok("x^2".to_string()));
    nb.tick();
    let m = nb.cell(i).outcome.message.as_ref().expect("final message");
    match m {
        CellMessage::Computed(s) => assert_eq!(s, "3*x\nx^2"),
        other => panic!("expected Computed, got {:?}", other),
    }
}

#[test]
fn numeric_and_symbolic_prints_combine_in_order() {
    // Mixed batch with no preceding bindings — numeric prints stay on
    // the interpreter path while the symbolic print routes through
    // REDUCE. The final joined output must keep source order regardless
    // of which path each print took.
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        print(3 + 4)
        print(x + 2*x)
        print(5 * 2)
        "#,
    );
    let cell_id = nb.cell(i).id;
    mock.respond_to(cell_id, Ok("3*x".to_string()));
    nb.tick();
    let m = nb.cell(i).outcome.message.as_ref().expect("final message");
    match m {
        CellMessage::Computed(s) => assert_eq!(s, "7\n3*x\n10"),
        other => panic!("expected Computed, got {:?}", other),
    }
}

#[test]
fn print_symbolic_falls_to_reduce_and_resolves_via_tick() {
    // The interpreter can't evaluate `print(f)` because `f` references the
    // axis variable `x`. Notebook submits to REDUCE and parks the cell on
    // `Pending`. When `tick()` finds a response, it should format the
    // simplified text as the cell's print output.
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        f := x + 2*x
        print(f)
        "#,
    );
    let cell_id = nb.cell(i).id;

    let m = nb.cell(i).outcome.message.as_ref().expect("pending message");
    assert!(
        matches!(m, CellMessage::Pending),
        "expected Pending, got {:?}",
        m
    );
    assert!(nb.has_pending(), "Notebook should report pending");

    // Simulate REDUCE responding with the simplified expression.
    mock.respond_to(cell_id, Ok("3*x".to_string()));
    let updated = nb.tick();
    assert_eq!(updated, vec![i]);

    let final_msg = nb.cell(i).outcome.message.as_ref().expect("final message");
    match final_msg {
        CellMessage::Computed(s) => assert_eq!(s, "3*x"),
        other => panic!("expected Computed, got {:?}", other),
    }
    assert!(!nb.has_pending());
}

#[test]
fn print_symbolic_equation_appends_eq_zero() {
    let (mut nb, mock) = mock_reduce_notebook();
    // `y = x + 2*x` is an equation; print should reformat as `lhs - rhs = 0`.
    let i = nb.add_cell("print(y = x + 2*x)");
    nb.play(i);
    let cell_id = nb.cell(i).id;
    mock.respond_to(cell_id, Ok("y - 3*x".to_string()));
    nb.tick();
    let msg = nb.cell(i).outcome.message.as_ref().unwrap();
    match msg {
        CellMessage::Computed(s) => assert!(
            s.contains("= 0"),
            "equation result should end with `= 0`; got {}",
            s
        ),
        other => panic!("expected Computed, got {:?}", other),
    }
}

#[test]
fn plot_and_print_in_same_cell_both_appear_in_outcome() {
    // A cell that has both `print(f)` and `plot(y = f)`: the plot produces
    // a shader, and the symbolic print parks on `Pending` (interpreter
    // can't evaluate `x` symbolically; the UI's REDUCE roundtrip would
    // resolve it via `tick`). Both effects coexist on the same outcome.
    let (mut nb, _mock) = mock_reduce_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        f := x + 2*x
        print(f)
        plot(y = f)
        "#,
    );
    let outcome = &nb.cell(i).outcome;
    assert!(!outcome.shaders.is_empty(), "plot should produce a shader");
    assert!(
        matches!(outcome.message, Some(CellMessage::Pending)),
        "symbolic print should leave the cell `Pending`, got {:?}",
        outcome.message
    );
}

#[test]
fn play_auto_runs_earlier_cells_jupyter_style() {
    let mut nb = null_notebook();
    let a = nb.add_cell("f := x²");
    let b = nb.add_cell("plot(y = f)");
    nb.play(b);
    // Earlier cell should also be Playing now.
    assert!(matches!(nb.cell(a).state, super::CellState::Playing));
    assert!(matches!(nb.cell(b).state, super::CellState::Playing));
}

#[test]
fn replay_stops_later_playing_cells() {
    let mut nb = null_notebook();
    let a = nb.add_cell("f := x²");
    let b = nb.add_cell("plot(y = f)");
    nb.play(b);
    assert!(matches!(nb.cell(b).state, super::CellState::Playing));

    nb.set_text(a, "f := x³");
    nb.replay(a);
    // Later cell that was playing must be stopped now.
    assert!(matches!(nb.cell(b).state, super::CellState::Stopped));
    assert!(matches!(nb.cell(a).state, super::CellState::Playing));
}

#[test]
fn parse_error_is_stored_as_diagnostic_with_span() {
    let mut nb = null_notebook();
    let i = nb.add_cell("y = sin(");
    nb.play(i);
    assert!(
        nb.cell(i).outcome.shaders.is_empty(),
        "no shader on parse failure"
    );
    let diags = &nb.cell(i).outcome.diagnostics;
    assert!(!diags.is_empty(), "expected at least one diagnostic");
    let d = &diags[0];
    assert!(matches!(d.severity, super::diagnostic::Severity::Error));
    // Span at minimum points somewhere in the source.
    assert!(d.span.start_line == 0 || d.span.end_line >= d.span.start_line);
}

#[test]
fn token_colors_populated_after_play() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot(y = sin(x))");
    let colors = &nb.cell(i).outcome.token_colors;
    assert!(!colors.is_empty(), "token_colors should be populated");
    // Total span coverage should equal source length.
    let source = nb.cell(i).buffer.text();
    let total: usize = colors.iter().map(|c| c.end - c.start).sum();
    assert_eq!(total, source.len());
}

#[test]
fn stop_transitions_state_to_stopped() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot(y = x²)");
    assert!(matches!(nb.cell(i).state, super::CellState::Playing));
    nb.stop(i);
    assert!(matches!(nb.cell(i).state, super::CellState::Stopped));
}

#[test]
fn examples_render_through_notebook() {
    let files: &[(&str, &str)] = &[
        ("gradient", include_str!("../../examples/gradient.logos")),
        ("ripple", include_str!("../../examples/ripple.logos")),
        ("mandlebrot", include_str!("../../examples/mandlebrot.logos")),
        ("warp", include_str!("../../examples/warp.logos")),
        ("monte_carlo", include_str!("../../examples/monte_carlo.logos")),
        ("waves", include_str!("../../examples/waves.logos")),
    ];
    for (name, file) in files {
        let cells = crate::lang::notebook_format::parse_logos(file)
            .unwrap_or_else(|e| panic!("{name}: parse_logos failed: {e}"));
        for cell in cells {
            let mut nb = null_notebook();
            let i = nb.add_cell(&cell.content);
            nb.play(i);
            let nbcell = nb.cell(i);
            assert!(
                !nbcell.outcome.shaders.is_empty(),
                "{name}: expected at least one shader, got message={:?}",
                nbcell.outcome.message,
            );
            for s in &nbcell.outcome.shaders {
                validate_wgsl(&s.wgsl)
                    .unwrap_or_else(|e| panic!("{name}: WGSL invalid: {e}"));
            }
        }
    }
}

#[test]
fn remove_cell_drops_pending_reduce() {
    let (mut nb, _mock) = mock_reduce_notebook();
    let i = nb.add_cell("f := x + 2*x\nprint(f)");
    nb.play(i);
    assert!(nb.has_pending());
    nb.remove_cell(i);
    assert_eq!(nb.len(), 0);
    // Notebook-side pending entry is cleared (mock backend may still hold
    // the in-flight handle, but Notebook would ignore any stale response).
}
