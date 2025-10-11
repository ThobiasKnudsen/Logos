#include "sdl3/core.h"
#include "code_monitoring.h"
#include <SDL3/SDL.h>
#include <SDL3_shadercross/SDL_shadercross.h>
#include <stdatomic.h>

static const struct tsm_key g_sdl3_core_type_key 	= { .key_union.string = "sdl3_core_type", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_sdl3_core_tsm_key 	= { .key_union.string = "sdl3_core_tsm", .key_type = TSM_KEY_TYPE_STRING };
static struct tsm_path 		g_sdl3_core_path 		= {0};
static struct tsm_key 		g_sdl3_core_key 		= {0};
static atomic_bool 			is_initialized 			= ATOMIC_VAR_INIT(false);

struct sdl3_core {
	struct tsm_base_node  base;
	bool  				  is_initialized;
};

static void _sdl3_core_type_free_callback(struct rcu_head* p_rcu) {
	CM_ASSERT(p_rcu);
	struct tsm_base_node* p_base = caa_container_of(p_rcu, struct tsm_base_node, rcu_head);
	struct sdl3_core* p_core = caa_container_of(p_base, struct sdl3_core, base);
	CM_ASSERT(p_core->is_initialized != false);
	p_core->is_initialized = false;
	bool fetched_is_initialized = atomic_load(&is_initialized);
	CM_ASSERT(fetched_is_initialized);
	SDL_Quit();
	SDL_ShaderCross_Quit();
	CM_SCOPE(tsm_base_node_free(p_base));
	CM_SCOPE(tsm_path_free(&g_sdl3_core_path));
	atomic_store(&is_initialized, false);
	CM_LOG_NOTICE("_sdl3_core_type_free_callback IS NODE\n");
}
static CM_RES _sdl3_core_type_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
	CM_SCOPE(CM_RES res = tsm_base_node_is_valid(p_tsm_base, p_base));
	if (res != CM_RES_TSM_NODE_IS_VALID) {
		return res;
	}
	const struct sdl3_core* p_core = caa_container_of(p_base, struct sdl3_core, base);
	CM_ASSERT(p_core->is_initialized);
	bool fetched_is_initialized = atomic_load(&is_initialized);
	CM_ASSERT(fetched_is_initialized);
	return CM_RES_TSM_NODE_IS_VALID;
}
static CM_RES _sdl3_core_type_print(const struct tsm_base_node* p_base) {
	CM_ASSERT(p_base);
	tsm_base_node_print(p_base);
	const struct sdl3_core* p_sdl3_core = caa_container_of(p_base, const struct sdl3_core, base);
	bool fetched_is_initialized = atomic_load(&is_initialized);
	CM_ASSERT(fetched_is_initialized);
	CM_LOG_TSM_PRINT("    is_initialized: %p\n", p_sdl3_core->is_initialized);
	return CM_RES_SUCCESS;
}

CM_RES sdl3_core_init(const struct tsm_base_node* p_tsm_base, const struct tsm_key* p_key) {

	CM_ASSERT(p_tsm_base && p_key);
	CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));
	CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));

	CM_ASSERT(!atomic_load(&is_initialized)) // sdl3_core cannot be initialized
	atomic_store(&is_initialized, true); // sets to true here so that g_sdl3_core_path can be accessed securely

	CM_ASSERT(CM_RES_SUCCESS == tsm_copy_path(p_tsm_base, &g_sdl3_core_path)); // create the new path for sdl3_core
	CM_ASSERT(CM_RES_SUCCESS == tsm_path_insert_key(&g_sdl3_core_path, p_key, -1)); // insert new key into new path 

	// create and insert the sdl3_core type node
	struct tsm_base_node* p_new_type_node = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
		&g_sdl3_core_type_key,
		sizeof(struct tsm_base_type_node),
		_sdl3_core_type_free_callback,
		_sdl3_core_type_is_valid,
		_sdl3_core_type_print,
		sizeof(struct sdl3_core),
		&p_new_type_node));
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_tsm_base, p_new_type_node));

	struct tsm_base_node* p_new_base_node = NULL; // create and insert the sdl3_core node
	CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(p_key, &g_sdl3_core_type_key, sizeof(struct sdl3_core), &p_new_base_node));
	struct sdl3_core* p_core = caa_container_of(p_new_base_node, struct sdl3_core, base);
	p_core->is_initialized = true;
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_tsm_base, p_new_base_node));

    CM_ASSERT(SDL_Init(SDL_INIT_VIDEO));
    CM_ASSERT(SDL_ShaderCross_Init());

	// create sdl3_tsm
	struct tsm_base_node* p_new_tsm_node = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_tsm_base, &g_sdl3_core_tsm_key, &p_new_tsm_node));
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_insert(p_tsm_base, p_new_tsm_node));
	
    return CM_RES_SUCCESS;
}
CM_RES sdl3_core_tsm_get(const struct tsm_base_node** pp_output_tsm_node) {
	CM_ASSERT(pp_output_tsm_node);

	const struct tsm_base_node* p_tsm_parent_base = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_get_by_path_at_depth(gtsm_get(), &g_sdl3_core_path, -2, &p_tsm_parent_base));
	const struct tsm_base_node* p_core_tsm = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_tsm_parent_base, &g_sdl3_core_tsm_key, &p_core_tsm));

	*pp_output_tsm_node = p_core_tsm;
	return CM_RES_SUCCESS;
}
CM_RES sdl3_core_quit() {
	CM_ASSERT(atomic_load(&is_initialized)); // sdl3_core must be initialized

	const struct tsm_base_node* p_tsm_base = NULL;
	CM_SCOPE(CM_RES_SUCCESS == tsm_node_get_by_path_at_depth(gtsm_get(), &g_sdl3_core_path, -2, &p_tsm_base));

	// get the three nodes to defer free
	const struct tsm_base_node* p_base_node_type = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_tsm_base, &g_sdl3_core_type_key, &p_base_node_type));
	const struct tsm_base_node* p_base_node_tsm = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_tsm_base, &g_sdl3_core_tsm_key, &p_base_node_tsm));
	const struct tsm_base_node* p_base_node = NULL;
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_get_by_path(gtsm_get(), &g_sdl3_core_path, &p_base_node));

	// defer free the three nodes
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm_base, p_base_node_tsm));
	// The freeing of this node (sdl3 core node) will ensure SDL and shadercross quitting
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm_base, p_base_node));
	CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm_base, p_base_node_type));

	return CM_RES_SUCCESS;
}