#define LFHT_SAFE_INTERNAL  /* Prevent macro redefinition in this file */
#include "urcu_safe.h"
#include <errno.h>

#ifdef URCU_LFHT_SAFETY_ON

/* Thread-local safety state for tracking RCU usage */
typedef struct {
    bool registered;          /* Is this thread registered with RCU? */
    int read_lock_count;      /* Nested read lock depth */
    SDL_ThreadID thread_id;   /* SDL3 thread ID for verification */
} rcu_thread_state_t;


/* Thread-local safety state */
static __thread rcu_thread_state_t thread_state = {
    .registered = false,
    .read_lock_count = 0,
    .thread_id = 0
};

/* RCU registration */
bool _rcu_register_thread_safe(void) {
    if (thread_state.registered) {
        tklog_warning("Thread %lu already registered", (unsigned long)SDL_GetCurrentThreadID());
        return true;
    }
    
    thread_state.thread_id = SDL_GetCurrentThreadID();
    thread_state.registered = true;
    thread_state.read_lock_count = 0;
    
    /* Use direct URCU function name after #undef */
    urcu_memb_register_thread();
    
    tklog_debug("Thread %lu registered with RCU", (unsigned long)thread_state.thread_id);
    return true;
}

bool _rcu_unregister_thread_safe(void) {
    if (!thread_state.registered) {
        tklog_error("Thread %lu not registered", (unsigned long)SDL_GetCurrentThreadID());
        return false;
    }
    
    if (thread_state.read_lock_count > 0) {
        tklog_critical("Thread %lu unregistering with %d pending read locks", 
                      (unsigned long)thread_state.thread_id, thread_state.read_lock_count);
        return false;
    }
    
    /* Use direct URCU function name after #undef */
    urcu_memb_unregister_thread();
    
    thread_state.registered = false;
    tklog_debug("Thread %lu unregistered from RCU", (unsigned long)thread_state.thread_id);
    
    return true;
}

/* RCU read locking */
bool _rcu_read_lock_safe(void) {
    if (!thread_state.registered) {
        tklog_critical("rcu_read_lock called from unregistered thread %lu", 
                      (unsigned long)SDL_GetCurrentThreadID());
        return false;
    }
    
    thread_state.read_lock_count++;
    /* Use direct URCU function name after #undef */
    urcu_memb_read_lock();
    
    tklog_debug("Read lock acquired (depth: %d)", thread_state.read_lock_count);
    return true;
}

bool _rcu_read_unlock_safe(void) {
    if (!thread_state.registered) {
        tklog_critical("rcu_read_unlock called from unregistered thread %lu", 
                      (unsigned long)SDL_GetCurrentThreadID());
        return false;
    }
    
    if (thread_state.read_lock_count <= 0) {
        tklog_critical("rcu_read_unlock called without matching read_lock");
        return false;
    }
    
    thread_state.read_lock_count--;
    /* Use direct URCU function name after #undef */
    urcu_memb_read_unlock();
    
    tklog_debug("Read lock released (depth: %d)", thread_state.read_lock_count);
    return true;
}

void _synchronize_rcu_safe(void) {
    if (_rcu_is_in_read_section()) {
        tklog_critical("synchronize_rcu called from within read-side critical section");
        return;
    }
    
    /* Use direct URCU function name after #undef */
    urcu_memb_synchronize_rcu();
    tklog_debug("RCU synchronization completed");
}

/* State query functions */
bool _rcu_is_registered(void) {
    return thread_state.registered;
}

bool _rcu_is_in_read_section(void) {
    return thread_state.read_lock_count > 0;
}

int _rcu_get_lock_depth(void) {
    return thread_state.read_lock_count;
}

/* Hash table operations */
struct cds_lfht_node *_cds_lfht_lookup_safe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key), const void *key) {
    
    if (!_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_lookup called without read lock");
        return NULL;
    }
    
    if (!ht || !match || !key) {
        tklog_error("cds_lfht_lookup called with NULL parameters");
        return NULL;
    }

    /* new API: cds_lfht_lookup takes an iterator output, and returns void */
    struct cds_lfht_iter iter;
    cds_lfht_lookup(ht, hash, match, key, &iter);
    /* extract the node pointer from the iterator */
    struct cds_lfht_node *result = cds_lfht_iter_get_node(&iter);
    tklog_debug("Lookup performed (found: %s)", result ? "yes" : "no");
    
    return result;
}

struct cds_lfht_node *_cds_lfht_add_safe(struct cds_lfht *ht, unsigned long hash,
    struct cds_lfht_node *node) {
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_add called from within read-side critical section");
        return NULL;
    }
    
    if (!ht || !node) {
        tklog_error("cds_lfht_add called with NULL parameters");
        return NULL;
    }
    
    /* new API: cds_lfht_add returns void, so just call it and return the node */
    cds_lfht_add(ht, hash, node);
    tklog_debug("Node added");
    return node;
}

struct cds_lfht_node *_cds_lfht_add_unique_safe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_node *node) {
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_add_unique called from within read-side critical section");
        return NULL;
    }
    
    if (!ht || !match || !key || !node) {
        tklog_error("cds_lfht_add_unique called with NULL parameters");
        return NULL;
    }
    
    struct cds_lfht_node *result = cds_lfht_add_unique(ht, hash, match, key, node);
    tklog_debug("Add unique performed");
    
    return result;
}

struct cds_lfht_node *_cds_lfht_add_replace_safe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_node *node) {
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_add_replace called from within read-side critical section");
        return NULL;
    }
    
    if (!ht || !match || !key || !node) {
        tklog_error("cds_lfht_add_replace called with NULL parameters");
        return NULL;
    }
    
    struct cds_lfht_node *result = cds_lfht_add_replace(ht, hash, match, key, node);
    tklog_debug("Add replace performed");
    
    return result;
}

int _cds_lfht_del_safe(struct cds_lfht *ht, struct cds_lfht_node *node) {
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_del called from within read-side critical section");
        return -EINVAL;
    }
    
    if (!ht || !node) {
        tklog_error("cds_lfht_del called with NULL parameters");
        return -EINVAL;
    }
    
    int result = cds_lfht_del(ht, node);
    if (result == 0) {
        tklog_debug("Node deleted");
    } else {
        tklog_error("Delete failed: %d", result);
    }
    
    return result;
}

int _cds_lfht_replace_safe(struct cds_lfht *ht, struct cds_lfht_iter *old_iter,
    unsigned long hash, int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_node *new_node) {
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_replace called from within read-side critical section");
        return -EINVAL;
    }
    
    if (!ht || !old_iter || !match || !key || !new_node) {
        tklog_error("cds_lfht_replace called with NULL parameters");
        return -EINVAL;
    }
    
    int result = cds_lfht_replace(ht, old_iter, hash, match, key, new_node);
    if (result == 0) {
        tklog_debug("Node replaced");
    } else {
        tklog_error("Replace failed: %d", result);
    }
    
    return result;
}

struct cds_lfht *_cds_lfht_new_safe(unsigned long init_size,
    unsigned long min_nr_alloc_buckets, unsigned long max_nr_buckets,
    int flags, const struct rcu_flavor_struct *flavor) {
    
    if (init_size == 0) {
        tklog_error("cds_lfht_new called with zero init_size");
        return NULL;
    }

    /*
     * new API: use cds_lfht_new_flavor() to pass a const struct rcu_flavor *;
     * the last argument is the optional pthread_attr_t * for the resize
     * worker thread (NULL = default).
     */
    struct cds_lfht *result = cds_lfht_new_flavor(
        init_size, min_nr_alloc_buckets, max_nr_buckets,
        flags, flavor, /*attr=*/NULL
    );
    
    if (result) {
        tklog_debug("Hash table created");
    } else {
        tklog_error("Hash table creation failed");
    }
    
    return result;
}

int _cds_lfht_destroy_safe(struct cds_lfht *ht, pthread_attr_t **attr) {
    
    if (!ht) {
        tklog_error("cds_lfht_destroy called with NULL hash table");
        return -EINVAL;
    }
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_destroy called from within read-side critical section");
        return -EINVAL;
    }
    
    int result = cds_lfht_destroy(ht, attr);
    if (result == 0) {
        tklog_debug("Hash table destroyed");
    } else {
        tklog_error("Hash table destruction failed: %d", result);
    }
    
    return result;
}

void _cds_lfht_resize_safe(struct cds_lfht *ht, unsigned long new_size) {
    
    if (!ht) {
        tklog_error("cds_lfht_resize called with NULL hash table");
        return;
    }
    
    if (new_size == 0) {
        tklog_error("cds_lfht_resize called with zero size");
        return;
    }
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_resize called from within read-side critical section");
        return;
    }
    
    cds_lfht_resize(ht, new_size);
    tklog_debug("Hash table resized to %lu", new_size);
}

void _cds_lfht_first_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter) {
    
    if (!ht || !iter) {
        tklog_error("cds_lfht_first called with NULL parameters");
        return;
    }
    
    if (!_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_first called without read lock");
        return;
    }
    
    cds_lfht_first(ht, iter);
    tklog_debug("Iterator positioned at first");
}

void _cds_lfht_next_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter) {
    
    if (!ht || !iter) {
        tklog_error("cds_lfht_next called with NULL parameters");
        return;
    }
    
    if (!_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_next called without read lock");
        return;
    }
    
    cds_lfht_next(ht, iter);
    tklog_debug("Iterator advanced");
}

void _cds_lfht_next_duplicate_safe(struct cds_lfht *ht,
    int (*match)(struct cds_lfht_node *node, const void *key),
    const void *key, struct cds_lfht_iter *iter) {
    
    if (!ht || !match || !key || !iter) {
        tklog_error("cds_lfht_next_duplicate called with NULL parameters");
        return;
    }
    
    if (!_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_next_duplicate called without read lock");
        return;
    }
    
    cds_lfht_next_duplicate(ht, match, key, iter);
    tklog_debug("Iterator advanced to next duplicate");
}

struct cds_lfht_node *_cds_lfht_iter_get_node_safe(struct cds_lfht_iter *iter) {
    
    if (!iter) {
        tklog_error("cds_lfht_iter_get_node called with NULL iterator");
        return NULL;
    }
    
    if (!_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_iter_get_node called without read lock");
        return NULL;
    }
    
    struct cds_lfht_node *result = cds_lfht_iter_get_node(iter);
    tklog_debug("Got node from iterator: %p", result);
    
    return result;
}

long _cds_lfht_count_nodes_safe(struct cds_lfht *ht,
    long *approx_before, unsigned long *count, long *approx_after)
{
    
    if (!ht) {
        tklog_error("cds_lfht_count_nodes called with NULL hash table");
        return -1;
    }
    
    if (_rcu_is_in_read_section()) {
        tklog_critical("cds_lfht_count_nodes called from within read-side critical section");
        return -1;
    }

    /* new API: cds_lfht_count_nodes returns void and writes to *count */
    cds_lfht_count_nodes(ht, approx_before, count, approx_after);
    long result = (long)*count;
    tklog_debug("Node count: %ld", result);
    return result;
}

#endif // URCU_LFHT_SAFETY_ON