#ifndef THREAD_SAFE_MAP_H
#define THREAD_SAFE_MAP_H
#define _LGPL_SOURCE
#include <urcu.h>
#include <urcu/rculfhash.h>
#include "global_data/urcu_safe.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
/**
 * @file thread_safe_map.h
 * @brief Thread Safe Map Reference
 *
 * ## Overview
 *
 * The Thread Safe Map (TSM) provides a thread-safe, RCU-protected hash table for storing arbitrary data structures.
 * It builds upon the Userspace RCU (URCU) library's lock-free hash table (LFHT) implementation, adding a type system,
 * key management (numeric or string keys), and safe memory handling. It uses xxHash for hashing and a Verstable set for type tracking during cleanup.
 *
 * Key features:
 * - Supports both numeric (uint64_t) and string keys via `union tsm_key_union`.
 * - Every node derives from `struct tsm_base_node` and has a type referenced by `type_key`, which points to a `struct tsm_base_type_node` in the same TSM.
 * - The system requires a special "base_type" node as the root type and first node because all nodes need a type and "base_type" has itself as type.
 * - The type is a set of functions which can be used on a node. All type-nodes has the same set of function names but they differ by pointer values
 * - Almost all functions in this library must be protected by RCU read-side critical sections (`rcu_read_lock()` / `rcu_read_unlock()`).
 * - Freeing the whole TSM handles type dependencies by removing nodes which arent used as types in other nodes first, because the freeing of each node depends on its type node
 *
 * This reference explains all structures, unions, and functions from `tsm.h`.
 * Since the system depends on URCU LFHT, relevant URCU concepts are integrated here (cross-referenced from URCU_LFHT_REFERENCE.md where applicable).
 * For full URCU details, consult URCU_LFHT_REFERENCE.md.
 *
 * ## RCU Integration and Rules
 *
 * The system inherits URCU LFHT rules (see URCU_LFHT_REFERENCE.md for full details):
 * - **Initialization**: The application must call `rcu_init()` before using the system.
 * - **Thread Registration**: Call `rcu_register_thread()` per thread before operations; `rcu_unregister_thread()` before exit.
 * - **Read Safety**: All gets/iterations must be in `rcu_read_lock()` / `rcu_read_unlock()`. As long as you are reading from a pointer obtained from the hash table, the read section must remain open until you are done using that pointers, unless youre using custom syncronization.
 * - **Write Safety**: Inserts/removes use RCU mechanisms; free via `call_rcu()`.
 * - **Restrictions**: Never call `rcu_barrier()`, or `synchronize_rcu()` inside read sections. Avoid deadlocks with mutexes. Never call `rcu_barrier()` within a `call_rcu()` callback.
 * - **Grace Period**: To wait for a grace period after writes (e.g., to ensure deletions are complete), call `synchronize_rcu()`.
 */


// ================================
// tsm_key_union
// ================================
/**
 * @union tsm_key_union
 * @brief Represents a key that can be either a 64-bit number or a string.
 *
 * @field number Used when the key is numeric (key_is_number == true). Must not be 0 (0 is invalid, except during creation for auto-assignment).
 * @field string Used when the key is a string (key_is_number == false). Must not be NULL or empty.
 *
 * @note Always pair with a `bool key_is_number` flag. String keys are limited to 63 characters (MAX_STRING_KEY_LEN - 1 = 63).
 * Use functions like `tsm_key_union_create()` and `tsm_key_union_free()` for ANY access to tsm_key_union. Never directly access the fields number and string manually;
 * only use the provided functions on the union. String keys are copied during creation.
 */
union tsm_key_union {
    uint64_t number;
    char* string;
};
/**
 * @brief Creates a tsm_key_union union. Copies string keys if applicable.
 *
 * @param number_key The numeric key value (if key_is_number == true). If 0, logs an error but returns the key (allowed for auto-assignment in node creation).
 * @param string_key The string key value (if key_is_number == false). Must not be NULL or empty; copied internally.
 * @param key_is_number Flag indicating if the key is numeric (true) or string (false).
 * @return A valid tsm_key_union or an invalid one (number=0 or string=NULL) on error.
 *
 * @note String keys are validated for length (<=63 chars) and copied. Numeric keys of 0 are allowed here for auto-ID assignment in tsm_base_node_create. Logs errors via tklog.
 * @note Prerequisites: None.
 * @note Call context: Any context (no RCU lock needed).
 */
union tsm_key_union tsm_key_union_create(uint64_t number_key, const char* string_key, bool key_is_number);
/**
 * @brief Frees a tsm_key_union if it's a string key (no-op for numeric keys).
 *
 * @param key The tsm_key_union to free.
 * @param key_is_number Flag indicating the key type.
 * @return true on success, false if invalid (e.g., number==0).
 *
 * @note Logs errors if number==0. Does not free numeric keys.
 * @note Prerequisites: Key created via tsm_key_union_create().
 * @note Call context: Any context (no RCU lock needed).
 */
bool tsm_key_union_free(union tsm_key_union key_union, bool key_is_number);
// ================================
// tsm_key
// ================================
/**
 * @struct tsm_key
 * @brief Convenience wrapper for tsm_key_union and its type flag, to simplify function parameters.
 *
 * @field key The tsm_key_union.
 * @field key_is_number The type flag.
 *
 * @note Used to avoid passing separate key and flag parameters. Frees the underlying key when freed.
 */
struct tsm_key {
    union tsm_key_union key_union;
    bool key_is_number;
};
/**
 * @brief Creates a tsm_key by creating the underlying tsm_key_union.
 *
 * @param number_key Numeric key if key_is_number == true.
 * @param string_key String key if key_is_number == false.
 * @param key_is_number Key type flag.
 * @return A tsm_key.
 *
 * @note Calls tsm_key_union_create() internally.
 * @note Prerequisites: Same as tsm_key_union_create().
 * @note Call context: Any context.
 */
struct tsm_key tsm_key_create(uint64_t number_key, const char* string_key, bool key_is_number);
/**
 * @brief Copies a tsm_key.
 *
 * @param key The key context to copy.
 * @return A new tsm_key copy.
 *
 * @note Calls tsm_key_create() internally with the values from key.
 * @note Prerequisites: key created via tsm_key_create().
 * @note Call context: Any context.
 */
struct tsm_key tsm_key_copy(struct tsm_key key);
/**
 * @brief Frees a tsm_key and its underlying tsm_key_union.
 *
 * @param key Pointer to the tsm_key.
 * @return true on success, false if NULL or failure.
 *
 * @note Calls tsm_key_union_free() internally. Sets key.number=0 after free.
 * @note Prerequisites: key created via tsm_key_create().
 * @note Call context: Any context.
 */
bool tsm_key_free(struct tsm_key* key);
/**
 * @brief Checks if keys are the same.
 *
 * @param key_1 First key context.
 * @param key_2 Second key context.
 * @return true if matching, false otherwise.
 *
 * @note Compares types and values (numbers or strings).
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
bool tsm_key_match(struct tsm_key key_1, struct tsm_key key_2);
/**
 * @brief Prints the key.
 *
 * @param key The key context to print.
 *
 * @note Uses tklog_notice to output the key value.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
void tsm_key_print(struct tsm_key key);
// ================================
// tsm_base_node
// ================================
/**
 * @struct tsm_base_node
 * @brief Base structure for all nodes stored in the hash table. All custom node types must embed this as their first member.
 *
 * @field lfht_node Internal URCU LFHT node (do not access directly; see URCU_LFHT_REFERENCE.md for details).
 * @field key The node's key.
 * @field type_key_union Key referencing the node's type (must point to a `struct tsm_base_type_node`).
 * @field key_is_number Flag for `key` type.
 * @field type_key_is_number Flag for `type_key_union` type.
 * @field this_size_bytes Total size of the node (must be >= `sizeof(struct tsm_base_node)`).
 * @field rcu_head Internal RCU head for deferred freeing (do not access directly; see URCU_LFHT_REFERENCE.md).
 *
 * @note Create via `tsm_base_node_create()`. Nodes are zero-initialized except for user-filled fields.
 * For custom types, define a struct like `struct my_node { struct tsm_base_node base; 'custom fields'; };`.
 * Does not use tsm_key internally to save space.
 */
struct tsm_base_node {
    struct cds_lfht_node lfht_node; // dont use this directly
    union tsm_key_union key_union;
    union tsm_key_union type_key_union;
    bool key_is_number;
    bool type_key_is_number;
    bool this_is_type;
    bool this_is_tsm;
    uint32_t this_size_bytes; // must be at least sizeof(struct tsm_base_node)
    struct rcu_head rcu_head; // dont use this directly
};
/**
 * @brief Allocates and initializes a new base node (zero-initialized).
 *
 * @param key Node key context (if numeric and 0, auto-assigns a unique ID from atomic counter).
 * @param type_key Type reference context.
 * @param this_size_bytes Total node size (>= `sizeof(struct tsm_base_node)`).
 * @param this_is_type Flag indicating if this is a type node.
 * @param this_is_tsm Flag indicating if this is a TSM node.
 * @return Pointer to new node or NULL on error.
 *
 * @note Initializes LFHT node (see `cds_lfht_node_init()` in URCU_LFHT_REFERENCE.md).
 * User must fill custom fields outside tsm_base_node before inserting. This is zero-copy for string pointers, so be careful.
 * If key.key.number == 0 and number, assigns a unique non-zero ID using atomic_fetch_add.
 * Logs errors if this_size_bytes too small or allocation fails.
 * @note Prerequisites: Keys must be created via `tsm_key_create()`. `type_key` must reference a valid type. System initialized via tsm_init().
 * @note Call context: Any context (no RCU lock needed).
 * @note takes ownership of both keys so you should not free the keys if the function is successfull
 */
struct tsm_base_node* tsm_base_node_create(
    struct tsm_key key,
    struct tsm_key type_key,
    uint32_t this_size_bytes,
    bool this_is_type,
    bool this_is_tsm);
/**
 * @brief Frees a node (should only be called from RCU callbacks or if node isnt inside a hashtable yet).
 *
 * @param p_base_node The node to free.
 * @return true on success, false if NULL.
 *
 * @note Frees keys via `tsm_key_union_free()`. For custom types, use the type's `fn_free()`. Logs info about keys being freed.
 * @note Prerequisites: Node must not be in the hash table.
 * @note Call context: From `call_rcu()` callback (outside RCU read sections).
 * @note remember tsm_base_node is the base of all nodes so you should never free whole node with this function,
 * but rather create fn_free_callback function for the specific nodes and then free the base inside the fn_free_callback function
 */
bool tsm_base_node_free(struct tsm_base_node* p_base_node);
/**
 * @brief This must be called when no other nodes depend on this node and you want to free it.
 * If other nodes still depend on this and you call this function it is UB.
 * You must use this inside the user implementation of try_free_callback functions in the section when
 * try_free_callback will return 1 and not 0. The reason that function returns 0 is to let other nodes not
 * depend on this node. Then you need to call try_free_callback again over and over untill you get 1 because
 * at that time there are no other nodes which depend on this node and therefore this function can then be called
 */
bool tsm_base_node_free_callback(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base);
/**
 * @brief Validates the base node.
 *
 * @param p_tsm_base The TSM base node.
 * @param p_base The base node to validate.
 * @return true if valid, false otherwise.
 *
 * @note Checks key, type, size, and deletion status.
 * @note Prerequisites: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool tsm_base_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base);
/**
 * @brief Prints only the base node.
 *
 * @param p_base The base node to print.
 * @return true on success.
 *
 * @note Uses tklog_info to output node details.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
bool tsm_base_node_print(struct tsm_base_node* p_base);
// ================================
// tsm_base_type_node
// ================================
/**
 * @struct tsm_base_type_node
 * @brief Represents a node type, embedding `struct tsm_base_node`. All nodes' `type_key` must reference a type node.
 *
 * @field base Embedded base node (its `type_key` points to itself or another type).
 * @field fn_free_callback RCU callback wrapper that calls `fn_free()` (used with `call_rcu()`).
 * @field fn_try_free_callback Function to attempt freeing a node of this type, may require multiple calls.
 * @field fn_is_valid Validation function for nodes of this type (user-called after get to check node integrity).
 * @field fn_print Print information about the node.
 * @field type_size_bytes Expected size of nodes of this type (for validation during insert/update).
 *
 * @note The system initializes a root "base_type" node with key `{ .string = "base_type" }` (string key). Custom types should derive from this.
 * Free functions must handle key freeing via `tsm_key_union_free()`. Validation should check `this_size_bytes` and other invariants.
 * Default callbacks provided for base_type: _tsm_base_type_node_free, etc.
 */
struct tsm_base_type_node {
    struct tsm_base_node base; // type_key will be set to point to the base_type node
    void (*fn_free_callback)(struct rcu_head*); // node to free by base node as callback which should call fn_free
    // -1 is error, 0 is scheduled but not deleted and 1 is scheculed and deleted. in the case of 0 it must be scheduled again after calling rcu_barrier()
    // if fn_try_free_callback is called without the previous callback function being called on the same node this will cause undefined behaviour. rcu_barrier() must be called to ensure that
    // fn_try_free_callback can be called again.
    int (*fn_try_free_callback)(struct tsm_base_node*, struct tsm_base_node*); // this is needed because for some nodes you want to call rcu multiple times because the node itself contains other nodes which must be freed first
    bool (*fn_is_valid)(struct tsm_base_node*, struct tsm_base_node*); // node to check is valid by base node
    bool (*fn_print)(struct tsm_base_node*); // print all information about the node
    uint32_t type_size_bytes; // bytes
};
/**
 * @brief Creates a base_type_node which is not the actuall base type but the type of this "base type"
 * is base type and the struct of this "base type" is the same as the actuall base type
 *
 * @param key Key for the type node.
 * @param this_size_bytes Size of the type node.
 * @param fn_free_callback Free callback function.
 * @param fn_try_free_callback Try free callback function.
 * @param fn_is_valid Validation function.
 * @param fn_print Print function.
 * @param type_size_bytes Expected size for nodes of this type.
 * @return Pointer to new base node or NULL on error.
 *
 * @note Uses tsm_base_node_create internally.
 * @note Prerequisites: Valid callbacks and sizes.
 * @note Call context: Any context.
 */
struct tsm_base_node* tsm_base_type_node_create(
    struct tsm_key key,
    uint32_t this_size_bytes,
    void (*fn_free_callback)(struct rcu_head*),
    int (*fn_try_free_callback)(struct tsm_base_node*, struct tsm_base_node*),
    bool (*fn_is_valid)(struct tsm_base_node*, struct tsm_base_node*),
    bool (*fn_print)(struct tsm_base_node*),
    uint32_t type_size_bytes);
// ================================
// tsm
// ================================
/**
 * @struct tsm
 * @brief Thread Safe Map structure, embedding tsm_base_node.
 *
 * @field base Embedded base node.
 * @field p_ht Pointer to the underlying LFHT.
 * @field path_from_global_to_parent_tsm Array of key contexts to parent TSM.
 * @field path_length Length of the path array.
 */
struct tsm {
    struct tsm_base_node base;
    struct cds_lfht* p_ht;
    // cannot store pointer to parent because it could change and in it changing it would need to update all children refeing to itself first
    struct tsm_key* path_from_global_to_parent_tsm;
    uint32_t path_length;
};
/**
 * @brief Creates a new TSM node but does not insert it. Use tsm_node_insert to add it.
 *
 * @param p_tsm_base Parent TSM base node.
 * @param tsm_key Key for the new TSM.
 * @return Pointer to new TSM base node or NULL on error.
 *
 * @note Creates underlying LFHT, sets up path to parent.
 * @note Prerequisites: Parent TSM valid, "tsm_type" exists or is created.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
struct tsm_base_node* tsm_create_and_insert(
    struct tsm_base_node* p_tsm_base,
    struct tsm_key tsm_key);
/**
 * @brief Gets the parent TSM node, traversing from global.
 *
 * @param p_tsm_base The TSM base node.
 * @return Parent TSM base node or NULL if global.
 *
 * @note Uses path_from_global_to_parent_tsm to traverse.
 * @note Prerequisites: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
struct tsm_base_node* tsm_get_parent_tsm(struct tsm_base_node* p_tsm_base);
/**
 * @brief Checks if the base node is a TSM node.
 *
 * @param p_base The base node to check.
 * @return true if TSM, false otherwise.
 *
 * @note Checks this_is_tsm flag.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
bool tsm_node_is_tsm(struct tsm_base_node* p_base);
/**
 * @brief Checks if the base node is a type node.
 *
 * @param p_base The base node to check.
 * @return true if type, false otherwise.
 *
 * @note Checks this_is_type flag.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
bool tsm_node_is_type(struct tsm_base_node* p_base);
/**
 * @brief Approximate node count.
 *
 * @param p_tsm_base The TSM base node.
 * @return Approximate count.
 *
 * @note Uses `cds_lfht_count_nodes()` (see URCU_LFHT_REFERENCE.md).
 * @note Prerequisites: System initialized.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
unsigned long tsm_nodes_count(struct tsm_base_node* p_tsm_base);
// ================================
// generic node functions
// ================================
/**
 * @brief Retrieves a node by key (read-only).
 *
 * @param p_tsm_base Pointer to Thread Safe Map from which to find and retrieve the node.
 * @param key The key context to look up.
 * @return Node pointer or NULL if not found.
 *
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 * Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */
struct tsm_base_node* tsm_node_get(struct tsm_base_node* p_tsm_base, struct tsm_key key);
/**
 * @brief Uses the fn_is_valid function which is in the type for every single node.
 *
 * @param p_tsm_base The TSM base node.
 * @param key The key context to look up.
 * @return true if valid, false if not.
 *
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 * Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */
bool tsm_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base);
/**
 * @brief Gets the type and runs the fn_print function in that type.
 *
 * @param p_tsm_base The TSM base node.
 * @param p_base The base node to print.
 * @return true on success, false if failed.
 *
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 * Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */
bool tsm_node_print(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base);
/**
 * @brief Inserts a new node (fails if key exists).
 *
 * @param p_tsm_base The TSM base node.
 * @param new_node The node to insert.
 * @return true on success, false if key exists or error.
 *
 * @note Validates type existence and uses `cds_lfht_add_unique()` for insertion (see URCU_LFHT_REFERENCE.md). 
 * @note Checks that the node's size matches the type's type_size_bytes.
 * @note Logs debug on success, critical if duplicate.
 * @note Prerequisites: Node created via `tsm_base_node_create()`. Type must exist.
 * @note Call context: Any context (acquires RCU read lock internally).
 * @note takes ownership of all data in new_node so should not free stuff in new_node if this is successfull
 */
bool tsm_node_insert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node);
/**
 * @brief Will run the try_free_callback for the node.
 *
 * @param p_tsm_base The TSM base node.
 * @param p_base The base node to try free.
 * @return -1 error, 0 scheduled but not deleted, 1 scheduled and deleted.
 *
 * @note May require multiple calls with rcu_barrier() in between for dependent nodes.
 * @note Prerequisites: Node exists, no dependencies when returning 1.
 * @note Call context: Any context.
 */
int tsm_node_try_free_callback(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base);
/**
 * @brief Replaces an existing node with a new one (same key/type/size).
 *
 * @param p_tsm_base The TSM base node.
 * @param new_node The new node.
 * @return true on success, false if not found or size mismatch.
 *
 * @note Uses `cds_lfht_replace()` and `call_rcu()` for old node (see URCU_LFHT_REFERENCE.md). Checks that the new node's size matches the type's type_size_bytes.
 * Logs debug on success/failure.
 * @note Prerequisites: Existing node with matching key.
 * @note Call context: Any context (acquires RCU read lock internally).
 * @note takes ownership of all data in new_node so should not free stuff in new_node if this is successful
 */
bool tsm_node_update(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node);
/**
 * @brief Inserts if key doesn't exist, updates if it does.
 *
 * @param p_tsm_base The TSM base node.
 * @param new_node The node to upsert.
 * @return true on success.
 *
 * @note Combines `tsm_node_get()` with insert or update.
 * @note Prerequisites: Same as insert/update.
 * @note Call context: Any context (acquires RCU read lock internally).
 * @note takes ownership of all data in new_node so should not free stuff in new_node if this is successful
 */
bool tsm_node_upsert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node);
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
bool tsm_node_is_deleted(struct tsm_base_node* node);
/**
 * @brief Initializes iterator to first node.
 *
 * @param p_tsm_base The TSM base node.
 * @param iter The iterator.
 * @return true if node found.
 *
 * @note Wrappers around URCU LFHT iterators (see URCU_LFHT_REFERENCE.md). Iteration order unspecified. Check `tsm_node_is_deleted()` during iteration.
 * @note Prerequisites: System initialized.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool tsm_iter_first(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter);
/**
 * @brief Advances to next node in the unordered hash table which is the TSM.
 *
 * @param p_tsm_base The TSM base node.
 * @param iter The iterator.
 * @return true if node found.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool tsm_iter_next(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter);
/**
 * @brief Gets the base node from iterator.
 *
 * @param iter The iterator.
 * @return Node or NULL.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
struct tsm_base_node* tsm_iter_get_node(struct cds_lfht_iter* iter);
/**
 * @brief retrieves the node with given key from within he given TSM and sets the given iterator to point at the retrieved node.
 *
 * @param p_tsm_base The TSM base node.
 * @param key The key context.
 * @param iter The iterator.
 * @return true if found false if not.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool tsm_iter_lookup(struct tsm_base_node* p_tsm_base, struct tsm_key key, struct cds_lfht_iter* iter);
#endif // THREAD_SAFE_MAP_H