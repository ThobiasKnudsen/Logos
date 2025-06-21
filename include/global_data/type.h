#ifndef GLOBAL_DATA_TYPE_H
#define GLOBAL_DATA_TYPE_H

#include "core.h"

struct gd_type_node {
    struct gd_base_node base; // type_key will be set to point to the fundamental type node
    bool (*fn_free_node)(struct gd_base_node*); // node to free by base node
    void (*fn_free_node_callback)(struct rcu_head*); // node to free by base node as callback which should call fn_free_node
    bool (*fn_is_valid)(struct gd_base_node*); // node to check is valid by base node
    uint32_t type_size; // bytes
    // probably more function will come
};

// Type node functions
uint64_t gd_create_type_node(uint32_t type_size, 
                            bool (*fn_free_node)(struct gd_base_node*),
                            void (*fn_free_node_callback)(struct rcu_head*),
                            bool (*fn_is_valid)(struct gd_base_node*));

// Gets the fundamental type node key (the type that all types use as their type)
uint64_t gd_get_fundamental_type_key(void);

// Internal initialization function for setting up the fundamental type
bool _gd_init_fundamental_type(void);

#endif /* GLOBAL_DATA_TYPE_H */
