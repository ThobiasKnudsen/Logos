#ifndef GPU_DEVICE_H
#define GPU_DEVICE_H

#include "global_data/core.h"
#include <SDL3/SDL.h>

typedef struct GPUDevice {
    struct gd_base_node base;
    SDL_GPUDevice* p_gpu_device;
} GPUDevice;

void gpu_device_type_init();
union gd_key gpu_device_create();
void gpu_device_destroy(union gd_key key, bool is_number);

#endif