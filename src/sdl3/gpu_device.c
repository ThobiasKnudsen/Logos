#include "sdl3/gpu_device.h"
#include "sdl3/core.h"
#include "code_monitoring.h"

static const struct tsm_key g_sdl3_gpu_device_type_key 	= { .key_union.string = "sdl3_gpu_device_type", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_sdl3_gpu_device_key 		= { .key_union.string = "sdl3_gpu_device", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_sdl3_gpu_device_tsm_key 	= { .key_union.string = "sdl3_gpu_device_tsm", .key_type = TSM_KEY_TYPE_STRING };
static atomic_bool 			is_initialized 				= ATOMIC_VAR_INIT(false);

struct sdl3_gpu_device {
	struct tsm_base_node 	base;
	SDL_GPUDevice* 			p_gpu_device;
};

static void _sdl3_gpu_device_type_free_callback(struct rcu_head* p_rcu) {
	CM_ASSERT(p_rcu);
	struct tsm_base_node* p_base = caa_container_of(p_rcu, struct tsm_base_node, rcu_head);
	struct sdl3_gpu_device* p_gpu_device = caa_container_of(p_base, struct sdl3_gpu_device, base);
	if (p_gpu_device->p_gpu_device) {
		SDL_DestroyGPUDevice(p_gpu_device->p_gpu_device);
		p_gpu_device->p_gpu_device = NULL;
	} else {
		CM_LOG_WARNING("p_gpu_device->p_gpu_device is NULL\n");
	}
	CM_ASSERT(atomic_load(&is_initialized));
	CM_SCOPE(tsm_base_node_free(p_base));
	atomic_store(&is_initialized, false);
}
static CM_RES _sdl3_gpu_device_type_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
	CM_SCOPE(CM_RES res = tsm_base_node_is_valid(p_tsm_base, p_base));
	CM_ASSERT(atomic_load(&is_initialized));
	return res;
}
static CM_RES _sdl3_gpu_device_type_print(const struct tsm_base_node* p_base) {
	CM_ASSERT(p_base);

	tsm_base_node_print(p_base);
	struct sdl3_gpu_device* p_gpu_device = caa_container_of(p_base, struct sdl3_gpu_device, base);
	CM_ASSERT(p_gpu_device->p_gpu_device);

	CM_LOG_NOTICE("    pointer to gpu device: %p\n", p_gpu_device->p_gpu_device);
	return CM_RES_SUCCESS;
}
CM_RES sdl3_gpu_device_create() {
	CM_ASSERT(!atomic_load(&is_initialized));

	// get the sdl3 core tsm
	const struct tsm_base_node* p_sdl3_tsm_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == sdl3_core_tsm_get(&p_sdl3_tsm_base));

	// create gpu_device type
	struct tsm_base_node* p_new_type_node = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
		&g_sdl3_gpu_device_type_key,
		sizeof(struct tsm_base_type_node),
		_sdl3_gpu_device_type_free_callback,
		_sdl3_gpu_device_type_is_valid,
		_sdl3_gpu_device_type_print,
		sizeof(struct sdl3_gpu_device),
		&p_new_type_node));
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_sdl3_tsm_base, p_new_type_node));

	struct tsm_base_node* p_new_node = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(&g_sdl3_gpu_device_key, &g_sdl3_gpu_device_type_key, sizeof(struct sdl3_gpu_device), &p_new_node));
	struct sdl3_gpu_device* p_gpu_device = caa_container_of(p_new_node, struct sdl3_gpu_device, base);

    #ifdef __linux__
        #ifdef DEBUG
            p_gpu_device->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, true, NULL);
            if (!p_gpu_device->p_gpu_device) {CM_LOG_ERROR("SDL_CreateGPUDevice failed with message: %s\n", SDL_GetError());}
        #else 
            p_gpu_device->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, false, NULL);
            if (!p_gpu_device->p_gpu_device) {CM_LOG_ERROR("SDL_CreateGPUDevice failed with message: %s\n", SDL_GetError());}
        #endif // LCPI_DEBUG
    #elif defined(_WIN64)
        CM_LOG_ERROR("windows 64-bit is not supported yet");
    #elif defined(_WIN32)
        CM_LOG_ERROR("windows 32-bit is not supported yet");
    #elif defined(__CYGWIN__)
        CM_LOG_ERROR("cygwin is not supported yet");
    #elif defined(__APPLE__)
        CM_LOG_ERROR("macos is not supported yet");
    #elif defined(__FreeBSD__)
        CM_LOG_ERROR("free bsd is not supported yet");
    #elif defined(__ANDROID__)
        CM_LOG_ERROR("android is not supported yet");
    #else 
        CM_LOG_ERROR("unrecignized os is not supported");
    #endif

    atomic_store(&is_initialized, true);
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_sdl3_tsm_base, p_new_node));

	struct tsm_base_node* p_gpu_device_tsm_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_sdl3_tsm_base, &g_sdl3_gpu_device_tsm_key, &p_gpu_device_tsm_base));
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_sdl3_tsm_base, p_gpu_device_tsm_base));

	return CM_RES_SUCCESS;
}
CM_RES sdl3_gpu_device_get(SDL_GPUDevice** pp_output_device) {
	CM_ASSERT(pp_output_device);

	const struct tsm_base_node* p_sdl3_tsm_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == sdl3_core_tsm_get(&p_sdl3_tsm_base));

	const struct tsm_base_node* p_gpu_device_base = NULL;
	CM_SCOPE(CM_RES res = tsm_node_get(p_sdl3_tsm_base, &g_sdl3_gpu_device_key, &p_gpu_device_base));
	if (res != CM_RES_SUCCESS) {
		CM_LOG_WARNING("tsm_node_get failed with code %d\n", res);
		return res;
	}
	const struct sdl3_gpu_device* p_gpu_device = caa_container_of(p_gpu_device_base, struct sdl3_gpu_device, base);
	*pp_output_device = p_gpu_device->p_gpu_device;
	
	return CM_RES_SUCCESS;
}
CM_RES sdl3_gpu_device_tsm_get(const struct tsm_base_node** pp_output_tsm_base) {
	CM_ASSERT(pp_output_tsm_base);
	const struct tsm_base_node* p_sdl3_tsm_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == sdl3_core_tsm_get(&p_sdl3_tsm_base));
	CM_SCOPE(CM_RES res = tsm_node_get(p_sdl3_tsm_base, &g_sdl3_gpu_device_tsm_key, pp_output_tsm_base));
	return res;
}
CM_RES sdl3_gpu_device_destroy() {
	rcu_read_lock();
	const struct tsm_base_node* p_core_tsm_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == sdl3_core_tsm_get(&p_core_tsm_base));
	const struct tsm_base_node* p_tsm_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == sdl3_gpu_device_tsm_get(&p_tsm_base));
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_core_tsm_base, p_tsm_base));
	rcu_read_unlock();
	rcu_barrier();rcu_barrier(); // must be twice
	rcu_read_lock();
	CM_ASSERT(CM_RES_SUCCESS == sdl3_core_tsm_get(&p_core_tsm_base));
	const struct tsm_base_node* p_gpu_device_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_core_tsm_base, &g_sdl3_gpu_device_key, &p_gpu_device_base));
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_core_tsm_base, p_gpu_device_base));
	rcu_read_unlock();
	return CM_RES_SUCCESS;
}