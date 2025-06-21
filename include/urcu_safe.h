#ifndef LFHT_SAFE_H
#define LFHT_SAFE_H

#include <urcu.h>
#include <urcu/rculfhash.h>
#include <stdbool.h>
#include <stdint.h>
#include <SDL3/SDL.h>
#include "tklog.h"

#ifdef URCU_LFHT_SAFETY_ON

/* -------------------------------------------------------------------------
 * RCU Safety Wrappers
 * These functions wrap the standard RCU functions with safety checks and
 * logging. They verify proper usage patterns and log errors when detected.
 * ------------------------------------------------------------------------- */
bool _rcu_register_thread_safe(void);
bool _rcu_unregister_thread_safe(void);
bool _rcu_read_lock_safe(void);
bool _rcu_read_unlock_safe(void);
void _synchronize_rcu_safe(void);

/* -------------------------------------------------------------------------
 * Hash Table Safety Wrappers
 * These functions wrap the cds_lfht functions with safety checks.
 * They verify proper RCU locking and parameter validity.
 * ------------------------------------------------------------------------- */
struct cds_lfht_node *_cds_lfht_lookup_safe(struct cds_lfht *ht, unsigned long hash, int (*match)(struct cds_lfht_node *node, const void *key), const void *key);
struct cds_lfht_node *_cds_lfht_add_safe(struct cds_lfht *ht, unsigned long hash, struct cds_lfht_node *node);
struct cds_lfht_node *_cds_lfht_add_unique_safe(struct cds_lfht *ht, unsigned long hash, int (*match)(struct cds_lfht_node *node, const void *key), const void *key, struct cds_lfht_node *node);
struct cds_lfht_node *_cds_lfht_add_replace_safe(struct cds_lfht *ht, unsigned long hash, int (*match)(struct cds_lfht_node *node, const void *key), const void *key, struct cds_lfht_node *node);
int _cds_lfht_del_safe(struct cds_lfht *ht, struct cds_lfht_node *node);
int _cds_lfht_replace_safe(struct cds_lfht *ht, struct cds_lfht_iter *old_iter, unsigned long hash, int (*match)(struct cds_lfht_node *node, const void *key), const void *key, struct cds_lfht_node *new_node);
struct cds_lfht *_cds_lfht_new_safe(unsigned long init_size, unsigned long min_nr_alloc_buckets, unsigned long max_nr_buckets, int flags, const struct rcu_flavor_struct *flavor);
int _cds_lfht_destroy_safe(struct cds_lfht *ht, pthread_attr_t **attr);
void _cds_lfht_resize_safe(struct cds_lfht *ht, unsigned long new_size);
void _cds_lfht_first_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
void _cds_lfht_next_safe(struct cds_lfht *ht, struct cds_lfht_iter *iter);
void _cds_lfht_next_duplicate_safe(struct cds_lfht *ht, int (*match)(struct cds_lfht_node *node, const void *key), const void *key, struct cds_lfht_iter *iter);
struct cds_lfht_node *_cds_lfht_iter_get_node_safe(struct cds_lfht_iter *iter);
long _cds_lfht_count_nodes_safe(struct cds_lfht *ht, long *approx_before, unsigned long *count, long *approx_after);

/* -------------------------------------------------------------------------
 * Helper Functions
 * These provide state information for debugging and verification.
 * ------------------------------------------------------------------------- */
bool _rcu_is_registered(void);
bool _rcu_is_in_read_section(void);
int _rcu_get_lock_depth(void);

/* -------------------------------------------------------------------------
 * Macro Overrides
 * These macros replace the standard RCU/LFHT functions with safety wrappers.
 * They use tklog_scope for proper logging context.
 * ------------------------------------------------------------------------- */

#ifndef LFHT_SAFE_INTERNAL

/* First undefine any existing macros from URCU headers */
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
#ifdef cds_lfht_lookup
#undef cds_lfht_lookup
#endif
#ifdef cds_lfht_add
#undef cds_lfht_add
#endif
#ifdef cds_lfht_add_unique
#undef cds_lfht_add_unique
#endif
#ifdef cds_lfht_add_replace
#undef cds_lfht_add_replace
#endif
#ifdef cds_lfht_del
#undef cds_lfht_del
#endif
#ifdef cds_lfht_replace
#undef cds_lfht_replace
#endif
#ifdef cds_lfht_new
#undef cds_lfht_new
#endif
#ifdef cds_lfht_destroy
#undef cds_lfht_destroy
#endif
#ifdef cds_lfht_resize
#undef cds_lfht_resize
#endif
#ifdef cds_lfht_first
#undef cds_lfht_first
#endif
#ifdef cds_lfht_next
#undef cds_lfht_next
#endif
#ifdef cds_lfht_next_duplicate
#undef cds_lfht_next_duplicate
#endif
#ifdef cds_lfht_iter_get_node
#undef cds_lfht_iter_get_node
#endif
#ifdef cds_lfht_count_nodes
#undef cds_lfht_count_nodes
#endif

/* Now define our safety wrappers */
#define rcu_read_lock() tklog_scope(_rcu_read_lock_safe())
#define rcu_read_unlock() tklog_scope(_rcu_read_unlock_safe())
#define rcu_register_thread() tklog_scope(_rcu_register_thread_safe())
#define rcu_unregister_thread() tklog_scope(_rcu_unregister_thread_safe())
#define synchronize_rcu() tklog_scope(_synchronize_rcu_safe())
#define cds_lfht_lookup(ht, hash, match, key) tklog_scope(_cds_lfht_lookup_safe(ht, hash, match, key))
#define cds_lfht_add(ht, hash, node) tklog_scope(_cds_lfht_add_safe(ht, hash, node))
#define cds_lfht_add_unique(ht, hash, match, key, node) tklog_scope(_cds_lfht_add_unique_safe(ht, hash, match, key, node))
#define cds_lfht_add_replace(ht, hash, match, key, node) tklog_scope(_cds_lfht_add_replace_safe(ht, hash, match, key, node))
#define cds_lfht_del(ht, node) tklog_scope(_cds_lfht_del_safe(ht, node))
#define cds_lfht_replace(ht, old_iter, hash, match, key, new_node) tklog_scope(_cds_lfht_replace_safe(ht, old_iter, hash, match, key, new_node))
#define cds_lfht_new(init_size, min_nr_alloc_buckets, max_nr_buckets, flags, flavor) tklog_scope(_cds_lfht_new_safe(init_size, min_nr_alloc_buckets, max_nr_buckets, flags, flavor))
#define cds_lfht_destroy(ht, attr) tklog_scope(_cds_lfht_destroy_safe(ht, attr))
#define cds_lfht_resize(ht, new_size) tklog_scope(_cds_lfht_resize_safe(ht, new_size))
#define cds_lfht_first(ht, iter) tklog_scope(_cds_lfht_first_safe(ht, iter))
#define cds_lfht_next(ht, iter) tklog_scope(_cds_lfht_next_safe(ht, iter))
#define cds_lfht_next_duplicate(ht, match, key, iter) tklog_scope(_cds_lfht_next_duplicate_safe(ht, match, key, iter))
#define cds_lfht_iter_get_node(iter) tklog_scope(_cds_lfht_iter_get_node_safe(iter))
#define cds_lfht_count_nodes(ht, approx_before, count, approx_after) tklog_scope(_cds_lfht_count_nodes_safe(ht, approx_before, count, approx_after))

#endif /* !LFHT_SAFE_INTERNAL */

#endif // URCU_LFHT_SAFETY_ON

#endif // LFHT_SAFE_H