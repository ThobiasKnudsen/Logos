#include "global_data/gpu/graphics_pipeline.h"
#include "global_data/gpu/gpu_device.h"
#include "global_data/gpu/shader.h"
#include <SDL3/SDL_gpu.h>
#include "tklog.h"

static const char* g_graphics_pipeline_type_key = "GraphicsPipeline";
static const bool g_graphics_pipeline_type_key_is_number = false;
static const struct gd_key_ctx g_graphics_pipeline_type_key_ctx = { .key = { .string = (char*)g_graphics_pipeline_type_key }, .key_is_number = g_graphics_pipeline_type_key_is_number };

struct GraphicsPipeline {
    struct gd_base_node base;
    struct gd_key_ctx   vertex_shader_key_ctx;
    struct gd_key_ctx   fragment_shader_key_ctx;
    struct gd_key_ctx   gpu_device_key_ctx;
    SDL_GPUGraphicsPipeline* p_graphics_pipeline;
};

bool graphics_pipeline_free(struct gd_base_node* node) {
    GraphicsPipeline* p = (GraphicsPipeline*)node;
    gd_key_ctx_free(&p->vertex_shader_key_ctx);
    gd_key_ctx_free(&p->fragment_shader_key_ctx);
    gd_key_ctx_free(&p->gpu_device_key_ctx);
    rcu_read_lock();
    struct gd_base_node* gpu_base = gd_node_get(p->gpu_device_key_ctx);
    if (gpu_base) {
        if (p->p_graphics_pipeline) {
            GPUDevice* gpu = (GPUDevice*)gpu_base;
            SDL_ReleaseGPUGraphicsPipeline(gpu->p_gpu_device, p->p_graphics_pipeline);
        } else {
            tklog_warning("p->p_graphics_pipeline is NULL for number key %llu\n", node->key.number);
            tklog_scope()
        }
    }
    rcu_read_unlock();
    return gd_base_node_free(node);
}

void graphics_pipeline_free_callback(struct rcu_head* head) {
    struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
    tklog_scope(bool free_result = graphics_pipeline_free(node));
    if (!free_result) {
        if (node->key_is_number) {
            tklog_error("failed to free graphics_pipeline with key number %lld\n", node->key.number);
        } else {
            tklog_error("failed to freee graphics_pipeline with key string %s\n", node->key.string);
        }
    }
}

bool graphics_pipeline_is_valid(struct gd_base_node* node) {
    GraphicsPipeline* p = (GraphicsPipeline*)node;
    return p->p_graphics_pipeline != NULL;
}

bool graphics_pipeline_print_info(struct gd_base_node* p_base) {

    if (!gd_base_node_print_info(p_base)) {
        tklog_error("failed to print info of base node\n");
        return false;
    }

    struct GraphicsPipeline* p_pipe = caa_container_of(p_base, struct GraphicsPipeline, base);

    tklog_info("GraphicsPipeline:\n");
    
    if (p_pipe->vertex_shader_key_ctx.key_is_number) {
        tklog_info("    vertex_shader key: %lld\n", p_pipe->vertex_shader_key_ctx.key.number);
    } else {
        tklog_info("    vertex_shader key: %s\n", p_pipe->vertex_shader_key_ctx.key.string);
    }
    
    if (p_pipe->fragment_shader_key_ctx.key_is_number) {
        tklog_info("    fragment_shader key: %lld\n", p_pipe->fragment_shader_key_ctx.key.number);
    } else {
        tklog_info("    fragment_shader key: %s\n", p_pipe->fragment_shader_key_ctx.key.string);
    }
    
    if (p_pipe->gpu_device_key_ctx.key_is_number) {
        tklog_info("    gpu_device key: %lld\n", p_pipe->gpu_device_key_ctx.key.number);
    } else {
        tklog_info("    gpu_device key: %s\n", p_pipe->gpu_device_key_ctx.key.string);
    }

    tklog_info("    p_graphics_pipeline: %p\n", p_base->p_graphics_pipeline);

    return true;
}

void graphics_pipeline_type_init() {
    tklog_scope(struct gd_key_ctx type_key_ctx = gd_key_ctx_create(0, g_graphics_pipeline_type_key, false));
    tklog_scope(struct gd_base_node* p_base_node = gd_base_node_create(
        type_key_ctx, 
        gd_base_type_key_ctx_copy(), 
        sizeof(struct gd_base_type_node)));
    struct gd_base_type_node* p_type_node = caa_container_of(p_base_node, struct gd_base_type_node, base);
    p_type_node->fn_free_node = graphics_pipeline_free;
    p_type_node->fn_free_node_callback = graphics_pipeline_free_callback;
    p_type_node->fn_is_valid = graphics_pipeline_is_valid;
    p_type_node->fn_print_info = graphics_pipeline_print_info;
    p_type_node->type_size =  sizeof(GraphicsPipeline);
    tklog_scope(gd_node_insert(&p_type_node->base));
    tklog_scope(gd_key_ctx_free(&type_key_ctx));
}

struct gd_key_ctx graphics_pipeline_create(struct gd_key_ctx vertex_shader_key_ctx, struct gd_key_ctx fragment_shader_key_ctx) {
    rcu_read_lock();
    struct gd_base_node* vs_base = gd_node_get(vertex_shader_key_ctx);
    struct gd_base_node* fs_base = gd_node_get(fragment_shader_key_ctx);
    if (!vs_base || !fs_base) {
        tklog_error("could not find vertex or fragment shader node");
        rcu_read_unlock();
        return gd_key_ctx_create(0, NULL, true);
    }
    Shader* vs = (Shader*)vs_base;
    Shader* fs = (Shader*)fs_base;
    bool same_device = false;
    if (vs->gpu_device_key_ctx.key_is_number == fs->gpu_device_key_ctx.key_is_number) {
        if (vs->gpu_device_key_ctx.key_is_number) {
            same_device = (vs->gpu_device_key_ctx.key.number == fs->gpu_device_key_ctx.key.number);
        } else {
            same_device = (strcmp(vs->gpu_device_key_ctx.key.string, fs->gpu_device_key_ctx.key.string) == 0);
        }
    }
    if (!same_device) {
        tklog_error("shaders have different gpu devices");
        rcu_read_unlock();
        return gd_key_ctx_create(0, NULL, true);
    }
    struct gd_key_ctx pipe_key_ctx = gd_key_ctx_create(0, NULL, true); // Auto key
    struct gd_base_node* base = gd_base_node_create(pipe_key_ctx, g_graphics_pipeline_type_key_ctx, sizeof(GraphicsPipeline));
    gd_key_ctx_free(&pipe_key_ctx);
    if (!base) {
        tklog_error("failed to create graphics pipeline base node");
        rcu_read_unlock();
        return gd_key_ctx_create(0, NULL, true);
    }
    GraphicsPipeline* p = (GraphicsPipeline*)base;
    p->vertex_shader_key_ctx = vertex_shader_key_ctx;
    p->fragment_shader_key_ctx = fragment_shader_key_ctx;
    p->gpu_device_key_ctx = vs->gpu_device_key_ctx;
    unsigned int vertex_attributes_count = 0;
    unsigned int vertex_binding_stride = 0;
    SDL_GPUVertexAttribute* vertex_attributes = shader_create_vertex_input_attrib_desc(vertex_shader_key_ctx, &vertex_attributes_count, &vertex_binding_stride);
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
    struct gd_base_node* gpu_base = gd_node_get(p->gpu_device_key_ctx);
    if (!gpu_base) {
        free(vertex_attributes);
        tklog_scope(bool free_result = graphics_pipeline_free(base));
        if (!free_result) {
            tklog_error("failed to free graphics pipeline\n");
        }
        rcu_read_unlock();
        return gd_key_ctx_create(0, NULL, true);
    }
    GPUDevice* gpu = (GPUDevice*)gpu_base;
    p->p_graphics_pipeline = SDL_CreateGPUGraphicsPipeline(gpu->p_gpu_device, &pipeline_create_info);
    rcu_read_unlock();
    free(vertex_attributes);
    if (!p->p_graphics_pipeline) {
        tklog_error("Failed to create SDL3 graphics pipeline: %s", SDL_GetError());
        tklog_scope(bool free_result = graphics_pipeline_free(base));
        if (!free_result) {
            tklog_error("failed to free graphics pipeline\n");
        }
        return gd_key_ctx_create(0, NULL, true);
    }
    tklog_scope(bool insert_result = gd_node_insert(base));
    if (!insert_result) {
        tklog_scope(bool free_result = graphics_pipeline_free(base));
        if (!free_result) {
            tklog_error("failed to free graphics pipeline\n");
        }
        return gd_key_ctx_create(0, NULL, true);
    }
    tklog_info("Graphics Pipeline created successfully.");
    return (struct gd_key_ctx){ .key = base->key, .key_is_number = base->key_is_number };
}

void graphics_pipeline_destroy(struct gd_key_ctx key_ctx) {
    gd_node_remove(key_ctx);
}