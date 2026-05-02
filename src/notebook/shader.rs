//! GPU-side specification produced by the notebook for a single cell.
//!
//! `ShaderSpec` is a *description* — the WGSL source plus the data and
//! dispatch shape the GPU caller needs to actually run it. The notebook
//! itself never allocates GPU resources; it just produces this spec, and the
//! renderer/caller reads it.
//!
//! For fragment shaders (the common case) `bindings` is empty — the renderer
//! already knows the standard `Uniforms` binding. For compute shaders
//! emitted from `parallel`/Monte-Carlo cells, `bindings` lists every storage
//! buffer the shader reads or writes, with name/access/element type/size.
//!
//! The `Compute`/`BindingSpec` half of this module is currently inert: the
//! notebook only emits `Fragment` dispatches. The types are stubbed in for
//! the parallel-emission path so the renderer can already pattern on
//! `DispatchKind`. Each is `#[allow(dead_code)]`-suppressed until then.

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ShaderSpec {
    /// Complete WGSL source (the same string `wgsl_gen::generate` produces today).
    pub wgsl: String,
    /// How the renderer should run this shader.
    pub dispatch: DispatchKind,
    /// Storage buffers the shader needs. Empty for fragment shaders.
    pub bindings: Vec<BindingSpec>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchKind {
    /// Fragment shader run over the plot viewport.
    Fragment,
    /// Compute shader (parallel for) — `workgroup_count` is the X-dimension.
    Compute { workgroup_count: u32 },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BindingSpec {
    pub group: u32,
    pub binding: u32,
    /// Logical name from the source (e.g. `samples`).
    pub name: String,
    pub access: Access,
    pub element_type: ScalarType,
    pub size: SizeSpec,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    F32,
    I32,
    U32,
    Vec2F32,
    Vec3F32,
    Vec4F32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeSpec {
    /// Compile-time-known element count.
    Static(usize),
    /// Element count comes from a uniform/parameter named at runtime.
    Dynamic(String),
}
