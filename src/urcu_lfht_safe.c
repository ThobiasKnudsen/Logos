#define LFHT_SAFE_INTERNAL  /* Prevent macro redefinition in this file */
#include "urcu_lfht_safe.h"
#include "code_monitoring.h"
#include <errno.h>
#include <unistd.h>


/* Global function pointer for getting node size and start pointer*/
static urcu_node_size_func_t g_node_size_function = NULL;
static urcu_node_start_ptr_func_t g_node_start_ptr_function = NULL;

void urcu_safe_set_node_size_function(urcu_node_size_func_t func) {
    g_node_size_function = func;
    CM_LOG_DEBUG("Node size function set to %p", (void*)func);
}
urcu_node_size_func_t urcu_safe_get_node_size_function(void) {
    return g_node_size_function;
}
void urcu_safe_set_node_start_ptr_function(urcu_node_start_ptr_func_t start_func) {
    g_node_start_ptr_function = start_func;
    CM_LOG_DEBUG("Node start pointer function set to %p", (void*)start_func);
}
urcu_node_start_ptr_func_t urcu_safe_get_node_start_ptr_function(void) {
    return g_node_start_ptr_function;
}

#ifdef URCU_LFHT_SAFETY_ON

/*
static size_t _default_node_size_function(struct cds_lfht_node* node) {
    (void)node;
    CM_LOG_ERROR("No node size function set - cannot determine actual node size");
    return 0;
}
*/

/* Thread-local safety state for tracking RCU usage */
typedef struct {
    bool registered;                 /* Is this thread registered with RCU? */
    int read_lock_count;             /* Nested read lock depth */
    pthread_t thread_id;             /* POSIX thread ID for verification */
    bool initialized;                /* Has thread state been initialized? */
} rcu_thread_state_t;

/* Thread-local safety state */
static __thread rcu_thread_state_t thread_state = {
    .registered = false,
    .read_lock_count = 0,
    .thread_id = 0,
    .initialized = false
};

/* Test mode flag - allows tests to indicate when errors are expected */
static atomic_bool test_mode = ATOMIC_VAR_INIT(false);

/* Safety checks enabled flag - allows disabling safety checks during initialization */
static atomic_bool safety_checks_enabled = ATOMIC_VAR_INIT(true);

/* RCU initialized flag */
static atomic_bool g_rcu_initialized = ATOMIC_VAR_INIT(false);

/* Initialize RCU */
void _rcu_init_safe(void) {
    if (atomic_load(&g_rcu_initialized)) {
        CM_LOG_WARNING("rcu_init called multiple times");
        return;
    }
    urcu_memb_init();
    atomic_store(&g_rcu_initialized, true);
    CM_LOG_DEBUG("RCU initialized");
}

/* Initialize thread state if not already done */
static void _ensure_thread_state_initialized(void) {
    if (!thread_state.initialized) {
        thread_state.thread_id = pthread_self();
        thread_state.initialized = true;
        thread_state.registered = false;
        thread_state.read_lock_count = 0;
    }
}

/* RCU registration with proper error handling */
void _rcu_register_thread_safe(void) {
    if (!atomic_load(&g_rcu_initialized)) {
        CM_LOG_ERROR("rcu_register_thread called before rcu_init");
        return;
    }

    CM_SCOPE(_ensure_thread_state_initialized());
    
    pthread_t current_thread = pthread_self();
    
    if (thread_state.registered) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("Thread %lu already registered (test mode)", (unsigned long)current_thread);
        } else {
            CM_LOG_ERROR("Thread %lu already registered", (unsigned long)current_thread);
        }
        return;
    }
    
    /* Set thread ID first for better error reporting */
    thread_state.thread_id = current_thread;
    
    /* Attempt URCU registration */
    CM_SCOPE(urcu_memb_register_thread());
    
    /* Only mark as registered after successful URCU registration */
    thread_state.registered = true;
    thread_state.read_lock_count = 0;
    
    CM_LOG_DEBUG("Thread %lu registered with RCU", (unsigned long)thread_state.thread_id);
    return;
}

/* RCU callback scheduling */
void _call_rcu_safe(struct rcu_head *head, void (*func)(struct rcu_head *)) {
    if (!head || !func) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("call_rcu called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("call_rcu called with NULL parameters");
        }
        return;
    }
    
    if (!thread_state.registered) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("call_rcu called from unregistered thread (test mode)");
        } else {
            CM_LOG_ERROR("call_rcu called from unregistered thread");
        }
        return;
    }
    
    urcu_memb_call_rcu(head, func);
    CM_LOG_DEBUG("RCU callback scheduled");
}

void _rcu_unregister_thread_safe(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    
    pthread_t current_thread = pthread_self();
    
    if (!thread_state.registered) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("Thread %lu not registered (test mode)", (unsigned long)current_thread);
        } else {
            CM_LOG_ERROR("Thread %lu not registered", (unsigned long)current_thread);
        }
        return;
    }
    
    int lock_depth = thread_state.read_lock_count;
    if (lock_depth > 0) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("Thread %lu unregistering with %d pending read locks (test mode)", 
                       (unsigned long)thread_state.thread_id, lock_depth);
        } else {
            CM_LOG_ERROR("Thread %lu unregistering with %d pending read locks", 
                          (unsigned long)thread_state.thread_id, lock_depth);
        }
        return;
    }
    
    /* Attempt URCU unregistration */
    urcu_memb_unregister_thread();
    
    thread_state.registered = false;
    
    CM_LOG_DEBUG("Thread %lu unregistered from RCU", (unsigned long)thread_state.thread_id);
    
}

/* RCU read locking with proper atomic operations */
void _rcu_read_lock_safe(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    CM_LOG_DEBUG("_rcu_read_lock_safe() ...\n");
    
    pthread_t current_thread = pthread_self();
    
    /* Skip safety checks if disabled (e.g., during initialization) */
    CM_SCOPE(bool result = _rcu_are_safety_checks_enabled());
    if (!result) {
        CM_SCOPE(urcu_memb_read_lock());
        return;
    }
    
    if (!thread_state.registered) {
        // have to check if this thread is the callback thread, because if it is that means that this urcu safe code has not 
        // been able to register the thread because the rcu_register_thread is not called explicitly for the callback thread
        // but is rather called implicitly inside the first call_rcu functions call
        // getting id of callback thread
        struct call_rcu_data *default_crdp = get_default_call_rcu_data();
        pthread_t callback_thread_id = get_call_rcu_thread(default_crdp);
        if (callback_thread_id == thread_state.thread_id) {
            thread_state.registered = true;
        } 
        // this branch is when current thread id is not the callback thread
        else {
            CM_SCOPE(result = _rcu_is_test_mode());
            if (result) {
                CM_LOG_DEBUG("rcu_read_lock called from unregistered thread %lu (test mode)", (unsigned long)current_thread);
            } else {
                CM_LOG_ERROR("rcu_read_lock called from unregistered thread %lu", (unsigned long)current_thread);
            }
            return;
        }
    }
    
    /* Increment lock count first */
    thread_state.read_lock_count++;
    
    /* Call URCU function */
    urcu_memb_read_lock();
    
    CM_LOG_DEBUG("Read lock acquired (depth: %d)", thread_state.read_lock_count);
}

void _rcu_read_unlock_safe(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    
    pthread_t current_thread = pthread_self();
    
    /* Skip safety checks if disabled (e.g., during initialization) */
    CM_SCOPE(bool result = _rcu_are_safety_checks_enabled());
    if (!result) {
        CM_SCOPE(urcu_memb_read_unlock());
        return;
    }
    
    if (!thread_state.registered) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("rcu_read_unlock called from unregistered thread %lu (test mode)", 
                       (unsigned long)current_thread);
        } else {
            CM_LOG_ERROR("rcu_read_unlock called from unregistered thread %lu", 
                          (unsigned long)current_thread);
        }
        return;
    }
    
    int current_depth = thread_state.read_lock_count;
    if (current_depth <= 0) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("rcu_read_unlock called without matching read_lock (test mode)");
        } else {
            CM_LOG_ERROR("rcu_read_unlock called without matching read_lock");
        }
        return;
    }
    
    /* Call URCU function first */
    urcu_memb_read_unlock();
    
    /* Then decrement our counter */
    thread_state.read_lock_count--;
    
    CM_LOG_DEBUG("Read lock released (depth: %d)", thread_state.read_lock_count);
}

void _synchronize_rcu_safe(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    
    /* Check if thread is registered */
    if (!thread_state.registered) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("synchronize_rcu called from unregistered thread (test mode)");
        } else {
            CM_LOG_ERROR("synchronize_rcu called from unregistered thread");
        }
        return;
    }
    
    CM_SCOPE(bool result = _rcu_is_in_read_section());
    if (result) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("synchronize_rcu called from within read-side critical section (test mode)");
        } else {
            CM_LOG_ERROR("synchronize_rcu called from within read-side critical section");
        }
        return;
    }
    
    urcu_memb_synchronize_rcu();
    CM_LOG_DEBUG("RCU synchronization completed");
}

void _rcu_barrier_safe(void) {
    CM_SCOPE(_ensure_thread_state_initialized());

    // if this thread is the main callback thread then its not allowed.
    // this does not support multiple callback threads yet
    struct call_rcu_data *default_crdp = get_default_call_rcu_data();
    pthread_t callback_thread_id = get_call_rcu_thread(default_crdp);
    if (callback_thread_id == thread_state.thread_id) {
        thread_state.registered = true;
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("rcu_barrier called from within callback which is not allowed");
        } else {
            CM_LOG_ERROR("rcu_barrier called from within callback which is not allowed");
        }
    } 
    
    /* Check if thread is registered */
    if (!thread_state.registered) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("rcu_barrier called from unregistered thread (test mode)");
        } else {
            CM_LOG_ERROR("rcu_barrier called from unregistered thread");
        }
        return;
    }
    
    CM_SCOPE(bool result = _rcu_is_in_read_section());
    if (result) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("rcu_barrier called from within read-side critical section (test mode)");
        } else {
            CM_LOG_ERROR("rcu_barrier called from within read-side critical section");
        }
        return;
    }
    
    urcu_memb_barrier();
    CM_LOG_DEBUG("RCU barrier completed");
}

/* State query functions */
bool _rcu_is_registered(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    return thread_state.registered;
}

bool _rcu_is_in_read_section(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    return thread_state.read_lock_count > 0;
}

int _rcu_get_lock_depth(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    return thread_state.read_lock_count;
}

pthread_t _rcu_get_thread_id(void) {
    CM_SCOPE(_ensure_thread_state_initialized());
    return thread_state.thread_id;
}

/* Test mode support */
void _rcu_set_test_mode(bool test_mode_flag) {
    atomic_store(&test_mode, test_mode_flag);
    CM_LOG_DEBUG("RCU test mode %s", test_mode_flag ? "enabled" : "disabled");
}

bool _rcu_is_test_mode(void) {
    return atomic_load(&test_mode);
}

/* Safety check control functions */
void _rcu_disable_safety_checks(void) {
    atomic_store(&safety_checks_enabled, false);
    CM_LOG_DEBUG("RCU safety checks disabled");
}

void _rcu_enable_safety_checks(void) {
    atomic_store(&safety_checks_enabled, true);
    CM_LOG_DEBUG("RCU safety checks enabled");
}

bool _rcu_are_safety_checks_enabled(void) {
    return atomic_load(&safety_checks_enabled);
}

/* Hash table node initialization */
void _cds_lfht_node_init_safe(struct cds_lfht_node *node) {
    if (!node) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_node_init called with NULL node (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_node_init called with NULL node");
        }
        return;
    }
    cds_lfht_node_init(node);
    CM_LOG_DEBUG("Hash table node initialized");
}

/* Hash table operations with exact API compatibility */
void _cds_lfht_lookup_safe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key), 
    const void *key, struct cds_lfht_iter *iter) {
    
    CM_SCOPE(bool result = _rcu_is_in_read_section());
    if (!result) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_lookup called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_lookup called without read lock");
        }
        if (iter) {
            iter->node = NULL;
            iter->next = NULL;
        }
        return;
    }
    
    if (!ht || !match || !key || !iter) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_lookup called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_lookup called with NULL parameters");
        }
        if (iter) {
            iter->node = NULL;
            iter->next = NULL;
        }
        return;
    }

    /* Call the original URCU function */
    CM_SCOPE(cds_lfht_lookup(ht, hash, match, key, iter));
    
    CM_LOG_DEBUG("Lookup performed (found: %s)", iter->node ? "yes" : "no");
}

void _cds_lfht_add_safe(struct cds_lfht *ht, unsigned long hash,
    struct cds_lfht_node *node) {
    
    CM_SCOPE(bool result = _rcu_is_in_read_section());
    if (!result) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_add called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_add called without read lock");
        }
        return;
    }
    
    if (!ht || !node) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_add called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_add called with NULL parameters");
        }
        return;
    }
    
    /* Call the original URCU function */
    cds_lfht_add(ht, hash, node);
    CM_LOG_DEBUG("Node added");
}

struct cds_lfht_node *_cds_lfht_add_unique_safe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_node *node) {
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_add_unique called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_add_unique called without read lock");
        }
        return NULL;
    }
    
    if (!ht || !match || !key || !node) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_add_unique called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_add_unique called with NULL parameters");
        }
        return NULL;
    }
    
    CM_SCOPE(struct cds_lfht_node *add_result = cds_lfht_add_unique(ht, hash, match, key, node));
    if (add_result == node) {
        CM_LOG_DEBUG("Add unique succeeded - new node inserted");
    } else {
        CM_LOG_DEBUG("Add unique failed - existing node found");
    }
    
    return add_result;
}

struct cds_lfht_node *_cds_lfht_add_replace_safe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_node *node) {
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_add_replace called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_add_replace called without read lock");
        }
        return NULL;
    }
    
    if (!ht || !match || !key || !node) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_add_replace called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_add_replace called with NULL parameters");
        }
        return NULL;
    }
    
    CM_SCOPE(struct cds_lfht_node *replace_result = cds_lfht_add_replace(ht, hash, match, key, node));
    if (replace_result) {
        CM_LOG_DEBUG("Add replace succeeded - existing node replaced");
    } else {
        CM_LOG_DEBUG("Add replace succeeded - new node inserted");
    }
    
    return replace_result;
}

int _cds_lfht_del_safe(struct cds_lfht *ht, struct cds_lfht_node *node) {
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_del called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_del called without read lock");
        }
        return -EINVAL;
    }
    
    if (!ht || !node) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_del called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_del called with NULL parameters");
        }
        return -EINVAL;
    }
    
    CM_SCOPE(int del_result = cds_lfht_del(ht, node));
    if (del_result == 0) {
        CM_LOG_DEBUG("Node deleted successfully\n");
    } else if (del_result == -ENOENT) {
        CM_LOG_DEBUG("NOde already deleted\n");
    } else {
        CM_LOG_ERROR("Delete failed: %d", del_result);
    }
    
    return del_result;
}

int _cds_lfht_replace_safe(struct cds_lfht *ht, struct cds_lfht_iter *old_iter,
    unsigned long hash, int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_node *new_node) {
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_replace called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_replace called without read lock");
        }
        return -EINVAL;
    }
    
    if (!ht || !old_iter || !match || !key || !new_node) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_replace called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_replace called with NULL parameters");
        }
        return -EINVAL;
    }
    
    CM_SCOPE(int replace_result = cds_lfht_replace(ht, old_iter, hash, match, key, new_node));
    if (replace_result == 0) {
        CM_LOG_DEBUG("Node replaced successfully");
    } else if (replace_result == -ENOENT) {
        CM_LOG_DEBUG("Node does not exist so it cannot be replaced\n");
    } else {
        CM_LOG_ERROR("Replace failed: %d", replace_result);
    }
    
    return replace_result;
}

struct cds_lfht *_cds_lfht_new_safe(unsigned long init_size,
    unsigned long min_nr_alloc_buckets, unsigned long max_nr_buckets,
    int flags, const struct rcu_flavor_struct *flavor) {
    
    if (init_size == 0) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_new called with zero init_size (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_new called with zero init_size");
        }
        return NULL;
    }

    struct cds_lfht *result = cds_lfht_new_flavor(
        init_size, min_nr_alloc_buckets, max_nr_buckets,
        flags, flavor, /*attr=*/NULL
    );
    
    if (result) {
        CM_LOG_DEBUG("Hash table created successfully");
    } else {
        CM_LOG_ERROR("Hash table creation failed");
    }
    
    return result;
}

int _cds_lfht_destroy_safe(struct cds_lfht *ht, pthread_attr_t **attr) {
    
    if (!ht) {
        CM_LOG_ERROR("cds_lfht_destroy called with NULL hash table");
        return -EINVAL;
    }

    if (!thread_state.registered) {
        CM_LOG_ERROR("cds_lfht_destroy must be called from a registered thread\n");
        return -EINVAL;
    }
    
    CM_SCOPE(int destroy_result = cds_lfht_destroy(ht, attr));
    if (destroy_result == 0) {
        CM_LOG_DEBUG("Hash table destroyed successfully");
    } else {
        CM_LOG_ERROR("Hash table destruction failed: %d", destroy_result);
    }
    
    return destroy_result;
}

void _cds_lfht_resize_safe(struct cds_lfht *ht, unsigned long new_size) {
    
    if (!ht) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_resize called with NULL hash table (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_resize called with NULL hash table");
        }
        return;
    }
    
    if (new_size == 0) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_resize called with zero size (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_resize called with zero size");
        }
        return;
    }
    
    CM_SCOPE(bool result = _rcu_is_in_read_section());
    if (result) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_resize called from within read-side critical section (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_resize called from within read-side critical section");
        }
        return;
    }
    
    cds_lfht_resize(ht, new_size);
    CM_LOG_DEBUG("Hash table resized to %lu", new_size);
}

void _cds_lfht_first_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter) {
    
    if (!ht || !iter) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_first called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_first called with NULL parameters");
        }
        if (iter) {
            iter->node = NULL;
            iter->next = NULL;
        }
        return;
    }
    
    CM_SCOPE(bool result = _rcu_is_in_read_section());
    if (!result) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_first called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_first called without read lock");
        }
        iter->node = NULL;
        iter->next = NULL;
        return;
    }
    
    cds_lfht_first(ht, iter);
    
    CM_LOG_DEBUG("Iterator positioned at first");
}

void _cds_lfht_next_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter) {
    
    if (!ht || !iter) {
        CM_SCOPE(bool result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_next called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_next called with NULL parameters");
        }
        if (iter) {
            iter->node = NULL;
            iter->next = NULL;
        }
        return;
    }
    
    CM_SCOPE(bool result = _rcu_is_in_read_section());
    if (!result) {
        CM_SCOPE(result = _rcu_is_test_mode());
        if (result) {
            CM_LOG_DEBUG("cds_lfht_next called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_next called without read lock");
        }
        iter->node = NULL;
        iter->next = NULL;
        return;
    }
    
    cds_lfht_next(ht, iter);
    
    CM_LOG_DEBUG("Iterator advanced");
}

void _cds_lfht_next_duplicate_safe(struct cds_lfht *ht,
    int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_iter *iter) {
    
    if (!ht || !match || !key || !iter) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_next_duplicate called with NULL parameters (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_next_duplicate called with NULL parameters");
        }
        if (iter) {
            CM_SCOPE(iter->node = NULL);
            CM_SCOPE(iter->next = NULL);
        }
        return;
    }
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_next_duplicate called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_next_duplicate called without read lock");
        }
        CM_SCOPE(iter->node = NULL);
        CM_SCOPE(iter->next = NULL);
        return;
    }
    
    CM_SCOPE(cds_lfht_next_duplicate(ht, match, key, iter));
    
    CM_LOG_DEBUG("Iterator advanced to next duplicate");
}

struct cds_lfht_node *_cds_lfht_iter_get_node_safe(struct cds_lfht_iter *iter) {
    
    if (!iter) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_iter_get_node called with NULL iterator (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_iter_get_node called with NULL iterator");
        }
        return NULL;
    }
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_iter_get_node called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_iter_get_node called without read lock");
        }
        return NULL;
    }
    
    CM_SCOPE(struct cds_lfht_node *result = cds_lfht_iter_get_node(iter));
    
    CM_LOG_DEBUG("Got node from iterator: %p", result);
    
    return result;
}

void _cds_lfht_count_nodes_safe(struct cds_lfht *ht,
    long *approx_before, unsigned long *count, long *approx_after) {
    
    if (!ht) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_count_nodes called with NULL hash table (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_count_nodes called with NULL hash table");
        }
        if (count) *count = 0;
        if (approx_before) *approx_before = 0;
        if (approx_after) *approx_after = 0;
        return;
    }
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("cds_lfht_count_nodes called without read lock (test mode)");
        } else {
            CM_LOG_ERROR("cds_lfht_count_nodes called without read lock");
        }
        if (count) *count = 0;
        if (approx_before) *approx_before = 0;
        if (approx_after) *approx_after = 0;
        return;
    }

    CM_SCOPE(cds_lfht_count_nodes(ht, approx_before, count, approx_after));
    CM_LOG_DEBUG("Node count: %lu", count ? *count : 0);
}

/* RCU pointer safety wrapper functions */
void* _rcu_dereference_safe(void* ptr, const char* file, int line) {
    CM_SCOPE(_ensure_thread_state_initialized());
    
    /* Skip safety checks if disabled */
    CM_SCOPE(bool safety_enabled = _rcu_are_safety_checks_enabled());
    if (!safety_enabled) {
        CM_SCOPE(void* deref_result = rcu_dereference_sym(ptr));
        return deref_result;
    }
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (!in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("rcu_dereference called outside read lock (test mode) at %s:%d", file, line);
        } else {
            CM_LOG_ERROR("rcu_dereference called outside read lock at %s:%d", file, line);
        }
        return ptr; /* Return original pointer even in error case */
    }
    
    CM_SCOPE(void* deref_result = rcu_dereference_sym(ptr));
    return deref_result;
}

void _rcu_assign_pointer_safe(void** ptr_ptr, void* val, const char* file, int line) {
    CM_SCOPE(_ensure_thread_state_initialized());
    
    /* Skip safety checks if disabled */
    CM_SCOPE(bool safety_enabled = _rcu_are_safety_checks_enabled());
    if (!safety_enabled) {
        CM_SCOPE(rcu_set_pointer_sym(*(void**)ptr_ptr, val));
        return;
    }
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("rcu_assign_pointer called inside read lock (test mode) at %s:%d", file, line);
        } else {
            CM_LOG_ERROR("rcu_assign_pointer called inside read lock at %s:%d", file, line);
        }
        return;
    }
    
    CM_SCOPE(rcu_set_pointer_sym(ptr_ptr, val));
}

void* _rcu_xchg_pointer_safe(void** ptr_ptr, void* val, const char* file, int line) {
    CM_SCOPE(_ensure_thread_state_initialized());
    
    /* Skip safety checks if disabled */
    CM_SCOPE(bool safety_enabled = _rcu_are_safety_checks_enabled());
    if (!safety_enabled) {
        CM_SCOPE(void* xchg_result = rcu_xchg_pointer_sym(ptr_ptr, val));
        return xchg_result;
    }
    
    CM_SCOPE(bool in_read_section = _rcu_is_in_read_section());
    if (in_read_section) {
        CM_SCOPE(bool test_mode_flag = _rcu_is_test_mode());
        if (test_mode_flag) {
            CM_LOG_DEBUG("rcu_xchg_pointer called inside read lock (test mode) at %s:%d", file, line);
        } else {
            CM_LOG_ERROR("rcu_xchg_pointer called inside read lock at %s:%d", file, line);
        }
        return NULL; /* Return original value even in error case */
    }
    
    CM_SCOPE(void* xchg_result = rcu_xchg_pointer_sym(ptr_ptr, val));
    return xchg_result;
}

void* _rcu_cmpxchg_pointer_safe(void** ptr_ptr, void* old, void* new, const char* file, int line) {
    (void)file;
    (void)line;
    CM_SCOPE(_ensure_thread_state_initialized());
    CM_SCOPE(void* xchg_result = rcu_cmpxchg_pointer_sym(ptr_ptr, old, new));
    return xchg_result;
}

#endif /* URCU_LFHT_SAFETY_ON */

/* Unsafe versions that bypass safety checks for cleanup operations */
/* These are always available, regardless of URCU_LFHT_SAFETY_ON setting */
void cds_lfht_first_unsafe(struct cds_lfht *ht, struct cds_lfht_iter *iter) {
    if (!ht || !iter) {
        if (iter) {
            CM_SCOPE(iter->node = NULL);
            CM_SCOPE(iter->next = NULL);
        }
        return;
    }
    
    // Call original URCU function directly, bypassing all safety checks
    CM_SCOPE(cds_lfht_first(ht, iter));

    CM_LOG_DEBUG("Iterator positioned at first (unsafe mode)");
}

void cds_lfht_next_unsafe(struct cds_lfht *ht, struct cds_lfht_iter *iter) {
    if (!ht || !iter) {
        if (iter) {
            CM_SCOPE(iter->node = NULL);
            CM_SCOPE(iter->next = NULL);
        }
        return;
    }
    
    // Call original URCU function directly, bypassing all safety checks
    CM_SCOPE(cds_lfht_next(ht, iter));

    CM_LOG_DEBUG("Iterator advanced (unsafe mode)");

}

struct cds_lfht_node *cds_lfht_iter_get_node_unsafe(struct cds_lfht_iter *iter) {
    if (!iter) {
        return NULL;
    }
    
    // Call original URCU function directly, bypassing all safety checks
    CM_SCOPE(struct cds_lfht_node *result = cds_lfht_iter_get_node(iter));

    CM_LOG_DEBUG("Got node from iterator: %p (unsafe mode)", result);
    
    return result;
}

int cds_lfht_del_unsafe(struct cds_lfht *ht, struct cds_lfht_node *node) {
    if (!ht || !node) {
        return -EINVAL;
    }
    
    // Call original URCU function directly, bypassing all safety checks
    CM_SCOPE(int result = cds_lfht_del(ht, node));
    if (result == 0) {
        CM_LOG_DEBUG("Node deleted successfully (unsafe mode)");
    } else {
        CM_LOG_ERROR("Node deletion failed: %d (unsafe mode)", result);
    }
    
    return result;
}



void cds_lfht_lookup_unsafe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key), 
    const void *key, struct cds_lfht_iter *iter) {
    
    if (!ht || !match || !key || !iter) {
        if (iter) {
            CM_SCOPE(iter->node = NULL);
            CM_SCOPE(iter->next = NULL);
        }
        return;
    }
    
    // Call original URCU function directly, bypassing all safety checks
    CM_SCOPE(cds_lfht_lookup(ht, hash, match, key, iter));
    
    CM_LOG_DEBUG("Lookup performed (unsafe mode, found: %s)", iter->node ? "yes" : "no");
}
