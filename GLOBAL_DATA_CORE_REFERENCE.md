# Global Data Core Reference

## Overview

The Global Data Core provides a thread-safe, RCU-protected hash table for storing arbitrary data structures. It builds upon the Userspace RCU (URCU) library's lock-free hash table (LFHT) implementation, adding a type system, key management (numeric or string keys), and safe memory handling.

Key features:
- Supports both numeric (uint64_t) and string keys via `union gd_key`.
- Every node derives from `struct gd_base_node` and has a type referenced by `type_key`, which points to a `struct gd_base_type_node`.
- The system requires a special "base_type" node as the root type and first node because all nodes need a type and "base_type" has itself as type. The type is a set of functions which can be used on a node. by default every type must inherit from `gd_base_type_node` by setting it as the first field. 
- All read operations must be protected by RCU read-side critical sections (`rcu_read_lock()` / `rcu_read_unlock()`).
- Write operations (insert/remove/update) are RCU-safe and use `call_rcu()` for deferred freeing.
- Cleanup handles type dependencies by removing nodes in layers (instances before types).

This reference explains all structures, unions, and functions from `global_data/core.h`. Since the system depends on URCU LFHT, relevant URCU concepts are integrated here (cross-referenced from URCU_LFHT_REFERENCE.md where applicable). For full URCU details, consult URCU_LFHT_REFERENCE.md.

## Core Structures and Unions

### `union gd_key`
- **Purpose**: Represents a key that can be either a 64-bit number or a string.
- **Fields**:
  - `uint64_t number`: Used when `key_is_number` is `true`. Must not be 0 (0 is invalid).
  - `char* string`: Used when `key_is_number` is `false`. Must be null-terminated and allocated via `gd_key_create()` (system copies strings).
- **Notes**: Always pair with a `bool key_is_number` flag. String keys are limited to 63 characters (MAX_STRING_KEY_LEN - 1). Use helper functions like `gd_key_create()` and `gd_key_free()` for management. Keys are zero-copy in nodes, so manage memory carefully.

### `struct gd_base_node`
- **Purpose**: Base structure for all nodes stored in the hash table. All custom node types must embed this as their first member.
- **Fields**:
  - `struct cds_lfht_node lfht_node`: Internal URCU LFHT node (do not access directly; see URCU_LFHT_REFERENCE.md for details).
  - `union gd_key key`: The node's key.
  - `union gd_key type_key`: Key referencing the node's type (must point to a `struct gd_base_type_node`).
  - `bool key_is_number`: Flag for `key` type.
  - `bool type_key_is_number`: Flag for `type_key` type.
  - `uint32_t size_bytes`: Total size of the node (must be >= `sizeof(struct gd_base_node)`).
  - `struct rcu_head rcu_head`: Internal RCU head for deferred freeing (do not access directly; see URCU_LFHT_REFERENCE.md).
- **Notes**: Create via `gd_base_node_create()`. Nodes are zero-initialized except for user-filled fields. For custom types, define a struct like `struct my_node { struct gd_base_node base; /* custom fields */; };`.

### `struct gd_base_type_node`
- **Purpose**: Represents a node type, embedding `struct gd_base_node`. All nodes' `type_key` must reference a type node.
- **Fields**:
  - `struct gd_base_node base`: Embedded base node (its `type_key` points to itself or another type).
  - `bool (*fn_free_node)(struct gd_base_node*)`: Function to free a node of this type (returns `true` on success).
  - `void (*fn_free_node_callback)(struct rcu_head*)`: RCU callback wrapper that calls `fn_free_node()` (used with `call_rcu()`).
  - `bool (*fn_is_valid)(struct gd_base_node*)`: Validation function for nodes of this type.
  - `uint32_t type_size`: Expected size of nodes of this type (for validation during insert/update).
- **Notes**: The system initializes a root "base_type" node with key `{ .string = "base_type" }` (string key). Custom types should derive from this. Free functions must handle key freeing via `gd_key_free()`. Validation should check `size_bytes` and other invariants.

## Initialization and Cleanup

### `gd_init()`
- **Purpose**: Initializes the global data system, including URCU and the hash table.
- **Prerequisites**: None (must be first function called).
- **Call context**: Any context.
- **Returns**: `true` on success, `false` on failure.
- **Notes**: Calls `rcu_init()` internally (see URCU_LFHT_REFERENCE.md). Creates the hash table with auto-resize and initializes the "base_type" node. If already initialized, returns `true` with a warning.

### `gd_cleanup()`
- **Purpose**: Cleans up the system, removing all nodes in layers to respect type dependencies.
- **Prerequisites**: Must be called after `gd_init()` and all threads have stopped using the system.
- **Call context**: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()` section. Call `rcu_barrier()` internally to wait for callbacks.
- **Notes**: Uses a tracking set to identify types in use, then removes non-type nodes first. Processes in batches (1000 nodes) and layers. Calls `cds_lfht_destroy()` at the end (see URCU_LFHT_REFERENCE.md). Logs layer count on completion.

## Node Creation and Management

### `gd_base_node_create(union gd_key key, bool key_is_number, union gd_key type_key, bool type_key_is_number, uint32_t size_bytes)`
- **Purpose**: Allocates and initializes a new base node (zero-initialized).
- **Prerequisites**: Keys must be created via `gd_key_create()`. `type_key` must reference a valid type.
- **Call context**: Any context (no RCU lock needed).
- **Parameters**:
  - `key`: Node key (if numeric and 0, auto-assigns a unique ID from atomic counter).
  - `key_is_number`: Flag for `key`.
  - `type_key`: Type reference.
  - `type_key_is_number`: Flag for `type_key`.
  - `size_bytes`: Total node size (>= `sizeof(struct gd_base_node)`).
- **Returns**: Pointer to new node or `NULL` on error.
- **Notes**: Copies string keys (zero-copy for callers). Initializes LFHT node (see `cds_lfht_node_init()` in URCU_LFHT_REFERENCE.md). User must fill custom fields before inserting.

### `gd_base_node_free(struct gd_base_node* p_base_node)`
- **Purpose**: Frees a node (should only be called from RCU callbacks).
- **Prerequisites**: Node must not be in the hash table.
- **Call context**: From `call_rcu()` callback (outside RCU read sections).
- **Returns**: `true` on success, `false` if NULL.
- **Notes**: Frees keys via `gd_key_free()`. For custom types, use the type's `fn_free_node()`.

## Node Operations

All operations require `rcu_register_thread()` per thread (see URCU_LFHT_REFERENCE.md).

### `gd_node_get(union gd_key key, bool key_is_number)`
- **Purpose**: Retrieves a node by key (read-only).
- **Prerequisites**: System initialized.
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
- **Returns**: Node pointer or `NULL` if not found.
- **Notes**: Uses `cds_lfht_lookup()` internally (see URCU_LFHT_REFERENCE.md). Do not modify the node unless using RCU-safe assignments (`rcu_assign_pointer()`).

### `gd_node_insert(struct gd_base_node* new_node)`
- **Purpose**: Inserts a new node (fails if key exists).
- **Prerequisites**: Node created via `gd_base_node_create()`. Type must exist.
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
- **Returns**: `true` on success, `false` if key exists or error.
- **Notes**: Validates type existence and uses `cds_lfht_add_unique()` (see URCU_LFHT_REFERENCE.md).

### `gd_node_remove(union gd_key key, bool key_is_number)`
- **Purpose**: Removes a node by key.
- **Prerequisites**: Node exists.
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
- **Returns**: `true` on success, `false` if not found.
- **Notes**: Uses `cds_lfht_del()` and schedules `call_rcu()` with the type's free callback (see URCU_LFHT_REFERENCE.md).

### `gd_node_update(struct gd_base_node* new_node)`
- **Purpose**: Replaces an existing node with a new one (same key/type/size).
- **Prerequisites**: Existing node with matching key.
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
- **Returns**: `true` on success, `false` if not found or size mismatch.
- **Notes**: Uses `cds_lfht_replace()` and `call_rcu()` for old node (see URCU_LFHT_REFERENCE.md).

### `gd_node_upsert(struct gd_base_node* new_node)`
- **Purpose**: Inserts if key doesn't exist, updates if it does.
- **Prerequisites**: Same as insert/update.
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section.
- **Returns**: `true` on success.
- **Notes**: Combines `gd_node_get()` with insert or update.

## Iteration

### `gd_iter_first(struct cds_lfht_iter* iter)`
- **Purpose**: Initializes iterator to first node.
- **Call context**: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
- **Returns**: `true` if node found.

### `gd_iter_next(struct cds_lfht_iter* iter)`
- **Purpose**: Advances to next node.
- **Call context**: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
- **Returns**: `true` if node found.

### `gd_iter_get_node(struct cds_lfht_iter* iter)`
- **Purpose**: Gets node from iterator.
- **Returns**: Node or `NULL`.

### `gd_iter_lookup(union gd_key key, bool key_is_number, struct cds_lfht_iter* iter)`
- **Purpose**: Looks up and sets iterator.
- **Call context**: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
- **Returns**: `true` if found.

**Notes**: Wrappers around URCU LFHT iterators (see URCU_LFHT_REFERENCE.md). Iteration order unspecified. Check `gd_node_is_deleted()` during iteration.

## Utility Functions

### `gd_node_is_deleted(struct gd_base_node* node)`
- **Purpose**: Checks if node is logically deleted.
- **Call context**: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
- **Returns**: `true` if deleted.

### `gd_nodes_count()`
- **Purpose**: Approximate node count.
- **Call context**: Inside `rcu_read_lock()`/`rcu_read_unlock()`.
- **Returns**: Count (uses `cds_lfht_count_nodes()`).

## Key Management

### `gd_key_create(uint64_t number_key, const char* string_key, bool key_is_number)`
- **Purpose**: Creates a key (copies strings).
- **Returns**: Valid key or invalid on error.

### `gd_key_free(union gd_key key, bool key_is_number)`
- **Purpose**: Frees string keys (no-op for numbers).
- **Returns**: `true` on success.

### `gd_key_get_number(union gd_key key, bool key_is_number)`
- **Purpose**: Gets number value (error if string).
- **Returns**: Number.

### `gd_key_get_string(union gd_key key, bool key_is_number)`
- **Purpose**: Gets string value (error if number).
- **Returns**: String.

### `gd_base_type_get_key_copy()`
- **Purpose**: Copies the "base_type" key.
- **Returns**: Key copy.

### `gd_base_type_key_is_number()`
- **Purpose**: Checks if "base_type" key is number.
- **Returns**: `false` (it's a string).

## RCU Integration and Rules

The system inherits URCU LFHT rules (see URCU_LFHT_REFERENCE.md for full details):
- **Thread Registration**: Call `rcu_register_thread()` per thread before operations; `rcu_unregister_thread()` before exit.
- **Read Safety**: All gets/iterations must be in `rcu_read_lock()` / `rcu_read_unlock()`.
- **Write Safety**: Inserts/removes use RCU mechanisms; free via `call_rcu()`.
- **Restrictions**: Never call `gd_cleanup()` or `rcu_barrier()` inside read sections. Avoid deadlocks with mutexes.
- **Cleanup**: Use `gd_cleanup()` at program end to handle layered removal.

## Common Patterns

### Basic Usage
```c
gd_init();
rcu_register_thread();

// Create and insert
union gd_key type_key = gd_base_type_get_key_copy();
struct gd_base_node* node = gd_base_node_create(gd_key_create(42, NULL, true), true, type_key, false, sizeof(struct gd_base_node));
rcu_read_lock();
gd_node_insert(node);
rcu_read_unlock();

// Get
rcu_read_lock();
struct gd_base_node* found = gd_node_get(gd_key_create(42, NULL, true), true);
rcu_read_unlock();

// Cleanup
rcu_unregister_thread();
gd_cleanup();
```

### Custom Type
Define type node with custom free/validate, insert it, then use its key for instances.

For errors, check logs (tklog). Test with `_gd_get_hash_table()` (internal).