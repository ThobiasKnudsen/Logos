#ifndef GLOBAL_DATA_CORE_H
#define GLOBAL_DATA_CORE_H

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
 * @file global_data/core.h
 * @brief Global Data Core Reference
 *
 * ## Overview
 *
 * The Global Data Core provides a thread-safe, RCU-protected hash table for storing arbitrary data structures.
 * It builds upon the Userspace RCU (URCU) library's lock-free hash table (LFHT) implementation, adding a type system,
 * key management (numeric or string keys), and safe memory handling. It uses xxHash for hashing and a Verstable set for type tracking during cleanup.
 *
 * Key features:
 * - Supports both numeric (uint64_t) and string keys via `union gd_key`.
 * - Every node derives from `struct gd_base_node` and has a type referenced by `type_key`, which points to a `struct gd_base_type_node`.
 * - The system requires a special "base_type" node as the root type and first node because all nodes need a type and "base_type" has itself as type.
 *   The type is a set of functions which can be used on a node. By default, every type must inherit from `gd_base_type_node` by setting it as the first field.
 * - All read operations must be protected by RCU read-side critical sections (`rcu_read_lock()` / `rcu_read_unlock()`).
 * - Write operations (insert/remove/update) are RCU-safe and use `call_rcu()` for deferred freeing.
 * - Cleanup handles type dependencies by removing nodes in layers (instances before types) using a Verstable set to track used types.
 *
 * This reference explains all structures, unions, and functions from `global_data/core.h`.
 * Since the system depends on URCU LFHT, relevant URCU concepts are integrated here (cross-referenced from URCU_LFHT_REFERENCE.md where applicable).
 * For full URCU details, consult URCU_LFHT_REFERENCE.md.
 *
 * ## RCU Integration and Rules
 *
 * The system inherits URCU LFHT rules (see URCU_LFHT_REFERENCE.md for full details):
 * - **Initialization**: The application must call `rcu_init()` before using the system.
 * - **Thread Registration**: Call `rcu_register_thread()` per thread before operations; `rcu_unregister_thread()` before exit.
 * - **Read Safety**: All gets/iterations must be in `rcu_read_lock()` / `rcu_read_unlock()`. As long as you are reading from a pointer obtained from the hash table, the read section must remain open until you are done using that pointer, unless youre using custom syncronization.
 * - **Write Safety**: Inserts/removes use RCU mechanisms; free via `call_rcu()`.
 * - **Restrictions**: Never call `gd_cleanup()`, `rcu_barrier()`, or `synchronize_rcu()` inside read sections. Avoid deadlocks with mutexes. Never call `rcu_barrier()` within a `call_rcu()` callback.
 * - **Cleanup**: Use `gd_cleanup()` at program end to handle layered removal. Calls `rcu_barrier()` internally.
 * - **Grace Period**: To wait for a grace period after writes (e.g., to ensure deletions are complete), call `synchronize_rcu()`.
 *
 * ## Common Patterns
 *
 * ### Basic Usage
 * ```c
 * rcu_init();
 * gd_init();
 * rcu_register_thread();
 *
 * // Create and insert
 * struct gd_key_ctx type_key_ctx = gd_base_type_key_ctx_copy();
 * struct gd_key_ctx key_ctx = gd_key_ctx_create(42, NULL, true);
 * struct gd_base_node* node = gd_base_node_create(key_ctx, type_key_ctx, sizeof(struct gd_base_node));
 * gd_node_insert(node);
 *
 * // Get
 * rcu_read_lock();
 * struct gd_base_node* found = gd_node_get(key_ctx);
 * rcu_read_unlock();
 *
 * // Cleanup
 * rcu_unregister_thread();
 * gd_cleanup();
 * ```
 *
 * ### Custom Type
 * Define type node with custom free/validate, insert it, then use its key for instances.
 * ```c
 * // Custom node
 * struct my_node {
 *     struct gd_base_node base;
 *     int value;
 * };
 *
 * // Custom callbacks
 * static bool my_node_free(struct gd_base_node* node) {
 *     // Custom free logic if needed
 *     return gd_base_node_free(node);
 * }
 *
 * static void my_node_free_callback(struct rcu_head* head) {
 *     struct gd_base_node* node = caa_container_of(head, struct gd_base_node, rcu_head);
 *     my_node_free(node);
 * }
 *
 * static bool my_node_is_valid(struct gd_base_node* node) {
 *     struct my_node* mn = (struct my_node*)node;
 *     return mn->value >= 0;  // Example validation
 * }
 *
 * // Create custom type
 * struct gd_key_ctx my_type_key = gd_key_ctx_create(0, "my_type", false);
 * struct gd_base_type_node* type_node = (struct gd_base_type_node*)gd_base_node_create(
 *     my_type_key, gd_base_type_key_ctx_copy(), sizeof(struct gd_base_type_node));
 * type_node->fn_free_node = my_node_free;
 * type_node->fn_free_node_callback = my_node_free_callback;
 * type_node->fn_is_valid = my_node_is_valid;
 * type_node->fn_print_info = my_node_print;
 * type_node->type_size = sizeof(struct my_node);
 * gd_node_insert(&type_node->base);
 *
 * // Create instance
 * struct gd_key_ctx inst_key = gd_key_ctx_create(100, NULL, true);
 * struct my_node* inst = (struct my_node*)gd_base_node_create(
 *     inst_key, my_type_key, sizeof(struct my_node));
 * inst->value = 42;
 * gd_node_insert(&inst->base);
 *
 * // Usage: After get, can call type's fn_is_valid
 * rcu_read_lock();
 * struct my_node* found = (struct my_node*)gd_node_get(inst_key);
 * if (found && my_node_is_valid(&found->base)) {
 *     // Use found
 * }
 * rcu_read_unlock();
 * ```
 *
 * ### Concurrent Usage
 * ```c
 * // In worker thread
 * rcu_register_thread();
 * // Perform inserts, gets, updates, removes
 * rcu_unregister_thread();
 * ```
 *
 * For errors, check logs (tklog). For testing, internal functions like `_gd_get_hash_table()` can be used (not declared in header).
 */

// ================================
// gd_key
// ================================

/**
 * @union gd_key
 * @brief Represents a key that can be either a 64-bit number or a string.
 *
 * @field number Used when the key is numeric (key_is_number == true). Must not be 0 (0 is invalid, except during creation for auto-assignment).
 * @field string Used when the key is a string (key_is_number == false). Must not be NULL or empty.
 *
 * @note Always pair with a `bool key_is_number` flag. String keys are limited to 63 characters (MAX_STRING_KEY_LEN - 1 = 63).
 *       Use functions like `gd_key_create()` and `gd_key_free()` for ANY access to gd_key. Never directly access the fields number and string manually;
 *       only use the provided functions on the union. String keys are copied during creation.
 */
union gd_key {
    uint64_t number;
    char* string;
};

/**
 * @brief Creates a gd_key union. Copies string keys if applicable.
 *
 * @param number_key The numeric key value (if key_is_number == true). If 0, logs an error but returns the key (allowed for auto-assignment in node creation).
 * @param string_key The string key value (if key_is_number == false). Must not be NULL or empty; copied internally.
 * @param key_is_number Flag indicating if the key is numeric (true) or string (false).
 * @return A valid gd_key or an invalid one (number=0 or string=NULL) on error.
 *
 * @note String keys are validated for length (<=63 chars) and copied. Numeric keys of 0 are allowed here for auto-ID assignment in gd_base_node_create. Logs errors via tklog.
 * @note Prerequisites: None.
 * @note Call context: Any context (no RCU lock needed).
 */
union gd_key            gd_key_create(uint64_t number_key, const char* string_key, bool key_is_number);

/**
 * @brief Frees a gd_key if it's a string key (no-op for numeric keys).
 *
 * @param key The gd_key to free.
 * @param key_is_number Flag indicating the key type.
 * @return true on success, false if invalid (e.g., number==0).
 *
 * @note Logs errors if number==0. Does not free numeric keys.
 * @note Prerequisites: Key created via gd_key_create().
 * @note Call context: Any context (no RCU lock needed).
 */
bool                    gd_key_free(union gd_key key, bool key_is_number);

/**
 * @brief Gets the numeric value from a gd_key.
 *
 * @param key The gd_key.
 * @param key_is_number Must be true; behavior undefined if false.
 * @return The numeric value.
 *
 * @note Prerequisites: key_is_number == true.
 * @note Call context: Any context.
 */
uint64_t                gd_key_get_number(union gd_key, bool key_is_number);

/**
 * @brief Gets the string value from a gd_key.
 *
 * @param key The gd_key.
 * @param key_is_number Must be false; behavior undefined if true.
 * @return The string value.
 *
 * @note Prerequisites: key_is_number == false.
 * @note Call context: Any context.
 */
const char*             gd_key_get_string(union gd_key, bool key_is_number);

// ================================
// gd_key_ctx
// ================================

/**
 * @struct gd_key_ctx
 * @brief Convenience wrapper for gd_key and its type flag, to simplify function parameters.
 *
 * @field key The gd_key.
 * @field key_is_number The type flag.
 *
 * @note Used to avoid passing separate key and flag parameters. Frees the underlying key when freed.
 */
struct gd_key_ctx {
    union gd_key key;
    bool key_is_number;
};

/**
 * @brief Creates a gd_key_ctx by creating the underlying gd_key.
 *
 * @param number_key Numeric key if key_is_number == true.
 * @param string_key String key if key_is_number == false.
 * @param key_is_number Key type flag.
 * @return A gd_key_ctx.
 *
 * @note Calls gd_key_create() internally.
 * @note Prerequisites: Same as gd_key_create().
 * @note Call context: Any context.
 */
struct gd_key_ctx       gd_key_ctx_create(uint64_t number_key, const char* string_key, bool key_is_number);

/**
 * @brief Frees a gd_key_ctx and its underlying gd_key.
 *
 * @param key_ctx Pointer to the gd_key_ctx.
 * @return true on success, false if NULL or failure.
 *
 * @note Calls gd_key_free() internally. Sets key.number=0 after free.
 * @note Prerequisites: key_ctx created via gd_key_ctx_create().
 * @note Call context: Any context.
 */
bool                    gd_key_ctx_free(struct gd_key_ctx* key_ctx);

// ================================
// gd_base_node
// ================================

/**
 * @struct gd_base_node
 * @brief Base structure for all nodes stored in the hash table. All custom node types must embed this as their first member.
 *
 * @field lfht_node Internal URCU LFHT node (do not access directly; see URCU_LFHT_REFERENCE.md for details).
 * @field key The node's key.
 * @field type_key Key referencing the node's type (must point to a `struct gd_base_type_node`).
 * @field key_is_number Flag for `key` type.
 * @field type_key_is_number Flag for `type_key` type.
 * @field size_bytes Total size of the node (must be >= `sizeof(struct gd_base_node)`).
 * @field rcu_head Internal RCU head for deferred freeing (do not access directly; see URCU_LFHT_REFERENCE.md).
 *
 * @note Create via `gd_base_node_create()`. Nodes are zero-initialized except for user-filled fields.
 *       For custom types, define a struct like `struct my_node { struct gd_base_node base; 'custom fields'; };`.
 *       Does not use gd_key_ctx internally to save space.
 */
struct gd_base_node {
    struct cds_lfht_node lfht_node; // dont use this directly
    union gd_key key;
    union gd_key type_key;
    bool key_is_number;
    bool type_key_is_number;
    uint32_t size_bytes; // must be at least sizeof(struct gd_base_node)
    struct rcu_head rcu_head; // dont use this directly
};

/**
 * @brief Allocates and initializes a new base node (zero-initialized).
 *
 * @param key_ctx Node key context (if numeric and 0, auto-assigns a unique ID from atomic counter).
 * @param type_key_ctx Type reference context.
 * @param whole_node_size_bytes Total node size (>= `sizeof(struct gd_base_node)`).
 * @return Pointer to new node or NULL on error.
 *
 * @note Initializes LFHT node (see `cds_lfht_node_init()` in URCU_LFHT_REFERENCE.md).
 *       User must fill custom fields outside gd_base_node before inserting. This is zero-copy for string pointers, so be careful.
 *       If key_ctx.key.number == 0 and number, assigns a unique non-zero ID using atomic_fetch_add.
 *       Logs errors if size_bytes too small or allocation fails.
 * @note Prerequisites: Keys must be created via `gd_key_ctx_create()`. `type_key` must reference a valid type. System initialized via gd_init().
 * @note Call context: Any context (no RCU lock needed).
 * @note takes ownership of both keys so you should not free the keys if the function is successfull
 */
struct gd_base_node*    gd_base_node_create(struct gd_key_ctx key_ctx, struct gd_key_ctx type_key_ctx, uint32_t whole_node_size_bytes);

/**
 * @brief Frees a node (should only be called from RCU callbacks).
 *
 * @param p_base_node The node to free.
 * @return true on success, false if NULL.
 *
 * @note Frees keys via `gd_key_free()`. For custom types, use the type's `fn_free_node()`. Logs info about keys being freed.
 * @note Prerequisites: Node must not be in the hash table.
 * @note Call context: From `call_rcu()` callback (outside RCU read sections).
 * @note remember gd_base_node is the base of all nodes so you should never free whole node with this function, 
 *       but rather create a free function for the specific nodes and then free the base inside the nodes with this function
 */
bool                    gd_base_node_free(struct gd_base_node* p_base);

/**
 * @breif validates the base node
 * 
 * 
 */
bool                    gd_base_node_is_valid(struct gd_base_node* p_base);

/**
 * @brief prints only the base node
 * 
 * @param the base node to print
 * @return void
 * 
 * @note
 */
bool                    gd_base_node_print_info(struct gd_base_node* p_base);

/**
 * @brief Retrieves a node by key (read-only).
 *
 * @param key_ctx The key context to look up.
 * @return Node pointer or NULL if not found.
 *
 * @note Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`) or custom syncronization primitives.
 *       Logs debug info about lookup. Errors if key.number==0.
 * @note Prerequisites: System initialized. All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).
 * @note Call context: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
 */
struct gd_base_node*    gd_node_get(struct gd_key_ctx key_ctx);

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
bool                    gd_node_is_valid(struct gd_key_ctx key_ctx);

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
bool                    gd_node_print_info(struct gd_base_node* p_base);

/**
 * @brief Inserts a new node (fails if key exists).
 *
 * @param new_node The node to insert.
 * @return true on success, false if key exists or error.
 *
 * @note Validates type existence and uses `cds_lfht_add_unique()` (see URCU_LFHT_REFERENCE.md). Checks that the node's size matches the type's type_size.
 *       Logs debug on success, critical if duplicate.
 * @note Prerequisites: Node created via `gd_base_node_create()`. Type must exist.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
bool                    gd_node_insert(struct gd_base_node* new_node);

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
bool                    gd_node_remove(struct gd_key_ctx key_ctx);

/**
 * @brief Replaces an existing node with a new one (same key/type/size).
 *
 * @param new_node The new node.
 * @return true on success, false if not found or size mismatch.
 *
 * @note Uses `cds_lfht_replace()` and `call_rcu()` for old node (see URCU_LFHT_REFERENCE.md). Checks that the new node's size matches the type's type_size.
 *       Logs debug on success/failure.
 * @note Prerequisites: Existing node with matching key.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
bool                    gd_node_update(struct gd_base_node* new_node);

/**
 * @brief Inserts if key doesn't exist, updates if it does.
 *
 * @param new_node The node to upsert.
 * @return true on success.
 *
 * @note Combines `gd_node_get()` with insert or update.
 * @note Prerequisites: Same as insert/update.
 * @note Call context: Any context (acquires RCU read lock internally).
 */
bool                    gd_node_upsert(struct gd_base_node* new_node);

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
bool                    gd_node_is_deleted(struct gd_base_node* node);

/**
 * @brief Approximate node count.
 *
 * @return Count (uses `cds_lfht_count_nodes()`).
 *
 * @note Acquires RCU read lock internally. Result is approximate due to concurrency.
 * @note Prerequisites: System initialized.
 * @note Call context: Any context.
 */
unsigned long           gd_nodes_count(void);

/**
 * @brief Initializes iterator to first node.
 *
 * @param iter The iterator.
 * @return true if node found.
 *
 * @note Wrappers around URCU LFHT iterators (see URCU_LFHT_REFERENCE.md). Iteration order unspecified. Check `gd_node_is_deleted()` during iteration.
 * @note Prerequisites: System initialized.
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool                    gd_iter_first(struct cds_lfht_iter* iter);

/**
 * @brief Advances to next node.
 *
 * @param iter The iterator.
 * @return true if node found.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool                    gd_iter_next(struct cds_lfht_iter* iter);

/**
 * @brief Gets node from iterator.
 *
 * @param iter The iterator.
 * @return Node or NULL.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
struct gd_base_node*    gd_iter_get_node(struct cds_lfht_iter* iter);

/**
 * @brief Looks up and sets iterator.
 *
 * @param key_ctx The key context.
 * @param iter The iterator.
 * @return true if found.
 *
 * @note Call context: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
 */
bool                    gd_iter_lookup(struct gd_key_ctx key_ctx, struct cds_lfht_iter* iter);

// ================================
// gd_base_type_node
// ================================

/**
 * @struct gd_base_type_node
 * @brief Represents a node type, embedding `struct gd_base_node`. All nodes' `type_key` must reference a type node.
 *
 * @field base Embedded base node (its `type_key` points to itself or another type).
 * @field fn_free_node Function to free a node of this type (returns true on success). Typically calls gd_base_node_free after custom logic.
 * @field fn_free_node_callback RCU callback wrapper that calls `fn_free_node()` (used with `call_rcu()`).
 * @field fn_is_valid Validation function for nodes of this type (user-called after get to check node integrity).
 * @field fn_print_info Print information about the node
 * @field type_size Expected size of nodes of this type (for validation during insert/update).
 *
 * @note The system initializes a root "base_type" node with key `{ .string = "base_type" }` (string key). Custom types should derive from this.
 *       Free functions must handle key freeing via `gd_key_free()`. Validation should check `size_bytes` and other invariants.
 *       Default callbacks provided for base_type: _gd_base_type_node_free, etc.
 */
struct gd_base_type_node {
    struct gd_base_node base; // type_key will be set to point to the base_type node
    bool (*fn_free_node)(struct gd_base_node*); // node to free by base node
    void (*fn_free_node_callback)(struct rcu_head*); // node to free by base node as callback which should call fn_free_node
    bool (*fn_is_valid)(struct gd_base_node*); // node to check is valid by base node
    bool (*fn_print_info)(struct gd_base_node*); // print all information about the node
    uint32_t type_size; // bytes
};

/**
 * @brief Copies the "base_type" key context.
 *
 * @return Key context copy.
 *
 * @note Calls gd_key_ctx_create() with "base_type".
 */
struct gd_key_ctx       gd_base_type_key_ctx_copy(void);

// ================================
// Core system functions
// ================================

/**
 * @brief Initializes the global data system, including URCU and the hash table.
 *
 * @return true on success, false on failure.
 *
 * @note Creates the hash table with auto-resize and initializes the "base_type" node.
 *       If already initialized, returns true with a warning. Sets urcu_safe functions.
 * @note Prerequisites: `rcu_init()` must be called by the application before this.
 * @note Call context: Any context.
 */
bool                    gd_init(void);

/**
 * @brief Cleans up the system, removing all nodes in layers to respect type dependencies.
 *
 * @note Uses a Verstable set to track types in use, then removes non-type nodes first. Processes in batches (1000 nodes) and layers.
 *       Calls `cds_lfht_destroy()` at the end (see URCU_LFHT_REFERENCE.md). Logs layer count on completion.
 *       If no progress, forces cleanup of remaining nodes.
 * @note Prerequisites: Must be called after `gd_init()` and all threads have stopped using the system.
 * @note Call context: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()` section. Calls `rcu_barrier()` internally to wait for callbacks.
 */
void                    gd_cleanup(void); // removes all nodes from the hash table

/**
 * @brief Removes all nodes from the hash table and frees the hash table.
 *
 * @note This function is declared but not implemented in the source; likely an alias or extension of gd_cleanup().
 */
void                    gd_free(void); // removes all nodes from the hash table and frees the hash table

#endif /* GLOBAL_DATA_CORE_H */