#ifndef SDL3_SHADER_H
#define SDL3_SHADER_H

#include "tsm.h"
#include "code_monitoring.h"

#include <SDL3/SDL.h>
#include <shaderc/shaderc.h>
#include <SDL3_shadercross/SDL_shadercross.h>
#include "spirv_reflect.h"

struct sdl3_shader {
	struct tsm_base_node 			base;
    const char*                     entrypoint;
	const char*  					glsl_file_path; // can be NULL if there is no file for this shader
    char*                           p_glsl_code;
    void*                           p_spv_code;
    uint32_t                    	spv_code_size;
    uint32_t                    	glsl_code_size;
    shaderc_shader_kind             shader_kind;
    SpvReflectShaderModule          reflect_shader_module;
    SDL_GPUShader*                  p_sdl_shader;
};

CM_RES sdl3_shader_create_from_glsl_file(
	const struct tsm_key* p_key, 
	const char* glsl_file_path, 
	const char* entrypoint, 
	shaderc_shader_kind shader_kind,
	const struct tsm_base_node** pp_output_shader);
CM_RES sdl3_shader_get(
	const struct tsm_key* p_key, 
	const struct tsm_base_node** pp_output_shader); 
CM_RES sdl3_shader_create_vertex_input_attribute_descriptions_1(
    const struct tsm_key* p_vertex_key,
    unsigned int* p_output_attribute_count, 
    unsigned int* p_output_binding_stride,
    SDL_GPUVertexAttribute** pp_output_attribs);
/**
 * Creates vertex input attribute descriptions from a vertex shader's reflection data.
 * Supports 0+ inputs, multiple buffers (loose attrs + blocks), and optional fragment reflection for targets.
 * Outputs: attributes (sorted by location), buffer descriptions (VERTEX rate), target info (color formats from fragment outputs).
 * Frees caller-allocated outputs if error.
 */
CM_RES sdl3_shader_create_vertex_input_attribute_descriptions(
    const struct tsm_key* p_vertex_key,
    const struct tsm_key* p_fragment_key,
    unsigned int* p_attribute_count, 
    SDL_GPUVertexAttribute** pp_output_attribs,
    unsigned int* p_num_vertex_buffers,
    SDL_GPUVertexBufferDescription** pp_vertex_buffer_descriptions,
    SDL_GPUGraphicsPipelineTargetInfo* p_target_info);

#endif // SDL3_SHADER_H