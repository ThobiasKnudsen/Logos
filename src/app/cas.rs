use std::sync::Arc;

use crate::lang;

/// Bridges the interpreter's `GpuDispatch` trait to the real wgpu compute
/// pipeline. Owns shared `Arc` handles to the renderer's device and queue
/// so it can live on a `'static` `Notebook` without a borrow lifetime.
pub struct WgpuGpuDispatch {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl WgpuGpuDispatch {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self { device, queue }
    }
}

impl lang::interpreter::GpuDispatch for WgpuGpuDispatch {
    fn dispatch(
        &self,
        request: &lang::interpreter::ParallelForRequest,
    ) -> Result<Vec<(String, Vec<f64>)>, String> {
        crate::render::compute_pipeline::dispatch(&self.device, &self.queue, request)
    }
}

/// Extract a human-readable message from a wgpu shader error,
/// locating the problematic identifier in the user's source if possible.
pub(super) fn format_shader_error(raw: &str, source: &str) -> String {
    let ident = raw.lines().find_map(|line| {
        let t = line.trim();
        if t.starts_with("no definition in scope for identifier:") {
            t.rsplit('\'').nth(1).map(|s| s.to_string())
        } else {
            None
        }
    });

    if let Some(ref name) = ident {
        if let Some(offset) = find_ident_offset(source, name) {
            let msg = format!("Undefined function or variable '{}'", name);
            return crate::lang::format_error_at(source, offset, &msg);
        }
        return format!("Undefined function or variable '{}'", name);
    }

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("no definition") || trimmed.starts_with("unknown") {
            return format!("Shader error: {}", trimmed);
        }
        if let Some(pos) = trimmed.find("error: ") {
            return format!("Shader error: {}", &trimmed[pos + 7..]);
        }
        if let Some(pos) = trimmed.find("parsing error: ") {
            return format!("Shader error: {}", &trimmed[pos + 15..]);
        }
    }
    let first = raw.lines().find(|l| !l.trim().is_empty()).unwrap_or(raw);
    format!("Shader error: {}", first.trim())
}

/// Find the byte offset of an identifier in source (word-boundary aware).
fn find_ident_offset(source: &str, name: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = source[start..].find(name) {
        let abs = start + pos;
        let before_ok = abs == 0
            || !source.as_bytes()[abs - 1].is_ascii_alphanumeric()
                && source.as_bytes()[abs - 1] != b'_';
        let end = abs + name.len();
        let after_ok = end >= source.len()
            || !source.as_bytes()[end].is_ascii_alphanumeric() && source.as_bytes()[end] != b'_';
        if before_ok && after_ok {
            return Some(abs);
        }
        start = abs + 1;
    }
    None
}
