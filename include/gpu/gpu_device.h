#ifndef GPU_GPU_DEVICE_h
#define GPU_GPU_DEVICE_h

#include "tsm.h"
#include <SDL3/SDL_gpu.h>

static struct tsm_path gpu_devices_tsm_path = {0};

struct gpu_device {
	struct tsm_base_node 	base;
	SDL_GPU_Device* 		p_gpu_device;
}

bool gpu_device_tsm_create(struct tsm_key key_tsm);
bool gpu_device_tsm_is_created();
struct tsm_base_node* gpu_device_tsm_get();
struct tsm_path gpu_device_tsm_get_path();
bool gpu_device_tsm_delete();

bool gpu_device_create(struct tsm_key key, );
struct tsm_base_node* gpu_device_get(struct tsm_key key);

#endif //GPU_GPU_DEVICE_h