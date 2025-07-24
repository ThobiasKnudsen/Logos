#include "global_data/gpu/gpu_device.h"
#include <SDL3/SDL_gpu.h>
#include "tklog.h"

static const char* g_gpu_device_type_key = "GPUDevice";
static const bool g_gpu_device_type_key_is_number = false;
static const struct gd_key_ctx g_gpu_device_type_key_ctx = { .key = { .string = (char*)g_gpu_device_type_key }, .key_is_number = g_gpu_device_type_key_is_number };

struct GPUDevice {
    struct gd_base_node base;
    SDL_GPUDevice* p_gpu_device;
};

bool gpu_device_free(struct gd_base_node* node) {
    GPUDevice* p = caa_container_of(node, GPUDevice, base);
    if (p->p_gpu_device) {
        SDL_DestroyGPUDevice(p->p_gpu_device);
    } else {
        tklog_notice("p->p_gpu_device is NULL\n");
    }
    tklog_scope(bool result = gd_base_node_free(node));
    return result;
}

void gpu_device_free_callback(struct rcu_head* head) {
    struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
    tklog_scope(bool result = gpu_device_free(node));
    if (!result) {
        tklog_error("gpu_device_free failed");
    }
}

bool gpu_device_is_valid(struct gd_base_node* node) {
    GPUDevice* p = caa_container_of(node, GPUDevice, base);
    return p->p_gpu_device != NULL;
}

bool gpu_device_print_info(struct gd_base_node* p_base) {

    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }

    if (!gd_base_node_print_info(p_base)) {
        tklog_error("failed to print base node\n");
        return false;
    }

    struct GPUDevice* p_gpu_device = caa_container_of(p_base, struct GPUDevice, base);

    tklog_info("GPUDevice:\n");
    tklog_info("    p_gpu_device: %p\n", p_gpu_device->p_gpu_device);

    return true;
}

void gpu_device_type_init() {
    tklog_scope(struct gd_key_ctx type_key_ctx = gd_key_ctx_create(0, g_gpu_device_type_key, false));
    tklog_scope(struct gd_base_node* p_base_node = gd_base_node_create(
        type_key_ctx, 
        gd_base_type_key_ctx_copy(), 
        sizeof(struct gd_base_type_node)));
    struct gd_base_type_node* p_type_node = caa_container_of(p_base_node, struct gd_base_type_node, base);
    p_type_node->fn_free_node = gpu_device_free;
    p_type_node->fn_free_node_callback = gpu_device_free_callback;
    p_type_node->fn_is_valid = gpu_device_is_valid;
    p_type_node->fn_print_info = gpu_device_print_info;
    p_type_node->type_size = sizeof(GPUDevice);
    tklog_scope(gd_node_insert(&p_type_node->base));
    tklog_scope(gd_key_ctx_free(&type_key_ctx));
}

struct gd_key_ctx gpu_device_create(struct gd_key_ctx key_ctx) {
    tklog_scope(
        struct gd_base_node* base = gd_base_node_create(
            key_ctx, 
            g_gpu_device_type_key_ctx, 
            sizeof(GPUDevice))
    );
    if (!base) {
        tklog_error("Failed to create GPU device: %s", SDL_GetError());
        return gd_key_ctx_create(0, NULL, true);
    }
    GPUDevice* p = caa_container_of(base, GPUDevice, base);
    #ifdef __linux__
        #ifdef DEBUG
            p->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, true, NULL);
        #else
            p->p_gpu_device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, false, NULL);
        #endif
        if (!p->p_gpu_device) {
            tklog_error("Failed to create GPU device: %s", SDL_GetError());
            gpu_device_free(base);
            return gd_key_ctx_create(0, NULL, true);
        } 
        tklog_info("GPU Backend: %s", SDL_GetGPUDeviceDriver(p->p_gpu_device));
    #elif defined(_WIN64)
        tklog_error("windows 64-bit is not supported yet");
        return gd_key_ctx_create(0, NULL, true);
    #elif defined(_WIN32)
        tklog_error("windows 32-bit is not supported yet");
        return gd_key_ctx_create(0, NULL, true);
    #elif defined(__CYGWIN__)
        tklog_error("cygwin is not supported yet");
        return gd_key_ctx_create(0, NULL, true);
    #elif defined(__APPLE__)
        tklog_error("macos is not supported yet");
        return gd_key_ctx_create(0, NULL, true);
    #elif defined(__FreeBSD__)
        tklog_error("free bsd is not supported yet");
        return gd_key_ctx_create(0, NULL, true);
    #elif defined(__ANDROID__)
        tklog_error("android is not supported yet");
        return gd_key_ctx_create(0, NULL, true);
    #else 
        tklog_error("unrecignized os is not supported");
        return gd_key_ctx_create(0, NULL, true);
    #endif
    
    if (!p->p_gpu_device) {
        tklog_error("Failed to create GPU device: %s", SDL_GetError());
        gpu_device_free(base);
        return gd_key_ctx_create(0, NULL, true);
    }
    rcu_read_lock();
    if (!gd_node_insert(base)) {
        SDL_DestroyGPUDevice(p->p_gpu_device);
        gpu_device_free(base);
        rcu_read_unlock();
        return gd_key_ctx_create(0, NULL, true);
    }
    rcu_read_unlock();
    tklog_info("SUCCESSFULLY created gpu device");
    return (struct gd_key_ctx){ .key = base->key, .key_is_number = base->key_is_number };
}

void gpu_device_destroy(struct gd_key_ctx key_ctx) {
    gd_node_remove(key_ctx);
}