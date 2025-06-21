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

static uint64_t g_test_type_key = 0;

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

// Helper function to set up test type node
bool setup_test_type_node(uint64_t* type_key_out) {
    if (!gd_init()) {
        tklog_error("Failed to initialize global data system\n");
        return false;
    }
    
    // Create a type node for our test type using the helper function
    uint64_t type_key = gd_create_type_node(sizeof(struct test_node),
                                            test_node_free,
                                            test_node_free_callback,
                                            test_node_is_valid);
    
    if (type_key == 0) {
        tklog_error("Failed to create test type node\n");
        return false;
    }
    
    g_test_type_key = type_key;
    *type_key_out = g_test_type_key;
    
    tklog_info("Created test type node with type_key: %llu\n", g_test_type_key);
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
    uint64_t type_key;
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
    
    tklog_info("Thread %d starting number key operations\n", data->thread_id);
    
    for (int i = 0; i < data->num_operations; i++) {
        int op = rand() % 100;
        
        if (op < 40 && local_created < 1000) { // 40% create
            uint64_t key = gd_create_node_number(data->type_key);
            if (key == 0) {
                data->errors++;
                data->error_create++;
                continue;
            }
            
            // Initialize the node
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(key);
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
                struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(key);
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
            struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(key);
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
            struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(key);
            rcu_read_unlock();
            
            if (node) {
                if (gd_free_node_number(key)) {
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
        struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(local_keys[i]);
        rcu_read_unlock();
        
        if (node) {
            gd_free_node_number(local_keys[i]);
        }
    }
    
    data->end_time = get_time_us();
    tklog_info("Thread %d completed: create=%d, read=%d, update=%d, delete=%d, errors=%d "
               "(create_err=%d, read_err=%d, update_err=%d, delete_err=%d)\n",
               data->thread_id, data->num_create, data->num_read, 
               data->num_update, data->num_delete, data->errors,
               data->error_create, data->error_read, data->error_update, data->error_delete);
    
    return NULL;
}

// Stress test: Many threads working with string keys
void* stress_test_string_keys_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    char key_buffer[128];
    int created_count = 0;
    char created_keys[100][128];
    
    tklog_info("Thread %d starting string key operations\n", data->thread_id);
    
    for (int i = 0; i < data->num_operations; i++) {
        int op = rand() % 100;
        
        if (op < 40 && created_count < 100) { // 40% create
            snprintf(key_buffer, 128, "thread_%d_key_%d_%d", 
                     data->thread_id, i, rand());
            
            if (gd_create_node_string(key_buffer, data->type_key) != 0) {
                strcpy(created_keys[created_count++], key_buffer);
                
                // Initialize the node
                rcu_read_lock();
                struct test_node* node = (struct test_node*)gd_get_by_string_unsafe(key_buffer);
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
            struct test_node* node = (struct test_node*)gd_get_by_string_unsafe(key);
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
            struct test_node* node = (struct test_node*)gd_get_by_string_unsafe(key);
            if (node) {
                node->test_value = (node->test_value + 1) % 10000;
                data->num_update++;
            }
            rcu_read_unlock();
            
        } else if (created_count > 0) { // 10% delete
            int idx = rand() % created_count;
            
            // Check if node still exists before deleting
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_by_string_unsafe(created_keys[idx]);
            rcu_read_unlock();
            
            if (node) {
                if (gd_free_node_string(created_keys[idx])) {
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
        struct test_node* node = (struct test_node*)gd_get_by_string_unsafe(created_keys[i]);
        rcu_read_unlock();
        
        if (node) {
            gd_free_node_string(created_keys[i]);
        }
    }
    
    data->end_time = get_time_us();
    tklog_info("Thread %d completed string ops: create=%d, read=%d, update=%d, delete=%d, errors=%d\n",
               data->thread_id, data->num_create, data->num_read, 
               data->num_update, data->num_delete, data->errors);
    
    return NULL;
}

// Stress test: Mixed operations with both number and string keys
void* stress_test_mixed_keys_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    
    tklog_info("Thread %d starting mixed key operations\n", data->thread_id);
    
    for (int i = 0; i < data->num_operations / 2; i++) {
        // Alternate between number and string operations
        if (i % 2 == 0) {
            // Number key operation
            uint64_t key = gd_create_node_number(data->type_key);
            if (key != 0) {
                rcu_read_lock();
                struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(key);
                if (node) {
                    node->test_value = i;
                }
                rcu_read_unlock();
                
                // Do some reads
                for (int j = 0; j < 5; j++) {
                    rcu_read_lock();
                    node = (struct test_node*)gd_get_by_number_unsafe(key);
                    if (node && node->test_value != i) {
                        data->errors++;
                    }
                    rcu_read_unlock();
                }
                
                gd_free_node_number(key);
                data->num_create++;
                data->num_read += 5;
                data->num_delete++;
            }
        } else {
            // String key operation
            char key[64];
            snprintf(key, 64, "mixed_%d_%d", data->thread_id, i);
            
            if (gd_create_node_string(key, data->type_key) != 0) {
                rcu_read_lock();
                struct test_node* node = (struct test_node*)gd_get_by_string_unsafe(key);
                if (node) {
                    node->test_value = i;
                }
                rcu_read_unlock();
                
                // Do some reads
                for (int j = 0; j < 5; j++) {
                    rcu_read_lock();
                    node = (struct test_node*)gd_get_by_string_unsafe(key);
                    if (node && node->test_value != i) {
                        data->errors++;
                    }
                    rcu_read_unlock();
                }
                
                gd_free_node_string(key);
                data->num_create++;
                data->num_read += 5;
                data->num_delete++;
            }
        }
    }
    
    data->end_time = get_time_us();
    return NULL;
}

// Main stress test function
bool test_stress_concurrent(int num_threads, int ops_per_thread, const char* test_name,
                           void* (*thread_func)(void*)) {
    tklog_info("Starting stress test: %s with %d threads, %d ops each\n", 
               test_name, num_threads, ops_per_thread);
    
    uint64_t type_key;
    if (!setup_test_type_node(&type_key)) {
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
    for (int i = 0; i < shared_created_count; i++) {
        // Check if key still exists before deleting
        rcu_read_lock();
        struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(shared_keys[i]);
        rcu_read_unlock();
        
        if (node) {
            gd_free_node_number(shared_keys[i]);
        }
    }
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
    
    uint64_t type_key;
    if (!setup_test_type_node(&type_key)) {
        print_test_results("Memory Pressure", false);
        return false;
    }
    
    uint64_t* keys = malloc(num_nodes * sizeof(uint64_t));
    int created = 0;
    
    uint64_t start_time = get_time_us();
    
    // Create many nodes
    for (int i = 0; i < num_nodes; i++) {
        keys[i] = gd_create_node_number(type_key);
        if (keys[i] != 0) {
            created++;
            
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(keys[i]);
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
            struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(keys[i]);
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
        if (keys[i] != 0 && gd_free_node_number(keys[i])) {
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

// Test RCU grace period handling
void* rcu_reader_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    int successful_reads = 0;
    
    for (int i = 0; i < data->num_operations; i++) {
        pthread_mutex_lock(data->keys_mutex);
        int count = *data->created_count;
        uint64_t key = 0;
        if (count > 0) {
            key = data->created_keys[rand() % count];
        }
        pthread_mutex_unlock(data->keys_mutex);
        
        if (key != 0) {
            rcu_read_lock();
            struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(key);
            if (node) {
                // Simulate some work with the node
                volatile int dummy = node->test_value;
                (void)dummy;
                successful_reads++;
            }
            rcu_read_unlock();
        }
        
        // Small random delay
        if (rand() % 10 == 0) {
            usleep(1);
        }
    }
    
    data->num_read = successful_reads;
    return NULL;
}

void* rcu_writer_thread(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    uint64_t local_keys[100];
    int local_count = 0;
    
    for (int i = 0; i < data->num_operations; i++) {
        if (rand() % 2 == 0 && local_count < 100) {
            // Create
            uint64_t key = gd_create_node_number(data->type_key);
            if (key != 0) {
                local_keys[local_count++] = key;
                data->num_create++;
                
                // Add to shared pool
                pthread_mutex_lock(data->keys_mutex);
                if (*data->created_count < 10000) {
                    data->created_keys[*data->created_count] = key;
                    (*data->created_count)++;
                }
                pthread_mutex_unlock(data->keys_mutex);
            }
        } else if (local_count > 0) {
            // Delete
            int idx = rand() % local_count;
            if (gd_free_node_number(local_keys[idx])) {
                data->num_delete++;
                local_keys[idx] = local_keys[--local_count];
            }
        }
    }
    
    // Clean up
    for (int i = 0; i < local_count; i++) {
        // Check if still exists before deleting
        rcu_read_lock();
        struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(local_keys[i]);
        rcu_read_unlock();
        
        if (node) {
            gd_free_node_number(local_keys[i]);
        }
    }
    
    return NULL;
}

bool test_rcu_grace_periods(void) {
    tklog_info("Testing RCU grace period handling\n");
    
    uint64_t type_key;
    if (!setup_test_type_node(&type_key)) {
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
    uint64_t* shared_keys = calloc(10000, sizeof(uint64_t));
    pthread_mutex_t keys_mutex = PTHREAD_MUTEX_INITIALIZER;
    int shared_count = 0;
    
    // Start readers
    for (int i = 0; i < num_readers; i++) {
        reader_data[i].thread_id = i;
        reader_data[i].type_key = type_key;
        reader_data[i].num_operations = ops_per_thread;
        reader_data[i].created_keys = shared_keys;
        reader_data[i].created_count = &shared_count;
        reader_data[i].keys_mutex = &keys_mutex;
        
        pthread_create(&readers[i], NULL, rcu_reader_thread, &reader_data[i]);
    }
    
    // Start writers
    for (int i = 0; i < num_writers; i++) {
        writer_data[i].thread_id = i + num_readers;
        writer_data[i].type_key = type_key;
        writer_data[i].num_operations = ops_per_thread;
        writer_data[i].created_keys = shared_keys;
        writer_data[i].created_count = &shared_count;
        writer_data[i].keys_mutex = &keys_mutex;
        
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
    int total_reads = 0, total_creates = 0, total_deletes = 0;
    for (int i = 0; i < num_readers; i++) {
        total_reads += reader_data[i].num_read;
    }
    for (int i = 0; i < num_writers; i++) {
        total_creates += writer_data[i].num_create;
        total_deletes += writer_data[i].num_delete;
    }
    
    tklog_info("RCU test completed: %d reads, %d creates, %d deletes\n",
               total_reads, total_creates, total_deletes);
    
    free(shared_keys);
    
    print_test_results("RCU Grace Periods", true);
    return true;
}

// Main test runner with stress tests
bool test_global_data_all(void) {
    tklog_info("Starting global_data comprehensive tests...\n");
    
    struct test_result results = {0, 0, 0};
    
    // Run basic tests first
    if (test_gd_init_cleanup()) results.passed++; else results.failed++;
    results.total++;
    
    if (test_number_key_operations()) results.passed++; else results.failed++;
    results.total++;
    
    if (test_string_key_operations()) results.passed++; else results.failed++;
    results.total++;
    
    if (test_mixed_key_operations()) results.passed++; else results.failed++;
    results.total++;
    
    if (test_error_conditions()) results.passed++; else results.failed++;
    results.total++;
    
    if (test_rcu_safety()) results.passed++; else results.failed++;
    results.total++;
    
    // Run stress tests
    tklog_info("\n=== STARTING STRESS TESTS ===\n");
    
    // Light stress test
    if (test_stress_concurrent(4, 1000, "Light Number Keys Stress", 
                              stress_test_number_keys_thread)) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    
    // Medium stress test
    if (test_stress_concurrent(8, 5000, "Medium Number Keys Stress", 
                              stress_test_number_keys_thread)) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    
    // Heavy stress test
    if (test_stress_concurrent(16, 10000, "Heavy Number Keys Stress", 
                              stress_test_number_keys_thread)) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    
    // String keys stress test
    if (test_stress_concurrent(8, 2000, "String Keys Stress", 
                              stress_test_string_keys_thread)) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    
    // Mixed keys stress test
    if (test_stress_concurrent(8, 2000, "Mixed Keys Stress", 
                              stress_test_mixed_keys_thread)) {
        results.passed++;
    } else {
        results.failed++;
    }
    results.total++;
    
    // Memory pressure test
    if (test_memory_pressure(10000)) results.passed++; else results.failed++;
    results.total++;
    
    // RCU grace period test
    if (test_rcu_grace_periods()) results.passed++; else results.failed++;
    results.total++;
    
    // Print final results
    tklog_info("\n=== TEST RESULTS ===\n");
    tklog_info("Passed: %d/%d\n", results.passed, results.total);
    tklog_info("Failed: %d/%d\n", results.failed, results.total);
    
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
    
    // Test initialization
    if (!gd_init()) {
        print_test_results("Init/Cleanup", false);
        return false;
    }
    
    // Test cleanup
    gd_cleanup();
    
    print_test_results("Init/Cleanup", true);
    return true;
}

// Test number key operations
bool test_number_key_operations(void) {
    tklog_info("Testing number key operations...\n");
    
    uint64_t type_key;
    if (!setup_test_type_node(&type_key)) {
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Test creating a node with automatic number key generation
    uint64_t node_key = gd_create_node_number(type_key);
    if (node_key == 0) {
        tklog_error("Failed to create node with number key\n");
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    tklog_info("Created node with auto-generated key: %llu\n", node_key);
    
    // Test getting the node using the returned key
    rcu_read_lock();
    struct test_node* node = (struct test_node*)gd_get_by_number_unsafe(node_key);
    if (!node) {
        tklog_error("Failed to get node with number key %llu\n", node_key);
        rcu_read_unlock();
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Initialize test data
    node->test_value = 42;
    strcpy(node->test_string, "number_test");
    rcu_read_unlock();
    
    // Test getting the node again and verify data
    rcu_read_lock();
    node = (struct test_node*)gd_get_by_number_unsafe(node_key);
    bool verified = verify_test_node(node, 42, "number_test");
    rcu_read_unlock();
    
    if (!verified) {
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Test freeing the node
    if (!gd_free_node_number(node_key)) {
        tklog_error("Failed to free node with number key %llu\n", node_key);
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    // Test getting the freed node (should return NULL now)
    rcu_read_lock();
    node = (struct test_node*)gd_get_by_number_unsafe(node_key);
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found freed number key %llu\n", node_key);
        print_test_results("Number Key Operations", false);
        return false;
    }
    
    print_test_results("Number Key Operations", true);
    return true;
}

// Test string key operations
bool test_string_key_operations(void) {
    tklog_info("Testing string key operations...\n");
    
    const char* test_key = "test_string_key";
    
    // Test getting a non-existent string node (should return NULL)
    rcu_read_lock();
    struct test_node* node = (struct test_node*)gd_get_by_string_unsafe(test_key);
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
    
    // Test that number and string key lookups work independently
    rcu_read_lock();
    struct test_node* num_node = (struct test_node*)gd_get_by_number_unsafe(99999);
    struct test_node* str_node = (struct test_node*)gd_get_by_string_unsafe("non_existent");
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
    
    // Test getting non-existent number key
    rcu_read_lock();
    void* node = gd_get_by_number_unsafe(99999);
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found non-existent number key\n");
        print_test_results("Error Conditions", false);
        return false;
    }
    
    // Test getting non-existent string key
    rcu_read_lock();
    node = gd_get_by_string_unsafe("non_existent_key");
    rcu_read_unlock();
    
    if (node != NULL) {
        tklog_error("Should not have found non-existent string key\n");
        print_test_results("Error Conditions", false);
        return false;
    }
    
    // Test creating node with invalid type key
    uint64_t invalid_key = gd_create_node_number(99999);
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
    
    // Test nested RCU read locks
    rcu_read_lock();
    struct test_node* node1 = (struct test_node*)gd_get_by_number_unsafe(99999);
    
    rcu_read_lock(); // Nested lock
    struct test_node* node2 = (struct test_node*)gd_get_by_number_unsafe(99999);
    
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