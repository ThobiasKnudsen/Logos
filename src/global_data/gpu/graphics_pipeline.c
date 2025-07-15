// Full implementation for graphics_pipeline.c
#include "global_data/gpu/graphics_pipeline.h"
#include "global_data/gpu/gpu_device.h"
#include "global_data/gpu/shader.h"
#include <SDL3/SDL_gpu.h>
#include "tklog.h"
static const char* g_graphics_pipeline_type_key = "GraphicsPipeline";
static const bool g_graphics_pipeline_type_key_is_number = false;
typedef struct GraphicsPipeline {
    struct gd_base_node base;
    union gd_key vertex_shader_key;
    bool vertex_shader_key_is_number;
    union gd_key fragment_shader_key;
    bool fragment_shader_key_is_number;
    union gd_key gpu_device_key;
    bool gpu_device_key_is_number;
    SDL_GPUGraphicsPipeline* p_graphics_pipeline;
} GraphicsPipeline;
bool graphics_pipeline_free(struct gd_base_node* node) {
    GraphicsPipeline* p = caa_container_of(node, GraphicsPipeline, base);
    rcu_read_lock();
    struct gd_base_node* gpu_base = gd_node_get(p->gpu_device_key, p->gpu_device_key_is_number);
    rcu_read_unlock();
    if (gpu_base) {
        GPUDevice* gpu = caa_container_of(gpu_base, GPUDevice, base);
        SDL_ReleaseGPUGraphicsPipeline(gpu->p_gpu_device, p->p_graphics_pipeline);
    }
    return gd_base_node_free(node);
}
void graphics_pipeline_free_callback(struct rcu_head* head) {
    struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
    graphics_pipeline_free(node);
}
bool graphics_pipeline_is_valid(struct gd_base_node* node) {
    GraphicsPipeline* p = caa_container_of(node, GraphicsPipeline, base);
    return p->p_graphics_pipeline != NULL;
}
void graphics_pipeline_type_init() {
    gd_create_node_type(g_graphics_pipeline_type_key, sizeof(GraphicsPipeline), graphics_pipeline_free, graphics_pipeline_free_callback, graphics_pipeline_is_valid);
}
union gd_key graphics_pipeline_create(union gd_key vertex_shader_key, bool vertex_is_number, union gd_key fragment_shader_key, bool fragment_is_number, bool enable_debug) {
    rcu_read_lock();
    struct gd_base_node* vs_base = gd_node_get(vertex_shader_key, vertex_is_number);
    struct gd_base_node* fs_base = gd_node_get(fragment_shader_key, fragment_is_number);
    rcu_read_unlock();
    if (!vs_base || !fs_base) return gd_key_create(0, NULL, true);
    Shader* vs = caa_container_of(vs_base, Shader, base);
    Shader* fs = caa_container_of(fs_base, Shader, base);
    if (vs->gpu_device_key.number != fs->gpu_device_key.number || vs->gpu_device_key_is_number != fs->gpu_device_key_is_number) {
        tklog_error("shaders have different gpu devices");
        return gd_key_create(0, NULL, true);
    }
    struct gd_base_node* base = gd_base_node_create(0, true, gd_key_create(0, g_graphics_pipeline_type_key, false), false, sizeof(GraphicsPipeline));
    if (!base) return gd_key_create(0, NULL, true);
    GraphicsPipeline* p = caa_container_of(base, GraphicsPipeline, base);
    p->vertex_shader_key = vertex_shader_key;
    p->vertex_shader_key_is_number = vertex_is_number;
    p->fragment_shader_key = fragment_shader_key;
    p->fragment_shader_key_is_number = fragment_is_number;
    p->gpu_device_key = vs->gpu_device_key;
    p->gpu_device_key_is_number = vs->gpu_device_key_is_number;
    unsigned int vertex_attributes_count;
    unsigned int vertex_binding_stride;
    SDL_GPUVertexAttribute* vertex_attributes = shader_create_vertex_input_attrib_desc(vertex_shader_key, vertex_is_number, &vertex_attributes_count, &vertex_binding_stride);
    SDL_GPUGraphicsPipelineCreateInfo pipeline_create_info = {
        .target_info = {
            .num_color_targets = 1,
            .color_target_descriptions = (SDL_GPUColorTargetDescription[]){{
                .format = SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM,
                .blend_state = {
                    .enable_blend = true,
                    .src_color_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA,
                    .dst_color_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                    .color_blend_op = SDL_GPU_BLENDOP_ADD,
                    .src_alpha_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA,
                    .dst_alpha_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                    .alpha_blend_op = SDL_GPU_BLENDOP_ADD,
                    .color_write_mask = SDL_GPU_COLORCOMPONENT_R | SDL_GPU_COLORCOMPONENT_G | SDL_GPU_COLORCOMPONENT_B | SDL_GPU_COLORCOMPONENT_A,
                    .enable_color_write_mask = true,
                }
            }},
        },
        .vertex_input_state = {
            .vertex_buffer_descriptions = (SDL_GPUVertexBufferDescription[]){{
                .slot = 0,
                .pitch = vertex_binding_stride,
                .input_rate = SDL_GPU_VERTEXINPUTRATE_INSTANCE,
                .instance_step_rate = 1
            }},
            .num_vertex_buffers = 1,
            .vertex_attributes = vertex_attributes,
            .num_vertex_attributes = vertex_attributes_count,
        },
        .primitive_type = SDL_GPU_PRIMITIVETYPE_TRIANGLESTRIP,
        .vertex_shader = vs->p_sdl_shader,
        .fragment_shader = fs->p_sdl_shader,
    };
    rcu_read_lock();
    struct gd_base_node* gpu_base = gd_node_get(p->gpu_device_key, p->gpu_device_key_is_number);
    rcu_read_unlock();
    if (!gpu_base) { free(vertex_attributes); gd_base_node_free(base); return gd_key_create(0, NULL, true); }
    GPUDevice* gpu = caa_container_of(gpu_base, GPUDevice, base);
    p->p_graphics_pipeline = SDL_CreateGPUGraphicsPipeline(gpu->p_gpu_device, &pipeline_create_info);
    free(vertex_attributes);
    if (!p->p_graphics_pipeline) {
        tklog_error("Failed to create graphics pipeline: %s", SDL_GetError());
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    }
    if (!gd_node_insert(base)) {
        SDL_ReleaseGPUGraphicsPipeline(gpu->p_gpu_device, p->p_graphics_pipeline);
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    }
    printf("SUCCESSFULLY created graphics pipeline\n");
    return base->key;
}
void graphics_pipeline_destroy(union gd_key key, bool is_number) {
    gd_node_remove(key, is_number);
}
