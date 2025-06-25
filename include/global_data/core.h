#ifndef GLOBAL_DATA_CORE_H
#define GLOBAL_DATA_CORE_H

#include <urcu.h>
#include <urcu/rculfhash.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include "global_data/urcu_safe.h"

// in _gd_get_node_size every type is treated as being gd_node_base_type


// Union for key types
union gd_key {
    uint64_t number;
    char* string;
};

struct gd_base_node {
    struct cds_lfht_node lfht_node;
    bool key_is_number;
    bool type_key_is_number;
    union gd_key key;
    union gd_key type_key;
    struct rcu_head rcu_head;
};

// Wrapper structure for key matching
struct gd_key_match_ctx {
    union gd_key key;
    bool key_is_number;
};

// Define the node size function
uint64_t _gd_get_node_size(struct cds_lfht_node* node);

// Core system functions
bool                    gd_init(void);
void                    gd_cleanup(void);

// Main node access function - UNSAFE because:
// 1. You MUST call rcu_read_lock() before and rcu_read_unlock() after use
// 2. The returned pointer is READ-ONLY - do not modify the data it points to
// 3. The pointer may become invalid after rcu_read_unlock()
struct gd_base_node*    gd_get_node_unsafe(const union gd_key* key, bool key_is_number);

// Node lifecycle functions
uint64_t                gd_create_node(const union gd_key* key, bool key_is_number, const union gd_key* type_key, bool type_key_is_number);
bool                    gd_remove_node(const union gd_key* key, bool key_is_number);

// Update functions - these properly handle RCU and atomic updates
bool                    gd_update(const union gd_key* key, bool key_is_number, void* new_data, size_t data_size);
bool                    gd_upsert(const union gd_key* key, bool key_is_number, const union gd_key* type_key, bool type_key_is_number, void* data, size_t data_size);

// Iterator functions for hash table traversal
bool                    gd_iter_first(struct cds_lfht_iter* iter);
bool                    gd_iter_next(struct cds_lfht_iter* iter);
struct gd_base_node*    gd_iter_get_node(struct cds_lfht_iter* iter);
bool                    gd_lookup_iter(const union gd_key* key, bool key_is_number, struct cds_lfht_iter* iter);

// Utility functions
bool                    gd_is_node_deleted(struct gd_base_node* node);
unsigned long           gd_count_nodes(void);

// Helper functions to create gd_key unions
union gd_key           gd_create_number_key(uint64_t number);
union gd_key           gd_create_string_key(const char* string);

// Internal functions for test access
struct cds_lfht*       _gd_get_hash_table(void);
uint64_t               _gd_hash_key(const union gd_key* key, bool key_is_number);
int                    _gd_key_match(struct cds_lfht_node *node, const void *key_ctx);
void                   _gd_node_base_type_free_callback(struct rcu_head* head);

#endif /* GLOBAL_DATA_CORE_H */