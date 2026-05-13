//! Examples-through-real-REDUCE coverage (issue #18, gap 1).
//!
//! Lives in its own integration binary because REDUCE/CSL has process-
//! global state and can only be initialized once per process — sharing a
//! binary with `lang::reduce::csl::tests::test_reduce_session` (which
//! also takes a `CslSession`) would crash. This file spawns the full
//! `ReduceService` exactly once and runs *every* shipped `.logos` example
//! through `Notebook` against it, asserting:
//!
//!   1. The pipeline reaches a terminal state — Computed, shader emitted,
//!      or a structured Error — within `EXAMPLE_BUDGET`. No cell may sit
//!      on `Pending` forever; that was the failure mode #18 calls out.
//!   2. Every CAS-bearing example resolves to a non-empty WGSL fragment
//!      that naga accepts.
//!   3. No leaked user-function call (`f(...)`) or unresolved CAS keyword
//!      (`integral`, `int`, `df`, …) survives into the final WGSL.
//!
//! Co-runs with the existing `examples_render_through_notebook` test in
//! `tests/notebook.rs` (which uses `NoSimplifier` and accepts `Pending`
//! as a pass): together they cover the no-REDUCE and with-REDUCE shapes
//! of the same examples directory.

use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Duration, Instant};

use logos::lang::notebook_format::parse_logos;
use logos::lang::reduce::service::ReduceService;
use logos::notebook::{
    CellMessage, CellState, Notebook, ReduceSimplifier, SharedReduce,
};

/// Maximum wall-clock a single example is allowed to spend driving its
/// cells to completion. Real REDUCE simplifications are typically <100 ms,
/// so anything past a few seconds is a wedge — fail loudly.
const EXAMPLE_BUDGET: Duration = Duration::from_secs(20);

/// Per-tick polling interval while waiting for REDUCE responses. Small
/// enough that a fast simplification (REDUCE typically ~50 ms) doesn't add
/// meaningful test latency.
const POLL: Duration = Duration::from_millis(5);

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

/// Drive `nb` until the most recently played cell reaches a terminal state
/// (Computed / Simplified / Error, or a non-Pending message with at least
/// one shader). Returns the cell index that reached terminal state. Panics
/// if `EXAMPLE_BUDGET` elapses with the cell still Pending — that's the
/// "park on `…` forever" failure mode #19 (and the original #18 gap)
/// surfaced as a hard test failure.
fn pump_until_terminal(nb: &mut Notebook, idx: usize, label: &str) {
    let start = Instant::now();
    loop {
        nb.tick();
        let cell = nb.cell(idx);
        let pending = matches!(cell.outcome.message, Some(CellMessage::Pending));
        if !pending {
            return;
        }
        if start.elapsed() > EXAMPLE_BUDGET {
            panic!(
                "{}: cell still `Pending` after {:?} — real REDUCE never \
                 produced a response (or notebook didn't dispatch on it).\n\
                 diagnostics: {:?}",
                label,
                EXAMPLE_BUDGET,
                cell.outcome.diagnostics,
            );
        }
        std::thread::sleep(POLL);
    }
}

#[test]
fn examples_render_through_real_reduce() {
    // ReduceService spawns a worker thread that opens CslSession on its own
    // thread. Per the comment in `tests/notebook.rs`, only one CSL session
    // can live per process — so this entire test must own the service for
    // its full duration. Don't add a second #[test] to this file.
    let service = Rc::new(RefCell::new(ReduceService::new()));

    let examples_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let entries = std::fs::read_dir(&examples_dir)
        .unwrap_or_else(|e| panic!("read_dir({}): {}", examples_dir.display(), e));

    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "logos"))
        .collect();
    paths.sort(); // deterministic test ordering

    assert!(
        !paths.is_empty(),
        "no `.logos` files found in {}",
        examples_dir.display(),
    );

    for path in &paths {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let file = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{name}: read_to_string failed: {e}"));
        let cells = parse_logos(&file)
            .unwrap_or_else(|e| panic!("{name}: parse_logos failed: {e}"));

        // Each example gets a fresh Notebook bound to the shared service.
        // Cells inside the example share scope (later cells reference
        // earlier bindings) — same shape as the live editor.
        let simplifier = ReduceSimplifier::new(Box::new(SharedReduce::new(service.clone())));
        let mut nb = Notebook::new(Box::new(simplifier), None);

        for (idx, cell) in cells.into_iter().enumerate() {
            let label = format!("{name} cell[{idx}]");
            let i = nb.add_cell(&cell.content);
            nb.play(i);
            pump_until_terminal(&mut nb, i, &label);

            let outcome = &nb.cell(i).outcome;

            // No example should produce an error against real REDUCE. If
            // one starts to, that's a bug — the example needs fixing or
            // REDUCE needs the right declaration.
            assert!(
                outcome.diagnostics.is_empty(),
                "{label}: real-REDUCE run produced diagnostics: {:?}\nsource:\n{}",
                outcome.diagnostics,
                cell.content,
            );

            // Terminal state: must be one of Computed/Simplified, or have
            // emitted a shader (plot cells), or have transitioned to
            // Playing (pure binding cells).
            let has_shader = !outcome.shaders.is_empty();
            let has_message = matches!(
                outcome.message,
                Some(CellMessage::Computed(_))
                    | Some(CellMessage::Simplified(_)),
            );
            let is_playing = matches!(nb.cell(i).state, CellState::Playing);
            assert!(
                has_shader || has_message || is_playing,
                "{label}: cell reached no terminal state.\n\
                 message={:?} shaders={} state={:?}\nsource:\n{}",
                outcome.message,
                outcome.shaders.len(),
                nb.cell(i).state,
                cell.content,
            );

            for s in &outcome.shaders {
                assert!(
                    !s.wgsl.contains("integral(") && !s.wgsl.contains("int(")
                        && !s.wgsl.contains("derivative(") && !s.wgsl.contains("df("),
                    "{label}: unresolved CAS leaked into WGSL:\n{}",
                    s.wgsl,
                );
                validate_wgsl(&s.wgsl).unwrap_or_else(|e| {
                    panic!("{label}: WGSL invalid: {e}\nsource:\n{}", cell.content)
                });
            }
        }
    }
}
