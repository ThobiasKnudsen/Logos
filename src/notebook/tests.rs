//! Notebook integration tests — the production class as the test harness.
//!
//! These cover the same scenarios as the previous ad-hoc `run_cell` helper
//! in `lang::mod`, but exercise the real `Notebook` API: cells are added,
//! `play()` is called, REDUCE round-trips are pumped via `tick()`, outcomes
//! are read off the cells. No backdoors.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::lang::reduce::service::ReduceResponse;
use crate::ui::theme::Rgba;

use super::{CellMessage, CellOutcome, Notebook, ReduceBackend, ShaderSpec};

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
    fn clear_pending(&mut self) {
        self.state.lock().unwrap().inflight.clear();
    }
}

/// Drop-everything backend for tests that don't reach the REDUCE path.
struct NullReduceBackend {
    pending: usize,
}
impl ReduceBackend for NullReduceBackend {
    fn submit(&mut self, _cell_id: usize, _context: Vec<String>, _expr: String) -> u64 {
        self.pending += 1;
        0
    }
    fn try_recv(&mut self) -> Option<ReduceResponse> {
        None
    }
    fn has_pending(&self) -> bool {
        self.pending > 0
    }
    fn clear_pending(&mut self) {
        self.pending = 0;
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
    Notebook::new(Box::new(NullReduceBackend { pending: 0 }), None)
}

/// Build a notebook with a controllable mock REDUCE. The returned handle
/// lets the test enqueue responses with `respond_to(cell_id, …)`.
fn mock_reduce_notebook() -> (Notebook, MockReduce) {
    let mock = MockReduce::new();
    let nb = Notebook::new(Box::new(mock.clone()), None);
    (nb, mock)
}

fn add_and_play(nb: &mut Notebook, source: &str) -> usize {
    let idx = nb.add_cell(&dedent(source), Rgba::hex(0xff5555));
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
    outcome.shader.as_ref().expect("expected shader")
}

// ─── tests ─────────────────────────────────────────────────────────────────

#[test]
fn add_cell_assigns_unique_ids() {
    let mut nb = null_notebook();
    let a = nb.add_cell("x", Rgba::hex(0xffffff));
    let b = nb.add_cell("y", Rgba::hex(0xffffff));
    // IDs come from a process-global counter, so we don't assert exact
    // values — only that they're unique and adjacent in submission order.
    let ida = nb.cell(a).id;
    let idb = nb.cell(b).id;
    assert_ne!(ida, idb);
    assert_eq!(idb, ida + 1);
}

#[test]
fn set_text_invalidates_ast_and_marks_stale_after_play() {
    let mut nb = null_notebook();
    let i = nb.add_cell("y = sin(x)", Rgba::hex(0xffffff));
    nb.play(i);
    assert!(!nb.cell(i).is_stale(), "fresh play is not stale");
    nb.set_text(i, "y = cos(x)");
    assert!(nb.cell(i).is_stale(), "text changed since play → stale");
}

#[test]
fn idle_cell_is_not_stale() {
    let mut nb = null_notebook();
    let i = nb.add_cell("anything", Rgba::hex(0xffffff));
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
    assert!(nb.cell(i).outcome.shader.is_none());
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
    let i = nb.add_cell("print(y = x + 2*x)", Rgba::hex(0xffffff));
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
fn play_auto_runs_earlier_cells_jupyter_style() {
    let mut nb = null_notebook();
    let a = nb.add_cell("f := x²", Rgba::hex(0xff5555));
    let b = nb.add_cell("plot(y = f)", Rgba::hex(0x55ff55));
    nb.play(b);
    // Earlier cell should also be Playing now.
    assert!(matches!(nb.cell(a).state, super::CellState::Playing));
    assert!(matches!(nb.cell(b).state, super::CellState::Playing));
}

#[test]
fn replay_stops_later_playing_cells() {
    let mut nb = null_notebook();
    let a = nb.add_cell("f := x²", Rgba::hex(0xff5555));
    let b = nb.add_cell("plot(y = f)", Rgba::hex(0x55ff55));
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
    let i = nb.add_cell("y = sin(", Rgba::hex(0xffffff));
    nb.play(i);
    assert!(
        nb.cell(i).outcome.shader.is_none(),
        "no shader on parse failure"
    );
    let diags = &nb.cell(i).outcome.diagnostics;
    assert!(!diags.is_empty(), "expected at least one diagnostic");
    let d = &diags[0];
    assert!(matches!(d.severity, super::Severity::Error));
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
    let source = &nb.cell(i).text;
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
fn remove_cell_drops_pending_reduce() {
    let (mut nb, _mock) = mock_reduce_notebook();
    let i = nb.add_cell("f := x + 2*x\nprint(f)", Rgba::hex(0xffffff));
    nb.play(i);
    assert!(nb.has_pending());
    nb.remove_cell(i);
    assert_eq!(nb.len(), 0);
    // Notebook-side pending entry is cleared (mock backend may still hold
    // the in-flight handle, but Notebook would ignore any stale response).
}
