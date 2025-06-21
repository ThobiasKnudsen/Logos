#ifndef GLOBAL_DATA_CORE_H
#define GLOBAL_DATA_CORE_H

#include <urcu.h>
#include <urcu/rculfhash.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

struct gd_base_node {
    struct cds_lfht_node lfht_node;
    bool key_is_number;
    bool type_key_is_number;
    void* key; // could be uint64_t or char*
    void* type_key; // could be uint64_t or char*
    struct rcu_head rcu_head;
};

// Utility function declarations
uint64_t _murmur3_64(const char *key, uint32_t len, uint64_t seed);
int _gd_key_match(struct cds_lfht_node *node, const void *key_ctx);
uint64_t _gd_hash_string(const char* string);
uint64_t _gd_hash_u64(uint64_t key);

// Internal utility functions
uint64_t _gd_get_next_key(void);
struct cds_lfht* _gd_get_hash_table(void);

// Core system functions
bool gd_init(void);
void gd_cleanup(void);

// Unified functions that work with both number and string keys
void* gd_get_unsafe(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number);
void* gd_get_copy(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number);
uint64_t gd_create_node(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number);
bool gd_free_node(const void* key, bool key_is_number);

// Convenience functions for backward compatibility
void* gd_get_by_number_unsafe(uint64_t key);
void* gd_get_by_string_unsafe(const char* key);
uint64_t gd_create_node_number(uint64_t type_key);
uint64_t gd_create_node_string(const char* key, uint64_t type_key);
bool gd_free_node_number(uint64_t key);
bool gd_free_node_string(const char* key);

#endif /* GLOBAL_DATA_CORE_H */
