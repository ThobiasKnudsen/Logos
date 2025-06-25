#ifndef GLOBAL_DATA_TYPE_H
#define GLOBAL_DATA_TYPE_H

#include "core.h"

struct gd_node_base_type {
    struct gd_base_node base; // type_key will be set to point to the base_type node
    bool (*fn_free_node)(struct gd_base_node*); // node to free by base node
    void (*fn_free_node_callback)(struct rcu_head*); // node to free by base node as callback which should call fn_free_node
    bool (*fn_is_valid)(struct gd_base_node*); // node to check is valid by base node
    uint32_t type_size; // bytes
    // probably more function will come
};

// Node type functions - creates a new type that uses base_type as its type_key
const char* gd_create_node_type(const char* type_name,
                                uint32_t type_size, 
                                bool (*fn_free_node)(struct gd_base_node*),
                                void (*fn_free_node_callback)(struct rcu_head*),
                                bool (*fn_is_valid)(struct gd_base_node*));

// Gets the base_type node key (the type that all types use as their type)
const char* gd_get_base_type_key(void);

// Internal initialization function for setting up the single base_type node
bool _gd_init_base_type(void);

// Internal cleanup callback for base_type nodes
void _gd_node_base_type_free_callback(struct rcu_head* rcu_head);

#endif /* GLOBAL_DATA_TYPE_H */
