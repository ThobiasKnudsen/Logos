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
CM_RES sdl3_shader_create_vertex_input_attribute_descriptions(
    const struct tsm_key* p_vertex_key,
    unsigned int* p_output_attribute_count, 
    unsigned int* p_output_binding_stride,
    SDL_GPUVertexAttribute** pp_output_attribs);

#endif // SDL3_SHADER_H