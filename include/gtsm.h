#ifndef GLOBAL_THREAD_SAFE_MAP_H
#define GLOBAL_THREAD_SAFE_MAP_H
#include "tsm.h"
#include "tklog.h"
#include "xxhash.h"
#include <stdatomic.h>
#include <pthread.h>
#include <unistd.h>
/**
 * @brief Initializes the Global Thread Safe Map.
 *
 * @return true on success, false if already initialized or error.
 *
 * @note Creates root "base_type" and "tsm_type" if needed, sets up global GTSM.
 * @note Prerequisites: rcu_init() called.
 * @note Call context: Any context.
 */
bool gtsm_init();
/**
 * @brief Gets the Global Thread Safe Map.
 *
 * @return Pointer to GTSM base node.
 *
 * @note Uses rcu_dereference for safe access.
 * @note Prerequisites: Initialized via gtsm_init().
 * @note Call context: Any context.
 * @note whats unique with this node is that it does not exist inside any TSM since its the first TSM
 */
struct tsm_base_node* gtsm_get();
/**
 * @brief Frees all nodes in GTSM and frees GTSM itself.
 *
 * @return true on success false on failure.
 *
 * @note Layered cleanup of nodes, then schedules GTSM free via call_rcu.
 * @note Prerequisites: No more insertions, rcu_barrier() for pending callbacks.
 * @warning this function should automatically make more insertions impossible by setting GTSM to NULL and call rcu_barrier but that is not implemented yet
 * @note Call context: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool gtsm_free();
/**
 * @brief Prints information about GTSM and all nodes.
 *
 * @return true on success false on failure.
 *
 * @note Iterates and prints each node inside GTSM using tsm_node_print.
 * @note Prerequisites: Initialized.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool gtsm_print();
/**
 * @brief Approximate node count in GTSM.
 * @note Wrapper for tsm_nodes_count on GTSM so see tsm_nodes_count.
 */
unsigned long gtsm_nodes_count();
// ================================
// generic node functions
// ================================
/**
 * @brief Retrieves a node by key from GTSM (read-only).
 * @note Wrapper for tsm_node_get on GTSM.
 */
struct tsm_base_node* gtsm_node_get(struct tsm_key key);
/**
 * @brief Checks if node in GTSM is valid using type's fn_is_valid.
 * @note Wrapper for tsm_node_is_valid so see tsm_node_is_valid.
 */
bool gtsm_node_is_valid(struct tsm_base_node* p_base);
/**
 * @brief Prints node info in GTSM using type's fn_print.
 * @note Wrapper for tsm_node_print so see tsm_node_print.
 */
bool gtsm_node_print(struct tsm_base_node* p_base);
/**
 * @brief Inserts a new node into GTSM (fails if key exists).
 * @note Wrapper for tsm_node_insert so see tsm_node_insert
 */
bool gtsm_node_insert(struct tsm_base_node* new_node);
/**
 * @brief Runs the try_free_callback for the node in GTSM.
 * @note Wrapper for tsm_node_try_free_callback so see tsm_node_try_free_callback
 */
int gtsm_node_try_free_callback(struct tsm_base_node* p_base);
/**
 * @brief Replaces an existing node in GTSM with a new one (same key/type/size).
 * @note Wrapper for tsm_node_update so see tsm_node_update
 */
bool gtsm_node_update(struct tsm_base_node* new_node);
/**
 * @brief Inserts if key doesn't exist in GTSM, updates if it does.
 * @note Wrapper for tsm_node_upsert so see tsm_node_upsert.
 */
bool gtsm_node_upsert(struct tsm_base_node* new_node);
/**
 * @brief Checks if node is logically deleted.
 * @note Wrapper for tsm_node_is_deleted so see tsm_node_is_deleted.
 */
bool gtsm_node_is_deleted(struct tsm_base_node* node);
/**
 * @brief Initializes iterator to first node in GTSM.
 * @note Wrapper for tsm_iter_first so see tsm_iter_first.
 */
bool gtsm_iter_first(struct cds_lfht_iter* iter);
/**
 * @brief Advances to next node in GTSM.
 * @note Wrapper for tsm_iter_next so see tsm_iter_next.
 */
bool gtsm_iter_next(struct cds_lfht_iter* iter);
/**
 * @brief Gets node from iterator.
 * @note Wrapper for tsm_iter_get_node so see tsm_iter_get_node.
 */
struct tsm_base_node* gtsm_iter_get_node(struct cds_lfht_iter* iter);
/**
 * @brief Looks up and sets iterator in GTSM.
 * @note Wrapper for tsm_iter_lookup so see tsm_iter_lookup
 */
bool gtsm_iter_lookup(struct tsm_key key, struct cds_lfht_iter* iter);
#endif // GLOBAL_THREAD_SAFE_MAP_H