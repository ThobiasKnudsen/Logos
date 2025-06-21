#include "global_data/type.h"
#include "global_data/core.h"
#include "tklog.h"

#include <stdatomic.h>

static uint64_t g_fundamental_type_key = 0;

// External functions from core.c that we need (declared in core.h)
// uint64_t _gd_get_next_key(void);
// struct cds_lfht* _gd_get_hash_table(void);

// Default free function for type nodes
static bool _gd_type_node_free(struct gd_base_node* node) {
    if (!node) return false;
    
    // Free key based on its type
    if (node->key) {
        free(node->key);
        node->key = NULL;
    }
    
    // Free type_key based on its type
    if (node->type_key) {
        free(node->type_key);
        node->type_key = NULL;
    }
    
    free(node);
    return true;
}

// Default free callback for type nodes
static void _gd_type_node_free_callback(struct rcu_head* rcu_head) {
    struct gd_base_node* node = caa_container_of(rcu_head, struct gd_base_node, rcu_head);
    _gd_type_node_free(node);
}

// Default validation function for type nodes
static bool _gd_type_node_is_valid(struct gd_base_node* node) {
    if (!node) return false;
    
    // Basic validation - check if it's a type node
    struct gd_type_node* type_node = caa_container_of(node, struct gd_type_node, base);
    return (type_node->type_size > 0 && 
            type_node->fn_free_node != NULL && 
            type_node->fn_free_node_callback != NULL);
}

// Initialize the fundamental type node
bool _gd_init_fundamental_type(void) {
    struct cds_lfht* ht = _gd_get_hash_table();
    if (!ht) {
        tklog_error("Hash table not initialized\n");
        return false;
    }
    
    // Create the fundamental type node that has itself as type
    struct gd_type_node* fundamental_type = malloc(sizeof(struct gd_type_node));
    if (!fundamental_type) {
        tklog_error("Failed to allocate memory for fundamental type node\n");
        return false;
    }
    
    // Initialize the fundamental type node
    cds_lfht_node_init(&fundamental_type->base.lfht_node);
    
    // Set up the key (number key for fundamental type)
    uint64_t fundamental_key = _gd_get_next_key();
    fundamental_type->base.key_is_number = true;
    fundamental_type->base.key = malloc(sizeof(uint64_t));
    if (!fundamental_type->base.key) {
        tklog_error("Failed to allocate memory for fundamental type key\n");
        free(fundamental_type);
        return false;
    }
    *(uint64_t*)fundamental_type->base.key = fundamental_key;
    
    // The fundamental type has itself as type (this creates the bootstrap)
    fundamental_type->base.type_key_is_number = true;
    fundamental_type->base.type_key = malloc(sizeof(uint64_t));
    if (!fundamental_type->base.type_key) {
        tklog_error("Failed to allocate memory for fundamental type type_key\n");
        free(fundamental_type->base.key);
        free(fundamental_type);
        return false;
    }
    *(uint64_t*)fundamental_type->base.type_key = fundamental_key;
    
    fundamental_type->fn_free_node = _gd_type_node_free;
    fundamental_type->fn_free_node_callback = _gd_type_node_free_callback;
    fundamental_type->fn_is_valid = _gd_type_node_is_valid;
    fundamental_type->type_size = sizeof(struct gd_type_node);
    
    // Store the fundamental type key
    g_fundamental_type_key = fundamental_key;
    
    // Insert the fundamental type node into the hash table
    uint64_t type_key_hash = _gd_hash_u64(fundamental_key);
    
    rcu_read_lock();
    struct gd_key_match_ctx {
        const void* key;
        bool key_is_number;
    } ctx = { .key = fundamental_type->base.key, .key_is_number = true };
    struct cds_lfht_node* existing_node = cds_lfht_add_unique(ht, 
                                                              type_key_hash, _gd_key_match, 
                                                              &ctx,
                                                              &fundamental_type->base.lfht_node);
    rcu_read_unlock();
    
    if (existing_node != &fundamental_type->base.lfht_node) {
        tklog_error("Failed to insert fundamental type node into hash table\n");
        free(fundamental_type->base.key);
        free(fundamental_type->base.type_key);
        free(fundamental_type);
        return false;
    }
    
    tklog_info("Created fundamental type node with key: %llu, size: %u\n", 
               fundamental_key, fundamental_type->type_size);
    return true;
}

uint64_t gd_get_fundamental_type_key(void) {
    return g_fundamental_type_key;
}

// Create a type node in the global data system
uint64_t gd_create_type_node(uint32_t type_size, 
                            bool (*fn_free_node)(struct gd_base_node*),
                            void (*fn_free_node_callback)(struct rcu_head*),
                            bool (*fn_is_valid)(struct gd_base_node*)) 
{
    struct cds_lfht* ht = _gd_get_hash_table();
    if (!ht) {
        tklog_error("Global data system not initialized\n");
        return 0;
    }
    
    if (type_size == 0) {
        tklog_error("Type size cannot be 0\n");
        return 0;
    }
    
    if (!fn_free_node || !fn_free_node_callback) {
        tklog_error("Free function callbacks cannot be NULL\n");
        return 0;
    }
    
    // Create a type node
    struct gd_type_node* type_node = malloc(sizeof(struct gd_type_node));
    if (!type_node) {
        tklog_error("Failed to allocate memory for type node\n");
        return 0;
    }
    
    // Initialize the type node
    cds_lfht_node_init(&type_node->base.lfht_node);
    
    // Set up the key (number key for type nodes)
    uint64_t new_key = _gd_get_next_key();
    type_node->base.key_is_number = true;
    type_node->base.key = malloc(sizeof(uint64_t));
    if (!type_node->base.key) {
        tklog_error("Failed to allocate memory for type node key\n");
        free(type_node);
        return 0;
    }
    *(uint64_t*)type_node->base.key = new_key;
    
    // All type nodes use the fundamental type as their type
    type_node->base.type_key_is_number = true;
    type_node->base.type_key = malloc(sizeof(uint64_t));
    if (!type_node->base.type_key) {
        tklog_error("Failed to allocate memory for type node type_key\n");
        free(type_node->base.key);
        free(type_node);
        return 0;
    }
    *(uint64_t*)type_node->base.type_key = g_fundamental_type_key;
    
    type_node->fn_free_node = fn_free_node;
    type_node->fn_free_node_callback = fn_free_node_callback;
    type_node->fn_is_valid = fn_is_valid;
    type_node->type_size = type_size;
    
    // Insert the type node into the hash table
    uint64_t type_key_hash = _gd_hash_u64(new_key);
    
    rcu_read_lock();
    struct gd_key_match_ctx {
        const void* key;
        bool key_is_number;
    } ctx = { .key = type_node->base.key, .key_is_number = true };
    struct cds_lfht_node* existing_node = cds_lfht_add_unique(ht, 
                                                              type_key_hash, _gd_key_match, 
                                                              &ctx,
                                                              &type_node->base.lfht_node);
    rcu_read_unlock();
    
    if (existing_node != &type_node->base.lfht_node) {
        tklog_error("Type node with key %llu already exists\n", new_key);
        free(type_node->base.key);
        free(type_node->base.type_key);
        free(type_node);
        return 0;
    }
    
    tklog_info("Created type node with key: %llu, size: %u\n", new_key, type_size);
    return new_key;
}
