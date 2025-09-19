#ifndef RCU_MAP_H
#define RCU_MAP_H

#include <stdbool.h>

typedef bool (*val_destructor_t)(void *);
typedef struct RcuMap RcuMap;
typedef struct RcuMap_Iter RcuMap_Iter;


bool rcu_map_Init(RcuMap* p_map);
RcuMap* rcu_map_Create();
bool rcu_map_Destroy(RcuMap* p_map);


bool rcu_map_RegisterThread(RcuMap* p_map);
bool rcu_map_UnregisterThread(RcuMap* p_map);


// plan Creation of key-value pair. key already exists this will not be planed and returns false
// Although you can plan removal of key then creation of the same key and that would be valid because the key will be removed before the creation
bool rcu_map_QueueCreate(RcuMap* p_map, const char* key, const void * const p_val);
// plan removal of key and val destructor function is needed because you usually dont want to delete it before this plan i executed in 
// rcu_map_ExecuteQueues because threads might be reading the value
bool rcu_map_QueueRemove(RcuMap* p_map, const char* key, val_destructor_t dtor);
// Replace value if key exists, else fail.
bool rcu_map_QueueUpdate(RcuMap *m, const char *key, void *new_val, val_destructor_t dtor);
// (create-or-update) is the sweet-spot op for most caches.
bool rcu_map_QueueUpsert(RcuMap *m, const char *key, void *new_val, val_destructor_t dtor);
// commit all queued writes and uses RCU to do so. the whole hash table is copied
bool rcu_map_CommitQueues(RcuMap* p_map);


bool rcu_map_StartRead(RcuMap *m);
bool rcu_map_EndRead(RcuMap *m);
bool rcu_map_IsReading(RcuMap *m);
// just checks if the key exists
bool rcu_map_Exists(RcuMap* p_map, const char* key);
// get the value for key and does lockfree read with help of epoch
void* rcu_map_Get(RcuMap* p_map, const char* key);

bool 			rcu_map_IterBegin(RcuMap *m);
bool 			rcu_map_IterNext(RcuMap *m);
const char*		rcu_map_IterKey(RcuMap *m);
void*			rcu_map_IterVal(RcuMap *m);
bool 			rcu_map_IterEnd(RcuMap *m);

#endif // RCU_MAP_H