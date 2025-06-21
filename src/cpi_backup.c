#define STB_IMAGE_IMPLEMENTATION
#include "stb_image.h"

#include "cpi.h"
#include "vec.h"
#include "vec_path.h"
#include <stdlib.h>
#include <SDL3/SDL.h>
#include <SDL3_shadercross/SDL_shadercross.h>
#include "spirv_reflect.h"
#include "tklog.h"

// ===============================================================================================================
// CPU states
// ===============================================================================================================
typedef struct CPI_Pointer {
#ifdef DEBUG
    unsigned long long              id;
#endif
    void*                           ptr;
    bool                            read;
    bool                            write;
} CPI_Pointer;

// ===============================================================================================================
// CPU operators
// ===============================================================================================================
typedef struct CPI_Process {
#ifdef DEBUG
    unsigned long long              id;
#endif
} CPI_Process;

typedef struct CPI_Thread {
#ifdef DEBUG
    unsigned long long              id;
#endif
    SDL_Thread*                     p_thread;
    SDL_ThreadID                    thread_id;
} CPI_Thread;

typedef struct CPI_Function{
#ifdef DEBUG
    unsigned long long              id;
#endif

} CPI_Function;

typedef struct CPI_ShadercCompiler{
#ifdef DEBUG
    unsigned long long              id;
#endif
    SDL_ThreadID                    thread_id;
    shaderc_compiler_t              shaderc_compiler;
    shaderc_compile_options_t       shaderc_options;
} CPI_ShadercCompiler;


// ===============================================================================================================
// GPU states
// ===============================================================================================================
typedef struct CPI_GPUDevice {
#if defined(DEBUG)
    long long                       id;
#endif
    SDL_GPUDevice*                  p_gpu_device;
} CPI_GPUDevice;

typedef struct CPI_Window {
#ifdef DEBUG
    unsigned long long              id;
#endif
    SDL_Window*                     p_sdl_window;
    int                             gpu_device_index;
    SDL_ThreadID                    thread_id;
} CPI_Window;

typedef struct CPI_Fence {
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_Fence;

typedef struct CPI_Sampler {
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_Sampler;

typedef struct CPI_Image {
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_Image;

typedef struct CPI_Transfer {
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_Transfer;

typedef struct CPI_Buffer{
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_Buffer;


// ===============================================================================================================
// GPU operators
// ===============================================================================================================
typedef struct CPI_Shader {
#if defined(DEBUG)
    long long                       id;
#endif
    char*                           p_glsl_code;
    void*                           p_spv_code;
    unsigned int                    spv_code_size;
    unsigned int                    glsl_code_size;
    const char*                     entrypoint;

    int                             shaderc_compiler_index;
    shaderc_shader_kind             shader_kind;

    SpvReflectShaderModule          reflect_shader_module;

    int                             gpu_device_index;
    SDL_GPUShader*                  p_sdl_shader;
} CPI_Shader;

typedef struct CPI_GraphicsPipeline {
#if defined(DEBUG)
    long long                       id;
#endif
    int                             vertex_shader_index;
    int                             fragment_shader_index;
    SDL_GPUGraphicsPipeline*        p_graphics_pipeline;
} CPI_GraphicsPipeline;

typedef struct CPI_ComputePipeline {
#if defined(DEBUG)
    long long                       id;
#endif
    CPI_Shader                      vertex_shader;
    CPI_Shader                      fragment_shader;
} CPI_ComputePipeline;

typedef struct CPI_RenderPass {
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_RenderPass;

typedef struct CPI_ComputePass {
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_ComputePass;

typedef struct CPI_CopyPass {
#if defined(DEBUG)
    long long                       id;
#endif
} CPI_CopyPass;

typedef struct CPI_Command {
#if defined(DEBUG)
    long long                       id;
#endif
    SDL_ThreadID                    thread_id;
    int*                            p_target_path; // either window or texture
    size_t                          target_path_length;
    int**                           pp_path_passes;
    size_t*                         p_path_passes;
} CPI_Command;

// CPU States
static Type cpi_box_type;
// CPU Operators
static Type cpi_process_type;
static Type cpi_thread_type;
static Type cpi_function_type;
static Type cpi_shaderc_compiler_type;
// GPU States
static Type cpi_window_type;
static Type cpi_fence_type;
static Type cpi_sampler_type;
static Type cpi_image_type;
static Type cpi_transfer_type;
static Type cpi_buffer_type;
// GPU Operators
static Type cpi_gpu_device_type;
static Type cpi_command_type;
static Type cpi_render_pass_type;
static Type cpi_compute_pass_type;
static Type cpi_copy_pass_type;
static Type cpi_graphics_pipeline_type;
static Type cpi_computer_pipeline_type;
static Type cpi_shader_type;

static Vec* g_vec = NULL;
#ifdef DEBUG
    SDL_Mutex*              g_unique_id_mutex = NULL;
    unsigned long long      g_unique_id = 0;
#endif

// ===============================================================================================================
// Internal Functions
// ===============================================================================================================

// ===============================================================================================================
// main
// ===============================================================================================================
void cpi_Initialize() {
    #ifdef DEBUG
        if (g_unique_id_mutex) {
            tklog_critical("not NULL pointer");
            exit(-1);
        }
        tklog_scope(g_unique_id_mutex = SDL_CreateMutex());
    #endif

    g_vec = malloc(sizeof(Vec));
    if (!g_vec) {
        tklog_critical("in cpi_Initialize(): malloc failed");
        return;
    }

    tklog_scope(*g_vec = vec_Create(NULL, vec_type));

    // You have to set the types that needs to be destroyed first, first
    tklog_scope(cpi_window_type = type_Create_Safe("CPI_Window", sizeof(CPI_Window), cpi_Window_Destructor));
    tklog_scope(cpi_shaderc_compiler_type = type_Create_Safe("CPI_ShadercCompiler", sizeof(CPI_ShadercCompiler), cpi_ShadercCompiler_Destructor));
    tklog_scope(cpi_shader_type = type_Create_Safe("CPI_Shader", sizeof(CPI_Shader), cpi_Shader_Destructor));
    tklog_scope(cpi_graphics_pipeline_type = type_Create_Safe("CPI_GraphicsPipeline", sizeof(CPI_GraphicsPipeline), cpi_GraphicsPipeline_Destructor));
    tklog_scope(cpi_gpu_device_type = type_Create_Safe("CPI_GPUDevice", sizeof(CPI_GPUDevice), cpi_GPUDevice_Destructor));

    if (!SDL_Init(SDL_INIT_VIDEO)) {
        tklog_critical("ERROR: failed to initialize SDL3: %s", SDL_GetError());
        return;
    }

    if (!SDL_ShaderCross_Init()) {
        tklog_critical("Failed to initialize SDL_ShaderCross. %s", SDL_GetError());
        return;
    }
}
// ===============================================================================================================
// Window
// ===============================================================================================================
int cpi_Window_Create(
    int gpu_device_index,
    unsigned int width,
    unsigned int height,
    const char* title)
{
    if (!title) {
        tklog_warning("title is NULL");
        return -1;
    }

    tklog_scope(Vec** pp_window_vec = vec_MoveStart(g_vec));
    tklog_scope(vec_SwitchReadToWrite(*pp_window_vec));
    tklog_scope(int window_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_window_vec, cpi_window_type));
    tklog_scope(vec_SwitchWriteToRead(*pp_window_vec));
    tklog_scope(vec_MoveToIndex(pp_window_vec, window_vec_index, cpi_window_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_window_vec));
    tklog_scope(int window_index = vec_UpsertNullElement_UnsafeWrite(*pp_window_vec, cpi_window_type));
    tklog_scope(CPI_Window* p_window = (CPI_Window*)vec_GetElement_UnsafeRead(*pp_window_vec, window_index, cpi_window_type));
    if (!p_window) {
        tklog_critical("NULL pointer");
        return -1;
    }
    if (p_window->p_sdl_window) {
        tklog_critical("INTERNAL ERROR: sdl window should be NULL");
        return -1;
    }
    p_window->p_sdl_window = SDL_CreateWindow(title, width, height, SDL_WINDOW_RESIZABLE);
    if (!p_window->p_sdl_window) {
        tklog_critical("ERROR: failed to create window: %s", SDL_GetError());
        return -1;
    }

    // getting gpu device
    p_window->gpu_device_index = gpu_device_index;
    tklog_scope(Vec** pp_gpu_device_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_gpu_device_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_gpu_device_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_gpu_device_vec, gpu_device_index, cpi_gpu_device_type));
    
    if (!p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        return -1;
    };

    if (!SDL_ClaimWindowForGPUDevice(p_gpu_device->p_gpu_device, p_window->p_sdl_window)) {
        tklog_error("Failed to claim window");
        return -1;
    }
    tklog_scope(vec_MoveEnd(pp_gpu_device_vec));

    #ifdef DEBUG
        SDL_LockMutex(g_unique_id_mutex);
        p_window->id = g_unique_id++;
        SDL_UnlockMutex(g_unique_id_mutex);
    #endif

    tklog_scope(vec_SwitchWriteToRead(*pp_window_vec));
    tklog_scope(vec_MoveEnd(pp_window_vec));

    printf("SUCCESSFULLY created window\n");
    return window_index;
}

int cpi_Window_Create(
    int gpu_device_index,
    unsigned int width,
    unsigned int height,
    const char* title)
{
    if (!title) {
        tklog_warning("title is NULL");
        return -1;
    }


    tklog_scope(Vec** pp_window_vec = vec_MoveStart(g_vec));
    tklog_scope(vec_SwitchReadToWrite(*pp_window_vec));
    tklog_scope(int window_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_window_vec, cpi_window_type));
    tklog_scope(vec_SwitchWriteToRead(*pp_window_vec));
    tklog_scope(vec_MoveToIndex(pp_window_vec, window_vec_index, cpi_window_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_window_vec));
    tklog_scope(int window_index = vec_UpsertNullElement_UnsafeWrite(*pp_window_vec, cpi_window_type));
    tklog_scope(CPI_Window* p_window = (CPI_Window*)vec_GetElement_UnsafeRead(*pp_window_vec, window_index, cpi_window_type));
    if (!p_window) {
        tklog_critical("NULL pointer");
        return -1;
    }
    if (p_window->p_sdl_window) {
        tklog_critical("INTERNAL ERROR: sdl window should be NULL");
        return -1;
    }
    p_window->p_sdl_window = SDL_CreateWindow(title, width, height, SDL_WINDOW_RESIZABLE);
    if (!p_window->p_sdl_window) {
        tklog_critical("ERROR: failed to create window: %s", SDL_GetError());
        return -1;
    }

    // getting gpu device
    p_window->gpu_device_index = gpu_device_index;
    tklog_scope(Vec** pp_gpu_device_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_gpu_device_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_gpu_device_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_gpu_device_vec, gpu_device_index, cpi_gpu_device_type));
    
    if (!p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        return -1;
    };

    if (!SDL_ClaimWindowForGPUDevice(p_gpu_device->p_gpu_device, p_window->p_sdl_window)) {
        tklog_error("Failed to claim window");
        return -1;
    }
    tklog_scope(vec_MoveEnd(pp_gpu_device_vec));

    #ifdef DEBUG
        SDL_LockMutex(g_unique_id_mutex);
        p_window->id = g_unique_id++;
        SDL_UnlockMutex(g_unique_id_mutex);
    #endif

    tklog_scope(vec_SwitchWriteToRead(*pp_window_vec));
    tklog_scope(vec_MoveEnd(pp_window_vec));

    printf("SUCCESSFULLY created window\n");
    return window_index;
}
void cpi_Window_Show(
    int window_index,
    int graphics_pipeline_index) 
{
    // get the window
    tklog_scope(Vec** pp_window_vec = vec_MoveStart(g_vec));
    tklog_scope(int window_vec_index = vec_GetVecWithType_UnsafeRead(*pp_window_vec, cpi_window_type));
    tklog_scope(vec_MoveToIndex(pp_window_vec, window_vec_index, cpi_window_type));
    tklog_scope(CPI_Window* p_window = (CPI_Window*)vec_GetElement_UnsafeRead(*pp_window_vec, window_index, cpi_window_type));
    tklog_scope(if (!p_window->p_sdl_window) {
        tklog_critical("NULL pointer");
        return;
    });

    // get the gpu device
    tklog_scope(Vec** pp_gpu_device_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_GetVecWithType_UnsafeRead(*pp_gpu_device_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_gpu_device_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_gpu_device_vec, p_window->gpu_device_index, cpi_gpu_device_type));
    tklog_scope(if (!p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        return;
    });

    SDL_GPUTextureFormat color_format = SDL_GetGPUSwapchainTextureFormat(p_gpu_device->p_gpu_device, p_window->p_sdl_window);
    if (SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM != color_format) {
        tklog_error("not correct color format");
        return;
    }

    // get the graphics pipeline
    tklog_scope(Vec** pp_graphics_pipeline_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_graphics_pipeline_index = vec_GetVecWithType_UnsafeRead(*pp_graphics_pipeline_vec, cpi_graphics_pipeline_type));
    tklog_scope(vec_MoveToIndex(pp_graphics_pipeline_vec, gpu_graphics_pipeline_index, cpi_graphics_pipeline_type));
    tklog_scope(CPI_GraphicsPipeline* p_graphics_pipeline = (CPI_GraphicsPipeline*)vec_GetElement_UnsafeRead(*pp_graphics_pipeline_vec, graphics_pipeline_index, cpi_graphics_pipeline_type));
    tklog_scope(if (!p_graphics_pipeline->p_graphics_pipeline) {
        tklog_critical("NULL pointer");
        return;
    });

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
    tklog_scope(SDL_GPUTransferBuffer* transfer_buffer = SDL_CreateGPUTransferBuffer(p_gpu_device->p_gpu_device, &transfer_info));
    if (!transfer_buffer) {
        tklog_critical("Failed to create transfer buffer: %s", SDL_GetError());
        return;
    }

    tklog_scope(void* mapped_data = SDL_MapGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer, false));
    if (!mapped_data) {
        tklog_critical("Failed to map transfer buffer: %s", SDL_GetError());
        return;
    }
    memcpy(mapped_data, rects, size);
    tklog_scope(SDL_UnmapGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer));
    SDL_GPUBufferCreateInfo buffer_create_info = {
        .usage = SDL_GPU_BUFFERUSAGE_VERTEX,
        .size = size
    };

    tklog_scope(SDL_GPUBuffer* gpu_buffer = SDL_CreateGPUBuffer(p_gpu_device->p_gpu_device, &buffer_create_info));
    if (!gpu_buffer) {
        tklog_critical("Failed to create GPU buffer: %s", SDL_GetError());
        return;
    }
    tklog_scope(SDL_GPUCommandBuffer* command_buffer = SDL_AcquireGPUCommandBuffer(p_gpu_device->p_gpu_device));
    if (!command_buffer) {
        tklog_critical("Failed to acquire command buffer: %s", SDL_GetError());
        return;
    }
    tklog_scope(SDL_GPUCopyPass* copy_pass = SDL_BeginGPUCopyPass(command_buffer));
    SDL_GPUTransferBufferLocation source = { transfer_buffer, 0 };
    SDL_GPUBufferRegion destination = { gpu_buffer, 0, size };
    tklog_scope(SDL_UploadToGPUBuffer(copy_pass, &source, &destination, false));
    tklog_scope(SDL_EndGPUCopyPass(copy_pass));
    if (!SDL_SubmitGPUCommandBuffer(command_buffer)) {
        tklog_critical("Failed to submit command buffer: %s", SDL_GetError());
        return;
    }
    SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, transfer_buffer);

    // Load image using stb_image
    char absolute_path[PATH_MAX];
    if (!realpath("../resources/Bitcoin.png", absolute_path)) {
        tklog_error("realpath failed");
        return;
    }
    printf("Loading image from: %s\n", absolute_path);
    

    int width, height, channels;
    unsigned char* p_data = stbi_load("../resources/Bitcoin.png", &width, &height, &channels, STBI_rgb_alpha);
    if (!p_data) {
        tklog_error("Failed to load image file: %s", "../resources/Bitcoin.png");
        return;
    }
    
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
    SDL_GPUTexture* bitcoin_texture = SDL_CreateGPUTexture(p_gpu_device->p_gpu_device, &tex_info);
    if (!bitcoin_texture) {
        tklog_critical("Failed to create bitcoin_texture: %s", SDL_GetError());
        return;
    }
    
    // Create a transfer buffer sized to hold the entire image.
    size_t image_size = width * height * 4; // 4 bytes per pixel (RGBA)
    SDL_GPUTransferBufferCreateInfo tex_transfer_info = {
        .usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD,
        .size = image_size
    };
    SDL_GPUTransferBuffer* tex_transfer_buffer = SDL_CreateGPUTransferBuffer(p_gpu_device->p_gpu_device, &tex_transfer_info);
    if (!tex_transfer_buffer) {
        tklog_critical("Failed to create transfer buffer: %s", SDL_GetError());
        return;
    }
    
    // Map the transfer buffer and copy the image data into it.
    void* tex_map = SDL_MapGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer, false);
    if (!tex_map) {
        tklog_critical("Failed to map transfer buffer: %s", SDL_GetError());
        return;
    }
    memcpy(tex_map, p_data, image_size);
    SDL_UnmapGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer);
    
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
    SDL_GPUCommandBuffer* tex_cmd = SDL_AcquireGPUCommandBuffer(p_gpu_device->p_gpu_device);
    SDL_GPUCopyPass* tex_copy = SDL_BeginGPUCopyPass(tex_cmd);
    SDL_UploadToGPUTexture(tex_copy, &tex_transfer, &tex_region, false);
    SDL_EndGPUCopyPass(tex_copy);
    SDL_SubmitGPUCommandBuffer(tex_cmd);
    SDL_ReleaseGPUTransferBuffer(p_gpu_device->p_gpu_device, tex_transfer_buffer);
    
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
    SDL_GPUSampler* bitcoin_sampler = SDL_CreateGPUSampler(p_gpu_device->p_gpu_device, &sampler_info);
    if (!bitcoin_sampler) {
        tklog_critical("Failed to create bitcoin_sampler: %s", SDL_GetError());
        return;
    }
    
    // Free the loaded image data as it is now uploaded to the GPU.
    stbi_image_free(p_data);
    // --- Main rendering loop ---
    bool running = true;
    while (running) {
        // Process events (quit if window is closed)
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_EVENT_QUIT) {
                running = false;
            }
        }

        // Acquire a command buffer for the current frame.
        SDL_GPUCommandBuffer* cmd_buffer = SDL_AcquireGPUCommandBuffer(p_gpu_device->p_gpu_device);
        if (!cmd_buffer) {
            tklog_error("Failed to acquire command buffer: %s", SDL_GetError());
            return;
        }

        SDL_GPUTexture *swapchain_tex = NULL;
        unsigned int tex_width = 0, tex_height = 0;
        if (!SDL_WaitAndAcquireGPUSwapchainTexture(cmd_buffer, p_window->p_sdl_window, &swapchain_tex, &tex_width, &tex_height)) {
            tklog_error("Failed to acquire swapchain texture: %s", SDL_GetError());
            return;
        }

        // Create dummy UBO data (for the vertex shader’s UBO in set 1).
        typedef struct {
            float targetWidth;
            float targetHeight;
            float padding[2];
        } UniformBufferObject;
        UniformBufferObject dummyUBO = { (float)tex_width, (float)tex_height, {0.0f, 0.0f} };

        // Push dummy uniform data onto the command buffer (for vertex shader UBO)
        // This pushes data into uniform slot 0 of the vertex stage.
        SDL_PushGPUVertexUniformData(cmd_buffer, 0, &dummyUBO, sizeof(dummyUBO));

        

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
        if (!render_pass) {
            tklog_error("Failed to begin render pass: %s", SDL_GetError());
            return;
        }

        // Set the viewport to cover the entire swapchain texture.
        SDL_GPUViewport viewport = {
            .x = 0,
            .y = 0,
            .w = (float)tex_width,
            .h = (float)tex_height,
            .min_depth = 0.0f,
            .max_depth = 1.0f
        };
        tklog_scope(SDL_SetGPUViewport(render_pass, &viewport));

        // Bind the graphics pipeline.
        tklog_scope(SDL_BindGPUGraphicsPipeline(render_pass, p_graphics_pipeline->p_graphics_pipeline));

        // Bind the vertex buffer.
        SDL_GPUBufferBinding buffer_binding = { .buffer = gpu_buffer, .offset = 0 };
        tklog_scope(SDL_BindGPUVertexBuffers(render_pass, 0, &buffer_binding, 1));

        // Bind dummy texture–sampler pairs for the fragment shader.
        SDL_GPUTextureSamplerBinding dummyTexBindings[8];
        for (int i = 0; i < 8; i++) {
            dummyTexBindings[i].texture = bitcoin_texture;
            dummyTexBindings[i].sampler = bitcoin_sampler;
        }
        SDL_BindGPUFragmentSamplers(render_pass, 0, dummyTexBindings, 8);

        tklog_scope(SDL_DrawGPUPrimitives(render_pass, 4, 2, 0, 0));

        // End the render pass.
        SDL_EndGPURenderPass(render_pass);

        // Submit the command buffer so that the commands are executed on the GPU.
        if (!SDL_SubmitGPUCommandBuffer(cmd_buffer)) {
            tklog_error("Failed to submit command buffer: %s", SDL_GetError());
            return;
        }

        // Optionally, delay to cap the frame rate (here ~60 FPS).
        // SDL_Delay(16);
    }

    tklog_scope(SDL_ReleaseGPUBuffer(p_gpu_device->p_gpu_device, gpu_buffer));
    // (Be sure to release/destroy your dummy resources when cleaning up.)
}
void cpi_Window_Destructor(
    void* p_void) 
{
    CPI_Window* p_window = (CPI_Window*)p_void;
    if (!p_window) {
        tklog_critical("NULL pointer");
        return;
    }
    if (!p_window->p_sdl_window) {
        tklog_critical("NULL pointer");
        return;
    }
    bool is_null = true;
    for (int i = 0; i < sizeof(CPI_ShadercCompiler); ++i) {
        if (((unsigned char*)p_window)[i] != 0) {
            is_null = false;
        }
    }
    if (is_null) {
        tklog_critical("shaderc compiler is null");
        return;
    }

    // getting gpu device
    /*
    tklog_scope(Vec** pp_gpu_device_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_gpu_device_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_gpu_device_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = vec_GetElement_UnsafeRead(*pp_gpu_device_vec, gpu_device_index, cpi_gpu_device_type));
    tklog_scope(if (!p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        exit(-1);
    });
    tklog_scope(SDL_ReleaseWindowFromGPUDevice(p_gpu_device->p_gpu_device, p_window->p_sdl_window));
    tklog_scope(vec_MoveEnd(pp_gpu_device_vec));
    */

    tklog_scope(SDL_DestroyWindow(p_window->p_sdl_window));
    memset(p_window, 0, sizeof(CPI_Window));
}
void cpi_Window_Destroy(
    int* p_window_index)
{
    if (!p_window_index) {
        tklog_critical("NULL pointer");
        return;
    }
    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(int window_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_window_type));
    tklog_scope(vec_MoveToIndex(pp_vec, window_vec_index, cpi_window_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(CPI_Window* p_window = (CPI_Window*)vec_GetElement_UnsafeRead(*pp_vec, *p_window_index, cpi_window_type));
    tklog_scope(cpi_Window_Destructor(p_window));
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveEnd(pp_vec));
    *p_window_index = 0;
    printf("SUCCESSFULLY destroyed window\n");
}

// ===============================================================================================================
// Shaderc Compiler
// ===============================================================================================================
int cpi_ShadercCompiler_GetIndex() 
{
    tklog_scope(SDL_ThreadID this_thread_id = SDL_GetCurrentThreadID());

    // Check if a shaderc compiler already exists for this thread
    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(int shaderc_compiler_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_vec, cpi_shaderc_compiler_type));
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveToIndex(pp_vec, shaderc_compiler_vec_index, cpi_shaderc_compiler_type));
    tklog_scope(unsigned int count = vec_GetCount_UnsafeRead(*pp_vec));
    if (count >= 1) {
        for (unsigned short i = 0; i < count; ++i) {
            tklog_scope(CPI_ShadercCompiler* p_compiler = (CPI_ShadercCompiler*)vec_GetElement_UnsafeRead(*pp_vec, i, cpi_shaderc_compiler_type));
            if (p_compiler[i].thread_id == this_thread_id) {
                tklog_scope(vec_MoveEnd(pp_vec));
                return i;
            }
        }
    }
        
    // at this point a shaderc compiler doesn't exist for this thread so the following will create it
    tklog_scope(vec_MoveToIndex(pp_vec, -1, vec_type));
    tklog_scope(vec_MoveToIndex(pp_vec, shaderc_compiler_vec_index, cpi_shaderc_compiler_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(int shaderc_compiler_index = vec_UpsertNullElement_UnsafeWrite(*pp_vec, cpi_shaderc_compiler_type));
    tklog_scope(CPI_ShadercCompiler* p_compiler = (CPI_ShadercCompiler*)vec_GetElement_UnsafeRead(*pp_vec, shaderc_compiler_index, cpi_shaderc_compiler_type));
    p_compiler->thread_id = this_thread_id;
    tklog_scope(p_compiler->shaderc_compiler = shaderc_compiler_initialize());
    if (!p_compiler->shaderc_compiler) {
        tklog_critical("failed to initialize");
        return -1;
    }
    tklog_scope(p_compiler->shaderc_options = shaderc_compile_options_initialize());
    if (!p_compiler->shaderc_options) {
        tklog_critical("failed to initialize");
        return -1;
    }
    tklog_scope(shaderc_compile_options_set_optimization_level(p_compiler->shaderc_options, shaderc_optimization_level_zero));
    #ifdef __linux__
        tklog_scope(shaderc_compile_options_set_target_env(p_compiler->shaderc_options, shaderc_target_env_vulkan, shaderc_env_version_vulkan_1_0));
    #else 
        tklog_error("OS not supported yet");
        return -1;
    #endif
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));

    tklog_scope(vec_MoveEnd(pp_vec));
    printf("SUCCESSFULLY created shaderc compiler\n");
    return shaderc_compiler_index;
}
void cpi_ShadercCompiler_Destructor(
    void* p_void) 
{
    CPI_ShadercCompiler* p_shaderc_compiler = (CPI_ShadercCompiler*)p_void;
    if (!p_shaderc_compiler) {
        tklog_critical("NULL pointer");
        return;
    }
    bool is_null = true;
    for (int i = 0; i < sizeof(CPI_ShadercCompiler); ++i) {
        if (((unsigned char*)p_shaderc_compiler)[i] != 0) {
            is_null = false;
        }
    }
    if (is_null) {
        tklog_error("shaderc compiler is null");
        return;
    }
    shaderc_compiler_release(p_shaderc_compiler->shaderc_compiler);
    shaderc_compile_options_release(p_shaderc_compiler->shaderc_options);
    memset(p_shaderc_compiler, 0, sizeof(CPI_ShadercCompiler));
}
void cpi_ShadercCompiler_Destroy(
    int* p_shaderc_compiler_index) 
{
    if (!p_shaderc_compiler_index) {
        tklog_critical("NULL pointer");
        return;
    }
    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(int shaderc_compiler_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_shaderc_compiler_type));
    tklog_scope(CPI_ShadercCompiler* p_compiler = (CPI_ShadercCompiler*)vec_GetElement_UnsafeRead(*pp_vec, *p_shaderc_compiler_index, cpi_shaderc_compiler_type));
    tklog_scope(cpi_ShadercCompiler_Destructor(p_compiler));
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveEnd(pp_vec));
    *p_shaderc_compiler_index = 0;
}

// ===============================================================================================================
// GPUDevice
// ===============================================================================================================
int cpi_GPUDevice_Create() 
{
    if (!vec_IsValid_UnsafeRead(g_vec)) {
        tklog_error("invlaid vec");
        return -1;
    }
    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(int gpu_device_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_vec, cpi_gpu_device_type));
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveToIndex(pp_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(int gpu_device_index = vec_UpsertNullElement_UnsafeWrite(*pp_vec, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_vec, gpu_device_index, cpi_gpu_device_type));

    if (p_gpu_device->p_gpu_device) {
        tklog_critical("pointer should be NULL");
        exit(-1);
    }

    #ifdef __linux__
        #ifdef DEBUG
            tklog_scope(p_gpu_device->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, true, NULL));
            printf("GPU Backend: %s\n", SDL_GetGPUDeviceDriver(p_gpu_device->p_gpu_device)); 
        #else 
            tklog_scope(p_gpu_device->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, false, NULL));
            printf("GPU Backend: %s\n", SDL_GetGPUDeviceDriver(p_gpu_device->p_gpu_device)); 
        #endif // LCPI_DEBUG
    #elif defined(_WIN64)
        tklog_error("windows 64-bit is not supported yet");
        return -1;
    #elif defined(_WIN32)
        tklog_error("windows 32-bit is not supported yet");
        return -1;
    #elif defined(__CYGWIN__)
        tklog_error("cygwin is not supported yet");
        return -1;
    #elif defined(__APPLE__)
        tklog_error("macos is not supported yet");
        return -1;
    #elif defined(__FreeBSD__)
        tklog_error("free bsd is not supported yet");
        return -1;
    #elif defined(__ANDROID__)
        tklog_error("android is not supported yet");
        return -1;
    #else 
        tklog_error("unrecignized os is not supported");
        return -1;
    #endif

    if (!p_gpu_device->p_gpu_device) {
        tklog_critical("ERROR: failed to create SDL3 device: %s", SDL_GetError());
        return -1;
    }
    #ifdef DEBUG 
        SDL_LockMutex(g_unique_id_mutex);
        p_gpu_device->id = g_unique_id++;
        SDL_UnlockMutex(g_unique_id_mutex);
    #endif 
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveEnd(pp_vec));

    printf("SUCCESSFULLY created gpu device\n");
    return gpu_device_index;
}
void cpi_GPUDevice_Destructor(
    void* p_void) 
{
    CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)p_void;
    if (!p_gpu_device) {
        tklog_critical("NULL pointer");
        return;
    }
    if (!(CPI_GPUDevice*)p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        return;
    }

    bool is_null = true;
    for (int i = 0; i < sizeof(CPI_ShadercCompiler); ++i) {
        if (((unsigned char*)p_gpu_device)[i] != 0) {
            is_null = false;
        }
    }
    if (is_null) {
        tklog_error("shaderc compiler is null");
        return;
    }
    tklog_scope(SDL_DestroyGPUDevice(p_gpu_device->p_gpu_device));
    memset(p_gpu_device, 0, sizeof(CPI_GPUDevice));
}
void cpi_GPUDevice_Destroy(
    int* p_gpu_device_index) 
{
    if (!p_gpu_device_index) {
        tklog_critical("NULL pointer");
        return;
    }
    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_vec, *p_gpu_device_index, cpi_gpu_device_type));
    tklog_scope(cpi_GPUDevice_Destructor(p_gpu_device->p_gpu_device));
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveEnd(pp_vec));
    *p_gpu_device_index = 0;
}

// ===============================================================================================================
// Shader
// ===============================================================================================================
unsigned long long _cpi_Shader_ReadFile(
    const char* filename,
    char** const dst_buffer)
{
    if (!filename) {
        tklog_critical("NULL pointer");
        return 0;
    }
    if (!dst_buffer) {
        tklog_critical("NULL pointer");
        return 0;
    }
    if (*dst_buffer) {
        tklog_critical("not NULL pointer");
        return 0;
    }

    FILE* file = fopen(filename, "rb");
    if (!file) {
        tklog_error("Failed to open shader source file '%s'", filename);
        return 0;
    }

    fseek(file, 0, SEEK_END);
    unsigned long long file_size = ftell(file);
    rewind(file);

    tklog_scope(*dst_buffer = (char*)malloc(file_size + 1));
    if (!*dst_buffer) {
        fclose(file);
        tklog_critical("Failed to allocate memory for shader source '%s'", filename);
        return 0;
    }

    unsigned long long readSize = fread(*dst_buffer, 1, file_size, file);
    (*dst_buffer)[file_size] = '\0'; 

    if (readSize != file_size) {
        free(*dst_buffer);
        fclose(file);
        tklog_error("Failed to read shader source '%s'", filename);
        return 0;
    }

    fclose(file);
    return (unsigned int)file_size;
}
unsigned int _cpi_Shader_FormatSize(
    SDL_GPUVertexElementFormat format) 
{
    if (format == SDL_GPU_VERTEXELEMENTFORMAT_INVALID) {
        tklog_critical("INTERNAL ERROR: format is invlaid");
        return 0;
    }
    unsigned int result = 0;
    switch (format) {
        case SDL_GPU_VERTEXELEMENTFORMAT_INVALID: result = 0; break;

        /* 32-bit Signed Integers */
        case SDL_GPU_VERTEXELEMENTFORMAT_INT: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_INT2: result = 8; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_INT3: result = 12; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_INT4: result = 16; break;

        /* 32-bit Unsigned Integers */
        case SDL_GPU_VERTEXELEMENTFORMAT_UINT: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_UINT2: result = 8; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_UINT3: result = 12; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_UINT4: result = 16; break;

        /* 32-bit Floats */
        case SDL_GPU_VERTEXELEMENTFORMAT_FLOAT: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_FLOAT2: result = 8; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3: result = 12; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_FLOAT4: result = 16; break;

        /* 8-bit Signed Integers */
        case SDL_GPU_VERTEXELEMENTFORMAT_BYTE2: result = 2; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_BYTE4: result = 4; break;

        /* 8-bit Unsigned Integers */
        case SDL_GPU_VERTEXELEMENTFORMAT_UBYTE2: result = 2; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_UBYTE4: result = 4; break;

        /* 8-bit Signed Normalized */
        case SDL_GPU_VERTEXELEMENTFORMAT_BYTE2_NORM: result = 2; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_BYTE4_NORM: result = 4; break;

        /* 8-bit Unsigned Normalized */
        case SDL_GPU_VERTEXELEMENTFORMAT_UBYTE2_NORM: result = 2; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_UBYTE4_NORM: result = 4; break;

        /* 16-bit Signed Integers */
        case SDL_GPU_VERTEXELEMENTFORMAT_SHORT2: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_SHORT4: result = 8; break;

        /* 16-bit Unsigned Integers */
        case SDL_GPU_VERTEXELEMENTFORMAT_USHORT2: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_USHORT4: result = 8; break;

        /* 16-bit Signed Normalized */
        case SDL_GPU_VERTEXELEMENTFORMAT_SHORT2_NORM: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_SHORT4_NORM: result = 8; break;

        /* 16-bit Unsigned Normalized */
        case SDL_GPU_VERTEXELEMENTFORMAT_USHORT2_NORM: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_USHORT4_NORM: result = 8; break;

        /* 16-bit Floats */
        case SDL_GPU_VERTEXELEMENTFORMAT_HALF2: result = 4; break;
        case SDL_GPU_VERTEXELEMENTFORMAT_HALF4: result = 8; break;
    }
    if (result == 0) {
        tklog_critical("INTERNAL ERROR: could not find any format");
        return 0;
    }
    return result;
}
SDL_GPUVertexElementFormat _cpi_spvReflectFormatToSDLGPUformat(
    SpvReflectFormat format) 
{
    switch(format) {
        case SPV_REFLECT_FORMAT_R16G16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_USHORT2;
        case SPV_REFLECT_FORMAT_R16G16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_SHORT2;
        case SPV_REFLECT_FORMAT_R16G16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_HALF2;
        case SPV_REFLECT_FORMAT_R16G16B16A16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_USHORT4;
        case SPV_REFLECT_FORMAT_R16G16B16A16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_SHORT4;
        case SPV_REFLECT_FORMAT_R16G16B16A16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_HALF4;
        case SPV_REFLECT_FORMAT_R32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT;
        case SPV_REFLECT_FORMAT_R32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT;
        case SPV_REFLECT_FORMAT_R32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT;
        case SPV_REFLECT_FORMAT_R32G32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT2;
        case SPV_REFLECT_FORMAT_R32G32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT2;
        case SPV_REFLECT_FORMAT_R32G32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT2;
        case SPV_REFLECT_FORMAT_R32G32B32A32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT4; 
        case SPV_REFLECT_FORMAT_R32G32B32A32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT4;
        case SPV_REFLECT_FORMAT_R32G32B32A32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT4;

        // 64-bit Formats (Unsupported)
        default: {
            tklog_error("spv reflect format is not supported");
            return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        }
    }
}
void _cpi_Shader_PrintAttributeDescriptions(
    SDL_GPUVertexAttribute* p_attributes, 
    unsigned long long attribs_count) 
{
    if (!p_attributes) {
        tklog_critical("NULL pointer");
        return;
    }
    for (unsigned int i = 0; i < attribs_count; ++i) {
        printf("attrib %d\n", i);
        printf("\t%d\n", p_attributes[i].location);
        printf("\t%d\n", p_attributes[i].buffer_slot);
        printf("\t%d\n", p_attributes[i].format);
        printf("\t%d\n", p_attributes[i].offset);
    }
}
SDL_GPUVertexAttribute* _cpi_Shader_Create_VertexInputAttribDesc(
    int vertex_shader_index,
    unsigned int* p_attribute_count, 
    unsigned int* p_binding_stride) 
{
    if (!p_attribute_count) {
        tklog_critical("NULL pointer");
        return NULL;
    }
    if (!p_binding_stride) {
        tklog_critical("NULL pointer");
        return NULL;
    }

    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(int vertex_shader_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_shader_type));
    tklog_scope(vec_MoveToIndex(pp_vec, vertex_shader_vec_index, cpi_shader_type));
    tklog_scope(CPI_Shader* p_shader = (CPI_Shader*)vec_GetElement_UnsafeRead(*pp_vec, vertex_shader_index, cpi_shader_type));
    if (p_shader->reflect_shader_module.shader_stage != SPV_REFLECT_SHADER_STAGE_VERTEX_BIT) {
        tklog_error("Provided shader is not a vertex shader");
        return NULL;
    }

    // Enumerate input variables
    unsigned int input_var_count = 0;
    tklog_scope(SpvReflectResult result = spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, NULL));
    if (result != SPV_REFLECT_RESULT_SUCCESS) {
        tklog_error("Failed to enumerate input variables");
        return NULL;
    }

    tklog_scope(SpvReflectInterfaceVariable** input_vars = malloc(input_var_count * sizeof(SpvReflectInterfaceVariable*)));
    if (!input_vars) {
        tklog_critical("Failed to allocate memory for input variables");
        return NULL;
    }

    tklog_scope(result = spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, input_vars));
    if (result != SPV_REFLECT_RESULT_SUCCESS) {
        tklog_error("Failed to get input variables");
        return NULL;
    }
    tklog_scope(vec_MoveEnd(pp_vec));

    // Create an array to hold SDL_GPUVertexAttribute
    tklog_scope(SDL_GPUVertexAttribute* attribute_descriptions = malloc(input_var_count * sizeof(SDL_GPUVertexAttribute)));
    if (!attribute_descriptions) {
        tklog_critical("Failed to allocate memory for vertex input attribute descriptions");
        return NULL;
    }

    unsigned int attribute_index = 0;
    for (unsigned int i = 0; i < input_var_count; ++i) {
        SpvReflectInterfaceVariable* refl_var = input_vars[i];

        // Ignore built-in variables
        if (refl_var->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) {
            continue;
        }

        attribute_descriptions[attribute_index].location = refl_var->location;
        attribute_descriptions[attribute_index].buffer_slot = 0; // ASSUMES ONLY ONE SLOT
        tklog_scope(attribute_descriptions[attribute_index].format = _cpi_spvReflectFormatToSDLGPUformat(refl_var->format));
        attribute_descriptions[attribute_index].offset = 0; // WILL CALCULATE OFFSET LATER
        attribute_index++;
    }

    // Update the attribute count
    *p_attribute_count = attribute_index;

    // Sort attributes by location
    for (unsigned int i = 0; i < attribute_index - 1; ++i) {
        for (unsigned int j = 0; j < attribute_index - i - 1; ++j) {
            if (attribute_descriptions[j].location > attribute_descriptions[j + 1].location) {
                SDL_GPUVertexAttribute temp = attribute_descriptions[j];
                attribute_descriptions[j] = attribute_descriptions[j + 1];
                attribute_descriptions[j+1] = temp;
            }
        }
    }

    // Compute offsets and buffer_slot stride
    unsigned int offset = 0;
    for (unsigned int i = 0; i < attribute_index; ++i) {
        tklog_scope(unsigned int format_size = _cpi_Shader_FormatSize(attribute_descriptions[i].format));

        if (format_size == 0) {
            printf("Unsupported format for input variable at location %u\n", attribute_descriptions[i].location);
            continue;
        }

        // Align the offset if necessary (e.g., 4-byte alignment)
        unsigned int alignment = 4;
        offset = (offset + (alignment - 1)) & ~(alignment - 1);
        attribute_descriptions[i].offset = offset;
        offset += format_size;
    }

    *p_binding_stride = offset;
    free(input_vars);
    _cpi_Shader_PrintAttributeDescriptions(attribute_descriptions, *p_attribute_count);

    return attribute_descriptions;  
}
SDL_GPUShaderCreateInfo _cpi_Shader_CreateShaderInfo(const Uint8 *spv_code, size_t spv_code_size, const char *entrypoint, int is_vert)
{
    SDL_GPUShaderCreateInfo shader_info = {0};
    shader_info.code       = spv_code;
    shader_info.code_size  = spv_code_size;
    shader_info.entrypoint = entrypoint;
    shader_info.format     = SDL_GPU_SHADERFORMAT_SPIRV;
    shader_info.stage      = is_vert ? SDL_GPU_SHADERSTAGE_VERTEX : SDL_GPU_SHADERSTAGE_FRAGMENT;
    shader_info.props      = 0;  // No extension properties for now

    // Create a SPIR-V reflection module
    SpvReflectShaderModule module;
    SpvReflectResult result = spvReflectCreateShaderModule(spv_code_size, spv_code, &module);
    if (result != SPV_REFLECT_RESULT_SUCCESS) {
        tklog_error("Error: Failed to create SPIR-V reflection module (result: %d)", result);
        return shader_info;
    }

    // First, determine how many descriptor bindings exist
    uint32_t binding_count = 0;
    result = spvReflectEnumerateDescriptorBindings(&module, &binding_count, NULL);
    if (result != SPV_REFLECT_RESULT_SUCCESS) {
        tklog_error("Error: Failed to enumerate descriptor bindings (result: %d)", result);
        spvReflectDestroyShaderModule(&module);
        return shader_info;
    }

    // Allocate an array to hold pointers to descriptor binding info
    SpvReflectDescriptorBinding **bindings = malloc(binding_count * sizeof(SpvReflectDescriptorBinding*));
    if (!bindings) {
        tklog_critical("Error: Out of memory.");
        spvReflectDestroyShaderModule(&module);
        return shader_info;
    }

    // Retrieve the descriptor bindings
    result = spvReflectEnumerateDescriptorBindings(&module, &binding_count, bindings);
    if (result != SPV_REFLECT_RESULT_SUCCESS) {
        tklog_error("Error: Failed to get descriptor bindings (result: %d)", result);
        free(bindings);
        spvReflectDestroyShaderModule(&module);
        return shader_info;
    }

    // Counters for each descriptor type
    uint32_t num_samplers          = 0;
    uint32_t num_storage_textures  = 0;
    uint32_t num_storage_buffers   = 0;
    uint32_t num_uniform_buffers   = 0;

    // Loop over each binding and update counts based on its type.
    for (uint32_t i = 0; i < binding_count; ++i) {
        const SpvReflectDescriptorBinding *binding = bindings[i];
        switch (binding->descriptor_type) {
            case SPV_REFLECT_DESCRIPTOR_TYPE_SAMPLER:
            case SPV_REFLECT_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER:
                num_samplers++;
                break;
            case SPV_REFLECT_DESCRIPTOR_TYPE_STORAGE_IMAGE:
                num_storage_textures++;
                break;
            case SPV_REFLECT_DESCRIPTOR_TYPE_STORAGE_BUFFER:
                num_storage_buffers++;
                break;
            case SPV_REFLECT_DESCRIPTOR_TYPE_UNIFORM_BUFFER:
                num_uniform_buffers++;
                break;
            default:
                break;
        }
    }

    // Populate the SDL3 shader creation info struct
    shader_info.num_samplers         = num_samplers;
    shader_info.num_storage_textures = num_storage_textures;
    shader_info.num_storage_buffers  = num_storage_buffers;
    shader_info.num_uniform_buffers  = num_uniform_buffers;

    // Clean up
    free(bindings);
    spvReflectDestroyShaderModule(&module);

    return shader_info;
}
int cpi_Shader_CreateFromGlslFile(
    int gpu_device_index,
    const char* glsl_file_path, 
    const char* entrypoint,
    shaderc_shader_kind shader_kind, 
    bool enable_debug) 
{
    if (!glsl_file_path) {
        tklog_critical("NULL pointer");
        return -1;
    }
    if (!entrypoint) {
        tklog_critical("NULL pointer");
        return -1;
    }
    if (!(shader_kind == shaderc_vertex_shader ||
          shader_kind == shaderc_fragment_shader || 
          shader_kind == shaderc_compute_shader)) {
        tklog_error("shader kind is not supported");
        return -1;
    }

    CPI_Shader shader = {0};
    shader.entrypoint = entrypoint;
    shader.shader_kind = shader_kind;

    // get shaderc compiler path
    {
        tklog_scope(int shaderc_compiler_index = cpi_ShadercCompiler_GetIndex());
        shader.shaderc_compiler_index = shaderc_compiler_index;
    }

    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));

    // spv code compilation
    {
        if (shader.p_glsl_code) {
            tklog_critical("not NULL pointer");
            return -1;
        }
        tklog_scope(unsigned long long glsl_code_size = _cpi_Shader_ReadFile(glsl_file_path, &shader.p_glsl_code));
        if (!shader.p_glsl_code) {
            tklog_critical("NULL pointer");
            return -1;
        }
        tklog_scope(int shaderc_compiler_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_shaderc_compiler_type));
        tklog_scope(vec_MoveToIndex(pp_vec, shaderc_compiler_vec_index, cpi_shaderc_compiler_type));
        tklog_scope(CPI_ShadercCompiler* p_shaderc_compiler = (CPI_ShadercCompiler*)vec_GetElement_UnsafeRead(*pp_vec, shader.shaderc_compiler_index, cpi_shaderc_compiler_type));
        tklog_scope(shaderc_compilation_result_t result = shaderc_compile_into_spv(p_shaderc_compiler->shaderc_compiler, shader.p_glsl_code, glsl_code_size, shader_kind, glsl_file_path, "main", p_shaderc_compiler->shaderc_options));
        tklog_scope(vec_MoveToIndex(pp_vec, -1, vec_type));
        if (shaderc_result_get_compilation_status(result) != shaderc_compilation_status_success) {
            tklog_error("Shader compilation error in '%s':\n%s", glsl_file_path, shaderc_result_get_error_message(result));
            return -1;
        }
        tklog_scope(shader.spv_code_size = shaderc_result_get_length(result));
        tklog_scope(shader.p_spv_code = malloc(shader.spv_code_size));
        if (!shader.p_spv_code) {
            tklog_critical("failed to allocate memory\n");
            return -1;
        }
        tklog_scope(memcpy(shader.p_spv_code, shaderc_result_get_bytes(result), shader.spv_code_size));
        tklog_scope(shaderc_result_release(result));
    }

    // SpvReflectShaderModule
    {
        tklog_scope(SpvReflectResult reflectResult = spvReflectCreateShaderModule(shader.spv_code_size, shader.p_spv_code, &shader.reflect_shader_module));
        if (reflectResult != SPV_REFLECT_RESULT_SUCCESS) {
            tklog_error("Failed to create SPIRV-Reflect shader module");
            return -1;
        }
        if (shader_kind == shaderc_vertex_shader && shader.reflect_shader_module.shader_stage != SPV_REFLECT_SHADER_STAGE_VERTEX_BIT) {
            tklog_error("generated reflect shader and shaderc kind is not the same");
            return -1;
        }
        if (shader_kind == shaderc_fragment_shader && shader.reflect_shader_module.shader_stage != SPV_REFLECT_SHADER_STAGE_FRAGMENT_BIT) {
            tklog_error("generated reflect shader and shaderc kind is not the same");
            return -1;
        }
        if (shader_kind == shaderc_compute_shader && shader.reflect_shader_module.shader_stage != SPV_REFLECT_SHADER_STAGE_COMPUTE_BIT) {
            tklog_error("generated reflect shader and shaderc kind is not the same");
            return -1;
        }
        if (!(shader_kind == shaderc_vertex_shader || shader_kind == shaderc_fragment_shader || shader_kind == shaderc_compute_shader)) {
            tklog_critical("shader kind is not supported. AND THIS SHOULD HAVE BEEN CHECKED EARLIER");
            return -1;
        }
    }

    // get gpu device path
    {
        shader.gpu_device_index = gpu_device_index;
    }

    // SDL shader 
    {
        // vertex or fragment shader
        if (shader_kind == shaderc_vertex_shader || shader_kind == shaderc_fragment_shader) {
            bool is_vert = shader_kind == shaderc_vertex_shader;
            // Prepare SPIRV_Info for Vertex Shader

            /*
            SDL_ShaderCross_SPIRV_Info shader_info = {
                .bytecode = shader.p_spv_code,
                .bytecode_size = shader.spv_code_size,
                .entrypoint = entrypoint,
                .shader_stage = is_vert ? SDL_SHADERCROSS_SHADERSTAGE_VERTEX : SDL_SHADERCROSS_SHADERSTAGE_FRAGMENT,
                .enable_debug = enable_debug,
                .name = NULL,
                .props = 0  // Assuming no special properties
            };
            */

            SDL_GPUShaderCreateInfo shader_info = _cpi_Shader_CreateShaderInfo(shader.p_spv_code, shader.spv_code_size, entrypoint, is_vert);
            

            tklog_scope(int gpu_device_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_gpu_device_type));
            tklog_scope(vec_MoveToIndex(pp_vec, gpu_device_vec_index, cpi_gpu_device_type));
            tklog_scope(vec_SwitchReadToWrite(*pp_vec));
            tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_vec, shader.gpu_device_index, cpi_gpu_device_type));
            if (!p_gpu_device) {
                tklog_critical("NULL pointer");
                return -1;
            }
            if (!p_gpu_device->p_gpu_device) {
                tklog_critical("NULL pointer");
                return -1;
            }
            SDL_ShaderCross_GraphicsShaderMetadata metadata;
            // tklog_scope(shader.p_sdl_shader = SDL_ShaderCross_CompileGraphicsShaderFromSPIRV(p_gpu_device->p_gpu_device, &shader_info, &metadata));
            // if (!shader.p_sdl_shader) {
            //  tklog_error("Failed to compile Shader from SPIR-V. %s", SDL_GetError());
            //  exit(-1);
            // }
            tklog_scope(shader.p_sdl_shader = SDL_CreateGPUShader(p_gpu_device->p_gpu_device, &shader_info));
            if (!shader.p_sdl_shader) {
                tklog_error("Failed to compile Shader from SPIR-V. %s", SDL_GetError());
                return -1;
            }
            tklog_scope(vec_SwitchWriteToRead(*pp_vec));
            tklog_scope(vec_MoveToIndex(pp_vec, -1, vec_type));
        } 
        // compute shader. there is no sdl shader for compute. its integrated directly into the pipeline
        else {
            shader.p_sdl_shader = NULL;
        }
    }

    #ifdef DEBUG 
        SDL_LockMutex(g_unique_id_mutex);
        shader.id = g_unique_id++;
        SDL_UnlockMutex(g_unique_id_mutex);
    #endif 



    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(int shader_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_vec, cpi_shader_type));
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveToIndex(pp_vec, shader_vec_index, cpi_shader_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));
    tklog_scope(int shader_index = vec_UpsertNullElement_UnsafeWrite(*pp_vec, cpi_shader_type));
    tklog_scope(CPI_Shader* p_shader = (CPI_Shader*)vec_GetElement_UnsafeRead(*pp_vec, shader_index, cpi_shader_type));
    if (!p_shader) {
        tklog_critical("NULL pointer");
        return -1;
    }
    memcpy(p_shader, &shader, sizeof(CPI_Shader));
    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveEnd(pp_vec));

    printf("SUCCESSFULLY created shader\n");
    return shader_index;
}
void cpi_Shader_Destructor(
    void* p_void) 
{
    CPI_Shader* p_shader = (CPI_Shader*)p_void;
    if (!p_shader) {
        tklog_critical("NULL pointer");
        return;
    }

    bool is_null = true;
    for (int i = 0; i < sizeof(CPI_ShadercCompiler); ++i) {
        if (((unsigned char*)p_shader)[i] != 0) {
            is_null = false;
        }
    }
    if (is_null) {
        tklog_critical("shaderc compiler is null");
        return;
    }

    if (!p_shader->p_glsl_code) {
        tklog_critical("NULL pointer before freeing");
        return;
    }
    if (!p_shader->p_spv_code) {
        tklog_critical("NULL pointer before freeing");
        return;
    }
    free(p_shader->p_glsl_code); 
    free(p_shader->p_spv_code);

    tklog_scope(spvReflectDestroyShaderModule(&p_shader->reflect_shader_module));

    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_vec, p_shader->gpu_device_index, cpi_gpu_device_type));
    if (!p_gpu_device) {
        tklog_critical("NULL pointer");
        return;
    }
    if (!p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        return;
    }
    tklog_scope(SDL_ReleaseGPUShader(p_gpu_device->p_gpu_device, p_shader->p_sdl_shader));
    tklog_scope(vec_MoveToIndex(pp_vec, -1, vec_type));
    memset(p_shader, 0, sizeof(CPI_Shader));
}
void cpi_Shader_Destroy(
    int* p_shader_index) 
{
    if (!p_shader_index) {
        tklog_critical("NULL pointer");
        return;
    }
    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(int shader_vec_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_shader_type));
    tklog_scope(vec_MoveToIndex(pp_vec, shader_vec_index, cpi_shader_type));
    tklog_scope(CPI_Shader* p_shader = (CPI_Shader*)vec_GetElement_UnsafeRead(*pp_vec, *p_shader_index, cpi_shader_type));
    tklog_scope(cpi_Shader_Destructor(p_shader));
    tklog_scope(vec_MoveEnd(pp_vec));
    *p_shader_index = -1;
}

// ===============================================================================================================
// Graphics Pipeline
// ===============================================================================================================
int cpi_GraphicsPipeline_Create(
    int vertex_shader_index,
    int fragment_shader_index,
    bool enable_debug)
{

    tklog_scope(Vec** pp_shader_vec = vec_MoveStart(g_vec));
    tklog_scope(int shader_vec_index = vec_GetVecWithType_UnsafeRead(*pp_shader_vec, cpi_shader_type));
    tklog_scope(vec_MoveToIndex(pp_shader_vec, shader_vec_index, cpi_shader_type));
    tklog_scope(CPI_Shader* p_vertex_shader = (CPI_Shader*)vec_GetElement_UnsafeRead(*pp_shader_vec, vertex_shader_index, cpi_shader_type));
    tklog_scope(CPI_Shader* p_fragment_shader = (CPI_Shader*)vec_GetElement_UnsafeRead(*pp_shader_vec, fragment_shader_index, cpi_shader_type));

    if (p_vertex_shader->gpu_device_index != p_fragment_shader->gpu_device_index) {
        tklog_error("shaders does not contain the same gpu device");
        return -1;
    }
    int gpu_device_index = p_vertex_shader->gpu_device_index;

    
    // 1. Vertex Input State
    unsigned int vertex_attributes_count;
    unsigned int vertex_binding_stride;
    tklog_scope(SDL_GPUVertexAttribute* vertex_attributes = _cpi_Shader_Create_VertexInputAttribDesc(vertex_shader_index, &vertex_attributes_count, &vertex_binding_stride));

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
        .vertex_shader = p_vertex_shader->p_sdl_shader,
        .fragment_shader = p_fragment_shader->p_sdl_shader,
    };
    
    tklog_scope(Vec** pp_gpu_device_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_GetVecWithType_UnsafeRead(*pp_gpu_device_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_gpu_device_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_gpu_device_vec, gpu_device_index, cpi_gpu_device_type));
    if (!p_gpu_device) {
        tklog_critical("NULL pointer");
        return -1;
    }
    if (!p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        return -1;
    }

    CPI_GraphicsPipeline pipeline = {0};
    pipeline.vertex_shader_index = vertex_shader_index;
    pipeline.fragment_shader_index = fragment_shader_index;
    tklog_scope(pipeline.p_graphics_pipeline = SDL_CreateGPUGraphicsPipeline(p_gpu_device->p_gpu_device, &pipeline_create_info));
    if (!pipeline.p_graphics_pipeline) {
        tklog_error("Failed to create SDL3 graphics pipeline: %s", SDL_GetError());
        return -1;
    }
    tklog_scope(vec_MoveEnd(pp_shader_vec));
    tklog_scope(vec_MoveEnd(pp_gpu_device_vec));

    tklog_scope(Vec** pp_pipeline_vec = vec_MoveStart(g_vec));
    tklog_scope(vec_SwitchReadToWrite(*pp_pipeline_vec));
    tklog_scope(int pipeline_vec_index = vec_UpsertVecWithType_UnsafeWrite(*pp_pipeline_vec, cpi_graphics_pipeline_type));
    tklog_scope(vec_SwitchWriteToRead(*pp_pipeline_vec));
    tklog_scope(vec_MoveToIndex(pp_pipeline_vec, pipeline_vec_index, cpi_graphics_pipeline_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_pipeline_vec));
    tklog_scope(int pipeline_index = vec_UpsertNullElement_UnsafeWrite(*pp_pipeline_vec, cpi_graphics_pipeline_type));
    tklog_scope(CPI_GraphicsPipeline* p_pipeline = (CPI_GraphicsPipeline*)vec_GetElement_UnsafeRead(*pp_pipeline_vec, pipeline_index, cpi_graphics_pipeline_type));
    tklog_scope(memcpy(p_pipeline, &pipeline, sizeof(CPI_GraphicsPipeline)));
    tklog_scope(vec_SwitchWriteToRead(*pp_pipeline_vec));
    tklog_scope(vec_MoveEnd(pp_pipeline_vec));

    printf("Graphics Pipeline created successfully.\n");
    return pipeline_index;
}
void cpi_GraphicsPipeline_Destructor(
    void* p_void)
{
    CPI_GraphicsPipeline* p_graphics_pipeline = (CPI_GraphicsPipeline*)p_void;
    if (!p_graphics_pipeline) {
        tklog_critical("NULL pointer");
        return;
    }

    tklog_scope(Vec** pp_shader_vec = vec_MoveStart(g_vec));
    tklog_scope(int shader_vec_index = vec_GetVecWithType_UnsafeRead(*pp_shader_vec, cpi_shader_type));
    tklog_scope(vec_MoveToIndex(pp_shader_vec, shader_vec_index, cpi_shader_type));
    tklog_scope(CPI_Shader* p_vertex_shader = (CPI_Shader*)vec_GetElement_UnsafeRead(*pp_shader_vec, p_graphics_pipeline->vertex_shader_index, cpi_shader_type));

    tklog_scope(Vec** pp_gpu_device_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_device_vec_index = vec_GetVecWithType_UnsafeRead(*pp_gpu_device_vec, cpi_gpu_device_type));
    tklog_scope(vec_MoveToIndex(pp_gpu_device_vec, gpu_device_vec_index, cpi_gpu_device_type));
    tklog_scope(CPI_GPUDevice* p_gpu_device = (CPI_GPUDevice*)vec_GetElement_UnsafeRead(*pp_gpu_device_vec, p_vertex_shader->gpu_device_index, cpi_gpu_device_type));
    if (!p_gpu_device->p_gpu_device) {
        tklog_critical("NULL pointer");
        return;
    }

    tklog_scope(SDL_ReleaseGPUGraphicsPipeline(p_gpu_device->p_gpu_device, p_graphics_pipeline->p_graphics_pipeline));  
    tklog_scope(memset(p_graphics_pipeline, 0, sizeof(CPI_GraphicsPipeline)));

    tklog_scope(vec_MoveEnd(pp_shader_vec));
    tklog_scope(vec_MoveEnd(pp_gpu_device_vec));
}
void cpi_GraphicsPipeline_Destroy(
    int* p_graphics_pipeline_index)
{
    if (!p_graphics_pipeline_index) {
        tklog_critical("NULL pointer");
        return;
    }
    tklog_scope(Vec** pp_vec = vec_MoveStart(g_vec));
    tklog_scope(int gpu_graphics_pipeline_index = vec_GetVecWithType_UnsafeRead(*pp_vec, cpi_graphics_pipeline_type));
    tklog_scope(vec_MoveToIndex(pp_vec, gpu_graphics_pipeline_index, cpi_graphics_pipeline_type));
    tklog_scope(vec_SwitchReadToWrite(*pp_vec));

    tklog_scope(CPI_GraphicsPipeline* p_pipeline = (CPI_GraphicsPipeline*)vec_GetElement_UnsafeRead(*pp_vec, *p_graphics_pipeline_index, cpi_graphics_pipeline_type));
    tklog_scope(cpi_GraphicsPipeline_Destructor(p_pipeline));

    tklog_scope(vec_SwitchWriteToRead(*pp_vec));
    tklog_scope(vec_MoveEnd(pp_vec));
    *p_graphics_pipeline_index = 0;
}