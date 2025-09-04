#include "urcu_lfht_safe.h"
#include "tklog.h"
#include <assert.h>
#include <pthread.h>
#include <unistd.h>
#include <string.h>

/* Test data structure */
struct test_node {
    struct cds_lfht_node lfht_node;
    int value;
    char key[32];
};

/* Test hash table */
static struct cds_lfht *test_ht = NULL;

/* Key matching function */
static int test_key_match(struct cds_lfht_node *node, const void *key) {
    struct test_node *test_node = caa_container_of(node, struct test_node, lfht_node);
    return strcmp(test_node->key, (const char*)key) == 0;
}

/* Test basic RCU registration and locking */
static bool test_rcu_basic_operations(void) {
    tklog_info("Testing basic RCU operations...");
    
    // Test registration
    rcu_register_thread();
    
    // Test read locking
    rcu_read_lock();
    
    // Test nested locking
    rcu_read_lock();
    
    // Test unlock in reverse order
    rcu_read_unlock();
    rcu_read_unlock();
    
    // Test unregistration
    rcu_unregister_thread();
    
    tklog_info("Basic RCU operations passed");
    return true;
}

/* Test hash table operations */
static bool test_hash_table_operations(void) {
    tklog_info("Testing hash table operations...");
    
    // Create hash table
    test_ht = cds_lfht_new_flavor(16, 16, 1024, CDS_LFHT_AUTO_RESIZE, &urcu_memb_flavor, NULL);
    if (!test_ht) {
        tklog_error("Failed to create hash table");
        return false;
    }
    
    // Register thread for hash table operations
    rcu_register_thread();
    
    // Test lookup on empty table
    rcu_read_lock();
    struct cds_lfht_iter iter;
    cds_lfht_lookup(test_ht, 123, test_key_match, "test_key", &iter);
    struct cds_lfht_node *node = cds_lfht_iter_get_node(&iter);
    if (node != NULL) {
        tklog_error("Lookup on empty table should return NULL");
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Test adding a node
    struct test_node *test_node = malloc(sizeof(struct test_node));
    if (!test_node) {
        tklog_error("Failed to allocate test node");
        return false;
    }
    
    strcpy(test_node->key, "test_key");
    test_node->value = 42;
    cds_lfht_node_init(&test_node->lfht_node);
    
    cds_lfht_add(test_ht, 123, &test_node->lfht_node);
    
    // Test lookup after adding
    rcu_read_lock();
    cds_lfht_lookup(test_ht, 123, test_key_match, "test_key", &iter);
    node = cds_lfht_iter_get_node(&iter);
    if (node == NULL) {
        tklog_error("Lookup should find the added node");
        rcu_read_unlock();
        return false;
    }
    
    struct test_node *found_node = caa_container_of(node, struct test_node, lfht_node);
    if (found_node->value != 42 || strcmp(found_node->key, "test_key") != 0) {
        tklog_error("Found node has wrong data");
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Test unique add
    struct test_node *test_node2 = malloc(sizeof(struct test_node));
    if (!test_node2) {
        tklog_error("Failed to allocate second test node");
        return false;
    }
    
    strcpy(test_node2->key, "test_key");
    test_node2->value = 99;
    cds_lfht_node_init(&test_node2->lfht_node);
    
    struct cds_lfht_node *result = cds_lfht_add_unique(test_ht, 123, test_key_match, "test_key", &test_node2->lfht_node);
    if (result == &test_node2->lfht_node) {
        tklog_error("add_unique should fail for duplicate key");
        free(test_node2);
        return false;
    }
    
    // Should return the original node
    struct test_node *original_node = caa_container_of(result, struct test_node, lfht_node);
    if (original_node->value != 42) {
        tklog_error("add_unique should return original node");
        free(test_node2);
        return false;
    }
    
    free(test_node2);
    
    // Test node counting
    long split_count_before, split_count_after;
    unsigned long count;
    rcu_read_lock();
    cds_lfht_count_nodes(test_ht, &split_count_before, &count, &split_count_after);
    rcu_read_unlock();
    if (count != 1) {
        tklog_error("Node count should be 1, got %lu", count);
        return false;
    }
    
    // Test deletion
    if (cds_lfht_del(test_ht, &test_node->lfht_node) != 0) {
        tklog_error("Failed to delete node");
        return false;
    }
    
    // Verify deletion
    rcu_read_lock();
    cds_lfht_lookup(test_ht, 123, test_key_match, "test_key", &iter);
    node = cds_lfht_iter_get_node(&iter);
    if (node != NULL) {
        tklog_error("Node should be deleted");
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    // Cleanup
    free(test_node);
    cds_lfht_destroy(test_ht, NULL);
    rcu_unregister_thread();
    
    tklog_info("Hash table operations passed");
    return true;
}

/* Test error conditions */
static bool test_error_conditions(void) {
    tklog_info("Testing error conditions...");
    
    // Enable test mode to avoid critical/error logging for expected test conditions
    _rcu_set_test_mode(true);
    
    // Test read lock without registration - this should fail but not crash
    rcu_read_lock();
    tklog_info("Read lock without registration should have logged an error");
    
    // Test unregistration without registration - this should fail but not crash
    rcu_unregister_thread();
    tklog_info("Unregistration without registration should have logged an error");
    
    // Register and test invalid operations
    rcu_register_thread();
    
    // Create hash table for testing
    test_ht = cds_lfht_new_flavor(16, 16, 1024, CDS_LFHT_AUTO_RESIZE, &urcu_memb_flavor, NULL);
    if (!test_ht) {
        tklog_error("Failed to create hash table for error testing");
        _rcu_set_test_mode(false);
        return false;
    }
    
    // Test hash table operations with read lock (should work)
    rcu_read_lock();
    struct cds_lfht_iter iter;
    cds_lfht_lookup(test_ht, 123, test_key_match, "test_key", &iter);
    rcu_read_unlock();
    
    // Test write operations with read lock (should fail)
    {
        struct test_node *test_node = malloc(sizeof(struct test_node));
        if (test_node) {
            strcpy(test_node->key, "test_key");
            test_node->value = 42;
            cds_lfht_node_init(&test_node->lfht_node);
            
            rcu_read_lock();
            cds_lfht_add(test_ht, 123, &test_node->lfht_node);
            // This should log a debug message but not crash
            rcu_read_unlock();
            
            free(test_node);
        }
    }
    
    // Cleanup
    cds_lfht_destroy(test_ht, NULL);
    rcu_unregister_thread();
    
    // Disable test mode
    _rcu_set_test_mode(false);
    
    tklog_info("Error condition tests completed");
    return true;
}

/* Test thread safety */
static void* test_thread_func(void* arg) {
    int thread_id = *(int*)arg;
    
    // Register this thread
    rcu_register_thread();
    
    // Perform some operations
    for (int i = 0; i < 10; i++) {
        rcu_read_lock();
        usleep(1000); // Small delay
        rcu_read_unlock();
    }
    
    // Unregister
    rcu_unregister_thread();
    
    return NULL;
}

static bool test_thread_safety(void) {
    tklog_info("Testing thread safety...");
    
    const int num_threads = 4;
    pthread_t threads[num_threads];
    int thread_ids[num_threads];
    
    // Create threads
    for (int i = 0; i < num_threads; i++) {
        thread_ids[i] = i;
        if (pthread_create(&threads[i], NULL, test_thread_func, &thread_ids[i]) != 0) {
            tklog_error("Failed to create thread %d", i);
            return false;
        }
    }
    
    // Wait for threads to complete
    for (int i = 0; i < num_threads; i++) {
        if (pthread_join(threads[i], NULL) != 0) {
            tklog_error("Failed to join thread %d", i);
            return false;
        }
    }
    
    tklog_info("Thread safety tests passed");
    return true;
}

/* Main test function */
bool test_urcu_safety_wrapper(void) {
    tklog_info("Starting RCU/LFHT safety wrapper tests...");
    
    bool all_passed = true;
    
    all_passed &= test_rcu_basic_operations();
    all_passed &= test_hash_table_operations();
    all_passed &= test_error_conditions();
    all_passed &= test_thread_safety();
    
    if (all_passed) {
        tklog_info("All RCU/LFHT safety wrapper tests passed!");
    } else {
        tklog_error("Some RCU/LFHT safety wrapper tests failed!");
    }
    
    return all_passed;
}

/* Main function for standalone test executable */
int main(int argc, char *argv[]) {
    (void)argc;
    (void)argv;
    
    tklog_info("RCU/LFHT Safety Wrapper Test Suite");
    tklog_info("==================================");
    
    bool success = test_urcu_safety_wrapper();
    
    if (success) {
        tklog_info("All tests passed successfully!");
        return 0;
    } else {
        tklog_error("Some tests failed!");
        return 1;
    }
} 