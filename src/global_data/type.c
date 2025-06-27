#include "global_data/type.h"
#include "global_data/core.h"
#include "tklog.h"
#include <urcu.h>

#include <stdatomic.h>

static const char* g_base_type_key = "base_type";

// Default free function for node type nodes
static bool _gd_node_base_type_free(struct gd_base_node* node) {
    if (!node) return false;
    
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
void _gd_node_base_type_free_callback(struct rcu_head* rcu_head) {
    struct gd_base_node* node = caa_container_of(rcu_head, struct gd_base_node, rcu_head);
    _gd_node_base_type_free(node);
}

// Default validation function for node type nodes
static bool _gd_node_base_type_is_valid(struct gd_base_node* node) {
    if (!node) return false;
    
    // Basic validation - check if it's a node type node
    struct gd_node_base_type* node_type = caa_container_of(node, struct gd_node_base_type, base);
    bool (*free_node)(struct gd_base_node*) = rcu_dereference(node_type->fn_free_node);
    void (*free_callback)(struct rcu_head*) = rcu_dereference(node_type->fn_free_node_callback);
    bool (*is_valid)(struct gd_base_node*) = rcu_dereference(node_type->fn_is_valid);
    return (node_type->type_size >= sizeof(struct gd_base_node) && free_node != NULL && free_callback != NULL);
}

// Initialize the base_type node
bool _gd_init_base_type(void) {
    // Create the base_type node that has itself as type
    struct gd_node_base_type* base_type = malloc(sizeof(struct gd_node_base_type));
    if (!base_type) {
        tklog_error("Failed to allocate memory for base_type node\n");
        return false;
    }
    
    // Initialize the base_type node
    tklog_scope(cds_lfht_node_init(&base_type->base.lfht_node));
    
    // Set up the key (string key for base_type)
    base_type->base.key_is_number = false;
    char* key_str = malloc(strlen(g_base_type_key) + 1);
    if (!key_str) {
        tklog_error("Failed to allocate memory for base_type key\n");
        free(base_type);
        return false;
    }
    strcpy(key_str, g_base_type_key);
    base_type->base.key.string = key_str;
    
    // The base_type has itself as type (this creates the bootstrap)
    base_type->base.type_key_is_number = false;
    char* type_key_str = malloc(strlen(g_base_type_key) + 1);
    if (!type_key_str) {
        tklog_error("Failed to allocate memory for base_type type_key\n");
        free(key_str);
        free(base_type);
        return false;
    }
    strcpy(type_key_str, g_base_type_key);
    base_type->base.type_key.string = type_key_str;
    
    base_type->fn_free_node = _gd_node_base_type_free;
    base_type->fn_free_node_callback = _gd_node_base_type_free_callback;
    base_type->fn_is_valid = _gd_node_base_type_is_valid;
    base_type->type_size = sizeof(struct gd_node_base_type); // this correct because the nodes this is type for must have the same fields but the type_size for the nodes this is type for would likely have a different size because at that point you narrowing down the data more and more for specific cases.
    base_type->base.size_bytes = sizeof(struct gd_node_base_type);
    
    // Insert the base_type node into the hash table using bootstrap function
    extern struct cds_lfht_node* _gd_add_unique_bootstrap(const union gd_key* key, bool key_is_number, struct cds_lfht_node* node);
    
    union gd_key base_type_key = gd_create_string_key(g_base_type_key);
    
    // During bootstrap, we don't need read locks since we're doing write operations
    tklog_scope(struct cds_lfht_node* result = _gd_add_unique_bootstrap(&base_type_key, false, &base_type->base.lfht_node));
    
    if (result != &base_type->base.lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        free(base_type->base.key.string);
        free(base_type->base.type_key.string);
        free(base_type);
        return false;
    }
    
    tklog_info("Created base_type node with key: %s, size: %u\n", 
               g_base_type_key, base_type->type_size);
    
    return true;
}

const char* gd_get_base_type_key(void) {
    return g_base_type_key;
}

// Create a node type in the global data system (uses base_type as its type_key)
const char* gd_create_node_type(const char* type_name,
                                uint32_t type_size, 
                                bool (*fn_free_node)(struct gd_base_node*),
                                void (*fn_free_node_callback)(struct rcu_head*),
                                bool (*fn_is_valid)(struct gd_base_node*)) 
{
    if (!type_name || strlen(type_name) == 0) {
        tklog_error("Type name cannot be NULL or empty\n");
        return NULL;
    }
    
    // Don't allow creating another base_type
    if (strcmp(type_name, g_base_type_key) == 0) {
        tklog_error("Cannot create base_type - it already exists and is unique\n");
        return NULL;
    }
    
    if (0 <= type_size && type_size <= sizeof(struct gd_node_base_type)) {
        tklog_error("Type size cannot be between 0 and %d\n", sizeof(struct gd_node_base_type));
        return NULL;
    }
    
    if (!fn_free_node || !fn_free_node_callback) {
        tklog_error("Free function callbacks cannot be NULL\n");
        return NULL;
    }
    
    // Create a node type
    struct gd_node_base_type* node_type = malloc(sizeof(struct gd_node_base_type));
    if (!node_type) {
        tklog_error("Failed to allocate memory for node type\n");
        return NULL;
    }
    
    // Initialize the node type
    tklog_scope(cds_lfht_node_init(&node_type->base.lfht_node));
    
    // Set up the key (string key for node types)
    node_type->base.key_is_number = false;
    char* key_str = malloc(strlen(type_name) + 1);
    if (!key_str) {
        tklog_error("Failed to allocate memory for node type key\n");
        free(node_type);
        return NULL;
    }
    strcpy(key_str, type_name);
    node_type->base.key.string = key_str;
    
    // All node types (except base_type itself) use the base_type as their type
    node_type->base.type_key_is_number = false;
    char* type_key_str = malloc(strlen(g_base_type_key) + 1);
    if (!type_key_str) {
        tklog_error("Failed to allocate memory for node type type_key\n");
        free(node_type->base.key.string);
        free(node_type);
        return NULL;
    }
    strcpy(type_key_str, g_base_type_key);
    node_type->base.type_key.string = type_key_str;
    
    node_type->fn_free_node = fn_free_node;
    node_type->fn_free_node_callback = fn_free_node_callback;
    node_type->fn_is_valid = fn_is_valid;
    node_type->type_size = type_size;
    node_type->base.size_bytes = type_size;
    
    // Insert the node type into the hash table using bootstrap function
    extern struct cds_lfht_node* _gd_add_unique_bootstrap(const union gd_key* key, bool key_is_number, struct cds_lfht_node* node);
    
    union gd_key node_type_key = gd_create_string_key(type_name);
    
    // During node type creation, we don't need read locks since we're doing write operations
    tklog_scope(struct cds_lfht_node* result = _gd_add_unique_bootstrap(&node_type_key, false, &node_type->base.lfht_node));
    
    if (result != &node_type->base.lfht_node) {
        tklog_error("Node type with key %s already exists\n", type_name);
        free(node_type->base.key.string);
        free(node_type->base.type_key.string);
        free(node_type);
        return NULL;
    }
    
    tklog_info("Created node type with key: %s, size: %u\n", type_name, type_size);
    
    return type_name;
}