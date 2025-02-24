#include "cpi.h"
#define CPI_DEBUG
#include "cpi_debug.h"

#define VK_USE_PLATFORM_XLIB_KHR
#include <SDL3/SDL.h>
#include <stdlib.h>
#include <shaderc/shaderc.h>
#include <SDL3_shadercross/SDL_shadercross.h>
#include "spirv_reflect.h"

typedef struct CPI_List {
#if defined(CPI_DEBUG)
	long long  						id;
#endif
	SDL_Mutex 						mutex_write;
	SDL_Mutex 						mutex_read;
	void*  							p_data;
	unsigned int 					element_size;
	unsigned int  					element_type;
	unsigned long long  			element_count;
	unsigned long long  			element_capacity;
} CPI_List;

typedef struct CPI_Location {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
    int 							index_1;
    int    							index_2;
} CPI_Location;

typedef struct CPI_Context {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
	CPI_List  						location_list;
	CPI_List  						list_list;
} CPI_Context;

typedef struct CPI_Box {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Box;

typedef struct CPI_Process {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Process;

typedef struct CPI_Thread {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
	SDL_Thread*  					p_thread;
	SDL_ThreadID        			thread_id;

	bool  							shaderc_initialized;
	shaderc_compiler_t  			shaderc_compiler;
	shaderc_compile_options_t 		shaderc_options
} CPI_Thread;

typedef struct CPI_Function{
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Function;

typedef struct CPI_Window {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
	SDL_Window* 					p_sdl_window;
	SDL_ThreadID        			thread_id;
} CPI_Window;

typedef struct CPI_Fence {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Fence;

typedef struct CPI_Sampler {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Sampler;

typedef struct CPI_Image {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Image;

typedef struct CPI_Transfer {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Transfer;

typedef struct CPI_Buffer{
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Buffer;



typedef struct CPI_Shader {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
    void*                           p_glsl_code;
    unsigned int                    glsl_code_size;
    void*                           p_spv_code;
    unsigned int                    spv_code_size;
    const char*  					entrypoint;
    shaderc_shader_kind             shader_kind;
    SpvReflectShaderModule          reflect_shader_module;
    SDL_GPUShader*    				p_sdl_shader;
} CPI_Shader;

typedef struct CPI_GraphicsPipeline {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
	CPI_ID_Handle  					vertex_shader_handle;
	CPI_ID_Handle 					fragment_shader_handle;
	SDL_GPUGraphicsPipeline* 		p_sdl_pipeline;
} CPI_GraphicsPipeline;

typedef struct CPI_ComputePipeline {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
	CPI_Shader 						vertex_shader;
	CPI_Shader 						fragment_shader;
} CPI_ComputePipeline;

typedef struct CPI_RenderPass {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_RenderPass;

typedef struct CPI_ComputePass {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_ComputePass;

typedef struct CPI_CopyPass {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_CopyPass;

typedef struct {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
	SDL_ThreadID        			thread_id;
} CPI_Command;

typedef struct {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
} CPI_Context;

typedef struct CPI_GPUdevice {
#if defined(CPI_DEBUG)
	long long   					id;
#endif
	SDL_GPUDevice*					p_gpu_device;
} CPI_GPUdevice;


// global context =====================================================================================
static SDL_GPUDevice* 				g_p_gpu_device = NULL;
static shaderc_compiler_t           g_shaderc_compiler;
static shaderc_compile_options_t    g_shaderc_options;

// context
static CPI_Context*  				g_p_contexts = NULL;
static int  						g_contexts_count = 0;

static CPI_Context 					g_context;


	
// internal functions =================================================================================

	// decleration ====================================================================================
	CPI_ID	_cpi_ID_Create(CPI_TYPE type, CPI_ID_Handle* p_handle);
	bool 	_cpi_ID_Destroy(CPI_ID id);

	int 	_cpi_GetAvailableIndex();
	void*  	_cpi_GetPointer(CPI_ID_Handle handle);

	// implementation =================================================================================

	CPI_ID 	_cpi_ID_Create(
		CPI_TYPE type, 
		CPI_ID_Handle* p_handle) 
	{
		ASSERT(p_handle, "INTERNAL ERROR: NULL POINTER");
		ASSERT(g_p_gpu_device, "INTERNAL ERROR: GPI has to be initialized first");
		ASSERT(g_p_ids && g_ids_capacity>=1, "INTERNAL ERROR: global context has to be initialized first");
		ASSERT(type != CPI_TYPE_ID && type != CPI_TYPE_NONE, "INTERNAL WARNING: why are you trying to create a CPI_NULL_ID?\n");

		int id_index = _cpi_GetAvailableIndex(CPI_TYPE_ID);
		ASSERT(g_ids_count > id_index, "INTERNALL ERROR: got id_index that is out of bounds: id_index = %d", id_index);

		g_p_ids[id_index].unique = g_unique_number_count++;
		g_p_ids[id_index].type = type;
		g_p_ids[id_index].index = _cpi_GetAvailableIndex(type);

		// success
		*p_handle = id_index;
		return g_p_ids[id_index];
	}
	void  	_cpi_ID_Destroy(CPI_ID id) {

	}
	int 	_cpi_GetAvailableIndex(
		CPI_TYPE type) 
	{
		char** pp_data = NULL;
		int* p_count = NULL;
		int* p_capacity = NULL;
		int  type_size = 0;

		switch (type) {
			case CPI_TYPE_ID: {
				pp_data = (char**)&g_p_ids;
				p_count = &g_ids_count;
				p_capacity = &g_ids_capacity;
			 	type_size = sizeof(CPI_ID);
			 	break;
			}
			case CPI_TYPE_GROUP: {
				pp_data = (char**)&g_p_groups;
				p_count = &g_groups_count;
				p_capacity = &g_groups_capacity;
			 	type_size = sizeof(CPI_Group);
			 	break;
			}
			case CPI_TYPE_FUNCTION: {
				pp_data = (char**)&g_p_functions;
				p_count = &g_functions_count;
				p_capacity = &g_functions_capacity;
			 	type_size = sizeof(CPI_Function);
			 	break;
			}
			case CPI_TYPE_COMMAND: {
				pp_data = (char**)&g_p_commands;
				p_count = &g_commands_count;
				p_capacity = &g_commands_capacity;
			 	type_size = sizeof(CPI_Command);
			 	break;
			}
			case CPI_TYPE_FENCE: {
				pp_data = (char**)&g_p_fences;
				p_count = &g_fences_count;
				p_capacity = &g_fences_capacity;
			 	type_size = sizeof(CPI_Fence);
			 	break;
			}
			case CPI_TYPE_RENDER_PASS: {
				pp_data = (char**)&g_p_render_passes;
				p_count = &g_render_passes_count;
				p_capacity = &g_render_passes_capacity;
			 	type_size = sizeof(CPI_RenderPass);
			 	break;
			}
			case CPI_TYPE_COMPUTE_PASS: {
				pp_data = (char**)&g_p_compute_passes;
				p_count = &g_copy_passes_count;
				p_capacity = &g_copy_passes_capacity;
			 	type_size = sizeof(CPI_ComputePass);
			 	break;
			}
			case CPI_TYPE_COPY_PASS: {
				pp_data = (char**)&g_p_copy_passes;
				p_count = &g_copy_passes_count;
				p_capacity = &g_copy_passes_capacity;
			 	type_size = sizeof(CPI_CopyPass);
			 	break;
			}
			case CPI_TYPE_GRAPHICS_PIPELINE: {
				pp_data = (char**)&g_p_graphics_pipelines;
				p_count = &g_graphics_pipelines_count;
				p_capacity = &g_graphics_pipelines_capacity;
			 	type_size = sizeof(CPI_GraphicsPipeline);
			 	break;
			}
			case CPI_TYPE_COMPUTE_PIPELINE: {
				pp_data = (char**)&g_p_compute_pipelines;
				p_count = &g_compute_pipelines_count;
				p_capacity = &g_compute_pipelines_capacity;
			 	type_size = sizeof(CPI_ComputePipeline);
			 	break;
			}
			case CPI_TYPE_SHADER: {
				pp_data = (char**)&g_p_shaders;
				p_count = &g_shaders_count;
				p_capacity = &g_shaders_capacity;
			 	type_size = sizeof(CPI_Shader);
			 	break;
			}
			case CPI_TYPE_WINDOW: {
				pp_data = (char**)&g_p_windows;
				p_count = &g_windows_count;
				p_capacity = &g_windows_capacity;
			 	type_size = sizeof(CPI_Window);
			 	break;
			}
			case CPI_TYPE_SAMPLER: {
				pp_data = (char**)&g_p_samplers;
				p_count = &g_samplers_count;
				p_capacity = &g_samplers_capacity;
			 	type_size = sizeof(CPI_Sampler);
			 	break;
			}
			case CPI_TYPE_IMAGE: {
				pp_data = (char**)&g_p_images;
				p_count = &g_images_count;
				p_capacity = &g_images_capacity;
			 	type_size = sizeof(CPI_Image);
			 	break;
			}
			case CPI_TYPE_TRANSFER: {
				pp_data = (char**)&g_p_transfers;
				p_count = &g_transfers_count;
				p_capacity = &g_transfers_capacity;
			 	type_size = sizeof(CPI_Transfer);
			 	break;
			}
			case CPI_TYPE_BUFFER: {
				pp_data = (char**)&g_p_buffers;
				p_count = &g_buffers_count;
				p_capacity = &g_buffers_capacity;
			 	type_size = sizeof(CPI_Buffer);
			 	break;
			}
			case CPI_TYPE_THREAD: {
				pp_data = (char**)&g_p_threads;
				p_count = &g_threads_count;
				p_capacity = &g_threads_capacity;
			 	type_size = sizeof(CPI_Thread);
			 	break;
			}
			default: {
				ASSERT(false, "CPI_TYPE unvalid\n");
			}	
		}

		ASSERT(pp_data, "INTERNAL ERROR: NULL POINTER");
		ASSERT(p_count, "INTERNAL ERROR: NULL POINTER");
		ASSERT(p_capacity, "INTERNAL ERROR: NULL POINTER");
		ASSERT(type_size!=0, "INTERNAL ERROR: NULL size");
		ASSERT(*pp_data!=NULL || (*p_count==0 && *p_capacity==0), "*pp_data is NULL while *p_count and/or *p_capacity is not 0");

		for (int i = 0; i < *p_count; ++i) {
			long long unique_id = *(long long*)(&(*pp_data)[i*type_size]);
			if (unique_id == 0) { return i; } 
			ASSERT(unique_id > 1 && unique_id < g_unique_number_count, "UNVALID unique_id in index %d", i);
		}

		if ((*p_count) >= (*p_capacity)) {
			int new_capacity = 1;
			while (new_capacity <= (*p_count)) { 
				new_capacity*=2; 
				ASSERT(new_capacity >= 1, "INTERNAL ERROR: new_capacity became negatve");
			}
			TRACK(*pp_data = alloc(*pp_data, type_size * new_capacity));
			memset(*pp_data + type_size*(*p_count), 0, type_size*(new_capacity-(*p_count)));			
		}

		return (*p_count)++;
	}
	void*  	_cpi_GetPointer(
		CPI_ID_Handle handle) 
	{
		ASSERT(cpi_ID_isValid(handle), "INTERNAL ERROR: UNVALID HANDLE");
		CPI_ID id = g_p_ids[handle];
		switch (id.type) {
			case CPI_TYPE_ID: {return (void*)&g_p_ids[id.index];}
			case CPI_TYPE_GROUP: {return (void*)&g_p_groups[id.index];}
			case CPI_TYPE_FUNCTION: {return (void*)&g_p_functions[id.index];}
			case CPI_TYPE_COMMAND: {return (void*)&g_p_commands[id.index];}
			case CPI_TYPE_FENCE: {return (void*)&g_p_fences[id.index];}
			case CPI_TYPE_RENDER_PASS: {return (void*)&g_p_render_passes[id.index];}
			case CPI_TYPE_COMPUTE_PASS: {return (void*)&g_p_compute_passes[id.index];}
			case CPI_TYPE_COPY_PASS: {return (void*)&g_p_copy_passes[id.index];}
			case CPI_TYPE_GRAPHICS_PIPELINE: {return (void*)&g_p_graphics_pipelines[id.index];}
			case CPI_TYPE_COMPUTE_PIPELINE: {return (void*)&g_p_compute_pipelines[id.index];}
			case CPI_TYPE_SHADER: {return (void*)&g_p_shaders[id.index];}
			case CPI_TYPE_WINDOW: {return (void*)&g_p_windows[id.index];}
			case CPI_TYPE_SAMPLER: {return (void*)&g_p_samplers[id.index];}
			case CPI_TYPE_IMAGE: {return (void*)&g_p_images[id.index];}
			case CPI_TYPE_TRANSFER: {return (void*)&g_p_transfers[id.index];}
			case CPI_TYPE_BUFFER: {return (void*)&g_p_buffers[id.index];}
			case CPI_TYPE_THREAD: {return (void*)&g_p_threads[id.index];}
		}
		ASSERT(false, "CPI_TYPE unvalid\n");
	}

// global context =====================================================================================
bool cpi_Initialize() {
	if (g_p_gpu_device) {
		printf("WARNING: CPI is already initialized\n");
		return false;
	}

	if (!SDL_Init(SDL_INIT_VIDEO)) {
		printf("ERROR: failed to initialize SDL3: %s\n", SDL_GetError());
		return false;
	}

	#ifdef __linux__
		#ifdef LCPI_DEBUG
			TRACK(g_p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, true, NULL));
		#else 
			TRACK(g_p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, false, NULL));
		#endif // LCPI_DEBUG
	#elif defined(_WIN64)
		printf("windows 64-bit is not supported yet\n");
		return false;
	#elif defined(_WIN32)
		printf("windows 32-bit is not supported yet\n");
		return false;
	#elif defined(__CYGWIN__)
		printf("cygwin is not supported yet\n");
		return false;
	#elif defined(__APPLE__)
		printf("macos is not supported yet\n");
		return false;
	#elif defined(__FreeBSD__)
		printf("free bsd is not supported yet\n");
		return false;
	#elif defined(__ANDROID__)
		printf("android is not supported yet\n");
		return false;
	#else 
		printf("unrecignized os is not supported\n");
		return false;
	#endif

	if (!g_p_gpu_device) {
		printf("ERROR: failed to create SDL3 device: %s\n", SDL_GetError());
		return false;
	}

	// shaderc
    {
        g_shaderc_compiler = shaderc_compiler_initialize();
        ASSERT(g_shaderc_compiler, "failed to initialize\n ");
        TRACK(g_shaderc_options = shaderc_compile_options_initialize());
        ASSERT(g_shaderc_options, "failed to initialize\n ");
        TRACK(shaderc_compile_options_set_optimization_level(g_shaderc_options, shaderc_optimization_level_zero));
        #ifdef __linux__
        	TRACK(shaderc_compile_options_set_target_env(g_shaderc_options, shaderc_target_env_vulkan, shaderc_env_version_vulkan_1_3));
        #else 
        	printf("os not supported yet\n");
        	exit(-1);
        #endif
    }

    // initializing SDL3_ShaderCross
    if (!SDL_ShaderCross_Init()) {
        SDL_LogError(SDL_LOG_CATEGORY_APPLICATION, "Failed to initialize SDL_ShaderCross.");
        return false;
    }

	// allocating one id
	TRACK(g_p_ids = alloc(g_p_ids, sizeof(CPI_ID)));
	g_ids_count = 0;
	g_ids_capacity = 1;
	g_p_ids[0].unique = 0;
	g_p_ids[0].type = CPI_TYPE_NONE;
	g_p_ids[0].index = -1;

	return true;
}
bool cpi_Destroy() {
	if (!g_p_gpu_device) {
		printf("WARNING: CPI is not initialized so you cannot destroy it\n");
		return false;
	}
	SDL_DestroyGPUDevice(g_p_gpu_device);

	SDL_ShaderCross_Quit();

	return true;
}

// gui_id =============================================================================================

CPI_TYPE cpi_ID_GetType(
	CPI_ID_Handle handle) 
{
	ASSERT(cpi_ID_isValid(handle), "ERROR: given handle is unvalid. handle = %d\n", handle);
	return g_p_ids[handle].type;
}
bool cpi_ID_isValid(
	CPI_ID_Handle handle) 
{
	ASSERT(0 <= handle && handle < g_ids_count, "UNVALID HANDLE");
	CPI_ID id = g_p_ids[handle];
	ASSERT(id.unique >= 0 && id.unique < g_unique_number_count, "UNVALID HANDLE");
	switch (id.type) {
		case CPI_TYPE_ID: {
			ASSERT(id.index != handle, "UNVALID HANDLE");
			ASSERT(0 <= id.index && id.index < g_ids_count, "UNVALID HANDLE");
			ASSERT(cpi_ID_isValid(id.index), "UNVALID HANDLE");
		}
		case CPI_TYPE_GROUP: { ASSERT(0 <= id.index && id.index < g_groups_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_BOX: { ASSERT(0 <= id.index && id.index < g_boxes_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_FUNCTION: { ASSERT(0 <= id.index && id.index < g_functions_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_COMMAND: { ASSERT(0 <= id.index && id.index < g_commands_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_FENCE: { ASSERT(0 <= id.index && id.index < g_fences_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_RENDER_PASS: { ASSERT(0 <= id.index && id.index < g_render_passes_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_COMPUTE_PASS: { ASSERT(0 <= id.index && id.index < g_compute_passes_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_COPY_PASS: { ASSERT(0 <= id.index && id.index < g_copy_passes_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_GRAPHICS_PIPELINE: { ASSERT(0 <= id.index && id.index < g_graphics_pipelines_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_COMPUTE_PIPELINE: { ASSERT(0 <= id.index && id.index < g_compute_pipelines_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_SHADER: { ASSERT(0 <= id.index && id.index < g_shaders_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_WINDOW: { ASSERT(0 <= id.index && id.index < g_windows_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_SAMPLER: { ASSERT(0 <= id.index && id.index < g_samplers_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_IMAGE: { ASSERT(0 <= id.index && id.index < g_images_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_TRANSFER: { ASSERT(0 <= id.index && id.index < g_transfers_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_BUFFER: { ASSERT(0 <= id.index && id.index < g_buffers_count, "UNVALID HANDLE"); break; }
		case CPI_TYPE_THREAD: { ASSERT(0 <= id.index && id.index < g_threads_count, "UNVALID HANDLE"); break; }
		default: { ASSERT(false, "UNVALID HANDLE"); }
	}

	return true;
}

// window =============================================================================================
CPI_ID_Handle cpi_Window_Create(
	unsigned int width, 
	unsigned int height,
	const char* title) 
{
	ASSERT(title, "title is NULL\n");
	CPI_ID_Handle handle = -1;
	TRACK(CPI_ID window = _cpi_ID_Create(CPI_TYPE_WINDOW, &handle));
	ASSERT(handle >= 0, "INTERNAL ERROR: UNVALID HANDLE");
	TRACK(CPI_Window* p_window = &g_p_windows[window.index]);
	ASSERT(!p_window->p_sdl_window, "INTERNAL ERROR: sdl window should be NULL\n");
	TRACK(p_window->p_sdl_window = SDL_CreateWindow(title, width, height, SDL_WINDOW_RESIZABLE));
	ASSERT(p_window->p_sdl_window, "ERROR: failed to create window: %s\n", SDL_GetError());

	return handle;
}
void cpi_Window_Show() {
	SDL_Event event;
    bool quit = false;
    while (!quit) {
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_EVENT_QUIT) {
                quit = true;
            }
        }
        SDL_Delay(16);
    }
}
bool cpi_Window_Destroy(
	CPI_ID_Handle* p_window_handle) 
{
	ASSERT(p_window_handle, "NULL POINTER");
	ASSERT(cpi_ID_isValid(*p_window_handle), "UNVALID WINDOW HANDLE");
	CPI_ID id = g_p_ids[*p_window_handle];
	CPI_Window* p_window = &g_p_windows[id.index];
	SDL_DestroyWindow(p_window->p_sdl_window);


	return true;
}

// shader =============================================================================================
size_t _cpi_Shader_ReadFile(
	const char* filename, 
	char** dst_buffer) 
{
    FILE* file = fopen(filename, "rb");
    if (!file) {
        printf("Failed to open shader source file '%s'\n", filename);
        exit(-1);
    }

    fseek(file, 0, SEEK_END);
    size_t file_size = ftell(file);
    rewind(file);

    *dst_buffer = (char*)alloc(NULL, file_size + 1);
    if (!*dst_buffer) {
        printf("Failed to allocate memory for shader source '%s'\n", filename);
        fclose(file);
        exit(-1);
    }

    size_t readSize = fread(*dst_buffer, 1, file_size, file);
    (*dst_buffer)[file_size] = '\0'; 

    if (readSize != file_size) {
        printf("Failed to read shader source '%s'\n", filename);
        free(*dst_buffer);
        fclose(file);
        exit(-1);
    }

    fclose(file);
    return (unsigned int)file_size;
}
unsigned int _cpi_Shader_FormatSize(
	SDL_GPUVertexElementFormat format) 
{
	ASSERT(format != SDL_GPU_VERTEXELEMENTFORMAT_INVALID, "INTERNAL ERROR: format is invlaid\n");
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
  	ASSERT(result == 0, "INTERNAL ERROR: could not find any format\n");
  	return result;
}
SDL_GPUVertexElementFormat _cpi_spvReflectFormatToSDLGPUformat(
	SpvReflectFormat format) 
{
    switch(format) {
        case SPV_REFLECT_FORMAT_R16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R16G16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_USHORT2;
        case SPV_REFLECT_FORMAT_R16G16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_SHORT2;
        case SPV_REFLECT_FORMAT_R16G16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_HALF2;
        case SPV_REFLECT_FORMAT_R16G16B16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R16G16B16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R16G16B16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R16G16B16A16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_USHORT4;
        case SPV_REFLECT_FORMAT_R16G16B16A16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_SHORT4;
        case SPV_REFLECT_FORMAT_R16G16B16A16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_HALF4;
        case SPV_REFLECT_FORMAT_R32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT;
        case SPV_REFLECT_FORMAT_R32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT;
        case SPV_REFLECT_FORMAT_R32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT;
        case SPV_REFLECT_FORMAT_R32G32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT2;
        case SPV_REFLECT_FORMAT_R32G32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT2;
        case SPV_REFLECT_FORMAT_R32G32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT2;
        case SPV_REFLECT_FORMAT_R32G32B32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R32G32B32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R32G32B32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        case SPV_REFLECT_FORMAT_R32G32B32A32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT4; 
        case SPV_REFLECT_FORMAT_R32G32B32A32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT4;
        case SPV_REFLECT_FORMAT_R32G32B32A32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT4;

        // 64-bit Formats (Unsupported)
        default:
            return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
    }
}
void _cpi_Shader_PrintAttributeDescriptions(
	SDL_GPUVertexAttribute* p_attributes, 
	size_t attribs_count) 
{
    for (unsigned int i = 0; i < attribs_count; ++i) {
        printf("attrib %d\n", i);
        printf("\t%d\n", p_attributes[i].location);
        printf("\t%d\n", p_attributes[i].buffer_slot);
        printf("\t%d\n", p_attributes[i].format);
        printf("\t%d\n", p_attributes[i].offset);
    }
}
SDL_GPUVertexAttribute* _cpi_Shader_Create_VertexInputAttribDesc(
	const CPI_Shader* p_shader,
	unsigned int* p_attribute_count, 
	unsigned int* p_binding_stride) 
{
	ASSERT(p_shader, " ");
	ASSERT(p_attribute_count, " ");
	ASSERT(p_binding_stride, " ");
    ASSERT(p_shader->reflect_shader_module.shader_stage == SPV_REFLECT_SHADER_STAGE_VERTEX_BIT, "Provided shader is not a vertex shader\n");

    // Enumerate input variables
    unsigned int input_var_count = 0;
    TRACK(SpvReflectResult result = spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, NULL));
    ASSERT(result == SPV_REFLECT_RESULT_SUCCESS, "Failed to enumerate input variables\n");

    TRACK(SpvReflectInterfaceVariable** input_vars = alloc(NULL, input_var_count * sizeof(SpvReflectInterfaceVariable*)));
    ASSERT(input_vars, "Failed to allocate memory for input variables\n");

    TRACK(result = spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, input_vars));
    ASSERT(result == SPV_REFLECT_RESULT_SUCCESS, "Failed to get input variables\n");

    // Create an array to hold SDL_GPUVertexAttribute
    TRACK(SDL_GPUVertexAttribute* attribute_descriptions = alloc(NULL, input_var_count * sizeof(SDL_GPUVertexAttribute)));
    ASSERT(attribute_descriptions, "Failed to allocate memory for vertex input attribute descriptions\n");

    unsigned int attribute_index = 0;
    for (unsigned int i = 0; i < input_var_count; ++i) {
        SpvReflectInterfaceVariable* refl_var = input_vars[i];

        // Ignore built-in variables
        if (refl_var->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) {
            continue;
        }

        attribute_descriptions[attribute_index].location = refl_var->location;
        attribute_descriptions[attribute_index].buffer_slot = 0;
        attribute_descriptions[attribute_index].format = _cpi_spvReflectFormatToSDLGPUformat(refl_var->format);
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
        TRACK(unsigned int format_size = _cpi_Shader_FormatSize(attribute_descriptions[i].format));

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
CPI_ID_Handle cpi_Shader_CreateFromGlslFile(
	const char* glsl_file_path, 
	const char* entrypoint,
	shaderc_shader_kind shader_kind, 
	bool enable_debug) 
{
	ASSERT(g_p_gpu_device, "NULL pointer");
	ASSERT(glsl_file_path, "NULL pointer");
	ASSERT(entrypoint, "NULL pointer");
	ASSERT(	shader_kind == shaderc_vertex_shader ||
  			shader_kind == shaderc_fragment_shader || 
  			shader_kind == shaderc_compute_shader,
  			"shader kind is not supported");

	CPI_ID_Handle handle = -1;
	CPI_ID id = _cpi_ID_Create(CPI_TYPE_SHADER, &handle);
	ASSERT(handle >= 0, "INTERNAL ERROR: UNVALID HANDLE");
	CPI_Shader* p_shader = _cpi_GetPointer(handle);

	// entrypoint
	{
		p_shader->entrypoint = entrypoint;
	}

	// shaderc_shader_kind
	{
		p_shader->shader_kind = shader_kind;
	}

	// spv code compilation
	{
		char* p_glsl_code = NULL;
	    TRACK(size_t glsl_code_size = _cpi_Shader_ReadFile(glsl_file_path, &p_glsl_code));
	    ASSERT(p_glsl_code, "NULL pointer");
	   	TRACK(shaderc_compilation_result_t result = shaderc_compile_intcpi_spv(
	    	g_shaderc_compiler, p_glsl_code, glsl_code_size, shader_kind, glsl_file_path, "main", g_shaderc_options));
	    ASSERT(shaderc_result_get_compilation_status(result) == shaderc_compilation_status_success, 
	    	"Shader compilation error in '%s':\n%s\n", glsl_file_path, shaderc_result_get_error_message(result));
		TRACK(p_shader->spv_code_size = shaderc_result_get_length(result));
	    TRACK(p_shader->p_spv_code = alloc(NULL, p_shader->spv_code_size));
	    ASSERT(p_shader->p_spv_code, "Failed to allocate memory for SPIR-V code");
	    memcpy(p_shader->p_spv_code, shaderc_result_get_bytes(result), p_shader->spv_code_size);
	    TRACK(shaderc_result_release(result));
	}

	SDL_GPUShaderStage sdl_shader_stage;

    // SpvReflectShaderModule
    {
		TRACK(SpvReflectResult reflectResult = spvReflectCreateShaderModule(p_shader->spv_code_size, p_shader->p_spv_code, &p_shader->reflect_shader_module));
	    ASSERT(reflectResult == SPV_REFLECT_RESULT_SUCCESS, "Failed to create SPIRV-Reflect shader module\n");
	    if (shader_kind == shaderc_vertex_shader) {
	    	ASSERT(p_shader->reflect_shader_module.shader_stage  == SPV_REFLECT_SHADER_STAGE_VERTEX_BIT, "generated reflect shader and shaderc kind is not the same");
	    }
	    else if (shader_kind == shaderc_fragment_shader) {
	    	ASSERT(p_shader->reflect_shader_module.shader_stage  == SPV_REFLECT_SHADER_STAGE_FRAGMENT_BIT, "generated reflect shader and shaderc kind is not the same");
	    }
	    else if (shader_kind == shaderc_compute_shader) {
	    	ASSERT(p_shader->reflect_shader_module.shader_stage  == SPV_REFLECT_SHADER_STAGE_COMPUTE_BIT, "generated reflect shader and shaderc kind is not the same");
	    }
	    else {
	    	ASSERT(false, "shader kind is not supported. AND THIS SHOULD HAVE BEEN CHECKED EARLIER");
	    }
	}

	// SDL shader 
	{
		// vertex or fragment shader
		if (shader_kind == shaderc_vertex_shader || shader_kind == shaderc_fragment_shader) {
			bool is_vert = shader_kind == shaderc_vertex_shader;
			// Prepare SPIRV_Info for Vertex Shader
		    SDL_ShaderCross_SPIRV_Info shader_info = {
		        .bytecode = p_shader->p_spv_code,
		        .bytecode_size = p_shader->spv_code_size,
		        .entrypoint = entrypoint,
		        .shader_stage = is_vert ? SDL_SHADERCROSS_SHADERSTAGE_VERTEX : SDL_SHADERCROSS_SHADERSTAGE_FRAGMENT,
		        .enable_debug = enable_debug,
		        .name = is_vert ? "Vertex Shader" : "Fragment Shader",
		        .props = 0  // Assuming no special properties
		    };
		    p_shader->p_sdl_shader = SDL_ShaderCross_CompileGraphicsShaderFromSPIRV(g_p_gpu_device, &shader_info, NULL);
		    ASSERT(p_shader->p_sdl_shader, "Failed to compile Vertex Shader from SPIR-V. %s\n", SDL_GetError());
		} 
		// compute shader. there is no sdl shader for compute. its integrated directly into the pipeline
		else {
			p_shader->p_sdl_shader = NULL;
		}
	}

    return handle;
}
// 	TODO:
// 		THIS IS NOT CORRECT. p_shader is not a pointer.
void cpi_Shader_Destroy(
	CPI_ID_Handle* p_shader_handle) 
{
    ASSERT(g_p_gpu_device, "NULL pointer");
    ASSERT(p_shader_handle, "NULL pointer");

	CPI_Shader* p_shader = _cpi_GetPointer(*p_shader_handle);
	ASSERT(p_shader, "There exists no vertex shader internally with handle = %d\n", p_shader_handle);

    if (p_shader->p_glsl_code) {
        free(p_shader->p_glsl_code);
    }
    if (p_shader->p_spv_code) {
        free(p_shader->p_spv_code);
    }
    spvReflectDestroyShaderModule(&p_shader->reflect_shader_module);
    SDL_ReleaseGPUShader(g_p_gpu_device, p_shader->p_sdl_shader);
    memset(p_shader, 0, sizeof(CPI_Shader));
    p_shader = NULL;
    *p_shader_handle = -1;
}

// graphics pipeline ==================================================================================
CPI_ID_Handle cpi_GraphicsPipeline_Create(
    CPI_ID_Handle vertex_shader_handle,
    CPI_ID_Handle fragment_shader_handle,
    bool enable_debug)
{
	ASSERT(g_p_gpu_device, "gpu_device is NULL");
	CPI_Shader* p_vertex_shader = _cpi_GetPointer(vertex_shader_handle);
	ASSERT(p_vertex_shader, "There exists no vertex shader internally with ID = %d\n", vertex_shader_handle);
	CPI_Shader* p_fragment_shader = _cpi_GetPointer(fragment_shader_handle);
	ASSERT(p_fragment_shader, "There exists no fragment shader internally with ID = %d\n", fragment_shader_handle);

	CPI_ID_Handle handle = -1;
	CPI_ID p_pipeline_id = _cpi_ID_Create(CPI_TYPE_GRAPHICS_PIPELINE, &handle);
	CPI_GraphicsPipeline* p_pipeline = _cpi_GetPointer(handle);

    // Create Graphics Pipeline with the compiled shaders
    p_pipeline->vertex_shader_handle = vertex_shader_handle;
    p_pipeline->fragment_shader_handle = fragment_shader_handle;

	// 1. Vertex Input State
	unsigned int vertex_attributes_count;
	unsigned int vertex_binding_stride;
	SDL_GPUVertexAttribute* vertex_attributes = _cpi_Shader_Create_VertexInputAttribDesc(p_vertex_shader, &vertex_attributes_count, &vertex_binding_stride);

	SDL_GPUVertexInputState vertex_input_state = {
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
	    .num_vertex_attributes = vertex_attributes_count
	};

	// 3. Rasterizer State
	SDL_GPURasterizerState rasterizer_state = {
	    .fill_mode = SDL_GPU_FILLMODE_FILL,
	    .cull_mode = SDL_GPU_CULLMODE_BACK,
	    .front_face = SDL_GPU_FRONTFACE_CLOCKWISE,
	    .depth_bias_constant_factor = 0.0f,
	    .depth_bias_clamp = 0.0f,
	    .depth_bias_slope_factor = 0.0f,
	    .enable_depth_bias = false,
	    .enable_depth_clip = true,
	    .padding1 = 0,
	    .padding2 = 0
	};

	// 4. Multisample State
	SDL_GPUMultisampleState multisample_state = {
	    .sample_count = SDL_GPU_SAMPLECOUNT_1,
	    .sample_mask = 0xFFFFFFFF,
	    .enable_mask = false,
	    .padding1 = 0,
	    .padding2 = 0,
	    .padding3 = 0
	};

	// 5. Depth Stencil State
	SDL_GPUDepthStencilState depth_stencil_state = {
	    .compare_op = SDL_GPU_COMPAREOP_LESS,
	    .back_stencil_state = { /* Initialize as needed */ },
	    .front_stencil_state = { /* Initialize as needed */ },
	    .compare_mask = 0xFF,
	    .write_mask = 0xFF,
	    .enable_depth_test = false,
	    .enable_depth_write = false,
	    .enable_stencil_test = false,
	    .padding1 = 0,
	    .padding2 = 0,
	    .padding3 = 0
	};

	// 7. Render Targets
	SDL_GPUTextureFormat color_format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM;

	SDL_GPUColorTargetDescription color_targets[] = {
	    {
	        .format = color_format,
	        .blend_state = (SDL_GPUColorTargetBlendState){
	            .src_color_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA,
	            .dst_color_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
	            .color_blend_op = SDL_GPU_BLENDOP_ADD,
	            .src_alpha_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA,
	            .dst_alpha_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
	            .alpha_blend_op = SDL_GPU_BLENDOP_ADD,
	            .color_write_mask = SDL_GPU_COLORCOMPONENT_R
	                              | SDL_GPU_COLORCOMPONENT_G
	                              | SDL_GPU_COLORCOMPONENT_B
	                              | SDL_GPU_COLORCOMPONENT_A,
	            .enable_blend = true,
	            .enable_color_write_mask = true
	        }
	    }
	};

	SDL_GPUGraphicsPipelineTargetInfo target_info = {
	    .color_target_descriptions = color_targets,
	    .num_color_targets = 1,
	    .depth_stencil_format = SDL_GPU_TEXTUREFORMAT_INVALID,
	    .has_depth_stencil_target = false,
	    .padding1 = 0,
	    .padding2 = 0,
	    .padding3 = 0
	};


	// 8. Pipeline Creation
	SDL_GPUGraphicsPipelineCreateInfo pipeline_create_info = {
	    .vertex_shader = p_vertex_shader->p_sdl_shader,
    	.fragment_shader = p_fragment_shader->p_sdl_shader,
	    .vertex_input_state = vertex_input_state,
	    .primitive_type = SDL_GPU_PRIMITIVETYPE_TRIANGLESTRIP,
	    .rasterizer_state = rasterizer_state,
	    .multisample_state = multisample_state,
	    .depth_stencil_state = depth_stencil_state,
	    .target_info = target_info,
	    .props = 0
	};

	p_pipeline->p_sdl_pipeline = SDL_CreateGPUGraphicsPipeline(g_p_gpu_device, &pipeline_create_info);
	ASSERT(p_pipeline->p_sdl_pipeline, "Failed to create SDL3 graphics pipeline: %s\n", SDL_GetError());

    // Optionally, you can set pipeline metadata or other properties here
    // For example, setting the pipeline name for debugging purposes
    // SDL_GPU_SetPipelineName(pipeline, "MyGraphicsPipeline");

    printf("Graphics Pipeline created successfully.");
    return handle;
}
void cpi_GraphicsPipeline_Destroy(
	CPI_ID_Handle* p_graphics_pipeline_handle) 
{
	SDL_GPUGraphicsPipeline** pp_pipeline = &((CPI_GraphicsPipeline*)_cpi_GetPointer(*p_graphics_pipeline_handle))->p_sdl_pipeline;
	SDL_ReleaseGPUGraphicsPipeline(g_p_gpu_device, *pp_pipeline);
	*pp_pipeline = NULL;
	*p_graphics_pipeline_handle = -1;
}

// compute pipeline ===================================================================================
SDL_GPUComputePipeline* CreateComputePipelineFromSPIRV(
    const Uint8 *p_compute_shader_spv,
    size_t compute_size,
    const char *compute_entrypoint,
    bool enable_debug)
{
	ASSERT(g_p_gpu_device, "gpu_device is NULL");
	ASSERT(p_compute_shader_spv, "p_compute_shader_spv is NULL");
	ASSERT(compute_entrypoint, "compute_entrypoint is NULL");

    // Initialize SDL_ShaderCross if not already initialized
    static bool shader_cross_initialized = false;
    if (!shader_cross_initialized) {
        if (!SDL_ShaderCross_Init()) {
            SDL_LogError(SDL_LOG_CATEGORY_APPLICATION, "Failed to initialize SDL_ShaderCross.");
            return NULL;
        }
        shader_cross_initialized = true;
    }

    // Prepare SPIRV_Info for Compute Shader
    SDL_ShaderCross_SPIRV_Info computeInfo = {
        .bytecode = p_compute_shader_spv,
        .bytecode_size = compute_size,
        .entrypoint = compute_entrypoint,
        .shader_stage = SDL_SHADERCROSS_SHADERSTAGE_COMPUTE,
        .enable_debug = enable_debug,
        .name = "Compute Shader",
        .props = 0  // Assuming no special properties
    };

    // Compile Compute Pipeline
    SDL_ShaderCross_ComputePipelineMetadata metadata = {0};
    SDL_GPUComputePipeline *computePipeline = SDL_ShaderCross_CompileComputePipelineFromSPIRV(g_p_gpu_device, &computeInfo, &metadata);

    if (!computePipeline) {
        SDL_LogError(SDL_LOG_CATEGORY_APPLICATION, "Failed to compile Compute Pipeline from SPIR-V.");
        return NULL;
    }

    // Optionally, you can use metadata for further configurations
    // For example, setting thread counts or resource bindings based on metadata

    SDL_LogInfo(SDL_LOG_CATEGORY_APPLICATION, "Compute Pipeline created successfully.");
    return computePipeline;
}
