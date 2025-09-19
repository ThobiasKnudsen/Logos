#ifndef TEST_GLOBAL_DATA_H
#define TEST_GLOBAL_DATA_H

#include "global_data/core.h"
#include "global_data/type.h"
#include <stdbool.h>

// Test node structure for testing purposes
struct test_node {
    struct gd_base_node base;
    int test_value;
    char test_string[64];
};

// Test result structure
struct test_result {
    int passed;
    int failed;
    int total;
};

// Wrapper functions for backward compatibility with tests
uint64_t gd_create_node_wrapper(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number);
struct gd_base_node* gd_get_node_unsafe_wrapper(const void* key, bool key_is_number);
bool gd_remove_node_wrapper(const void* key, bool key_is_number);
bool gd_update_wrapper(const void* key, bool key_is_number, void* new_data, size_t data_size);
bool gd_upsert_wrapper(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number, void* data, size_t data_size);
bool gd_lookup_iter_wrapper(const void* key, bool key_is_number, struct cds_lfht_iter* iter);

// Truly unsafe version for cleanup operations - bypasses all safety checks
struct gd_base_node* gd_get_node_cleanup_unsafe(const union gd_key* key, bool key_is_number);

// Test-specific cleanup function that uses cleanup unsafe operations
void gd_cleanup_test(void);

// Main test function - runs all tests
bool test_global_data_all(void);

// Individual test functions
bool test_gd_init_cleanup(void);
bool test_number_key_operations(void);
bool test_string_key_operations(void);
bool test_mixed_key_operations(void);
bool test_error_conditions(void);
bool test_rcu_safety(void);

// New test functions for additional core functionality
bool test_gd_update_operations(void);
bool test_gd_upsert_operations(void);
bool test_iterator_operations(void);
bool test_gd_is_node_deleted(void);
bool test_gd_count_nodes(void);

// Helper functions for testing
bool setup_test_node_types(const char** test_node_type_key_out);
void cleanup_test_nodes(void);
bool verify_test_node(void* node, int expected_value, const char* expected_string);
void print_test_results(const char* test_name, bool passed);

// Test node callback functions
bool test_node_free(struct gd_base_node* node);
void test_node_free_callback(struct rcu_head* head);
bool test_node_is_valid(struct gd_base_node* node);

// Performance test functions
bool test_performance_number_keys(int num_operations);
bool test_performance_string_keys(int num_operations);
bool test_performance_mixed_keys(int num_operations);

// Stress test functions
bool test_concurrent_access(int num_threads, int operations_per_thread);
bool test_memory_pressure(int num_nodes);

#endif /* TEST_GLOBAL_DATA_H */ 