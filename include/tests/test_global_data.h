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

// Main test function - runs all tests
bool test_global_data_all(void);

// Individual test functions
bool test_gd_init_cleanup(void);
bool test_number_key_operations(void);
bool test_string_key_operations(void);
bool test_mixed_key_operations(void);
bool test_error_conditions(void);
bool test_rcu_safety(void);

// Helper functions for testing
bool setup_test_type_node(uint64_t* type_key_out);
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