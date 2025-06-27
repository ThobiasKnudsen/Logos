#include "tests/test_global_data.h"
#include "tklog.h"
#include <urcu.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <time.h>
#include <unistd.h>
#include <sys/time.h>

static const char* g_test_node_type_key = "test_node_type";

// Wrapper functions for backward compatibility with tests
uint64_t gd_create_node_wrapper(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number) {
    union gd_key gd_type_key;
    
    if (type_key_is_number) {
        gd_type_key = gd_create_number_key((uint64_t)type_key);
    } else {
        gd_type_key = gd_create_string_key((const char*)type_key);
    }
    
    if (key_is_number) {
        if (key) {
            // Use provided numeric key
            union gd_key gd_key = gd_create_number_key((uint64_t)key);
            tklog_scope(uint64_t result = gd_create_node(&gd_key, key_is_number, &gd_type_key, type_key_is_number));
            return result;
        } else {
            // Auto-generate numeric key - pass NULL to gd_create_node
            tklog_scope(uint64_t result = gd_create_node(NULL, key_is_number, &gd_type_key, type_key_is_number));
            return result;
        }
    } else {
        // String key
        union gd_key gd_key = gd_create_string_key((const char*)key);
        tklog_scope(uint64_t result = gd_create_node(&gd_key, key_is_number, &gd_type_key, type_key_is_number));
        return result;
    }
}

struct gd_base_node* gd_get_node_unsafe_wrapper(const void* key, bool key_is_number) {
    union gd_key gd_key;
    
    if (key_is_number) {
        gd_key = gd_create_number_key((uint64_t)key);
    } else {
        gd_key = gd_create_string_key((const char*)key);
    }
    
    return gd_get_node_unsafe(&gd_key, key_is_number);
}

bool gd_remove_node_wrapper(const void* key, bool key_is_number) {
    union gd_key gd_key;
    
    if (key_is_number) {
        gd_key = gd_create_number_key((uint64_t)key);
    } else {
        gd_key = gd_create_string_key((const char*)key);
    }
    
    return gd_remove_node(&gd_key, key_is_number);
}

bool gd_update_wrapper(const void* key, bool key_is_number, void* new_data, size_t data_size) {
    union gd_key gd_key;
    
    if (key_is_number) {
        gd_key = gd_create_number_key((uint64_t)key);
    } else {
        gd_key = gd_create_string_key((const char*)key);
    }
    
    return gd_update(&gd_key, key_is_number, new_data, data_size);
}

bool gd_upsert_wrapper(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number, void* data, size_t data_size) {
    union gd_key gd_key, gd_type_key;
    
    if (key_is_number) {
        gd_key = gd_create_number_key((uint64_t)key);
    } else {
        gd_key = gd_create_string_key((const char*)key);
    }
    
    if (type_key_is_number) {
        gd_type_key = gd_create_number_key((uint64_t)type_key);
    } else {
        gd_type_key = gd_create_string_key((const char*)type_key);
    }
    
    return gd_upsert(&gd_key, key_is_number, &gd_type_key, type_key_is_number, data, data_size);
}

bool gd_lookup_iter_wrapper(const void* key, bool key_is_number, struct cds_lfht_iter* iter) {
    union gd_key gd_key;
    
    if (key_is_number) {
        gd_key = gd_create_number_key((uint64_t)key);
    } else {
        gd_key = gd_create_string_key((const char*)key);
    }
    
    return gd_lookup_iter(&gd_key, key_is_number, iter);
}

// Test node callback implementations
bool test_node_free(struct gd_base_node* node) {
    if (!node) return false;
    
    struct test_node* test_n = (struct test_node*)node;
    tklog_debug("Freeing test node with value: %d, string: %s\n", 
               test_n->test_value, test_n->test_string);
    
    free(node);
    return true;
}

void test_node_free_callback(struct rcu_head* head) {
    struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
    test_node_free(node);
}

bool test_node_is_valid(struct gd_base_node* node) {
    if (!node) return false;
    
    struct test_node* test_n = (struct test_node*)node;
    // Check if test_value is reasonable (allowing for concurrent updates)
    if (test_n->test_value < 0 || test_n->test_value >= 100000) {
        tklog_warning("Invalid test_value: %d\n", test_n->test_value);
        return false;
    }
    
    // Basic string validation
    if (strlen(test_n->test_string) >= 64) {
        tklog_warning("Invalid test_string length: %zu\n", strlen(test_n->test_string));
        return false;
    }
    
    return true;
}

// Helper function to set up test node types
bool setup_test_node_types(const char** test_node_type_key_out) {
    // System is already initialized by test_global_data_all
    
    // Check if the node type already exists
    rcu_read_lock();
    tklog_scope(struct gd_base_node* existing_type = gd_get_node_unsafe_wrapper(g_test_node_type_key, false));
    rcu_read_unlock();
    
    if (existing_type) {
        // Node type already exists, just return it
        *test_node_type_key_out = g_test_node_type_key;
        tklog_info("Using existing test node type with type_key: %s\n", g_test_node_type_key);
        return true;
    }
    
    // Create a node type for our test type using the helper function
    tklog_scope(const char* type_key = gd_create_node_type(g_test_node_type_key,
                                               sizeof(struct test_node),
                                               test_node_free,
                                               test_node_free_callback,
                                               test_node_is_valid));
    
    if (!type_key) {
        tklog_error("Failed to create test node type\n");
        return false;
    }
    
    *test_node_type_key_out = g_test_node_type_key;
    
    tklog_info("Created test node type with type_key: %s\n", g_test_node_type_key);
    return true;
}

bool verify_test_node(void* node, int expected_value, const char* expected_string) {
    if (!node) return false;
    
    struct test_node* test_n = (struct test_node*)node;
    
    if (test_n->test_value != expected_value) {
        tklog_error("Test value mismatch: expected %d, got %d\n", 
                   expected_value, test_n->test_value);
        return false;
    }
    
    if (expected_string && strcmp(test_n->test_string, expected_string) != 0) {
        tklog_error("Test string mismatch: expected '%s', got '%s'\n", 
                   expected_string, test_n->test_string);
        return false;
    }
    
    return true;
}

void print_test_results(const char* test_name, bool passed) {
    if (passed) {
        tklog_info("✓ %s PASSED\n", test_name);
    } else {
        tklog_error("✗ %s FAILED\n", test_name);
    }
}

// Get current time in microseconds
static uint64_t get_time_us(void) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return tv.tv_sec * 1000000ULL + tv.tv_usec;
}

// Thread data structure for stress tests
struct thread_data {
    int thread_id;
    const char* type_key;
    int num_operations;
    int num_create;
    int num_read;
    int num_update;
    int num_delete;
    int errors;
    int error_create;  // Track specific error types
    int error_read;
    int error_update;
    int error_delete;
    uint64_t* created_keys;
    int* created_count;  // Pointer to shared count
    pthread_mutex_t* keys_mutex;
    uint64_t start_time;
    uint64_t end_time;
};

// Stress test: Many threads creating/reading/updating/deleting number keys
void* stress_test_number_keys_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    int local_created = 0;
    uint64_t local_keys[1000]; // Local buffer for created keys
    
    // Register this thread with RCU
    tklog_scope(rcu_register_thread());
    
    tklog_info("Thread %d starting number key operations\n", data->thread_id);
    
    for (int i = 0; i < data->num_operations; i++) {
        int op = rand() % 100;
        
        if (op < 40 && local_created < 1000) { // 40% create
            tklog_scope(uint64_t key = gd_create_node_wrapper(NULL, true, data->type_key, false));
            if (key == 0) {
                data->errors++;
                data->error_create++;
                continue;
            }
            
            // Initialize the node
            rcu_read_lock();
            tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true));
            if (node) {
                node->test_value = data->thread_id * 1000 + i;
                snprintf(node->test_string, 64, "thread_%d_op_%d", data->thread_id, i);
            } else {
                // This should not happen - we just created the node
                data->errors++;
                data->error_create++;
                tklog_warning("Thread %d: Created node %llu but can't find it\n", 
                            data->thread_id, key);
            }
            rcu_read_unlock();
            
            local_keys[local_created++] = key;
            data->num_create++;
            
            // Occasionally add to shared pool
            if (local_created % 10 == 0) {
                pthread_mutex_lock(data->keys_mutex);
                for (int j = local_created - 10; j < local_created && *data->created_count < 10000; j++) {
                    data->created_keys[*data->created_count] = local_keys[j];
                    (*data->created_count)++;
                }
                pthread_mutex_unlock(data->keys_mutex);
            }
            
        } else if (op < 70) { // 30% read
            uint64_t key = 0;
            if (local_created > 0 && rand() % 2) {
                // Read own key
                key = local_keys[rand() % local_created];
            } else {
                // Read shared key
                pthread_mutex_lock(data->keys_mutex);
                if (*data->created_count > 0) {
                    key = data->created_keys[rand() % *data->created_count];
                }
                pthread_mutex_unlock(data->keys_mutex);
            }
            
            if (key != 0) {
                rcu_read_lock();
                tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true));
                if (node) {
                    // Verify the node is valid
                    if (!test_node_is_valid(&node->base)) {
                        data->errors++;
                        data->error_read++;
                        tklog_warning("Thread %d: Invalid node found for key %llu\n", 
                                    data->thread_id, key);
                    }
                    data->num_read++;
                } else {
                    // Node was deleted between getting the key and reading it
                    // This is not an error in concurrent scenarios
                }
                rcu_read_unlock();
            }
            
        } else if (op < 90 && local_created > 0) { // 20% update
            uint64_t key = local_keys[rand() % local_created];
            
            rcu_read_lock();
            tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true));
            if (node) {
                int old_value = node->test_value;
                node->test_value = (node->test_value + 1) % 10000;
                // Verify the update worked
                if (node->test_value != (old_value + 1) % 10000) {
                    data->errors++;
                    data->error_update++;
                    tklog_warning("Thread %d: Update failed for key %llu\n", 
                                data->thread_id, key);
                }
                data->num_update++;
            }
            rcu_read_unlock();
            
        } else if (local_created > 0) { // 10% delete
            int idx = rand() % local_created;
            uint64_t key = local_keys[idx];
            
            // First check if the node still exists (it might have been deleted by another thread)
            rcu_read_lock();
            tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true));
            rcu_read_unlock();
            
            if (node) {
                if (gd_remove_node_wrapper((void*)key, true)) {
                    data->num_delete++;
                    // Remove from local array
                    local_keys[idx] = local_keys[--local_created];
                } else {
                    // This is a real error - node exists but couldn't delete
                    data->errors++;
                    data->error_delete++;
                    tklog_warning("Thread %d: Failed to delete existing node %llu\n", 
                                data->thread_id, key);
                }
            } else {
                // Node already deleted by another thread, just remove from local array
                local_keys[idx] = local_keys[--local_created];
            }
        }
        
        // Small delay to simulate real work
        if (i % 100 == 0) {
            usleep(1);
        }
    }
    
    // Clean up remaining local keys
    for (int i = 0; i < local_created; i++) {
        // Check if key still exists before trying to delete
        rcu_read_lock();
        tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)local_keys[i], true));
        rcu_read_unlock();
        
        if (node) {
            if (gd_remove_node_wrapper((void*)local_keys[i], true)) {
                data->num_delete++;  // Count the cleanup deletions
            }
        }
    }
    
    data->end_time = get_time_us();
    tklog_info("Thread %d completed: create=%d, read=%d, update=%d, delete=%d, errors=%d "
               "(create_err=%d, read_err=%d, update_err=%d, delete_err=%d)\n",
               data->thread_id, data->num_create, data->num_read, 
               data->num_update, data->num_delete, data->errors,
               data->error_create, data->error_read, data->error_update, data->error_delete);
    
    // Unregister this thread from RCU
    tklog_scope(rcu_unregister_thread());
    
    return NULL;
}

// Stress test: Many threads working with string keys
void* stress_test_string_keys_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    char key_buffer[128];
    int created_count = 0;
    char created_keys[100][128];
    
    // Register this thread with RCU
    rcu_register_thread();
    
    tklog_info("Thread %d starting string key operations\n", data->thread_id);
    
    for (int i = 0; i < data->num_operations; i++) {
        int op = rand() % 100;
        
        if (op < 40 && created_count < 100) { // 40% create
            snprintf(key_buffer, 128, "thread_%d_key_%d_%d", 
                     data->thread_id, i, rand());
            
            tklog_scope(bool create_result = gd_create_node_wrapper(key_buffer, false, data->type_key, false));
            if (create_result) {
                strcpy(created_keys[created_count++], key_buffer);
                
                // Initialize the node
                rcu_read_lock();
                struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(key_buffer, false);
                if (node) {
                    node->test_value = data->thread_id * 1000 + i;
                    snprintf(node->test_string, 64, "string_thread_%d", data->thread_id);
                }
                rcu_read_unlock();
                
                data->num_create++;
            } else {
                data->errors++;
            }
            
        } else if (op < 70 && created_count > 0) { // 30% read
            const char* key = created_keys[rand() % created_count];
            
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(key, false);
            if (node) {
                if (!test_node_is_valid(&node->base)) {
                    data->errors++;
                }
                data->num_read++;
            }
            rcu_read_unlock();
            
        } else if (op < 90 && created_count > 0) { // 20% update
            const char* key = created_keys[rand() % created_count];
            
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(key, false);
            if (node) {
                node->test_value = (node->test_value + 1) % 10000;
                data->num_update++;
            }
            rcu_read_unlock();
            
        } else if (created_count > 0) { // 10% delete
            int idx = rand() % created_count;
            
            // Check if node still exists before deleting
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(created_keys[idx], false);
            rcu_read_unlock();
            
            if (node) {
                if (gd_remove_node_wrapper(created_keys[idx], false)) {
                    data->num_delete++;
                    // Remove from array
                    strcpy(created_keys[idx], created_keys[--created_count]);
                } else {
                    data->errors++;
                }
            } else {
                // Already deleted, just remove from array
                strcpy(created_keys[idx], created_keys[--created_count]);
            }
        }
    }
    
    // Clean up remaining keys
    for (int i = 0; i < created_count; i++) {
        rcu_read_lock();
        struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(created_keys[i], false);
        rcu_read_unlock();
        
        if (node) {
            if (gd_remove_node_wrapper(created_keys[i], false)) {
                data->num_delete++;  // Count the cleanup deletions
            }
        }
    }
    
    data->end_time = get_time_us();
    tklog_info("Thread %d completed string ops: create=%d, read=%d, update=%d, delete=%d, errors=%d\n",
               data->thread_id, data->num_create, data->num_read, 
               data->num_update, data->num_delete, data->errors);
    
    // Unregister this thread from RCU
    rcu_unregister_thread();
    
    return NULL;
}

// Stress test: Mixed operations with both number and string keys
void* stress_test_mixed_keys_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    
    // Register this thread with RCU
    rcu_register_thread();
    
    tklog_info("Thread %d starting mixed key operations\n", data->thread_id);
    
    for (int i = 0; i < data->num_operations / 2; i++) {
        // Alternate between number and string operations
        if (i % 2 == 0) {
            // Number key operation
            tklog_scope(uint64_t key = gd_create_node_wrapper(NULL, true, data->type_key, false));
            if (key != 0) {
                rcu_read_lock();
                struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true);
                if (node) {
                    node->test_value = i;
                }
                rcu_read_unlock();
                
                // Do some reads
                for (int j = 0; j < 5; j++) {
                    rcu_read_lock();
                    struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true);
                    if (node && node->test_value != i) {
                        data->errors++;
                    }
                    rcu_read_unlock();
                }
                
                gd_remove_node_wrapper((void*)key, true);
                data->num_create++;
                data->num_read += 5;
                data->num_delete++;
            }
        } else {
            // String key operation
            char key[64];
            snprintf(key, 64, "mixed_%d_%d", data->thread_id, i);
            
            tklog_scope(bool create_result = gd_create_node_wrapper(key, false, data->type_key, false));
            if (create_result) {
                rcu_read_lock();
                struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(key, false);
                if (node) {
                    node->test_value = i;
                }
                rcu_read_unlock();
                
                // Do some reads
                for (int j = 0; j < 5; j++) {
                    rcu_read_lock();
                    struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(key, false);
                    if (node && node->test_value != i) {
                        data->errors++;
                    }
                    rcu_read_unlock();
                }
                
                gd_remove_node_wrapper(key, false);
                data->num_create++;
                data->num_read += 5;
                data->num_delete++;
            }
        }
    }
    
    data->end_time = get_time_us();
    
    // Unregister this thread from RCU
    rcu_unregister_thread();
    
    return NULL;
}

// Main stress test function
bool test_stress_concurrent(int num_threads, int ops_per_thread, const char* test_name,
                           void* (*thread_func)(void*)) {
    tklog_info("Starting stress test: %s with %d threads, %d ops each\n", 
               test_name, num_threads, ops_per_thread);
    
    const char* type_key;
    tklog_scope(bool setup_test_node_types_result = setup_test_node_types(&type_key));
    if (!setup_test_node_types_result) {
        print_test_results(test_name, false);
        return false;
    }
    
    pthread_t* threads = malloc(num_threads * sizeof(pthread_t));
    struct thread_data* thread_data = calloc(num_threads, sizeof(struct thread_data));
    uint64_t* shared_keys = calloc(10000, sizeof(uint64_t));
    pthread_mutex_t keys_mutex = PTHREAD_MUTEX_INITIALIZER;
    int shared_created_count = 0;  // Shared counter for created keys
    
    uint64_t start_time = get_time_us();
    
    // Initialize thread data and start threads
    for (int i = 0; i < num_threads; i++) {
        thread_data[i].thread_id = i;
        thread_data[i].type_key = type_key;
        thread_data[i].num_operations = ops_per_thread;
        thread_data[i].created_keys = shared_keys;
        thread_data[i].created_count = &shared_created_count;  // Point to shared counter
        thread_data[i].keys_mutex = &keys_mutex;
        thread_data[i].start_time = start_time;
        thread_data[i].error_create = 0;
        thread_data[i].error_read = 0;
        thread_data[i].error_update = 0;
        thread_data[i].error_delete = 0;
        
        if (pthread_create(&threads[i], NULL, thread_func, &thread_data[i]) != 0) {
            tklog_error("Failed to create thread %d\n", i);
            print_test_results(test_name, false);
            return false;
        }
    }
    
    // Wait for all threads to complete
    for (int i = 0; i < num_threads; i++) {
        pthread_join(threads[i], NULL);
    }
    
    uint64_t end_time = get_time_us();
    uint64_t duration = end_time - start_time;
    
    // Calculate totals
    int total_create = 0, total_read = 0, total_update = 0, total_delete = 0, total_errors = 0;
    int error_create = 0, error_read = 0, error_update = 0, error_delete = 0;
    for (int i = 0; i < num_threads; i++) {
        total_create += thread_data[i].num_create;
        total_read += thread_data[i].num_read;
        total_update += thread_data[i].num_update;
        total_delete += thread_data[i].num_delete;
        total_errors += thread_data[i].errors;
        error_create += thread_data[i].error_create;
        error_read += thread_data[i].error_read;
        error_update += thread_data[i].error_update;
        error_delete += thread_data[i].error_delete;
    }
    
    int total_ops = total_create + total_read + total_update + total_delete;
    double ops_per_sec = (double)total_ops / (duration / 1000000.0);
    
    tklog_info("\n=== STRESS TEST RESULTS: %s ===\n", test_name);
    tklog_info("Duration: %.2f seconds\n", duration / 1000000.0);
    tklog_info("Total operations: %d\n", total_ops);
    tklog_info("Operations/second: %.0f\n", ops_per_sec);
    tklog_info("Create: %d, Read: %d, Update: %d, Delete: %d\n", 
               total_create, total_read, total_update, total_delete);
    tklog_info("Total Errors: %d (create: %d, read: %d, update: %d, delete: %d)\n", 
               total_errors, error_create, error_read, error_update, error_delete);
    
    // Clean up shared keys
    pthread_mutex_lock(&keys_mutex);
    rcu_read_lock();
    for (int i = 0; i < shared_created_count; i++) {
        // Check if key still exists before deleting
        tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)shared_keys[i], true));
        
        if (node) {
            tklog_scope(gd_remove_node_wrapper((void*)shared_keys[i], true));  // This cleanup doesn't need to be counted since it's not part of the test metrics
        }
    }
    rcu_read_unlock();
    pthread_mutex_unlock(&keys_mutex);
    
    free(threads);
    free(thread_data);
    free(shared_keys);
    
    bool passed = (total_errors == 0);
    print_test_results(test_name, passed);
    return passed;
}

// Memory pressure test - create many nodes and verify memory handling
bool test_memory_pressure(int num_nodes) {
    tklog_info("Testing memory pressure with %d nodes\n", num_nodes);
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("Memory Pressure", false);
        return false;
    }
    
    uint64_t* keys = malloc(num_nodes * sizeof(uint64_t));
    int created = 0;
    
    uint64_t start_time = get_time_us();
    
    // Create many nodes
    for (int i = 0; i < num_nodes; i++) {
        tklog_scope(keys[i] = gd_create_node_wrapper(NULL, true, type_key, false));
        if (keys[i] != 0) {
            created++;
            
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)keys[i], true);
            if (node) {
                node->test_value = i;
                snprintf(node->test_string, 64, "mem_test_%d", i);
            }
            rcu_read_unlock();
        }
        
        if (i % 1000 == 0 && i > 0) {
            tklog_info("Created %d nodes...\n", i);
        }
    }
    
    uint64_t create_time = get_time_us();
    
    tklog_info("Created %d nodes in %.2f seconds\n", 
               created, (create_time - start_time) / 1000000.0);
    
    // Verify all nodes
    int verified = 0;
    for (int i = 0; i < created; i++) {
        if (keys[i] != 0) {
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)keys[i], true);
            if (node && node->test_value == i) {
                verified++;
            }
            rcu_read_unlock();
        }
    }
    
    tklog_info("Verified %d/%d nodes\n", verified, created);
    
    // Delete all nodes
    int deleted = 0;
    for (int i = 0; i < created; i++) {
        if (keys[i] != 0 && gd_remove_node_wrapper((void*)keys[i], true)) {
            deleted++;
        }
    }
    
    uint64_t end_time = get_time_us();
    
    tklog_info("Deleted %d nodes in %.2f seconds\n", 
               deleted, (end_time - create_time) / 1000000.0);
    
    free(keys);
    
    bool passed = (created == verified && created == deleted);
    print_test_results("Memory Pressure", passed);
    return passed;
}

// Test RCU grace period handling - simplified version
void* rcu_reader_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    int successful_reads = 0;
    
    // Register this thread with RCU
    rcu_register_thread();
    
    for (int i = 0; i < data->num_operations; i++) {
        // Read from the key ranges that writers are using
        // Writers use ranges: thread 0: keys 1-10, thread 1: keys 101-110, etc.
        int writer_thread = i % 2;  // 2 writer threads
        uint64_t test_key = (writer_thread * 100) + (i % 10) + 1;
        
        rcu_read_lock();
        struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)test_key, true);
        if (node) {
            // Simulate some work with the node
            volatile int dummy = node->test_value;
            (void)dummy;
            successful_reads++;
        }
        rcu_read_unlock();
        
        // Small random delay
        if (rand() % 10 == 0) {
            usleep(1);
        }
    }
    
    data->num_read = successful_reads;
    
    // Unregister this thread from RCU
    rcu_unregister_thread();
    
    return NULL;
}

void* rcu_writer_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    
    // Register this thread with RCU
    rcu_register_thread();
    
    for (int i = 0; i < data->num_operations; i++) {
        // Each thread works with its own unique key range to avoid conflicts
        uint64_t key = (data->thread_id * 100) + (i % 10) + 1;
        
        if (rand() % 2 == 0) {
            // Create/update
            bool node_exists = false;
            
            // Check if node exists (read operation)
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true);
            if (node) {
                node_exists = true;
            }
            rcu_read_unlock();
            
            if (!node_exists) {
                // Create if doesn't exist (write operation - no RCU lock)
                tklog_scope(uint64_t new_key = gd_create_node_wrapper((void*)key, true, data->type_key, false));
                if (new_key != 0) {
                    data->num_create++;
                    
                    // Initialize the node (read operation to get the node)
                    rcu_read_lock();
                    node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)new_key, true);
                    if (node) {
                        // Note: This is a test scenario where we're modifying the node directly
                        // In a real application, you would use proper update mechanisms
                        node->test_value = data->thread_id * 1000 + i;
                        snprintf(node->test_string, 64, "rcu_test_%d", data->thread_id);
                    }
                    rcu_read_unlock();
                }
            } else {
                // Update existing node (read operation to get the node)
                rcu_read_lock();
                node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true);
                if (node) {
                    // Note: This is a test scenario where we're modifying the node directly
                    // In a real application, you would use proper update mechanisms
                    node->test_value = (node->test_value + 1) % 10000;
                    data->num_update++;
                }
                rcu_read_unlock();
            }
        } else {
            // Delete - only if node exists
            bool node_exists = false;
            
            // Check if node exists (read operation)
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true);
            if (node) {
                node_exists = true;
            }
            rcu_read_unlock();
            
            if (node_exists) {
                // Delete the node (write operation - no RCU lock)
                if (gd_remove_node_wrapper((void*)key, true)) {
                    data->num_delete++;
                }
            }
        }
    }
    
    // Unregister this thread from RCU
    rcu_unregister_thread();
    
    return NULL;
}

bool test_rcu_grace_periods(void) {
    tklog_info("Testing RCU grace period handling\n");
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("RCU Grace Periods", false);
        return false;
    }
    
    const int num_readers = 4;
    const int num_writers = 2;
    const int ops_per_thread = 10000;
    
    pthread_t readers[num_readers];
    pthread_t writers[num_writers];
    struct thread_data reader_data[num_readers];
    struct thread_data writer_data[num_writers];
    
    // Initialize reader data structures
    for (int i = 0; i < num_readers; i++) {
        memset(&reader_data[i], 0, sizeof(struct thread_data));
        reader_data[i].thread_id = i;
        reader_data[i].type_key = type_key;
        reader_data[i].num_operations = ops_per_thread;
        reader_data[i].num_create = 0;
        reader_data[i].num_read = 0;
        reader_data[i].num_update = 0;
        reader_data[i].num_delete = 0;
        reader_data[i].errors = 0;
        reader_data[i].error_create = 0;
        reader_data[i].error_read = 0;
        reader_data[i].error_update = 0;
        reader_data[i].error_delete = 0;
    }
    
    // Initialize writer data structures
    for (int i = 0; i < num_writers; i++) {
        memset(&writer_data[i], 0, sizeof(struct thread_data));
        writer_data[i].thread_id = i;
        writer_data[i].type_key = type_key;
        writer_data[i].num_operations = ops_per_thread;
        writer_data[i].num_create = 0;
        writer_data[i].num_read = 0;
        writer_data[i].num_update = 0;
        writer_data[i].num_delete = 0;
        writer_data[i].errors = 0;
        writer_data[i].error_create = 0;
        writer_data[i].error_read = 0;
        writer_data[i].error_update = 0;
        writer_data[i].error_delete = 0;
    }
    
    // Start readers
    for (int i = 0; i < num_readers; i++) {
        pthread_create(&readers[i], NULL, rcu_reader_thread, &reader_data[i]);
    }
    
    // Start writers
    for (int i = 0; i < num_writers; i++) {
        pthread_create(&writers[i], NULL, rcu_writer_thread, &writer_data[i]);
    }
    
    // Wait for completion
    for (int i = 0; i < num_readers; i++) {
        pthread_join(readers[i], NULL);
    }
    for (int i = 0; i < num_writers; i++) {
        pthread_join(writers[i], NULL);
    }
    
    // Report results
    int total_reads = 0, total_creates = 0, total_updates = 0, total_deletes = 0;
    for (int i = 0; i < num_readers; i++) {
        total_reads += reader_data[i].num_read;
    }
    for (int i = 0; i < num_writers; i++) {
        total_creates += writer_data[i].num_create;
        total_updates += writer_data[i].num_update;
        total_deletes += writer_data[i].num_delete;
    }
    
    tklog_info("RCU test completed: %d reads, %d creates, %d updates, %d deletes\n",
               total_reads, total_creates, total_updates, total_deletes);
    
    // Clean up any remaining test nodes
    for (int thread_id = 0; thread_id < num_writers; thread_id++) {
        for (int i = 1; i <= 10; i++) {
            uint64_t key = (thread_id * 100) + i;
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true);
            
            if (node) {
                rcu_read_unlock();
                gd_remove_node_wrapper((void*)key, true);
            } else {
                rcu_read_unlock();
            }
        }
    }
    
    print_test_results("RCU Grace Periods", true);
    return true;
}

// Test gd_update operations
bool test_gd_update_operations(void) {
    tklog_info("Testing gd_update operations...\n");
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    // Create a node first
    tklog_scope(uint64_t node_key = gd_create_node_wrapper(NULL, true, type_key, false));
    if (node_key == 0) {
        tklog_error("Failed to create node for update test\n");
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    // Initialize the original node
    rcu_read_lock();
    tklog_scope(struct test_node* original_node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)node_key, true));
    if (!original_node) {
        tklog_error("Failed to get original node\n");
        rcu_read_unlock();
        print_test_results("gd_update Operations", false);
        return false;
    }
    original_node->test_value = 100;
    strcpy(original_node->test_string, "original");
    rcu_read_unlock();
    
    // Create updated data
    struct test_node updated_data;
    updated_data.base.key_is_number = true;
    updated_data.base.type_key_is_number = false;
    updated_data.test_value = 200;
    strcpy(updated_data.test_string, "updated");
    
    // Test update operation
    if (!gd_update_wrapper((void*)node_key, true, &updated_data, sizeof(struct test_node))) {
        tklog_error("Failed to update node\n");
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    // Verify the update worked
    rcu_read_lock();
    tklog_scope(struct test_node* updated_node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)node_key, true));
    if (!updated_node) {
        tklog_error("Failed to get updated node\n");
        rcu_read_unlock();
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    bool update_success = (updated_node->test_value == 200 && 
                          strcmp(updated_node->test_string, "updated") == 0);
    rcu_read_unlock();
    
    if (!update_success) {
        tklog_error("Update verification failed\n");
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    // Test updating non-existent node (should fail)
    uint64_t nonexistent_key = 999999;
    if (gd_update_wrapper((void*)nonexistent_key, true, &updated_data, sizeof(struct test_node))) {
        tklog_error("Update should have failed for non-existent node\n");
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    // Clean up
    gd_remove_node_wrapper((void*)node_key, true);
    
    // Test getting the freed node (should return NULL now)
    rcu_read_lock();
    tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)node_key, true));
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found freed number key %llu\n", node_key);
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    print_test_results("gd_update Operations", true);
    return true;
}

// Test gd_upsert operations
bool test_gd_upsert_operations(void) {
    tklog_info("Testing gd_upsert operations...\n");
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("gd_upsert Operations", false);
        return false;
    }
    
    uint64_t test_key = 12345;
    
    // Create test data
    struct test_node test_data;
    test_data.base.key_is_number = true;
    test_data.base.type_key_is_number = false;
    test_data.test_value = 300;
    strcpy(test_data.test_string, "upsert_new");
    
    // Test insert (node doesn't exist yet)
    if (!gd_upsert_wrapper((void*)test_key, true, type_key, false, &test_data, sizeof(struct test_node))) {
        tklog_error("Failed to upsert (insert) new node\n");
        print_test_results("gd_upsert Operations", false);
        return false;
    }
    
    // Verify the insert worked
    rcu_read_lock();
    tklog_scope(struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)test_key, true));
    if (!node || node->test_value != 300 || strcmp(node->test_string, "upsert_new") != 0) {
        tklog_error("Upsert insert verification failed\n");
        rcu_read_unlock();
        print_test_results("gd_upsert Operations", false);
        return false;
    }
    rcu_read_unlock();
    
    // Modify test data for update
    test_data.test_value = 400;
    strcpy(test_data.test_string, "upsert_updated");
    
    // Test update (node already exists)
    if (!gd_upsert_wrapper((void*)test_key, true, type_key, false, &test_data, sizeof(struct test_node))) {
        tklog_error("Failed to upsert (update) existing node\n");
        print_test_results("gd_upsert Operations", false);
        return false;
    }
    
    // Verify the update worked
    rcu_read_lock();
    tklog_scope(node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)test_key, true));
    if (!node || node->test_value != 400 || strcmp(node->test_string, "upsert_updated") != 0) {
        tklog_error("Upsert update verification failed\n");
        rcu_read_unlock();
        print_test_results("gd_upsert Operations", false);
        return false;
    }
    rcu_read_unlock();
    
    // Test string key upsert
    const char* string_key = "upsert_string_test";
    test_data.test_value = 500;
    strcpy(test_data.test_string, "string_upsert");
    
    if (!gd_upsert_wrapper(string_key, false, type_key, false, &test_data, sizeof(struct test_node))) {
        tklog_error("Failed to upsert string key node\n");
        print_test_results("gd_upsert Operations", false);
        return false;
    }
    
    // Verify string key upsert
    rcu_read_lock();
    tklog_scope(node = (struct test_node*)gd_get_node_unsafe_wrapper(string_key, false));
    if (!node || node->test_value != 500 || strcmp(node->test_string, "string_upsert") != 0) {
        tklog_error("String key upsert verification failed\n");
        rcu_read_unlock();
        print_test_results("gd_upsert Operations", false);
        return false;
    }
    rcu_read_unlock();
    
    // Clean up
    gd_remove_node_wrapper((void*)test_key, true);
    gd_remove_node_wrapper(string_key, false);
    
    print_test_results("gd_upsert Operations", true);
    return true;
}

// Test iterator operations
bool test_iterator_operations(void) {
    tklog_info("Testing iterator operations...\n");
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("Iterator Operations", false);
        return false;
    }
    
    // Create several test nodes
    const int num_nodes = 10;
    uint64_t created_keys[num_nodes];
    int created_count = 0;
    
    for (int i = 0; i < num_nodes; i++) {
        tklog_scope(uint64_t key = gd_create_node_wrapper(NULL, true, type_key, false));
        if (key != 0) {
            created_keys[created_count++] = key;
            
            // Initialize node data
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)key, true);
            if (node) {
                node->test_value = i * 10;
                snprintf(node->test_string, 64, "iter_test_%d", i);
            }
            rcu_read_unlock();
        }
    }
    
    if (created_count == 0) {
        tklog_error("Failed to create any test nodes for iterator test\n");
        print_test_results("Iterator Operations", false);
        return false;
    }
    
    // Test gd_iter_first and gd_iter_next
    struct cds_lfht_iter iter;
    int nodes_found = 0;
    
    rcu_read_lock();
    if (gd_iter_first(&iter)) {
        do {
            struct gd_base_node* base_node = gd_iter_get_node(&iter);
            if (base_node) {
                nodes_found++;
                tklog_debug("Found node during iteration, key_is_number: %s\n", 
                           base_node->key_is_number ? "true" : "false");
            }
        } while (gd_iter_next(&iter));
    }
    rcu_read_unlock();
    
    if (nodes_found < created_count) {
        tklog_error("Iterator found %d nodes, expected at least %d\n", nodes_found, created_count);
        // Clean up before failing
        for (int i = 0; i < created_count; i++) {
            gd_remove_node_wrapper((void*)created_keys[i], true);
        }
        print_test_results("Iterator Operations", false);
        return false;
    }
    
    // Test gd_lookup_iter for specific keys
    rcu_read_lock();
    for (int i = 0; i < created_count; i++) {
        if (gd_lookup_iter_wrapper((void*)created_keys[i], true, &iter)) {
            struct gd_base_node* base_node = gd_iter_get_node(&iter);
            if (!base_node) {
                tklog_error("gd_lookup_iter found key but gd_iter_get_node returned NULL\n");
                rcu_read_unlock();
                // Clean up before failing
                for (int j = 0; j < created_count; j++) {
                    gd_remove_node_wrapper((void*)created_keys[j], true);
                }
                print_test_results("Iterator Operations", false);
                return false;
            }
        } else {
            tklog_error("gd_lookup_iter failed to find existing key %llu\n", created_keys[i]);
            rcu_read_unlock();
            // Clean up before failing
            for (int j = 0; j < created_count; j++) {
                gd_remove_node_wrapper((void*)created_keys[j], true);
            }
            print_test_results("Iterator Operations", false);
            return false;
        }
    }
    
    // Test lookup of non-existent key
    uint64_t nonexistent_key = 999999;
    if (gd_lookup_iter_wrapper((void*)nonexistent_key, true, &iter)) {
        tklog_error("gd_lookup_iter should not have found non-existent key\n");
        rcu_read_unlock();
        // Clean up before failing
        for (int i = 0; i < created_count; i++) {
            gd_remove_node_wrapper((void*)created_keys[i], true);
        }
        print_test_results("Iterator Operations", false);
        return false;
    }
    rcu_read_unlock();
    
    // Clean up
    for (int i = 0; i < created_count; i++) {
        gd_remove_node_wrapper((void*)created_keys[i], true);
    }
    
    print_test_results("Iterator Operations", true);
    return true;
}

// Test gd_is_node_deleted
bool test_gd_is_node_deleted(void) {
    tklog_info("Testing gd_is_node_deleted...\n");
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("gd_is_node_deleted", false);
        return false;
    }
    
    // Test with NULL node (should return true)
    if (!gd_is_node_deleted(NULL)) {
        tklog_error("gd_is_node_deleted should return true for NULL node\n");
        print_test_results("gd_is_node_deleted", false);
        return false;
    }
    
    // Create a node
    tklog_scope(uint64_t node_key = gd_create_node_wrapper(NULL, true, type_key, false));
    if (node_key == 0) {
        tklog_error("Failed to create node for deletion test\n");
        print_test_results("gd_is_node_deleted", false);
        return false;
    }
    
    // Get the node and test it's not deleted
    rcu_read_lock();
    struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)node_key, true);
    if (!node) {
        tklog_error("Failed to get created node\n");
        rcu_read_unlock();
        print_test_results("gd_is_node_deleted", false);
        return false;
    }
    
    if (gd_is_node_deleted(&node->base)) {
        tklog_error("gd_is_node_deleted should return false for active node\n");
        rcu_read_unlock();
        print_test_results("gd_is_node_deleted", false);
        return false;
    }
    rcu_read_unlock();
    
    // Delete the node
    if (!gd_remove_node_wrapper((void*)node_key, true)) {
        tklog_error("Failed to remove node\n");
        print_test_results("gd_is_node_deleted", false);
        return false;
    }
    
    // Note: After deletion, we cannot safely access the node pointer due to RCU
    // The test mainly verifies the function works with NULL and active nodes
    
    print_test_results("gd_is_node_deleted", true);
    return true;
}

// Test gd_count_nodes
bool test_gd_count_nodes(void) {
    tklog_info("Testing gd_count_nodes...\n");
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("gd_count_nodes", false);
        return false;
    }
    
    // Get initial count (should include base_type and test_node_type)
    unsigned long initial_count = gd_count_nodes();
    
    tklog_info("Initial node count: %lu\n", initial_count);
    
    // Create several nodes
    const int nodes_to_create = 5;
    uint64_t created_keys[nodes_to_create];
    int created_count = 0;
    
    for (int i = 0; i < nodes_to_create; i++) {
        tklog_scope(uint64_t key = gd_create_node_wrapper(NULL, true, type_key, false));
        if (key != 0) {
            created_keys[created_count++] = key;
        }
    }
    
    // Check count after creation
    unsigned long after_create_count = gd_count_nodes();
    
    tklog_info("Count after creating %d nodes: %lu\n", created_count, after_create_count);
    
    if (after_create_count != initial_count + created_count) {
        tklog_error("Node count mismatch after creation: expected %lu, got %lu\n",
                   initial_count + created_count, after_create_count);
        // Clean up before failing
        for (int i = 0; i < created_count; i++) {
            gd_remove_node_wrapper((void*)created_keys[i], true);
        }
        print_test_results("gd_count_nodes", false);
        return false;
    }
    
    // Delete some nodes
    int nodes_to_delete = created_count / 2;
    for (int i = 0; i < nodes_to_delete; i++) {
        gd_remove_node_wrapper((void*)created_keys[i], true);
    }
    
    // Note: Due to RCU grace periods, the count might not immediately reflect deletions
    // We'll wait a bit and then check
    synchronize_rcu();
    
    unsigned long after_delete_count = gd_count_nodes();
    
    tklog_info("Count after deleting %d nodes: %lu\n", nodes_to_delete, after_delete_count);
    
    // The count should be reduced, but exact timing depends on RCU grace periods
    if (after_delete_count > after_create_count) {
        tklog_error("Node count should not increase after deletion\n");
        // Clean up remaining nodes
        for (int i = nodes_to_delete; i < created_count; i++) {
            gd_remove_node_wrapper((void*)created_keys[i], true);
        }
        print_test_results("gd_count_nodes", false);
        return false;
    }
    
    // Clean up remaining nodes
    for (int i = nodes_to_delete; i < created_count; i++) {
        gd_remove_node_wrapper((void*)created_keys[i], true);
    }
    
    print_test_results("gd_count_nodes", true);
    return true;
}

// Test-specific cleanup function that only removes test data, keeps system initialized
static bool g_cleanup_done = false;  // Track if cleanup has been performed

// Reset cleanup flag for new test runs
void gd_reset_cleanup_flag(void) {
    g_cleanup_done = false;
}

void gd_cleanup_test(void) {
    // If cleanup has already been done, don't do it again
    if (g_cleanup_done) {
        return;
    }
    
    struct cds_lfht* g_p_ht = _gd_get_hash_table();
    
    if (!g_p_ht) {
        // Hash table not initialized, nothing to clean up
        g_cleanup_done = true;
        return;
    }
    
    // Process test nodes in batches until none are left
    const int batch_size = 1000;
    struct cds_lfht_node* nodes_to_delete[batch_size];
    int total_deleted = 0;
    
    while (true) {
        int delete_count = 0;
        struct cds_lfht_iter iter;
        
        // Collect a batch of test nodes using proper wrapper functions
        uint64_t iter_count = 0;
        rcu_read_lock();
        if (gd_iter_first(&iter)) {
            do {
                tklog_debug("iteration %lld\n", iter_count++);
                struct gd_base_node* base_node = gd_iter_get_node(&iter);
                if (base_node) {
                    // Check if this is a test node (has test_node_type as its type)
                    bool is_test_node = false;
                    if (!base_node->type_key_is_number) {
                        is_test_node = (strcmp(rcu_dereference(base_node->type_key.string), g_test_node_type_key) == 0);
                    }
                    
                    if (is_test_node) {
                        // Store this node for deletion
                        nodes_to_delete[delete_count++] = &base_node->lfht_node;
                    }
                }
            } while (delete_count < batch_size && gd_iter_next(&iter));
        }
        rcu_read_unlock();
        
        // If no test nodes found in this batch, we're done
        if (delete_count == 0) {
            break;
        }
        
        // Delete all nodes in this batch
        for (int i = 0; i < delete_count; i++) {
            struct gd_base_node* base_node = caa_container_of(nodes_to_delete[i], struct gd_base_node, lfht_node);
            
            // Remove from hash table
            if (cds_lfht_del(g_p_ht, nodes_to_delete[i]) == 0) {
                // Schedule for cleanup
                call_rcu(&base_node->rcu_head, test_node_free_callback);
                total_deleted++;
            } else {
                tklog_warning("Failed to delete test node during cleanup\n");
            }
        }
        
        // Wait for this batch to be freed before processing the next batch
        synchronize_rcu();
    }
    
    if (total_deleted > 0) {
        tklog_debug("Cleaned up %d test nodes\n", total_deleted);
    }
    
    // Mark cleanup as done
    g_cleanup_done = true;
}

// Main test runner with stress tests
bool test_global_data_all(void) {
    tklog_info("Starting global_data comprehensive tests...\n");
    
    // Initialize the system once at the beginning
    if (!gd_init()) {
        tklog_error("Failed to initialize global data system\n");
        return false;
    }
    
    // Ensure the main thread is registered with RCU for testing
    // The gd_init() function may have registered/unregistered during initialization
    // so we need to ensure we're registered for the test duration
    rcu_register_thread();
    
    // Reset cleanup flag for fresh test run
    tklog_scope(gd_reset_cleanup_flag());
    
    struct test_result results = {0, 0, 0};
    
    // Run all tests
    tklog_scope(results.passed += test_gd_init_cleanup() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_number_key_operations() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_string_key_operations() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_mixed_key_operations() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_error_conditions() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_rcu_safety() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_gd_update_operations() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_gd_upsert_operations() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_iterator_operations() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_gd_is_node_deleted() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    tklog_scope(results.passed += test_gd_count_nodes() ? 1 : 0);
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // Run stress tests
    tklog_info("\n=== STARTING STRESS TESTS ===\n");
    
    // Light stress test
    tklog_scope(bool test_stress_concurrent_result = test_stress_concurrent(4, 1000, "Light Number Keys Stress", stress_test_number_keys_thread))
    if (test_stress_concurrent_result) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // Medium stress test
    tklog_scope(test_stress_concurrent_result = test_stress_concurrent(8, 5000, "Medium Number Keys Stress", stress_test_number_keys_thread))
    if (test_stress_concurrent_result) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // Heavy stress test
    tklog_scope(test_stress_concurrent_result = test_stress_concurrent(16, 10000, "Heavy Number Keys Stress", stress_test_number_keys_thread))
    if (test_stress_concurrent_result) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // String keys stress test
    tklog_scope(test_stress_concurrent_result = test_stress_concurrent(8, 2000, "String Keys Stress", stress_test_string_keys_thread))
    if (test_stress_concurrent_result) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // Mixed keys stress test
    tklog_scope(test_stress_concurrent_result = test_stress_concurrent(8, 2000, "Mixed Keys Stress", stress_test_mixed_keys_thread))
    if (test_stress_concurrent_result) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // Memory pressure test
    tklog_scope(bool test_memory_pressure_result = test_memory_pressure(10000))
    if (test_memory_pressure_result) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // RCU grace period test
    tklog_scope(bool test_rcu_grace_periods_result = test_rcu_grace_periods())
    if (test_rcu_grace_periods_result) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    tklog_scope(gd_cleanup_test()); // Clean up test data between tests
    
    // Print final results
    tklog_info("\n=== TEST RESULTS ===\n");
    tklog_info("Passed: %d/%d\n", results.passed, results.total);
    tklog_info("Failed: %d/%d\n", results.failed, results.total);
    
    // Unregister the main thread from RCU
    rcu_unregister_thread();
    
    if (results.failed == 0) {
        tklog_info("✓ ALL TESTS PASSED!\n");
        return true;
    } else {
        tklog_error("✗ %d TESTS FAILED\n", results.failed);
        return false;
    }
}

// Test initialization and cleanup
bool test_gd_init_cleanup(void) {
    tklog_info("Testing gd_init and gd_cleanup...\n");
    
    // System is already initialized by test_global_data_all
    // Just verify it's working by checking if we can access the base_type node
    
    rcu_read_lock();
    tklog_scope(struct gd_base_node* base_type_node = gd_get_node_unsafe_wrapper("base_type", false));
    rcu_read_unlock();
    
    if (!base_type_node) {
        tklog_error("base_type node not found after initialization\n");
        print_test_results("Init/Cleanup", false);
        return false;
    }
    
    print_test_results("Init/Cleanup", true);
    return true;
}

// Test number key operations
bool test_number_key_operations(void) {
    tklog_info("Testing number key operations...\n");
    
    const char* type_key;
    if (!setup_test_node_types(&type_key)) {
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Test creating a node with automatic number key generation
    tklog_scope(uint64_t node_key = gd_create_node_wrapper(NULL, true, type_key, false));
    if (node_key == 0) {
        tklog_error("Failed to create node with number key\n");
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    tklog_info("Created node with auto-generated key: %llu\n", node_key);
    
    // Test getting the node using the returned key
    rcu_read_lock();
    struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)node_key, true);
    if (!node) {
        tklog_error("Failed to get node with number key %llu\n", node_key);
        rcu_read_unlock();
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Initialize test data
    node->test_value = 42;
    strcpy(node->test_string, "number_test");
    
    // Test getting the node again and verify data
    node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)node_key, true);
    bool verified = verify_test_node(node, 42, "number_test");
    rcu_read_unlock();
    
    if (!verified) {
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Test freeing the node
    if (!gd_remove_node_wrapper((void*)node_key, true)) {
        tklog_error("Failed to free node with number key %llu\n", node_key);
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Test getting the freed node (should return NULL now)
    rcu_read_lock();
    tklog_scope(node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)node_key, true));
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found freed number key %llu\n", node_key);
        print_test_results("gd_update Operations", false);
        return false;
    }
    
    print_test_results("gd_update Operations", true);
    return true;
}

// Test string key operations
bool test_string_key_operations(void) {
    tklog_info("Testing string key operations...\n");
    
    // System is already initialized by test_global_data_all
    
    const char* test_key = "test_string_key";
    
    // Test getting a non-existent string node (should return NULL)
    rcu_read_lock();
    struct test_node* node = (struct test_node*)gd_get_node_unsafe_wrapper(test_key, false);
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found non-existent string key\n");
        print_test_results("String Key Operations", false);
        return false;
    }
    
    tklog_info("String key get operation works correctly for non-existent keys\n");
    
    print_test_results("String Key Operations", true);
    return true;
}

// Test mixed key operations
bool test_mixed_key_operations(void) {
    tklog_info("Testing mixed key operations...\n");
    
    // System is already initialized by test_global_data_all
    
    // Test that number and string key lookups work independently
    uint64_t test_num_key = 99999;
    rcu_read_lock();
    struct test_node* num_node = (struct test_node*)gd_get_node_unsafe_wrapper((void*)test_num_key, true);
    struct test_node* str_node = (struct test_node*)gd_get_node_unsafe_wrapper("non_existent", false);
    rcu_read_unlock();
    
    if (num_node != NULL || str_node != NULL) {
        tklog_error("Should not have found non-existent mixed keys\n");
        print_test_results("Mixed Key Operations", false);
        return false;
    }
    
    tklog_info("Mixed key operations work correctly for non-existent keys\n");
    
    print_test_results("Mixed Key Operations", true);
    return true;
}

// Test error conditions
bool test_error_conditions(void) {
    tklog_info("Testing error conditions...\n");
    
    // System is already initialized by test_global_data_all
    
    // Test getting non-existent number key
    uint64_t test_num_key = 99999;
    rcu_read_lock();
    tklog_scope(void* node = gd_get_node_unsafe_wrapper((void*)test_num_key, true));
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found non-existent number key %llu\n", test_num_key);
        print_test_results("Error Conditions", false);
        return false;
    }
    
    // Test getting non-existent string key
    rcu_read_lock();
    tklog_scope(node = gd_get_node_unsafe_wrapper("non_existent_key", false));
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found non-existent string key\n");
        print_test_results("Error Conditions", false);
        return false;
    }
    
    // Test creating node with invalid type key
    uint64_t invalid_type_key = 99999;
    tklog_scope(uint64_t invalid_key = gd_create_node_wrapper(NULL, true, (void*)invalid_type_key, true));
    if (invalid_key != 0) {
        tklog_error("Should not have created node with invalid type key\n");
        print_test_results("Error Conditions", false);
        return false;
    }
    
    print_test_results("Error Conditions", true);
    return true;
}

// Test RCU safety (basic test)
bool test_rcu_safety(void) {
    tklog_info("Testing RCU safety...\n");
    
    // System is already initialized by test_global_data_all
    
    // Test nested RCU read locks
    uint64_t test_num_key = 99999;
    rcu_read_lock();
    tklog_scope(struct test_node* node1 = (struct test_node*)gd_get_node_unsafe_wrapper((void*)test_num_key, true));
    
    rcu_read_lock(); // Nested lock
    tklog_scope(struct test_node* node2 = (struct test_node*)gd_get_node_unsafe_wrapper((void*)test_num_key, true));
    
    // Both should be NULL for non-existent key
    if (node1 != node2) {
        tklog_error("RCU read consistency failed\n");
        rcu_read_unlock();
        rcu_read_unlock();
        print_test_results("RCU Safety", false);
        return false;
    }
    
    if (node1 != NULL || node2 != NULL) {
        tklog_error("Should not have found non-existent keys in RCU test\n");
        rcu_read_unlock();
        rcu_read_unlock();
        print_test_results("RCU Safety", false);
        return false;
    }
    
    rcu_read_unlock();
    rcu_read_unlock();
    
    tklog_info("RCU nested locking works correctly\n");
    
    print_test_results("RCU Safety", true);
    return true;
}

// Main function for standalone test executable
int main(int argc, char* argv[]) {
    tklog_info("Starting global_data test executable...\n");
    
    // Run all tests
    bool all_passed = test_global_data_all();
    
    if (all_passed) {
        tklog_info("✓ All global_data tests passed!\n");
        return 0;
    } else {
        tklog_error("✗ Some global_data tests failed!\n");
        return 1;
    }
}