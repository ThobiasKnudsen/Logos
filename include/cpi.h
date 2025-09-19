#ifndef CPI_H
#define CPI_H

#include "map.h"
#include <stdbool.h>
#include <stdarg.h>
#include <shaderc/shaderc.h>

typedef struct {
    struct { float x, y, w, h; } 	rect;
    float 							rotation;
    float 							corner_radius_pixels;
    struct { unsigned char r, g, b, a; } 	color;
    unsigned int 					tex_index;
    struct { float x, y, w, h; } 	tex_rect;
} Rect;

// ======================================================================================================================
// main
// ======================================================================================================================
void 					cpi_Initialize(
							void);
void 					cpi_Debug(
							void);

// ======================================================================================================================
// CPU Device
// ======================================================================================================================
int  					cpi_GPUDevice_Create(
							void);
void    				cpi_GPUDevice_Destructor(
							void* p_gpu_device);
void    				cpi_GPUDevice_Destroy(
							int* gpu_device_id);

// ======================================================================================================================
// Windows
// ======================================================================================================================
int 					cpi_Window_Create(
							int gpu_device_id,
							unsigned int width, 
							unsigned int height, 
							const char* title);
void 					cpi_Window_Show(
							int window_id,
							int graphics_pipeline_id);
void  					cpi_Window_Destructor(
							void* p_window);
void  					cpi_Window_Destroy(
							int* p_window_index);

// ======================================================================================================================
// Shader
// ======================================================================================================================
int						cpi_Shader_CreateFromGlslFile(
							int gpu_device_id,
							const char* glsl_file_path, 
							const char* entrypoint, 
							shaderc_shader_kind shader_kind, 
							bool enable_debug);
void 					cpi_Shader_Destructor(
							void* p_shader);
void 					cpi_Shader_Destroy(
							int* p_shader_id);

// ======================================================================================================================
// Graphics Pipeline
// ======================================================================================================================
int  					cpi_GraphicsPipeline_Create(
						    int vertex_shader_id,
						    int fragment_shader_id,
						    bool enable_debug);
void 					cpi_GraphicsPipeline_Destructor(
							void* p_graphics_pipeline);
void 					cpi_GraphicsPipeline_Destroy(
							int* graphics_pipeline_id);

// ======================================================================================================================
// Shaderc Compiler
// ======================================================================================================================
// char* 					cpi_ShadercCompiler_Create();
void 					cpi_ShadercCompiler_Destructor(
							void* p_graphics_pipeline);
void 					cpi_ShadercCompiler_Destroy(
							int* graphics_pipeline_id);

#endif // CPI_H