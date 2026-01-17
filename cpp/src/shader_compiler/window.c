#include "sdl3/window.h"
#include "sdl3/gpu_device.h"
#include "sdl3/graphics_pipeline.h"
#include "code_monitoring.h"
#define STB_IMAGE_IMPLEMENTATION
#define STBI_MALLOC(sz)       cm_malloc ((sz), __CM_FILE_NAME__, __LINE__)
#define STBI_CALLOC(n, sz)    cm_calloc((n), (sz), __CM_FILE_NAME__, __LINE__)
#define STBI_REALLOC(p, sz)   cm_realloc((p), (sz), __CM_FILE_NAME__, __LINE__)
#define STBI_FREE(p)          do {if (p) cm_free((p), __CM_FILE_NAME__, __LINE__); } while(0);
#include "stb_image.h"

#include <limits.h>  // Defines PATH_MAX


// FPS constants for capping below 60 (e.g., 30 FPS)
#define kTargetFPS 20
#define kTargetFrameMS (1000 / kTargetFPS)  // ~33 ms for 30 FPS

static const struct tsm_key g_window_tsm_key    = { .key_union.string = "window_tsm", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_window_type_key   = { .key_union.string = "window_type", .key_type = TSM_KEY_TYPE_STRING };

struct sdl3_window {
	struct tsm_base_node 	base;
    SDL_Window*           	p_sdl_window;
};

typedef struct {
    struct { float x, y, w, h; }    rect;
    float                           rotation;
    float                           corner_radius_pixels;
    struct { unsigned char r, g, b, a; }    color;
    unsigned int                    tex_index;
    struct { float x, y, w, h; }    tex_rect;
} Rect;

static void _sdl3_window_type_free_callback(struct rcu_head* p_rcu) {
	CM_ASSERT(p_rcu);
	struct tsm_base_node* p_base = caa_container_of(p_rcu, struct tsm_base_node, rcu_head);
	struct sdl3_window* p_window = caa_container_of(p_base, struct sdl3_window, base);
	// Assume 'window' is your SDL_Window* from main thread (passed safely, e.g., via atomic or mutex)
	SDL_Event event;
	SDL_zero(event);  // Zero-initialize
	event.type = SDL_EVENT_WINDOW_CLOSE_REQUESTED;
	event.user.code = 1;  // Custom code for "destroy window"
	event.user.data1 = (void*)p_window->p_sdl_window;  // Pass window pointer
	// event.user.data2 = optional extra data;
	CM_ASSERT(SDL_PushEvent(&event));
	CM_SCOPE(tsm_base_node_free(p_base));
}
static CM_RES _sdl3_window_type_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
	CM_SCOPE(CM_RES res = tsm_base_node_is_valid(p_tsm_base, p_base));
	return res;
}
static CM_RES _sdl3_window_type_print(const struct tsm_base_node* p_base) {
	CM_ASSERT(p_base);
	tsm_base_node_print(p_base);
	struct sdl3_window* p_window = caa_container_of(p_base, struct sdl3_window, base);
	CM_ASSERT(p_window->p_sdl_window);
	CM_LOG_TSM_PRINT("    pointer to SDL3 Window: %p\n", p_window->p_sdl_window);
	return CM_RES_SUCCESS;
}

CM_RES sdl3_window_create(
    const struct tsm_key* p_key,
    unsigned int width,
    unsigned int height,
    const char* title)
{
    CM_ASSERT(SDL_IsMainThread());
    CM_ASSERT(title && p_key);

    const struct tsm_base_node* p_gpu_device_tsm = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_tsm_get(&p_gpu_device_tsm));

    const struct tsm_base_node* p_window_tsm = NULL;
    CM_SCOPE(CM_RES res = tsm_node_get(p_gpu_device_tsm, &g_window_tsm_key, &p_window_tsm));
    if (res == CM_RES_TSM_NODE_NOT_FOUND) {
        CM_LOG_NOTICE("Window TSM node doesnt exist\n");
        struct tsm_key window_tsm_key = {0};
        CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(&g_window_tsm_key, &window_tsm_key));
        struct tsm_base_node* p_new_window_tsm = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_gpu_device_tsm, &window_tsm_key, &p_new_window_tsm));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_gpu_device_tsm, p_new_window_tsm));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_gpu_device_tsm, &g_window_tsm_key, &p_window_tsm));

        // creating the window type node
        struct tsm_base_node* p_new_window_type = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
            &g_window_type_key,
            sizeof(struct tsm_base_type_node),
            _sdl3_window_type_free_callback,
            _sdl3_window_type_is_valid,
            _sdl3_window_type_print,
            sizeof(struct sdl3_window),
            &p_new_window_type));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_window_tsm, p_new_window_type));
    } else if (res != CM_RES_SUCCESS) {
        CM_LOG_ERROR("tsm_node_get failed with code %d\n", res);
    }

    // creating the window node
    struct tsm_base_node* p_new_window = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(p_key, &g_window_type_key, sizeof(struct sdl3_window), &p_new_window));
    struct sdl3_window* p_window = caa_container_of(p_new_window, struct sdl3_window, base);
    CM_ASSERT(p_window);
    CM_ASSERT(!p_window->p_sdl_window);
    p_window->p_sdl_window = SDL_CreateWindow(title, width, height, SDL_WINDOW_RESIZABLE);
    CM_ASSERT(p_window->p_sdl_window);
    SDL_GPUDevice* p_gpu_device = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_get(&p_gpu_device));
    CM_ASSERT(SDL_ClaimWindowForGPUDevice(p_gpu_device, p_window->p_sdl_window));
    CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_window_tsm, p_new_window));

    return CM_RES_SUCCESS;
}
CM_RES sdl3_window_get(const struct tsm_key* p_key, SDL_Window** pp_output_window) {
    CM_ASSERT(SDL_IsMainThread());
    CM_ASSERT(p_key && pp_output_window);

    const struct tsm_base_node* p_gpu_device_tsm = NULL; // get the gpu_device TSM
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_tsm_get(&p_gpu_device_tsm));
    const struct tsm_base_node* p_window_tsm = NULL; // get the window TSM
    CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_gpu_device_tsm, &g_window_tsm_key, &p_window_tsm));
    const struct tsm_base_node* p_window_base = NULL; // get the window
    CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_window_tsm, p_key, &p_window_base));
    struct sdl3_window* p_window = caa_container_of(p_window_base, struct sdl3_window, base);
    CM_ASSERT(p_window->p_sdl_window);

    *pp_output_window = p_window->p_sdl_window;
    return CM_RES_SUCCESS;
}

CM_RES sdl3_window_show_1(const struct tsm_key* p_window_key, const struct tsm_key* p_graphics_pipeline_key) {
    CM_ASSERT(SDL_IsMainThread());
    CM_ASSERT(p_window_key);
    CM_ASSERT(p_graphics_pipeline_key);

    // get the window
    SDL_Window* p_window = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_window_get(p_window_key, &p_window));
    // get the gpu device
    SDL_GPUDevice* p_gpu_device = NULL; 
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_get(&p_gpu_device));


    SDL_GPUTextureFormat color_format = SDL_GetGPUSwapchainTextureFormat(p_gpu_device, p_window);
    CM_ASSERT(SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM == color_format);

    // get the graphics pipeline
    const struct tsm_base_node* p_graphics_pipeline_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_graphics_pipeline_get(p_graphics_pipeline_key, &p_graphics_pipeline_base));
    struct sdl3_graphics_pipeline* p_graphics_pipeline = caa_container_of(p_graphics_pipeline_base, struct sdl3_graphics_pipeline, base);
    CM_ASSERT(p_graphics_pipeline->p_graphics_pipeline);

    Rect rects[2] = {
        { 
            .rect = { .x = 100.f, .y = 100.f, .w = 300.f, .h = 300.f },
            .rotation = 0.0f,
            .corner_radius_pixels = 20.f,
            .color = { .r = 200, .g = 200, .b = 200, .a = 255 },
            .tex_index = 1,
            .tex_rect = { .x = 0.0f, .y = 0.0f, .w = 1.0f, .h = 1.0f }
        }, { 
            .rect = { .x = 100.f, .y = 100.f, .w = 100.f, .h = 100.f },
            .rotation = 50.0f,
            .corner_radius_pixels = 20.f,
            .color = { .r = 200, .g = 200, .b = 200, .a = 100 },
            .tex_index = 0,
            .tex_rect = { .x = 0.0f, .y = 0.0f, .w = 1.0f, .h = 1.0f }
        }
    };
    unsigned int size = sizeof(rects);

    SDL_GPUTransferBufferCreateInfo transfer_info = {
        .usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD,
        .size = size
    };
    CM_SCOPE(SDL_GPUTransferBuffer* transfer_buffer = SDL_CreateGPUTransferBuffer(p_gpu_device, &transfer_info));
    CM_ASSERT(transfer_buffer);

    CM_SCOPE(void* mapped_data = SDL_MapGPUTransferBuffer(p_gpu_device, transfer_buffer, false));
    CM_ASSERT(mapped_data);
    memcpy(mapped_data, rects, size);
    CM_SCOPE(SDL_UnmapGPUTransferBuffer(p_gpu_device, transfer_buffer));
    SDL_GPUBufferCreateInfo buffer_create_info = {
        .usage = SDL_GPU_BUFFERUSAGE_VERTEX,
        .size = size
    };

    CM_SCOPE(SDL_GPUBuffer* gpu_buffer = SDL_CreateGPUBuffer(p_gpu_device, &buffer_create_info));
    CM_ASSERT(gpu_buffer);
    CM_SCOPE(SDL_GPUCommandBuffer* command_buffer = SDL_AcquireGPUCommandBuffer(p_gpu_device));
    CM_ASSERT(command_buffer);
    CM_SCOPE(SDL_GPUCopyPass* copy_pass = SDL_BeginGPUCopyPass(command_buffer));
    SDL_GPUTransferBufferLocation source = { transfer_buffer, 0 };
    SDL_GPUBufferRegion destination = { gpu_buffer, 0, size };
    CM_SCOPE(SDL_UploadToGPUBuffer(copy_pass, &source, &destination, false));
    CM_SCOPE(SDL_EndGPUCopyPass(copy_pass));
    CM_ASSERT(SDL_SubmitGPUCommandBuffer(command_buffer));
    SDL_ReleaseGPUTransferBuffer(p_gpu_device, transfer_buffer);

    // Load image using stb_image
    char absolute_path[PATH_MAX];
    CM_ASSERT(realpath("../resources/Bitcoin.png", absolute_path));
    printf("Loading image from: %s\n", absolute_path);

    int width, height, channels;
    CM_SCOPE(unsigned char* p_data = stbi_load("../resources/Bitcoin.png", &width, &height, &channels, STBI_rgb_alpha));
    CM_ASSERT(p_data);
    
    // Create a bitcoin_texture with the dimensions of the loaded image.
    SDL_GPUTextureCreateInfo tex_info = {
        .type = SDL_GPU_TEXTURETYPE_2D,
        .format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM,
        .usage = SDL_GPU_TEXTUREUSAGE_SAMPLER | SDL_GPU_TEXTUREUSAGE_COLOR_TARGET,
        .width = width,
        .height = height,
        .layer_count_or_depth = 1,
        .num_levels = 1,
        .sample_count = SDL_GPU_SAMPLECOUNT_1,
        .props = 0
    };
    SDL_GPUTexture* bitcoin_texture = SDL_CreateGPUTexture(p_gpu_device, &tex_info);
    CM_ASSERT(bitcoin_texture);

    // Create a transfer buffer sized to hold the entire image.
    size_t image_size = width * height * 4; // 4 bytes per pixel (RGBA)
    SDL_GPUTransferBufferCreateInfo tex_transfer_info = {
        .usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD,
        .size = image_size
    };
    SDL_GPUTransferBuffer* tex_transfer_buffer = SDL_CreateGPUTransferBuffer(p_gpu_device, &tex_transfer_info);
    CM_ASSERT(tex_transfer_buffer);

    // Map the transfer buffer and copy the image data into it.
    void* tex_map = SDL_MapGPUTransferBuffer(p_gpu_device, tex_transfer_buffer, false);
    CM_ASSERT(tex_map);
    memcpy(tex_map, p_data, image_size);
    SDL_UnmapGPUTransferBuffer(p_gpu_device, tex_transfer_buffer);

    // Define a bitcoin_texture region covering the entire bitcoin_texture.
    SDL_GPUTextureRegion tex_region = {
        .texture = bitcoin_texture,
        .mip_level = 0,
        .layer = 0,
        .x = 0,
        .y = 0,
        .w = width,
        .h = height,
        .d = 1
    };
    
    // Set up transfer info. Here, pixels_per_row and rows_per_layer match the image dimensions.
    SDL_GPUTextureTransferInfo tex_transfer = {
        .transfer_buffer = tex_transfer_buffer,
        .offset = 0,
        .pixels_per_row = width,
        .rows_per_layer = height
    };
    
    // Upload the bitcoin_texture data using a copy pass.
    SDL_GPUCommandBuffer* tex_cmd = SDL_AcquireGPUCommandBuffer(p_gpu_device);
    SDL_GPUCopyPass* tex_copy = SDL_BeginGPUCopyPass(tex_cmd);
    SDL_UploadToGPUTexture(tex_copy, &tex_transfer, &tex_region, false);
    SDL_EndGPUCopyPass(tex_copy);
    SDL_SubmitGPUCommandBuffer(tex_cmd);
    SDL_ReleaseGPUTransferBuffer(p_gpu_device, tex_transfer_buffer);
    // Create a bitcoin_sampler for the bitcoin_texture.
    SDL_GPUSamplerCreateInfo sampler_info = {
        .min_filter = SDL_GPU_FILTER_NEAREST,
        .mag_filter = SDL_GPU_FILTER_NEAREST,
        .mipmap_mode = SDL_GPU_SAMPLERMIPMAPMODE_NEAREST,
        .address_mode_u = SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE,
        .address_mode_v = SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE,
        .address_mode_w = SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE,
        .mip_lod_bias = 0.0f,
        .max_anisotropy = 1.0f,
        .compare_op = SDL_GPU_COMPAREOP_ALWAYS,
        .min_lod = 0.0f,
        .max_lod = 0.0f,
        .enable_anisotropy = false,
        .enable_compare = false,
        .props = 0
    };
    SDL_GPUSampler* bitcoin_sampler = SDL_CreateGPUSampler(p_gpu_device, &sampler_info);
    CM_ASSERT(bitcoin_sampler);
    // Free the loaded image data as it is now uploaded to the GPU.
    CM_SCOPE(stbi_image_free(p_data));
    // --- Main rendering loop ---
    bool running = true;
    while (running) {
        // Process events (quit if window is closed)
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED && event.user.code == 1) {
                SDL_Window* targetWindow = (SDL_Window*)event.user.data1;
                SDL_DestroyWindow(targetWindow);
                // Optional: Clean up other resources
            }
            if (event.type == SDL_EVENT_QUIT) {
                running = false;
            }
        }
        // Acquire a command buffer for the current frame.
        SDL_GPUCommandBuffer* cmd_buffer = SDL_AcquireGPUCommandBuffer(p_gpu_device);
        CM_ASSERT(cmd_buffer);

        SDL_GPUTexture *swapchain_tex = NULL;
        unsigned int tex_width = 0, tex_height = 0;
        CM_ASSERT(SDL_WaitAndAcquireGPUSwapchainTexture(cmd_buffer, p_window, &swapchain_tex, &tex_width, &tex_height));
        // Create dummy UBO data (for the vertex shader's UBO in set 1).
        typedef struct {
            float targetWidth;
            float targetHeight;
            float padding[2];
        } UniformBufferObject;
        UniformBufferObject vertex_ubo = { (float)tex_width, (float)tex_height, {0.0f, 0.0f} };

        // Push dummy uniform data onto the command buffer (for vertex shader UBO)
        // Push uniform data 
        SDL_PushGPUVertexUniformData(cmd_buffer, 0, &vertex_ubo, sizeof(vertex_ubo));
        // Set up the color target info for the render pass.
        SDL_GPUColorTargetInfo color_target = {0};
        color_target.texture = swapchain_tex;
        color_target.mip_level = 0;
        color_target.layer_or_depth_plane = 0;
        color_target.clear_color = (SDL_FColor){ .r = 0.0f, .g = 0.0f, .b = 0.0f, .a = 0.5f };
        color_target.load_op = SDL_GPU_LOADOP_CLEAR;
        color_target.store_op = SDL_GPU_STOREOP_STORE;
        color_target.resolve_texture = NULL;
        color_target.cycle = false;
        color_target.cycle_resolve_texture = false;

        // Begin the render pass.
        SDL_GPURenderPass *render_pass = SDL_BeginGPURenderPass(cmd_buffer, &color_target, 1, NULL);
        CM_ASSERT(render_pass);
        // Set the viewport to cover the entire swapchain texture.
        SDL_GPUViewport viewport = {
            .x = 0,
            .y = 0,
            .w = (float)tex_width,
            .h = (float)tex_height,
            .min_depth = 0.0f,
            .max_depth = 1.0f
        };
        CM_SCOPE(SDL_SetGPUViewport(render_pass, &viewport));
        // Bind the graphics pipeline.
        CM_SCOPE(SDL_BindGPUGraphicsPipeline(render_pass, p_graphics_pipeline->p_graphics_pipeline));
        // Bind the vertex buffer.
        SDL_GPUBufferBinding buffer_binding = { .buffer = gpu_buffer, .offset = 0 };
        SDL_BindGPUVertexBuffers(render_pass, 0, &buffer_binding, 1);
        // Bind dummy texture–sampler pairs for the fragment shader.
        SDL_GPUTextureSamplerBinding dummyTexBindings[8];
        for (int i = 0; i < 8; i++) {
            dummyTexBindings[i].texture = bitcoin_texture;
            dummyTexBindings[i].sampler = bitcoin_sampler;
        }
        // Bind samplers
        SDL_BindGPUFragmentSamplers(render_pass, 0, dummyTexBindings, 8);
        CM_SCOPE(SDL_DrawGPUPrimitives(render_pass, 4, 2, 0, 0));
        // End the render pass.
        SDL_EndGPURenderPass(render_pass);
        // Submit the command buffer so that the commands are executed on the GPU.
        CM_ASSERT(SDL_SubmitGPUCommandBuffer(cmd_buffer));

        // Optionally, delay to cap the frame rate (here ~60 FPS).
        // SDL_Delay(16);
    }

    SDL_ReleaseGPUBuffer(p_gpu_device, gpu_buffer);
    SDL_ReleaseGPUTexture(p_gpu_device, bitcoin_texture);
    SDL_ReleaseGPUSampler(p_gpu_device, bitcoin_sampler);
    // (Be sure to release/destroy your dummy resources when cleaning up.)

    return CM_RES_SUCCESS;
}
CM_RES sdl3_window_show(const struct tsm_key* p_window_key, const struct tsm_key* p_graphics_pipeline_key) {
    CM_ASSERT(SDL_IsMainThread());
    CM_ASSERT(p_window_key);
    CM_ASSERT(p_graphics_pipeline_key);
    // get the window
    SDL_Window* p_window = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_window_get(p_window_key, &p_window));
    // get the gpu device
    SDL_GPUDevice* p_gpu_device = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_get(&p_gpu_device));
    SDL_GPUTextureFormat color_format = SDL_GetGPUSwapchainTextureFormat(p_gpu_device, p_window);
    CM_ASSERT(SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM == color_format);
    // get the graphics pipeline
    const struct tsm_base_node* p_graphics_pipeline_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_graphics_pipeline_get(p_graphics_pipeline_key, &p_graphics_pipeline_base));
    struct sdl3_graphics_pipeline* p_graphics_pipeline = caa_container_of(p_graphics_pipeline_base, struct sdl3_graphics_pipeline, base);
    CM_ASSERT(p_graphics_pipeline->p_graphics_pipeline);

    // Record start time for absolute elapsed seconds
    Uint64 start_ticks = SDL_GetTicks();

    // Initialize interactive state
    int window_w = 0;
    int window_h = 0;
    SDL_GetWindowSize(p_window, &window_w, &window_h);
    int prev_w = window_w;
    int prev_h = window_h;
    float aspect = (float)window_w / (float)window_h;
    float initial_center_x = -0.7f;
    float initial_center_y = 0.0f;
    float initial_zoom = 0.2f;
    float half_width = 0.5f * aspect / initial_zoom;
    float half_height = 0.5f / initial_zoom;
    float min_x = initial_center_x - half_width;
    float max_x = initial_center_x + half_width;
    float min_y = initial_center_y - half_height;
    float max_y = initial_center_y + half_height;
    bool dragging = false;
    float last_mouse_x = 0.0f;
    float last_mouse_y = 0.0f;

    // --- Main rendering loop ---
    bool running = true;
    float time = 0.0f;
    while (running) {
        // Start frame timer
        Uint64 frame_start = SDL_GetTicks();

        // Process events (quit if window is closed)
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            switch (event.type) {
                case SDL_EVENT_WINDOW_CLOSE_REQUESTED:
                    if (event.user.code == 1) {
                        SDL_Window* targetWindow = (SDL_Window*)event.user.data1;
                        SDL_DestroyWindow(targetWindow);
                        // Optional: Clean up other resources
                    }
                    break;
                case SDL_EVENT_QUIT:
                    running = false;
                    break;
                case SDL_EVENT_WINDOW_RESIZED:
                    window_w = event.window.data1;
                    window_h = event.window.data2;
                    break;
                case SDL_EVENT_MOUSE_WHEEL: {
                    float mx, my;
                    float mouse_x, mouse_y;
                    SDL_GetMouseState(&mouse_x, &mouse_y);
                    mx = mouse_x / (float)window_w;
                    my = mouse_y / (float)window_h;
                    float world_x = min_x + mx * (max_x - min_x);
                    float world_y = min_y + my * (max_y - min_y);
                    float factor = powf(1.2f, (float)event.wheel.y);
                    if (factor == 1.0f) {
                        break;
                    }
                    float scale = 1.0f / factor;
                    float new_width_x = (max_x - min_x) * scale;
                    float new_height_y = (max_y - min_y) * scale;
                    min_x = world_x - mx * new_width_x;
                    max_x = min_x + new_width_x;
                    min_y = world_y - my * new_height_y;
                    max_y = min_y + new_height_y;
                    break;
                }
                case SDL_EVENT_MOUSE_BUTTON_DOWN:
                    if (event.button.button == SDL_BUTTON_LEFT) {
                        dragging = true;
                        SDL_GetMouseState(&last_mouse_x, &last_mouse_y);
                    }
                    break;
                case SDL_EVENT_MOUSE_BUTTON_UP:
                    if (event.button.button == SDL_BUTTON_LEFT) {
                        dragging = false;
                    }
                    break;
                case SDL_EVENT_MOUSE_MOTION:
                    if (dragging) {
                        float x = (float)event.motion.x;
                        float y = (float)event.motion.y;
                        float dx_screen = (x - last_mouse_x) / (float)window_w;
                        float dy_screen = (y - last_mouse_y) / (float)window_h;
                        float delta_x = dx_screen * (max_x - min_x);
                        float delta_y = dy_screen * (max_y - min_y);
                        min_x -= delta_x;
                        max_x -= delta_x;
                        min_y -= delta_y;
                        max_y -= delta_y;
                        last_mouse_x = x;
                        last_mouse_y = y;
                    }
                    break;
            }
        }
        // Update time to real elapsed seconds (absolute)
        Uint64 current_ticks = SDL_GetTicks();
        time = (float)((current_ticks - start_ticks) / 1000.0);

        // Update window size in case of resize without event (rare)
        SDL_GetWindowSize(p_window, &window_w, &window_h);
        if (window_w != prev_w || window_h != prev_h) {
            float new_aspect = (float)window_w / (float)window_h;
            float view_height = max_y - min_y;
            float new_width = new_aspect * view_height;
            float center_x = (min_x + max_x) / 2.0f;
            min_x = center_x - 0.5f * new_width;
            max_x = center_x + 0.5f * new_width;
            prev_w = window_w;
            prev_h = window_h;
        }

        // Acquire a command buffer for the current frame.
        SDL_GPUCommandBuffer* cmd_buffer = SDL_AcquireGPUCommandBuffer(p_gpu_device);
        CM_ASSERT(cmd_buffer);
        SDL_GPUTexture *swapchain_tex = NULL;
        unsigned int tex_width = 0, tex_height = 0;
        CM_ASSERT(SDL_WaitAndAcquireGPUSwapchainTexture(cmd_buffer, p_window, &swapchain_tex, &tex_width, &tex_height));

        typedef struct {
            float rect_x, rect_y, rect_w, rect_h;  // Pixel coords for the quad
            float screen_w, screen_h;              // For NDC normalization
            float padding[2];
        } VertexUBO;
        VertexUBO vertex_ubo = { 
            50.0f, 
            50.0f, 
            (float)tex_width - 100.0f, 
            (float)tex_height - 100.0f, 
            (float)tex_width, 
            (float)tex_height, 0.0f, 0.0f 
        };
        SDL_PushGPUVertexUniformData(cmd_buffer, 0, &vertex_ubo, sizeof(VertexUBO));
        
        // Define FragmentUBO struct matching GLSL std140 layout
        typedef struct {
            float time;
            float padding_1;
            float res_x;
            float res_y;
            float min_x;
            float max_x;
            float min_y;
            float max_y;
        } FragmentUBO;
        FragmentUBO fragment_ubo = { time, 0.0f, (float)tex_width, (float)tex_height, min_x, max_x, min_y, max_y };
        SDL_PushGPUFragmentUniformData(cmd_buffer, 0, &fragment_ubo, sizeof(FragmentUBO));
        
        // Set up the color target info for the render pass.
        SDL_GPUColorTargetInfo color_target = {0};
        color_target.texture = swapchain_tex;
        color_target.mip_level = 0;
        color_target.layer_or_depth_plane = 0;
        color_target.clear_color = (SDL_FColor){ .r = 0.0f, .g = 0.0f, .b = 0.0f, .a = 1.0f };
        color_target.load_op = SDL_GPU_LOADOP_CLEAR;
        color_target.store_op = SDL_GPU_STOREOP_STORE;
        color_target.resolve_texture = NULL;
        color_target.cycle = false;
        color_target.cycle_resolve_texture = false;
        // Begin the render pass.
        SDL_GPURenderPass *render_pass = SDL_BeginGPURenderPass(cmd_buffer, &color_target, 1, NULL);
        CM_ASSERT(render_pass);
        // Set the viewport to cover the entire swapchain texture.
        SDL_GPUViewport viewport = {
            .x = 0,
            .y = 0,
            .w = (float)tex_width,
            .h = (float)tex_height,
            .min_depth = 0.0f,
            .max_depth = 1.0f
        };
        SDL_SetGPUViewport(render_pass, &viewport);
        // Bind the graphics pipeline.
        SDL_BindGPUGraphicsPipeline(render_pass, p_graphics_pipeline->p_graphics_pipeline);
        // Draw one instance of the quad
        SDL_DrawGPUPrimitives(render_pass, 4, 1, 0, 0);
        // End the render pass.
        SDL_EndGPURenderPass(render_pass);
        // Submit the command buffer so that the commands are executed on the GPU.
        CM_ASSERT(SDL_SubmitGPUCommandBuffer(cmd_buffer));

        // End frame timer and cap FPS
        Uint64 frame_end = SDL_GetTicks();
        Uint64 frame_ms = frame_end - frame_start;
        if (frame_ms < kTargetFrameMS) {
            SDL_Delay(kTargetFrameMS - frame_ms);
        }
    }
    return CM_RES_SUCCESS;
}