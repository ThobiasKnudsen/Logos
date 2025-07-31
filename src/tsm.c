#include "tsm.h"
#include "tklog.h"
#include "xxhash.h"

#include <stdatomic.h>
#include <pthread.h>
#include <unistd.h>


// ==========================================================================================
// PUBLIC
// ==========================================================================================

#define MAX_STRING_KEY_LEN 64

static _Atomic uint64_t g_key_counter = 1; // 0 is invalid. 1 is the first valid key
static atomic_bool tsm_is_initialized = false;

static const struct tsm_key_ctx base_type_key_ctx = { .key.string = "base_type", .key_is_number = false };
static const struct tsm_key_ctx tsm_type_key_ctx  = { .key.string = "tsm_type",  .key_is_number = false };

static int _tsm_key_match(struct cds_lfht_node *node, const void *key_ctx) {
    struct tsm_base_node *base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    struct tsm_key_ctx ctx_1 = { .key = base_node->key, .key_is_number = base_node->key_is_number };
    struct tsm_key_ctx* p_ctx_2 = (struct tsm_key_ctx *)key_ctx;
    return tsm_key_ctx_match(ctx_1, *p_ctx_2);
}
static uint64_t _tsm_hash_key(union tsm_key key, bool key_is_number) {
    if (key_is_number) {
        return XXH3_64bits(&key.number, sizeof(uint64_t));
    } else {
        return XXH3_64bits(key.string, strlen(key.string));
    }
}
static uint64_t _tsm_get_node_size(struct cds_lfht_node* node) {
    if (!node) {
        tklog_error("_tsm_get_node_size called with NULL node\n");
        return 0;
    }
    struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    return base_node->this_size_bytes;
}
static void* _tsm_get_node_start_ptr(struct cds_lfht_node* node) {
    if (!node) {
        tklog_error("_tsm_get_node_start_ptr called with NULL node\n");
        return NULL;
    }
    struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    return (void*)base_node;
}

static bool _tsm_base_type_node_free(struct tsm_base_node* node) {
    if (!node) {
        tklog_error("given tsm_base_node arguemnt is NULL\n");
        return false;
    }
    tklog_scope(bool free_result = tsm_base_node_free(node));
    if (!free_result) {
        tklog_error("failed to free\n");
        return false;
    }
    return true;
}
static void _tsm_base_type_node_free_callback(struct rcu_head* rcu_head) {
    struct tsm_base_node* node = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);
    _tsm_base_type_node_free(node);
}
static bool _tsm_base_type_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    if (!p_base) {
        tklog_debug("p_base is NULL\n");
        return false;
    }
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("p_tsm_base is not TSM\n");
        return false;
    }

    tklog_scope(bool is_valid = tsm_base_node_is_valid(p_tsm_base, p_base));
    if (!is_valid) {
        tklog_info("base is not valid\n");
        return false;
    }

    // Basic validation - check if it's a node type node
    struct tsm_base_type_node* p_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    if (!p_type->fn_free) {
        tklog_debug("p_type->fn_free is NULL\n");
        return false;
    }
    if (!p_type->fn_free_callback) {
        tklog_debug("p_type->fn_free_callback is NULL\n");
        return false;
    }
    if (!p_type->fn_is_valid) {
        tklog_debug("p_type->fn_is_valid is NULL\n");
        return false;
    }
    if (!p_type->fn_print_info) {
        tklog_debug("p_type->fn_print_info is NULL\n");
        return false;
    }
    if (p_type->type_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_debug("node_type->type_size_bytes(%d) < sizeof(struct tsm_base_node)(%d)\n", node_type->type_size_bytes, sizeof(struct tsm_base_node));
        return false;
    }

    return true;
}
static bool _tsm_base_type_node_print_info(struct tsm_base_node* p_base) {

    if (!tsm_base_node_print_info(p_base)) {
        tklog_error("failed to print base node\n");
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_base, struct tsm_base_type_node, base);

    tklog_info("tsm_base_type_node:\n");
    tklog_info("    fn_free: %p\n", p_type_node->fn_free);
    tklog_info("    fn_free_callback: %p\n", p_type_node->fn_free_callback);
    tklog_info("    fn_is_valid: %p\n", p_type_node->fn_is_valid);
    tklog_info("    fn_print_info: %p\n", p_type_node->fn_print_info);
    tklog_info("    size of node this is type for in bytes: %d\n", p_type_node->type_size_bytes);

    return true;
}


static bool _tsm_tsm_type_free(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_warning("p_base is NULL\n");
        return false;
    }
    tklog_scope(uint32_t nodes_count = tsm_nodes_count(p_base));
    if (nodes_count > 0) {
        tklog_error("tsm is not empty before freeing it");
        return false;
    }
    tsm_key_free(p_base->key, p_base->key_is_number);
    tsm_key_free(p_base->type_key, p_base->type_key_is_number);

    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    cds_lfht_destroy(p_tsm->p_ht, NULL);

    for (unsigned int i = 0; i < p_tsm->path_length; ++i) {
        tsm_key_ctx_free(&p_tsm->path_from_global_to_parent_tsm[i]);
    }
    free(p_tsm->path_from_global_to_parent_tsm);
    free(p_base);
    return true;
}
static void _tsm_tsm_type_free_callback(struct rcu_head* rcu_head) {
    struct tsm_base_node* node = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);
    tklog_scope(bool free_result = _tsm_tsm_type_free(node));
    if (!free_result) {
        tklog_error("failed to free TSM\n");
    }
}
static bool _tsm_tsm_type_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    (void)p_tsm_base;

    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_base));
    if (!is_tsm) {
        tklog_error("p_base is not a TSM\n");
        return false;
    }

    struct cds_lfht_iter* p_iter = NULL;
    tklog_scope(tsm_iter_first(p_base, p_iter));
    bool iter_next = p_iter != NULL;
    while (iter_next) {
        tklog_scope(struct tsm_base_node* iter_node = tsm_iter_get_node(p_iter));
        tklog_scope(bool is_valid = tsm_base_node_is_valid(p_base, iter_node));
        if (!is_valid) {
            tklog_info("This node is not valid:\n");
            tklog_scope(tsm_node_print_info(p_base, iter_node));
            return false;
        }
        tklog_scope(iter_next = tsm_iter_next(p_base, p_iter));
    }

    return true;
}
static bool _tsm_tsm_type_print_info(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_base));
    if (!is_tsm) {
        tklog_error("p_base is not a TSM\n");
        return false;
    }

    tklog_scope(tsm_base_node_print_info(p_base));
    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    tklog_info("path to parent from global TSM:\n");
    for (uint32_t i = 0; i < p_tsm->path_length; i++) {
        if (p_tsm->path_from_global_to_parent_tsm[i].key_is_number) {
            tklog_info("%lld -> ", p_tsm->path_from_global_to_parent_tsm[i].key.number);
        } else {
            tklog_info("%s -> ", p_tsm->path_from_global_to_parent_tsm[i].key.string);
        }
    }

    struct cds_lfht_iter* p_iter = NULL;
    tklog_scope(tsm_iter_first(p_base, p_iter));
    bool iter_next = p_iter != NULL;
    while (iter_next) {
        tklog_scope(struct tsm_base_node* iter_node = tsm_iter_get_node(p_iter));
        tklog_scope(tsm_node_print_info(p_base, iter_node));
        tklog_scope(iter_next = tsm_iter_next(p_base, p_iter));
    }

    return true;
}

// Type tracking structure for cleanup
struct _type_tracking_tsm_clean_key {
    union tsm_key key;
    bool key_is_number;
};
// Hash function for type tracking
static uint64_t _type_tracking_tsm_clean_hash(struct _type_tracking_tsm_clean_key tk) {
    return _tsm_hash_key(tk.key, tk.key_is_number);
}
// Comparison function for type tracking
static int _type_tracking_tsm_clean_cmpr(struct _type_tracking_tsm_clean_key a, struct _type_tracking_tsm_clean_key b) {
    if (a.key_is_number != b.key_is_number) {
        return a.key_is_number ? 1 : -1;
    }
    
    if (a.key_is_number) {
        return (a.key.number > b.key.number) - (a.key.number < b.key.number);
    } else {
        return strcmp(a.key.string, b.key.string);
    }
}
#define NAME _type_tracking_tsm_clean_set
#define KEY_TY struct _type_tracking_tsm_clean_key
#define HASH_FN _type_tracking_tsm_clean_hash
#define CMPR_FN _type_tracking_tsm_clean_cmpr
#include "verstable.h"


// ==========================================================================================
// PUBLIC
// ==========================================================================================
union tsm_key tsm_key_create(uint64_t number_key, const char* string_key, bool key_is_number) {
    if (key_is_number && number_key == 0) {
        tklog_error("number key is 0\n");
        return (union tsm_key){ .number = 0 };
    }
    if (!key_is_number && string_key == NULL) {
        tklog_error("string key is NULL\n");
        return (union tsm_key){ .string = NULL };
    }
    union tsm_key key;
    if (key_is_number) {
        key.number = number_key;
    } else {
        // Validate and copy string key
        if (strlen(string_key) == 0) {
            tklog_error("string key is empty\n");
            return (union tsm_key){ .string = NULL };
        }
        
        size_t key_len = strlen(string_key) + 1;
        if (key_len > MAX_STRING_KEY_LEN) {
            tklog_error("string key too long (max %d characters)\n", MAX_STRING_KEY_LEN - 1);
            return (union tsm_key){ .string = NULL };
        }
        
        char* copied_string = calloc(1, key_len);
        if (!copied_string) {
            tklog_error("Failed to allocate memory for string key\n");
            return (union tsm_key){ .string = NULL };
        }
        
        if (strncpy(copied_string, string_key, key_len - 1) == NULL) {
            tklog_error("Failed to copy string key\n");
            free(copied_string);
            return (union tsm_key){ .string = NULL };
        }
        copied_string[key_len - 1] = '\0'; // Ensure null termination
        
        key.string = copied_string;
    }
    return key;
}
bool tsm_key_free(union tsm_key key, bool key_is_number) {
    if (key.number == 0) {
        tklog_error("number key is 0\n");
        return false;
    }
    if (key_is_number) {
        return true;
    } else {
        if (key.string) {
            free(key.string);
            return true;
        }
    }
    return false;
}
struct tsm_key_ctx tsm_key_ctx_create(uint64_t number_key, const char* string_key, bool key_is_number) {
    struct tsm_key_ctx return_value = {0};
    tklog_scope(return_value.key = tsm_key_create(number_key, string_key, key_is_number));
    return_value.key_is_number = key_is_number;
    return return_value;
}
struct tsm_key_ctx tsm_key_ctx_copy(struct tsm_key_ctx key_ctx) {
    tklog_scope(struct tsm_key_ctx new_key_ctx = tsm_key_ctx_create(key_ctx.key.number, key_ctx.key.string, key_ctx.key_is_number));
    return new_key_ctx;
}
bool tsm_key_ctx_free(struct tsm_key_ctx* key_ctx) {
    if (!key_ctx) {
        tklog_warning("key_ctx is NULL\n");
        return false;
    }
    tklog_scope(bool free_result = tsm_key_free(key_ctx->key, key_ctx->key_is_number));
    if (!free_result) {
        tklog_scope("failed to free node\n");
        return false;
    }
    key_ctx->key.number = 0;
    key_ctx->key_is_number = true;
    return true;
}
bool tsm_key_ctx_match(struct tsm_key_ctx key_ctx_1, struct tsm_key_ctx key_ctx_2) {
    // if not both is string or both is number
    if (key_ctx_1.key_is_number != key_ctx_2.key_is_number)
        return false;
    // if both are number
    if (key_ctx_1.key_is_number) 
        return key_ctx_1.key.number == key_ctx_2.key.number;
    // if both are string
    else 
        return strcmp(key_ctx_1.key.string, key_ctx_2.key.string) == 0;
}


bool tsm_init(void) {

    if (atomic_load(&tsm_is_initialized)) {
        tklog_error("tsm is already initialized\n");
        return false;
    }
    // Register the node size function with urcu_safe  system
    urcu_safe_set_node_size_function(_tsm_get_node_size);
    // Register the node bounds functions with urcu_safe system
    urcu_safe_set_node_start_ptr_function(_tsm_get_node_start_ptr);

    // set initialized to true
    atomic_store(&tsm_is_initialized, true);

    return true;
}
bool tsm_clean(struct tsm_base_node* p_tsm_base) {

    if (!p_tsm_base) {
        tklog_warning("Global data system not initialized\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!p_tsm_base) {
        tklog_warning("given tsm is not a TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Create a Verstable set to track which nodes are used as types
    _type_tracking_tsm_clean_set used_as_type_set;
    _type_tracking_tsm_clean_set_init(&used_as_type_set);
    
    // Iteratively remove nodes layer by layer
    const int batch_size = 1000;
    struct cds_lfht_node* nodes_to_delete[batch_size];
    int layer = 0;
    
    while (true) {
        int total_nodes = 0;
        int delete_count = 0;
        struct cds_lfht_iter iter;
        
        // Clear the used_as_type tracking set
        _type_tracking_tsm_clean_set_clear(&used_as_type_set);
        
        // Phase 1: Count nodes and identify which are used as types
        rcu_read_lock();
        cds_lfht_first(p_tsm->p_ht, &iter);
        struct cds_lfht_node* node;
        
        while ((node = cds_lfht_iter_get_node(&iter)) != NULL) {
            struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
            total_nodes++;
            
            // Add this node's type to the used_as_type set
            struct _type_tracking_tsm_clean_key track_key;
            track_key.key = base_node->type_key;
            track_key.key_is_number = base_node->type_key_is_number;
            
            _type_tracking_tsm_clean_set_itr itr = _type_tracking_tsm_clean_set_insert(&used_as_type_set, track_key);
            if (_type_tracking_tsm_clean_set_is_end(itr)) {
                tklog_error("Out of memory tracking used types\n");
                // Continue anyway, will force cleanup
            }
            
            cds_lfht_next(p_tsm->p_ht, &iter);
        }
        
        // If no nodes left, we're done
        if (total_nodes == 0) {
            rcu_read_unlock();
            break;
        }
        
        // Phase 2: Collect nodes that are not used as types
        cds_lfht_first(p_tsm->p_ht, &iter);
        while ((node = cds_lfht_iter_get_node(&iter)) != NULL && delete_count < batch_size) {
            struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
            
            // Check if this node is used as a type
            struct _type_tracking_tsm_clean_key lookup_key;
            lookup_key.key = base_node->key;
            lookup_key.key_is_number = base_node->key_is_number;
            
            _type_tracking_tsm_clean_set_itr found = _type_tracking_tsm_clean_set_get(&used_as_type_set, lookup_key);
            
            if (_type_tracking_tsm_clean_set_is_end(found)) {
                // This node is not used as a type, mark for deletion
                nodes_to_delete[delete_count++] = node;
            }
            
            cds_lfht_next(p_tsm->p_ht, &iter);
        }
        
        // If no nodes to delete in this layer, force cleanup of remaining nodes
        if (delete_count == 0 && total_nodes > 0) {
            tklog_warning("No progress made, forcing cleanup of remaining %d nodes\n", total_nodes);
            cds_lfht_first(p_tsm->p_ht, &iter);
            while ((node = cds_lfht_iter_get_node(&iter)) != NULL && delete_count < batch_size) {
                nodes_to_delete[delete_count++] = node;
                cds_lfht_next(p_tsm->p_ht, &iter);
            }
        }
        
        // Phase 3: Delete collected nodes
        for (int i = 0; i < delete_count; i++) {
            struct tsm_base_node* base_node = caa_container_of(nodes_to_delete[i], struct tsm_base_node, lfht_node);
            
            // Remove from hash table
            if (cds_lfht_del(p_tsm->p_ht, nodes_to_delete[i]) == 0) {
                // Get the type node to find the correct free callback
                struct tsm_key_ctx type_key_ctx = { .key = base_node->type_key, .key_is_number = base_node->type_key_is_number };
                tklog_scope(struct tsm_base_node* type_base = tsm_node_get(p_tsm_base, type_key_ctx));
                
                if (type_base) {
                    struct tsm_base_type_node* type_node = caa_container_of(type_base, struct tsm_base_type_node, base);
                    void (*free_callback)(struct rcu_head*) = rcu_dereference(type_node->fn_free_callback);
                    call_rcu(&base_node->rcu_head, free_callback);
                } else {
                    // Fallback to default callback if type not found
                    call_rcu(&base_node->rcu_head, _tsm_base_type_node_free_callback);
                }
            } else {
                tklog_warning("Failed to delete node during cleanup\n");
            }
        }
        
        rcu_read_unlock();
        
        if (delete_count > 0) {
            tklog_info("Layer %d: Cleaned up %d nodes\n", layer, delete_count);
            layer++;
        }
        
        // The RCU barrier thread will handle waiting for call_rcu callbacks
        // No need to call rcu_barrier() here as it's handled by the dedicated thread
    }
    
    // Clean up tracking set
    _type_tracking_tsm_clean_set_cleanup(&used_as_type_set);

    rcu_barrier();
    
    tklog_info("Global data system cleanup completed (%d layers)\n", layer);
    return true;
}
struct tsm_base_node* tsm_create_and_insert(
    struct tsm_base_node* p_tsm_base,
    struct tsm_key_ctx tsm_key_ctx) {

    // validating TSM
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return NULL;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!p_tsm_base) {
        tklog_warning("given tsm is not a TSM\n");
        return NULL;
    }
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    // check if tsm_type exists in given TSM
    tklog_scope(struct tsm_base_node* p_tsm_type_base = tsm_node_get(p_tsm_base, tsm_type_key_ctx));
    if (!p_tsm_type_base) {
        tklog_error("did not find tsm_type in given TSM\n");
        return NULL;
    }

    // create the new tsm node
    tklog_scope(struct tsm_base_node* p_new_tsm_base = tsm_base_node_create(tsm_key_ctx, tsm_type_key_ctx, sizeof(struct tsm), false));
    if (!p_new_tsm_base) {
        tklog_error("Failed to create base node for new TSM\n");
        return NULL;
    }
    struct tsm* p_new_tsm = caa_container_of(p_new_tsm_base, struct tsm, base);
    p_new_tsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    if (!p_new_tsm->p_ht) {
        tklog_error("Failed to create lock free hash table\n");
        return NULL;
    }
    p_new_tsm->path_length = p_tsm->path_length + 1;
    p_new_tsm->path_from_global_to_parent_tsm = calloc(1, p_new_tsm->path_length * sizeof(tsm_key_ctx));
    if (!p_new_tsm->path_from_global_to_parent_tsm) {
        tklog_error("calloc failed\n");
        cds_lfht_destroy(p_new_tsm->p_ht, NULL);
        return NULL;
    }
    // add path of parent TSM
    for (uint32_t i = 0; i < p_new_tsm->path_length-1; ++i) {
        p_new_tsm->path_from_global_to_parent_tsm[i] = tsm_key_ctx_copy(p_tsm->path_from_global_to_parent_tsm[i]);
    }
    // at last index at key_ctx of parent_map
    struct tsm_key_ctx key_ctx_tsm = { .key = p_tsm->base.key, .key_is_number = p_tsm->base.key_is_number };
    tklog_scope(p_new_tsm->path_from_global_to_parent_tsm[p_new_tsm->path_length-1] = tsm_key_ctx_copy(key_ctx_tsm));

    // Create proper copies of the string keys for the base_type node
    tklog_scope(struct tsm_key_ctx base_key_ctx = tsm_key_ctx_copy(base_type_key_ctx));
    tklog_scope(struct tsm_base_node* base_type_base = tsm_base_type_node_create(
        base_key_ctx, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free,
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print_info,
        sizeof(struct tsm_base_type_node)));

    if (!base_type_base) {
        tklog_error("Failed to create base_type node\n");
        tsm_key_ctx_free(&base_key_ctx);
        _tsm_tsm_type_free(p_new_tsm_base);
        return false;
    }
    
    uint64_t hash = _tsm_hash_key(base_type_base->key, base_type_base->key_is_number);

    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &base_key_ctx, &base_type_base->lfht_node));
    
    if (result != &base_type_base->lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        tsm_base_node_free(base_type_base);
        _tsm_tsm_type_free(p_new_tsm_base);
        return false;
    }

    tklog_scope(struct tsm_base_node* base_node = tsm_node_get(p_new_tsm_base, base_type_key_ctx));
    if (!base_node) {
        tklog_error("base_type node not found after initialization\n");
        _tsm_tsm_type_free(p_new_tsm_base);
        return NULL;
    }
    
    tklog_info("Created base_type node inside new TSM with key: %s, size: %u\n", base_node->key.string, base_node->this_size_bytes);

    // creating tsm_type now, as base_type is created
    tklog_scope(struct tsm_key_ctx tsm_type_key_ctx_copy = tsm_key_ctx_copy(tsm_type_key_ctx));
    tklog_scope(struct tsm_base_node* p_new_tsm_type_base = tsm_base_type_node_create(
        tsm_type_key_ctx_copy, 
        sizeof(struct tsm_base_type_node), 
        _tsm_tsm_type_free,
        _tsm_tsm_type_free_callback,
        _tsm_tsm_type_is_valid,
        _tsm_tsm_type_print_info,
        sizeof(struct tsm)));
    if (!p_new_tsm_type_base) {
        tklog_error("Failed to create tsm_type\n");
        tsm_key_ctx_free(&tsm_type_key_ctx_copy);
        _tsm_tsm_type_free(p_new_tsm_base);
        return NULL;
    }

    tklog_scope(bool insert_result = tsm_node_insert(p_new_tsm_base, p_new_tsm_type_base));
    if (!insert_result) {
        tklog_error("failed to insert tsm_type into new TSM\n");
        _tsm_tsm_type_free(p_new_tsm_base);
        return NULL;
    }
    tklog_scope(insert_result = tsm_node_insert(p_tsm_base, p_new_tsm_base));
    if (!insert_result) {
        tklog_error("Failed to insert new tsm into given TSM\n");
        _tsm_tsm_type_free(p_new_tsm_base);
        return NULL;
    }

    return p_new_tsm_base;
}


struct tsm_base_node* tsm_base_node_create(struct tsm_key_ctx key_ctx, struct tsm_key_ctx type_key_ctx, uint32_t this_size_bytes, bool this_is_type) {
    if (key_ctx.key.string == NULL && !key_ctx.key_is_number) {
        tklog_error("key.string is NULL and key_is_number is false\n");
        return NULL;
    }
    if (key_ctx.key.number == 0) {
        tklog_error("type_key.number is 0\n");
        return NULL;
    }
    if (this_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_error("this_size_bytes is less than the size of the tsm_base_node\n");
        return NULL;
    }
    struct tsm_base_node* node = calloc(1, this_size_bytes);
    if (!node) {
        tklog_error("Failed to allocate memory for node\n");
        return NULL;
    }
    cds_lfht_node_init(&node->lfht_node);
    
    // Handle key assignment - copy string keys, use number keys directly
    if (key_ctx.key_is_number) {
        if (key_ctx.key.number == 0) {
            // id key is number and is 0 that means we must find a valid number key because 0 is invalid. 
            // this is the only function where number key being 0 is allowed.
            node->key.number = atomic_fetch_add(&g_key_counter, 1);
        } else {
            node->key.number = key_ctx.key.number;
        }
    } else {
        node->key.string = key_ctx.key.string;
    }
    
    // Handle type_key assignment - copy string keys, use number keys directly
    if (type_key_ctx.key_is_number) {
        node->type_key.number = type_key_ctx.key.number;
    } else {
        // String type_key is already copied by tsm_key_create
        node->type_key.string = type_key_ctx.key.string;
    }
    
    node->key_is_number = key_ctx.key_is_number;
    node->type_key_is_number = type_key_ctx.key_is_number;
    node->this_size_bytes = this_size_bytes;
    node->this_is_type = this_is_type;
    return node;
}
bool tsm_base_node_free(struct tsm_base_node* p_base_node) {
    if (!p_base_node) {
        tklog_error("p_base_node is NULL when trying to free it");
        return false;
    }
    tsm_key_free(p_base_node->key, p_base_node->key_is_number);
    tsm_key_free(p_base_node->type_key, p_base_node->type_key_is_number);
    free(p_base_node);
    return true;
}
bool tsm_base_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm_key_ctx key_ctx = { .key = p_base->key, .key_is_number = p_base->key_is_number};
    struct tsm_key_ctx type_key_ctx = { .key = p_base->type_key, .key_is_number = p_base->type_key_is_number};

    tklog_scope(struct tsm_base_node* p_base_copy = tsm_node_get(p_tsm_base, key_ctx));
    if (p_base != p_base_copy) {
        tklog_debug("given p_base and p_base gotten from key in given p_base is not the same\n");
        tklog_info("given p_base:\n");
        tklog_scope(bool print_result = tsm_node_print_info(p_tsm_base, p_base));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        tklog_info("retrieved p_base from given p_base:\n");
        tklog_scope(print_result = tsm_node_print_info(p_tsm_base, p_base_copy));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        return false;
    }

    tklog_scope(struct tsm_base_node* p_base_type = tsm_node_get(p_tsm_base, type_key_ctx));
    if (!p_base_type) {
        tklog_debug("did not find type for given base node\n");
        tklog_scope(bool print_result = tsm_node_print_info(p_tsm_base, p_base));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        return false;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    if (p_base->this_size_bytes != p_type->type_size_bytes) {
        tklog_debug("p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)", p_base->this_size_bytes, p_type->type_size_bytes);
        tklog_scope(bool print_result = tsm_node_print_info(p_tsm_base, p_base));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        return false;
    }

    tklog_scope(bool is_deleted = tsm_node_is_deleted(p_base));
    if (is_deleted) {
        tklog_debug("node is deleted and therefore not valid\n");
        return false;
    }

    return true;
}
bool tsm_base_node_print_info(struct tsm_base_node* p_base) {

    tklog_info("tsm_base_node:\n");
    tklog_scope(bool is_deleted = tsm_node_is_deleted(p_base));
    if (is_deleted) {
        tklog_info("    is deleted\n");
    } else {
        tklog_info("    is not deleted\n");
    }
    if (p_base->key_is_number) {
        tklog_info("    key: %lld\n", p_base->key.number);
    } else {
        tklog_info("    key: %s\n", p_base->key.string);
    }

    if (p_base->type_key_is_number) {
        tklog_info("    type_key: %lld\n", p_base->type_key.number);
    } else {
        tklog_info("    type_key: %s\n", p_base->type_key.string);
    }

    tklog_info("    size of whole node in bytes: %d\n", p_base->this_size_bytes);

    return true;
}


struct tsm_base_node* tsm_base_type_node_create(
    struct tsm_key_ctx key_ctx, 
    uint32_t this_size_bytes,
    bool (*fn_free)(struct tsm_base_node*),
    void (*fn_free_callback)(struct rcu_head*),
    bool (*fn_is_valid)(struct tsm_base_node*, struct tsm_base_node*),
    bool (*fn_print_info)(struct tsm_base_node*),
    uint32_t type_size_bytes) {
    if (this_size_bytes < sizeof(struct tsm_base_type_node)) {
        tklog_error("this_size_bytes(%d) is less than sizeof(tsm_base_type_node)\n", sizeof(struct tsm_base_type_node));
        return NULL;
    }
    if (type_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_error("type_size_bytes(%d) is less than sizeof(tsm_base_node)\n", sizeof(struct tsm_base_node));
        return NULL;
    }
    if (fn_free == NULL) {
        tklog_error("fn_free is NULL\n");
        return NULL;
    }
    if (fn_free_callback == NULL) {
        tklog_error("fn_free is NULL\n");
        return NULL;
    }
    if (fn_is_valid == NULL) {
        tklog_error("fn_free is NULL\n");
        return NULL;
    }
    if (fn_print_info == NULL) {
        tklog_error("fn_free is NULL\n");
        return NULL;
    }

    // Create proper copies of the string keys for the base_type node
    tklog_scope(struct tsm_key_ctx base_type_key_ctx_copy = tsm_key_ctx_copy(base_type_key_ctx));
    
    tklog_scope(struct tsm_base_node* p_base = tsm_base_node_create(
        key_ctx, 
        base_type_key_ctx_copy, 
        sizeof(struct tsm_base_type_node),
        true));

    if (!p_base) {
        tklog_error("Failed to create base_type node\n");
        tsm_key_ctx_free(&base_type_key_ctx_copy);
        return NULL;
    }

    struct tsm_base_type_node* p_base_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    p_base_type->fn_free = _tsm_base_type_node_free;
    p_base_type->fn_free_callback = _tsm_base_type_node_free_callback;
    p_base_type->fn_is_valid = _tsm_base_type_node_is_valid;
    p_base_type->fn_print_info = _tsm_base_type_node_print_info;
    p_base_type->type_size_bytes = sizeof(struct tsm_base_type_node);

    return p_base;
}
struct tsm_base_node* tsm_node_get(struct tsm_base_node* p_tsm_base, struct tsm_key_ctx key_ctx) {
    if (!p_tsm_base) {
        tklog_error("p_tsm is NULL\n");
        return NULL;
    }
    if (key_ctx.key.number == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }
    if (key_ctx.key_is_number && key_ctx.key.number == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return NULL;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash internally
    uint64_t hash = _tsm_hash_key(key_ctx.key, key_ctx.key_is_number);
    struct cds_lfht_iter iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key_ctx, &iter);
    struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(&iter);
    
    if (lfht_node) {
        struct tsm_base_node* result = caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
        return result;
    }
    return NULL;
}
bool tsm_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_key_ctx key_ctx) {

    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return false;
    }

    tklog_scope(struct tsm_base_node* p_base = tsm_node_get(p_tsm_base, key_ctx));
    if (!p_base) {
        tklog_notice("getting base node failed\n");
        return false;
    }

    struct tsm_key_ctx type_key_ctx = { .key = p_base->type_key, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_base_type = tsm_node_get(p_tsm_base, type_key_ctx));
    if (!p_base_type) {
        tklog_notice("tried to get type node but got NULL for node: \n");
        return false;
    }

    tklog_scope(struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base));
    if (p_base->this_size_bytes != p_type->type_size_bytes) {
        tklog_notice("p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)\n", p_base->this_size_bytes, p_type->type_size_bytes);
        tklog_scope(p_type->fn_print_info(p_base));
        return false;
    }

    tklog_scope(bool return_result = p_type->fn_is_valid(p_tsm_base, p_base));

    return return_result;
}
bool tsm_node_print_info(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return false;
    }

    if (!p_base) {
        tklog_error("p_base is NULL");
        return false;
    }

    struct tsm_key_ctx type_key_ctx = { .key = p_base->type_key, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_base_type = tsm_node_get(p_tsm_base, type_key_ctx));

    if (!p_base_type) {
        if (p_base->type_key_is_number) {
            tklog_error("could not get type node with key %lld\n", p_base->type_key.number);
        } else {
            tklog_error("could not get type node with key %s\n", p_base->type_key.string);
        }
        return false;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    p_type->fn_print_info(p_base);

    return true;
}
bool tsm_node_insert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key.number == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }
    if (new_node->type_key.number == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }
    if (!tsm_node_is_tsm(p_tsm_base)) {
        tklog_error("given p_tsm_base is not TSM\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }
    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base is not TSM\n");
        tklog_scope(tsm_node_print_info(p_tsm_base , new_node));
        return false;
    }

    struct tsm_key_ctx type_key_ctx = { .key = new_node->type_key, .key_is_number = new_node->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_node_base = tsm_node_get(p_tsm_base, type_key_ctx));

    if (!p_type_node_base) {
        tklog_error("didnt find node_type");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_node_base, struct tsm_base_type_node, base);
    uint32_t type_size_bytes = p_type_node->type_size_bytes;

    if (type_size_bytes != new_node->this_size_bytes) {
        tklog_error("type_size_bytes(%d) != new_node->this_size_bytes(%d)", type_size_bytes, new_node->this_size_bytes);
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    uint64_t hash = _tsm_hash_key(new_node->key, new_node->key_is_number);
    struct tsm_key_ctx key_ctx = { .key = new_node->key, .key_is_number = new_node->key_is_number };
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_tsm->p_ht, hash, _tsm_key_match, &key_ctx, &new_node->lfht_node));
    
    if (result != &new_node->lfht_node) {
        tklog_error("Somehow node already exists\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    if (new_node->key_is_number) {
        tklog_debug("Successfully inserted node with number key %llu\n", new_node->key.number);
        return true;
    } else {
        tklog_debug("Successfully inserted node with string key %s\n", new_node->key.string);
        return true;
    }
}
bool tsm_node_free(struct tsm_base_node* p_tsm_base, struct tsm_key_ctx key_ctx) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    if (key_ctx.key.number == 0) {
        tklog_error("trying to delete node by NULL/0 key\n");
        return false;
    }

    
    // Find the node to remove
    tklog_scope(struct tsm_base_node* p_base = tsm_node_get(p_tsm_base, key_ctx));
    if (!p_base) {
        tklog_warning("Cannot remove - node with key which is not found\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, p_base));
        return false;
    }

    // Get the type information
    struct tsm_key_ctx type_key_ctx = { .key = p_base->type_key, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_base_node = tsm_node_get(p_tsm_base, type_key_ctx));
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for node to be removed\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, p_base));
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_base_node, struct tsm_base_type_node, base);
    
    // Copy the free callback while still holding the read lock
    // This ensures the callback remains valid even after we release the lock
    void (*free_callback)(struct rcu_head*) = p_type_node->fn_free_callback;
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, p_base));
        return false;
    }
    
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    if (cds_lfht_del(p_tsm->p_ht, &p_base->lfht_node) != 0) {
        tklog_error("Failed to delete node thread safe map\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, p_base));
        return false;
    }

    // Schedule for RCU cleanup after successful removal
    tklog_scope(call_rcu(&p_base->rcu_head, free_callback));

    return true;
}
bool tsm_node_update(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key.number == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }
    if (new_node->type_key.number == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base node is not a TSM\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Find the existing node
    uint64_t                hash = _tsm_hash_key(new_node->key, new_node->key_is_number);
    struct tsm_key_ctx ctx = { .key = new_node->key, .key_is_number = new_node->key_is_number };
    struct cds_lfht_iter    old_iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &ctx, &old_iter);
    struct cds_lfht_node*   old_lfht_node = cds_lfht_iter_get_node(&old_iter);
    struct tsm_base_node*    old_node = caa_container_of(old_lfht_node, struct tsm_base_node, lfht_node);
    if (!old_node) {
        if (new_node->key_is_number) {
            tklog_debug("Cannot update - node with number key %llu not found\n", new_node->key.number);
        } else {
            tklog_debug("Cannot update - node with string key %s not found\n", new_node->key.string);
        }
        return false;
    }

    // Get the type information
    struct tsm_key_ctx old_type_key_ctx = { .key = old_node->type_key, .key_is_number = old_node->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_base_node = tsm_node_get(p_tsm_base, old_type_key_ctx));
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for existing node\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_base_node, struct tsm_base_type_node, base);
    
    // Verify data size matches expected type size 
    if (new_node->this_size_bytes != p_type_node->type_size_bytes) {
        tklog_error("new node size %u doesn't match current node size %u\n", new_node->this_size_bytes, p_type_node->type_size_bytes);
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    // Get the free callback while still holding the read lock
    void (*free_callback)(struct rcu_head*) = p_type_node->fn_free_callback;
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    int replace_result = cds_lfht_replace(p_tsm->p_ht, &old_iter, hash, _tsm_key_match, &ctx, &new_node->lfht_node);
    
    if (replace_result == 0) {
        // Schedule old node for RCU cleanup
        tklog_scope(call_rcu(&old_node->rcu_head, free_callback));
        if (new_node->key_is_number) {
            tklog_debug("Successfully updated node with number key %llu\n", new_node->key.number);
        } else {
            tklog_debug("Successfully updated node with string key %s\n", new_node->key.string);
        }
        return true;
    } else {
        tklog_error("Failed to replace node in hash table\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }
}
bool tsm_node_upsert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key.number == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }
    if (new_node->type_key.number == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print_info(p_tsm_base, new_node));
        return false;
    }

    struct tsm_key_ctx key_ctx = { .key = new_node->key, .key_is_number = new_node->key_is_number };
    tklog_scope(struct tsm_base_node* old_node = tsm_node_get(p_tsm_base, key_ctx));

    if (old_node) {
        tklog_scope(bool update_result = tsm_node_update(p_tsm_base, new_node));
        if (!update_result) {
            tklog_error("update inside upsert function failed\n");
            return false;
        }
    } else {
        tklog_scope(bool insert_result = tsm_node_insert(p_tsm_base, new_node));
        if (!insert_result) {
            tklog_error("insert function inside upsert function failed\n");
            return false;
        }
    }
    return true;
}
bool tsm_node_is_deleted(struct tsm_base_node* node) {
    if (!node) {
        tklog_warning("given node is NULL\n");
        return true; // NULL node is considered deleted
    }
    tklog_scope(bool result = cds_lfht_is_node_deleted(&node->lfht_node));
    return result;
}


bool tsm_iter_first(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    if (!p_tsm_base || !iter) {
        tklog_error("tsm_iter_first called with NULL parameters or uninitialized system\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    tklog_scope(cds_lfht_first(p_tsm->p_ht, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}
bool tsm_iter_next(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    if (!p_tsm_base || !iter) {
        tklog_error("tsm_iter_next called with NULL parameters or uninitialized system\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    tklog_scope(cds_lfht_next(p_tsm->p_ht, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}
struct tsm_base_node* tsm_iter_get_node(struct cds_lfht_iter* iter) {
    if (!iter) {
        tklog_error("tsm_iter_next called with NULL parameters or uninitialized system\n");
        return false;
    }
    
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    if (lfht_node) {
        return caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
    }
    return NULL;
}
bool tsm_iter_lookup(struct tsm_base_node* p_tsm_base, struct tsm_key_ctx key_ctx, struct cds_lfht_iter* iter) {
    if (!p_tsm_base || !iter) {
        tklog_error("tsm_lookup_iter called with NULL parameters\n");
        return false;
    }
    if (key_ctx.key.number == 0) {
        tklog_error("number key is 0\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash
    uint64_t hash = _tsm_hash_key(key_ctx.key, key_ctx.key_is_number);
    tklog_scope(cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key_ctx, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}
unsigned long tsm_nodes_count(struct tsm_base_node* p_tsm_base) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return 0;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    long split_count_before, split_count_after;
    unsigned long count;
    
    // cds_lfht_count_nodes MUST be called from within read-side critical section
    // according to URCU specification
    cds_lfht_count_nodes(p_tsm->p_ht, &split_count_before, &count, &split_count_after);
    
    return count;
}