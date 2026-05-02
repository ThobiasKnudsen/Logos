# TODO

Items intentionally left unfinished by the post-Notebook-migration cleanup
(Phases A–F, commits `62320e3`..`3779f68`). Each is a one-to-three-line
change once the consumer side is written.

## Renderer / UI bindings

- **Wire per-cell `plot_color`.** `NotebookCell.plot_color` is set per cell
  but the renderer ignores it — every plot draws with `theme.primary_color`.
  When you want per-cell coloring, drop the `#[allow(dead_code)]` on the
  field and have `sync_active_tab` pass it through to `CellInfo`.
  - `src/notebook/cell.rs` (field), `src/app.rs::sync_active_tab` (consumer).

- **Show the replay affordance from `is_stale`.** `NotebookCell::is_stale()`
  returns true when a cell has been played and edited since. The UI doesn't
  bind to it yet — staleness is invisible in the cell header. Render a
  replay icon (or recolor the play button) when `cell.is_stale()` is true.
  - `src/notebook/cell.rs::is_stale`, `src/app.rs::sync_active_tab`,
    `src/render/mod.rs` (cell-header drawing).

- **Bind `Notebook::replay(idx)`.** Method exists but no UI calls it. Hook
  the replay icon click → `notebook.replay(idx)`. Drop the `#[allow]`.
  - `src/notebook/mod.rs::replay`, mouse-click dispatch in `app.rs`.

- **Render structured `Diagnostic`s as red squiggles.** `outcome.diagnostics`
  carry character-based row/col `Span`s suitable for underlining. The
  renderer currently shows only the single-string `outcome.message`. Once
  consumed, drop the `#[allow(dead_code)]` from `Diagnostic`'s fields and
  re-export `Severity` at the notebook crate level.
  - `src/notebook/diagnostic.rs`, `src/notebook/mod.rs` (re-export),
    `src/render/mod.rs` (squiggle drawing).

## Parallel / Monte-Carlo emission

- **Populate `ShaderSpec::bindings` and `DispatchKind::Compute`.** The
  notebook always emits `DispatchKind::Fragment` with empty `bindings`.
  When `parallel for`/array cells move from interpreter dispatch into a
  proper compute-shader emission path, the codegen needs to fill these in:
  storage buffer name, group/binding indices, access mode, element type,
  size. The struct shape is in place.
  - `src/notebook/shader.rs` (already has the types).
  - Codegen in `src/lang/compute_gen.rs` is what produces these today via
    side channels; lift the binding metadata into `ShaderSpec`.
  - Drop the per-type `#[allow(dead_code)]` on `BindingSpec`, `Access`,
    `ScalarType`, `SizeSpec` once consumed; re-export them from
    `src/notebook/mod.rs`.

## Notebook public API not yet bound

- **`Notebook::set_text(idx, text)` and `set_plot_color(idx, color)`** are
  public programmatic setters used only by tests. CLI / scripting
  consumers will want them; drop the `#[allow]`s when a real caller lands.

- **`Notebook::diagnostics()`** returns `Vec<(usize, &Diagnostic)>` — a flat
  "show all errors across the notebook" snapshot. The UI doesn't bind to
  this. Useful for a future "problems pane".

- **`Notebook::has_pending()`** is exposed for sync test loops. Production
  uses `state.reduce_service.borrow().has_pending()` directly. Either
  collapse them when convenient or leave as a backend-agnostic accessor.

## REDUCE backend

- **Per-tab REDUCE response routing.** Cell IDs are process-globally
  unique (`alloc_cell_id` AtomicUsize), so the shared `ReduceService`
  can't confuse responses across notebooks. But only the active tab's
  notebook is ticked — responses for an inactive tab arrive but are
  silently dropped because `pending_reduce` was cleared on tab switch.
  Background ticking (or a per-cell-id → `(tab_idx, cell_idx)` lookup)
  would let the inactive tab finish its work in the background.

## Future architectural

- **Variables with bounds.** Today `axis_bounds` lives on `NotebookView`
  as a `(xmin, ymin, xmax, ymax)` tuple. The user's longer-term model:
  every variable has its own `(min, max, resolution)`, and "axis-bound"
  is a role flag layered on top. When this lands, the bounds belong to
  the variable on the notebook engine side, and `NotebookView` keeps
  only viewport state (which variables are currently shown as x/y axes).

- **CPU JIT (Cranelift) pipeline.** `CellOutcome.program_ir` already
  exposes the Logos IR for every successful run. When CPU-side execution
  starts going through Cranelift, this is what the JIT consumes — adding
  type annotations and scope-resolved identifiers to `Ir` is the likely
  prerequisite, since cranelift wants typed values and SSA-able input.

- **`tests/` integration tests with real REDUCE.** REDUCE has process-
  global CSL state, so the existing `test_reduce_session` and the
  notebook tests can't both spin up a fresh service in the same test
  binary. A `tests/notebook_reduce.rs` integration test (separate
  binary, separate process) would exercise the real REDUCE path through
  the notebook end-to-end. Requires making the crate `lib + bin`.

## Naming nits

- **`Session` method names.** `new_tab` / `active_tab` / `close_tab`
  / `set_active` / `open_file` keep "tab" terminology because the
  tab bar is still a UI concept. Once the UI grows a non-tab window
  (split panes? popouts?) revisit.
