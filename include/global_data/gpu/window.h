#ifndef GLOBAL_DATA_GPU_WINDOW_H
#define GLOBAL_DATA_GPU_WINDOW_H

#include "global_data/core.h"
#include <SDL3/SDL.h>
#include <stdbool.h>
#include <stdatomic.h>

typedef struct Window {
    struct gd_base_node     base;
    SDL_Window*             p_sdl_window;
    atomic_bool             atomic_is_in_use;
    atomic_bool             atomic_should_close;
    bool                    device_key_is_number;
    union gd_key            gpu_device_key;
} Window;

void window_type_init();
union gd_key window_create(
    struct gd_key_ctx new_key_ctx,
    struct gd_key_ctx gpu_device_key_ctx,
    int width, int height, const char* title);
void window_show(
    struct gd_key_ctx window_key_ctx,
    struct gd_key_ctx graphics_pipeline_key_ctx);

#endif 