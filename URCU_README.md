Function Reference
RCU Core Functions
rcu_init()

Initializes the RCU subsystem
Must be called once before any other RCU functions
No other RCU functions required before this

rcu_register_thread()

Registers current thread with RCU
Must be called after rcu_init()
Must be called before any rcu_read_lock() in this thread
Each thread needs its own registration

rcu_unregister_thread()

Unregisters current thread from RCU
Must be called after rcu_register_thread()
Must be called before thread exits
Must not be inside rcu_read_lock()/rcu_read_unlock() section

rcu_read_lock()

Begins RCU read-side critical section
Must be called after rcu_register_thread()
Can be nested (multiple calls in same thread)
Must be balanced with rcu_read_unlock()

rcu_read_unlock()

Ends RCU read-side critical section
Must match a previous rcu_read_lock()
Must be called before rcu_unregister_thread()

synchronize_rcu()

Waits for all current RCU read-side critical sections to complete
Must be called after rcu_register_thread()
Must not be called inside rcu_read_lock()/rcu_read_unlock() section
Blocks calling thread until grace period completes

call_rcu(struct rcu_head *head, void (*func)(struct rcu_head *))

Schedules callback to run after grace period
Must be called after rcu_register_thread()
Can be called inside rcu_read_lock()/rcu_read_unlock() section
Callback function runs outside of critical sections

rcu_barrier()

Waits for all pending call_rcu() callbacks to complete
Must be called after rcu_register_thread()
Must not be called from within a call_rcu() callback
Should be called before program cleanup

Hash Table Functions
cds_lfht_new(init_size, min_buckets, max_buckets, flags, attr)

Creates new hash table
Must be called after rcu_init()
No rcu_read_lock() required
Returns NULL on failure

cds_lfht_destroy(struct cds_lfht *ht, pthread_attr_t **attr)

Destroys hash table and frees memory
Must be called after rcu_register_thread()
Must not be called inside rcu_read_lock()/rcu_read_unlock() section
Should call rcu_barrier() before this to ensure no pending callbacks

cds_lfht_node_init(struct cds_lfht_node *node)

Initializes hash table node
Must be called before adding node to any hash table
No RCU functions required around this

cds_lfht_node_init_deleted(struct cds_lfht_node *node)

Initializes node to deleted state
Must be called before using node with cds_lfht_is_node_deleted()
No RCU functions required around this

cds_lfht_add(struct cds_lfht *ht, unsigned long hash, struct cds_lfht_node *node)

Adds node to hash table (allows duplicates)
Must call cds_lfht_node_init() on node first
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Thread must be registered with rcu_register_thread()

cds_lfht_add_unique(struct cds_lfht *ht, unsigned long hash, match_func, key, struct cds_lfht_node *node)

Adds node only if key doesn't exist, returns existing node if found
Must call cds_lfht_node_init() on node first
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Thread must be registered with rcu_register_thread()

cds_lfht_add_replace(struct cds_lfht *ht, unsigned long hash, match_func, key, struct cds_lfht_node *node)

Replaces existing node or adds new one, returns old node if replaced
Must call cds_lfht_node_init() on new node first
Must be called inside rcu_read_lock()/rcu_read_unlock() section
If returns old node, must use call_rcu() to free it later

cds_lfht_replace(struct cds_lfht *ht, struct cds_lfht_iter *old_iter, unsigned long hash, match_func, key, struct cds_lfht_node *new_node)

Replaces node found by iterator
Must call cds_lfht_node_init() on new node first
Must be called inside same rcu_read_lock()/rcu_read_unlock() section as the lookup that filled old_iter
Must use call_rcu() to free old node after successful replacement

cds_lfht_lookup(struct cds_lfht *ht, unsigned long hash, match_func, key, struct cds_lfht_iter *iter)

Finds first node matching key, stores result in iterator
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Thread must be registered with rcu_register_thread()
Use cds_lfht_iter_get_node() to get node from iterator

cds_lfht_next_duplicate(struct cds_lfht *ht, match_func, key, struct cds_lfht_iter *iter)

Finds next node with same key, updates iterator
Must be called inside same rcu_read_lock()/rcu_read_unlock() section as initial lookup
Iterator must be initialized by cds_lfht_lookup() first
Use cds_lfht_iter_get_node() to get node from iterator

cds_lfht_first(struct cds_lfht *ht, struct cds_lfht_iter *iter)

Gets first node in table, stores in iterator
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Thread must be registered with rcu_register_thread()
Use cds_lfht_iter_get_node() to get node from iterator

cds_lfht_next(struct cds_lfht *ht, struct cds_lfht_iter *iter)

Gets next node in table, updates iterator
Must be called inside same rcu_read_lock()/rcu_read_unlock() section as cds_lfht_first()
Iterator must be initialized by cds_lfht_first() or previous cds_lfht_next()
Use cds_lfht_iter_get_node() to get node from iterator

cds_lfht_del(struct cds_lfht *ht, struct cds_lfht_node *node)

Removes node from hash table
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Node must have been found by lookup in same critical section
Must use call_rcu() to free node after successful deletion

cds_lfht_is_node_deleted(struct cds_lfht_node *node)

Checks if node has been deleted
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Node must have been found by lookup in same critical section

cds_lfht_iter_get_node(struct cds_lfht_iter *iter)

Extracts node pointer from iterator
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Iterator must be filled by lookup/iteration functions first
Returns NULL if iterator contains no node

cds_lfht_resize(struct cds_lfht *ht, unsigned long new_size)

Forces hash table resize to specified size
Must be called after rcu_register_thread()
Must not be called inside rcu_read_lock()/rcu_read_unlock() section

cds_lfht_count_nodes(struct cds_lfht *ht, long *before, unsigned long *count, long *after)

Counts nodes in hash table
Must be called inside rcu_read_lock()/rcu_read_unlock() section
Thread must be registered with rcu_register_thread()