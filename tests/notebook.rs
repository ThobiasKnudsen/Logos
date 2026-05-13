//! Notebook integration tests — the production class as the test harness.
//!
//! These cover the same scenarios as the previous ad-hoc `run_cell` helper
//! in `lang::mod`, but exercise the real `Notebook` API: cells are added,
//! `play()` is called, REDUCE round-trips are pumped via `tick()`, outcomes
//! are read off the cells. No backdoors.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use logos::lang::reduce::service::ReduceResponse;
use logos::lang::symbolic::NoSimplifier;
use logos::notebook::{
    CellMessage, CellOutcome, CellState, Notebook, ReduceBackend, ReduceSimplifier, Severity,
    ShaderSpec,
};

// REDUCE's CSL has process-global state and crashes on re-init, so even
// `--test-threads=1` can't safely combine multiple `CslSession::new()`
// calls in one binary (the existing `test_reduce_session` already takes
// CSL). Tests that need to drive REDUCE round-trips use a mock backend
// the test directly controls.

/// One captured submission to the mock REDUCE backend. Tests inspect these
/// via `MockReduce::submissions_for` to assert on what *would have been sent*
/// to real REDUCE — closing the gap where the mock only stubbed responses
/// and silently swallowed bad submissions.
#[derive(Clone, Debug)]
struct MockSubmission {
    #[allow(dead_code)]
    cell_id: usize,
    #[allow(dead_code)]
    context: Vec<String>,
    expression: String,
}

#[derive(Default)]
struct MockState {
    /// Submitted requests waiting for `respond_to(cell_id, …)`.
    inflight: HashMap<usize, u64>,
    /// Queued responses ready for `try_recv`.
    inbox: VecDeque<ReduceResponse>,
    next_request_id: u64,
    /// Every submission ever received, in submission order. Tests use this
    /// to verify what REDUCE *would have seen* — not just what the mock
    /// replied with.
    submissions: Vec<MockSubmission>,
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
    /// All submissions made for `cell_id`, in submission order.
    fn submissions_for(&self, cell_id: usize) -> Vec<MockSubmission> {
        self.state
            .lock()
            .unwrap()
            .submissions
            .iter()
            .filter(|s| s.cell_id == cell_id)
            .cloned()
            .collect()
    }
}

impl ReduceBackend for MockReduce {
    fn submit(&mut self, cell_id: usize, context: Vec<String>, expression: String) -> u64 {
        let mut s = self.state.lock().unwrap();
        let id = s.next_request_id;
        s.next_request_id += 1;
        s.inflight.insert(cell_id, id);
        s.submissions.push(MockSubmission {
            cell_id,
            context,
            expression,
        });
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
fn auto_rerun_fires_after_quiet_period() {
    use std::time::{Duration, Instant};
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot(y = sin(x))");
    let edit_time = Instant::now();
    nb.set_text(i, "plot(y = cos(x))");
    nb.mark_edited(i, edit_time);
    let replayed = nb.tick_auto_rerun(edit_time + Duration::from_millis(50));
    assert!(
        replayed.is_empty(),
        "should not replay within the quiet period"
    );
    let replayed = nb.tick_auto_rerun(edit_time + Duration::from_millis(250));
    assert_eq!(replayed, vec![i], "should replay after quiet period");
    assert!(!nb.cell(i).is_stale());
    assert!(nb.cell(i).outcome.shaders[0].wgsl.contains("cos("));
}

#[test]
fn auto_rerun_resets_when_user_keeps_typing() {
    use std::time::{Duration, Instant};
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot(y = x)");
    let t0 = Instant::now();
    nb.set_text(i, "plot(y = x+1)");
    nb.mark_edited(i, t0);
    nb.set_text(i, "plot(y = x+2)");
    nb.mark_edited(i, t0 + Duration::from_millis(150));
    let replayed = nb.tick_auto_rerun(t0 + Duration::from_millis(200));
    assert!(
        replayed.is_empty(),
        "continued typing should reset the quiet window"
    );
    let replayed = nb.tick_auto_rerun(t0 + Duration::from_millis(400));
    assert_eq!(replayed, vec![i]);
}

#[test]
fn auto_rerun_deadline_reports_next_wake() {
    use std::time::{Duration, Instant};
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot(y = x)");
    let t0 = Instant::now();
    nb.set_text(i, "plot(y = x+1)");
    nb.mark_edited(i, t0);
    let deadline = nb
        .next_auto_rerun_deadline(t0)
        .expect("should have a pending deadline");
    assert_eq!(deadline, t0 + Notebook::AUTO_RERUN_QUIET_PERIOD);
    nb.tick_auto_rerun(t0 + Duration::from_millis(250));
    assert!(
        nb.next_auto_rerun_deadline(Instant::now()).is_none(),
        "no pending deadline after replay"
    );
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
    assert!(matches!(nb.cell(i).state, CellState::Playing));
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
        let mut lex = logos::lang::lexer::Lexer::new(prefix);
        if let Ok(tokens) = lex.tokenize() {
            let mut parser =
                logos::lang::parser::Parser::new(tokens, prefix.to_string());
            let _ = parser.parse(); // ok or err — we only care it returns
        }
        let _ = logos::lang::highlight::highlight(prefix);
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
fn plot_with_user_function_body_inlines_call_before_reduce() {
    // User's exact bug report from this session: defining `F(x) := ∫(f(x), x)`
    // with `f` and `e` as prior bindings used to ship `int(f(x), x)` to REDUCE,
    // which rejected it with "Declare f operator? (Y or N)" because `f` was
    // undeclared. The fix (ir.rs `substitute_idents_inner`) beta-reduces user
    // function calls during CAS substitution, so the submitted text contains
    // the inlined body — `f(...)` and `e` are gone.
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        e := 2.718
        f(x) := x*e^(x²)
        F(x) := ∫(f(x), x)
        plot(y = F(x))
        "#,
    );
    let cell_id = nb.cell(i).id;
    let subs = mock.submissions_for(cell_id);
    assert_eq!(subs.len(), 1, "expected one submission; got {:?}", subs);
    let expr = &subs[0].expression;
    assert!(
        !expr.contains("f("),
        "user function `f` must be inlined before REDUCE submission;\n\
         got: {}",
        expr,
    );
    assert!(
        !expr.contains(" e ") && !expr.contains("(e)") && !expr.contains(",e"),
        "constant `e` must be substituted with its value;\n\
         got: {}",
        expr,
    );
    assert!(
        expr.contains("2.718"),
        "submitted text should contain the value of `e` (2.718);\n\
         got: {}",
        expr,
    );
}

#[test]
fn plot_routes_through_cas_when_function_body_contains_integral() {
    // Regression for: a cell with a function binding whose body contains a
    // CAS call AND a `plot(...)` of that function would skip the iterative-
    // CAS path and feed `integral(...)` straight to WGSL gen. Naga rejected
    // the resulting shader with "no definition in scope for identifier:
    // 'integral'", surfaced to the user as
    //   "Undefined function or variable 'integral'".
    //
    // After the fix, the cell parks on `Pending` waiting for REDUCE; when the
    // simplifier responds, the splice into `effective_ir` makes F's body
    // WGSL-compatible, and `compile_after_simplify` produces the shader.
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(
        &mut nb,
        r#"
        e := 2.718
        f(x) := x*e^(x²)
        F(x) := ∫(f(x), x)
        plot(y = F(x))
        "#,
    );
    let cell_id = nb.cell(i).id;
    assert!(
        matches!(nb.cell(i).outcome.message, Some(CellMessage::Pending)),
        "cell should park on Pending while the CAS round-trip runs; got {:?}",
        nb.cell(i).outcome.message,
    );
    assert!(
        nb.cell(i).outcome.shaders.is_empty(),
        "shader must not be emitted before CAS resolves",
    );

    // REDUCE-style answer: any well-formed expression in x that WGSL accepts.
    mock.respond_to(cell_id, Ok("x*x".to_string()));
    nb.tick();

    let outcome = &nb.cell(i).outcome;
    // No "Undefined function or variable 'integral'" error.
    for d in &outcome.diagnostics {
        assert!(
            !d.message.contains("'integral'"),
            "unsubstituted integral leaked to WGSL gen: {}",
            d.message,
        );
    }
    assert!(
        !outcome.shaders.is_empty(),
        "plot shader should be emitted after CAS resolves; diagnostics: {:?}",
        outcome.diagnostics,
    );
    let s = &outcome.shaders[0];
    assert!(
        !s.wgsl.contains("integral("),
        "WGSL must not reference `integral(`; got:\n{}",
        s.wgsl,
    );
    validate_wgsl(&s.wgsl).expect("wgsl validates");
}

#[test]
fn interpreter_path_routes_through_cas_when_function_body_contains_integral() {
    // Same shape as `plot_routes_through_cas_…` but for the interpreter path:
    // a cell whose top-level expression triggers `needs_interpreter` (array
    // literal here) AND references a function whose body contains a CAS
    // call. Before the fix, the interpreter dispatch ran `interpreter::eval`
    // directly on the unresolved IR and bailed on the unknown `integral`
    // callee. The fix routes through `submit_cas_at` first; after the splice,
    // `compile_after_simplify` resumes the interpreter branch.
    let (mut nb, mock) = mock_reduce_notebook();
    // Array-literal cell that triggers `needs_interpreter` and references
    // a function whose body contains a CAS call. The trailing expression
    // (`g`) makes the cell's final value the array itself, not Void.
    let i = add_and_play(
        &mut nb,
        r#"
        F(x) := ∫(x, x)
        g := [F(1.0), F(2.0)]
        g
        "#,
    );
    let cell_id = nb.cell(i).id;
    assert!(
        matches!(nb.cell(i).outcome.message, Some(CellMessage::Pending)),
        "cell should park on Pending while the CAS round-trip runs; got {:?}",
        nb.cell(i).outcome.message,
    );

    mock.respond_to(cell_id, Ok("x*x/2".to_string()));
    nb.tick();

    let outcome = &nb.cell(i).outcome;
    for d in &outcome.diagnostics {
        assert!(
            !d.message.contains("'integral'") && !d.message.contains("Unknown function: integral"),
            "unsubstituted integral leaked past CAS resolution: {}",
            d.message,
        );
    }
    match &outcome.message {
        Some(CellMessage::Computed(s)) => assert!(
            s.contains('[') && s.contains(']'),
            "expected array-shaped Computed value; got {}",
            s,
        ),
        other => panic!(
            "expected Computed after CAS resolves, got {:?}\ndiagnostics: {:?}",
            other, outcome.diagnostics
        ),
    }
}

#[test]
fn play_auto_runs_earlier_cells_jupyter_style() {
    let mut nb = null_notebook();
    let a = nb.add_cell("f := x²");
    let b = nb.add_cell("plot(y = f)");
    nb.play(b);
    // Earlier cell should also be Playing now.
    assert!(matches!(nb.cell(a).state, CellState::Playing));
    assert!(matches!(nb.cell(b).state, CellState::Playing));
}

#[test]
fn replay_stops_later_playing_cells() {
    let mut nb = null_notebook();
    let a = nb.add_cell("f := x²");
    let b = nb.add_cell("plot(y = f)");
    nb.play(b);
    assert!(matches!(nb.cell(b).state, CellState::Playing));

    nb.set_text(a, "f := x³");
    nb.replay(a);
    // Later cell that was playing must be stopped now.
    assert!(matches!(nb.cell(b).state, CellState::Stopped));
    assert!(matches!(nb.cell(a).state, CellState::Playing));
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
    assert!(matches!(d.severity, Severity::Error));
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
    assert!(matches!(nb.cell(i).state, CellState::Playing));
    nb.stop(i);
    assert!(matches!(nb.cell(i).state, CellState::Stopped));
}

#[test]
fn examples_render_through_notebook() {
    // Walk `examples/` at test time — any `.logos` file in the directory is
    // covered automatically without editing this list. CARGO_MANIFEST_DIR is
    // set by Cargo to the project root, so this works from `cargo test`
    // regardless of working directory.
    let examples_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let entries = std::fs::read_dir(&examples_dir)
        .unwrap_or_else(|e| panic!("read_dir({}): {}", examples_dir.display(), e));

    let mut found = 0;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "logos") {
            continue;
        }
        found += 1;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let file = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{name}: read_to_string failed: {e}"));

        let cells = logos::lang::notebook_format::parse_logos(&file)
            .unwrap_or_else(|e| panic!("{name}: parse_logos failed: {e}"));

        // Cells in a single example share scope (later cells can reference
        // functions defined earlier — see `statistikk.logos`). Mirror that
        // by playing each cell in the *same* notebook, sequentially.
        let mut nb = null_notebook();
        for (idx, cell) in cells.into_iter().enumerate() {
            let i = nb.add_cell(&cell.content);
            nb.play(i);
            let nbcell = nb.cell(i);

            // Snapshot the outcome's invariants — every example cell must:
            //   (a) produce no error diagnostics, ever,
            //   (b) either emit at least one validated shader, OR park on
            //       `Pending` because it has CAS work that needs a real
            //       symbolic backend (this test uses `null_notebook`'s
            //       `NoSimplifier`, so CAS-bearing examples like
            //       `integral.logos` legitimately stay Pending here — they
            //       resolve to shaders against the production REDUCE).
            for d in &nbcell.outcome.diagnostics {
                panic!(
                    "{name} cell[{idx}]: diagnostic raised: {:?}\nsource:\n{}",
                    d, cell.content,
                );
            }
            let is_pending = matches!(
                nbcell.outcome.message,
                Some(CellMessage::Pending),
            );
            if !is_pending {
                assert!(
                    !nbcell.outcome.shaders.is_empty(),
                    "{name} cell[{idx}]: expected at least one shader or Pending; \
                     message={:?}\nsource:\n{}",
                    nbcell.outcome.message,
                    cell.content,
                );
            }
            for s in &nbcell.outcome.shaders {
                validate_wgsl(&s.wgsl).unwrap_or_else(|e| {
                    panic!("{name} cell[{idx}]: WGSL invalid: {e}\nsource:\n{}", cell.content)
                });
            }
        }
    }
    assert!(
        found > 0,
        "no `.logos` files found in {}",
        examples_dir.display(),
    );
}

/// Issue #28: `plot(<array of 2-tuples>)` routes through the vertex-
/// plot path and surfaces vertex data on the ShaderSpec. The
/// fragment-only `wgsl` field carries the canonical vertex+fragment
/// pair, and the renderer-facing `vertices` field carries the
/// uploaded positions.
#[test]
fn plot_with_literal_vertex_array_emits_vertex_shader() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot([(0, 0), (1, 1), (2, 0)])");
    let spec = shader(&nb.cell(i).outcome);
    let verts = spec
        .vertices
        .as_ref()
        .expect("vertex plot must populate `vertices`");
    assert_eq!(
        verts.positions,
        vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]],
        "exact positions from the literal array",
    );
    assert!(
        spec.wgsl.contains("@vertex"),
        "ShaderSpec.wgsl should be the vertex+fragment pair, got:\n{}",
        spec.wgsl,
    );
}

/// Issue #28: re-running a cell whose vertex data hasn't changed must
/// produce a ShaderSpec with the same hash, so the renderer's
/// content-keyed pipeline cache skips the GPU re-upload.
#[test]
fn replayed_unchanged_vertex_plot_keeps_same_hash() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot([(0, 0), (1, 1)])");
    let first = shader(&nb.cell(i).outcome)
        .vertices
        .as_ref()
        .unwrap()
        .hash;
    nb.play(i);
    let second = shader(&nb.cell(i).outcome)
        .vertices
        .as_ref()
        .unwrap()
        .hash;
    assert_eq!(first, second, "identical replays must produce equal hashes");
}

/// Issue #28: editing the vertex data flips the hash so the cache
/// in `ShaderPipelineManager::set_cell_shaders` rebuilds the
/// pipeline + buffer.
#[test]
fn editing_vertex_data_changes_hash() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot([(0, 0), (1, 1)])");
    let before = shader(&nb.cell(i).outcome)
        .vertices
        .as_ref()
        .unwrap()
        .hash;
    nb.set_text(i, "plot([(0, 0), (1, 2)])");
    nb.play(i);
    let after = shader(&nb.cell(i).outcome)
        .vertices
        .as_ref()
        .unwrap()
        .hash;
    assert_ne!(before, after, "different positions must hash differently");
}

/// Issue #28: analytic plots (curve, surface, lambda) continue to
/// route through the fragment-only path — `vertices` stays `None`
/// so the renderer doesn't accidentally try to bind a non-existent
/// buffer.
#[test]
fn analytic_plot_does_not_attach_vertices() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "plot(y = sin(x))");
    let spec = shader(&nb.cell(i).outcome);
    assert!(
        spec.vertices.is_none(),
        "analytic plot must leave `vertices` None"
    );
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

// ─── LaTeX-symbol substitutions ────────────────────────────────────────────
//
// Verify every Unicode codepoint the autocomplete LATEX_SYMBOLS table inserts
// (src/editor/autocomplete.rs) is actually usable end-to-end through Notebook.
// Tests are grouped by the lexer's semantic category — what TokenType the
// codepoint produces.
//
// Runtime layer asserted on: `CellMessage::Computed(String)` (the interpreter's
// formatted print output). For CAS-only symbols (∫, ∑, ∂, …) a mock REDUCE
// supplies the simplified response synchronously and the joined Computed text
// is checked.

fn computed(nb: &Notebook, idx: usize) -> String {
    match &nb.cell(idx).outcome.message {
        Some(CellMessage::Computed(s)) => s.clone(),
        Some(other) => panic!(
            "expected Computed, got {:?}\ndiagnostics: {:?}",
            other,
            nb.cell(idx).outcome.diagnostics,
        ),
        None => panic!(
            "no message set\ndiagnostics: {:?}",
            nb.cell(idx).outcome.diagnostics,
        ),
    }
}

fn diag_messages(nb: &Notebook, idx: usize) -> Vec<String> {
    nb.cell(idx)
        .outcome
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

// ── Numeric constants ──────────────────────────────────────────────────────
// Lexer maps these codepoints directly to `TokenType::Number(_)`.

#[test]
fn latex_pi_evaluates_to_pi_constant() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(π)");
    assert_eq!(computed(&nb, i), std::f64::consts::PI.to_string());
}

#[test]
fn latex_euler_evaluates_to_e_constant() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(ℯ)");
    assert_eq!(computed(&nb, i), std::f64::consts::E.to_string());
}

// ── Unicode binary operators ───────────────────────────────────────────────
// × → Star, ÷ → Slash, − → Minus. These should behave identically to the
// ASCII *, /, -.

#[test]
fn latex_times_multiplies() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(3 × 4)");
    assert_eq!(computed(&nb, i), "12");
}

#[test]
fn latex_div_divides() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(8 ÷ 2)");
    assert_eq!(computed(&nb, i), "4");
}

#[test]
fn latex_unicode_minus_subtracts() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(5 − 3)");
    assert_eq!(computed(&nb, i), "2");
}

// ── Relation operators ─────────────────────────────────────────────────────
// ≤ → Lte, ≥ → Gte, ≠ → Neq. Same tokens as their ASCII two-char forms.

#[test]
fn latex_leq_compares() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(3 ≤ 4)");
    assert_eq!(computed(&nb, i), "true");
}

#[test]
fn latex_geq_compares() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(4 ≥ 3)");
    assert_eq!(computed(&nb, i), "true");
}

#[test]
fn latex_neq_compares() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(3 ≠ 4)");
    assert_eq!(computed(&nb, i), "true");
}

// ── Builtin-mapped symbols ─────────────────────────────────────────────────
// √ → Builtin("sqrt"); superscript digits → Builtin("pow{n}"); ² → square,
// ³ → cube.

#[test]
fn latex_sqrt_computes() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(√(16))");
    assert_eq!(computed(&nb, i), "4");
}

#[test]
fn latex_superscript_squares() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(3²)");
    assert_eq!(computed(&nb, i), "9");
}

#[test]
fn latex_superscript_cubes() {
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(2³)");
    assert_eq!(computed(&nb, i), "8");
}

#[test]
fn latex_superscript_digits_4_through_9() {
    let cases = [
        ("2⁴", "16"),
        ("2⁵", "32"),
        ("2⁶", "64"),
        ("2⁷", "128"),
        ("2⁸", "256"),
        ("2⁹", "512"),
    ];
    for (src, want) in cases {
        let mut nb = null_notebook();
        let i = add_and_play(&mut nb, &format!("print({})", src));
        assert_eq!(computed(&nb, i), want, "input: {}", src);
    }
}

// ── Greek letters as user identifiers ──────────────────────────────────────
// Greek codepoints (except π/τ which are number constants) are `is_alphabetic`
// per Unicode and lex as ordinary `Identifier(_)` tokens — user code can bind
// them like any other name.

#[test]
fn latex_lowercase_greek_letters_bind_as_identifiers() {
    // Skip π (number constant) and τ (number constant). The rest of the
    // lowercase Greek alphabet should round-trip as identifiers.
    let chars = ['α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ',
                 'ν', 'ξ', 'ρ', 'σ', 'υ', 'φ', 'χ', 'ψ', 'ω'];
    for (n, ch) in chars.iter().enumerate() {
        let mut nb = null_notebook();
        let src = format!("{} := {}\nprint({})", ch, n + 1, ch);
        let i = add_and_play(&mut nb, &src);
        assert_eq!(
            computed(&nb, i),
            (n + 1).to_string(),
            "Greek letter {} did not round-trip",
            ch,
        );
    }
}

#[test]
fn latex_uppercase_greek_letters_bind_as_identifiers() {
    let chars = ['Γ', 'Δ', 'Θ', 'Λ', 'Ξ', 'Π', 'Σ', 'Φ', 'Ψ', 'Ω'];
    for (n, ch) in chars.iter().enumerate() {
        let mut nb = null_notebook();
        let src = format!("{} := {}\nprint({})", ch, n + 1, ch);
        let i = add_and_play(&mut nb, &src);
        assert_eq!(
            computed(&nb, i),
            (n + 1).to_string(),
            "Greek letter {} did not round-trip",
            ch,
        );
    }
}

// ── CAS-only identifiers ───────────────────────────────────────────────────
// ∫ → Identifier("integral"), ∑ → "sum", ∏ → "prod", ∂ → "partial",
// ⅆ → "derivative", ∇ → "nabla". The translator in
// src/lang/reduce/translate.rs maps these to REDUCE's int/df/etc. Each test
// drives a mock REDUCE response so we can assert the final Computed string
// without taking REDUCE's process-global CSL state.

#[test]
fn latex_integral_routes_through_cas() {
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(&mut nb, "print(∫(sin(x), x))");
    let cell_id = nb.cell(i).id;
    assert!(
        matches!(nb.cell(i).outcome.message, Some(CellMessage::Pending)),
        "∫ must park on Pending; got {:?}",
        nb.cell(i).outcome.message,
    );
    // Assert on the submitted REDUCE text, not just the canned response. The
    // mock previously discarded `expression`, so a malformed submission could
    // pass undetected as long as the canned reply was well-formed. With the
    // capture in place, this test verifies the translator actually emits
    // `int(...)` (not `integral(...)` — the lexer's display name).
    let subs = mock.submissions_for(cell_id);
    assert_eq!(subs.len(), 1, "expected one submission, got {:?}", subs);
    assert!(
        subs[0].expression.contains("int(sin(x)"),
        "submitted text should contain `int(sin(x)…)`; got {:?}",
        subs[0].expression,
    );
    mock.respond_to(cell_id, Ok("-cos(x)".to_string()));
    nb.tick();
    assert_eq!(computed(&nb, i), "-cos(x)");
}

#[test]
fn latex_derivative_routes_through_cas() {
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(&mut nb, "print(ⅆ(sin(x), x))");
    let cell_id = nb.cell(i).id;
    assert!(
        matches!(nb.cell(i).outcome.message, Some(CellMessage::Pending)),
        "ⅆ must park on Pending; got {:?}",
        nb.cell(i).outcome.message,
    );
    mock.respond_to(cell_id, Ok("cos(x)".to_string()));
    nb.tick();
    assert_eq!(computed(&nb, i), "cos(x)");
}

#[test]
fn latex_partial_routes_through_cas() {
    let (mut nb, mock) = mock_reduce_notebook();
    let i = add_and_play(&mut nb, "print(∂(sin(x), x))");
    let cell_id = nb.cell(i).id;
    assert!(
        matches!(nb.cell(i).outcome.message, Some(CellMessage::Pending)),
        "∂ must park on Pending; got {:?}",
        nb.cell(i).outcome.message,
    );
    mock.respond_to(cell_id, Ok("cos(x)".to_string()));
    nb.tick();
    assert_eq!(computed(&nb, i), "cos(x)");
}

#[test]
fn latex_infinity_lexes_as_identifier() {
    // ∞ → Identifier("infinity"). No binding to "infinity" exists at runtime,
    // so this should NOT produce a Computed value — but it MUST lex cleanly
    // (no "Unexpected character" diagnostic). This pins the lexer's contract.
    let mut nb = null_notebook();
    let i = add_and_play(&mut nb, "print(∞)");
    for msg in diag_messages(&nb, i) {
        assert!(
            !msg.contains("Unexpected character"),
            "∞ must lex without 'Unexpected character'; got: {}",
            msg,
        );
    }
}

// ── Coverage gate ──────────────────────────────────────────────────────────
// Every entry in the autocomplete LATEX_SYMBOLS table must produce text the
// lexer can tokenize. Inserting a codepoint the lexer rejects creates a
// broken cell the moment the user types the trigger — `\to` → `→` and the
// next keystroke produces "Unexpected character". This test fails if anyone
// adds a substitution whose Unicode value isn't lexable. Fix is either:
//   (a) trim the entry from LATEX_SYMBOLS, or
//   (b) extend the lexer to accept the codepoint (preferred when a real
//       semantic mapping exists, e.g. `≤` → Lte).

#[test]
fn every_latex_symbol_substitution_lexes() {
    use logos::editor::autocomplete::LATEX_SYMBOLS;
    use logos::lang::lexer::Lexer;

    let mut broken: Vec<String> = Vec::new();
    for &(cmd, sym) in LATEX_SYMBOLS {
        // Lex the bare substitution. Identifier-class codepoints would be
        // accepted as a token; operators/relations have explicit lexer cases.
        // Anything else triggers "Unexpected character".
        let mut lex = Lexer::new(sym);
        if let Err(e) = lex.tokenize() {
            broken.push(format!(
                "{cmd} → {sym:?} (U+{:04X}): {e}",
                sym.chars().next().map(|c| c as u32).unwrap_or(0),
            ));
        }
    }
    assert!(
        broken.is_empty(),
        "LATEX_SYMBOLS contains {} substitution(s) the lexer rejects. \
         Either remove from the table or extend the lexer.\n  {}",
        broken.len(),
        broken.join("\n  "),
    );
}
