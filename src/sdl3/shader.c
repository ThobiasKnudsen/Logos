#include "sdl3/shader.h"
#include "sdl3/gpu_device.h"
#include "sdl3/core.h"

#include <shaderc/shaderc.h>

static const struct tsm_key g_sdl3_shaderc_compiler_type_key    = { .key_union.string = "sdl3_shaderc_compiler_type", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_sdl3_shaderc_compiler_tsm_key     = { .key_union.string = "sdl3_shaderc_compiler_tsm", .key_type = TSM_KEY_TYPE_STRING };

static const struct tsm_key g_sdl3_shader_type_key  = { .key_union.string = "sdl3_shader_type", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_sdl3_shader_tsm_key   = { .key_union.string = "sdl3_shader_tsm", .key_type = TSM_KEY_TYPE_STRING };

struct sdl3_shaderc_compiler {
    struct tsm_base_node            base;
    shaderc_compiler_t              shaderc_compiler;
    shaderc_compile_options_t       shaderc_options;
};

static void _sdl3_shaderc_compiler_type_free_callback(struct rcu_head* p_rcu) {
    CM_ASSERT(p_rcu);
    struct tsm_base_node* p_base = caa_container_of(p_rcu, struct tsm_base_node, rcu_head);
    struct sdl3_shaderc_compiler* p_shaderc_compiler = caa_container_of(p_base, struct sdl3_shaderc_compiler, base);

    shaderc_compiler_release(p_shaderc_compiler->shaderc_compiler);
    shaderc_compile_options_release(p_shaderc_compiler->shaderc_options);
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(p_base));
}
static CM_RES _sdl3_shaderc_compiler_type_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    CM_SCOPE(CM_RES res = tsm_base_node_is_valid(p_tsm_base, p_base));
    return res;
}
static CM_RES _sdl3_shaderc_compiler_type_print(const struct tsm_base_node* p_base) {
    CM_SCOPE(CM_RES res = tsm_base_node_print(p_base));
    return res;
}

static void _sdl3_shader_type_free_callback(struct rcu_head* p_rcu) {
	CM_ASSERT(p_rcu);
	struct tsm_base_node* p_base = caa_container_of(p_rcu, struct tsm_base_node, rcu_head);
	struct sdl3_shader* p_shader = caa_container_of(p_base, struct sdl3_shader, base);

    free(p_shader->p_glsl_code); 
    free(p_shader->p_spv_code);
    spvReflectDestroyShaderModule(&p_shader->reflect_shader_module);
    SDL_GPUDevice* p_gpu_device = NULL;
    rcu_read_lock();
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_get(&p_gpu_device));
    SDL_ReleaseGPUShader(p_gpu_device, p_shader->p_sdl_shader);
    rcu_read_unlock();

	CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(p_base));
}
static CM_RES _sdl3_shader_type_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
	CM_SCOPE(CM_RES res = tsm_base_node_is_valid(p_tsm_base, p_base));
	if (res != CM_RES_TSM_NODE_IS_VALID) {
		return res;
	}
	const struct sdl3_shader* p_shader = caa_container_of(p_base, struct sdl3_shader, base);
	if (!p_shader->entrypoint || !p_shader->p_glsl_code || !p_shader->p_spv_code || !p_shader->p_sdl_shader) {
		return CM_RES_NULL_FIELDS;
	}
	if (p_shader->shader_kind != shaderc_vertex_shader &&
        p_shader->shader_kind != shaderc_fragment_shader &&
		p_shader->shader_kind != shaderc_compute_shader) {
		return CM_RES_SDL3_UNKOWN_SHADER_KIND;
	}
	return CM_RES_TSM_NODE_IS_VALID;
}
static CM_RES _sdl3_shader_type_print(const struct tsm_base_node* p_base) {
	CM_ASSERT(p_base);

	tsm_base_node_print(p_base);
	struct sdl3_shader* p_shader = caa_container_of(p_base, struct sdl3_shader, base);

	CM_LOG_TSM_PRINT("    entrypoint: %s\n", p_shader->entrypoint);
	CM_LOG_TSM_PRINT("	  glsl code:\n%s\n", p_shader->p_glsl_code);
	return CM_RES_SUCCESS;
}

/**
 * This function will create a shaderc compiler for each thread if one doesnt exist
 */
CM_RES _sdl3_shaderc_compiler_get(const struct tsm_base_node** pp_output_compiler) {
    CM_ASSERT(pp_output_compiler);
    // getting the core TSM where the shaderc compiler TSM should be
    const struct tsm_base_node* p_core_tsm = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_core_tsm_get(&p_core_tsm));
    // getting the shaderc compiler TSM
    const struct tsm_base_node* p_shaderc_compiler_tsm = NULL;
    CM_SCOPE(CM_RES res = tsm_node_get(p_core_tsm, &g_sdl3_shaderc_compiler_tsm_key, &p_shaderc_compiler_tsm));
    // if not found then we will create and TSM and shaderc compiler type inside it
    if (res != CM_RES_SUCCESS) {
        CM_ASSERT(res == CM_RES_TSM_NODE_NOT_FOUND);
        // create the TSM
        struct tsm_base_node* p_new_tsm = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_core_tsm, &g_sdl3_shaderc_compiler_tsm_key, &p_new_tsm));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_core_tsm, p_new_tsm));
        // create the type inside the TSM
        struct tsm_base_node* p_type = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
            &g_sdl3_shaderc_compiler_type_key,
            sizeof(struct tsm_base_type_node),
            _sdl3_shaderc_compiler_type_free_callback,
            _sdl3_shaderc_compiler_type_is_valid,
            _sdl3_shaderc_compiler_type_print,
            sizeof(struct sdl3_shaderc_compiler),
            &p_type));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_new_tsm, p_type));
        // trying to get the shaderc compiler TSM again
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_core_tsm, &g_sdl3_shaderc_compiler_tsm_key, &p_shaderc_compiler_tsm));
    }
    // getting the thread id
    CM_SCOPE(SDL_ThreadID this_thread_id = SDL_GetCurrentThreadID());
    // creating the thread id key
    struct tsm_key thread_key = {0};
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(this_thread_id, &thread_key));
    // getting the shaderc compiler
    CM_SCOPE(res = tsm_node_get(p_shaderc_compiler_tsm, &thread_key, pp_output_compiler));
    // If the shaderc compiler for this thread is not found then we must create it
    if (res != CM_RES_SUCCESS) {
        CM_ASSERT(res == CM_RES_TSM_NODE_NOT_FOUND);
        // creating the shaderc compiler node
        struct tsm_base_node* p_new_compiler_base = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(&thread_key, &g_sdl3_shaderc_compiler_type_key, sizeof(struct sdl3_shaderc_compiler), &p_new_compiler_base));
        struct sdl3_shaderc_compiler* p_new_compiler = caa_container_of(p_new_compiler_base, struct sdl3_shaderc_compiler, base);
        CM_SCOPE(p_new_compiler->shaderc_compiler = shaderc_compiler_initialize());
        CM_ASSERT(p_new_compiler->shaderc_compiler)
        CM_SCOPE(p_new_compiler->shaderc_options = shaderc_compile_options_initialize());
        CM_ASSERT(p_new_compiler->shaderc_options);
        shaderc_compile_options_set_optimization_level(p_new_compiler->shaderc_options, shaderc_optimization_level_zero);
        #ifdef __linux__
            shaderc_compile_options_set_target_env(p_new_compiler->shaderc_options, shaderc_target_env_vulkan, shaderc_env_version_vulkan_1_0);
        #else 
            CM_LOG_ERROR("OS not supported yet");
        #endif
        // inserting the shaderc compiler node
        CM_SCOPE(CM_RES res = tsm_node_insert(p_shaderc_compiler_tsm, p_new_compiler_base));
        if (res != CM_RES_SUCCESS) {
            gtsm_print();
            CM_LOG_WARNING("res is %d\n", res);
            return res;
        }
        // getting the same shaderc compiler node
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_shaderc_compiler_tsm, &thread_key, pp_output_compiler)); 
    } 
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_free(&thread_key)); // must be freed as the use of it was depp copied
    return CM_RES_SUCCESS;
}
CM_RES _sdl3_shader_glsl_to_spv(
    const char*         entrypoint, 
    shaderc_shader_kind shader_kind, 
    struct sdl3_shader* p_shader) {

    CM_ASSERT(entrypoint && p_shader);
    CM_ASSERT(p_shader->glsl_file_path && p_shader->p_glsl_code>0 && p_shader->glsl_code_size);
    CM_ASSERT(  shader_kind == shaderc_vertex_shader ||
                shader_kind == shaderc_fragment_shader || 
                shader_kind == shaderc_compute_shader);

    p_shader->entrypoint = entrypoint;
    p_shader->shader_kind = shader_kind;

    // get shaderc compiler path
    const struct tsm_base_node* p_compiler_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == _sdl3_shaderc_compiler_get(&p_compiler_base));
    const struct sdl3_shaderc_compiler* p_compiler = caa_container_of(p_compiler_base, struct sdl3_shaderc_compiler, base);

    shaderc_compilation_result_t result = shaderc_compile_into_spv(
        p_compiler->shaderc_compiler, 
        p_shader->p_glsl_code, 
        p_shader->glsl_code_size, 
        p_shader->shader_kind, 
        p_shader->glsl_file_path, 
        p_shader->entrypoint, 
        p_compiler->shaderc_options);
    if (shaderc_result_get_compilation_status(result) != shaderc_compilation_status_success) {
        CM_LOG_ERROR("Shader compilation error in '%s':\n%s", p_shader->glsl_file_path, shaderc_result_get_error_message(result));
    }
    CM_SCOPE(p_shader->spv_code_size = shaderc_result_get_length(result));
    CM_SCOPE(p_shader->p_spv_code = malloc(p_shader->spv_code_size));
    CM_ASSERT(p_shader->p_spv_code);
    CM_SCOPE(memcpy(p_shader->p_spv_code, shaderc_result_get_bytes(result), p_shader->spv_code_size));
    CM_SCOPE(shaderc_result_release(result));

    return CM_RES_SUCCESS;
}
CM_RES _sdl3_shader_read_file(const char* filename, char** const pp_dst_buffer, unsigned long long* p_dst_size) {
    CM_ASSERT(filename && pp_dst_buffer && p_dst_size);
    CM_ASSERT(*pp_dst_buffer == NULL);
    FILE* file = fopen(filename, "rb");
    CM_ASSERT(file);
    fseek(file, 0, SEEK_END);
    unsigned long long file_size = ftell(file);
    rewind(file);
    CM_SCOPE(*pp_dst_buffer = (char*)malloc(file_size + 1));
    CM_ASSERT(*pp_dst_buffer != NULL)
    unsigned long long readSize = fread(*pp_dst_buffer, 1, file_size, file);
    (*pp_dst_buffer)[file_size] = '\0'; 
    CM_ASSERT(readSize == file_size);
    fclose(file);
    *p_dst_size = file_size;
    return CM_RES_SUCCESS;
}
CM_RES _sdl3_shader_read_glsl_file(const char* filename, struct sdl3_shader* p_shader) {
    CM_ASSERT(filename && p_shader);
    CM_ASSERT(p_shader->glsl_file_path == NULL && p_shader->p_glsl_code == NULL && p_shader->glsl_code_size == 0);
    FILE* file = fopen(filename, "rb");
    CM_ASSERT(file);
    fseek(file, 0, SEEK_END);
    p_shader->glsl_code_size = ftell(file);
    rewind(file);
    CM_SCOPE(p_shader->p_glsl_code = (char*)malloc(p_shader->glsl_code_size + 1));
    CM_ASSERT(p_shader->p_glsl_code != NULL)
    unsigned long long readSize = fread(p_shader->p_glsl_code, 1, p_shader->glsl_code_size, file);
    p_shader->p_glsl_code[p_shader->glsl_code_size] = '\0'; 
    CM_ASSERT(readSize == p_shader->glsl_code_size);
    fclose(file);
    p_shader->glsl_file_path = filename;
    return CM_RES_SUCCESS;
}
CM_RES _sdl3_shader_format_size(SDL_GPUVertexElementFormat format, unsigned int* p_dst_size) {
    CM_ASSERT(format != SDL_GPU_VERTEXELEMENTFORMAT_INVALID && p_dst_size);
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
    CM_ASSERT(result != 0);
    *p_dst_size = result;
    return CM_RES_SUCCESS;
}
SDL_GPUVertexElementFormat _sdl3_shader_SPV_REFLECT_format_to_SDL_GPU_TEXTUREFORMAT(SpvReflectFormat format) {
    switch(format) {
        case SPV_REFLECT_FORMAT_R16G16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_USHORT2;
        case SPV_REFLECT_FORMAT_R16G16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_SHORT2;
        case SPV_REFLECT_FORMAT_R16G16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_HALF2;
        case SPV_REFLECT_FORMAT_R16G16B16A16_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_USHORT4;
        case SPV_REFLECT_FORMAT_R16G16B16A16_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_SHORT4;
        case SPV_REFLECT_FORMAT_R16G16B16A16_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_HALF4;
        case SPV_REFLECT_FORMAT_R32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT;
        case SPV_REFLECT_FORMAT_R32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT;
        case SPV_REFLECT_FORMAT_R32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT;
        case SPV_REFLECT_FORMAT_R32G32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT2;
        case SPV_REFLECT_FORMAT_R32G32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT2;
        case SPV_REFLECT_FORMAT_R32G32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT2;
        case SPV_REFLECT_FORMAT_R32G32B32A32_UINT: return SDL_GPU_VERTEXELEMENTFORMAT_UINT4; 
        case SPV_REFLECT_FORMAT_R32G32B32A32_SINT: return SDL_GPU_VERTEXELEMENTFORMAT_INT4;
        case SPV_REFLECT_FORMAT_R32G32B32A32_SFLOAT: return SDL_GPU_VERTEXELEMENTFORMAT_FLOAT4;

        // 64-bit Formats (Unsupported)
        default: {
            CM_LOG_ERROR("spv reflect format is not supported");
            return SDL_GPU_VERTEXELEMENTFORMAT_INVALID;
        }
    }
}
CM_RES _sdl3_shader_print_attribute_descriptions(SDL_GPUVertexAttribute* p_attributes, unsigned long long attribs_count) {
    CM_ASSERT(p_attributes);
    for (unsigned int i = 0; i < attribs_count; ++i) {
        CM_LOG_INFO("attrib %d\n", i);
        CM_LOG_INFO("\t%d\n", p_attributes[i].location);
        CM_LOG_INFO("\t%d\n", p_attributes[i].buffer_slot);
        CM_LOG_INFO("\t%d\n", p_attributes[i].format);
        CM_LOG_INFO("\t%d\n", p_attributes[i].offset);
    }
    return CM_RES_SUCCESS;
}
CM_RES sdl3_shader_create_vertex_input_attribute_descriptions_1(
    const struct tsm_key* p_vertex_key,
    unsigned int* p_attribute_count, 
    unsigned int* p_binding_stride,
    SDL_GPUVertexAttribute** pp_output_attribs) 
{
    CM_ASSERT(p_vertex_key && p_attribute_count && p_binding_stride && pp_output_attribs);

    // get the vertex shader
    const struct tsm_base_node* p_shader_base = NULL;
    CM_SCOPE(CM_RES res = sdl3_shader_get(p_vertex_key, &p_shader_base));
    if (res != CM_RES_SUCCESS) {
    	CM_LOG_WARNING("shader node not found with code %d\n", res);
    	return res;
    }
    const struct sdl3_shader* p_shader = caa_container_of(p_shader_base, struct sdl3_shader, base);
	CM_ASSERT(p_shader->reflect_shader_module.shader_stage == SPV_REFLECT_SHADER_STAGE_VERTEX_BIT);

    // Enumerate input variables
    unsigned int input_var_count = 0;
    CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, NULL));

    // allocate memory
    SpvReflectInterfaceVariable** input_vars = malloc(input_var_count * sizeof(SpvReflectInterfaceVariable*));
    CM_ASSERT(input_vars);

    CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, input_vars));

    // Create an array to hold SDL_GPUVertexAttribute
    CM_SCOPE(SDL_GPUVertexAttribute* attribute_descriptions = malloc(input_var_count * sizeof(SDL_GPUVertexAttribute)));
    CM_ASSERT(attribute_descriptions)

    unsigned int attribute_index = 0;
    for (unsigned int i = 0; i < input_var_count; ++i) {
        SpvReflectInterfaceVariable* refl_var = input_vars[i];
        if (refl_var->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) // Ignore built-in variables
        	continue;
        attribute_descriptions[attribute_index].location = refl_var->location;
        attribute_descriptions[attribute_index].buffer_slot = 0; // ASSUMES ONLY ONE SLOT
        CM_SCOPE(attribute_descriptions[attribute_index].format = _sdl3_shader_SPV_REFLECT_format_to_SDL_GPU_TEXTUREFORMAT(refl_var->format));
        attribute_descriptions[attribute_index].offset = 0; // WILL CALCULATE OFFSET LATER
        attribute_index++;
    }

    // Update the attribute count
    *p_attribute_count = attribute_index;
    // Sort attributes by location (guarded to prevent underflow when <=1 attrs)
    if (attribute_index >= 2) {
        for (unsigned int i = 0; i < attribute_index - 1; ++i) {
            for (unsigned int j = 0; j < attribute_index - i - 1; ++j) {
                if (attribute_descriptions[j].location > attribute_descriptions[j + 1].location) {
                    SDL_GPUVertexAttribute temp = attribute_descriptions[j];
                    attribute_descriptions[j] = attribute_descriptions[j + 1];
                    attribute_descriptions[j + 1] = temp;
                }
            }
        }
    }

    // Compute offsets and buffer_slot stride
    unsigned int offset = 0;
    for (unsigned int i = 0; i < attribute_index; ++i) {
    	unsigned int format_size = 0;
        CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_format_size(attribute_descriptions[i].format, &format_size));
        // Align the offset if necessary (e.g., 4-byte alignment)
        unsigned int alignment = 4;
        offset = (offset + (alignment - 1)) & ~(alignment - 1);
        attribute_descriptions[i].offset = offset;
        offset += format_size;
    }

    *p_binding_stride = offset;
    free(input_vars);
    // CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_print_attribute_descriptions(attribute_descriptions, *p_attribute_count));

    *pp_output_attribs = attribute_descriptions;
    return CM_RES_SUCCESS;  
}
CM_RES sdl3_shader_create_vertex_input_attribute_descriptions(
    const struct tsm_key* p_vertex_key,
    const struct tsm_key* p_fragment_key,  // Optional: NULL if no fragment reflection
    unsigned int* p_attribute_count, 
    SDL_GPUVertexAttribute** pp_output_attribs,
    unsigned int* p_num_vertex_buffers,
    SDL_GPUVertexBufferDescription** pp_vertex_buffer_descriptions,
    SDL_GPUGraphicsPipelineTargetInfo* p_target_info)  // Optional: NULL if no target reflection
{
    CM_ASSERT(p_vertex_key && p_attribute_count && pp_output_attribs && p_num_vertex_buffers && pp_vertex_buffer_descriptions);
    // p_fragment_key and p_target_info optional

    *p_attribute_count = 0;
    *pp_output_attribs = NULL;
    *p_num_vertex_buffers = 0;
    *pp_vertex_buffer_descriptions = NULL;
    if (p_target_info) {
        p_target_info->num_color_targets = 0;
        p_target_info->color_target_descriptions = NULL;
        // depth_stencil_target remains {0}
    }

    // Get vertex shader
    const struct tsm_base_node* p_shader_base = NULL;
    CM_SCOPE(CM_RES res = sdl3_shader_get(p_vertex_key, &p_shader_base));
    if (res != CM_RES_SUCCESS) {
        CM_LOG_WARNING("Vertex shader node not found with code %d\n", res);
        return res;
    }
    const struct sdl3_shader* p_shader = caa_container_of(p_shader_base, struct sdl3_shader, base);
    CM_ASSERT(p_shader->reflect_shader_module.shader_stage == SPV_REFLECT_SHADER_STAGE_VERTEX_BIT);

    // Enumerate input variables
    unsigned int input_var_count = 0;
    CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, NULL));

    if (input_var_count == 0) {
        return CM_RES_SUCCESS;  // Valid: no inputs (e.g., procedural verts)
    }

    // Allocate and enumerate input vars (array of pointers)
    SpvReflectInterfaceVariable** input_vars = malloc(input_var_count * sizeof(SpvReflectInterfaceVariable*));
    CM_ASSERT(input_vars);
    CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateInputVariables(&p_shader->reflect_shader_module, &input_var_count, input_vars));

    // Phase 1: Count total attributes, blocks, and loose
    unsigned int total_attribs = 0;
    unsigned int num_blocks = 0;
    bool has_loose = false;
    for (unsigned int i = 0; i < input_var_count; ++i) {
        SpvReflectInterfaceVariable* refl_var = input_vars[i];
        if (refl_var->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) continue;
        if (refl_var->member_count > 0) {
            num_blocks++;
            for (unsigned int j = 0; j < refl_var->member_count; ++j) {
                if (!(input_vars[i]->members[j].decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN)) {
                    total_attribs++;
                }
            }
        } else {
            has_loose = true;
            total_attribs++;
        }
    }

    unsigned int num_buffers = (has_loose ? 1 : 0) + num_blocks;
    if (num_buffers > 32 || total_attribs > 32) {  // Vulkan limits
        free(input_vars);
        return CM_RES_SDL3_TOO_MANY_VERTEX_BUFFERS;
    }

    // Alloc outputs
    CM_SCOPE(SDL_GPUVertexAttribute* attribute_descriptions = malloc(total_attribs * sizeof(SDL_GPUVertexAttribute)));
    CM_ASSERT(attribute_descriptions);
    CM_SCOPE(SDL_GPUVertexBufferDescription* buffer_descriptions = malloc(num_buffers * sizeof(SDL_GPUVertexBufferDescription)));
    CM_ASSERT(buffer_descriptions);

    // Phase 2: Fill attributes and buffers
    unsigned int attr_idx = 0;
    unsigned int buf_idx = 0;
    unsigned int current_slot = 0;

    // Loose attributes (packed, slot 0)
    if (has_loose) {
        unsigned int loose_offset = 0;
        for (unsigned int i = 0; i < input_var_count; ++i) {
            SpvReflectInterfaceVariable* refl_var = input_vars[i];
            if (refl_var->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) continue;
            if (refl_var->member_count > 0) continue;  // Blocks later
            // Loose
            attribute_descriptions[attr_idx].location = refl_var->location;
            attribute_descriptions[attr_idx].buffer_slot = current_slot;
            attribute_descriptions[attr_idx].format = _sdl3_shader_SPV_REFLECT_format_to_SDL_GPU_TEXTUREFORMAT(refl_var->format);
            unsigned int fsize = 0;
            CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_format_size(attribute_descriptions[attr_idx].format, &fsize));
            attribute_descriptions[attr_idx].offset = loose_offset;
            loose_offset += fsize;
            unsigned int alignment = 4;
            loose_offset = (loose_offset + (alignment - 1)) & ~(alignment - 1);
            attr_idx++;
        }
        // Buffer desc for slot 0
        buffer_descriptions[buf_idx].slot = current_slot;
        buffer_descriptions[buf_idx].pitch = loose_offset;
        buffer_descriptions[buf_idx].input_rate = SDL_GPU_VERTEXINPUTRATE_INSTANCE;  // Per-instance for procedural quads
        buffer_descriptions[buf_idx].instance_step_rate = 1;
        buf_idx++;
        current_slot++;
    }

    // Blocks (sequential slots)
    for (unsigned int i = 0; i < input_var_count; ++i) {
        SpvReflectInterfaceVariable* refl_var = input_vars[i];
        if (refl_var->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) continue;
        if (refl_var->member_count == 0) continue;  // Loose done
        // Block
        unsigned int block_offset = 0;
        unsigned int block_max = 0;
        for (unsigned int j = 0; j < refl_var->member_count; ++j) {
            SpvReflectInterfaceVariable member = refl_var->members[j];
            if (member.decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) continue;
            attribute_descriptions[attr_idx].location = member.location;
            attribute_descriptions[attr_idx].buffer_slot = current_slot;
            attribute_descriptions[attr_idx].format = _sdl3_shader_SPV_REFLECT_format_to_SDL_GPU_TEXTUREFORMAT(member.format);
            attribute_descriptions[attr_idx].offset = block_offset;  // Cumulative offset (bytes)
            unsigned int fsize = 0;
            CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_format_size(attribute_descriptions[attr_idx].format, &fsize));
            block_offset += fsize;
            unsigned int alignment = 4;  // Align for next member (matches loose case)
            block_offset = (block_offset + (alignment - 1)) & ~(alignment - 1);
            block_max = SDL_max(block_max, block_offset);
            attr_idx++;
        }
        // Buffer desc (align to 16 for vec4 blocks)
        unsigned int alignment = 16;
        block_max = (block_max + (alignment - 1)) & ~(alignment - 1);
        buffer_descriptions[buf_idx].slot = current_slot;
        buffer_descriptions[buf_idx].pitch = block_max;
        buffer_descriptions[buf_idx].input_rate = SDL_GPU_VERTEXINPUTRATE_VERTEX;
        buffer_descriptions[buf_idx].instance_step_rate = 0;
        buf_idx++;
        current_slot++;
    }

    free(input_vars);

    // Sort attributes by location
    unsigned int final_count = attr_idx;  // May be less if all built-in
    if (final_count >= 2) {
        for (unsigned int i = 0; i < final_count - 1; ++i) {
            for (unsigned int j = 0; j < final_count - i - 1; ++j) {
                if (attribute_descriptions[j].location > attribute_descriptions[j + 1].location) {
                    SDL_GPUVertexAttribute temp = attribute_descriptions[j];
                    attribute_descriptions[j] = attribute_descriptions[j + 1];
                    attribute_descriptions[j + 1] = temp;
                }
            }
        }
    }

    *p_attribute_count = final_count;
    *pp_output_attribs = attribute_descriptions;
    *p_num_vertex_buffers = num_buffers;
    *pp_vertex_buffer_descriptions = buffer_descriptions;

    // Optional: Fragment reflection for targets
    if (p_fragment_key && p_target_info) {
        const struct tsm_base_node* p_frag_base = NULL;
        CM_SCOPE(res = sdl3_shader_get(p_fragment_key, &p_frag_base));
        if (res == CM_RES_SUCCESS) {
            const struct sdl3_shader* p_frag_shader = caa_container_of(p_frag_base, struct sdl3_shader, base);
            CM_ASSERT(p_frag_shader->reflect_shader_module.shader_stage == SPV_REFLECT_SHADER_STAGE_FRAGMENT_BIT);

            unsigned int output_var_count = 0;
            CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateOutputVariables(&p_frag_shader->reflect_shader_module, &output_var_count, NULL));

            unsigned int out_count = 0;
            if (output_var_count > 0) {
                SpvReflectInterfaceVariable** output_vars = malloc(output_var_count * sizeof(SpvReflectInterfaceVariable*));
                CM_ASSERT(output_vars);
                CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateOutputVariables(&p_frag_shader->reflect_shader_module, &output_var_count, output_vars));

                for (unsigned int i = 0; i < output_var_count; ++i) {
                    if (!(output_vars[i]->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN)) out_count++;
                }
                free(output_vars);
            }

            if (out_count > 0) {
                SDL_GPUColorTargetDescription *descs = malloc(out_count * sizeof(SDL_GPUColorTargetDescription));
                CM_ASSERT(descs);
                unsigned int out_idx = 0;
                // Re-enumerate to fill (or cache, but simple re-do for clarity)
                CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateOutputVariables(&p_frag_shader->reflect_shader_module, &output_var_count, NULL));
                SpvReflectInterfaceVariable** output_vars = malloc(output_var_count * sizeof(SpvReflectInterfaceVariable*));
                CM_ASSERT(output_vars);
                CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateOutputVariables(&p_frag_shader->reflect_shader_module, &output_var_count, output_vars));

                for (unsigned int i = 0; i < output_var_count; ++i) {
                    SpvReflectInterfaceVariable* out_var = output_vars[i];
                    if (out_var->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) continue;
                    descs[out_idx].format = SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM;  // Hardcode to swapchain format (shader output is always float, SDL converts)
                    // Set default blend and write mask (unchanged)
                    descs[out_idx].blend_state.enable_blend = true;
                    descs[out_idx].blend_state.src_color_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA;
                    descs[out_idx].blend_state.dst_color_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA;
                    descs[out_idx].blend_state.color_blend_op = SDL_GPU_BLENDOP_ADD;
                    descs[out_idx].blend_state.src_alpha_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA;
                    descs[out_idx].blend_state.dst_alpha_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA;
                    descs[out_idx].blend_state.alpha_blend_op = SDL_GPU_BLENDOP_ADD;
                    descs[out_idx].blend_state.color_write_mask = SDL_GPU_COLORCOMPONENT_R | SDL_GPU_COLORCOMPONENT_G | SDL_GPU_COLORCOMPONENT_B | SDL_GPU_COLORCOMPONENT_A;
                    descs[out_idx].blend_state.enable_color_write_mask = true;
                    out_idx++;
                }
                free(output_vars);
                p_target_info->color_target_descriptions = (const SDL_GPUColorTargetDescription*) descs;
            }
            p_target_info->num_color_targets = out_count;
            if (out_count == 0) {
                // Fallback: Assume single standard output (common for swapchain rendering)
                p_target_info->num_color_targets = 1;
                SDL_GPUColorTargetDescription *fallback_desc = malloc(sizeof(SDL_GPUColorTargetDescription));
                CM_ASSERT(fallback_desc);
                fallback_desc[0].format = SDL_GPU_TEXTUREFORMAT_B8G8R8A8_UNORM;
                fallback_desc[0].blend_state.enable_blend = true;
                fallback_desc[0].blend_state.src_color_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA;
                fallback_desc[0].blend_state.dst_color_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA;
                fallback_desc[0].blend_state.color_blend_op = SDL_GPU_BLENDOP_ADD;
                fallback_desc[0].blend_state.src_alpha_blendfactor = SDL_GPU_BLENDFACTOR_SRC_ALPHA;
                fallback_desc[0].blend_state.dst_alpha_blendfactor = SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA;
                fallback_desc[0].blend_state.alpha_blend_op = SDL_GPU_BLENDOP_ADD;
                fallback_desc[0].blend_state.color_write_mask = SDL_GPU_COLORCOMPONENT_R |
                                                                SDL_GPU_COLORCOMPONENT_G |
                                                                SDL_GPU_COLORCOMPONENT_B |
                                                                SDL_GPU_COLORCOMPONENT_A;
                fallback_desc[0].blend_state.enable_color_write_mask = true;
                p_target_info->color_target_descriptions = (const SDL_GPUColorTargetDescription*) fallback_desc;
            }
        }
    }

    // Optional print
    // CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_print_attribute_descriptions(attribute_descriptions, *p_attribute_count));

    return CM_RES_SUCCESS;  
}
CM_RES _sdl3_shader_create_spv_reflect_module(struct sdl3_shader* p_shader) {
    // SpvReflectShaderModule
    CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectCreateShaderModule(p_shader->spv_code_size, p_shader->p_spv_code, &p_shader->reflect_shader_module));
    if (p_shader->shader_kind == shaderc_vertex_shader && p_shader->reflect_shader_module.shader_stage != SPV_REFLECT_SHADER_STAGE_VERTEX_BIT)
        CM_LOG_ERROR("generated reflect shader and shaderc kind is not the same");
    if (p_shader->shader_kind == shaderc_fragment_shader && p_shader->reflect_shader_module.shader_stage != SPV_REFLECT_SHADER_STAGE_FRAGMENT_BIT)
        CM_LOG_ERROR("generated reflect shader and shaderc kind is not the same");
    if (p_shader->shader_kind == shaderc_compute_shader && p_shader->reflect_shader_module.shader_stage != SPV_REFLECT_SHADER_STAGE_COMPUTE_BIT)
        CM_LOG_ERROR("generated reflect shader and shaderc kind is not the same");
    if (!(p_shader->shader_kind == shaderc_vertex_shader || p_shader->shader_kind == shaderc_fragment_shader || p_shader->shader_kind == shaderc_compute_shader))
        CM_LOG_ERROR("shader kind is not supported. AND THIS SHOULD HAVE BEEN CHECKED EARLIER");
    return CM_RES_SUCCESS;
}
CM_RES _sdl3_shader_create_sdl3_shader(struct sdl3_shader* p_shader) {
    CM_ASSERT(p_shader);
    CM_ASSERT(p_shader->p_spv_code && p_shader->spv_code_size>0 && p_shader->entrypoint);
    CM_ASSERT(p_shader->p_sdl_shader == NULL);
    CM_ASSERT(p_shader->glsl_file_path && p_shader->p_glsl_code>0 && p_shader->glsl_code_size);

    // vertex or fragment shader
    if (p_shader->shader_kind == shaderc_vertex_shader || p_shader->shader_kind == shaderc_fragment_shader) {
        bool is_vert = p_shader->shader_kind == shaderc_vertex_shader;

        SDL_GPUShaderCreateInfo shader_info = {0};
        shader_info.code       = p_shader->p_spv_code;
        shader_info.code_size  = p_shader->spv_code_size;
        shader_info.entrypoint = p_shader->entrypoint;
        shader_info.format     = SDL_GPU_SHADERFORMAT_SPIRV;
        shader_info.stage      = is_vert ? SDL_GPU_SHADERSTAGE_VERTEX : SDL_GPU_SHADERSTAGE_FRAGMENT;
        shader_info.props      = 0;  // No extension properties for now

        // Create a SPIR-V reflection module
        SpvReflectShaderModule module;
        CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectCreateShaderModule(p_shader->spv_code_size, p_shader->p_spv_code, &module));
        // First, determine how many descriptor bindings exist
        uint32_t binding_count = 0;
        CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateDescriptorBindings(&module, &binding_count, NULL));
        // Allocate an array to hold pointers to descriptor binding info
        SpvReflectDescriptorBinding **bindings = malloc(binding_count * sizeof(SpvReflectDescriptorBinding*));
        CM_ASSERT(bindings);
        // Retrieve the descriptor bindings
        CM_ASSERT(SPV_REFLECT_RESULT_SUCCESS == spvReflectEnumerateDescriptorBindings(&module, &binding_count, bindings));

        // Loop over each binding and update counts based on its type.
        for (uint32_t i = 0; i < binding_count; ++i) {
            const SpvReflectDescriptorBinding *binding = bindings[i];
            switch (binding->descriptor_type) {
                case SPV_REFLECT_DESCRIPTOR_TYPE_SAMPLER:
                case SPV_REFLECT_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER: shader_info.num_samplers++; break;
                case SPV_REFLECT_DESCRIPTOR_TYPE_STORAGE_IMAGE: shader_info.num_storage_textures++; break;
                case SPV_REFLECT_DESCRIPTOR_TYPE_STORAGE_BUFFER: shader_info.num_storage_buffers++; break;
                case SPV_REFLECT_DESCRIPTOR_TYPE_UNIFORM_BUFFER: shader_info.num_uniform_buffers++; break;
                default: break;
            }
        }

        // Clean up
        free(bindings);
        spvReflectDestroyShaderModule(&module);

        // get the one and only gpu device
        SDL_GPUDevice* p_gpu_device = NULL;
        CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_get(&p_gpu_device));
        // create the SDL3 shader
        CM_SCOPE(p_shader->p_sdl_shader = SDL_CreateGPUShader(p_gpu_device, &shader_info));
        CM_ASSERT(p_shader->p_sdl_shader);
    } 
    // compute shader. there is no sdl shader for compute. its integrated directly into the pipeline
    else if (p_shader->shader_kind == shaderc_compute_shader) {
        p_shader->p_sdl_shader = NULL;
    } else {
        CM_LOG_ERROR("shader_kind is something else than vertex, fragment or compute: %d\n", p_shader->shader_kind);
    }

    return CM_RES_SUCCESS;
}

CM_RES sdl3_shader_create_from_glsl_file(
    const struct tsm_key* p_key,
    const char* glsl_file_path, 
    const char* entrypoint,
    shaderc_shader_kind shader_kind,
    const struct tsm_base_node** pp_output_shader) {
    
    CM_ASSERT(p_key && glsl_file_path && entrypoint && pp_output_shader);
    CM_ASSERT(	shader_kind == shaderc_vertex_shader ||
        		shader_kind == shaderc_fragment_shader || 
        		shader_kind == shaderc_compute_shader);

    // getting the gpu_device TSM as there should be a shader TSM inside it
    const struct tsm_base_node* p_gpu_device_tsm = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_tsm_get(&p_gpu_device_tsm));
    // trying to get the shader TSM
    const struct tsm_base_node* p_shader_tsm = NULL;
    CM_SCOPE(CM_RES res = tsm_node_get(p_gpu_device_tsm, &g_sdl3_shader_tsm_key, &p_shader_tsm));
    // if not not found then we need to create it and insert it
    if (res != CM_RES_SUCCESS) {
        CM_ASSERT(res == CM_RES_TSM_NODE_NOT_FOUND);
        // create the TSM
        struct tsm_base_node* p_new_tsm = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_gpu_device_tsm, &g_sdl3_shader_tsm_key, &p_new_tsm));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_gpu_device_tsm, p_new_tsm));
        // create the type inside the TSM
        struct tsm_base_node* p_type = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
            &g_sdl3_shader_type_key,
            sizeof(struct tsm_base_type_node),
            _sdl3_shader_type_free_callback,
            _sdl3_shader_type_is_valid,
            _sdl3_shader_type_print,
            sizeof(struct sdl3_shader),
            &p_type));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_new_tsm, p_type));
        // trying to get the shaderc compiler TSM again
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_gpu_device_tsm, &g_sdl3_shader_tsm_key, &p_shader_tsm));
    }

    // creatin the base shader node
    struct tsm_base_node* p_shader_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(p_key, &g_sdl3_shader_type_key, sizeof(struct sdl3_shader), &p_shader_base));
    struct sdl3_shader* p_shader = caa_container_of(p_shader_base, struct sdl3_shader, base);
    // writing the the shader specific fields in the node
    CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_read_glsl_file(glsl_file_path, p_shader));
    CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_glsl_to_spv(entrypoint, shader_kind, p_shader));
    CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_create_spv_reflect_module(p_shader));
    CM_ASSERT(CM_RES_SUCCESS == _sdl3_shader_create_sdl3_shader(p_shader));
    // inserting into the shader TSM
    CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_shader_tsm, p_shader_base));

    *pp_output_shader = p_shader_base;

    CM_LOG_DEBUG("SUCCESSFULLY created shader\n");
    return CM_RES_SUCCESS;
}
CM_RES sdl3_shader_get(const struct tsm_key* p_key, const struct tsm_base_node** pp_output_shader) {
    CM_ASSERT(p_key);
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    // getting the gpu_device TSM as the shader TSM should be inside it
    const struct tsm_base_node* p_gpu_device_tsm = NULL;
    CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_tsm_get(&p_gpu_device_tsm));
    // trying to get the shader TSM
    const struct tsm_base_node* p_shader_tsm = NULL;
    CM_SCOPE(CM_RES res = tsm_node_get(p_gpu_device_tsm, &g_sdl3_shader_tsm_key, &p_shader_tsm));
    if (res != CM_RES_SUCCESS) {
        return res;
    }
    // getting the shader
    CM_SCOPE(res = tsm_node_get(p_shader_tsm, p_key, pp_output_shader));
    return res;
}
