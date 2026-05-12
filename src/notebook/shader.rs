//! GPU-side specification produced by the notebook for a single cell.
//!
//! Right now the notebook only emits fragment shaders; the renderer reads
//! `wgsl` and uses its standard `Uniforms` binding. `ShaderSpec` exists as a
//! nominal wrapper so the type system distinguishes "WGSL the notebook
//! produced" from arbitrary strings.

#[derive(Debug, Clone)]
pub struct ShaderSpec {
    /// Complete WGSL source (the same string `wgsl_gen::generate` produces).
    pub wgsl: String,
}
