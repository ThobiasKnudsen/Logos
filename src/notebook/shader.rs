//! GPU-side specification produced by the notebook for a single cell.
//!
//! The notebook emits either a fullscreen fragment shader (analytic
//! curves and surfaces, issues #24 / #25) or a vertex+fragment pair
//! backed by uploaded vertex data (vertex plots, issue #28). `ShaderSpec`
//! is the union; the renderer reads it back and chooses the appropriate
//! GPU pipeline shape.

#[derive(Debug, Clone)]
pub struct ShaderSpec {
    /// Complete WGSL source. For the analytic-plot path this is the
    /// fragment-only shader `wgsl_gen::generate` produces; for the
    /// vertex-plot path it is the canonical vertex+fragment pair
    /// `notebook::vertex_plot::shader()` returns.
    pub wgsl: String,
    /// Vertex data to bind for this plot. `None` for analytic plots —
    /// the renderer draws a fullscreen triangle and the fragment
    /// shader does all the work. `Some` for vertex plots — the
    /// renderer uploads the positions to a vertex buffer and draws
    /// them as a line strip.
    pub vertices: Option<VertexData>,
}

/// CPU-side vertex data for a vertex plot (issue #28).
#[derive(Debug, Clone)]
pub struct VertexData {
    /// Flat list of 2D positions in world coordinates. The vertex
    /// shader maps them to clip space using the standard axis-bounds
    /// uniform so panning / zooming the render area moves the plot
    /// along with the analytic shaders.
    pub positions: Vec<[f32; 2]>,
    /// Stable hash over `positions`. The renderer compares against
    /// the previously-uploaded hash for this cell pipeline and skips
    /// `queue.write_buffer` when unchanged — the explicit "re-upload
    /// only on change" half of the acceptance criteria.
    pub hash: u64,
}

impl VertexData {
    /// Build a `VertexData` from a position list, computing the hash
    /// once so callers don't accidentally diverge on the hash function.
    pub fn from_positions(positions: Vec<[f32; 2]>) -> Self {
        let hash = hash_positions(&positions);
        Self { positions, hash }
    }
}

/// Stable 64-bit hash over `positions`. We bitcast each `f32` to its
/// `u32` representation before hashing so the hash is deterministic
/// across runs (which `f32`'s `Hash` impl isn't — it would refuse NaN).
fn hash_positions(positions: &[[f32; 2]]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    positions.len().hash(&mut hasher);
    for [x, y] in positions {
        x.to_bits().hash(&mut hasher);
        y.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}
