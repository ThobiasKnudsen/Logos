#include "code_monitoring.h"
#include "tsm.h"
#include "sdl3/core.h"
#include "sdl3/gpu_device.h"
#include "sdl3/shader.h"
#include "sdl3/graphics_pipeline.h"
#include "sdl3/window.h"


CM_RES main(void) {

    CM_TIMER_START();
    	rcu_init();
    	rcu_register_thread();
    CM_TIMER_STOP();

    CM_TIMER_START();
    	CM_ASSERT(CM_RES_SUCCESS == gtsm_init());
    CM_TIMER_STOP();


    rcu_read_lock();
	    CM_TIMER_START();
	    	struct tsm_key core_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &core_key));
	    CM_TIMER_STOP();

    	CM_TIMER_START();
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_core_init(gtsm_get(), &core_key));
	    CM_TIMER_STOP();

	    CM_TIMER_START();
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_create());
	    CM_TIMER_STOP();

	    CM_TIMER_START();
	    	struct tsm_key vert_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &vert_key));
	    	const struct tsm_base_node* p_vert_node = NULL;
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_shader_create_from_glsl_file(&vert_key, "../shaders/shader.vert.glsl", "main", shaderc_vertex_shader, &p_vert_node));
	    CM_TIMER_STOP();
	    CM_TIMER_START();
	    	struct tsm_key frag_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &frag_key));
	    	const struct tsm_base_node* p_frag_node = NULL;
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_shader_create_from_glsl_file(&frag_key, "../shaders/shader.frag.glsl", "main", shaderc_fragment_shader, &p_frag_node));
	    CM_TIMER_STOP();
	    CM_TIMER_START();
	    	struct tsm_key mandel_vert_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &mandel_vert_key));
	    	const struct tsm_base_node* p_mandel_vert_node = NULL;
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_shader_create_from_glsl_file(&mandel_vert_key, "../shaders/mandelbrot.vert.glsl", "main", shaderc_vertex_shader, &p_mandel_vert_node));
	    CM_TIMER_STOP();
	    CM_TIMER_START();
	    	struct tsm_key mandel_frag_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &mandel_frag_key));
	    	const struct tsm_base_node* p_mandel_frag_node = NULL;
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_shader_create_from_glsl_file(&mandel_frag_key, "../shaders/mandelbrot.frag.glsl", "main", shaderc_fragment_shader, &p_mandel_frag_node));
	    CM_TIMER_STOP();

	    CM_TIMER_START();
	    	struct tsm_key graphics_pipeline_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &graphics_pipeline_key));
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_graphics_pipeline_create(&graphics_pipeline_key, &vert_key, &frag_key));
	    CM_TIMER_STOP();

	    CM_TIMER_START();
	    	struct tsm_key mandel_graphics_pipeline_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &mandel_graphics_pipeline_key));
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_graphics_pipeline_create(&mandel_graphics_pipeline_key, &mandel_vert_key, &mandel_frag_key));
	    CM_TIMER_STOP();

	    CM_TIMER_START();
	    	struct tsm_key window_key = {0};
	    	CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, &window_key));
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_window_create(&window_key, 800, 600, "Λόγος"));
	    CM_TIMER_STOP();

	    CM_TIMER_START();
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_window_show(&window_key, &graphics_pipeline_key));
	    CM_TIMER_STOP();

	    CM_TIMER_START();
	    	CM_ASSERT(CM_RES_SUCCESS == sdl3_window_show_1(&window_key, &mandel_graphics_pipeline_key));
	    CM_TIMER_STOP();
    rcu_read_unlock();

    CM_TIMER_START();
    	CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_destroy());
    CM_TIMER_STOP();

    rcu_read_lock();
    	CM_ASSERT(CM_RES_SUCCESS == gtsm_print());
	    CM_TIMER_START();
	    	CM_SCOPE(CM_RES_SUCCESS == gtsm_free());
	    CM_TIMER_STOP();
    rcu_read_unlock();

    CM_TIMER_START();
    	rcu_barrier();
    	rcu_barrier();
    	rcu_unregister_thread();
    CM_TIMER_STOP();

    CM_TIMER_PRINT();

    CM_LOG_NOTICE("Logos successfully finished\n");
	
	return 0;
}