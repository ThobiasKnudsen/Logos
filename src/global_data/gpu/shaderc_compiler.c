#include "global_data/gpu/shaderc_compiler.h"
#include <SDL3/SDL_gpu.h>
#include <shaderc/shaderc.h>
#include "tklog.h"
static const char* g_shaderc_compiler_type_key = "ShadercCompiler";
static const bool g_shaderc_compiler_type_key_is_number = false;
typedef struct ShadercCompiler {
    struct gd_base_node base;
    SDL_ThreadID thread_id;
    shaderc_compiler_t shaderc_compiler;
    shaderc_compile_options_t shaderc_options;
} ShadercCompiler;
bool shaderc_compiler_free(struct gd_base_node* node) {
    ShadercCompiler* p = caa_container_of(node, ShadercCompiler, base);
    shaderc_compiler_release(p->shaderc_compiler);
    shaderc_compile_options_release(p->shaderc_options);
    return gd_base_node_free(node);
}
void shaderc_compiler_free_callback(struct rcu_head* head) {
    struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
    shaderc_compiler_free(node);
}
bool shaderc_compiler_is_valid(struct gd_base_node* node) {
    ShadercCompiler* p = caa_container_of(node, ShadercCompiler, base);
    return p->shaderc_compiler != NULL && p->shaderc_options != NULL;
}
void shaderc_compiler_type_init() {
    gd_create_node_type(g_shaderc_compiler_type_key, sizeof(ShadercCompiler), shaderc_compiler_free, shaderc_compiler_free_callback, shaderc_compiler_is_valid);
}
union gd_key shaderc_compiler_get(SDL_ThreadID thread_id) {
    union gd_key key = gd_key_create(thread_id, NULL, true);
    rcu_read_lock();
    struct gd_base_node* base = gd_node_get(key, true);
    rcu_read_unlock();
    if (base) return key;
    base = gd_base_node_create(key, true, gd_key_create(0, g_shaderc_compiler_type_key, false), false, sizeof(ShadercCompiler));
    if (!base) return gd_key_create(0, NULL, true);
    ShadercCompiler* p = caa_container_of(base, ShadercCompiler, base);
    p->thread_id = thread_id;
    p->shaderc_compiler = shaderc_compiler_initialize();
    if (!p->shaderc_compiler) {
        tklog_critical("failed to initialize shaderc compiler");
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    }
    p->shaderc_options = shaderc_compile_options_initialize();
    if (!p->shaderc_options) {
        tklog_critical("failed to initialize shaderc options");
        shaderc_compiler_release(p->shaderc_compiler);
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    }
    shaderc_compile_options_set_optimization_level(p->shaderc_options, shaderc_optimization_level_zero);
    #ifdef __linux__
        shaderc_compile_options_set_target_env(p->shaderc_options, shaderc_target_env_vulkan, shaderc_env_version_vulkan_1_0);
    #else
        tklog_error("OS not supported yet");
        shaderc_compiler_release(p->shaderc_compiler);
        shaderc_compile_options_release(p->shaderc_options);
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    #endif
    if (!gd_node_insert(base)) {
        shaderc_compiler_release(p->shaderc_compiler);
        shaderc_compile_options_release(p->shaderc_options);
        gd_base_node_free(base);
        return gd_key_create(0, NULL, true);
    }
    printf("SUCCESSFULLY created shaderc compiler\n");
    return key;
}
void shaderc_compiler_destroy(union gd_key key, bool is_number) {
    gd_node_remove(key, is_number);
}
