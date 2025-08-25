#include "tsm.h"
#include "gtsm.h"
#include "tklog.h"
#include "xxhash.h"

#include <stdatomic.h>
#include <pthread.h>
#include <unistd.h>

// ==========================================================================================
// PRIVATE
// ==========================================================================================

// ==========================================================================================
// GLOBAL THREAD SAFE MAP
// ==========================================================================================

#define MAX_STRING_KEY_LEN 64
static _Atomic uint64_t g_key_counter = 1; // 0 is invalid. 1 is the first valid key
static struct tsm_base_node* GTSM = NULL;
static const struct tsm_key g_gtsm_key = { .key_union.string = "gtsm", .key_is_number = false };
static const struct tsm_key g_base_type_key = { .key_union.string = "base_type", .key_is_number = false };
static const struct tsm_key g_tsm_type_key  = { .key_union.string = "tsm_type",  .key_is_number = false };
// ==========================================================================================
// THREAD SAFE MAP
// ==========================================================================================

static int _tsm_key_match(struct cds_lfht_node *node, const void *key) {
    struct tsm_base_node *base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    struct tsm_key ctx_1 = { .key_union = base_node->key_union, .key_is_number = base_node->key_is_number };
    struct tsm_key* p_ctx_2 = (struct tsm_key *)key;
    return tsm_key_match(ctx_1, *p_ctx_2);
}
static uint64_t _tsm_hash_key(union tsm_key_union key_union, bool key_is_number) {
    if (key_is_number) {
        return XXH3_64bits(&key_union.number, sizeof(uint64_t));
    } else {
        return XXH3_64bits(key_union.string, strlen(key_union.string));
    }
}
static uint64_t _tsm_get_node_size(struct cds_lfht_node* node) {
    if (!node) {
        tklog_error("_tsm_get_node_size called with NULL node\n");
        return 0;
    }
    struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    return base_node->this_size_bytes;
}
static void* _tsm_get_node_start_ptr(struct cds_lfht_node* node) {
    if (!node) {
        tklog_error("_tsm_get_node_start_ptr called with NULL node\n");
        return NULL;
    }
    struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    return (void*)base_node;
}

// Base Type
static void _tsm_base_type_node_free_callback(struct rcu_head* rcu_head) {
    if (!rcu_head) {
        tklog_error("given rcu_head is NULL\n");
        return;
    }
    struct tsm_base_node* p_base = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);
    tklog_scope(bool free_result = tsm_base_node_free(p_base));
    if (!free_result) {
        tklog_error("failed to free\n");
    }
}
static int _tsm_base_type_node_try_free_callback(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    if (!p_tsm_base || !p_base) {
        tklog_error("NULL arguments\n");
        return -1;
    }
    tklog_scope(bool free_callback_result = tsm_base_node_free_callback(p_tsm_base, p_base));
    if (!free_callback_result) {
        tklog_error("tsm_base_node_free_callback failed\n");
        return -1;
    } 
    return 1;
}
static bool _tsm_base_type_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    if (!p_base) {
        tklog_debug("p_base is NULL\n");
        return false;
    }
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("p_tsm_base is not TSM\n");
        return false;
    }

    tklog_scope(bool is_valid = tsm_base_node_is_valid(p_tsm_base, p_base));
    if (!is_valid) {
        tklog_info("base is not valid\n");
        return false;
    }

    // Basic validation - check if it's a node type node
    struct tsm_base_type_node* p_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    if (!p_type->fn_free_callback) {
        tklog_debug("p_type->fn_free_callback is NULL\n");
        return false;
    }
    if (!p_type->fn_try_free_callback) {
        tklog_debug("p_type->fn_free_callback is NULL\n");
        return false;
    }
    if (!p_type->fn_is_valid) {
        tklog_debug("p_type->fn_is_valid is NULL\n");
        return false;
    }
    if (!p_type->fn_print) {
        tklog_debug("p_type->fn_print is NULL\n");
        return false;
    }
    if (p_type->type_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_debug("node_type->type_size_bytes(%d) < sizeof(struct tsm_base_node)(%d)\n", p_type->type_size_bytes, sizeof(struct tsm_base_node));
        return false;
    }

    return true;
}
static bool _tsm_base_type_node_print(struct tsm_base_node* p_base) {

    if (!tsm_base_node_print(p_base)) {
        tklog_error("failed to print base node\n");
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_base, struct tsm_base_type_node, base);

    tklog_info("    fn_free_callback: %p\n", p_type_node->fn_free_callback);
    tklog_info("    fn_try_free_callback: %p\n", p_type_node->fn_try_free_callback);
    tklog_info("    fn_is_valid: %p\n", p_type_node->fn_is_valid);
    tklog_info("    fn_print: %p\n", p_type_node->fn_print);
    tklog_info("    size of the node this node is type for: %d bytes\n", p_type_node->type_size_bytes);

    return true;
}

// TSM Type

// Hash function for type tracking
static uint64_t _type_tracking_tsm_clean_hash(struct tsm_key tk) {
    return _tsm_hash_key(tk.key_union, tk.key_is_number);
}
// Comparison function for type tracking
static int _type_tracking_tsm_clean_cmpr(struct tsm_key a, struct tsm_key b) {
    if (a.key_is_number != b.key_is_number) {
        return a.key_is_number ? 1 : -1;
    }
    
    if (a.key_is_number) {
        return (a.key_union.number > b.key_union.number) - (a.key_union.number < b.key_union.number);
    } else {
        return strcmp(a.key_union.string, b.key_union.string);
    }
}
#define NAME _type_tracking_tsm_clean_set
#define KEY_TY struct tsm_key
#define HASH_FN _type_tracking_tsm_clean_hash
#define CMPR_FN _type_tracking_tsm_clean_cmpr
#include "verstable.h"

static void _tsm_tsm_type_free_callback(struct rcu_head* rcu_head) {
    if (!rcu_head) {
        tklog_error("rcu_head is NULL\n");
        return;
    }

    struct tsm_base_node* p_base = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);

    rcu_read_lock();
    tklog_scope(uint32_t nodes_count = tsm_nodes_count(p_base));
    rcu_read_unlock();

    if (nodes_count > 0) {
        tklog_critical("tsm is not empty before freeing it\n");
    }

    tsm_key_union_free(p_base->key_union, p_base->key_is_number);
    tsm_key_union_free(p_base->type_key_union, p_base->type_key_is_number);
    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    cds_lfht_destroy(p_tsm->p_ht, NULL);
    for (unsigned int i = 0; i < p_tsm->path_length; ++i) {
        tsm_key_free(&p_tsm->path_from_global_to_parent_tsm[i]);
    }
    free(p_tsm->path_from_global_to_parent_tsm);
    free(p_base);
}
static int _tsm_tsm_type_free_children(struct tsm_base_node* p_tsm_base) {
    // Create a Verstable set to track which nodes are used as types
    _type_tracking_tsm_clean_set used_as_type_set;
    _type_tracking_tsm_clean_set_init(&used_as_type_set);
    
    // Iteratively remove nodes layer by layer
    const int batch_size = 1000;
    struct cds_lfht_node* nodes_to_delete[batch_size];
    int layer = 0;
    
    while (true) {
        int total_nodes = 0;
        int delete_count = 0;
        
        // Clear the used_as_type tracking set
        _type_tracking_tsm_clean_set_clear(&used_as_type_set);
        
        // Phase 1: Count nodes and identify which are used as types
        rcu_read_lock();

        struct cds_lfht_iter iter;
        tklog_scope(bool not_at_iter_end = tsm_iter_first(p_tsm_base, &iter));
        
        while (not_at_iter_end) {

            tklog_scope(struct tsm_base_node* base_node = tsm_iter_get_node(&iter));
            if (base_node == NULL) {
                tklog_warning("somehow base_node is NULL when total_nodes is %d\n", total_nodes);
                break;
            }

            total_nodes++;
            
            // Add this node's type to the used_as_type set
            struct tsm_key track_key;
            track_key.key_union = base_node->type_key_union;
            track_key.key_is_number = base_node->type_key_is_number;
            
            _type_tracking_tsm_clean_set_itr itr = _type_tracking_tsm_clean_set_insert(&used_as_type_set, track_key);
            if (_type_tracking_tsm_clean_set_is_end(itr)) {
                tklog_error("Out of memory tracking used types\n");
                // Continue anyway, will force cleanup
            }
            
            tklog_scope(not_at_iter_end = tsm_iter_next(p_tsm_base, &iter));
        }
        
        // If no nodes left, we're done
        if (total_nodes == 0) {
            rcu_read_unlock();
            break;
        }
        
        // Phase 2: Collect nodes that are not used as types
        tklog_scope(not_at_iter_end = tsm_iter_first(p_tsm_base, &iter));
        while (not_at_iter_end) {

            tklog_scope(struct tsm_base_node* base_node = tsm_iter_get_node(&iter));
            if (base_node == NULL) {
                tklog_warning("somehow base_node is NULL\n");
                break;
            }

            // Check if this node is used as a type
            struct tsm_key lookup_key;
            lookup_key.key_union = base_node->key_union;
            lookup_key.key_is_number = base_node->key_is_number;
            
            _type_tracking_tsm_clean_set_itr found = _type_tracking_tsm_clean_set_get(&used_as_type_set, lookup_key);
            
            if (_type_tracking_tsm_clean_set_is_end(found)) {
                // This node is not used as a type, mark for deletion
                nodes_to_delete[delete_count++] = &base_node->lfht_node;
            }
            
            tklog_scope(not_at_iter_end = tsm_iter_next(p_tsm_base, &iter));
        }
        
        // If no nodes to delete in this layer
        if (delete_count == 0 && total_nodes > 0) {
            if (total_nodes == 1) {
                tklog_scope(not_at_iter_end = tsm_iter_first(p_tsm_base, &iter));
                tklog_scope(struct tsm_base_node* base_node = tsm_iter_get_node(&iter));
                if (base_node == NULL) {
                    tklog_error("somehow base_node is NULL\n");
                    break;
                }
                nodes_to_delete[delete_count++] = &base_node->lfht_node;
            } else {
                tklog_error("No progress made, meaning there are only nodes left which are used as types. There are %d nodes left\n", total_nodes);
                rcu_read_unlock();
                break;
            }
        }
        
        // Phase 3: try free callback for the collected nodes
        for (int i = 0; i < delete_count; i++) {
            struct tsm_base_node* base_node = caa_container_of(nodes_to_delete[i], struct tsm_base_node, lfht_node);
            
            // try free callback
            tklog_scope(int try_free_callback_result = tsm_node_try_free_callback(p_tsm_base, base_node));
            if (try_free_callback_result == -1) {
                tklog_error("tsm_node_try_free_callback failed for child node\n");
                rcu_read_unlock();
                return -1;
            }
        }
        
        rcu_read_unlock();
        
        if (delete_count > 0) {
            tklog_info("loop %d: Cleaned up %d nodes\n", layer, delete_count);
            layer++;
        }
    }
    
    // Clean up tracking set
    _type_tracking_tsm_clean_set_cleanup(&used_as_type_set);
    
    tklog_info("Global data system cleanup completed (%d layers)\n", layer);

    return 1;
}
static int _tsm_tsm_type_try_free_callback(struct tsm_base_node* p_parent_tsm_base, struct tsm_base_node* p_tsm_base) {
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_parent_tsm_base));
    if (!is_tsm) {
        tklog_warning("given tsm is not a TSM\n");
        return -1;
    }
    tklog_scope(is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_warning("given tsm is not a TSM\n");
        return -1;
    }

    tklog_scope(uint32_t nodes_count = tsm_nodes_count(p_tsm_base));

    // if true then this function must call the try_free_callback on the top layered children 
    // then this function must be called again so that the next layer can be freed
    if (nodes_count > 0) {
        tklog_scope(int free_children_result = _tsm_tsm_type_free_children(p_tsm_base));
        return free_children_result;
    } 

    // all child nodes are freed so now this node can be deleted and tryd for free callback
    else {
        tklog_scope(bool free_callback_result = tsm_base_node_free_callback(p_parent_tsm_base, p_tsm_base));
        if (!free_callback_result) {
            tklog_error("tsm_base_node_free_callback failed\n");
            return -1;
        } 
        return 1;
    }
}
static bool _tsm_tsm_type_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    (void)p_tsm_base;

    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_base));
    if (!is_tsm) {
        tklog_error("p_base is not a TSM\n");
        return false;
    }

    rcu_read_lock();
    struct cds_lfht_iter* p_iter = calloc(1, sizeof(struct cds_lfht_iter));
    tklog_scope(tsm_iter_first(p_base, p_iter));
    bool iter_next = p_iter != NULL;
    while (iter_next) {
        tklog_scope(struct tsm_base_node* iter_node = tsm_iter_get_node(p_iter));
        if (!iter_node) {
            tklog_warning("iter_node is NULL\n");
            break;
        }
        tklog_scope(bool is_valid = tsm_base_node_is_valid(p_base, iter_node));
        if (!is_valid) {
            tklog_info("This node is not valid:\n");
            tklog_scope(tsm_node_print(p_base, iter_node));
            free(p_iter);
            rcu_read_unlock();
            return false;
        }
        tklog_scope(iter_next = tsm_iter_next(p_base, p_iter));
    }
    free(p_iter);
    rcu_read_unlock();

    return true;
}
static bool _tsm_tsm_type_print(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_base));
    if (!is_tsm) {
        tklog_error("p_base is not a TSM\n");
        return false;
    }

    tklog_scope(tsm_base_node_print(p_base));
    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    tklog_info("path to parent from global TSM:\n");
    
    char full_str[256];
    char *ptr = full_str;
    size_t remaining = sizeof(full_str);
    for (uint32_t i = 0; i < p_tsm->path_length; i++) {
        int len;
        if (p_tsm->path_from_global_to_parent_tsm[i].key_is_number) {
            len = snprintf(ptr, remaining, "%lld -> ", p_tsm->path_from_global_to_parent_tsm[i].key_union.number);
        } else {
            len = snprintf(ptr, remaining, "%s -> ", p_tsm->path_from_global_to_parent_tsm[i].key_union.string);
        }
        if (len > 0) {
            size_t advance = (size_t)len < remaining ? (size_t)len : remaining - 1;
            ptr += advance;
            remaining -= advance;
        }
        if (remaining <= 1) break;  // No more space, including for null terminator
    }
    *ptr = '\0';
    tklog_info("%s", full_str);

    rcu_read_lock();
    struct cds_lfht_iter* p_iter = calloc(1, sizeof(struct cds_lfht_iter));
    tklog_scope(tsm_iter_first(p_base, p_iter));
    bool iter_next = p_iter != NULL;
    while (iter_next) {
        tklog_scope(struct tsm_base_node* iter_node = tsm_iter_get_node(p_iter));
        tklog_scope(tsm_node_print(p_base, iter_node));
        tklog_scope(iter_next = tsm_iter_next(p_base, p_iter));
    }
    rcu_read_unlock();
    free(p_iter);

    return true;
}

// ==========================================================================================
// PUBLIC
// ==========================================================================================

// ==========================================================================================
// KEY
// ==========================================================================================
union tsm_key_union tsm_key_union_create(uint64_t number_key, const char* string_key, bool key_is_number) {
    if (!key_is_number && string_key == NULL) {
        tklog_error("string key is NULL\n");
        return (union tsm_key_union){ .string = NULL };
    }
    union tsm_key_union key_union;
    if (key_is_number) {
        if (number_key == 0) {
            number_key = atomic_fetch_add(&g_key_counter, 1);
        }
        key_union.number = number_key;
    } else {
        // Validate and copy string key
        if (strlen(string_key) == 0) {
            tklog_error("string key is empty\n");
            return (union tsm_key_union){ .string = NULL };
        }
        
        size_t key_len = strlen(string_key) + 1;
        if (key_len > MAX_STRING_KEY_LEN) {
            tklog_error("string key too long (max %d characters)\n", MAX_STRING_KEY_LEN - 1);
            return (union tsm_key_union){ .string = NULL };
        }
        
        char* copied_string = calloc(1, key_len);
        if (!copied_string) {
            tklog_error("Failed to allocate memory for string key\n");
            return (union tsm_key_union){ .string = NULL };
        }
        
        if (strncpy(copied_string, string_key, key_len - 1) == NULL) {
            tklog_error("Failed to copy string key\n");
            free(copied_string);
            return (union tsm_key_union){ .string = NULL };
        }
        copied_string[key_len - 1] = '\0'; // Ensure null termination
        
        key_union.string = copied_string;
    }
    return key_union;
}
bool tsm_key_union_free(union tsm_key_union key_union, bool key_is_number) {
    if (key_union.number == 0) {
        tklog_error("number key is 0\n");
        return false;
    }
    if (key_is_number) {
        return true;
    } else {
        if (key_union.string) {
            free(key_union.string);
            return true;
        }
    }
    return false;
}
struct tsm_key tsm_key_create(uint64_t number_key, const char* string_key, bool key_is_number) {
    struct tsm_key return_value = {0};
    tklog_scope(return_value.key_union = tsm_key_union_create(number_key, string_key, key_is_number));
    return_value.key_is_number = key_is_number;
    return return_value;
}
struct tsm_key tsm_key_copy(struct tsm_key key) {
    tklog_scope(struct tsm_key new_key = tsm_key_create(key.key_union.number, key.key_union.string, key.key_is_number));
    return new_key;
}
bool tsm_key_free(struct tsm_key* key) {
    if (!key) {
        tklog_warning("key is NULL\n");
        return false;
    }
    tklog_scope(bool free_result = tsm_key_union_free(key->key_union, key->key_is_number));
    if (!free_result) {
        tklog_scope("failed to free node\n");
        return false;
    }
    key->key_union.number = 0;
    key->key_is_number = true;
    return true;
}
bool tsm_key_match(struct tsm_key key_1, struct tsm_key key_2) {
    // if not both is string or both is number
    if (key_1.key_is_number != key_2.key_is_number)
        return false;
    // if both are number
    if (key_1.key_is_number) 
        return key_1.key_union.number == key_2.key_union.number;
    // if both are string
    else 
        return strcmp(key_1.key_union.string, key_2.key_union.string) == 0;
}
void tsm_key_print(struct tsm_key key) {
    if (key.key_is_number) {
        tklog_notice("key: %d\n", key.key_union.number);
    } else {
        tklog_notice("key: %s\n", key.key_union.string);
    }
}

// ==========================================================================================
// BASE NODE
// ==========================================================================================
struct tsm_base_node* tsm_base_node_create(struct tsm_key key, struct tsm_key type_key, uint32_t this_size_bytes, bool this_is_type, bool this_is_tsm) {
    if (key.key_union.number == 0) {
        tklog_error("key.number is 0\n");
        return NULL;
    }
    if (type_key.key_union.number == 0) {
        tklog_error("type_key.number is 0\n");
        return NULL;
    }
    if (this_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_error("this_size_bytes is less than the size of the tsm_base_node\n");
        return NULL;
    }
    struct tsm_base_node* node = calloc(1, this_size_bytes);
    if (!node) {
        tklog_error("Failed to allocate memory for node\n");
        return NULL;
    }
    cds_lfht_node_init(&node->lfht_node);
    
    // Handle key assignment - copy string keys, use number keys directly
    if (key.key_is_number) {
        node->key_union.number = key.key_union.number;
    } else {
        // String key is already copied by tsm_key_union_create
        node->key_union.string = key.key_union.string;
    }
    
    // Handle type_key assignment - copy string keys, use number keys directly
    if (type_key.key_is_number) {
        node->type_key_union.number = type_key.key_union.number;
    } else {
        // String type_key is already copied by tsm_key_union_create
        node->type_key_union.string = type_key.key_union.string;
    }
    
    node->key_is_number = key.key_is_number;
    node->type_key_is_number = type_key.key_is_number;
    node->this_size_bytes = this_size_bytes;
    node->this_is_type = this_is_type;
    node->this_is_tsm = this_is_tsm;
    return node;
}
bool tsm_base_node_free(struct tsm_base_node* p_base_node) {
    if (!p_base_node) {
        tklog_error("p_base_node is NULL when trying to free it");
        return false;
    }
    tsm_key_union_free(p_base_node->key_union, p_base_node->key_is_number);
    tsm_key_union_free(p_base_node->type_key_union, p_base_node->type_key_is_number);
    free(p_base_node);
    return true;
}
bool tsm_base_node_free_callback(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    if (!p_tsm_base || !p_base) {
        tklog_error("NULL arguments\n");
        return false;
    }

    // Get the type information
    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_base_node = tsm_node_get(p_tsm_base, type_key));
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for node to be removed\n");
        tklog_scope(tsm_node_print(p_tsm_base, p_base));
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_base_node, struct tsm_base_type_node, base);
    
    // Copy the free callback while still holding the read lock
    // This ensures the callback remains valid even after we release the lock
    void (*free_callback)(struct rcu_head*) = p_type_node->fn_free_callback;
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(tsm_node_print(p_tsm_base, p_base));
        return false;
    }
    
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    if (cds_lfht_del(p_tsm->p_ht, &p_base->lfht_node) != 0) {
        tklog_error("Failed to delete node thread safe map\n");
        tklog_scope(tsm_node_print(p_tsm_base, p_base));
        return false;
    }

    // Schedule for RCU cleanup after successful removal
    call_rcu(&p_base->rcu_head, free_callback);

    return true;
}
bool tsm_base_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm_key key = { .key_union = p_base->key_union, .key_is_number = p_base->key_is_number};
    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_is_number = p_base->type_key_is_number};

    tklog_scope(struct tsm_base_node* p_base_copy = tsm_node_get(p_tsm_base, key));
    if (p_base != p_base_copy) {
        tklog_debug("given p_base and p_base gotten from key in given p_base is not the same\n");
        tklog_info("given p_base:\n");
        tklog_scope(bool print_result = tsm_node_print(p_tsm_base, p_base));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        tklog_info("retrieved p_base from given p_base:\n");
        tklog_scope(print_result = tsm_node_print(p_tsm_base, p_base_copy));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        return false;
    }

    tklog_scope(struct tsm_base_node* p_base_type = tsm_node_get(p_tsm_base, type_key));
    if (!p_base_type) {
        tklog_debug("did not find type for given base node\n");
        tklog_scope(bool print_result = tsm_node_print(p_tsm_base, p_base));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        return false;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    if (p_base->this_size_bytes != p_type->type_size_bytes) {
        tklog_debug("p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)", p_base->this_size_bytes, p_type->type_size_bytes);
        tklog_scope(bool print_result = tsm_node_print(p_tsm_base, p_base));
        if (!print_result) {
            tklog_error("failed to print p_base node\n");
        }
        return false;
    }

    tklog_scope(bool is_deleted = tsm_node_is_deleted(p_base));
    if (is_deleted) {
        tklog_debug("node is deleted and therefore not valid\n");
        return false;
    }

    return true;
}
bool tsm_base_node_print(struct tsm_base_node* p_base) {

    tklog_scope(bool is_deleted = tsm_node_is_deleted(p_base));

    if (p_base->key_is_number) {
        tklog_info("key: %lld\n", p_base->key_union.number);
    } else {
        tklog_info("key: %s\n", p_base->key_union.string);
    }

    if (is_deleted) {
        tklog_info("    is deleted\n");
    } else {
        tklog_info("    is not deleted\n");
    }

    if (p_base->type_key_is_number) {
        tklog_info("    type_key: %lld\n", p_base->type_key_union.number);
    } else {
        tklog_info("    type_key: %s\n", p_base->type_key_union.string);
    }

    tklog_info("    size: %d bytes\n", p_base->this_size_bytes);

    return true;
}

// ==========================================================================================
// BASE TYPE
// ==========================================================================================
struct tsm_base_node* tsm_base_type_node_create(
    struct tsm_key key, 
    uint32_t this_size_bytes,
    void (*fn_free_callback)(struct rcu_head*),
    int (*fn_try_free_callback)(struct tsm_base_node*, struct tsm_base_node*),
    bool (*fn_is_valid)(struct tsm_base_node*, struct tsm_base_node*),
    bool (*fn_print)(struct tsm_base_node*),
    uint32_t type_size_bytes) {
    if (this_size_bytes < sizeof(struct tsm_base_type_node)) {
        tklog_error("this_size_bytes(%d) is less than sizeof(tsm_base_type_node)\n", sizeof(struct tsm_base_type_node));
        return NULL;
    }
    if (type_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_error("type_size_bytes(%d) is less than sizeof(tsm_base_node)\n", sizeof(struct tsm_base_node));
        return NULL;
    }
    if (fn_try_free_callback == NULL) {
        tklog_error("fn_try_free_callback is NULL\n");
        return NULL;
    }
    if (fn_free_callback == NULL) {
        tklog_error("fn_free_callback is NULL\n");
        return NULL;
    }
    if (fn_is_valid == NULL) {
        tklog_error("fn_is_valid is NULL\n");
        return NULL;
    }
    if (fn_print == NULL) {
        tklog_error("fn_print is NULL\n");
        return NULL;
    }

    // Create proper copies of the string keys for the base_type node
    tklog_scope(struct tsm_key base_type_key_copy = tsm_key_copy(g_base_type_key));
    
    tklog_scope(struct tsm_base_node* p_base = tsm_base_node_create(key, base_type_key_copy, sizeof(struct tsm_base_type_node), true, false));

    if (!p_base) {
        tklog_error("Failed to create base_type node\n");
        tsm_key_free(&base_type_key_copy);
        return NULL;
    }

    struct tsm_base_type_node* p_base_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    p_base_type->fn_free_callback = fn_free_callback;
    p_base_type->fn_try_free_callback = fn_try_free_callback;
    p_base_type->fn_is_valid = fn_is_valid;
    p_base_type->fn_print = fn_print;
    p_base_type->type_size_bytes = type_size_bytes;

    return p_base;
}

// ==========================================================================================
// THREAD SAFE MAP
// ==========================================================================================
struct tsm_base_node* tsm_create_and_insert(
    struct tsm_base_node* p_tsm_base,
    struct tsm_key tsm_key) {

    // validating TSM
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return NULL;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_warning("given tsm is not a TSM\n");
        return NULL;
    }
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    // check if tsm_type exists in given TSM
    tklog_scope(struct tsm_base_node* p_tsm_type_base = tsm_node_get(p_tsm_base, g_tsm_type_key));
    if (!p_tsm_type_base) {
        // have to insert "tsm_type" type node into parent TSM because it doesnt exist
        tklog_info("did not find tsm_type in given TSM\n");

        // creating tsm_type now, as base_type is created
        tklog_scope(struct tsm_key tsm_type_key_copy = tsm_key_copy(g_tsm_type_key));
        tklog_scope(struct tsm_base_node* p_new_tsm_type = tsm_base_type_node_create(
            tsm_type_key_copy, 
            sizeof(struct tsm_base_type_node),
            _tsm_tsm_type_free_callback,
            _tsm_tsm_type_try_free_callback,
            _tsm_tsm_type_is_valid,
            _tsm_tsm_type_print,
            sizeof(struct tsm)));
        if (!p_new_tsm_type) {
            tklog_error("Failed to create tsm_type\n");
            tsm_key_free(&tsm_type_key_copy);
            return false;
        }

        tklog_scope(bool insert_result = tsm_node_insert(p_tsm_base, p_new_tsm_type));
        if (!insert_result) {
            tklog_error("failed to insert tsm_type into parent TSM\n");
            return false;
        }
    }

    // create the new tsm node
    tklog_scope(struct tsm_key tsm_type_key_copy = tsm_key_copy(g_tsm_type_key));
    tklog_scope(struct tsm_base_node* p_new_tsm_base = tsm_base_node_create(tsm_key, tsm_type_key_copy, sizeof(struct tsm), false, true));
    if (!p_new_tsm_base) {
        tklog_error("Failed to create base node for new TSM\n");
        tsm_key_free(&tsm_type_key_copy);
        return NULL;
    }
    struct tsm* p_new_tsm = caa_container_of(p_new_tsm_base, struct tsm, base);
    p_new_tsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    if (!p_new_tsm->p_ht) {
        tklog_error("Failed to create lock free hash table\n");
        return NULL;
    }
    p_new_tsm->path_length = p_tsm->path_length + 1;
    p_new_tsm->path_from_global_to_parent_tsm = calloc(1, p_new_tsm->path_length * sizeof(tsm_key));
    if (!p_new_tsm->path_from_global_to_parent_tsm) {
        tklog_error("calloc failed\n");
        return NULL;
    }
    // add path of parent TSM
    for (uint32_t i = 0; i < p_new_tsm->path_length-1; ++i) {
        p_new_tsm->path_from_global_to_parent_tsm[i] = tsm_key_copy(p_tsm->path_from_global_to_parent_tsm[i]);
    }
    // at last index at key of parent_map
    struct tsm_key key_tsm = { .key_union = p_tsm->base.key_union, .key_is_number = p_tsm->base.key_is_number };
    tklog_scope(p_new_tsm->path_from_global_to_parent_tsm[p_new_tsm->path_length-1] = tsm_key_copy(key_tsm));

    // Create proper copies of the string keys for the base_type node
    tklog_scope(struct tsm_key base_key = tsm_key_copy(g_base_type_key));
    tklog_scope(struct tsm_base_node* base_type_base = tsm_base_type_node_create(
        base_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_try_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node)));

    if (!base_type_base) {
        tklog_error("Failed to create base_type node\n");
        tsm_base_node_free(base_type_base);
        goto free_tsm;
    }
    
    uint64_t hash = _tsm_hash_key(base_type_base->key_union, base_type_base->key_is_number);

    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &base_key, &base_type_base->lfht_node));
    
    if (result != &base_type_base->lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        tsm_base_node_free(base_type_base);
        goto free_tsm;
    }

    tklog_scope(struct tsm_base_node* base_node = tsm_node_get(p_new_tsm_base, g_base_type_key));
    if (!base_node) {
        tklog_error("base_type node not found after initialization\n");
        goto free_tsm;
    }
    
    tklog_info("Created base_type node inside new TSM with key: %s, size: %u\n", base_node->key_union.string, base_node->this_size_bytes);

    tklog_scope(bool insert_result = tsm_node_insert(p_tsm_base, p_new_tsm_base));
    if (!insert_result) {
        tklog_error("Failed to insert new tsm into given TSM\n");
        goto free_tsm;
    }

    return p_new_tsm_base;

    free_tsm:

    tklog_scope(int free_children_result = _tsm_tsm_type_free_children(p_new_tsm_base));
    if (free_children_result == -1) {
        tklog_error("freeing children for failed TSM failed\n");
    }
    tklog_scope(_tsm_tsm_type_free_callback(&p_new_tsm_base->rcu_head));

    return NULL;
}
struct tsm_base_node* tsm_get_parent_tsm(struct tsm_base_node* p_tsm_base) {
    if (!GTSM) {
        tklog_error("GTSM is NULL\n");
        return NULL;
    }
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return NULL;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    // starts with the Global TSM as parent but will work our way towards the actuall parent
    struct tsm_base_node* p_parent_base = GTSM;
    for (unsigned int i = 1; i < p_tsm->path_length; ++i) {
        struct tsm_key* p_tsm_key = &p_tsm->path_from_global_to_parent_tsm[i];
        tklog_scope(struct tsm_base_node* p_temp_parent = tsm_node_get(p_parent_base, *p_tsm_key));
        if (!p_temp_parent) {
            tklog_error("TSM Key to get didnt exist:\n");
            tsm_key_print(*p_tsm_key);
            tklog_error("failed to get TSM inside TSM:\n");
            return NULL;
        }
        p_parent_base = p_temp_parent;
    }

    return p_parent_base;
}
struct tsm_base_node* tsm_node_get(struct tsm_base_node* p_tsm_base, struct tsm_key key) {
    if (!p_tsm_base) {
        tklog_error("p_tsm is NULL\n");
        return NULL;
    }
    if (key.key_union.number == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }
    if (key.key_is_number && key.key_union.number == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return NULL;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash internally
    uint64_t hash = _tsm_hash_key(key.key_union, key.key_is_number);
    struct cds_lfht_iter iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key, &iter);
    struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(&iter);
    
    if (lfht_node) {
        struct tsm_base_node* result = caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
        return result;
    }
    return NULL;
}
bool tsm_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    if (!p_base) {
        tklog_notice("getting base node failed\n");
        return false;
    }
    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return false;
    }

    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_base_type = tsm_node_get(p_tsm_base, type_key));
    if (!p_base_type) {
        tklog_notice("tried to get type node but got NULL for node: \n");
        return false;
    }

    tklog_scope(struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base));
    if (p_base->this_size_bytes != p_type->type_size_bytes) {
        tklog_notice("p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)\n", p_base->this_size_bytes, p_type->type_size_bytes);
        tklog_scope(p_type->fn_print(p_base));
        return false;
    }

    tklog_scope(bool return_result = p_type->fn_is_valid(p_tsm_base, p_base));
    if (!return_result) {
        tklog_notice("fn_is_valid for node returned false:\n");
        tsm_node_print(p_tsm_base, p_base);
    }

    return return_result;
}
bool tsm_node_is_tsm(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    return p_base->this_is_tsm;
}
bool tsm_node_is_type(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return false;
    }
    return p_base->this_is_type;
}
bool tsm_node_print(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return false;
    }

    if (!p_base) {
        tklog_error("p_base is NULL");
        return false;
    }

    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_base_type = tsm_node_get(p_tsm_base, type_key));

    if (!p_base_type) {
        if (p_base->type_key_is_number)
            if (p_base->key_is_number) 
                tklog_error("Could not get type node with key %lld for node with key %lld\n", p_base->type_key_union.number, p_base->key_union.number);
            else 
                tklog_error("Could not get type node with key %lld for node with key %s\n", p_base->type_key_union.number, p_base->key_union.string);
        else
            if (p_base->key_is_number) 
                tklog_error("Could not get type node with key %s for node with key %lld\n", p_base->type_key_union.string, p_base->key_union.number);
            else 
                tklog_error("Could not get type node with key %s for node with key %s\n", p_base->type_key_union.string, p_base->key_union.string);
        return false;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    tklog_scope(p_type->fn_print(p_base));

    return true;
}
bool tsm_node_insert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key_union.number == 0) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("key number is 0\n");
        return false;
    }
    if (new_node->type_key_union.number == 0) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("type_key number is 0\n");
        return false;
    }
    if (!tsm_node_is_tsm(p_tsm_base)) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }
    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_scope(tsm_node_print(p_tsm_base , new_node));
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm_key type_key = { .key_union = new_node->type_key_union, .key_is_number = new_node->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_node_base = tsm_node_get(p_tsm_base, type_key));

    if (!p_type_node_base) {
        tklog_scope(tsm_base_node_print(p_tsm_base));
        tklog_scope(tsm_base_node_print(new_node));
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("didnt find node_type");
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_node_base, struct tsm_base_type_node, base);
    uint32_t type_size_bytes = p_type_node->type_size_bytes;

    if (type_size_bytes != new_node->this_size_bytes) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_scope(tsm_node_print(p_tsm_base, p_type_node_base));
        tklog_error("type_size_bytes(%d) != new_node->this_size_bytes(%d)", type_size_bytes, new_node->this_size_bytes);
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    uint64_t hash = _tsm_hash_key(new_node->key_union, new_node->key_is_number);
    struct tsm_key key = { .key_union = new_node->key_union, .key_is_number = new_node->key_is_number };
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_tsm->p_ht, hash, _tsm_key_match, &key, &new_node->lfht_node));
    
    if (result != &new_node->lfht_node) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("Somehow node already exists\n");
        return false;
    }

    tklog_scope(bool is_valid_type_function_result = tsm_node_is_valid(p_tsm_base, new_node));
    if (!is_valid_type_function_result) {
        tklog_error("after inserting then running tsm_node_is_valid for the inserted node it returned false\n");
        return false;
    }

    if (new_node->key_is_number) {
        tklog_debug("Successfully inserted node with number key %llu\n", new_node->key_union.number);
        return true;
    } else {
        tklog_debug("Successfully inserted node with string key %s\n", new_node->key_union.string);
        return true;
    }
}
bool tsm_node_update(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key_union.number == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }
    if (new_node->type_key_union.number == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }

    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base node is not a TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Find the existing node
    uint64_t                hash = _tsm_hash_key(new_node->key_union, new_node->key_is_number);
    struct tsm_key ctx = { .key_union = new_node->key_union, .key_is_number = new_node->key_is_number };
    struct cds_lfht_iter    old_iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &ctx, &old_iter);
    struct cds_lfht_node*   old_lfht_node = cds_lfht_iter_get_node(&old_iter);
    struct tsm_base_node*    old_node = caa_container_of(old_lfht_node, struct tsm_base_node, lfht_node);
    if (!old_node) {
        if (new_node->key_is_number) {
            tklog_debug("Cannot update - node with number key %llu not found\n", new_node->key_union.number);
        } 
        else {
            tklog_debug("Cannot update - node with string key %s not found\n", new_node->key_union.string);
        }
        return false;
    }
    if (old_node == new_node) {
        tklog_error("when replacing a node its not allowed to replace with the same node. given new_node == old_node\n");
        return false;
    }
 
    // Get the type information
    struct tsm_key old_type_key = { .key_union = old_node->type_key_union, .key_is_number = old_node->type_key_is_number };
    struct tsm_key new_type_key = { .key_union = new_node->type_key_union, .key_is_number = new_node->type_key_is_number };
    if (!tsm_key_match(old_type_key, new_type_key)) {
        tklog_error("key of old and new are not the same\n");
        tklog_error("new:\n");
        tsm_key_print(new_type_key);
        tklog_error("old:\n");
        tsm_key_print(old_type_key);
        return false;
    }

    tklog_scope(struct tsm_base_node* p_type_base_node = tsm_node_get(p_tsm_base, old_type_key));
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for existing node\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_base_node, struct tsm_base_type_node, base);
    
    // Verify data size matches expected type size 
    if (new_node->this_size_bytes != p_type_node->type_size_bytes) {
        tklog_error("new node size %u doesn't match current node size %u\n", new_node->this_size_bytes, p_type_node->type_size_bytes);
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }

    // Get the free callback
    void (*free_callback)(struct rcu_head*) = p_type_node->fn_free_callback;
    if (!free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }

    int replace_result = cds_lfht_replace(p_tsm->p_ht, &old_iter, hash, _tsm_key_match, &ctx, &new_node->lfht_node);
    
    if (replace_result == 0) {
        // Schedule old node for RCU cleanup
        tklog_scope(call_rcu(&old_node->rcu_head, free_callback));
        if (new_node->key_is_number) {
            tklog_debug("Successfully updated node with number key %llu\n", new_node->key_union.number);
        } else {
            tklog_debug("Successfully updated node with string key %s\n", new_node->key_union.string);
        }
        return true;
    } else {
        tklog_error("Failed to replace node in hash table\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }
}
bool tsm_node_upsert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return false;
    }
    if (new_node->key_union.number == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }
    if (new_node->type_key_union.number == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return false;
    }

    struct tsm_key key = { .key_union = new_node->key_union, .key_is_number = new_node->key_is_number };
    tklog_scope(struct tsm_base_node* old_node = tsm_node_get(p_tsm_base, key));

    if (old_node) {
        tklog_scope(bool update_result = tsm_node_update(p_tsm_base, new_node));
        if (!update_result) {
            tklog_error("update inside upsert function failed\n");
            return false;
        }
    } else {
        tklog_scope(bool insert_result = tsm_node_insert(p_tsm_base, new_node));
        if (!insert_result) {
            tklog_error("insert function inside upsert function failed\n");
            return false;
        }
    }
    return true;
}
int tsm_node_try_free_callback(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    if (!p_tsm_base || !p_base) {
        tklog_error("NULL arguments not valid\n");
        return -1;
    }
    if (!tsm_node_is_tsm(p_tsm_base)) {
        tsm_base_node_print(p_tsm_base);
        tklog_error("given p_tsm_base is not a TSM\n");
        return -1;
    }
    struct tsm_key type_key = { .key_union.string = p_base->type_key_union.string, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_base = tsm_node_get(p_tsm_base, type_key));
    if (!p_type_base) {
        gtsm_print();
        tsm_base_node_print(p_tsm_base);
        tsm_base_node_print(p_base);
        tklog_error("could not get type for node to free"); 
        return -1;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_type_base, struct tsm_base_type_node, base);

    tklog_scope(int try_free_callback_result = p_type->fn_try_free_callback(p_tsm_base, p_base));

    return try_free_callback_result;
}
bool tsm_node_is_deleted(struct tsm_base_node* node) {
    if (!node) {
        tklog_warning("given node is NULL\n");
        return true; // NULL node is considered deleted
    }
    tklog_scope(bool result = cds_lfht_is_node_deleted(&node->lfht_node));
    return result;
}
unsigned long tsm_nodes_count(struct tsm_base_node* p_tsm_base) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return 0;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    long split_count_before, split_count_after;
    unsigned long count;
    
    // cds_lfht_count_nodes MUST be called from within read-side critical section
    // according to URCU specification
    cds_lfht_count_nodes(p_tsm->p_ht, &split_count_before, &count, &split_count_after);
    
    return count;
}

bool tsm_iter_first(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    if (!p_tsm_base) {
        tklog_error("tsm_iter_first called with NULL p_tsm_base\n");
        return false;
    }
    if (!iter) {
        tklog_error("iter is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    tklog_scope(cds_lfht_first(p_tsm->p_ht, iter));
    
    return true;
}
bool tsm_iter_next(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    if (!p_tsm_base || !iter) {
        tklog_error("tsm_iter_next called with NULL parameters or uninitialized system\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    tklog_scope(cds_lfht_next(p_tsm->p_ht, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}
struct tsm_base_node* tsm_iter_get_node(struct cds_lfht_iter* iter) {
    if (!iter) {
        tklog_error("tsm_iter_next called with NULL parameters or uninitialized system\n");
        return false;
    }
    
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    if (lfht_node) {
        return caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
    }
    return NULL;
}
bool tsm_iter_lookup(struct tsm_base_node* p_tsm_base, struct tsm_key key, struct cds_lfht_iter* iter) {
    if (!p_tsm_base || !iter) {
        tklog_error("tsm_lookup_iter called with NULL parameters\n");
        return false;
    }
    if (key.key_union.number == 0) {
        tklog_error("number key is 0\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash
    uint64_t hash = _tsm_hash_key(key.key_union, key.key_is_number);
    tklog_scope(cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key, iter));
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    
    return (lfht_node != NULL);
}

// ==========================================================================================
// GLOBAL THREAD SAFE MAP
// ==========================================================================================
bool gtsm_init() {

    rcu_read_lock();
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (GTSM_rcu) {
        tklog_info("GTSM is already initialized because its not NULL\n");
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();

    // Register the node size function with urcu_safe  system
    urcu_safe_set_node_size_function(_tsm_get_node_size);
    // Register the node bounds functions with urcu_safe system
    urcu_safe_set_node_start_ptr_function(_tsm_get_node_start_ptr);

    // Create proper copies of the string keys for the base_type node
    tklog_scope(struct tsm_key base_key = tsm_key_copy(g_base_type_key));
    tklog_scope(struct tsm_base_node* base_type_base = tsm_base_type_node_create(
        base_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_try_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node)));

    if (!base_type_base) {
        tklog_error("Failed to create base_type node\n");
        tsm_key_free(&base_key);
        return false;
    }

    // create the GTSM node
    tklog_scope(struct tsm_key gtsm_key = tsm_key_copy(g_gtsm_key));
    tklog_scope(struct tsm_key gtsm_type_key = tsm_key_copy(g_tsm_type_key));
    tklog_scope(struct tsm_base_node* p_new_gtsm_base = tsm_base_node_create(gtsm_key, gtsm_type_key, sizeof(struct tsm), false, true));
    if (!p_new_gtsm_base) {
        tklog_error("Failed to create base node for new TSM\n");
        tsm_key_free(&gtsm_key);
        tsm_key_free(&gtsm_type_key);
        return false;
    }
    struct tsm* p_new_gtsm = caa_container_of(p_new_gtsm_base, struct tsm, base);
    p_new_gtsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    if (!p_new_gtsm->p_ht) {
        tklog_error("Failed to create lock free hash table\n");
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        return false;
    }
    p_new_gtsm->path_length = 0;
    p_new_gtsm->path_from_global_to_parent_tsm = NULL;

    uint64_t hash = _tsm_hash_key(base_type_base->key_union, base_type_base->key_is_number);

    rcu_read_lock();
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_new_gtsm->p_ht, hash, _tsm_key_match, &base_key, &base_type_base->lfht_node));
    
    if (result != &base_type_base->lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        _tsm_base_type_node_free_callback(&base_type_base->rcu_head);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        return false;
    }

    tklog_scope(struct tsm_base_node* base_node = tsm_node_get(p_new_gtsm_base, g_base_type_key));
    if (!base_node) {
        tklog_error("base_type node not found after initialization\n");
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        return false;
    }
    rcu_read_unlock();
    
    tklog_info("Created base_type node inside new TSM with key: %s, size: %u\n", base_node->key_union.string, base_node->this_size_bytes);

    void* old_pointer = rcu_cmpxchg_pointer(&GTSM, NULL, p_new_gtsm_base);
    if (old_pointer != NULL) {
        tklog_notice("failed to swap pointer\n");
        return false;
    }

    return true;
}
struct tsm_base_node* gtsm_get() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    return GTSM_rcu;
}
bool gtsm_free() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (GTSM_rcu == NULL) {
        tklog_info("GTSM is NULL");
        return false;
    }
    // assigning NULL now so that no more nodes can be inserted into GTSM when we try to remove all nodes inside GTSM
    // actually even though GTSM is assigned to NULL threads that is inside a long read could still access the old GTSM
    // and insert into GTSM even after all nodes are scheduled to be removed. How HOW TO FIX THIS????
    void* old_pointer = rcu_cmpxchg_pointer(&GTSM, GTSM_rcu, NULL);
    if (old_pointer != GTSM_rcu) {
        tklog_notice("failed to swap pointer to NULL\n");
        return false;
    }

    tklog_scope(bool is_tsm = tsm_node_is_tsm(GTSM_rcu));
    if (!is_tsm) {
        tklog_warning("given tsm is not a TSM\n");
        return false;
    }

    tklog_scope(uint32_t nodes_count = tsm_nodes_count(GTSM_rcu));

    // if true then this function must call the try_free_callback on the top layered children 
    // then this function must be called again so that the next layer can be freed
    if (nodes_count > 0) {

        
        // Create a Verstable set to track which nodes are used as types
        _type_tracking_tsm_clean_set used_as_type_set;
        _type_tracking_tsm_clean_set_init(&used_as_type_set);
        
        // Iteratively remove nodes layer by layer
        const int batch_size = 1000;
        struct cds_lfht_node* nodes_to_delete[batch_size];
        int layer = 0;

        rcu_read_lock();
        
        while (true) {
            int total_nodes = 0;
            int delete_count = 0;
            
            // Clear the used_as_type tracking set
            _type_tracking_tsm_clean_set_clear(&used_as_type_set);
            
            // Phase 1: Count nodes and identify which are used as types

            struct cds_lfht_iter iter;
            tklog_scope(bool not_at_iter_end = tsm_iter_first(GTSM_rcu, &iter));
            
            while (not_at_iter_end) {

                tklog_scope(struct tsm_base_node* base_node = tsm_iter_get_node(&iter));
                if (!base_node) {
                    tklog_warning("somehow base_node is NULL\n");
                    break;
                }
                total_nodes++;
                
                // Add this node's type to the used_as_type set
                struct tsm_key track_key;
                track_key.key_union = base_node->type_key_union;
                track_key.key_is_number = base_node->type_key_is_number;
                
                _type_tracking_tsm_clean_set_itr itr = _type_tracking_tsm_clean_set_insert(&used_as_type_set, track_key);
                if (_type_tracking_tsm_clean_set_is_end(itr)) {
                    tklog_error("Out of memory tracking used types\n");
                    // Continue anyway, will force cleanup
                }
                
                tklog_scope(not_at_iter_end = tsm_iter_next(GTSM_rcu, &iter));
            }
            
            // If no nodes left, we're done
            if (total_nodes == 0) {
                break;
            }
            
            // Phase 2: Collect nodes that are not used as types
            tklog_scope(not_at_iter_end = tsm_iter_first(GTSM_rcu, &iter));
            while (not_at_iter_end) {

                tklog_scope(struct tsm_base_node* base_node = tsm_iter_get_node(&iter));
                if (!base_node) {
                    tklog_warning("somehow base_node is NULL\n");
                    break;
                }

                // Check if this node is used as a type
                struct tsm_key lookup_key;
                lookup_key.key_union = base_node->key_union;
                lookup_key.key_is_number = base_node->key_is_number;
                
                _type_tracking_tsm_clean_set_itr found = _type_tracking_tsm_clean_set_get(&used_as_type_set, lookup_key);
                
                if (_type_tracking_tsm_clean_set_is_end(found)) {
                    // This node is not used as a type, mark for deletion
                    nodes_to_delete[delete_count++] = &base_node->lfht_node;
                }
                
                tklog_scope(not_at_iter_end = tsm_iter_next(GTSM_rcu, &iter));
            }
            
            // If no nodes to delete in this layer, force cleanup of remaining nodes
            if (delete_count == 0 && total_nodes > 0) {
                if (total_nodes == 1) {
                    tklog_scope(not_at_iter_end = tsm_iter_first(GTSM_rcu, &iter));
                    tklog_scope(struct tsm_base_node* base_node = tsm_iter_get_node(&iter));
                    if (base_node == NULL) {
                        tklog_error("somehow base_node is NULL\n");
                        rcu_read_unlock();
                        break;
                    }
                    nodes_to_delete[delete_count++] = &base_node->lfht_node;
                } else {
                    tklog_error("No progress made, meaning there are only nodes left which are used as types. There are %d nodes left\n", total_nodes);
                    gtsm_print();
                    rcu_read_unlock();
                    break;
                }
            }
            
            // Phase 3: try free callback for the collected nodes
            for (int i = 0; i < delete_count; i++) {
                struct tsm_base_node* base_node = caa_container_of(nodes_to_delete[i], struct tsm_base_node, lfht_node);
                
                // try free callback
                tklog_scope(int try_free_callback_result = tsm_node_try_free_callback(GTSM_rcu, base_node));
                if (try_free_callback_result == -1) {
                    tklog_error("tsm_node_try_free_callback failed for child node\n");
                    rcu_read_unlock();
                    return false;
                }
            }
            
            if (delete_count > 0) {
                tklog_info("loop %d: Cleaned up %d nodes\n", layer, delete_count);
                layer++;
            }
        }
        
        rcu_read_unlock();
        
        // Clean up tracking set
        _type_tracking_tsm_clean_set_cleanup(&used_as_type_set);
        
        tklog_info("Global data system cleanup completed (%d layers)\n", layer);
    } 
    
    // Schedule for RCU cleanup after successful removal
    call_rcu(&GTSM_rcu->rcu_head, _tsm_tsm_type_free_callback);

    return true;
}
bool gtsm_print() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }

    tklog_scope(tsm_base_node_print(GTSM_rcu));
    struct tsm* p_tsm = caa_container_of(GTSM_rcu, struct tsm, base);
    tklog_info("path to parent from global TSM:\n");
    for (uint32_t i = 0; i < p_tsm->path_length; i++) {
        if (p_tsm->path_from_global_to_parent_tsm[i].key_is_number) {
            tklog_info("%lld -> ", p_tsm->path_from_global_to_parent_tsm[i].key_union.number);
        } else {
            tklog_info("%s -> ", p_tsm->path_from_global_to_parent_tsm[i].key_union.string);
        }
    }


    bool success = true;
    rcu_read_lock();
    struct cds_lfht_iter* p_iter = calloc(1, sizeof(struct cds_lfht_iter));
    tklog_scope(tsm_iter_first(GTSM_rcu, p_iter));
    bool iter_next = p_iter != NULL;
    while (iter_next) {
        tklog_scope(struct tsm_base_node* iter_node = tsm_iter_get_node(p_iter));
        tklog_scope(bool print_result = tsm_node_print(GTSM_rcu, iter_node));
        if (!print_result) {
            success = false;
        }
        tklog_scope(iter_next = tsm_iter_next(GTSM_rcu, p_iter));
    }
    if (!p_iter) {
        tklog_error("p_iter should not be NULL\n");
    } else {
        free(p_iter);
    }
    rcu_read_unlock();

    return true;
}

int gtsm_node_try_free_callback(struct tsm_base_node* p_base) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("GTSM is NULL\n");
        return -1;
    }
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return -1;
    }
    tklog_scope(int try_free_callback_result = tsm_node_try_free_callback(GTSM, p_base));
    return try_free_callback_result;
}
bool gtsm_node_is_valid(struct tsm_base_node* p_base) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_node_is_valid(GTSM_rcu, p_base));
    return result;
}
bool gtsm_node_print(struct tsm_base_node* p_base) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_node_print(GTSM_rcu, p_base));
    return result;
}
struct tsm_base_node* gtsm_node_get(struct tsm_key key) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return NULL;
    }
    tklog_scope(struct tsm_base_node* result = tsm_node_get(GTSM_rcu, key));
    return result;
}
bool gtsm_node_insert(struct tsm_base_node* new_node) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_node_insert(GTSM_rcu, new_node));
    return result;
}
bool gtsm_node_update(struct tsm_base_node* new_node) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_node_update(GTSM_rcu, new_node));
    return result;
}
bool gtsm_node_upsert(struct tsm_base_node* new_node) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_node_upsert(GTSM_rcu, new_node));
    return result;
}
bool gtsm_node_is_deleted(struct tsm_base_node* node) {
    tklog_scope(bool result = tsm_node_is_deleted(node));
    return result;
}
unsigned long gtsm_nodes_count() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return 0;
    }
    tklog_scope(unsigned long result = tsm_nodes_count(GTSM_rcu));
    return result;
}

bool gtsm_iter_first(struct cds_lfht_iter* iter) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_iter_first(GTSM_rcu, iter));
    return result;
}
bool gtsm_iter_next(struct cds_lfht_iter* iter) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_iter_next(GTSM_rcu, iter));
    return result;
}
struct tsm_base_node* gtsm_iter_get_node(struct cds_lfht_iter* iter) {
    // This doesn't depend on GTSM directly
    tklog_scope(struct tsm_base_node* result = tsm_iter_get_node(iter));
    return result;
}
bool gtsm_iter_lookup(struct tsm_key key, struct cds_lfht_iter* iter) {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return false;
    }
    tklog_scope(bool result = tsm_iter_lookup(GTSM_rcu, key, iter));
    return result;
}