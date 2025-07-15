#include "global_data/core.h"
#include "tklog.h"
#include "xxhash.h"

#include <stdatomic.h>
#include <pthread.h>
#include <unistd.h>

#define MAX_STRING_KEY_LEN 64

static struct cds_lfht *g_p_ht = NULL;
static _Atomic uint64_t g_key_counter = 1; // 0 is invalid. 1 is the first valid key
static const union gd_key g_base_type_key = { .string = "base_type" };
static const bool g_base_type_key_is_number = false;

struct gd_key_match_ctx {
    union gd_key key;
    bool key_is_number;
};

// Unified key matching function that handles both number and string keys
static int _gd_key_match(struct cds_lfht_node *node, const void *key_ctx) {
    struct gd_base_node *base_node = caa_container_of(node, struct gd_base_node, lfht_node);
    struct gd_key_match_ctx *ctx = (struct gd_key_match_ctx *)key_ctx;
    
    // Check if key types match
    if (base_node->key_is_number != ctx->key_is_number) {
        return 0; // No match if key types don't match
    }
    
    if (ctx->key_is_number) {
        // Number key comparison
        uint64_t node_key = base_node->key.number;
        uint64_t lookup_key = ctx->key.number;
        tklog_debug("Comparing number key: node_key=%llu, lookup_key=%llu\n", node_key, lookup_key);
        return node_key == lookup_key;
    } else {
        // String key comparison - access string fields directly since we're within hash table operations
        const char* node_key = base_node->key.string;
        const char* lookup_key = ctx->key.string;
        tklog_debug("Comparing string key: node_key=%s, lookup_key=%s\n", node_key, lookup_key);
        return strcmp(node_key, lookup_key) == 0;
    }
}

// Hash function for gd_key union
static uint64_t _gd_hash_key(union gd_key key, bool key_is_number) {
    if (key_is_number) {
        return XXH3_64bits(&key.number, sizeof(uint64_t));
    } else {
        return XXH3_64bits(key.string, strlen(key.string));
    }
}

// Implementation of gd_get_node_size function
static uint64_t _gd_get_node_size(struct cds_lfht_node* node) {
    if (!node) {
        tklog_error("_gd_get_node_size called with NULL node\n");
        return 0;
    }
    struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
    return base_node->size_bytes;
}

// Implementation of gd_get_node_start_ptr function
static void* _gd_get_node_start_ptr(struct cds_lfht_node* node) {
    if (!node) {
        tklog_error("_gd_get_node_start_ptr called with NULL node\n");
        return NULL;
    }
    struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
    return (void*)base_node;
}

// Default free function for node type nodes
static bool _gd_base_type_node_free(struct gd_base_node* node) {
    if (!node) return false;

    if (!node->key_is_number) {
        tklog_info("inside free node key.string which is %s\n", node->key.string);
    } else {
        tklog_info("inside free node key.number which is %llu\n", node->key.number);
    }
    if (!node->type_key_is_number) {
        tklog_info("freeing type_key.string which is %s\n", node->type_key.string);
    } else {
        tklog_info("freeing type_key.number which is %llu\n", node->type_key.number);
    }

    // Free key based on its type (only free if string key)
    if (node->key.string && !node->key_is_number) {
        free(node->key.string);
        node->key.string = NULL;
    }
    
    // Free type_key based on its type (only free if string key)
    if (node->type_key.string && !node->type_key_is_number) {
        free(node->type_key.string);
        node->type_key.string = NULL;
    }
    
    free(node);
    return true;
}

// Default free callback for node type nodes
static void _gd_base_type_node_free_callback(struct rcu_head* rcu_head) {
    struct gd_base_node* node = caa_container_of(rcu_head, struct gd_base_node, rcu_head);
    _gd_base_type_node_free(node);
}

// Default validation function for node type nodes
static bool _gd_base_type_node_is_valid(struct gd_base_node* node) {
    if (!node) return false;
    
    // Basic validation - check if it's a node type node
    struct gd_base_type_node* node_type = caa_container_of(node, struct gd_base_type_node, base);
    bool (*free_node)(struct gd_base_node*) = rcu_dereference(node_type->fn_free_node);
    void (*free_callback)(struct rcu_head*) = rcu_dereference(node_type->fn_free_node_callback);
    bool (*is_valid)(struct gd_base_node*) = rcu_dereference(node_type->fn_is_valid);
    return (node_type->type_size >= sizeof(struct gd_base_node) && free_node != NULL && free_callback != NULL);
}

// Initialize the base_type node
bool _gd_init_base_type(void) {
    
    // Create proper copies of the string keys for the base_type node
    union gd_key base_key = gd_key_create(g_base_type_key.number, g_base_type_key.string, g_base_type_key_is_number);
    union gd_key base_type_key_copy = gd_key_create(g_base_type_key.number, g_base_type_key.string, g_base_type_key_is_number);
    
    struct gd_base_type_node* base_type = (struct gd_base_type_node*)gd_base_node_create(
        base_key, g_base_type_key_is_number, 
        base_type_key_copy, g_base_type_key_is_number, 
        sizeof(struct gd_base_type_node));
    if (!base_type) {
        tklog_error("Failed to create base_type node\n");
        gd_key_free(base_key, g_base_type_key_is_number);
        gd_key_free(base_type_key_copy, g_base_type_key_is_number);
        return false;
    }
    base_type->fn_free_node = _gd_base_type_node_free;
    base_type->fn_free_node_callback = _gd_base_type_node_free_callback;
    base_type->fn_is_valid = _gd_base_type_node_is_valid;
    base_type->type_size = sizeof(struct gd_base_type_node);
    
    uint64_t hash = _gd_hash_key(base_type->base.key, base_type->base.key_is_number);
    struct gd_key_match_ctx ctx = { .key = base_type->base.key, .key_is_number = base_type->base.key_is_number };

    rcu_register_thread();
    rcu_read_lock();
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(g_p_ht, hash, _gd_key_match, &ctx, &base_type->base.lfht_node));
    
    if (result != &base_type->base.lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        gd_key_free(base_type->base.key, g_base_type_key_is_number);
        gd_key_free(base_type->base.type_key, g_base_type_key_is_number);
        free(base_type);
        return false;
    }

    tklog_scope(union gd_key key = gd_key_create(g_base_type_key.number, g_base_type_key.string, g_base_type_key_is_number));
    tklog_scope(struct gd_base_node* base_node = gd_node_get(key, g_base_type_key_is_number));
    tklog_scope(gd_key_free(key, g_base_type_key_is_number));
    rcu_read_unlock();
    rcu_unregister_thread();
    if (!base_node) {
        tklog_error("base_type node not found after initialization\n");
        return false;
    }
    
    tklog_info("Created base_type node with key: %s, size: %u\n", base_node->key.string, base_node->size_bytes);
    
    return true;
}

union gd_key gd_base_type_get_key_copy(void) {
    return gd_key_create(g_base_type_key.number, g_base_type_key.string, g_base_type_key_is_number);
}
bool gd_base_type_key_is_number(void) {
    return g_base_type_key_is_number;
}

// Initialize the global data system
bool gd_init(void) {
    if (g_p_ht != NULL) {
        tklog_warning("Global data system already initialized\n");
        return true;
    }
    
    // Initialize RCU system first
    rcu_init();
    
    // Register the node size function with urcu_safe  system
    urcu_safe_set_node_size_function(_gd_get_node_size);
    // Register the node bounds functions with urcu_safe system
    urcu_safe_set_node_start_ptr_function(_gd_get_node_start_ptr);
    
    tklog_scope(g_p_ht = cds_lfht_new(8, 8,0,CDS_LFHT_AUTO_RESIZE,NULL));
    if (!g_p_ht) {
        tklog_error("Failed to create global hash table\n");
        return false;
    }
    
    // Initialize the base_type node first
    if (!_gd_init_base_type()) {
        tklog_error("Failed to initialize base_type node\n");
        tklog_scope(cds_lfht_destroy(g_p_ht, NULL));
        g_p_ht = NULL;
        return false;
    }
    
    tklog_info("Global data system initialized\n");
    return true;
}

// Type tracking structure for cleanup
struct type_tracking_cleanup_key {
    union gd_key key;
    bool key_is_number;
};

// Hash function for type tracking
static uint64_t type_tracking_cleanup_hash(struct type_tracking_cleanup_key tk) {
    return _gd_hash_key(tk.key, tk.key_is_number);
}

// Comparison function for type tracking
static int type_tracking_cleanup_cmpr(struct type_tracking_cleanup_key a, struct type_tracking_cleanup_key b) {
    if (a.key_is_number != b.key_is_number) {
        return a.key_is_number ? 1 : -1;
    }
    
    if (a.key_is_number) {
        return (a.key.number > b.key.number) - (a.key.number < b.key.number);
    } else {
        return strcmp(a.key.string, b.key.string);
    }
}

// Define Verstable set for type tracking
#define NAME type_tracking_cleanup_set
#define KEY_TY struct type_tracking_cleanup_key
#define HASH_FN type_tracking_cleanup_hash
#define CMPR_FN type_tracking_cleanup_cmpr
#include "verstable.h"

// Cleanup the global data system
void gd_cleanup(void) {

    if (!g_p_ht) {
        tklog_warning("Global data system not initialized\n");
        return;
    }
    
    // Create a Verstable set to track which nodes are used as types
    type_tracking_cleanup_set used_as_type_set;
    type_tracking_cleanup_set_init(&used_as_type_set);
    
    // Iteratively remove nodes layer by layer
    const int batch_size = 1000;
    struct cds_lfht_node* nodes_to_delete[batch_size];
    int layer = 0;
    
    while (true) {
        int total_nodes = 0;
        int delete_count = 0;
        struct cds_lfht_iter iter;
        
        // Clear the used_as_type tracking set
        type_tracking_cleanup_set_clear(&used_as_type_set);
        
        // Phase 1: Count nodes and identify which are used as types
        rcu_read_lock();
        cds_lfht_first(g_p_ht, &iter);
        struct cds_lfht_node* node;
        
        while ((node = cds_lfht_iter_get_node(&iter)) != NULL) {
            struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
            total_nodes++;
            
            // Add this node's type to the used_as_type set
            struct type_tracking_cleanup_key track_key;
            track_key.key = base_node->type_key;
            track_key.key_is_number = base_node->type_key_is_number;
            
            type_tracking_cleanup_set_itr itr = type_tracking_cleanup_set_insert(&used_as_type_set, track_key);
            if (type_tracking_cleanup_set_is_end(itr)) {
                tklog_error("Out of memory tracking used types\n");
                // Continue anyway, will force cleanup
            }
            
            cds_lfht_next(g_p_ht, &iter);
        }
        
        // If no nodes left, we're done
        if (total_nodes == 0) {
            rcu_read_unlock();
            break;
        }
        
        // Phase 2: Collect nodes that are not used as types
        cds_lfht_first(g_p_ht, &iter);
        while ((node = cds_lfht_iter_get_node(&iter)) != NULL && delete_count < batch_size) {
            struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
            
            // Check if this node is used as a type
            struct type_tracking_cleanup_key lookup_key;
            lookup_key.key = base_node->key;
            lookup_key.key_is_number = base_node->key_is_number;
            
            type_tracking_cleanup_set_itr found = type_tracking_cleanup_set_get(&used_as_type_set, lookup_key);
            
            if (type_tracking_cleanup_set_is_end(found)) {
                // This node is not used as a type, mark for deletion
                nodes_to_delete[delete_count++] = node;
            }
            
            cds_lfht_next(g_p_ht, &iter);
        }
        
        // If no nodes to delete in this layer, force cleanup of remaining nodes
        if (delete_count == 0 && total_nodes > 0) {
            tklog_warning("No progress made, forcing cleanup of remaining %d nodes\n", total_nodes);
            cds_lfht_first(g_p_ht, &iter);
            while ((node = cds_lfht_iter_get_node(&iter)) != NULL && delete_count < batch_size) {
                nodes_to_delete[delete_count++] = node;
                cds_lfht_next(g_p_ht, &iter);
            }
        }
        
        // Phase 3: Delete collected nodes
        for (int i = 0; i < delete_count; i++) {
            struct gd_base_node* base_node = caa_container_of(nodes_to_delete[i], struct gd_base_node, lfht_node);
            
            // Remove from hash table
            if (cds_lfht_del(g_p_ht, nodes_to_delete[i]) == 0) {
                // Get the type node to find the correct free callback
                struct gd_base_node* type_base = gd_node_get(base_node->type_key, base_node->type_key_is_number);
                
                if (type_base) {
                    struct gd_base_type_node* type_node = caa_container_of(type_base, struct gd_base_type_node, base);
                    void (*free_callback)(struct rcu_head*) = rcu_dereference(type_node->fn_free_node_callback);
                    call_rcu(&base_node->rcu_head, free_callback);
                } else {
                    // Fallback to default callback if type not found
                    call_rcu(&base_node->rcu_head, _gd_base_type_node_free_callback);
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
    type_tracking_cleanup_set_cleanup(&used_as_type_set);

    rcu_barrier();

    // Now destroy the hash table
    cds_lfht_destroy(g_p_ht, NULL);
    g_p_ht = NULL;
    
    tklog_info("Global data system cleanup completed (%d layers)\n", layer);
}

// Internal function - moved from header to be internal only
static void _gd_lfht_node_init(struct gd_base_node* node) {
    if (!node) {
        tklog_error("_gd_lfht_node_init called with NULL node\n");
        return;
    }
    tklog_scope(cds_lfht_node_init(&node->lfht_node));
}

struct gd_base_node* gd_base_node_create(union gd_key key, bool key_is_number, union gd_key type_key, bool type_key_is_number, uint32_t size_bytes) 
{
    if (key.string == NULL && !key_is_number) {
        tklog_error("key.string is NULL and key_is_number is false\n");
        return NULL;
    }
    if (type_key.number == 0) {
        tklog_error("type_key.number is 0\n");
        return NULL;
    }
    if (size_bytes < sizeof(struct gd_base_node)) {
        tklog_error("size_bytes is less than the size of the gd_base_node\n");
        return NULL;
    }
    struct gd_base_node* node = calloc(1, size_bytes);
    if (!node) {
        tklog_error("Failed to allocate memory for node\n");
        return NULL;
    }
    tklog_scope(_gd_lfht_node_init(node));
    
    // Handle key assignment - copy string keys, use number keys directly
    if (key_is_number) {
        if (key.number == 0) {
            // id key is number and is 0 that means we must find a valid number key because 0 is invalid. 
            // this is the only function where number key being 0 is allowed.
            node->key.number = atomic_fetch_add(&g_key_counter, 1);
        } else {
            node->key.number = key.number;
        }
    } else {
        // String key is already copied by gd_key_create
        node->key.string = key.string;
    }
    
    // Handle type_key assignment - copy string keys, use number keys directly
    if (type_key_is_number) {
        node->type_key.number = type_key.number;
    } else {
        // String type_key is already copied by gd_key_create
        node->type_key.string = type_key.string;
    }
    
    node->key_is_number = key_is_number;
    node->type_key_is_number = type_key_is_number;
    node->size_bytes = size_bytes;
    return node;
}
bool gd_base_node_free(struct gd_base_node* p_base_node) {
    if (!p_base_node) {
        tklog_error("p_base_node is NULL when trying to free it");
        return false;
    }
    gd_key_free(p_base_node->key, p_base_node->key_is_number);
    gd_key_free(p_base_node->type_key, p_base_node->type_key_is_number);
    free(p_base_node);
    return true;
}

// Getter function for the global hash table (for test access)
struct cds_lfht* _gd_get_hash_table(void) {
    return g_p_ht;
}



// Main node access function - must be used with RCU read lock
struct gd_base_node* gd_node_get(union gd_key key, bool key_is_number) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return NULL;
    }
    if (key.number == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }
    if (key_is_number && key.number == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }
    if (key_is_number) {
        tklog_debug("Looking up number key %llu\n", key.number);
    } else {
        tklog_debug("Looking up string key %s\n", key.string);
    }
    
    // Calculate hash internally
    uint64_t hash = _gd_hash_key(key, key_is_number);
    
    struct gd_key_match_ctx ctx = { .key = key, .key_is_number = key_is_number };
    struct cds_lfht_iter iter = {0};
    
    cds_lfht_lookup(g_p_ht, hash, _gd_key_match, &ctx, &iter);
    struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(&iter);
    
    if (lfht_node) {
        struct gd_base_node* result = caa_container_of(lfht_node, struct gd_base_node, lfht_node);
        if (key_is_number) {
            tklog_debug("Found node for number key %llu\n", key.number);
        } else {
            tklog_debug("Found node for string key %s\n", key.string);
        }
        return result;
    }
    
    if (key_is_number) {
        tklog_debug("Node for number key %llu not found\n", key.number);
    } else {
        tklog_debug("Node for string key %s not found\n", key.string);
    }
    return NULL;
}

// Insert a new node into the global data system
bool gd_node_insert(struct gd_base_node* new_node) {
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key.number == 0) {
        tklog_error("key number is 0\n");
        return false;
    }
    if (new_node->type_key.number == 0) {
        tklog_error("type_key number is 0\n");
        return false;
    }

    rcu_read_lock();

    tklog_scope(struct gd_base_node* p_type_node_base = gd_node_get(new_node->type_key, new_node->type_key_is_number));

    if (!p_type_node_base) {
        if (new_node->type_key_is_number) {
            tklog_error("didnt find node_type for given type_key: %lld\n", new_node->type_key.number);
        } else {
            tklog_error("didnt find node_type for given type_key: %s\n", new_node->type_key.string);
        }
        rcu_read_unlock();
        return false;
    }

    struct gd_base_type_node* p_type_node = caa_container_of(p_type_node_base, struct gd_base_type_node, base);
    uint32_t type_size = p_type_node->type_size;

    uint64_t hash = _gd_hash_key(new_node->key, new_node->key_is_number);
    
    struct gd_key_match_ctx ctx = { .key = new_node->key, .key_is_number = new_node->key_is_number };
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(g_p_ht, hash, _gd_key_match, &ctx, &new_node->lfht_node));
    
    if (result != &new_node->lfht_node) {
        if (new_node->key_is_number) {
            tklog_critical("Somehow node with number key %llu already exists\n", new_node->key.number);
        } else {
            tklog_critical("Somehow node with string key %s already exists\n", new_node->key.string);
        }
        rcu_read_unlock();
        return false;
    }

    if (new_node->key_is_number) {
        tklog_debug("Successfully inserted node with number key %llu\n", new_node->key.number);
        rcu_read_unlock();
        return true;
    } else {
        tklog_debug("Successfully inserted node with string key %s\n", new_node->key.string);
        rcu_read_unlock();
        return true;
    }
}

// Remove a node from the global data system
bool gd_node_remove(union gd_key key, bool key_is_number) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return false;
    }
    if (key.number == 0) {
        tklog_error("trying to delete node by NULL/0 key\n");
        return false;
    }

    rcu_read_lock();
    
    // Find the node to remove
    tklog_scope(struct gd_base_node* p_base = gd_node_get(key, key_is_number));
    if (!p_base) {
        if (key_is_number) {
            tklog_debug("Cannot remove - node with number key %llu not found\n", key.number);
        } else {
            tklog_debug("Cannot remove - node with string key %s not found\n", key.string);
        }
        rcu_read_unlock();
        return false;
    }

    // Get the type information
    tklog_scope(struct gd_base_node* p_type_base_node = gd_node_get(p_base->type_key, p_base->type_key_is_number));
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for node to be removed\n");
        rcu_read_unlock();
        return false;
    }

    struct gd_base_type_node* p_type_node = caa_container_of(p_type_base_node, struct gd_base_type_node, base);
    
    // Copy the free callback while still holding the read lock
    // This ensures the callback remains valid even after we release the lock
    void (*free_callback)(struct rcu_head*) = p_type_node->fn_free_node_callback;
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        rcu_read_unlock();
        return false;
    }
    

    if (cds_lfht_del(g_p_ht, &p_base->lfht_node) != 0) {
        if (key_is_number) {
            tklog_error("Failed to delete node with number key %llu from hash table\n", key.number);
        } else {
            tklog_error("Failed to delete node with string key %s from hash table\n", key.string);
        }
        rcu_read_unlock();
        return false;
    }

    // Schedule for RCU cleanup after successful removal
    tklog_scope(call_rcu(&p_base->rcu_head, free_callback));

    rcu_read_unlock();

    return true;
}

// Update an existing node with new data (RCU-safe)
bool gd_node_update(struct gd_base_node* new_node) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return false;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key.number == 0) {
        tklog_error("key number is 0\n");
        return false;
    }
    if (new_node->type_key.number == 0) {
        tklog_error("type_key number is 0\n");
        return false;
    }

    rcu_read_lock();
    
    // Find the existing node
    uint64_t                hash = _gd_hash_key(new_node->key, new_node->key_is_number);
    struct gd_key_match_ctx ctx = { .key = new_node->key, .key_is_number = new_node->key_is_number };
    struct cds_lfht_iter    old_iter = {0};
    cds_lfht_lookup(g_p_ht, hash, _gd_key_match, &ctx, &old_iter);
    struct cds_lfht_node*   old_lfht_node = cds_lfht_iter_get_node(&old_iter);
    struct gd_base_node*    old_node = caa_container_of(old_lfht_node, struct gd_base_node, lfht_node);
    if (!old_node) {
        if (new_node->key_is_number) {
            tklog_debug("Cannot update - node with number key %llu not found\n", new_node->key.number);
        } else {
            tklog_debug("Cannot update - node with string key %s not found\n", new_node->key.string);
        }
        rcu_read_unlock();
        return false;
    }

    // Get the type information
    tklog_scope(struct gd_base_node* p_type_base_node = gd_node_get(old_node->type_key, old_node->type_key_is_number));
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for existing node\n");
        rcu_read_unlock();
        return false;
    }

    struct gd_base_type_node* p_type_node = caa_container_of(p_type_base_node, struct gd_base_type_node, base);
    
    // Verify data size matches expected type size 
    if (new_node->size_bytes != p_type_node->type_size) {
        tklog_error("new node size %u doesn't match current node size %u\n", new_node->size_bytes, p_type_node->type_size);
        rcu_read_unlock();
        return false;
    }

    // Get the free callback while still holding the read lock
    void (*free_callback)(struct rcu_head*) = p_type_node->fn_free_node_callback;
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        rcu_read_unlock();
        return false;
    }

    int replace_result = cds_lfht_replace(g_p_ht, &old_iter, hash, _gd_key_match, &ctx, &new_node->lfht_node);
    
    if (replace_result == 0) {
        // Schedule old node for RCU cleanup
        tklog_scope(call_rcu(&old_node->rcu_head, free_callback));
        
        if (new_node->key_is_number) {
            tklog_debug("Successfully updated node with number key %llu\n", new_node->key.number);
        } else {
            tklog_debug("Successfully updated node with string key %s\n", new_node->key.string);
        }

        rcu_read_unlock();
        return true;
    } else {
        tklog_error("Failed to replace node in hash table\n");
        rcu_read_unlock();
        return false;
    }
}

// Insert or update a node (RCU-safe)
bool gd_node_upsert(struct gd_base_node* new_node) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return false;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key.number == 0) {
        tklog_error("key number is 0\n");
        return false;
    }
    if (new_node->type_key.number == 0) {
        tklog_error("type_key number is 0\n");
        return false;
    }

    rcu_read_lock();
    tklog_scope(struct gd_base_node* old_node = gd_node_get(new_node->key, new_node->key_is_number));
    rcu_read_unlock();
    if (old_node) {
        tklog_scope(bool update_result = gd_node_update(new_node));
        if (update_result) {
            return true;
        }
        return false;
    } else {
        tklog_scope(bool insert_result = gd_node_insert(new_node));
        return insert_result;
    }
}

// Wrapper functions for URCU hash table operations
bool gd_node_is_deleted(struct gd_base_node* node) {
    if (!node) {
        return true; // NULL node is considered deleted
    }
    tklog_scope(bool result = cds_lfht_is_node_deleted(&node->lfht_node));
    return result;
}

// Iterator functions for hash table traversal
bool gd_iter_first(struct cds_lfht_iter* iter) {
    if (!g_p_ht || !iter) {
        tklog_error("gd_iter_first called with NULL parameters or uninitialized system\n");
        return false;
    }
    
    tklog_scope(cds_lfht_first(g_p_ht, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}

bool gd_iter_next(struct cds_lfht_iter* iter) {
    if (!g_p_ht || !iter) {
        tklog_error("gd_iter_next called with NULL parameters or uninitialized system\n");
        return false;
    }
    
    tklog_scope(cds_lfht_next(g_p_ht, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}

struct gd_base_node* gd_iter_get_node(struct cds_lfht_iter* iter) {
    if (!iter) {
        return NULL;
    }
    
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    if (lfht_node) {
        return caa_container_of(lfht_node, struct gd_base_node, lfht_node);
    }
    return NULL;
}

bool gd_iter_lookup(union gd_key key, bool key_is_number, struct cds_lfht_iter* iter) {
    if (!g_p_ht || !iter) {
        tklog_error("gd_lookup_iter called with NULL parameters\n");
        return false;
    }
    if (key.number == 0) {
        tklog_error("number key is 0\n");
        return false;
    }
    
    // Calculate hash
    uint64_t hash = _gd_hash_key(key, key_is_number);
    
    struct gd_key_match_ctx ctx = { .key = key, .key_is_number = key_is_number };
    
    tklog_scope(cds_lfht_lookup(g_p_ht, hash, _gd_key_match, &ctx, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}

unsigned long gd_nodes_count(void) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return 0;
    }
    
    long split_count_before, split_count_after;
    unsigned long count;
    
    // cds_lfht_count_nodes MUST be called from within read-side critical section
    // according to URCU specification
    rcu_read_lock();
    tklog_scope(cds_lfht_count_nodes(g_p_ht, &split_count_before, &count, &split_count_after));
    rcu_read_unlock();
    
    return count;
}

bool gd_key_free(union gd_key key, bool key_is_number) {
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
// Helper functions to create gd_key unions
// if key is string then the string will be copied
union gd_key gd_key_create(uint64_t number_key, const char* string_key, bool key_is_number) {
    if (key_is_number && number_key == 0) {
        tklog_error("number key is 0\n");
        return (union gd_key){ .number = 0 };
    }
    if (!key_is_number && string_key == NULL) {
        tklog_error("string key is NULL\n");
        return (union gd_key){ .string = NULL };
    }
    union gd_key key;
    if (key_is_number) {
        key.number = number_key;
    } else {
        // Validate and copy string key
        if (strlen(string_key) == 0) {
            tklog_error("string key is empty\n");
            return (union gd_key){ .string = NULL };
        }
        
        size_t key_len = strlen(string_key) + 1;
        if (key_len > MAX_STRING_KEY_LEN) {
            tklog_error("string key too long (max %d characters)\n", MAX_STRING_KEY_LEN - 1);
            return (union gd_key){ .string = NULL };
        }
        
        char* copied_string = calloc(1, key_len);
        if (!copied_string) {
            tklog_error("Failed to allocate memory for string key\n");
            return (union gd_key){ .string = NULL };
        }
        
        if (strncpy(copied_string, string_key, key_len - 1) == NULL) {
            tklog_error("Failed to copy string key\n");
            free(copied_string);
            return (union gd_key){ .string = NULL };
        }
        copied_string[key_len - 1] = '\0'; // Ensure null termination
        
        key.string = copied_string;
    }
    return key;
}