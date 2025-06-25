#ifndef LFHT_SAFE_H
#define LFHT_SAFE_H

#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>
#include <stdatomic.h>
#include <string.h>
#include <urcu.h>
#include <urcu/rculfhash.h>
#include "tklog.h"

#ifdef URCU_LFHT_SAFETY_ON

/* -------------------------------------------------------------------------
 * Version Detection and API Compatibility
 * ------------------------------------------------------------------------- */
#ifndef URCU_VERSION_MAJOR
#define URCU_VERSION_MAJOR 0
#endif

#ifndef URCU_VERSION_MINOR  
#define URCU_VERSION_MINOR 13
#endif

/* -------------------------------------------------------------------------
 * URCU LFHT Safety Wrappers
 * 
 * This header provides safety wrappers for URCU lock-free hash table operations.
 * It includes comprehensive validation, logging, and error detection.
 * 
 * USAGE:
 * 1. Define a function to get node size: size_t my_get_node_size(struct cds_lfht_node* node)
 * 2. Register it with: _urcu_safe_set_node_size_function(my_get_node_size)
 * 3. Include this header: #include "urcu_safe.h"
 * ------------------------------------------------------------------------- */

/* Function pointer type for getting node size */
typedef size_t (*urcu_node_size_func_t)(struct cds_lfht_node* node);

/* Function to set the node size function - call this before using any safety features */
void _urcu_safe_set_node_size_function(urcu_node_size_func_t func);

/* Function to get the current node size function */
urcu_node_size_func_t _urcu_safe_get_node_size_function(void);

/* -------------------------------------------------------------------------
 * RCU Safety Wrappers
 * These functions wrap the standard RCU functions with safety checks and
 * logging. They verify proper usage patterns and log errors when detected.
 * ------------------------------------------------------------------------- */
void _rcu_register_thread_safe(void);
void _rcu_unregister_thread_safe(void);
void _rcu_read_lock_safe(void);
void _rcu_read_unlock_safe(void);
void _synchronize_rcu_safe(void);

/* RCU pointer safety wrapper functions */
void* _rcu_dereference_safe(void* ptr, const char* file, int line);
void _rcu_assign_pointer_safe(void* ptr_addr, void* val, const char* file, int line);
void* _rcu_xchg_pointer_safe(void* ptr_addr, void* val, const char* file, int line);

/* -------------------------------------------------------------------------
 * Hash Table Safety Wrappers
 * These functions wrap the cds_lfht functions with safety checks.
 * They verify proper RCU locking and parameter validity.
 * 
 * Note: These wrappers maintain the exact same API signatures as the
 * underlying URCU functions to ensure compatibility.
 * ------------------------------------------------------------------------- */
void _cds_lfht_lookup_safe(struct cds_lfht *ht, unsigned long hash, 
                          int (*match)(struct cds_lfht_node *node, const void *key), 
                          const void *key, struct cds_lfht_iter *iter);
void _cds_lfht_add_safe(struct cds_lfht *ht, unsigned long hash, struct cds_lfht_node *node);
struct cds_lfht_node *_cds_lfht_add_unique_safe(struct cds_lfht *ht, unsigned long hash, 
                                               int (*match)(struct cds_lfht_node *node, const void *key), 
                                               const void *key, struct cds_lfht_node *node);
struct cds_lfht_node *_cds_lfht_add_replace_safe(struct cds_lfht *ht, unsigned long hash, 
                                                int (*match)(struct cds_lfht_node *node, const void *key), 
                                                const void *key, struct cds_lfht_node *node);
int _cds_lfht_del_safe(struct cds_lfht *ht, struct cds_lfht_node *node);
int _cds_lfht_replace_safe(struct cds_lfht *ht, struct cds_lfht_iter *old_iter, 
                          unsigned long hash, int (*match)(struct cds_lfht_node *node, const void *key), 
                          const void *key, struct cds_lfht_node *new_node);
struct cds_lfht *_cds_lfht_new_safe(unsigned long init_size, unsigned long min_nr_alloc_buckets, 
                                   unsigned long max_nr_buckets, int flags, 
                                   const struct rcu_flavor_struct *flavor);
int _cds_lfht_destroy_safe(struct cds_lfht *ht, pthread_attr_t **attr);
void _cds_lfht_resize_safe(struct cds_lfht *ht, unsigned long new_size);
void _cds_lfht_first_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
void _cds_lfht_next_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
void _cds_lfht_next_duplicate_safe(struct cds_lfht *ht, 
                                  int (*match)(struct cds_lfht_node *node, const void *key), 
                                  const void *key, struct cds_lfht_iter *iter);
struct cds_lfht_node *_cds_lfht_iter_get_node_safe(struct cds_lfht_iter *iter);
void _cds_lfht_count_nodes_safe(struct cds_lfht *ht, long *approx_before, 
                               unsigned long *count, long *approx_after);

/* -------------------------------------------------------------------------
 * Helper Functions
 * These provide state information for debugging and verification.
 * ------------------------------------------------------------------------- */
bool _rcu_is_registered(void);
bool _rcu_is_in_read_section(void);
int _rcu_get_lock_depth(void);
pthread_t _rcu_get_thread_id(void);

/* Test mode support - allows tests to indicate when errors are expected */
void _rcu_set_test_mode(bool test_mode);
bool _rcu_is_test_mode(void);

/* Initialization safety control - allows disabling safety checks during critical initialization */
void _rcu_disable_safety_checks(void);
void _rcu_enable_safety_checks(void);
bool _rcu_are_safety_checks_enabled(void);

/* Pointer tracking debugging functions */
size_t _rcu_get_tracked_pointer_count(void);
void _rcu_get_tracked_pointers(struct cds_lfht_node** pointers, size_t max_count);
bool _rcu_is_pointer_tracked(struct cds_lfht_node* ptr);

/* Function to track LFHT nodes when accessed */
void _rcu_track_lfht_node(struct cds_lfht_node* node);

/* -------------------------------------------------------------------------
 * RCU Pointer Function Validation
 * These functions validate RCU pointer operations with comprehensive checks
 * ------------------------------------------------------------------------- */

/* Check if a pointer is likely an RCU-protected field in our data structures */
bool _is_valid_rcu_protected_field(void* field_ptr, const char* operation);

/* Validate RCU dereference context */
bool _validate_rcu_dereference_context(void* ptr, const char* file, int line);

/* Validate RCU assign pointer context */
bool _validate_rcu_assign_context(void* ptr_addr, const char* file, int line);

/* Enhanced validation for specific field types */
bool _validate_gd_node_field(void* field_ptr, void* node_ptr, const char* field_name);

/* -------------------------------------------------------------------------
 * Macro Overrides
 * These macros replace the standard RCU/LFHT functions with safety wrappers.
 * They use tklog_scope for proper logging context.
 * ------------------------------------------------------------------------- */

#ifndef LFHT_SAFE_INTERNAL

/* Helper macros for different return types */
#define _URCU_SAFE_VOID_CALL(func, ...) do { \
    tklog_scope(func(__VA_ARGS__)); \
} while(0)

#define _URCU_SAFE_BOOL_CALL(func, ...) ({ \
    bool _result; \
    tklog_scope(_result = func(__VA_ARGS__)); \
    _result; \
})

#define _URCU_SAFE_INT_CALL(func, ...) ({ \
    int _result; \
    tklog_scope(_result = func(__VA_ARGS__)); \
    _result; \
})

#define _URCU_SAFE_PTR_CALL(func, ...) ({ \
    void *_result; \
    tklog_scope(_result = func(__VA_ARGS__)); \
    _result; \
})

/* Only override URCU macros if safety is explicitly enabled */
#ifdef URCU_LFHT_SAFETY_ON

/* Undefine original URCU macros to prevent redefinition errors */
#ifdef rcu_read_lock
#undef rcu_read_lock
#endif

#ifdef rcu_read_unlock
#undef rcu_read_unlock
#endif

#ifdef rcu_register_thread
#undef rcu_register_thread
#endif

#ifdef rcu_unregister_thread
#undef rcu_unregister_thread
#endif

#ifdef synchronize_rcu
#undef synchronize_rcu
#endif

#ifdef rcu_dereference
#undef rcu_dereference
#endif

#ifdef rcu_assign_pointer
#undef rcu_assign_pointer
#endif

#ifdef rcu_xchg_pointer
#undef rcu_xchg_pointer
#endif

/* Define our safety wrappers that override the original URCU functions */
#define rcu_read_lock() tklog_scope(_rcu_read_lock_safe())
#define rcu_read_unlock() tklog_scope(_rcu_read_unlock_safe())
#define rcu_register_thread() tklog_scope(_rcu_register_thread_safe())
#define rcu_unregister_thread() tklog_scope(_rcu_unregister_thread_safe())
#define synchronize_rcu() tklog_scope(_synchronize_rcu_safe())

/* RCU pointer function overrides with comprehensive validation */
#define rcu_dereference(ptr) ({ \
    tklog_scope(void* _temp = _rcu_dereference_safe((void*)(ptr), __FILE__, __LINE__)); \
    (typeof((ptr) + 0))_temp; \
})
#define rcu_assign_pointer(ptr, val) tklog_scope(_rcu_assign_pointer_safe((void*)&(ptr), (void*)(val), __FILE__, __LINE__))
#define rcu_xchg_pointer(ptr, val) tklog_scope((typeof(val))_rcu_xchg_pointer_safe((void*)&(ptr), (void*)(val), __FILE__, __LINE__))

/* Hash table functions with exact API compatibility */
/* Note: We don't override cds_lfht_new to avoid initialization issues */
#define cds_lfht_lookup(ht, hash, match, key, iter) _URCU_SAFE_VOID_CALL(_cds_lfht_lookup_safe, ht, hash, match, key, iter)
#define cds_lfht_add(ht, hash, node) _URCU_SAFE_VOID_CALL(_cds_lfht_add_safe, ht, hash, node)
#define cds_lfht_add_unique(ht, hash, match, key, node) _URCU_SAFE_PTR_CALL(_cds_lfht_add_unique_safe, ht, hash, match, key, node)
#define cds_lfht_add_replace(ht, hash, match, key, node) _URCU_SAFE_PTR_CALL(_cds_lfht_add_replace_safe, ht, hash, match, key, node)
#define cds_lfht_del(ht, node) _URCU_SAFE_INT_CALL(_cds_lfht_del_safe, ht, node)
#define cds_lfht_replace(ht, old_iter, hash, match, key, new_node) _URCU_SAFE_INT_CALL(_cds_lfht_replace_safe, ht, old_iter, hash, match, key, new_node)
#define cds_lfht_destroy(ht, attr) _URCU_SAFE_INT_CALL(_cds_lfht_destroy_safe, ht, attr)
#define cds_lfht_resize(ht, new_size) _URCU_SAFE_VOID_CALL(_cds_lfht_resize_safe, ht, new_size)
#define cds_lfht_first(ht, iter) _URCU_SAFE_VOID_CALL(_cds_lfht_first_safe, ht, iter)
#define cds_lfht_next(ht, iter) _URCU_SAFE_VOID_CALL(_cds_lfht_next_safe, ht, iter)
#define cds_lfht_next_duplicate(ht, match, key, iter) _URCU_SAFE_VOID_CALL(_cds_lfht_next_duplicate_safe, ht, match, key, iter)
#define cds_lfht_iter_get_node(iter) _URCU_SAFE_PTR_CALL(_cds_lfht_iter_get_node_safe, iter)
#define cds_lfht_count_nodes(ht, approx_before, count, approx_after) _URCU_SAFE_VOID_CALL(_cds_lfht_count_nodes_safe, ht, approx_before, count, approx_after)

#endif /* URCU_LFHT_SAFETY_ON */

#endif /* !LFHT_SAFE_INTERNAL */

/* Unsafe versions that bypass safety checks for cleanup operations */
void _cds_lfht_first_unsafe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
void _cds_lfht_next_unsafe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
struct cds_lfht_node *_cds_lfht_iter_get_node_unsafe(struct cds_lfht_iter *iter);
int _cds_lfht_del_unsafe(struct cds_lfht *ht, struct cds_lfht_node *node);
void cds_lfht_lookup_unsafe(struct cds_lfht *ht, unsigned long hash,
    int (*match)(struct cds_lfht_node *node, const void *key), 
    const void *key, struct cds_lfht_iter *iter);

#endif // URCU_LFHT_SAFETY_ON

/* Unsafe versions that bypass safety checks for cleanup operations */
/* These are always available, regardless of URCU_LFHT_SAFETY_ON setting */
void cds_lfht_first_unsafe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
void cds_lfht_next_unsafe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
struct cds_lfht_node *cds_lfht_iter_get_node_unsafe(struct cds_lfht_iter *iter);
int cds_lfht_del_unsafe(struct cds_lfht *ht, struct cds_lfht_node *node);

#endif // LFHT_SAFE_H