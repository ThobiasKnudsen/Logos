#ifndef CPI_H
#define CPI_H

#include <stdbool.h>
#include <stdarg.h>
#include <shaderc/shaderc.h>

// ======================================================================================================================
// main
// ======================================================================================================================
void 					cpi_Initialize();

// ======================================================================================================================
// CPU Device
// ======================================================================================================================
char*   				cpi_GPUDevice_Create();
void    				cpi_GPUDevice_Destructor(
							void* p_gpu_device);
void    				cpi_GPUDevice_Destroy(
							char** p_gpu_device_path);

// ======================================================================================================================
// Windows
// ======================================================================================================================
char*  					cpi_Window_Create(
							unsigned int width, 
							unsigned int height, 
							const char* title);
void 					cpi_Window_Show(
							char* window_path);
char*					cpi_Window_GetGroupID(
							char* window_path);
void  					cpi_Window_Destructor(
							void* p_window);
void  					cpi_Window_Destroy(
							char** p_window_path);

// ======================================================================================================================
// Shader
// ======================================================================================================================
char* 					cpi_Shader_CreateFromGlslFile(
							char* gpu_device_path,
							const char* glsl_file_path, 
							const char* entrypoint, 
							shaderc_shader_kind shader_kind, 
							bool enable_debug);
void 					cpi_Shader_Destructor(
							void* p_shader);
void 					cpi_Shader_Destroy(
							char** p_shader_path);

// ======================================================================================================================
// Graphics Pipeline
// ======================================================================================================================
char* 					cpi_GraphicsPipeline_Create(
						    char* vertex_shader_path,
						    char* fragment_shader_path,
						    bool enable_debug);
void 					cpi_GraphicsPipeline_Destructor(
							void* p_graphics_pipeline);
void 					cpi_GraphicsPipeline_Destroy(
							char** p_graphics_pipeline_path);

// ======================================================================================================================
// Shaderc Compiler
// ======================================================================================================================
// char* 					cpi_ShadercCompiler_Create();
void 					cpi_ShadercCompiler_Destructor(
							void* p_graphics_pipeline);
void 					cpi_ShadercCompiler_Destroy(
							char** p_graphics_pipeline_path);

#endif // CPI_H