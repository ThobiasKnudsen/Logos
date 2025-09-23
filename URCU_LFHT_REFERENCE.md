# RCU and Hash Table Function Reference

## RCU Core Functions

### `rcu_init()`  
**Purpose**: Initializes the RCU subsystem.  
**Usage Context**: Must be called once before any other RCU functions. No prerequisites beyond being the first RCU function invoked. Can be called from any context.  
**Notes**: Essential for setting up the RCU environment. In multi-threaded applications, typically called early in the main thread

### `rcu_register_thread()`  
**Purpose**: Registers the current thread with RCU.  
**Usage Context**: Must be called after `rcu_init()`. Required before any `rcu_read_lock()` in the thread. Can be called from any context. Each thread must register individually. In QSBR flavor, threads are online by default after registration.  
**Notes**: Not needed for threads that never enter RCU read-side critical sections. For "bulletproof" RCU (rcu-bp), registration is automatic

### `rcu_unregister_thread()`  
**Purpose**: Unregisters the current thread from RCU.  
**Usage Context**: Must be called after `rcu_register_thread()`. Must be called before thread exits. Must not be called inside an RCU critical section. In QSBR flavor, thread must be online before unregistering.  
**Notes**: Ensures proper cleanup. Failure to unregister can lead to resource leaks.  

### `rcu_read_lock()`  
**Purpose**: Begins an RCU read-side critical section.  
**Usage Context**: Must be called after `rcu_register_thread()`. Can be called from any context and nested. In QSBR flavor, must be called while thread is online.   
**Notes**: Must be balanced with `rcu_read_unlock()`. Protects data from concurrent updates. Acts as a reader-side primitive; does not block writers.   

### `rcu_read_unlock()`   
**Purpose**: Ends an RCU read-side critical section.   
**Usage Context**: Must match a previous `rcu_read_lock()`. Can be called from any context. In QSBR flavor, must be called while thread is online.   
**Notes**: Must be called before `rcu_unregister_thread()`. Cannot call `rcu_quiescent_state()` inside critical section (QSBR).   

### `synchronize_rcu()`   
**Purpose**: Waits for all pre-existing RCU read-side critical sections to complete.   
**Usage Context**: Must be called after `rcu_register_thread()`. Must NOT be called inside an RCU critical section (causes deadlock).   
**Notes**: Used for synchronization after updates. Can cause deadlock if called while holding mutexes acquired within RCU read-side critical sections. In QSBR, requires periodic quiescent state reporting.   

### `call_rcu()`   
**Purpose**: Schedules a callback to run after a grace period.   
**Signature**: `call_rcu(struct rcu_head *head, void (*func)(struct rcu_head *head))`   
**Usage Context**: Must be called after `rcu_register_thread()`. Can be called inside an RCU critical section. Should not call `rcu_barrier()` within the callback. For QSBR flavor, caller should be online.   
**Notes**: Callback runs outside critical sections, typically to free memory. The `rcu_head` is embedded in the structure to be freed. Ensures safe deferred reclamation.   

### `rcu_barrier()`  
**Purpose**: Waits for all pending `call_rcu()` callbacks to complete.  
**Usage Context**: Must be called after `rcu_register_thread()`. Must NOT be called inside an RCU critical section. Should not be called within a `call_rcu()` callback.  
**Notes**: Useful before program cleanup.  Ensures all deferred operations complete. May invoke `synchronize_rcu()` internally.   

### `rcu_quiescent_state()`   
**Purpose**: Reports a momentary quiescent state for the current thread.   
**Usage Context**: Must be called after `rcu_register_thread()`. Must NOT be called inside an RCU critical section. Must be called while thread is online (QSBR).   
**Notes**: Essential for QSBR flavor to advance grace periods. No-op in other flavors. Failure to call regularly in QSBR can stall `synchronize_rcu()`.   

### `rcu_thread_offline()`   
**Purpose**: Begins an extended quiescent state for the current thread.   
**Usage Context**: Must be called after `rcu_register_thread()`. Must NOT be called inside an RCU critical section.   
**Notes**: QSBR flavor only (no-op in others). For long inactivity periods (e.g., sleep/I/O). Pair with `rcu_thread_online()`. Cannot use RCU read functions while offline.   

### `rcu_thread_online()`   
**Purpose**: Ends an extended quiescent state for the current thread.   
**Usage Context**: Must follow a previous `rcu_thread_offline()`. Can be called from any context.   
**Notes**: QSBR flavor only (no-op in others). Resumes normal RCU operations. If used in signal handlers, disable signals around calls.   

---

## Hash Table Functions

### Creation and Destruction

#### `cds_lfht_new()` / `cds_lfht_new_flavor()`

**Purpose**: Creates a new lock-free RCU hash table.

**Signatures**:
```c
struct cds_lfht *cds_lfht_new(
    unsigned long init_size,
    unsigned long min_nr_alloc_buckets,
    unsigned long max_nr_buckets,
    int flags,
    pthread_attr_t *attr
);

struct cds_lfht *cds_lfht_new_flavor(
    unsigned long init_size,
    unsigned long min_nr_alloc_buckets,
    unsigned long max_nr_buckets,
    int flags,
    const struct rcu_flavor_struct *flavor,
    pthread_attr_t *attr
);
```

**Parameters**:
- `init_size`: Initial number of buckets (must be power of 2)
- `min_nr_alloc_buckets`: Minimum allocated buckets (must be power of 2)
- `max_nr_buckets`: Maximum buckets (0 for unlimited, must be power of 2)
- `flags`: Bitwise OR of:
  - `CDS_LFHT_AUTO_RESIZE`: Enable automatic resizing
  - `CDS_LFHT_ACCOUNTING`: Enable node counting
- `flavor`: RCU flavor struct (for `_flavor` variant)
- `attr`: Optional pthread attributes for resize worker thread

**Usage Context**: 
- Must be called after `rcu_init()`
- No `rcu_read_lock()` required
- Can be called before thread registration

**Returns**: Pointer to `struct cds_lfht` on success, `NULL` on failure

#### `cds_lfht_destroy()`

**Purpose**: Destroys the hash table and frees its memory.

**Signature**: 
```c
int cds_lfht_destroy(struct cds_lfht *ht, pthread_attr_t **attr)
```

**Usage Context**: 
- Must be called after `rcu_register_thread()`
- Should call `rcu_barrier()` beforehand
- Since liburcu 0.10, can be called from RCU critical sections

**Returns**: 0 on success, negative error on failure

### Node Initialization

#### `cds_lfht_node_init()`

**Purpose**: Initializes a hash table node.

**Usage Context**: 
- Must be called before adding node to any hash table
- No RCU functions required

**Notes**: 
- Node must not be reused without reinitialization
- Node should be aligned on 8-byte boundaries

#### `cds_lfht_node_init_deleted()`

**Purpose**: Initializes a node to the "deleted" state.

**Usage Context**: 
- Must be called before using with `cds_lfht_is_node_deleted()`
- No RCU functions required

### Adding Nodes

#### `cds_lfht_add()`

**Purpose**: Adds a node to the hash table (allows duplicates).

**Usage Context**: 
- Must be inside an RCU critical section
- Thread must be registered
- Node must be initialized first

**Notes**: 
- Always succeeds (no return value)
- Issues full memory barriers before/after

#### `cds_lfht_add_unique()`

**Purpose**: Adds a node only if the key does not exist.

**Usage Context**: 
- Must be inside an RCU critical section
- Thread must be registered
- Node must be initialized first

**Returns**: 
- Added node on success
- Existing node if key already present

**Notes**: Ensures no duplicates if used exclusively for additions

#### `cds_lfht_add_replace()`

**Purpose**: Replaces an existing node with same key or adds if absent.

**Usage Context**: 
- Must be inside an RCU critical section
- Thread must be registered
- New node must be initialized first

**Returns**: 
- Old node if replaced (defer free with `call_rcu()`)
- `NULL` if new addition

### Replacing Nodes

#### `cds_lfht_replace()`

**Purpose**: Replaces a node pointed to by the iterator.

**Usage Context**: 
- Must be inside same RCU critical section as lookup
- Thread must be registered
- New node must be initialized first

**Returns**: 
- 0 on success
- Negative on failure (-ENOENT, -EINVAL)

### Lookup and Iteration

#### `cds_lfht_lookup()`

**Purpose**: Finds the first node matching the key.

**Usage Context**: 
- Must be inside an RCU critical section
- Thread must be registered

**Notes**: 
- Use `cds_lfht_iter_get_node()` to extract node
- Acts as `rcu_dereference()`

#### `cds_lfht_next_duplicate()`

**Purpose**: Finds the next node with the same key.

**Usage Context**: 
- Must be inside same RCU critical section as initial lookup
- Iterator must be initialized by `cds_lfht_lookup()`

#### `cds_lfht_first()`

**Purpose**: Gets the first node in the table.

**Usage Context**: 
- Must be inside an RCU critical section
- Thread must be registered

#### `cds_lfht_next()`

**Purpose**: Gets the next node in the table.

**Usage Context**: 
- Must be inside same RCU critical section as `cds_lfht_first()`
- Iterator must be initialized

### Iteration Macros

#### `cds_lfht_for_each()`

**Purpose**: Iterates over all nodes in the hash table.

**Syntax**: 
```c
cds_lfht_for_each(ht, iter, node)
```

**Usage Context**: Must be inside an RCU critical section

#### `cds_lfht_for_each_duplicate()`

**Purpose**: Iterates over all nodes with the same key.

**Syntax**: 
```c
cds_lfht_for_each_duplicate(ht, hash, match, key, iter, node)
```

#### `cds_lfht_for_each_entry()`

**Purpose**: Iterates over all container structures in the hash table.

**Syntax**: 
```c
cds_lfht_for_each_entry(ht, iter, pos, member)
```

**Parameters**:
- `pos`: Pointer to container type
- `member`: Name of `cds_lfht_node` member in container

#### `cds_lfht_for_each_entry_duplicate()`

**Purpose**: Iterates over all container structures with the same key.

**Syntax**: 
```c
cds_lfht_for_each_entry_duplicate(ht, hash, match, key, iter, pos, member)
```

### Deletion and Status

#### `cds_lfht_del()`

**Purpose**: Removes a node from the hash table.

**Usage Context**: 
- Must be inside an RCU critical section
- Node must be found by lookup in same section
- Thread must be registered

**Returns**: 
- 0 on success
- Negative if node already deleted or NULL

**Notes**: 
- Marks node as logically deleted
- On success, defer free with `call_rcu()`

#### `cds_lfht_is_node_deleted()`

**Purpose**: Checks if a node is deleted.

**Usage Context**: 
- Must be inside an RCU critical section
- Node must be found by lookup in same section

**Returns**: Non-zero if deleted, 0 otherwise

### Utility Functions

#### `cds_lfht_iter_get_node()`

**Purpose**: Extracts the node pointer from an iterator.

**Usage Context**: 
- Must be inside an RCU critical section
- Iterator must be filled by lookup/iteration

**Returns**: Node pointer or `NULL`

#### `cds_lfht_resize()`

**Purpose**: Forces a hash table resize to specified size.

**Usage Context**: 
- Must be called after `rcu_register_thread()`
- Must NOT be called inside an RCU critical section

**Notes**: 
- May trigger worker threads
- `new_size` must be power of 2

#### `cds_lfht_count_nodes()`

**Purpose**: Counts nodes in the hash table (approximate).

**Usage Context**: 
- Must be inside an RCU critical section
- Thread must be registered
- Requires `CDS_LFHT_ACCOUNTING` flag

---

## Critical Ordering Rules

1. **Program startup**: 
   ```
   rcu_init() → rcu_register_thread() → hash table operations
   ```

2. **Read operations**: 
   ```
   rcu_read_lock() → lookup/iteration → rcu_read_unlock()
   ```

3. **Write operations**: 
   ```
   rcu_read_lock() → add/delete/replace → rcu_read_unlock() → call_rcu()
   ```

4. **Thread cleanup**: 
   ```
   finish operations → rcu_read_unlock() → rcu_unregister_thread()
   ```

5. **Program cleanup**: 
   ```
   rcu_barrier() → cds_lfht_destroy() → rcu_unregister_thread()
   ```

---

## Important Restrictions and Warnings

### RCU Read-Side Critical Section Restrictions

**NEVER call these from within `rcu_read_lock()`/`rcu_read_unlock()`:**
- `rcu_barrier()`
- `synchronize_rcu()`
- `rcu_unregister_thread()`
- `cds_lfht_resize()`
- `cds_lfht_destroy()` (allowed since liburcu 0.10, but avoid)

### Iterator Safety

- Iterators are valid only within originating RCU critical section
- Never reuse iterators across different hash tables
- Never use iterators after their RCU critical section ends
- Enable debugging with `--enable-cds-lfht-iter-debug`

### Memory Management

- Always defer freeing deleted/replaced nodes with `call_rcu()`
- Call `rcu_barrier()` before program exit
- Hash table destruction may use worker threads

### Deadlock Prevention

- Avoid holding mutexes in `synchronize_rcu()` if acquired in RCU read-side sections
- In QSBR, manage quiescence to avoid stalls

### Additional RCU APIs

- **Non-blocking grace period checks**: 
  - `start_poll_synchronize_rcu()`
  - `poll_state_synchronize_rcu()`
  
- **Custom `call_rcu` threads**:
  - `create_call_rcu_data()`
  - `get_call_rcu_data()`
  
- **Fork handling**:
  - `call_rcu_before_fork_parent()`

---

## Common Call Patterns

### Safe Lookup Pattern

```c
rcu_read_lock();
cds_lfht_lookup(ht, hash, match, key, &iter);
node = cds_lfht_iter_get_node(&iter);
if (node) {
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
if (node && cds_lfht_del(ht, node) == 0) {
    // Node deleted successfully
}
rcu_read_unlock();
if (node) {
    call_rcu(&container_of(node, struct mydata, ht_node)->rcu_head, 
             free_callback);
}
```

### Safe Iteration Pattern

```c
rcu_read_lock();
cds_lfht_for_each_entry(ht, &iter, my_struct, ht_node) {
    if (!cds_lfht_is_node_deleted(&my_struct->ht_node)) {
        // Process my_struct...
    }
}
rcu_read_unlock();
```

### Safe Hash Table Creation

```c
// With explicit flavor
ht = cds_lfht_new_flavor(1 << 10, 1 << 10, 0,
    CDS_LFHT_AUTO_RESIZE | CDS_LFHT_ACCOUNTING,
    &rcu_flavor, NULL);

// Or with default flavor
ht = cds_lfht_new(1 << 10, 1 << 10, 0,
    CDS_LFHT_AUTO_RESIZE | CDS_LFHT_ACCOUNTING, NULL);
```

### Program Cleanup Sequence

```c
// Stop adding new nodes...

// Wait for all call_rcu() callbacks
rcu_barrier();

// Destroy hash table
cds_lfht_destroy(ht, NULL);

// Unregister thread
rcu_unregister_thread();
```

---

## Debugging and Development

### Build-Time Options

- `--enable-cds-lfht-iter-debug`: Enables iterator validation (changes ABI)
- `--enable-rcu-debug`: Enables internal RCU debugging (performance penalty)
- `DEBUG_YIELD`: Adds random delays for race condition testing

### Common Errors

| Error | Solution |
|-------|----------|
| "rcu_barrier() called from within RCU read-side critical section" | Move `rcu_barrier()` outside critical section |
| Iterator reuse across tables | Use separate iterators |
| Memory leaks post-deletion | Ensure `call_rcu()` for cleanup |
| Deadlocks with mutexes | Avoid mutexes in grace period waits |
| Stalls in QSBR | Call `rcu_quiescent_state()` periodically |

### Performance Considerations

- **QSBR flavor**: Fastest reads; requires explicit quiescence management
- **Memory barrier flavor**: Balanced; good for general use
- **Signal flavor**: Moderate; needs dedicated signal
- **Bulletproof flavor**: Easiest but slowest; minimal app changes
- **Auto-resize**: May cause temporary slowdowns
- **Accounting**: Adds minor overhead but enables counting
- **Long bucket chains**: Possible if resizes starved; prioritize workers appropriately