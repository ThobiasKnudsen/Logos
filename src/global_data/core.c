#include "global_data/core.h"
#include "global_data/type.h"
#include "global_data/urcu_safe.h"
#include "tklog.h"
#include "xxhash.h"

#include <stdatomic.h>

static struct cds_lfht *g_p_ht = NULL;
static _Atomic uint64_t g_key_counter = 1; // 0 is invalid. 1 is the first valid key

// Unified key matching function that handles both number and string keys
int _gd_key_match(struct cds_lfht_node *node, const void *key_ctx) {
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

uint64_t _gd_hash_string(const char* string) {
    return XXH3_64bits(string, strlen(string));
}

uint64_t _gd_hash_u64(uint64_t key) {
    return XXH3_64bits(&key, sizeof(uint64_t));
}

// Hash function for gd_key union
uint64_t _gd_hash_key(const union gd_key* key, bool key_is_number) {
    if (key_is_number) {
        return _gd_hash_u64(key->number);
    } else {
        return _gd_hash_string(key->string);
    }
}

// Initialize the global data system
bool gd_init(void) {
    if (g_p_ht != NULL) {
        tklog_warning("Global data system already initialized\n");
        return true;
    }
    
    // Register the node size function with urcu_safe system
    urcu_safe_set_node_size_function(_gd_get_node_size);
    
    // Register the node bounds functions with urcu_safe system
    urcu_safe_set_node_start_ptr_function(_gd_get_node_start_ptr);
    
    tklog_scope(g_p_ht = cds_lfht_new(8, 8, 0, CDS_LFHT_AUTO_RESIZE, NULL));
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

// Cleanup the global data system
void gd_cleanup(void) {
    if (!g_p_ht) {
        return;
    }
    
    rcu_register_thread();
    
    // Phase 1: Process non-type nodes in batches until none are left
    const int batch_size = 1000;
    struct cds_lfht_node* nodes_to_delete[batch_size];
    int total_deleted = 0;
    
    tklog_info("Phase 1: Cleaning up non-type nodes\n");
    while (true) {
        int delete_count = 0;
        struct cds_lfht_iter iter;
        
        // Collect a batch of non-type nodes
        rcu_read_lock();
        cds_lfht_first(g_p_ht, &iter);
        
        struct cds_lfht_node* node;
        while ((node = cds_lfht_iter_get_node(&iter)) != NULL && delete_count < batch_size) {
            struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
            
            // Check if this is a non-type node (not base_type and not a type node)
            bool is_base_type = (!base_node->key_is_number && strcmp(rcu_dereference(base_node->key.string), "base_type") == 0);
            bool is_type_node = false;
            
            if (!is_base_type && !base_node->type_key_is_number) {
                // Check if this node's type is base_type (making it a type node)
                is_type_node = (strcmp(rcu_dereference(base_node->type_key.string), "base_type") == 0);
            }
            
            if (!is_base_type && !is_type_node) {
                // Store this node for deletion
                nodes_to_delete[delete_count++] = node;
            }
            
            // Move to next node
            cds_lfht_next(g_p_ht, &iter);
        }
        rcu_read_unlock();
        
        // If no non-type nodes found in this batch, we're done with phase 1
        if (delete_count == 0) {
            break;
        }
        
        // Delete all nodes in this batch
        for (int i = 0; i < delete_count; i++) {
            struct gd_base_node* base_node = caa_container_of(nodes_to_delete[i], struct gd_base_node, lfht_node);
            
            // Remove from hash table
            if (cds_lfht_del(g_p_ht, nodes_to_delete[i]) == 0) {
                // Get the type node to find the correct free callback
                rcu_read_lock();
                struct gd_base_node* type_base = gd_get_node_unsafe(&base_node->type_key, base_node->type_key_is_number);
                
                if (type_base) {
                    struct gd_node_base_type* type_node = caa_container_of(type_base, struct gd_node_base_type, base);
                    void (*free_callback)(struct rcu_head*) = rcu_dereference(type_node->fn_free_node_callback);
                    rcu_read_unlock();
                    call_rcu(&base_node->rcu_head, free_callback);
                } else {
                    // Fallback to default callback if type not found
                    rcu_read_unlock();
                    call_rcu(&base_node->rcu_head, _gd_node_base_type_free_callback);
                }
                total_deleted++;
            } else {
                tklog_warning("Failed to delete non-type node during cleanup\n");
            }
        }
        
        // Wait for this batch's call_rcu callbacks to finish before processing the next batch
        rcu_barrier();
    }
    
    if (total_deleted > 0) {
        tklog_info("Phase 1: Cleaned up %d non-type nodes\n", total_deleted);
    }
    
    // Phase 2: Process type nodes (except base_type) in batches until none are left
    total_deleted = 0;
    
    tklog_info("Phase 2: Cleaning up type nodes\n");
    while (true) {
        int delete_count = 0;
        struct cds_lfht_iter iter;
        
        // Collect a batch of type nodes
        rcu_read_lock();
        cds_lfht_first(g_p_ht, &iter);
        
        struct cds_lfht_node* node;
        while ((node = cds_lfht_iter_get_node(&iter)) != NULL && delete_count < batch_size) {
            struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
            
            // Check if this is a type node (not base_type)
            bool is_base_type = (!base_node->key_is_number && strcmp(rcu_dereference(base_node->key.string), "base_type") == 0);
            bool is_type_node = false;
            
            if (!is_base_type && !base_node->type_key_is_number) {
                is_type_node = (strcmp(rcu_dereference(base_node->type_key.string), "base_type") == 0);
            }
            
            if (!is_base_type && is_type_node) {
                // Store this node for deletion
                nodes_to_delete[delete_count++] = node;
            }
            
            // Move to next node
            cds_lfht_next(g_p_ht, &iter);
        }
        rcu_read_unlock();
        
        // If no type nodes found in this batch, we're done with phase 2
        if (delete_count == 0) {
            break;
        }
        
        // Delete all nodes in this batch
        for (int i = 0; i < delete_count; i++) {
            struct gd_base_node* base_node = caa_container_of(nodes_to_delete[i], struct gd_base_node, lfht_node);
            
            // Remove from hash table
            if (cds_lfht_del(g_p_ht, nodes_to_delete[i]) == 0) {
                call_rcu(&base_node->rcu_head, _gd_node_base_type_free_callback);
                total_deleted++;
            } else {
                tklog_warning("Failed to delete type node during cleanup\n");
            }
        }
        
        // Wait for this batch's call_rcu callbacks to finish before processing the next batch
        rcu_barrier();
    }
    
    if (total_deleted > 0) {
        tklog_info("Phase 2: Cleaned up %d type nodes\n", total_deleted);
    }
    
    // Phase 3: Remove the base_type node (should be the last remaining node)
    tklog_info("Phase 3: Cleaning up base_type node\n");
    rcu_read_lock();
    struct cds_lfht_iter iter;
    cds_lfht_first(g_p_ht, &iter);
    struct cds_lfht_node* node = cds_lfht_iter_get_node(&iter);
    
    if (node) {
        struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
        
        // This should be the base_type node
        if (!base_node->key_is_number && strcmp(base_node->key.string, "base_type") == 0) {
            rcu_read_unlock(); // Release read lock before delete operation
            
            if (cds_lfht_del(g_p_ht, node) == 0) {
                call_rcu(&base_node->rcu_head, _gd_node_base_type_free_callback);
                tklog_info("Phase 3: Cleaned up base_type node\n");
            }
        } else {
            tklog_warning("Expected base_type node but found different node during cleanup\n");
            rcu_read_unlock();
        }
    } else {
        rcu_read_unlock();
    }
    
    // Wait for all call_rcu callbacks to finish
    rcu_barrier();
    
    // Only unregister if we registered this thread (don't unregister if it was already registered)
    rcu_unregister_thread();
    
    // Now destroy the hash table
    cds_lfht_destroy(g_p_ht, NULL);
    g_p_ht = NULL;
    
    tklog_info("Global data system cleanup completed\n");
}

// Get key counter for internal use
uint64_t _gd_get_next_key(void) {
    return atomic_fetch_add(&g_key_counter, 1);
}

// Getter function for the global hash table (for test access)
struct cds_lfht* _gd_get_hash_table(void) {
    return g_p_ht;
}

// Internal function for type.c to add nodes directly to hash table during bootstrap
struct cds_lfht_node* _gd_add_unique_bootstrap(const union gd_key* key, bool key_is_number, struct cds_lfht_node* node) {
    if (!g_p_ht || !key || !node) {
        return NULL;
    }
    
    uint64_t hash = _gd_hash_key(key, key_is_number);
    
    struct gd_key_match_ctx ctx = { .key = *key, .key_is_number = key_is_number };
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(g_p_ht, hash, _gd_key_match, &ctx, node));
    return result;
}

// Internal function - moved from header to be internal only
static void gd_node_init(struct gd_base_node* node) {
    if (!node) {
        tklog_error("gd_node_init called with NULL node\n");
        return;
    }
    tklog_scope(cds_lfht_node_init(&node->lfht_node));
}

// Internal function for adding nodes uniquely (used internally by gd_create_node)
static struct gd_base_node* _gd_add_unique_internal(const union gd_key* key, bool key_is_number, struct gd_base_node* node) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return NULL;
    }
    if (!key || !node) {
        tklog_error("_gd_add_unique_internal called with NULL parameters\n");
        return NULL;
    }
    
    // Calculate hash internally
    uint64_t hash = _gd_hash_key(key, key_is_number);
    
    struct gd_key_match_ctx ctx = { .key = *key, .key_is_number = key_is_number };
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(g_p_ht, hash, _gd_key_match, &ctx, &node->lfht_node));
    
    if (result == &node->lfht_node) {
        return node; // Success - our node was inserted
    } else {
        // Another node with the same key already exists
        return caa_container_of(result, struct gd_base_node, lfht_node);
    }
}

// Main node access function - must be used with RCU read lock
struct gd_base_node* gd_get_node_unsafe(const union gd_key* key, bool key_is_number) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return NULL;
    }
    if (!key) {
        tklog_error("gd_get_node_unsafe called with NULL key\n");
        return NULL;
    }
    if (key_is_number && key->number == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }
    if (key_is_number) {
        tklog_debug("Looking up number key %llu\n", key->number);
    } else {
        tklog_debug("Looking up string key %s\n", key->string);
    }
    
    // Calculate hash internally
    uint64_t hash = _gd_hash_key(key, key_is_number);
    
    struct gd_key_match_ctx ctx = { .key = *key, .key_is_number = key_is_number };
    struct cds_lfht_iter iter;
    
    // Temporarily disable safety checks since this function is designed to be called
    // from within a read lock section
    // bool safety_checks_enabled = _rcu_are_safety_checks_enabled();
    // if (safety_checks_enabled) {
    //     _rcu_disable_safety_checks();
    // }
    
    cds_lfht_lookup(g_p_ht, hash, _gd_key_match, &ctx, &iter);
    struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(&iter);
    
    // Re-enable safety checks if they were enabled before
    // if (safety_checks_enabled) {
    //     _rcu_enable_safety_checks();
    // }
    
    if (lfht_node) {
        struct gd_base_node* result = caa_container_of(lfht_node, struct gd_base_node, lfht_node);
        if (key_is_number) {
            tklog_debug("Found node for number key %llu\n", key->number);
        } else {
            tklog_debug("Found node for string key %s\n", key->string);
        }
        return result;
    }
    
    if (key_is_number) {
        tklog_debug("Node for number key %llu not found\n", key->number);
    } else {
        tklog_debug("Node for string key %s not found\n", key->string);
    }
    return NULL;
}

// returns 1 on success for string keys, the newly created numeric key for auto-generated keys, or 0 on failure
uint64_t gd_create_node(const union gd_key* key, bool key_is_number, const union gd_key* type_key, bool type_key_is_number) {
    if (!type_key) {
        tklog_error("type_key is NULL\n");
        return 0;
    }
    if (type_key_is_number && type_key->number == 0) {
        tklog_error("type_key number is 0\n");
        return 0;
    }
    if (!key_is_number && !key) {
        tklog_error("string key is NULL\n");
        return 0;
    }

    // Look up the type node
    uint64_t type_key_hash = _gd_hash_key(type_key, type_key_is_number);

    tklog_scope(rcu_read_lock());

    tklog_scope(struct gd_base_node* p_base_type = gd_get_node_unsafe(type_key, type_key_is_number));

    if (!p_base_type) {
        if (type_key_is_number) {
            tklog_notice("didnt find node_type for given type_key: %lld\n", type_key->number);
        } else {
            tklog_notice("didnt find node_type for given type_key: %s\n", type_key->string);
        }
        tklog_scope(rcu_read_unlock());
        return 0;
    }

    struct gd_node_base_type* p_type_node = caa_container_of(p_base_type, struct gd_node_base_type, base);
    struct gd_base_node* p_new_node = malloc(p_type_node->type_size);

    tklog_scope(rcu_read_unlock());

    if (!p_new_node) {
        tklog_error("malloc failed to allocate %d bytes\n", p_type_node->type_size);
        return 0;
    }

    tklog_scope(gd_node_init(p_new_node));
    p_new_node->key_is_number = key_is_number;
    p_new_node->type_key_is_number = type_key_is_number;
    p_new_node->size_bytes = p_type_node->type_size;
    
    // Initialize union fields to prevent uninitialized pointer access
    p_new_node->key.string = NULL;
    p_new_node->type_key.string = NULL;
    
    // Set up the key
    uint64_t actual_key = 0;
    if (key_is_number) {
        if (key) {
            // Use provided numeric key
            actual_key = key->number;
        } else {
            // Auto-generate numeric key
            actual_key = atomic_fetch_add(&g_key_counter, 1);
        }
        // Store number key in union
        p_new_node->key.number = actual_key;
    } else {
        // String key
        const char* str_key = key->string;
        char* new_key_str = malloc(strlen(str_key) + 1);
        if (!new_key_str) {
            tklog_error("malloc failed to allocate string key\n");
            free(p_new_node);
            return 0;
        }
        strcpy(new_key_str, str_key);
        p_new_node->key.string = new_key_str;
    }
    
    // Set up the type key
    if (type_key_is_number) {
        // Store number type key in union
        p_new_node->type_key.number = type_key->number;
    } else {
        const char* str_type_key = type_key->string;
        char* new_type_key_str = malloc(strlen(str_type_key) + 1);
        if (!new_type_key_str) {
            tklog_error("malloc failed to allocate string type key\n");
            if (!key_is_number) {
                free(p_new_node->key.string); // Only free if string key
            }
            free(p_new_node);
            return 0;
        }
        strcpy(new_type_key_str, str_type_key);
        p_new_node->type_key.string = new_type_key_str;
    }

    // Log what we're creating
    if (key_is_number) {
        tklog_debug("Creating node with number key %llu\n", actual_key);
    } else {
        tklog_debug("Creating node with string key %s\n", key ? key->string : "NULL");
    }

    // Insert the new node into the hash table (no read lock needed for write operations)
    tklog_scope(struct gd_base_node* p_same_node = _gd_add_unique_internal(&p_new_node->key, key_is_number, p_new_node));
    
    if (p_same_node != p_new_node) {
        if (key_is_number) {
            tklog_critical("Somehow node with number key %llu already exists\n", actual_key);
        } else {
            tklog_critical("Somehow node with string key %s already exists\n", key ? key->string : "NULL");
        }
        // Free allocated memory (only for string keys)
        if (!key_is_number) {
            free(p_new_node->key.string);
        }
        if (!type_key_is_number) {
            free(p_new_node->type_key.string);
        }
        free(p_new_node);
        return 0;
    }

    if (key_is_number) {
        tklog_debug("Successfully inserted node with number key %llu\n", actual_key);
        return actual_key;
    } else {
        tklog_debug("Successfully inserted node with string key %s\n", key ? key->string : "NULL");
        return 1; // Success for string keys
    }
}

bool gd_remove_node(const union gd_key* key, bool key_is_number) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return false;
    }
    if (!key) {
        tklog_error("key is NULL\n");
        return false;
    }
    if (key_is_number && key->number == 0) {
        tklog_error("trying to delete node by number key which is 0\n");
        return false;
    }

    tklog_scope(rcu_read_lock());
    
    // Find the node to remove
    tklog_scope(struct gd_base_node* p_base = gd_get_node_unsafe(key, key_is_number));
    if (!p_base) {
        if (key_is_number) {
            tklog_debug("Cannot remove - node with number key %llu not found\n", key->number);
        } else {
            tklog_debug("Cannot remove - node with string key %s not found\n", key->string);
        }
        tklog_scope(rcu_read_unlock());
        return false;
    }

    // Get the type information
    struct gd_base_node* p_type_base_node = gd_get_node_unsafe(&p_base->type_key, p_base->type_key_is_number);
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for node to be removed\n");
        tklog_scope(rcu_read_unlock());
        return false;
    }

    struct gd_node_base_type* p_type_node = caa_container_of(p_type_base_node, struct gd_node_base_type, base);
    
    // Copy the free callback while still holding the read lock
    // This ensures the callback remains valid even after we release the lock
    void (*free_callback)(struct rcu_head*) = rcu_dereference(p_type_node->fn_free_node_callback);
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(rcu_read_unlock());
        return false;
    }
    
    // Release read lock before doing write operations
    tklog_scope(rcu_read_unlock());

    // Remove from hash table (write operation - no read lock needed)
    if (cds_lfht_del(g_p_ht, &p_base->lfht_node) != 0) {
        if (key_is_number) {
            tklog_error("Failed to delete node with number key %llu from hash table\n", key->number);
        } else {
            tklog_error("Failed to delete node with string key %s from hash table\n", key->string);
        }
        return false;
    }

    // Schedule for RCU cleanup after successful removal
    tklog_scope(call_rcu(&p_base->rcu_head, free_callback));

    return true;
}

// Update an existing node with new data (RCU-safe)
bool gd_update(const union gd_key* key, bool key_is_number, void* new_data, size_t data_size) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return false;
    }
    if (!key || !new_data) {
        tklog_error("key or new_data is NULL\n");
        return false;
    }
    if (key_is_number && key->number == 0) {
        tklog_error("number key is 0\n");
        return false;
    }

    tklog_scope(rcu_read_lock());
    
    // Find the existing node
    struct gd_base_node* p_existing = gd_get_node_unsafe(key, key_is_number);
    if (!p_existing) {
        if (key_is_number) {
            tklog_debug("Cannot update - node with number key %llu not found\n", key->number);
        } else {
            tklog_debug("Cannot update - node with string key %s not found\n", key->string);
        }
        tklog_scope(rcu_read_unlock());
        return false;
    }

    // Get the type information
    struct gd_base_node* p_type_base_node = gd_get_node_unsafe(&p_existing->type_key, p_existing->type_key_is_number);
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for existing node\n");
        tklog_scope(rcu_read_unlock());
        return false;
    }

    struct gd_node_base_type* p_type_node = caa_container_of(p_type_base_node, struct gd_node_base_type, base);
    
    // Verify data size matches expected type size 
    if (data_size != p_type_node->type_size) {
        tklog_error("Data size %zu doesn't match type size %u\n", data_size, p_type_node->type_size);
        tklog_scope(rcu_read_unlock());
        return false;
    }

    // Get the free callback while still holding the read lock
    void (*free_callback)(struct rcu_head*) = rcu_dereference(p_type_node->fn_free_node_callback);
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(rcu_read_unlock());
        return false;
    }

    // Create new node
    struct gd_base_node* p_new_node = malloc(p_type_node->type_size);
    if (!p_new_node) {
        tklog_error("Failed to allocate memory for updated node\n");
        tklog_scope(rcu_read_unlock());
        return false;
    }

    // Copy the new data to the new node
    memcpy(p_new_node, new_data, data_size);
    
    // Initialize the LFHT node part
    tklog_scope(gd_node_init(p_new_node));
    
    // Copy key information from existing node
    p_new_node->key_is_number = p_existing->key_is_number;
    p_new_node->type_key_is_number = p_existing->type_key_is_number;
    p_new_node->size_bytes = p_existing->size_bytes;
    
    // Initialize union fields to prevent uninitialized pointer access
    p_new_node->key.string = NULL;
    p_new_node->type_key.string = NULL;
    
    // Set up the key
    if (key_is_number) {
        p_new_node->key.number = key->number;
    } else {
        const char* str_key = key->string;
        char* new_key_str = malloc(strlen(str_key) + 1);
        if (!new_key_str) {
            tklog_error("Failed to allocate string key for updated node\n");
            free(p_new_node);
            tklog_scope(rcu_read_unlock());
            return false;
        }
        strcpy(new_key_str, str_key);
        p_new_node->key.string = new_key_str;
    }
    
    // Set up the type key
    if (p_existing->type_key_is_number) {
        p_new_node->type_key.number = p_existing->type_key.number;
    } else {
        const char* str_type_key = p_existing->type_key.string;
        char* new_type_key_str = malloc(strlen(str_type_key) + 1);
        if (!new_type_key_str) {
            tklog_error("Failed to allocate string type key for updated node\n");
            if (!key_is_number) {
                free(p_new_node->key.string);
            }
            free(p_new_node);
            tklog_scope(rcu_read_unlock());
            return false;
        }
        strcpy(new_type_key_str, str_type_key);
        p_new_node->type_key.string = new_type_key_str;
    }

    // Release read lock before doing write operations
    tklog_scope(rcu_read_unlock());

    // Calculate hash for replacement
    uint64_t hash = _gd_hash_key(key, key_is_number);

    struct gd_key_match_ctx ctx = { .key = *key, .key_is_number = key_is_number };
    
    // Atomically replace the node (write operation - no read lock needed)
    tklog_scope(struct cds_lfht_node* replaced_node = cds_lfht_add_replace(g_p_ht, hash, _gd_key_match, &ctx, &p_new_node->lfht_node));
    
    if (replaced_node) {
        // Schedule old node for RCU cleanup
        struct gd_base_node* old_base_node = caa_container_of(replaced_node, struct gd_base_node, lfht_node);
        tklog_scope(call_rcu(&old_base_node->rcu_head, free_callback));
        
        if (key_is_number) {
            tklog_debug("Successfully updated node with number key %llu\n", key->number);
        } else {
            tklog_debug("Successfully updated node with string key %s\n", key->string);
        }
    } else {
        tklog_error("Failed to replace node in hash table\n");
        // Clean up the new node we created
        if (!key_is_number) {
            free(p_new_node->key.string);
        }
        if (!p_existing->type_key_is_number) {
            free(p_new_node->type_key.string);
        }
        free(p_new_node);
        return false;
    }

    return true;
}

// Insert or update a node (RCU-safe)
bool gd_upsert(const union gd_key* key, bool key_is_number, const union gd_key* type_key, bool type_key_is_number, void* data, size_t data_size) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return false;
    }
    if (!key || !type_key || !data) {
        tklog_error("key, type_key, or data is NULL\n");
        return false;
    }
    if (key_is_number && key->number == 0) {
        tklog_error("number key is 0\n");
        return false;
    }
    if (type_key_is_number && type_key->number == 0) {
        tklog_error("type_key number is 0\n");
        return false;
    }

    // First try to update existing node
    tklog_scope(bool update_result = gd_update(key, key_is_number, data, data_size));
    if (update_result) {
        return true;
    }

    // Node doesn't exist, create it
    tklog_scope(uint64_t create_result = gd_create_node(key, key_is_number, type_key, type_key_is_number));

    if (create_result == 0) {
        tklog_error("Failed to create new node in upsert\n");
        return false;
    }

    // Now update the newly created node with the provided data
    tklog_scope(bool final_update_result = gd_update(key, key_is_number, data, data_size));
    return final_update_result;
}

// Wrapper functions for URCU hash table operations
bool gd_is_node_deleted(struct gd_base_node* node) {
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

bool gd_lookup_iter(const union gd_key* key, bool key_is_number, struct cds_lfht_iter* iter) {
    if (!g_p_ht || !key || !iter) {
        tklog_error("gd_lookup_iter called with NULL parameters\n");
        return false;
    }
    
    // Calculate hash
    uint64_t hash = _gd_hash_key(key, key_is_number);
    
    struct gd_key_match_ctx ctx = { .key = *key, .key_is_number = key_is_number };
    
    tklog_scope(cds_lfht_lookup(g_p_ht, hash, _gd_key_match, &ctx, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}

unsigned long gd_count_nodes(void) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return 0;
    }
    
    long split_count_before, split_count_after;
    unsigned long count;
    
    // cds_lfht_count_nodes should NOT be called from within read-side critical section
    // This function can be called without holding RCU read lock
    tklog_scope(cds_lfht_count_nodes(g_p_ht, &split_count_before, &count, &split_count_after));
    
    return count;
}

// Helper functions to create gd_key unions
union gd_key gd_create_number_key(uint64_t number) {
    union gd_key key = { .number = number };
    return key;
}

union gd_key gd_create_string_key(const char* string) {
    union gd_key key;
    key.string = (char*)string;
    return key;
}

// Implementation of gd_get_node_size function
uint64_t _gd_get_node_size(struct cds_lfht_node* node) {
    if (!node) {
        return 0;
    }
    
    // Cast to our base node structure
    struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
    
    // Return the size directly from the node
    return base_node->size_bytes;
}

// Implementation of gd_get_node_start_ptr function
void* _gd_get_node_start_ptr(struct cds_lfht_node* node) {
    if (!node) {
        return NULL;
    }
    
    // Cast to our base node structure and return the start pointer
    struct gd_base_node* base_node = caa_container_of(node, struct gd_base_node, lfht_node);
    return (void*)base_node;
}
