# RCU and Hash Table Function Reference

## RCU Core Functions

### `rcu_init()`
- **Purpose**: Initializes the RCU subsystem
- **Prerequisites**: None (must be first RCU function called)
- **Call context**: Any context
- **Notes**: Must be called once before any other RCU functions

### `rcu_register_thread()`
- **Purpose**: Registers current thread with RCU
- **Prerequisites**: Must be called after `rcu_init()`
- **Call context**: Any context  
- **Notes**: Must be called before any `rcu_read_lock()` in this thread. Each thread needs its own registration

### `rcu_unregister_thread()`
- **Purpose**: Unregisters current thread from RCU
- **Prerequisites**: Must be called after `rcu_register_thread()`
- **Call context**: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Must be called before thread exits

### `rcu_read_lock()`
- **Purpose**: Begins RCU read-side critical section
- **Prerequisites**: Must be called after `rcu_register_thread()`
- **Call context**: Any context
- **Notes**: Can be nested (multiple calls in same thread). Must be balanced with `rcu_read_unlock()`

### `rcu_read_unlock()`
- **Purpose**: Ends RCU read-side critical section
- **Prerequisites**: Must match a previous `rcu_read_lock()`
- **Call context**: Any context
- **Notes**: Must be called before `rcu_unregister_thread()`

### `synchronize_rcu()`
- **Purpose**: Waits for all current RCU read-side critical sections to complete
- **Prerequisites**: Must be called after `rcu_register_thread()`
- **Call context**: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Blocks calling thread until grace period completes. Can cause deadlock if called while holding mutexes that are also acquired within RCU read-side critical sections

### `call_rcu(struct rcu_head *head, void (*func)(struct rcu_head *))`
- **Purpose**: Schedules callback to run after grace period
- **Prerequisites**: Must be called after `rcu_register_thread()`
- **Call context**: Can be called inside `rcu_read_lock()`/`rcu_read_unlock()` section. `rcu_barrier()` should not be called within a callback fucntion given to `call_rcu()`
- **Notes**: Callback function runs outside of critical sections

### `rcu_barrier()`
- **Purpose**: Waits for all pending `call_rcu()` callbacks to complete
- **Prerequisites**: Must be called after `rcu_register_thread()`
- **Call context**: **Must NOT be called from within a `rcu_read_lock()`/`rcu_read_unlock()` section** (generates error: "rcu_barrier() called from within RCU read-side critical section.").  `rcu_barrier()` should not be called within a callback fucntion given to `call_rcu()`
- **Notes**: Should be called before program cleanup to ensure all pending memory deallocations complete

---

## Hash Table Functions

### Creation and Destruction

#### `cds_lfht_new(init_size, min_buckets, max_buckets, flags, attr)`
#### `cds_lfht_new_flavor(init_size, min_buckets, max_buckets, flags, flavor, attr)`
- **Purpose**: Creates new hash table
- **Prerequisites**: Must be called after `rcu_init()`
- **Call context**: No `rcu_read_lock()` required
- **Parameters**:
  - `init_size`: Initial number of buckets (must be power of 2)
  - `min_buckets`: Minimum number of buckets 
  - `max_buckets`: Maximum number of buckets (0 = unlimited)
  - `flags`: Combination of:
    - `CDS_LFHT_AUTO_RESIZE`: Enable automatic resizing
    - `CDS_LFHT_ACCOUNTING`: Enable node counting
  - `flavor`: RCU flavor (for `_flavor` variant)
  - `attr`: Custom memory allocator (can be NULL)
- **Returns**: Hash table pointer or `NULL` on failure
- **Notes**: The `_flavor` variant is preferred for explicit RCU flavor control

#### `cds_lfht_destroy(struct cds_lfht *ht, pthread_attr_t **attr)`
- **Purpose**: Destroys hash table and frees memory
- **Prerequisites**: Must be called after `rcu_register_thread()`
- **Call context**: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Should call `rcu_barrier()` before this to ensure no pending callbacks. May use worker threads for cleanup

### Node Initialization

#### `cds_lfht_node_init(struct cds_lfht_node *node)`
- **Purpose**: Initializes hash table node
- **Prerequisites**: Must be called before adding node to any hash table
- **Call context**: No RCU functions required around this
- **Notes**: Node must not be reused between hash tables without reinitialization

#### `cds_lfht_node_init_deleted(struct cds_lfht_node *node)`
- **Purpose**: Initializes node to deleted state
- **Prerequisites**: Must be called before using node with `cds_lfht_is_node_deleted()`
- **Call context**: No RCU functions required around this

### Adding Nodes

#### `cds_lfht_add(struct cds_lfht *ht, unsigned long hash, struct cds_lfht_node *node)`
- **Purpose**: Adds node to hash table (allows duplicates)
- **Prerequisites**: Must call `cds_lfht_node_init()` on node first
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Thread must be registered with `rcu_register_thread()`. Always succeeds (no return value)

#### `cds_lfht_add_unique(struct cds_lfht *ht, unsigned long hash, match_func, key, struct cds_lfht_node *node)`
- **Purpose**: Adds node only if key doesn't exist
- **Prerequisites**: Must call `cds_lfht_node_init()` on node first
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Returns**: New node if added, existing node if key already exists
- **Notes**: Thread must be registered with `rcu_register_thread()`. If existing node returned, caller's node was not added

#### `cds_lfht_add_replace(struct cds_lfht *ht, unsigned long hash, match_func, key, struct cds_lfht_node *node)`
- **Purpose**: Replaces existing node or adds new one
- **Prerequisites**: Must call `cds_lfht_node_init()` on new node first
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Returns**: Old node if replaced, `NULL` if new addition
- **Notes**: If returns old node, must use `call_rcu()` to free it later

### Replacing Nodes

#### `cds_lfht_replace(struct cds_lfht *ht, struct cds_lfht_iter *old_iter, unsigned long hash, match_func, key, struct cds_lfht_node *new_node)`
- **Purpose**: Replaces node found by iterator
- **Prerequisites**: Must call `cds_lfht_node_init()` on new node first
- **Call context**: Must be inside same `rcu_read_lock()`/`rcu_read_unlock()` section as the lookup that filled `old_iter`
- **Returns**: 0 on success, negative value on failure
- **Notes**: Must use `call_rcu()` to free old node after successful replacement. Iterator must point to a valid, non-deleted node

### Lookup and Iteration

#### `cds_lfht_lookup(struct cds_lfht *ht, unsigned long hash, match_func, key, struct cds_lfht_iter *iter)`
- **Purpose**: Finds first node matching key, stores result in iterator
- **Prerequisites**: Thread must be registered with `rcu_register_thread()`
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Use `cds_lfht_iter_get_node()` to get node from iterator. Which node is found among duplicates is unspecified

#### `cds_lfht_next_duplicate(struct cds_lfht *ht, match_func, key, struct cds_lfht_iter *iter)`
- **Purpose**: Finds next node with same key, updates iterator
- **Prerequisites**: Iterator must be initialized by `cds_lfht_lookup()` first
- **Call context**: Must be inside same `rcu_read_lock()`/`rcu_read_unlock()` section as initial lookup
- **Notes**: Use `cds_lfht_iter_get_node()` to get node from iterator

#### `cds_lfht_first(struct cds_lfht *ht, struct cds_lfht_iter *iter)`
- **Purpose**: Gets first node in table, stores in iterator
- **Prerequisites**: Thread must be registered with `rcu_register_thread()`
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Use `cds_lfht_iter_get_node()` to get node from iterator. Order is unspecified

#### `cds_lfht_next(struct cds_lfht *ht, struct cds_lfht_iter *iter)`
- **Purpose**: Gets next node in table, updates iterator
- **Prerequisites**: Iterator must be initialized by `cds_lfht_first()` or previous `cds_lfht_next()`
- **Call context**: Must be inside same `rcu_read_lock()`/`rcu_read_unlock()` section as `cds_lfht_first()`
- **Notes**: Use `cds_lfht_iter_get_node()` to get node from iterator

### Iteration Macros

#### `cds_lfht_for_each(struct cds_lfht *ht, struct cds_lfht_iter *iter, struct cds_lfht_node *node)`
- **Purpose**: Iterates over all nodes in hash table
- **Prerequisites**: Thread must be registered with `rcu_register_thread()`
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Macro that expands to a for loop. Iteration order is unspecified

#### `cds_lfht_for_each_entry(struct cds_lfht *ht, struct cds_lfht_iter *iter, type, member)`
- **Purpose**: Iterates over all container structures in hash table
- **Prerequisites**: Thread must be registered with `rcu_register_thread()`
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Parameters**:
  - `type`: Container structure type
  - `member`: Name of `cds_lfht_node` member in container
- **Notes**: Macro that expands to a for loop. More convenient than `cds_lfht_for_each`

### Deletion and Status

#### `cds_lfht_del(struct cds_lfht *ht, struct cds_lfht_node *node)`
- **Purpose**: Removes node from hash table
- **Prerequisites**: Node must have been found by lookup in same critical section
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Returns**: 0 on success, negative value if node already deleted
- **Notes**: Must use `call_rcu()` to free node after successful deletion. Node becomes logically deleted but physically present

#### `cds_lfht_is_node_deleted(struct cds_lfht_node *node)`
- **Purpose**: Checks if node has been deleted
- **Prerequisites**: Node must have been found by lookup in same critical section
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Returns**: Non-zero if deleted, 0 otherwise

### Utility Functions

#### `cds_lfht_iter_get_node(struct cds_lfht_iter *iter)`
- **Purpose**: Extracts node pointer from iterator
- **Prerequisites**: Iterator must be filled by lookup/iteration functions first
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Returns**: Node pointer or `NULL` if iterator contains no node

#### `cds_lfht_resize(struct cds_lfht *ht, unsigned long new_size)`
- **Purpose**: Forces hash table resize to specified size
- **Prerequisites**: Must be called after `rcu_register_thread()`
- **Call context**: Must NOT be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: May trigger worker thread activity

#### `cds_lfht_count_nodes(struct cds_lfht *ht, long *before, unsigned long *count, long *after)`
- **Purpose**: Counts nodes in hash table (approximation)
- **Prerequisites**: Thread must be registered with `rcu_register_thread()`
- **Call context**: Must be inside `rcu_read_lock()`/`rcu_read_unlock()` section
- **Notes**: Results are approximate due to concurrent operations. `before`/`after` indicate nodes added/removed during counting

---

## Critical Ordering Rules

1. **Program startup**: `rcu_init()` → `rcu_register_thread()` → hash table operations
2. **Read operations**: `rcu_read_lock()` → lookup/iteration → `rcu_read_unlock()`
3. **Write operations**: `rcu_read_lock()` → add/delete/replace → `rcu_read_unlock()` → `call_rcu()` (for deletions)
4. **Thread cleanup**: finish all operations → `rcu_read_unlock()` → `rcu_unregister_thread()`
5. **Program cleanup**: `rcu_barrier()` → `cds_lfht_destroy()` → `rcu_unregister_thread()`

## Important Restrictions and Warnings

### RCU Read-Side Critical Section Restrictions
- **NEVER** call `rcu_barrier()` from within `rcu_read_lock()`/`rcu_read_unlock()`
- **NEVER** call `synchronize_rcu()` from within `rcu_read_lock()`/`rcu_read_unlock()`
- **NEVER** call `rcu_unregister_thread()` from within `rcu_read_lock()`/`rcu_read_unlock()`
- **NEVER** call `cds_lfht_destroy()` from within `rcu_read_lock()`/`rcu_read_unlock()`
- **NEVER** call `cds_lfht_resize()` from within `rcu_read_lock()`/`rcu_read_unlock()`

### Iterator Safety
- Iterators are only valid within the RCU read-side critical section where they were created
- **NEVER** reuse iterators across different hash tables
- **NEVER** use iterators after their originating RCU read-side critical section ends
- Iterator debugging can be enabled with `--enable-cds-lfht-iter-debug` (changes ABI)

### Memory Management
- Always use `call_rcu()` to defer freeing deleted nodes
- Call `rcu_barrier()` before program exit to ensure all callbacks complete
- Hash table destruction may use worker threads; ensure proper cleanup

### Deadlock Prevention
- Avoid holding mutexes when calling `synchronize_rcu()` if those mutexes are also acquired within RCU read-side critical sections
- This applies especially to QSBR flavor where threads are "online" by default

## Common Call Patterns

### Safe Lookup Pattern
```c
rcu_read_lock();
cds_lfht_lookup(ht, hash, match, key, &iter);
node = cds_lfht_iter_get_node(&iter);
if (node) {
    // Use node data here
    struct mydata *data = caa_container_of(node, struct mydata, ht_node);
    // Process data...
}
rcu_read_unlock();
```

### Safe Deletion Pattern
```c
rcu_read_lock();
cds_lfht_lookup(ht, hash, match, key, &iter);
node = cds_lfht_iter_get_node(&iter);
if (node) {
    ret = cds_lfht_del(ht, node);
}
rcu_read_unlock();
if (node && ret == 0) {
    call_rcu(&my_data->rcu_head, free_callback);
}
```

### Safe Iteration Pattern
```c
rcu_read_lock();
cds_lfht_for_each_entry(ht, &iter, my_struct, ht_node) {
    // Process each node
    if (!cds_lfht_is_node_deleted(&my_struct->ht_node)) {
        // Use my_struct...
    }
}
rcu_read_unlock();
```

### Safe Hash Table Creation
```c
// With explicit flavor
ht = cds_lfht_new_flavor(1, 1, 0,
    CDS_LFHT_AUTO_RESIZE | CDS_LFHT_ACCOUNTING,
    &urcu_memb_flavor, NULL);

// Or with default flavor
ht = cds_lfht_new(1, 1, 0, CDS_LFHT_AUTO_RESIZE, NULL);
```

### Program Cleanup Sequence
```c
// Stop adding new nodes...

// Wait for all call_rcu() callbacks to complete
rcu_barrier();

// Destroy hash table (may trigger worker threads)
cds_lfht_destroy(ht, NULL);

// Unregister thread
rcu_unregister_thread();
```

## Debugging and Development

### Build-Time Options
- `--enable-cds-lfht-iter-debug`: Enables iterator validation (changes ABI)
- `--enable-rcu-debug`: Enables internal RCU debugging (performance penalty)
- `DEBUG_YIELD`: Adds random delays for testing race conditions

### Common Errors
- "rcu_barrier() called from within RCU read-side critical section" → Move `rcu_barrier()` outside critical section
- Iterator reuse across hash tables → Use separate iterators or reinitialize
- Memory leaks after deletion → Ensure `call_rcu()` is used for cleanup
- Deadlocks with mutexes → Review mutex acquisition patterns vs. RCU critical sections

## Performance Considerations

- **QSBR flavor**: Fastest read-side performance, requires explicit quiescent state management
- **Memory barrier flavor**: Good general purpose choice
- **Signal flavor**: Moderate performance, requires dedicated signal
- **Bulletproof flavor**: Slowest but requires minimal application changes
- Auto-resize can cause temporary performance impacts during resize operations
- Node counting (accounting) adds small overhead but enables `cds_lfht_count_nodes()`