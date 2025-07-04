#include "global_data/core.h"
#include "tklog.h"
#include <urcu.h>
#include <pthread.h>
#include <assert.h>
#include <unistd.h>
#include <time.h>

// Test node structure
struct test_node {
    struct gd_base_node base;
    int value;
    char data[64];
};

// Test node type callbacks
static bool test_node_free(struct gd_base_node* node) {
    if (!node) {
        tklog_debug("test_node_free called with NULL node\n");
        return false;
    }
    
    struct test_node* tn = (struct test_node*)node;
    tklog_debug("Freeing test node with value: %d, data: %s\n", tn->value, tn->data);
    
    // Free string keys if allocated
    if (!node->key_is_number && node->key.string) {
        free(node->key.string);
        node->key.string = NULL;
    }
    if (!node->type_key_is_number && node->type_key.string) {
        free(node->type_key.string);
        node->type_key.string = NULL;
    }
    
    // Clear the node data to help with debugging
    memset(tn->data, 0, sizeof(tn->data));
    tn->value = -1;
    
    free(node);
    return true;
}

static void test_node_free_callback(struct rcu_head* head) {
    struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
    tklog_debug("RCU callback freeing test node\n");
    test_node_free(node);
}

static bool test_node_is_valid(struct gd_base_node* node) {
    if (!node) {
        tklog_debug("test_node_is_valid called with NULL node\n");
        return false;
    }
    
    struct test_node* tn = (struct test_node*)node;
    bool valid = tn->value >= 0 && tn->value < 1000000;
    
    if (!valid) {
        tklog_warning("Invalid test node value: %d\n", tn->value);
    }
    
    return valid;
}

// Helper to create test node type
static bool create_test_node_type(void) {
    tklog_info("Creating test node type...\n");
    
    union gd_key type_key = gd_key_create(0, "test_node_type", false);
    
    // Check if already exists
    rcu_read_lock();
    tklog_scope(struct gd_base_node* existing = gd_node_get(type_key, false));
    rcu_read_unlock();
    
    if (existing) {
        tklog_warning("Test node type already exists\n");
        return true;
    }
    
    // Create type node
    tklog_scope(struct gd_base_type_node* type_node = (struct gd_base_type_node*)gd_base_node_create(
        type_key, false,
        gd_base_type_get_key_copy(), gd_base_type_key_is_number(),
        sizeof(struct gd_base_type_node)
    ));
    
    if (!type_node) {
        tklog_error("Failed to create test node type node\n");
        return false;
    }
    
    type_node->fn_free_node = test_node_free;
    type_node->fn_free_node_callback = test_node_free_callback;
    type_node->fn_is_valid = test_node_is_valid;
    type_node->type_size = sizeof(struct test_node);
    
    tklog_scope(bool insert_result = gd_node_insert(&type_node->base));
    if (!insert_result) {
        tklog_error("Failed to insert test node type\n");
        return false;
    }
    
    tklog_info("Successfully created test node type\n");
    return true;
}

// Create a test node with given value
static struct test_node* create_test_node(uint64_t key, int value) {
    tklog_debug("Creating test node with key: %llu, value: %d\n", key, value);
    
    tklog_scope(struct test_node* node = (struct test_node*)gd_base_node_create(
        gd_key_create(key, NULL, true), true,
        gd_key_create(0, "test_node_type", false), false,
        sizeof(struct test_node)
    ));
    
    if (node) {
        node->value = value;
        snprintf(node->data, 64, "test_data_%d", value);
        tklog_debug("Created test node successfully\n");
    } else {
        tklog_error("Failed to create test node\n");
    }
    
    return node;
}

// Test basic operations
static bool test_basic_operations(void) {
    tklog_info("Testing basic operations...\n");
    
    // Create a node
    tklog_scope(struct test_node* node = create_test_node(1001, 42));
    if (!node) {
        tklog_error("Failed to create test node for basic operations test\n");
        return false;
    }
    
    // Insert it
    tklog_scope(bool insert_result = gd_node_insert(&node->base));
    if (!insert_result) {
        tklog_error("Failed to insert test node\n");
        return false;
    }
    
    // Read it back
    rcu_read_lock();
    tklog_scope(struct test_node* found = (struct test_node*)gd_node_get(gd_key_create(1001, NULL, true), true));
    if (!found) {
        tklog_error("Failed to retrieve inserted test node\n");
        rcu_read_unlock();
        return false;
    }
    
    if (found->value != 42) {
        tklog_error("Retrieved node has wrong value: expected 42, got %d\n", found->value);
        rcu_read_unlock();
        return false;
    }
    
    if (strcmp(found->data, "test_data_42") != 0) {
        tklog_error("Retrieved node has wrong data: expected 'test_data_42', got '%s'\n", found->data);
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Update it
    tklog_scope(struct test_node* updated = create_test_node(1001, 84));
    if (!updated) {
        tklog_error("Failed to create updated test node\n");
        return false;
    }
    
    tklog_scope(bool update_result = gd_node_update(&updated->base));
    if (!update_result) {
        tklog_error("Failed to update test node\n");
        return false;
    }
    
    // Verify update
    rcu_read_lock();
    tklog_scope(found = (struct test_node*)gd_node_get(gd_key_create(1001, NULL, true), true));
    if (!found) {
        tklog_error("Failed to retrieve updated test node\n");
        rcu_read_unlock();
        return false;
    }
    
    if (found->value != 84) {
        tklog_error("Updated node has wrong value: expected 84, got %d\n", found->value);
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Remove it
    tklog_scope(bool remove_result = gd_node_remove(gd_key_create(1001, NULL, true), true));
    if (!remove_result) {
        tklog_error("Failed to remove test node\n");
        return false;
    }
    
    // Verify removal
    rcu_read_lock();
    tklog_scope(found = (struct test_node*)gd_node_get(gd_key_create(1001, NULL, true), true));
    if (found != NULL) {
        tklog_error("Node still exists after removal\n");
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    tklog_info("✓ Basic operations test passed\n");
    return true;
}

// Test string keys
static bool test_string_keys(void) {
    tklog_info("Testing string keys...\n");
    
    // Create node with string key
    tklog_scope(struct test_node* node = (struct test_node*)gd_base_node_create(
        gd_key_create(0, "string_key_test", false), false,
        gd_key_create(0, "test_node_type", false), false,
        sizeof(struct test_node)
    ));
    
    if (!node) {
        tklog_error("Failed to create string key test node\n");
        return false;
    }
    
    node->value = 123;
    
    tklog_scope(bool insert_result = gd_node_insert(&node->base));
    if (!insert_result) {
        tklog_error("Failed to insert string key test node\n");
        return false;
    }
    
    // Read it back
    rcu_read_lock();
    tklog_scope(struct test_node* found = (struct test_node*)gd_node_get(
        gd_key_create(0, "string_key_test", false), false
    ));
    
    if (!found) {
        tklog_error("Failed to retrieve string key test node\n");
        rcu_read_unlock();
        return false;
    }
    
    if (found->value != 123) {
        tklog_error("String key node has wrong value: expected 123, got %d\n", found->value);
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Clean up
    tklog_scope(bool remove_result = gd_node_remove(gd_key_create(0, "string_key_test", false), false));
    if (!remove_result) {
        tklog_error("Failed to remove string key test node\n");
        return false;
    }
    
    tklog_info("✓ String keys test passed\n");
    return true;
}

// Test iterators
static bool test_iterators(void) {
    tklog_info("Testing iterators...\n");
    
    // Create multiple nodes
    const int count = 10;
    for (int i = 0; i < count; i++) {
        tklog_scope(struct test_node* node = create_test_node(2000 + i, i * 10));
        if (!node) {
            tklog_error("Failed to create test node %d for iterator test\n", i);
            return false;
        }
        
        tklog_scope(bool insert_result = gd_node_insert(&node->base));
        if (!insert_result) {
            tklog_error("Failed to insert test node %d for iterator test\n", i);
            return false;
        }
    }
    
    // Count nodes using iterator
    int found = 0;
    struct cds_lfht_iter iter;
    
    rcu_read_lock();
    tklog_scope(bool first_result = gd_iter_first(&iter));
    if (first_result) {
        do {
            tklog_scope(struct gd_base_node* node = gd_iter_get_node(&iter));
            if (node) found++;
        } while (gd_iter_next(&iter));
    }
    rcu_read_unlock();
    
    if (found < count) {
        tklog_error("Iterator found %d nodes, expected at least %d\n", found, count);
        return false;
    }
    
    // Test lookup iterator
    rcu_read_lock();
    tklog_scope(bool lookup_result = gd_iter_lookup(gd_key_create(2005, NULL, true), true, &iter));
    if (!lookup_result) {
        tklog_error("Failed to lookup key 2005 with iterator\n");
        rcu_read_unlock();
        return false;
    }
    
    tklog_scope(struct test_node* node = (struct test_node*)gd_iter_get_node(&iter));
    if (!node) {
        tklog_error("Iterator lookup returned NULL node\n");
        rcu_read_unlock();
        return false;
    }
    
    if (node->value != 50) {
        tklog_error("Lookup node has wrong value: expected 50, got %d\n", node->value);
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Clean up
    for (int i = 0; i < count; i++) {
        tklog_scope(bool remove_result = gd_node_remove(gd_key_create(2000 + i, NULL, true), true));
        if (!remove_result) {
            tklog_warning("Failed to remove test node %d during cleanup\n", i);
        }
    }
    
    // Wait for cleanup to complete
    synchronize_rcu();
    
    tklog_info("✓ Iterator test passed\n");
    return true;
}

// Thread data for concurrent tests
struct thread_data {
    int thread_id;
    int operations;
    int start_key;
    int successful_ops;
};

// Worker thread for concurrent test
static void* concurrent_worker(void* arg) {
    struct thread_data* data = (struct thread_data*)arg;
    
    tklog_info("Thread %d starting concurrent operations\n", data->thread_id);
    rcu_register_thread();
    
    for (int i = 0; i < data->operations; i++) {
        uint64_t key = data->start_key + (i % 100);
        int op = rand() % 4;
        
        switch (op) {
            case 0: { // Insert/Upsert
                tklog_scope(struct test_node* node = create_test_node(key, data->thread_id * 1000 + i));
                if (node) {
                    tklog_scope(bool upsert_result = gd_node_upsert(&node->base));
                    if (upsert_result) {
                        data->successful_ops++;
                    } else {
                        tklog_debug("Thread %d: upsert failed for key %llu\n", data->thread_id, key);
                    }
                }
                break;
            }
            case 1: { // Read
                rcu_read_lock();
                tklog_scope(struct test_node* found = (struct test_node*)gd_node_get(
                    gd_key_create(key, NULL, true), true
                ));
                if (found && test_node_is_valid(&found->base)) {
                    data->successful_ops++;
                }
                rcu_read_unlock();
                break;
            }
            case 2: { // Update
                tklog_scope(struct test_node* updated = create_test_node(key, rand() % 1000));
                if (updated) {
                    tklog_scope(bool update_result = gd_node_update(&updated->base));
                    if (update_result) {
                        data->successful_ops++;
                    } else {
                        tklog_debug("Thread %d: update failed for key %llu\n", data->thread_id, key);
                        free(updated);
                    }
                }
                break;
            }
            case 3: { // Delete
                tklog_scope(bool remove_result = gd_node_remove(gd_key_create(key, NULL, true), true));
                if (remove_result) {
                    data->successful_ops++;
                }
                break;
            }
        }
        
        // Small delay to simulate real work
        if (i % 100 == 0) {
            usleep(1);
        }
    }
    
    tklog_info("Thread %d completed with %d successful operations\n", data->thread_id, data->successful_ops);
    rcu_unregister_thread();
    return NULL;
}

// Test concurrent operations
static bool test_concurrent_operations(void) {
    tklog_info("Testing concurrent operations...\n");
    
    const int num_threads = 8;
    const int ops_per_thread = 10000;
    pthread_t threads[num_threads];
    struct thread_data thread_data[num_threads];
    
    // Start threads
    for (int i = 0; i < num_threads; i++) {
        thread_data[i].thread_id = i;
        thread_data[i].operations = ops_per_thread;
        thread_data[i].start_key = 10000 + (i * 1000);
        thread_data[i].successful_ops = 0;
        
        tklog_scope(int create_result = pthread_create(&threads[i], NULL, concurrent_worker, &thread_data[i]));
        if (create_result != 0) {
            tklog_error("Failed to create thread %d\n", i);
            return false;
        }
    }
    
    // Wait for completion
    int total_ops = 0;
    for (int i = 0; i < num_threads; i++) {
        tklog_scope(int join_result = pthread_join(threads[i], NULL));
        if (join_result != 0) {
            tklog_error("Failed to join thread %d\n", i);
        }
        total_ops += thread_data[i].successful_ops;
    }
    
    tklog_info("Completed %d successful operations across %d threads\n", total_ops, num_threads);
    
    // Clean up any remaining nodes - collect all keys first, then remove
    struct cds_lfht_iter iter;
    uint64_t keys_to_remove[1000]; // Should be enough for test data
    int key_count = 0;
    
    rcu_read_lock();
    tklog_scope(bool first_result = gd_iter_first(&iter));
    if (first_result) {
        do {
            tklog_scope(struct gd_base_node* node = gd_iter_get_node(&iter));
            if (node && node->key_is_number && node->key.number >= 10000 && key_count < 1000) {
                keys_to_remove[key_count++] = node->key.number;
            }
        } while (gd_iter_next(&iter));
    }
    rcu_read_unlock();
    
    // Now remove all collected nodes
    for (int i = 0; i < key_count; i++) {
        tklog_scope(bool remove_result = gd_node_remove(gd_key_create(keys_to_remove[i], NULL, true), true));
        if (!remove_result) {
            tklog_debug("Failed to remove cleanup node with key %llu\n", keys_to_remove[i]);
        }
    }
    
    // Wait for all deletions to complete
    synchronize_rcu();
    
    tklog_info("✓ Concurrent operations test passed\n");
    return true;
}

// Helper function for RCU grace period test deleter thread
static void* rcu_deleter_thread(void* arg) {
    rcu_register_thread();
    tklog_scope(bool remove_result = gd_node_remove(gd_key_create(5000, NULL, true), true));
    if (!remove_result) {
        tklog_error("Failed to remove test node in deleter thread\n");
    }
    rcu_unregister_thread();
    return NULL;
}

// Test RCU grace periods
static bool test_rcu_grace_periods(void) {
    tklog_info("Testing RCU grace periods...\n");
    
    // Create a node
    tklog_scope(struct test_node* node = create_test_node(5000, 999));
    if (!node) {
        tklog_error("Failed to create test node for RCU grace period test\n");
        return false;
    }
    
    tklog_scope(bool insert_result = gd_node_insert(&node->base));
    if (!insert_result) {
        tklog_error("Failed to insert test node for RCU grace period test\n");
        return false;
    }
    
    // Hold a reference in read-side critical section
    rcu_read_lock();
    tklog_scope(struct test_node* ref = (struct test_node*)gd_node_get(
        gd_key_create(5000, NULL, true), true
    ));
    if (!ref) {
        tklog_error("Failed to get test node for RCU grace period test\n");
        rcu_read_unlock();
        return false;
    }
    
    if (ref->value != 999) {
        tklog_error("Test node has wrong value: expected 999, got %d\n", ref->value);
        rcu_read_unlock();
        return false;
    }
    
    // Another thread deletes the node
    pthread_t deleter;
    tklog_scope(int create_result = pthread_create(&deleter, NULL, rcu_deleter_thread, NULL));
    
    if (create_result != 0) {
        tklog_error("Failed to create deleter thread\n");
        rcu_read_unlock();
        return false;
    }
    
    tklog_scope(int join_result = pthread_join(deleter, NULL));
    if (join_result != 0) {
        tklog_error("Failed to join deleter thread\n");
    }
    
    // We can still access the node safely
    if (ref->value != 999) {
        tklog_error("Node value changed during RCU grace period: expected 999, got %d\n", ref->value);
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // After unlock and grace period, node should be gone
    rcu_read_lock();
    tklog_scope(ref = (struct test_node*)gd_node_get(gd_key_create(5000, NULL, true), true));
    if (ref != NULL) {
        tklog_error("Node still exists after RCU grace period\n");
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    tklog_info("✓ RCU grace periods test passed\n");
    return true;
}

// Test edge cases
static bool test_edge_cases(void) {
    tklog_info("Testing edge cases...\n");
    
    // Test NULL operations
    tklog_scope(bool insert_result = gd_node_insert(NULL));
    if (insert_result) {
        tklog_error("gd_node_insert should fail with NULL\n");
        return false;
    }
    
    tklog_scope(bool update_result = gd_node_update(NULL));
    if (update_result) {
        tklog_error("gd_node_update should fail with NULL\n");
        return false;
    }
    
    tklog_scope(bool upsert_result = gd_node_upsert(NULL));
    if (upsert_result) {
        tklog_error("gd_node_upsert should fail with NULL\n");
        return false;
    }
    
    tklog_scope(bool is_deleted_result = gd_node_is_deleted(NULL));
    if (!is_deleted_result) {
        tklog_error("gd_node_is_deleted should return true for NULL\n");
        return false;
    }
    
    // Test non-existent keys
    rcu_read_lock();
    tklog_scope(struct gd_base_node* num_node = gd_node_get(gd_key_create(99999, NULL, true), true));
    if (num_node != NULL) {
        tklog_error("Should not find non-existent number key\n");
        rcu_read_unlock();
        return false;
    }
    
    tklog_scope(struct gd_base_node* str_node = gd_node_get(gd_key_create(0, "nonexistent", false), false));
    if (str_node != NULL) {
        tklog_error("Should not find non-existent string key\n");
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Test update/remove non-existent
    tklog_scope(struct test_node* node = create_test_node(99999, 0));
    if (node) {
        tklog_scope(bool update_result = gd_node_update(&node->base));
        if (update_result) {
            tklog_error("Update should fail for non-existent node\n");
            return false;
        }
        free(node);
    }
    
    tklog_scope(bool remove_result = gd_node_remove(gd_key_create(99999, NULL, true), true));
    if (remove_result) {
        tklog_error("Remove should fail for non-existent node\n");
        return false;
    }
    
    // Test invalid type key
    tklog_scope(node = (struct test_node*)gd_base_node_create(
        gd_key_create(88888, NULL, true), true,
        gd_key_create(0, "invalid_type", false), false,
        sizeof(struct test_node)
    ));
    if (node) {
        tklog_scope(bool insert_result = gd_node_insert(&node->base));
        if (insert_result) {
            tklog_error("Insert should fail with invalid type key\n");
            return false;
        }
        free(node);
    }
    
    tklog_info("✓ Edge cases test passed\n");
    return true;
}

// Test node count
static bool test_node_count(void) {
    tklog_info("Testing node count...\n");
    
    unsigned long initial_count = gd_nodes_count();
    tklog_info("Initial count: %lu\n", initial_count);
    
    // Add nodes
    const int to_add = 5;
    for (int i = 0; i < to_add; i++) {
        tklog_scope(struct test_node* node = create_test_node(7000 + i, i));
        if (!node) {
            tklog_error("Failed to create test node %d for count test\n", i);
            return false;
        }
        
        tklog_scope(bool insert_result = gd_node_insert(&node->base));
        if (!insert_result) {
            tklog_error("Failed to insert test node %d for count test\n", i);
            return false;
        }
    }
    
    unsigned long after_add = gd_nodes_count();
    if (after_add != initial_count + to_add) {
        tklog_error("Node count mismatch after add: expected %lu, got %lu\n", 
                   initial_count + to_add, after_add);
        return false;
    }
    
    // Remove some
    for (int i = 0; i < 3; i++) {
        tklog_scope(bool remove_result = gd_node_remove(gd_key_create(7000 + i, NULL, true), true));
        if (!remove_result) {
            tklog_error("Failed to remove test node %d for count test\n", i);
            return false;
        }
    }
    
    synchronize_rcu(); // Wait for deletions
    unsigned long after_remove = gd_nodes_count();
    if (after_remove != initial_count + 2) {
        tklog_error("Node count mismatch after remove: expected %lu, got %lu\n", 
                   initial_count + 2, after_remove);
        return false;
    }
    
    // Clean up
    for (int i = 3; i < to_add; i++) {
        tklog_scope(bool remove_result = gd_node_remove(gd_key_create(7000 + i, NULL, true), true));
        if (!remove_result) {
            tklog_warning("Failed to remove test node %d during cleanup\n", i);
        }
    }
    
    // Wait for cleanup to complete
    synchronize_rcu();
    
    tklog_info("✓ Node count test passed\n");
    return true;
}

// Main test runner
int main(void) {
    tklog_info("Starting global_data tests...\n");
    
    // Initialize system
    tklog_scope(bool init_result = gd_init());
    if (!init_result) {
        tklog_error("Failed to initialize global data system\n");
        return 1;
    }
    
    // Register main thread
    rcu_register_thread();
    
    // Create test node type
    tklog_scope(bool type_result = create_test_node_type());
    if (!type_result) {
        tklog_error("Failed to create test node type\n");
        rcu_unregister_thread();
        gd_cleanup();
        return 1;
    }
    
    // Run tests
    bool all_passed = true;
    
    tklog_scope(bool basic_result = test_basic_operations());
    if (!basic_result) {
        tklog_error("Basic operations test failed\n");
        all_passed = false;
    }
    
    tklog_scope(bool string_result = test_string_keys());
    if (!string_result) {
        tklog_error("String keys test failed\n");
        all_passed = false;
    }
    
    tklog_scope(bool iterator_result = test_iterators());
    if (!iterator_result) {
        tklog_error("Iterator test failed\n");
        all_passed = false;
    }
    
    tklog_scope(bool edge_result = test_edge_cases());
    if (!edge_result) {
        tklog_error("Edge cases test failed\n");
        all_passed = false;
    }

    tklog_scope(bool count_result = test_node_count());
    if (!count_result) {
        tklog_error("Node count test failed\n");
        all_passed = false;
    }
    
    tklog_scope(bool concurrent_result = test_concurrent_operations());
    if (!concurrent_result) {
        tklog_error("Concurrent operations test failed\n");
        all_passed = false;
    }
    
    tklog_scope(bool rcu_result = test_rcu_grace_periods());
    if (!rcu_result) {
        tklog_error("RCU grace periods test failed\n");
        all_passed = false;
    }
    
    // Cleanup
    rcu_unregister_thread();
    
    // Wait for any pending RCU callbacks before cleanup
    synchronize_rcu();
    
    tklog_scope(gd_cleanup());
    
    // Wait for all RCU callbacks to complete
    rcu_barrier();
    
    // Additional barrier to ensure all memory is freed
    synchronize_rcu();
    
    // Final barrier to catch any remaining callbacks
    rcu_barrier();
    
    if (all_passed) {
        tklog_info("\n✓ All tests PASSED!\n");
        return 0;
    } else {
        tklog_error("\n✗ Some tests FAILED!\n");
        return 1;
    }
}