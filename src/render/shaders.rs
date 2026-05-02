use std::sync::Arc;

use super::Renderer;

impl Renderer {
    pub fn compile_cell_shader(&mut self, cell_id: usize, wgsl_source: &str) -> Result<(), String> {
        self.shader_pipeline
            .compile_and_add(&self.device, cell_id, wgsl_source)
    }

    pub fn remove_cell_shader(&mut self, cell_id: usize) {
        self.shader_pipeline.remove(cell_id);
    }

    /// Clone the shared `Arc`s for device and queue. Used when something
    /// needs to outlive the renderer borrow — e.g. the notebook's
    /// `WgpuGpuDispatch`, which stores `'static` GPU handles.
    pub fn gpu_arcs(&self) -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        (self.device.clone(), self.queue.clone())
    }

    pub fn stash_tab_shaders(&mut self, tab_index: usize) {
        self.shader_pipeline.stash(tab_index);
    }

    pub fn restore_tab_shaders(&mut self, tab_index: usize) {
        self.shader_pipeline.restore(tab_index);
    }

    pub fn drop_stashed_tab_shaders(&mut self, tab_index: usize) {
        self.shader_pipeline.drop_stashed(tab_index);
    }

    pub fn has_active_shaders(&self) -> bool {
        self.shader_pipeline.has_active()
    }
}
