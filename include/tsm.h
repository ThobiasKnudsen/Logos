#ifndef THREAD_SAFE_MAP_H
#define THREAD_SAFE_MAP_H
#define _LGPL_SOURCE
#include <urcu.h>
#include <urcu/rculfhash.h>
#include "urcu_lfht_safe.h"
#include "code_monitoring.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

/**
 * 
 * ==========================================================================================
 * | High-Concurrency Lock-Free RCU-based Self-Containing Type-Generic Recursive Hash Table |
 * ==========================================================================================
 * 
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
typedef enum tsm_key_type {
    TSM_KEY_TYPE_UINT64,
    TSM_KEY_TYPE_STRING,
    TSM_KEY_TYPE_PARENT, // usefull in path when you want to go to parent instead of a child
    TSM_KEY_TYPE_NONE
} tsm_key_type_t;
/**
 * @union tsm_key_union
 * @brief Represents a key that can be either a 64-bit number or a string.
 *
 * @field number Used when the key is uint64. Must not be 0 (0 is invalid, except during creation for auto-assignment).
 * @field string Used when the key is a string. Must not be NULL or empty.
 *
 * @note Always pair with a `bool key_type` flag. String keys are limited to 63 characters (MAX_STRING_KEY_LEN - 1 = 63).
 * Use functions like `tsm_key_union_create()` and `tsm_key_union_free()` for ANY access to tsm_key_union. Never directly access the fields number and string manually;
 * only use the provided functions on the union. String keys are copied during creation.
 */
union tsm_key_union {
    uint64_t uint64;
    char* string;
};
/**
 * @union tsm_key_union
 * @brief Represents a key that can be either a 64-bit number or a string.
 *
 * @field uint64_key 0 is invalid usually, except during creation in this funcction for auto-assignment.
 *
 * @note Always pair with a `uint8_t key_type`.
 *       Never directly access the fields inside tsm_key_union manually;
 */
CM_RES tsm_key_union_uint64_create(uint64_t uint64_key, union tsm_key_union* p_output_key);
/**
 * @union tsm_key_union
 * @brief Represents a key that can be either a 64-bit number or a string.
 *
 * @field string Must not be NULL or empty.
 *
 * @note Always pair with a `uint8_t key_type`. String keys are limited to 63 characters (MAX_STRING_KEY_LEN - 1 = 63).
 *       Never directly access the fields inside tsm_key_union manually;
 *       only use the provided functions on the union. String keys are copied during creation.
 */
CM_RES tsm_key_union_string_create(const char* string_key, union tsm_key_union* p_output_key);
/**
 * @brief Creates a tsm_key_union union. Copies string keys if applicable.
 *
 * @param uint64_key The numeric key value (if key_type == TSM_KEY_TYPE_UINT64). If 0, logs an error but returns the key (allowed for auto-assignment in node creation).
 * @param string_key The string key value (if key_type == TSM_KEY_TYPE_STRING). Must not be NULL or empty; copied internally.
 * @param key_type Flag indicating if the key is numeric or string.
 * @return A valid tsm_key_union or an invalid one (uint64=0 or string=NULL) on error.
 *
 * @note String keys are validated for length (<=63 chars) and copied. Numeric keys of 0 are allowed here for auto-ID assignment in tsm_base_node_create. Logs errors via tklog.
 * @note Prerequisites: None.
 * @note Call context: Any context (no RCU lock needed).
 */
CM_RES tsm_key_union_free(union tsm_key_union key_union, uint8_t key_type);

// ================================
// tsm_key
// ================================
/**
 * @struct tsm_key
 * @brief Convenience wrapper for tsm_key_union and its type flag, to simplify function parameters.
 *
 * @field key The tsm_key_union.
 * @field key_type The type flag.
 *
 * @note Used to avoid passing separate key and flag parameters. Frees the underlying key when freed.
 */
struct tsm_key {
    union tsm_key_union key_union;
    uint8_t key_type;
};
/**
 * @brief Creates a tsm_key by creating the underlying tsm_key_union.
 *
 * @param uint64_key Numeric key
 * @return A tsm_key with key_type set to uint64.
 *
 * @note Calls tsm_key_union_create() internally.
 * @note Prerequisites: Same as tsm_key_union_create().
 * @note Call context: Any context.
 */
CM_RES tsm_key_uint64_create(uint64_t uint64_key, struct tsm_key* p_output_key);
/**
 * @brief Creates a tsm_key by creating the underlying tsm_key_union.
 *
 * @param uint64_key Numeric key if key_type == TSM_KEY_TYPE_UINT64.
 * @param string_key String key if key_type == TSM_KEY_TYPE_STRING.
 * @param key_type Key type flag.
 * @return A tsm_key.
 *
 * @note Calls tsm_key_union_create() internally.
 * @note Prerequisites: Same as tsm_key_union_create().
 * @note Call context: Any context.
 */ 
CM_RES tsm_key_string_create(const char* string_key, struct tsm_key* p_output_key);
/**
 * @breif checks if key is valid and returns TSM_RESULT_SUCCESS if valid
 */
CM_RES tsm_key_is_valid(const struct tsm_key* key);
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
CM_RES tsm_key_copy(const struct tsm_key* p_src_key, struct tsm_key* p_output_key);
/**
 * @brief Frees a tsm_key and its underlying tsm_key_union.
 *
 * @param key Pointer to the tsm_key.
 * @return CM_RES
 *
 * @note Calls tsm_key_union_free() internally. Sets key.number=0 after free.
 * @note Prerequisites: key created via tsm_key_create().
 * @note Call context: Any context.
 */
CM_RES tsm_key_free(struct tsm_key* key);
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
CM_RES tsm_key_match(const struct tsm_key* p_key_1, const struct tsm_key* p_key_2);
/**
 * @brief Prints the key.
 *
 * @param key The key context to print.
 * @return CM_RES
 *
 * @note Uses tklog_notice to output the key value.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
CM_RES tsm_key_print(const struct tsm_key* p_key);

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
 * @field key_type Flag for `key` type.
 * @field type_key_type Flag for `type_key_union` type.
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
    uint8_t key_type;
    uint8_t type_key_type;
    bool this_is_type;
    bool this_is_tsm;
    uint32_t this_size_bytes; // must be at least sizeof(struct tsm_base_node). this refers to the whole node not just tsm_base_node.
    struct rcu_head rcu_head; // dont use this directly
};
/**
 * @brief Allocates and initializes a new base node (zero-initialized).
 *
 * @param key Node key context (if numeric and 0, auto-assigns a unique ID from atomic counter).
 * @param type_key Type reference context.
 * @param this_size_bytes Total node size (>= `sizeof(struct tsm_base_node)`).
 * @param p_output_node The new node. Pointer to new node. is not written to it result is not TSM_RESULT_SUCCESS
 * @return CM_RES
 *
 * @note Initializes LFHT node (see `cds_lfht_node_init()` in URCU_LFHT_REFERENCE.md).
 * User must fill custom fields outside tsm_base_node before inserting. This is zero-copy for string pointers, so be careful.
 * If key.key.number == 0 and number, assigns a unique non-zero ID using atomic_fetch_add.
 * Logs errors if this_size_bytes too small or allocation fails.
 * @note Prerequisites: Keys must be created via `tsm_key_create()`. `type_key` must reference a valid type. System initialized via tsm_init().
 * @note Call context: Any context (no RCU lock needed).
 * @note takes ownership of both keys so you should not free the keys if the function is successfull
 */
CM_RES tsm_base_node_create(
    const struct tsm_key* p_key,
    const struct tsm_key* p_type_key,
    uint32_t this_size_bytes,
    struct tsm_base_node** pp_output_node);
/**
 * @brief Frees a node (should only be called from RCU callbacks or if node isnt inside a hashtable yet).
 *
 * @param p_base_node The node to free.
 * @return CM_RES
 *
 * @note Frees keys via `tsm_key_union_free()`. For custom types, use the type's `fn_free()`. Logs info about keys being freed.
 * @note Prerequisites: Node must not be in the hash table.
 * @note Call context: From `call_rcu()` callback (outside RCU read sections).
 * @note remember tsm_base_node is the base of all nodes so you should never free whole node with this function,
 * but rather create fn_free_callback function for the specific nodes and then free the base inside the fn_free_callback function
 */
CM_RES tsm_base_node_free(struct tsm_base_node* p_base_node);
/**
 * @brief Validates the base node.
 *
 * @param p_tsm_base The TSM base node.
 * @param p_base The base node to validate.
 * @return CM_RES TSM_RESULT_SUCCESS if valid
 *
 * @note Checks key, type, size, and deletion status.
 * @note Prerequisites: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES tsm_base_node_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base);
/**
 * @brief Prints only the base node.
 *
 * @param p_base The base node to print.
 * @return CM_RES
 *
 * @note Uses tklog_info to output node details.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
CM_RES tsm_base_node_print(const struct tsm_base_node* p_base);

// ================================
// tsm_base_type_node
// ================================
/**
 * @struct tsm_base_type_node
 * @brief Represents a node type, embedding `struct tsm_base_node`. All nodes' `type_key` must reference a type node.
 *
 * @field base Embedded base node (its `type_key` points to itself or another type).
 * @field fn_free_callback RCU callback wrapper that calls `fn_free()` (used with `call_rcu()`).
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
    CM_RES (*fn_is_valid)(const struct tsm_base_node*, const struct tsm_base_node*); // node to check is valid by base node
    CM_RES (*fn_print)(const struct tsm_base_node*); // print all information about the node
    uint32_t type_size_bytes; // bytes
};
/**
 * @brief Creates a base_type_node which is not the actuall base type but the type of this "base type"
 * is base type and the struct of this "base type" is the same as the actuall base type
 *
 * @param key Key for the type node.
 * @param this_size_bytes Size of the type node.
 * @param fn_free_callback Free callback function.
 * @param fn_is_valid Validation function.
 * @param fn_print Print function.
 * @param type_size_bytes Expected size for nodes of this type.
 * @param p_output_node
 * @return Pointer to new base node or NULL on error.
 *
 * @note Uses tsm_base_node_create internally.
 * @note Prerequisites: Valid callbacks and sizes.
 * @note Call context: Any context.
 */
CM_RES tsm_base_type_node_create(
    const struct tsm_key* p_key,
    uint32_t this_size_bytes,
    void (*fn_free_callback)(struct rcu_head*),
    CM_RES (*fn_is_valid)(const struct tsm_base_node*, const struct tsm_base_node*),
    CM_RES (*fn_print)(const struct tsm_base_node*),
    uint32_t type_size_bytes,
    struct tsm_base_node** pp_output_node);

// ==========================================================================================
// tsm_path
// ==========================================================================================
struct tsm_path {
    struct tsm_key* key_chain;
    uint32_t length;
};
/**
 * @brief Inserts a key into the given path at the specified index. Takes ownership of the key if successful.
 *
 * @param p_path Pointer to the tsm_path structure.
 * @param key The tsm_key to insert (passed by value; ownership transferred on success).
 * @param index The position to insert at. Can be negative (counts from the end: -1 for append).
 *              Valid range: -length-1 to length (inclusive).
 * @return CM_RES
 *
 * @note If index < 0, it is converted to a positive index as length + index + 1.
 *       For example, -1 appends to the end (becomes length), -length-1 inserts at the beginning (becomes 0).
 *       The caller should not free the key if the function succeeds.
 * @note Prerequisites: p_path must be valid. key must be created via tsm_key_create() or tsm_key_copy().
 * @note Call context: Any context (no RCU lock needed).
 */
CM_RES tsm_path_insert_key(struct tsm_path* p_path, const struct tsm_key* p_key, int32_t index);
/**
 * @brief Removes a key from the given path at the specified index and frees it.
 *
 * @param p_path Pointer to the tsm_path structure.
 * @param index The position to remove. Can be negative (counts from the end: -1 for last element).
 *              Valid range: -length to -1 or 0 to length-1.
 * @return CM_RES
 *
 * @note If index < 0, it is converted to a positive index as length + index.
 *       For example, -1 removes the last element (becomes length-1).
 * @note Prerequisites: p_path must be valid and non-empty.
 * @note Call context: Any context (no RCU lock needed).
 */
CM_RES tsm_path_remove_key(struct tsm_path* p_path, int32_t index);
/**
 * @brief check if path is valid. will also check all keys in the chain if those are valid as well. will not check if the chain of nodes exist though
 */
CM_RES tsm_path_is_valid(const struct tsm_path* p_path);
/**
 * @brief Frees the path structure and all its contained keys.
 *
 * @param path The tsm_path to free (passed by value).
 * @return true on success, false if the path is in an invalid state.
 *
 * @note Frees each key and the array. Safe to call on null/empty path.
 * @note Prerequisites: Path created via tsm_path_copy().
 * @note Call context: Any context (no RCU lock needed).
 */
CM_RES tsm_path_free(struct tsm_path* path);
/**
 * @brief Prints the path keys in a human-readable format.
 *
 * @param path The tsm_path to print (passed by value).
 * @return true on success, false if buffer overflow or error.
 *
 * @note Outputs to tklog_info, with keys separated by " -> ".
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
CM_RES tsm_path_print(const struct tsm_path* path);
/**
 * @brief Creates a deep copy of the given path, including all keys.
 *
 * @param path_to_copy The tsm_path to copy (passed by value).
 * @return A new tsm_path copy, or null path {NULL, 0} on error.
 *
 * @note Each key is copied via tsm_key_copy().
 * @note Prerequisites: path_to_copy must be valid.
 * @note Call context: Any context.
 */
CM_RES tsm_path_copy(const struct tsm_path* p_path_to_copy, struct tsm_path* p_output_path);
/**
 * 
 */
CM_RES tsm_path_get_key_ref(const struct tsm_path* p_path, int32_t index, const struct tsm_key** pp_key_ref);
/**
 * NOT TESTED
 */
CM_RES tsm_path_create_between_paths(const struct tsm_path* p_path_1, const struct tsm_path* p_path_2, struct tsm_path* p_output_path);
/**
 * NOT TESTED
 */
CM_RES tsm_path_insert_path(const struct tsm_path* p_src_path, struct tsm_path* p_dst_path, int32_t index);
/**
 * NOT TESTED
 */
CM_RES tsm_path_length(const struct tsm_path* p_path, uint32_t* p_output_length);


// ================================
// tsm Thread Safe Map
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
    // cannot store pointer to parent because it could change and when it changes it would need to update all children refeing to itself which is unsustainable
    struct tsm_path path;
};
/**
 * @brief Creates a new TSM node but does not insert it. Use tsm_node_insert to add it.
 *
 * @param p_tsm_base Parent TSM base node which is the only one you can insert into afterwards because the new TSM will inherit the path of the parent.
 * @param tsm_key Key for the new TSM.
 * @param pp_output_node the new tsm node which is not inserted and you have to do manually because you may want to do stuff to it before inserting
 * @return CM_RES
 *
 * @note Creates underlying LFHT, sets up path to parent, and inserts "base_type" and "tsm_type"
 * @note Prerequisites: Parent TSM valid, "tsm_type" exists or is created.
 * @note Call context: must be called within rcu_read section
 */
CM_RES tsm_create(const struct tsm_base_node* p_parent_tsm_base, const struct tsm_key* tsm_key, struct tsm_base_node** pp_output_node);
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
CM_RES tsm_nodes_count(const struct tsm_base_node* p_tsm_base, uint64_t* p_output_count);
/**
 * @breif copy the path of this TSM
 */
CM_RES tsm_copy_path(const struct tsm_base_node* p_tsm_base, struct tsm_path* p_output_path);
/**
 * @breif prints all nodes inside the given TSM
 */
CM_RES tsm_print(const struct tsm_base_node* p_tsm_base);

// ================================
// generic node functions
// ================================
/**
 * @brief Checks if the base node is a TSM node.
 *
 * @param p_base The base node to check.
 * @return TSM_RESULT_SUCCESS if true and TSM_RESULT_NODE_NOT_TSM if false
 *
 * @note Checks this_is_tsm flag.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
CM_RES tsm_node_is_tsm(const struct tsm_base_node* p_base);
/**
 * @brief Checks if the base node is a type node.
 *        ASSUMES p_base is valid
 *
 * @param p_base The base node to check.
 * @return TSM_RESULT_NODE_IS_TYPE if true and TSM_RESULT_NODE_NOT_TYPE if false
 *
 * @note Checks this_is_type flag.
 * @note Prerequisites: None.
 * @note Call context: Any context.
 */
CM_RES tsm_node_is_type(const struct tsm_base_node* p_base);
/**
 * @brief Checks if node is logically deleted.
 *        ASSUMES p_base is valid
 *
 * @param node The node to check.
 * @return TSM_RESULT_SUCCESS if true and TSM_RESULT_NODE_NOT_REMOVED if false
 *
 * @note Uses `cds_lfht_is_node_deleted()` (see URCU_LFHT_REFERENCE.md). Returns true for NULL.
 * @note Prerequisites: Node obtained from get/iterator in same critical section.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES tsm_node_is_removed(const struct tsm_base_node* node);
/**
 * @brief Uses the fn_is_valid function which is in the type for every single node.
 *        ASSUMES p_base is valid
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
CM_RES tsm_node_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base);
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
CM_RES tsm_node_print(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base);
/**
 * @brief Retrieves a node by key (read-only).
 *
 * @param p_tsm_base Pointer to Thread Safe Map from which to find and retrieve the node.
 * @param key The key context to look up.
 * @param p_output_node is the node you are getting. it is NULL if return value is not TSM_SUCCESS
 * @return CM_RES
 *
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 * Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */
CM_RES tsm_node_get(const struct tsm_base_node* p_tsm_base, const struct tsm_key* p_key, const struct tsm_base_node** pp_output_node);
/**
 * @brief Retrieves a node by following the given path from the starting TSM.
 *
 * @param p_tsm_base The starting TSM base node (usually GTSM).
 * @param path The path of keys to follow.
 * @param p_output_node is the node you are getting. it is NULL if return value is not TSM_SUCCESS
 * @return CM_RES
 *
 * @note Each step must resolve to a TSM node except the last.
 * @note Call context: Must be inside rcu_read_lock()/rcu_read_unlock().
 */
CM_RES tsm_node_get_by_path(const struct tsm_base_node* p_tsm_base, const struct tsm_path* p_path, const struct tsm_base_node** pp_output_node);
/**
 * @brief Retrieves a node by following the given path from the starting TSM until the given depth of the key-chain of the path
 *
 * @param p_tsm_base The starting TSM base node (usually GTSM).
 * @param path The path of keys to follow. Can be negative so that it starts at back
 * @param depth The index of the key in the key-chain to stop at
 * @param p_output_node is the node you are getting. it is NULL if return value is not TSM_SUCCESS
 * @return CM_RES
 *
 * @note Each step must resolve to a TSM node except the last.
 * @note Call context: Must be inside rcu_read_lock()/rcu_read_unlock().
 */
CM_RES tsm_node_get_by_path_at_depth(const struct tsm_base_node* p_tsm_base, const struct tsm_path* p_path, int depth, const struct tsm_base_node** pp_output_node);
/**
 * @brief Inserts a new node (fails if key exists).
 *
 * @param p_tsm_base The TSM base node.
 * @param new_node The node to insert.
 * @return CM_RES
 *
 * @note Validates type existence and uses `cds_lfht_add_unique()` for insertion (see URCU_LFHT_REFERENCE.md). 
 * @note Checks that the node's size matches the type's type_size_bytes.
 * @note Logs debug on success, critical if duplicate.
 * @note Prerequisites: Node created via `tsm_base_node_create()`. Type must exist.
 * @note Call context: must be called within rcu_read section
 * @note takes ownership of all data in new_node so should not free stuff in new_node if this is successfull
 */
CM_RES tsm_node_insert(const struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_new_node);
/**
 * @brief Replaces an existing node with a new one (same key/type/size).
 *
 * @param p_tsm_base The TSM base node.
 * @param new_node The new node.
 * @return CM_RES
 *
 * @note Uses `cds_lfht_replace()` and `call_rcu()` for old node (see URCU_LFHT_REFERENCE.md). Checks that the new node's size matches the type's type_size_bytes.
 * Logs debug on success/failure.
 * @note Prerequisites: Existing node with matching key.
 * @note Call context: must be called within rcu_read section
 * @note takes ownership of all data in new_node so should not free stuff in new_node if this is successful
 */
CM_RES tsm_node_update(const struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_new_node);
/**
 * @brief Inserts if key doesn't exist, updates if it does.
 *
 * @param p_tsm_base The TSM base node.
 * @param new_node The node to upsert.
 * @return CM_RES
 *
 * @note Combines `tsm_node_get()` with insert or update.
 * @note Prerequisites: Same as insert/update.
 * @note Call context: must be called within rcu_read section
 * @note takes ownership of all data in new_node so should not free stuff in new_node if this is successful
 */
CM_RES tsm_node_upsert(const struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_new_node);
/**
 * @brief Will delete the node from the TSM then call_rcu with the nodes fn_free_callback type function
 * 
 * Does not check of this provided node exists inside the TSM as there are edge cases where you didnt get 
 * to insert the node and just wants to free it.
 *
 * @param p_tsm_base The TSM base parent node.
 * @param p_base The base node to try free.
 * @return CM_RES
 *
 * @note May require multiple calls with rcu_barrier() in between for dependent nodes.
 * @note Prerequisites: Node exists, no dependencies when returning 1.
 * @note Call context: must NOT be called within rcu_read section
 */
CM_RES tsm_node_defer_free(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base);
/**
 * 
 */
CM_RES tsm_node_copy_key(const struct tsm_base_node* p_base, struct tsm_key* p_output_key);
/**
 * 
 */
CM_RES tsm_node_copy_key_type(const struct tsm_base_node* p_base, struct tsm_key* p_output_key);
/**
 * @brief Initializes iterator to first node.
 *
 * @param p_tsm_base The TSM base node.
 * @param iter The iterator.
 * @return CM_RES
 *
 * @note Wrappers around URCU LFHT iterators (see URCU_LFHT_REFERENCE.md). Iteration order unspecified. Check `tsm_node_is_removed()` during iteration.
 * @note Prerequisites: System initialized.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES tsm_iter_first(const struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter);
/**
 * @brief Advances to next node in the unordered hash table which is the TSM.
 *
 * @param p_tsm_base The TSM base node.
 * @param iter The iterator.
 * @return CM_RES
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES tsm_iter_next(const struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter);
/**
 * @brief Gets the base node from iterator.
 *
 * @param iter The iterator.
 * @param pp_output_node is the node you are getting. it is NULL if return value is not TSM_SUCCESS
 * @return CM_RES
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES tsm_iter_get_node(struct cds_lfht_iter* iter, const struct tsm_base_node** pp_output_node);
/**
 * @brief retrieves the node with given key from within he given TSM and sets the given iterator to point at the retrieved node.
 *
 * @param p_tsm_base The TSM base node.
 * @param key The key context.
 * @param iter The iterator.
 * @return CM_RES
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES tsm_iter_lookup(const struct tsm_base_node* p_tsm_base, const struct tsm_key* p_key, struct cds_lfht_iter* iter);

// ================================
// gtsm Global Thread Safe Map
// ================================
/**
 * @brief Initializes the Global Thread Safe Map.
 *
 * @return true on success, false if already initialized or error.
 *
 * @note Creates root "base_type" and "tsm_type" if needed, sets up global GTSM.
 * @note Prerequisites: rcu_init() called.
 * @note Call context: Any context.
 */
CM_RES gtsm_init();
/**
 * @brief Gets the Global Thread Safe Map.
 *
 * @param p_output_node is the node you are getting. it is NULL if return value is not TSM_SUCCESS
 * @return GTSM base node
 *
 * @note Uses rcu_dereference for safe access.
 * @note Prerequisites: Initialized via gtsm_init().
 * @note Call context: Any context.
 * @note whats unique with this node is that it does not exist inside any TSM since its the first TSM
 */
const struct tsm_base_node* gtsm_get();
/**
 * @brief Frees all nodes in GTSM and frees GTSM itself.
 *
 * @return CM_RES
 *
 * @note Layered cleanup of nodes, then schedules GTSM free via call_rcu.
 * @note Prerequisites: No more insertions, rcu_barrier() for pending callbacks.
 * @warning this function should automatically make more insertions impossible by setting GTSM to NULL and call rcu_barrier but that is not implemented yet
 * @note Call context: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES gtsm_free();
/**
 * @brief Prints information about GTSM and all nodes.
 *
 * @return true on success false on failure.
 *
 * @note Iterates and prints each node inside GTSM using tsm_node_print.
 * @note Prerequisites: Initialized.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
CM_RES gtsm_print();

#endif // THREAD_SAFE_MAP_H