#ifndef GLOBAL_THREAD_SAFE_MAP_H
#define GLOBAL_THREAD_SAFE_MAP_H

#include "tsm.h"

bool  					gtsm_init();

/**
 * @brief get the Global Thread Safe Map
 */
struct tsm_base_node* 	gtsm_get();

/**
 * @breif Frees all nodes
 */
bool 					gtsm_clean();

/**
 * @brief Approximate node count.
 *
 * @return Count (uses `cds_lfht_count_nodes()`).
 *
 * @note Acquires RCU read lock internally. Result is approximate due to concurrency.
 * @note Prerequisites: System initialized.
 * @note Call context: Any context.
 */
unsigned long           gtsm_nodes_count();


// ================================
// generic node functions
// ================================
/**
 * @brief Retrieves a node by key (read-only).
 *
 * @param p_tsm pointer to Thread Safe Map from which to find and retrieve the node
 * @param key_ctx The key context to look up.
 * @return Node pointer or NULL if not found.
 *
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 *       Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */
struct tsm_base_node* 	gtsm_node_get(struct tsm_key_ctx key_ctx);

/**
 * @brief uses the fn_is_valid function which is in the type for every single node
 * 
 * @param key_ctx THe key context to look up
 * @return Returns true if valid false if not
 *
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 *       Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */
bool                    gtsm_node_is_valid(struct tsm_key_ctx key_ctx);

/**
 * @brief gets the type and runs the fn_print_info function in that type
 * 
 * @return false if something failed
 * 
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 *       Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */ 
bool                    gtsm_node_print_info(struct tsm_base_node* p_base);

/**
 * @brief Inserts a new node (fails if key exists).
 *
 * @param new_node The node to insert.
 * @return true on success, false if key exists or error.
 *
 * @note Validates type existence and uses `cds_lfht_add_unique()` (see URCU_LFHT_REFERENCE.md). Checks that the node's size matches the type's type_size_bytes.
 *       Logs debug on success, critical if duplicate.
 * @note Prerequisites: Node created via `tsm_base_node_create()`. Type must exist.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
bool                    gtsm_node_insert(struct tsm_base_node* new_node);

/**
 * @brief Removes a node by key.
 *
 * @param key_ctx The key context to remove.
 * @return true on success, false if not found.
 *
 * @note Uses `cds_lfht_del()` and schedules `call_rcu()` with the type's free callback (see URCU_LFHT_REFERENCE.md).
 *       Falls back to default callback if type not found. Logs debug if not found.
 * @note Prerequisites: Node exists.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
bool                    gtsm_node_free(struct tsm_key_ctx key_ctx);

/**
 * @brief Replaces an existing node with a new one (same key/type/size).
 *
 * @param new_node The new node.
 * @return true on success, false if not found or size mismatch.
 *
 * @note Uses `cds_lfht_replace()` and `call_rcu()` for old node (see URCU_LFHT_REFERENCE.md). Checks that the new node's size matches the type's type_size_bytes.
 *       Logs debug on success/failure.
 * @note Prerequisites: Existing node with matching key.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
bool                    gtsm_node_update(struct tsm_base_node* new_node);

/**
 * @brief Inserts if key doesn't exist, updates if it does.
 *
 * @param new_node The node to upsert.
 * @return true on success.
 *
 * @note Combines `tsm_node_get()` with insert or update.
 * @note Prerequisites: Same as insert/update.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
bool                    gtsm_node_upsert(struct tsm_base_node* new_node);

/**
 * @brief Checks if node is logically deleted.
 *
 * @param node The node to check.
 * @return true if deleted.
 *
 * @note Uses `cds_lfht_is_node_deleted()` (see URCU_LFHT_REFERENCE.md). Returns true for NULL.
 * @note Prerequisites: Node obtained from get/iterator in same critical section.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool                    gtsm_node_is_deleted(struct tsm_base_node* node);

/**
 * @brief Initializes iterator to first node.
 *
 * @param iter The iterator.
 * @return true if node found.
 *
 * @note Wrappers around URCU LFHT iterators (see URCU_LFHT_REFERENCE.md). Iteration order unspecified. Check `tsm_node_is_deleted()` during iteration.
 * @note Prerequisites: System initialized.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool                    gtsm_iter_first(struct cds_lfht_iter* iter);

/**
 * @brief Advances to next node.
 *
 * @param iter The iterator.
 * @return true if node found.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool                    gtsm_iter_next(struct cds_lfht_iter* iter);

/**
 * @brief Gets node from iterator.
 *
 * @param iter The iterator.
 * @return Node or NULL.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
struct tsm_base_node* 	gtsm_iter_get_node(struct cds_lfht_iter* iter);

/**
 * @brief Looks up and sets iterator.
 *
 * @param key_ctx The key context.
 * @param iter The iterator.
 * @return true if found.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool                    gtsm_iter_lookup(struct tsm_key_ctx key_ctx, struct cds_lfht_iter* iter);

#endif // GLOBAL_THREAD_SAFE_MAP_H