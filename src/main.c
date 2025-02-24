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
/*
n_1:u8:43
arr_1:[]u8:[43, 23, 541, 632, 15]
arr_1:[]{a:u32,b:f32}:[{2, 4.3}, {14, 24.5}, {241, 42.5}]
p_arr_1:&[]{a:u32,b:f32}:&arr_1;

arr_2:p_arr_1@
arr_2[0].a 
*/




void* alloc(void* ptr, size_t size) {
    void* tmp = (ptr == NULL) ? malloc(size) : realloc(ptr, size);
    if (!tmp) {
        printf("Memory allocation failed\n");
        exit(-1);
    }
    return tmp;
}

int main() {
	DEBUG_SCOPE(cpi_Initialize());
	DEBUG_SCOPE(char* gpu_device_path = cpi_GPUDevice_Create());
	DEBUG_SCOPE(char* window_path = cpi_Window_Create(800, 600, "Λόγος"));
	DEBUG_SCOPE(char* vert_path = cpi_Shader_CreateFromGlslFile(gpu_device_path, "../shaders/shader.vert.glsl", "main", shaderc_vertex_shader, true));
	DEBUG_SCOPE(char* frag_path = cpi_Shader_CreateFromGlslFile(gpu_device_path, "../shaders/shader.frag.glsl", "main", shaderc_fragment_shader, true));
	DEBUG_SCOPE(char* graphics_pipeline_path = cpi_GraphicsPipeline_Create(vert_path, frag_path, true));
	DEBUG_SCOPE(cpi_GraphicsPipeline_Destroy(&graphics_pipeline_path));
	DEBUG_SCOPE(cpi_Shader_Destroy(&vert_path));
	DEBUG_SCOPE(cpi_Shader_Destroy(&frag_path));

	DEBUG_SCOPE(cpi_Window_Show(window_path));
	DEBUG_SCOPE(cpi_Window_Destroy(&window_path));
	/*
	TRACK(ASSERT(o_Initialize(), "ERROR: could not initialize Lo_GUI\n"));
	TRACK(CPI_ID_Handle window_handle = o_Window_Create(800, 600, "Λόγος"));
	TRACK(ASSERT(window_handle>=0, "ERROR: failed to create window\n"));
	TRACK(o_Window_Show(window_handle));
	TRACK(ASSERT(o_Window_Destroy(&window_handle), "ERROR: failed to destroy window\n"));
	TRACK(ASSERT(o_Destroy(), "ERROR: could not destroy Lo_GUI\n"));
	*/
	return 0;
}