#ifndef GPU_GPU_DEVICE_h
#define GPU_GPU_DEVICE_h

#include "tsm.h"
#include <SDL3/SDL_gpu.h>


/**
 * This is a SINGLETON module. Only one should exist at the time. 
 * But dont worry. You will not be able to create multiple instances 
 * of this module.
 */
CM_RES sdl3_gpu_device_create();
CM_RES sdl3_gpu_device_get(SDL_GPUDevice** pp_output_device);
CM_RES sdl3_gpu_device_tsm_get(const struct tsm_base_node** pp_output_tsm_base);
CM_RES sdl3_gpu_device_destroy();

#endif //GPU_GPU_DEVICE_h