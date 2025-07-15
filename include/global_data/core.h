#ifndef GLOBAL_DATA_CORE_H
#define GLOBAL_DATA_CORE_H

#define _LGPL_SOURCE
#include <urcu.h>
#include <urcu/rculfhash.h>
#include "global_data/urcu_safe.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

// in _gd_get_node_size every type is treated as being gd_node_base_type


// Union for key types
union gd_key {
    uint64_t number;
    char* string;
};

struct gd_base_node {
    struct cds_lfht_node lfht_node; // dont use this directly
    union gd_key key;
    union gd_key type_key;
    bool key_is_number;
    bool type_key_is_number;
    uint32_t size_bytes; // must be at least sizeof(struct gd_base_node)
    struct rcu_head rcu_head; // dont use this directly
};

struct gd_base_type_node {
    struct gd_base_node base; // type_key will be set to point to the base_type node
    bool (*fn_free_node)(struct gd_base_node*); // node to free by base node
    void (*fn_free_node_callback)(struct rcu_head*); // node to free by base node as callback which should call fn_free_node
    bool (*fn_is_valid)(struct gd_base_node*); // node to check is valid by base node
    uint32_t type_size; // bytes
};

union gd_key            gd_base_type_get_key_copy(void);
bool                    gd_base_type_key_is_number(void);

// Core system functions
bool                    gd_init(void);
void                    gd_cleanup(void);

// allocates 0 initialized memory for the given size and fills in base node fields then you must fill in the rest
// this is zero-copy so you must be careful with the string key pointers
// when key is 0 it will find a unique non-0 number key
struct gd_base_node*    gd_base_node_create(
                            union gd_key key, 
                            bool key_is_number, 
                            union gd_key type_key, 
                            bool type_key_is_number, 
                            uint32_t size_bytes);
// should only be used inside free callback function called with call_rcu
bool                    gd_base_node_free(struct gd_base_node* p_base_node);

// must use rcu_read_lock before and rcu_read_unlock after use
// READ ONLY unless you use rcu_dereference and rcu_assign_pointer for any pointer fields
struct gd_base_node*    gd_node_get(union gd_key key, bool key_is_number);
// write functions. this is zero-copy so be careful with the pointers
bool                    gd_node_insert(struct gd_base_node* new_node);
bool                    gd_node_remove(union gd_key key, bool key_is_number);
bool                    gd_node_update(struct gd_base_node* new_node);
bool                    gd_node_upsert(struct gd_base_node* new_node);
// Utility functions
bool                    gd_node_is_deleted(struct gd_base_node* node);
unsigned long           gd_nodes_count(void);

// Iterator functions for hash table traversal
bool                    gd_iter_first(struct cds_lfht_iter* iter);
bool                    gd_iter_next(struct cds_lfht_iter* iter);
struct gd_base_node*    gd_iter_get_node(struct cds_lfht_iter* iter);
bool                    gd_iter_lookup(union gd_key key, bool key_is_number, struct cds_lfht_iter* iter);

// Helper functions for gd_key unions
union gd_key            gd_key_create(uint64_t number_key, const char* string_key, bool key_is_number);
bool                    gd_key_free(union gd_key key, bool key_is_number);
uint64_t                gd_key_get_number(union gd_key, bool key_is_number);
const char*             gd_key_get_string(union gd_key, bool key_is_number);

#endif /* GLOBAL_DATA_CORE_H */