#include "vec.h"
#include "cpi.h"
#include "global_data/core.h"
#include <stdlib.h>
#include <SDL3/SDL.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <assert.h>

// PLAN:
// 	create TKLOG
//  create nodes and graphs
//  store everyting in the GOD graph
//  then were back again
//  create functions for images

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

/*
 * test_1.c  —  Exercises every feature of the compile‑time tklogger.
 *
 * Build example (SDL 3 installed and tklog.c / tklog.h in same dir):
 *   cc -std=c11 -Wall -Wextra -I/path/to/SDL3/include \
 *      test_1.c tklog.c `sdl3-config --libs` -o test_1
 */

/*
int test_lfht(void)
{
    printf("URCU Lock-Free Hash Table Test\n");
    printf("==============================\n\n");
    
    // Initialize SDL 
    if (!SDL_Init(0)) {
        printf("Failed to initialize SDL: %s\n", SDL_GetError());
        return 1;
    }
    
    // Initialize RCU 
    rcu_init();
    
    // Run tests 
    if (gd_test_basic_operations() != 0) {
        printf("Basic operations test failed\n");
        SDL_Quit();
        return 1;
    }
    
    if (gd_test_concurrent_access() != 0) {
        printf("Concurrent access test failed\n");
        SDL_Quit();
        return 1;
    }
    
    printf("All tests passed!\n");
    
    SDL_Quit();
    return 0;
}
*/

int main(void) {
    // Initialize SDL3 (required for tklog)
	
	cpi_Initialize();
	int gpu_device_id = cpi_GPUDevice_Create();
	int window_id = cpi_Window_Create(gpu_device_id, 800, 600, "Λόγος");
	int vert_id = cpi_Shader_CreateFromGlslFile(gpu_device_id, "../shaders/shader.vert.glsl", "main", shaderc_vertex_shader, true);
	int frag_id = cpi_Shader_CreateFromGlslFile(gpu_device_id, "../shaders/shader.frag.glsl", "main", shaderc_fragment_shader, true);
	int graphics_pipeline_id = cpi_GraphicsPipeline_Create(vert_id, frag_id, true);
	
	cpi_Window_Show(window_id, graphics_pipeline_id);
	cpi_GraphicsPipeline_Destroy(&graphics_pipeline_id);
	cpi_Shader_Destroy(&vert_id);
	cpi_Shader_Destroy(&frag_id);
	cpi_Window_Destroy(&window_id);
	
	return 0;
}