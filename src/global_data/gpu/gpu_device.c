#include "global_data/gpu/gpu_device.h"
#include <SDL3/SDL_gpu.h>
#include "tklog.h"

static const char* g_gpu_device_type_key = "GPUDevice";
static const bool g_gpu_device_type_key_is_number = false;
bool gpu_device_free(struct gd_base_node* node) {
    GPUDevice* p = caa_container_of(node, GPUDevice, base);
    if (p->p_gpu_device) SDL_DestroyGPUDevice(p->p_gpu_device);
    tklog_scope(bool result = gd_base_node_free(node));
    return result;
}
void gpu_device_free_callback(struct rcu_head* head) {
    struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
    tklog_scope(bool result = gpu_device_free(node));
    if (!result) tklog_error("gpu_device_free failed");
}
bool gpu_device_is_valid(struct gd_base_node* node) {
    GPUDevice* p = caa_container_of(node, GPUDevice, base);
    return p->p_gpu_device != NULL;
}
void gpu_device_type_init() {
    tklog_scope(gd_create_node_type(g_gpu_device_type_key, sizeof(GPUDevice), gpu_device_free, gpu_device_free_callback, gpu_device_is_valid));
}
union gd_key gpu_device_create() {
    tklog_scope(
        struct gd_base_node* base = gd_base_node_create(
            gd_key_create(0, NULL, true), true, 
            gd_key_create(0, g_gpu_device_type_key, false), false, 
            sizeof(GPUDevice))
    );
    if (!base) {
        tklog_error("Failed to create GPU device: %s", SDL_GetError());
        return gd_key_create(0, NULL, true);
    }
    GPUDevice* p = caa_container_of(base, GPUDevice, base);
    #ifdef __linux__
        #ifdef DEBUG
            p->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, true, NULL);
        #else
            p->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, false, NULL);
        #endif
    #else
        tklog_error("OS not supported yet");
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    #endif
    if (!p->p_gpu_device) {
        tklog_error("Failed to create GPU device: %s", SDL_GetError());
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    }
    rcu_read_lock();
    if (!gd_node_insert(base)) {
        SDL_DestroyGPUDevice(p->p_gpu_device);
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    }
    rcu_read_unlock();
    tklog_info("SUCCESSFULLY created gpu device");
    return base->key;
}
void gpu_device_destroy(union gd_key key, bool is_number) {
    gd_node_remove(key, is_number);
}
