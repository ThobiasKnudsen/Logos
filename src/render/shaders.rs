use std::sync::Arc;

use super::Renderer;

impl Renderer {
    /// Compile and install one pipeline per `(primary_color, wgsl)` pair for
    /// `cell_id`. Replaces any pipelines previously installed for this cell.
    /// Used by the multi-plot path: each `plot(...)` in a cell becomes its
    /// own pipeline.
    pub fn set_cell_shaders(
        &mut self,
        cell_id: usize,
        sources: &[([f32; 4], &str)],
    ) -> Result<(), String> {
        self.shader_pipeline
            .set_cell_shaders(&self.device, cell_id, sources)
    }

    /// Update the plot color used by every pipeline for `cell_id`. Cheap —
    /// just a uniform write next frame.
    pub fn set_cell_primary_color(&mut self, cell_id: usize, primary_color: [f32; 4]) {
        self.shader_pipeline
            .set_cell_primary_color(cell_id, primary_color);
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

    pub fn stash_tab_shaders(&mut self, tab_id: u64) {
        self.shader_pipeline.stash(tab_id);
    }

    pub fn restore_tab_shaders(&mut self, tab_id: u64) {
        self.shader_pipeline.restore(tab_id);
    }

    pub fn drop_stashed_tab_shaders(&mut self, tab_id: u64) {
        self.shader_pipeline.drop_stashed(tab_id);
    }

    /// Drop all currently-active shader pipelines (no stash). Used when the
    /// active tab is being closed — its pipelines have nowhere to go.
    pub fn clear_active_shaders(&mut self) {
        self.shader_pipeline.clear_active();
    }

    pub fn has_active_shaders(&self) -> bool {
        self.shader_pipeline.has_active()
    }
}
