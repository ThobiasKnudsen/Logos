#ifndef LFHT_SAFE_H
#define LFHT_SAFE_H

#include "code_monitoring.h"

#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>
#include <stdatomic.h>
#include <string.h>
#include <urcu.h>
#include <urcu/rculfhash.h>

/* -------------------------------------------------------------------------
 * URCU LFHT Safety Wrappers
 * 
 * This header provides safety wrappers for URCU lock-free hash table operations.
 * It includes comprehensive validation, logging, and error detection.
 * 
 * USAGE:
 * 1. Define a function to get node size: size_t my_get_node_size(struct cds_lfht_node* node)
 * 2. Register it with: urcu_safe_set_node_size_function(my_get_node_size)
 * 3. Include this header: #include "urcu_lfht_safe.h"
 * ------------------------------------------------------------------------- */

typedef size_t (*urcu_node_size_func_t)(struct cds_lfht_node* node);
typedef void* (*urcu_node_start_ptr_func_t)(struct cds_lfht_node* node);
void urcu_safe_set_node_size_function(urcu_node_size_func_t func);
urcu_node_size_func_t urcu_safe_get_node_size_function(void);
void urcu_safe_set_node_start_ptr_function(urcu_node_start_ptr_func_t start_func);
urcu_node_start_ptr_func_t urcu_safe_get_node_start_ptr_function(void);

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
 * RCU Safety Wrappers
 * These functions wrap the standard RCU functions with safety checks and
 * logging. They verify proper usage patterns and log errors when detected.
 * ------------------------------------------------------------------------- */
void _rcu_register_thread_safe(void);
void _rcu_unregister_thread_safe(void);
void _rcu_read_lock_safe(void);
void _rcu_read_unlock_safe(void);
void _synchronize_rcu_safe(void);
void _rcu_barrier_safe(void);
void _rcu_init_safe(void);
void _call_rcu_safe(struct rcu_head *head, void (*func)(struct rcu_head *));

/* RCU pointer safety wrapper functions */
void* _rcu_dereference_safe(void* ptr, const char* file, int line);
void _rcu_assign_pointer_safe(void** ptr_addr, void* val, const char* file, int line);
void* _rcu_xchg_pointer_safe(void** ptr_addr, void* val, const char* file, int line);
void* _rcu_cmpxchg_pointer_safe(void** ptr_addr, void* old, void* new, const char* file, int line);

/* -------------------------------------------------------------------------
 * Hash Table Safety Wrappers
 * These functions wrap the cds_lfht functions with safety checks.
 * They verify proper RCU locking and parameter validity.
 * 
 * Note: These wrappers maintain the exact same API signatures as the
 * underlying URCU functions to ensure compatibility.
 * ------------------------------------------------------------------------- */
void _cds_lfht_node_init_safe(struct cds_lfht_node *node);
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

/* Missing functions required by URCU_LFHT_REFERENCE.md */
void _cds_lfht_node_init_deleted_safe(struct cds_lfht_node *node);
int _cds_lfht_is_node_deleted_safe(struct cds_lfht_node *node);

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

/* RCU function call state query functions */
bool _rcu_has_called_barrier(void);
bool _rcu_has_called_call_rcu(void);

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
 * They use CM_SCOPE for proper logging context.
 * ------------------------------------------------------------------------- */

#ifndef LFHT_SAFE_INTERNAL

/* Helper macros for different return types */
#define _URCU_SAFE_VOID_CALL(func, ...) do { \
    CM_SCOPE(func(__VA_ARGS__)); \
} while(0)

#define _URCU_SAFE_BOOL_CALL(func, ...) ({ \
    bool _result; \
    CM_SCOPE(_result = func(__VA_ARGS__)); \
    _result; \
})

#define _URCU_SAFE_INT_CALL(func, ...) ({ \
    int _result; \
    CM_SCOPE(_result = func(__VA_ARGS__)); \
    _result; \
})

#define _URCU_SAFE_PTR_CALL(func, ...) ({ \
    void *_result; \
    CM_SCOPE(_result = func(__VA_ARGS__)); \
    _result; \
})

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

#ifdef rcu_cmpxchg_pointer
#undef rcu_cmpxchg_pointer
#endif

#ifdef rcu_barrier
#undef rcu_barrier
#endif

#ifdef rcu_init
#undef rcu_init
#endif

#ifdef call_rcu
#undef call_rcu
#endif

/* Define our safety wrappers that override the original URCU functions */
#define rcu_init() CM_SCOPE(_rcu_init_safe())
#define rcu_read_lock() CM_SCOPE(_rcu_read_lock_safe())
#define rcu_read_unlock() CM_SCOPE(_rcu_read_unlock_safe())
#define rcu_register_thread() CM_SCOPE(_rcu_register_thread_safe())
#define rcu_unregister_thread() CM_SCOPE(_rcu_unregister_thread_safe())
#define synchronize_rcu() CM_SCOPE(_synchronize_rcu_safe())
#define rcu_barrier() CM_SCOPE(_rcu_barrier_safe())
#define call_rcu(head, func) CM_SCOPE(_call_rcu_safe(head, func))

/* RCU pointer function overrides with comprehensive validation */
#define rcu_dereference(ptr) ({ \
    CM_SCOPE(void* _temp = _rcu_dereference_safe((void*)(ptr), __FILE__, __LINE__)); \
    (typeof((ptr) + 0))_temp; \
})
#define rcu_assign_pointer(ptr_ptr, val) _rcu_assign_pointer_safe((void**)&(ptr_ptr), (void*)(val), __FILE__, __LINE__)
#define rcu_xchg_pointer(ptr_ptr, val) (typeof(val))_rcu_xchg_pointer_safe((void**)ptr_ptr, (void*)(val), __FILE__, __LINE__)
#define rcu_cmpxchg_pointer(ptr_ptr, old, new) (typeof(new))_rcu_cmpxchg_pointer_safe((void**)ptr_ptr, (void*)(old), (void*)(new), __FILE__, __LINE__)

/* Hash table functions with exact API compatibility */
/* Note: We don't override cds_lfht_new to avoid initialization issues */
#define cds_lfht_node_init(node) _URCU_SAFE_VOID_CALL(_cds_lfht_node_init_safe, node)
#define cds_lfht_new(init_size, min_buckets, max_buckets, flags, attr) _URCU_SAFE_PTR_CALL(_cds_lfht_new_safe, init_size, min_buckets, max_buckets, flags, &urcu_memb_flavor)
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
// #define cds_lfht_node_init_deleted(node) _URCU_SAFE_VOID_CALL(_cds_lfht_node_init_deleted_safe, node)
// #define cds_lfht_is_node_deleted(node) _URCU_SAFE_INT_CALL(_cds_lfht_is_node_deleted_safe, node)

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