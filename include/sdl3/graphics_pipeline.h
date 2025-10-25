#ifndef SDL3_GRAPHICS_PIPELINE_H
#define SDL3_GRAPHICS_PIPELINE_H

#include "tsm.h"
#include "code_monitoring.h"
#include <SDL3/SDL.h>

struct sdl3_graphics_pipeline {
    struct tsm_base_node            base;
    struct tsm_key                  vertex_shader_key;
    struct tsm_key                  fragment_shader_key;
    SDL_GPUGraphicsPipeline*        p_graphics_pipeline;
};

CM_RES sdl3_graphics_pipeline_create(
	const struct tsm_key* p_key,
	const struct tsm_key* p_vertex_key,
	const struct tsm_key* p_fragment_key);
CM_RES sdl3_graphics_pipeline_create_1(
    const struct tsm_key* p_key,
    const struct tsm_key* p_vertex_key,
    const struct tsm_key* p_fragment_key);
CM_RES sdl3_graphics_pipeline_get(
	const struct tsm_key* p_key,
	const struct tsm_base_node** p_output_pipeline);

#endif // SDL3_GRAPHICS_PIPELINE_h