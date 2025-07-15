#include "global_data/gpu/window.h"
#include "global_data/core.h"
#include "global_data/gpu/gpu_device.h"
#include "global_data/gpu/graphics_pipeline.h"
#include "../../../include/cpi.h" // for Rect
#include "../../stb_image.h"
#include <limits.h>
#include <SDL3/SDL.h>
#include <stdatomic.h>
#include <sched.h>

#ifndef LOGOS_MAX_WINDOW_WIDTH
    #define LOGOS_MAX_WINDOW_WIDTH 8000
#endif
#ifndef LOGOS_MAX_WINDOW_HEIGHT
    #define LOGOS_MAX_WINDOW_HEIGHT 4000
#endif

// this is the type key for the GPU_Window node type and should always be used when accessing GPU_Window nodes
static const char* g_window_type_key = "Window";
static const bool g_window_type_key_is_number = false;

// you can use this struct after read critical section if youre able to set atomic_is_in_use from false to true
// then when done set atomic_is_in_use back to false
typedef struct Window {
    struct gd_base_node     base;
    SDL_Window*             p_sdl_window;
    atomic_bool             atomic_is_in_use;
    atomic_bool             atomic_should_close;
    bool                    device_key_is_number;
    union gd_key            gpu_device_key;
    SDL_ThreadID            thread_id;
} Window;

bool window_free(struct gd_base_node* node) {
    Window* p_window = caa_container_of(node, Window, base);
    if (p_window->p_sdl_window == NULL) {
        tklog_error("SDL_Window is NULL\n");
        return false;
    }
    if (atomic_load(&p_window->atomic_is_in_use)) {
        atomic_store(&p_window->atomic_should_close, true);
        tklog_warning("window is running when trying to free. will still be freed after window is shut down\n");
        while(atomic_load(&p_window->atomic_is_in_use)) {
            sched_yield();
        }
    }
    SDL_DestroyWindow(p_window->p_sdl_window);
    gd_base_node_free(node);
    return true;
}

void window_free_callback(struct rcu_head* rcu_head) {
    Window* p_window = caa_container_of(rcu_head, Window, base);
    tklog_scope(bool result = window_free(&p_window->base));
    if (!result) {
        tklog_error("Failed to free window\n");
    }
}

bool window_is_valid(struct gd_base_node* node) {
    Window* p_window = caa_container_of(node, Window, base);
    if (p_window->p_sdl_window == NULL) {
        tklog_notice("SDL_Window is NULL\n");
        return false;
    }
    rcu_read_lock();
    tklog_scope(struct gd_base_node* p_gpu_device = gd_get_node_unsafe(p_window->gpu_device_key, p_window->device_key_is_number));
    if (p_gpu_device == NULL) {
        tklog_notice("GPU device not found\n");
        return false;
    }
    rcu_read_unlock();
    return true;
}

void window_type_init() {
    tklog_scope(union gd_key type_key = gd_create_key(0, gd_key_get_string(g_window_type_key, g_window_type_key_is_number), g_window_type_key_is_number));
    tklog_scope(type_key = gd_create_node_type(type_key, false,
                                               sizeof(Window),
                                               window_free,
                                               window_free_callback,
                                               window_is_valid));
    if (type_key.string == NULL) {
        tklog_error("Failed to create window type\n");
        return;
    }
    tklog_info("Created window type with type_key: %s\n", g_window_type_key);
}

union gd_key window_create(
    union gd_key gpu_device_key,
    bool gpu_device_key_is_number,
    int width,
    int height,
    const char* title) 
{
    if (title == NULL) {
        tklog_error("title arguemnt is NULL\n");
        return gd_key_create(0, NULL, true);
    }
    if (width < 0 || width >= LOGOS_MAX_WINDOW_WIDTH) {
        tklog_error("given width arguemnt, which is %d, is too large or small\n", width);
        return gd_key_create(0, NULL, true);
    }
    if (height < 0 || height >= LOGOS_MAX_WINDOW_HEIGHT) {
        tklog_error("given height arguemnt, which is %d, is too large or small\n", height);
        return gd_key_create(0, NULL, true);
    }
    rcu_read_lock();
    tklog_scope(void* p_gpu_device = gd_node_get(gd_node_get, gpu_device_key_is_number));
    rcu_read_unlock();
    if (p_gpu_device == NULL) {
        if (gpu_device_key_is_number) {
            tklog_error("could not find gpu_device by key number %lld\n", gpu_device_key.number);
        } else {
            tklog_error("could not find gpu_device by key string %s\n", gpu_device_key.string);
        }
        return (union gd_key){.number=0};
    }

    Window* p_window = (Window*)calloc(1, sizeof(Window));
    if (p_window == NULL) {
        tklog_error("Failed to allocate memory for window\n");
        return gd_key_create(0, NULL, true);
    }

    tklog_scope(
        struct gd_base_node* p_window_base = 
            gd_base_node_create(
                gd_key_create(0, NULL, true), true,
                gd_key_create(0, g_window_type_key, false), false,
                sizeof(Window))
    );
    Window* p_window = caa_container_of(p_window_base, Window, base);
    
    // Set up GPU window specific fields
    p_window->device_key_is_number = gpu_device_key_is_number;
    p_window->gpu_device_key = gpu_device_key;
    p_window->thread_id = SDL_GetThreadID(NULL);
    p_window->p_sdl_window = SDL_CreateWindow(title, SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, width, height, SDL_WINDOW_RESIZABLE);
    if (!p_window->p_sdl_window) {
        tklog_error("Failed to create SDL window: %s\n", SDL_GetError());
        free(p_window);
        return gd_key_create(0, NULL, true);
    }
    
    // Insert the node into the global data system
    if (gd_node_insert(&p_window->base)) {
        tklog_error("Failed to insert GPU window into global data system\n");
        SDL_DestroyWindow(p_window->p_sdl_window);
        free(p_window);
        return gd_key_create(0, NULL, true);
    }
    
    tklog_info("Successfully created GPU window with key %llu\n", result_key.number);
    return result_key;
}

void window_show(
    union gd_key window_key,
    bool window_key_is_number,
    union gd_key graphics_pipeline_key,
    bool graphics_pipeline_key_is_number) 
{
    rcu_read_lock();

    // Window
    tklog_scope(struct gd_base_node* p_window_base = gd_node_get(window_key, window_key_is_number));
    if (p_window_base == NULL) {
        if (window_key_is_number) {
            tklog_error("window key number %lld doesnt exist\n", gd_key_get_number(window_key, window_key_is_number));
        } else {
            tklog_error("window key string %s doesnt exist\n", gd_key_get_string(window_key, window_key_is_number));
        }
        rcu_read_unlock();
        return;
    }
    Window* p_window = caa_container_of(p_window_base, Window, base);
    if (p_window->p_sdl_window == NULL) {
        tklog_error("SDL_Window is NULL\n");
        rcu_read_unlock();
        return;
    }
    if (atomic_load(&p_window->atomic_is_in_use)) {
        tklog_error("window is already in use\n");
        rcu_read_unlock();
        return;
    }
    if (atomic_load(&p_window->atomic_should_close)) {
        tklog_error("window should close\n");
        rcu_read_unlock();
        return;
    }
    atomic_store(&p_window->atomic_is_in_use, true);

    // GPU Device
    tklog_scope(struct gd_base_node* p_gpu_device_base = gd_node_get(p_window->gpu_device_key, p_window->device_key_is_number));
    if (p_gpu_device_base == NULL) {
        tklog_error("gpu device not found\n");
        atomic_store(&p_window->atomic_is_in_use, false);
        rcu_read_unlock();
        return;
    }
    GPUDevice* p_gpu_device = caa_container_of(p_gpu_device_base, GPUDevice, base);

    SDL_GPUTextureFormat color_format = SDL_GetGPUSwapchainTextureFormat(p_gpu_device->p_gpu_device, p_window->p_sdl_window);
    if (SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM != color_format) {
        tklog_error("not correct color format");
        atomic_store(&p_window->atomic_is_in_use, false);
        rcu_read_unlock();
        return;
    }

    // Graphics Pipeline
    tklog_scope(struct gd_base_node* p_graphics_pipeline_base = gd_node_get(graphics_pipeline_key, graphics_pipeline_key_is_number));
    if (p_graphics_pipeline_base == NULL) {
        if (p_graphics_pipeline_key_is_number) {
            tklog_error("graphics pipleine key number %lld doesnt exist\n", gd_key_get_number(p_graphics_pipeline_key, p_graphics_pipeline_key_is_number));
        } else {
            tklog_error("graphics pipeline window key string %s doesnt exist\n", gd_key_get_string(p_graphics_pipeline_key, p_graphics_pipeline_key_is_number));
        }
        atomic_store(&p_window->atomic_is_in_use, false);
        rcu_read_unlock();
        return;
    }
    GraphicsPipeline* p_graphics_pipeline = caa_container_of(p_graphics_pipeline_base, GraphicsPipeline, base);
    if (!p_graphics_pipeline->p_graphics_pipeline) {
        tklog_critical("NULL pointer");
        atomic_store(&p_window->atomic_is_in_use, false);
        rcu_read_unlock();
        return;
    }
    rcu_read_unlock();
    Rect rects[2] = {
        { .rect = { .x = 100.f, .y = 100.f, .w = 300.f, .h = 300.f }, .rotation = 0.0f, .corner_radius_pixels = 20.f, .color = { .r = 200, .g = 200, .b = 200, .a = 255 }, .tex_index = 1, .tex_rect = { .x = 0.0f, .y = 0.0f, .w = 1.0f, .h = 1.0f } },
        { .rect = { .x = 100.f, .y = 100.f, .w = 100.f, .h = 100.f }, .rotation = 50.0f, .corner_radius_pixels = 20.f, .color = { .r = 200, .g = 200, .b = 200, .a = 100 }, .tex_index = 0, .tex_rect = { .x = 0.0f, .y = 0.0f, .w = 1.0f, .h = 1.0f } }
    };
    unsigned int size = sizeof(rects);
    SDL_GPUTransferBufferCreateInfo transfer_info = { .usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD, .size = size };
    SDL_GPUTransferBuffer* transfer_buffer = SDL_CreateGPUTransferBuffer(p_gpu_device->p_gpu_device, &transfer_info);
    if (!transfer_buffer) { 
        tklog_critical("Failed to create transfer buffer: %s", SDL_GetError()); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    void* mapped_data = SDL_MapGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer, false);
    if (!mapped_data) { 
        tklog_critical("Failed to map transfer buffer: %s", SDL_GetError()); 
        SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    memcpy(mapped_data, rects, size);
    SDL_UnmapGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer);
    SDL_GPUBufferCreateInfo buffer_create_info = { .usage = SDL_GPU_BUFFERUSAGE_VERTEX, .size = size };
    SDL_GPUBuffer* gpu_buffer = SDL_CreateGPUBuffer(p_gpu_device->p_gpu_device, &buffer_create_info);
    if (!gpu_buffer) { 
        tklog_critical("Failed to create GPU buffer: %s", SDL_GetError()); 
        SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    SDL_GPUCommandBuffer* command_buffer = SDL_AcquireGPUCommandBuffer(p_gpu_device->p_gpu_device);
    if (!command_buffer) { 
        tklog_critical("Failed to acquire command buffer: %s", SDL_GetError()); 
        SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return;
    }
    SDL_GPUCopyPass* copy_pass = SDL_BeginGPUCopyPass(command_buffer);
    SDL_GPUTransferBufferLocation source = { transfer_buffer, 0 };
    SDL_GPUBufferRegion destination = { gpu_buffer, 0, size };
    SDL_UploadToGPUBuffer(copy_pass, &source, &destination, false);
    SDL_EndGPUCopyPass(copy_pass);
    if (!SDL_SubmitGPUCommandBuffer(command_buffer)) { 
        tklog_critical("Failed to submit command buffer: %s", SDL_GetError()); 
        SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer);
    
    char absolute_path[PATH_MAX];
    if (!realpath("../resources/Bitcoin.png", absolute_path)) { 
        tklog_error("realpath failed"); SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    printf("Loading image from: %s\n", absolute_path);
    int img_width, img_height, channels;
    unsigned char* p_data = stbi_load(absolute_path, &img_width, &img_height, &channels, STBI_rgb_alpha);
    if (!p_data) { 
        tklog_error("Failed to load image file: %s", absolute_path); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    SDL_GPUTextureCreateInfo tex_info = { .type = SDL_GPU_TEXTURETYPE_2D, .format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM, .usage = SDL_GPU_TEXTUREUSAGE_SAMPLER | SDL_GPU_TEXTUREUSAGE_COLOR_TARGET, .width = img_width, .height = img_height, .layer_count_or_depth = 1, .num_levels = 1, .sample_count = SDL_GPU_SAMPLECOUNT_1, .props = 0 };
    SDL_GPUTexture* bitcoin_texture = SDL_CreateGPUTexture(p_gpu_device->p_gpu_device, &tex_info);
    if (!bitcoin_texture) { 
        tklog_critical("Failed to create texture: %s", SDL_GetError()); 
        stbi_image_free(p_data); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    size_t image_size = img_width * img_height * 4;
    SDL_GPUTransferBufferCreateInfo tex_transfer_info = { .usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD, .size = image_size };
    SDL_GPUTransferBuffer* tex_transfer_buffer = SDL_CreateGPUTransferBuffer(p_gpu_device->p_gpu_device, &tex_transfer_info);
    if (!tex_transfer_buffer) { 
        tklog_critical("Failed to create transfer buffer: %s", SDL_GetError()); 
        stbi_image_free(p_data); 
        SDL_ReleaseGPUTexture(p_gpu_device->p_gpu_device, bitcoin_texture); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    void* tex_map = SDL_MapGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer, false);
    if (!tex_map) { 
        tklog_critical("Failed to map transfer buffer: %s", SDL_GetError()); 
        stbi_image_free(p_data); 
        SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer); 
        SDL_ReleaseGPUTexture(p_gpu_device->p_gpu_device, bitcoin_texture); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    memcpy(tex_map, p_data, image_size);
    SDL_UnmapGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer);
    SDL_GPUTextureRegion tex_region = { .texture = bitcoin_texture, .mip_level = 0, .layer = 0, .x = 0, .y = 0, .w = img_width, .h = img_height, .d = 1 };
    SDL_GPUTextureTransferInfo tex_transfer = { .transfer_buffer = tex_transfer_buffer, .offset = 0, .pixels_per_row = img_width, .rows_per_layer = img_height };
    SDL_GPUCommandBuffer* tex_cmd = SDL_AcquireGPUCommandBuffer(p_gpu_device->p_gpu_device);
    if (!tex_cmd) { 
        tklog_critical("Failed to acquire command buffer: %s", SDL_GetError()); 
        stbi_image_free(p_data); 
        SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer); 
        SDL_ReleaseGPUTexture(p_gpu_device->p_gpu_device, bitcoin_texture); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    SDL_GPUCopyPass* tex_copy = SDL_BeginGPUCopyPass(tex_cmd);
    SDL_UploadToGPUTexture(tex_copy, &tex_transfer, &tex_region, false);
    SDL_EndGPUCopyPass(tex_copy);
    if (!SDL_SubmitGPUCommandBuffer(tex_cmd)) { 
        tklog_critical("Failed to submit command buffer: %s", SDL_GetError()); 
        stbi_image_free(p_data); 
        SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer); 
        SDL_ReleaseGPUTexture(p_gpu_device->p_gpu_device, bitcoin_texture); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer);
    SDL_GPUSamplerCreateInfo sampler_info = { .min_filter = SDL_GPU_FILTER_NEAREST, .mag_filter = SDL_GPU_FILTER_NEAREST, .mipmap_mode = SDL_GPU_SAMPLERMIPMAPMODE_NEAREST, .address_mode_u = SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE, .address_mode_v = SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE, .address_mode_w = SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE, .mip_lod_bias = 0.0f, .max_anisotropy = 1.0f, .compare_op = SDL_GPU_COMPAREOP_ALWAYS, .min_lod = 0.0f, .max_lod = 0.0f, .enable_anisotropy = false, .enable_compare = false, .props = 0 };
    SDL_GPUSampler* bitcoin_sampler = SDL_CreateGPUSampler(p_gpu_device->p_gpu_device, &sampler_info);
    if (!bitcoin_sampler) { 
        tklog_critical("Failed to create sampler: %s", SDL_GetError()); 
        stbi_image_free(p_data); 
        SDL_ReleaseGPUTexture(p_gpu_device->p_gpu_device, bitcoin_texture); 
        SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer); 
        atomic_store(&p_window->atomic_is_in_use, false); 
        return; 
    }
    stbi_image_free(p_data);
    bool running = true;
    while (running) {
        if (atomic_load(&p_window->atomic_should_close)) { 
            running = false; 
            continue; 
        }
        SDL_Event event;
        while (SDL_PollEvent(&event)) { 
            if (event.type == SDL_EVENT_QUIT) { 
                running = false; 
            } 
        }
        SDL_GPUCommandBuffer* cmd_buffer = SDL_AcquireGPUCommandBuffer(p_gpu_device->p_gpu_device);
        if (!cmd_buffer) { 
            tklog_error("Failed to acquire command buffer: %s", SDL_GetError()); 
            running = false; 
            continue; 
        }
        SDL_GPUTexture *swapchain_tex = NULL;
        unsigned int tex_width = 0, tex_height = 0;
        if (!SDL_WaitAndAcquireGPUSwapchainTexture(cmd_buffer, p_window->p_sdl_window, &swapchain_tex, &tex_width, &tex_height)) { 
            tklog_error("Failed to acquire swapchain texture: %s", SDL_GetError()); 
            running = false; 
            continue; 
        }
        typedef struct { 
            float targetWidth; 
            float targetHeight; 
            float padding[2]; 
        } UniformBufferObject;
        UniformBufferObject dummyUBO = { 
            (float)tex_width, 
            (float)tex_height, 
            {0.0f, 0.0f} 
        };
        SDL_PushGPUVertexUniformData(cmd_buffer, 0, &dummyUBO, sizeof(dummyUBO));
        SDL_GPUColorTargetInfo color_target = { 
            .texture = swapchain_tex, 
            .mip_level = 0, 
            .layer_or_depth_plane = 0, 
            .clear_color = (SDL_FColor){ 
                .r = 0.0f, 
                .g = 0.0f, 
                .b = 0.0f, 
                .a = 0.5f }, 
            .load_op = SDL_GPU_LOADOP_CLEAR, 
            .store_op = SDL_GPU_STOREOP_STORE, 
            .cycle = false 
        };
        SDL_GPURenderPass *render_pass = SDL_BeginGPURenderPass(cmd_buffer, &color_target, 1, NULL);
        if (!render_pass) { 
            tklog_error("Failed to begin render pass: %s", SDL_GetError()); 
            running = false; 
            continue; 
        }
        SDL_GPUViewport viewport = { 
            .x = 0, 
            .y = 0, 
            .w = (float)tex_width, 
            .h = (float)tex_height, 
            .min_depth = 0.0f, 
            .max_depth = 1.0f 
        };
        SDL_SetGPUViewport(render_pass, &viewport);
        SDL_BindGPUGraphicsPipeline(render_pass, p_graphics_pipeline->p_graphics_pipeline);
        SDL_GPUBufferBinding buffer_binding = { .buffer = gpu_buffer, .offset = 0 };
        SDL_BindGPUVertexBuffers(render_pass, 0, &buffer_binding, 1);
        SDL_GPUTextureSamplerBinding dummyTexBindings[8];
        for (int i = 0; i < 8; i++) { 
            dummyTexBindings[i].texture = bitcoin_texture; 
            dummyTexBindings[i].sampler = bitcoin_sampler; 
        }
        SDL_BindGPUFragmentSamplers(render_pass, 0, dummyTexBindings, 8);
        SDL_DrawGPUPrimitives(render_pass, 4, 2, 0, 0);
        SDL_EndGPURenderPass(render_pass);
        if (!SDL_SubmitGPUCommandBuffer(cmd_buffer)) { 
            tklog_error("Failed to submit command buffer: %s", SDL_GetError()); 
            running = false; 
            continue; 
        }
    }
    SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer);
    SDL_ReleaseGPUTexture(p_gpu_device->p_gpu_device, bitcoin_texture);
    SDL_ReleaseGPUSampler(p_gpu_device->p_gpu_device, bitcoin_sampler);
    atomic_store(&p_window->atomic_is_in_use, false);
}
