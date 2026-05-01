use bytemuck::{Pod, Zeroable};

use crate::lang::compute_gen;
use crate::lang::interpreter::ParallelForRequest;

const MAX_SCALARS: usize = 15;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParamsBuffer {
    len: u32,
    _pad: u32,
    scalars: [f32; MAX_SCALARS],
    _pad2: f32,
}

/// Execute a compute shader for a parallel-for request and read back results.
/// Returns updated (name, data) pairs for each read-write array.
pub fn dispatch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    request: &ParallelForRequest,
) -> Result<Vec<(String, Vec<f64>)>, String> {
    if request.scalars.len() > MAX_SCALARS {
        return Err(format!(
            "Too many scalar constants ({}, max {})",
            request.scalars.len(),
            MAX_SCALARS
        ));
    }

    let len = request.range_end - request.range_start;
    if len == 0 {
        return Ok(request
            .readwrite_arrays
            .iter()
            .map(|(name, _)| (name.clone(), Vec::new()))
            .collect());
    }

    // 1. Generate WGSL
    let wgsl = compute_gen::generate(request)?;

    // 2. Create shader module
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("compute_shader"),
        source: wgpu::ShaderSource::Wgsl(wgsl.clone().into()),
    });
    if let Some(err) = pollster::block_on(device.pop_error_scope()) {
        return Err(format!(
            "Compute shader error: {}\n\n--- WGSL ---\n{}",
            err, wgsl
        ));
    }

    // 3. Create buffers
    let buf_size = (len * std::mem::size_of::<f32>()) as u64;

    // Params uniform
    let mut params = ParamsBuffer::zeroed();
    params.len = len as u32;
    for (i, (_, val)) in request.scalars.iter().enumerate() {
        params.scalars[i] = *val as f32;
    }
    let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute_params"),
        size: std::mem::size_of::<ParamsBuffer>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));

    // Read-only storage buffers
    let mut readonly_buffers = Vec::new();
    for (_, data) in &request.readonly_arrays {
        let f32_data: Vec<f32> = data.iter().map(|&v| v as f32).collect();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compute_readonly"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&f32_data));
        readonly_buffers.push(buffer);
    }

    // Read-write storage buffers (need COPY_SRC for readback)
    let mut readwrite_buffers = Vec::new();
    for (_, data) in &request.readwrite_arrays {
        let f32_data: Vec<f32> = data.iter().map(|&v| v as f32).collect();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compute_readwrite"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&f32_data));
        readwrite_buffers.push(buffer);
    }

    // Staging buffers for readback (one per read-write array)
    let mut staging_buffers = Vec::new();
    for _ in &request.readwrite_arrays {
        staging_buffers.push(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compute_staging"),
            size: buf_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }

    // 4. Create bind group layout + pipeline
    let _total_bindings = compute_gen::binding_count(request);
    let mut layout_entries = Vec::new();

    // binding 0: params uniform
    layout_entries.push(wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });

    // Read-only storage buffers
    for i in 0..request.readonly_arrays.len() as u32 {
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1 + i,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }

    // Read-write storage buffers
    let rw_offset = 1 + request.readonly_arrays.len() as u32;
    for i in 0..request.readwrite_arrays.len() as u32 {
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: rw_offset + i,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("compute_bgl"),
        entries: &layout_entries,
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("compute_pipeline_layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("compute_pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: "main",
        compilation_options: Default::default(),
        cache: None,
    });
    if let Some(err) = pollster::block_on(device.pop_error_scope()) {
        return Err(format!("Compute pipeline creation error: {}", err));
    }

    // 5. Create bind group
    let mut bind_entries: Vec<wgpu::BindGroupEntry> = Vec::new();
    bind_entries.push(wgpu::BindGroupEntry {
        binding: 0,
        resource: params_buffer.as_entire_binding(),
    });
    for (i, buf) in readonly_buffers.iter().enumerate() {
        bind_entries.push(wgpu::BindGroupEntry {
            binding: 1 + i as u32,
            resource: buf.as_entire_binding(),
        });
    }
    for (i, buf) in readwrite_buffers.iter().enumerate() {
        bind_entries.push(wgpu::BindGroupEntry {
            binding: rw_offset + i as u32,
            resource: buf.as_entire_binding(),
        });
    }

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("compute_bind_group"),
        layout: &bind_group_layout,
        entries: &bind_entries,
    });

    // 6. Dispatch compute
    let wg = compute_gen::WORKGROUP_SIZE;
    let workgroups = (len as u32).div_ceil(wg);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("compute_encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("compute_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }

    // 7. Copy read-write buffers → staging
    for (i, rw_buf) in readwrite_buffers.iter().enumerate() {
        encoder.copy_buffer_to_buffer(rw_buf, 0, &staging_buffers[i], 0, buf_size);
    }
    queue.submit(Some(encoder.finish()));

    // 8. Readback all read-write arrays. Issue all map_async calls first,
    //    then a single device.poll. Previously we polled per buffer, which
    //    serialised N round trips to the driver.
    let mut receivers = Vec::with_capacity(request.readwrite_arrays.len());
    for staging in &staging_buffers {
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        receivers.push(rx);
    }
    device.poll(wgpu::Maintain::Wait);

    let mut results = Vec::with_capacity(request.readwrite_arrays.len());
    for (i, (name, _)) in request.readwrite_arrays.iter().enumerate() {
        receivers[i]
            .recv()
            .map_err(|e| format!("Readback channel error: {}", e))?
            .map_err(|e| format!("Buffer map error: {:?}", e))?;
        let slice = staging_buffers[i].slice(..);
        let data = slice.get_mapped_range();
        let f32_result: &[f32] = bytemuck::cast_slice(&data);
        let result_vec: Vec<f64> = f32_result.iter().map(|&v| v as f64).collect();
        drop(data);
        staging_buffers[i].unmap();

        results.push((name.clone(), result_vec));
    }

    Ok(results)
}
