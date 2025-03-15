#include "debug.h"
#include "vec.h"
#include "cpi.h"
#include <stdlib.h>

// Λόγος
// libuv
// libcurl

// 	TODO:
// 		ID shouldnt be int but long long
//  	GPI_Shader should be integrated into graphics and compute pipeline
//  	remove this_type as static vec_type exists now
// 		change from read dominant to write dominant so that writes can be prioritized faster
//  	when doing vec_SetCount_SafeWrite you should actually lock write before finding the new count, but Vec is not designed to do that so one thread could SetCount before this thread so that when this thread does it both threads could end up having the same index. i dont know how to fix this without making other stuff more difficult. for now you must check that the new index is null 
// 		when starting a Vec** how can you be sure that all parents are locked? maybe you should have a vec function that gets a Vec* then checks that the parent is NULL then locks that Vec*
// 		now youve added switch between read and write functions. to make this more usefull maybe you need to add more mutexes
/*
n_1:u8:43
arr_1:[]u8:[43, 23, 541, 632, 15]
arr_1:[]{a:u32,b:f32}:[{2, 4.3}, {14, 24.5}, {241, 42.5}]
p_arr_1:&[]{a:u32,b:f32}:&arr_1;

arr_2:p_arr_1@
arr_2[0].a 
*/

int main() {

	//cpi_Debug();
	
	DEBUG_SCOPE(cpi_Initialize());
	DEBUG_SCOPE(int gpu_device_index = cpi_GPUDevice_Create());
	DEBUG_SCOPE(int window_index = cpi_Window_Create(gpu_device_index, 800, 600, "Λόγος"));
	DEBUG_SCOPE(int vert_index = cpi_Shader_CreateFromGlslFile(gpu_device_index, "../shaders/shader.vert.glsl", "main", shaderc_vertex_shader, true));
	DEBUG_SCOPE(int frag_index = cpi_Shader_CreateFromGlslFile(gpu_device_index, "../shaders/shader.frag.glsl", "main", shaderc_fragment_shader, true));
	DEBUG_SCOPE(int graphics_pipeline_index = cpi_GraphicsPipeline_Create(vert_index, frag_index, true));
	
	DEBUG_SCOPE(cpi_Window_Show(window_index, graphics_pipeline_index));
	DEBUG_SCOPE(cpi_GraphicsPipeline_Destroy(&graphics_pipeline_index));
	DEBUG_SCOPE(cpi_Shader_Destroy(&vert_index));
	DEBUG_SCOPE(cpi_Shader_Destroy(&frag_index));
	DEBUG_SCOPE(cpi_Window_Destroy(&window_index));
	
	return 0;
}