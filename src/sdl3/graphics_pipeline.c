#include "sdl3/graphics_pipeline.h"
#include "sdl3/gpu_device.h"
#include "sdl3/shader.h"

static const struct tsm_key g_sdl3_graphics_pipeline_type_key    = { .key_union.string = "sdl3_graphics_pipeline_type", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_sdl3_graphics_pipeline_tsm_key     = { .key_union.string = "sdl3_graphics_pipeline_tsm", .key_type = TSM_KEY_TYPE_STRING };

static void _sdl3_graphics_pipeline_type_free_callback(struct rcu_head* p_rcu) {
    CM_ASSERT(p_rcu);
    struct tsm_base_node* p_base = caa_container_of(p_rcu, struct tsm_base_node, rcu_head);
    struct sdl3_graphics_pipeline* p_graphics_pipeline = caa_container_of(p_base, struct sdl3_graphics_pipeline, base);

    CM_ASSERT(CM_RES_SUCCESS == tsm_key_free(&p_graphics_pipeline->vertex_shader_key));
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_free(&p_graphics_pipeline->fragment_shader_key));
    SDL_GPUDevice* p_gpu_device = NULL;
    rcu_read_lock();
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_get(&p_gpu_device));
    SDL_ReleaseGPUGraphicsPipeline(p_gpu_device, p_graphics_pipeline->p_graphics_pipeline);  
    rcu_read_unlock();
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(p_base));
}
static CM_RES _sdl3_graphics_pipeline_type_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    CM_SCOPE(CM_RES res = tsm_base_node_is_valid(p_tsm_base, p_base));
    if (res != CM_RES_TSM_NODE_IS_VALID) return res;
    struct sdl3_graphics_pipeline* p_graphics_pipeline = caa_container_of(p_base, struct sdl3_graphics_pipeline, base);
    CM_SCOPE(res = tsm_key_is_valid(&p_graphics_pipeline->vertex_shader_key));
    if (res != CM_RES_TSM_KEY_IS_VALID) return res;
    CM_SCOPE(res = tsm_key_is_valid(&p_graphics_pipeline->fragment_shader_key));
    if (res != CM_RES_TSM_KEY_IS_VALID) return res;
    return CM_RES_TSM_NODE_IS_VALID;
}
static CM_RES _sdl3_graphics_pipeline_type_print(const struct tsm_base_node* p_base) {
    CM_SCOPE(CM_RES res = tsm_base_node_print(p_base));
    struct sdl3_graphics_pipeline* p_graphics_pipeline = caa_container_of(p_base, struct sdl3_graphics_pipeline, base);
    CM_SCOPE(CM_RES_SUCCESS == tsm_key_print(&p_graphics_pipeline->vertex_shader_key));
    CM_SCOPE(CM_RES_SUCCESS == tsm_key_print(&p_graphics_pipeline->fragment_shader_key));
    return res;
}

CM_RES sdl3_graphics_pipeline_create(
	const struct tsm_key* p_key,
	const struct tsm_key* p_vertex_key,
	const struct tsm_key* p_fragment_key)
{
    CM_ASSERT(p_key && p_vertex_key && p_fragment_key);

    const struct tsm_base_node* p_vertex_base = NULL;
    const struct tsm_base_node* p_fragment_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_shader_get(p_vertex_key, &p_vertex_base));
    CM_ASSERT(CM_RES_SUCCESS == sdl3_shader_get(p_fragment_key, &p_fragment_base));
    const struct sdl3_shader* p_vertex = caa_container_of(p_vertex_base, struct sdl3_shader, base);
    const struct sdl3_shader* p_fragment = caa_container_of(p_fragment_base, struct sdl3_shader, base);

    // 1. Vertex Input State
    uint32_t vertex_attributes_count = 0;
    uint32_t vertex_binding_stride = 0;
    SDL_GPUVertexAttribute* vertex_attributes = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_shader_create_vertex_input_attribute_descriptions(
        p_vertex_key,  &vertex_attributes_count,   &vertex_binding_stride,  &vertex_attributes));

    SDL_GPUGraphicsPipelineCreateInfo pipeline_create_info = {
        .target_info = {
            .num_color_targets = 1,
            .color_target_descriptions = (SDL_GPUColorTargetDescription[]){{
                //.format = SDL_GetGPUSwapchainTextureFormat(p_gpu_device, p_sdl_window)
                .format = SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM,
                .blend_state = {
                    .enable_blend = true,
                    .src_color_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA,
                    .dst_color_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                    .color_blend_op = SDL_GPU_BLENDOP_ADD,        // Valid blend op for RGB
                    .src_alpha_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA,
                    .dst_alpha_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                    .alpha_blend_op = SDL_GPU_BLENDOP_ADD,        // Valid blend op for alpha
                    .color_write_mask = SDL_GPU_COLORCOMPONENT_R |
                                        SDL_GPU_COLORCOMPONENT_G |
                                        SDL_GPU_COLORCOMPONENT_B |
                                        SDL_GPU_COLORCOMPONENT_A,
                    .enable_color_write_mask = true,
                }
            }},
        },
        .vertex_input_state = {
            .vertex_buffer_descriptions = (SDL_GPUVertexBufferDescription[]) {
                {
                    .slot = 0,
                    .pitch = vertex_binding_stride,
                    .input_rate = SDL_GPU_VERTEXINPUTRATE_INSTANCE,
                    .instance_step_rate = 1
                }
            },
            .num_vertex_buffers = 1,
            .vertex_attributes = vertex_attributes,
            .num_vertex_attributes = vertex_attributes_count,
        },
        .primitive_type = SDL_GPU_PRIMITIVETYPE_TRIANGLESTRIP,
        .vertex_shader = p_vertex->p_sdl_shader,
        .fragment_shader = p_fragment->p_sdl_shader,
    };
    
    SDL_GPUDevice* p_gpu_device = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_get(&p_gpu_device));

    struct tsm_base_node* p_pipeline_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(p_key, &g_sdl3_graphics_pipeline_type_key, sizeof(struct sdl3_graphics_pipeline), &p_pipeline_base));
    struct sdl3_graphics_pipeline* p_pipeline = caa_container_of(p_pipeline_base, struct sdl3_graphics_pipeline, base);
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(p_vertex_key, &p_pipeline->vertex_shader_key));
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(p_fragment_key, &p_pipeline->fragment_shader_key));
    p_pipeline->p_graphics_pipeline = SDL_CreateGPUGraphicsPipeline(p_gpu_device, &pipeline_create_info);
    CM_ASSERT(p_pipeline->p_graphics_pipeline);
    free(vertex_attributes);
    
    // Get GPU device TSM (shaders are under it, so pipelines should be too).
    const struct tsm_base_node* p_gpu_device_tsm = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_tsm_get(&p_gpu_device_tsm));

    // Get or create pipeline TSM under GPU device TSM.
    static const struct tsm_key g_sdl3_graphics_pipeline_tsm_key = { .key_union.string = "sdl3_graphics_pipeline_tsm", .key_type = TSM_KEY_TYPE_STRING };
    const struct tsm_base_node* p_pipeline_tsm = NULL;
    CM_SCOPE(CM_RES res = tsm_node_get(p_gpu_device_tsm, &g_sdl3_graphics_pipeline_tsm_key, &p_pipeline_tsm));
    if (res != CM_RES_SUCCESS) {
        CM_ASSERT(res == CM_RES_TSM_NODE_NOT_FOUND);
        // Create the TSM.
        struct tsm_base_node* p_new_tsm = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_gpu_device_tsm, &g_sdl3_graphics_pipeline_tsm_key, &p_new_tsm));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_gpu_device_tsm, p_new_tsm));
        // Create the type inside the TSM.
        struct tsm_base_node* p_type = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
            &g_sdl3_graphics_pipeline_type_key,
            sizeof(struct tsm_base_type_node),
            _sdl3_graphics_pipeline_type_free_callback,
            _sdl3_graphics_pipeline_type_is_valid,
            _sdl3_graphics_pipeline_type_print,
            sizeof(struct sdl3_graphics_pipeline),
            &p_type));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_new_tsm, p_type));
        // Get the pipeline TSM again.
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_gpu_device_tsm, &g_sdl3_graphics_pipeline_tsm_key, &p_pipeline_tsm));
    }

    // Insert the pipeline node into the pipeline TSM.
    CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_pipeline_tsm, p_pipeline_base));

    CM_LOG_DEBUG("Graphics Pipeline created successfully.\n");
    return CM_RES_SUCCESS;
}
CM_RES sdl3_graphics_pipeline_get(
    const struct tsm_key* p_key,
    const struct tsm_base_node** p_output_pipeline) 
{
    CM_ASSERT(p_key);
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    // getting the gpu_device TSM as the shader TSM should be inside it
    const struct tsm_base_node* p_gpu_device_tsm = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_tsm_get(&p_gpu_device_tsm));
    // trying to get the shader TSM
    const struct tsm_base_node* p_graphics_pipeline_tsm = NULL;
    CM_SCOPE(CM_RES res = tsm_node_get(p_gpu_device_tsm, &g_sdl3_graphics_pipeline_tsm_key, &p_graphics_pipeline_tsm));
    if (res != CM_RES_SUCCESS) {
        return res;
    }
    // getting the shader
    CM_SCOPE(res = tsm_node_get(p_graphics_pipeline_tsm, p_key, p_output_pipeline));
    return res;
}