#include "tsm.h"
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
// #define TSM_DEBUG
static _Atomic uint64_t g_key_counter = 1; // 0 is invalid. 1 is the first valid key
static struct tsm_base_node* GTSM = NULL;
static struct tsm_base_node* GTSM_tmp = NULL;
static const struct tsm_key g_gtsm_key = { .key_union.string = "gtsm", .key_is_number = false };
static const struct tsm_key g_base_type_key = { .key_union.string = "base_type", .key_is_number = false };
static const struct tsm_key g_tsm_type_key  = { .key_union.string = "tsm_type",  .key_is_number = false };
// ==========================================================================================
// THREAD SAFE MAP
// ==========================================================================================

static int32_t _tsm_key_match(struct cds_lfht_node *node, const void *key) {
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
    tklog_info("    fn_is_valid: %p\n", p_type_node->fn_is_valid);
    tklog_info("    fn_print: %p\n", p_type_node->fn_print);
    tklog_info("    size of the node this node is type for: %d bytes\n", p_type_node->type_size_bytes);

    return true;
}

// TSM Type
static int32_t _tsm_tsm_type_free_children(struct tsm_base_node* p_tsm_base) {
    // first remove all non-types
    struct cds_lfht_iter iter;
    bool iter_valid = tsm_iter_first(p_tsm_base, &iter);
    while (iter_valid) {
        struct tsm_base_node* p_base = tsm_iter_get_node(&iter);
        // Advance BEFORE possible delete
        iter_valid = tsm_iter_next(p_tsm_base, &iter);
        if (!p_base) {
            tklog_warning("somehow p_base is NULL\n");
            continue;
        }
        if (!tsm_node_is_type(p_base)) {
            TSM_Result defer_free_result = tsm_node_defer_free(p_tsm_base, p_base);
            if (defer_free_result != TSM_SUCCESS) {
                tklog_error("tsm_node_defer_free failed for child node. it is %d\n", defer_free_result);
                return -1;
            }
        }
    }
    // now types - collect first to avoid iter invalidation
    while (true) {
        struct tsm_base_node* to_free[1024]; // Adjust size as needed or use dynamic allocation
        size_t to_free_count = 0;
        bool freed_something = false;
        iter_valid = tsm_iter_first(p_tsm_base, &iter);
        while (iter_valid) {
            struct tsm_base_node* p_base = tsm_iter_get_node(&iter);
            iter_valid = tsm_iter_next(p_tsm_base, &iter);
            if (!p_base) {
                tklog_warning("somehow p_base is NULL\n");
                continue;
            }
            if (!tsm_node_is_type(p_base)) {
                tklog_error("somehow found non type when there shouldnt be any more types left\n");
                return -1;
            }
            // since p_base is type then we want to check all nodes if this key is found as type key in any other node
            bool found_in_other_node = false;
            struct tsm_key base_key = { .key_union = p_base->key_union, .key_is_number = p_base->key_is_number };
            struct cds_lfht_iter iter_2;
            bool iter_2_valid = tsm_iter_first(p_tsm_base, &iter_2);
            while (iter_2_valid) {
                struct tsm_base_node* p_iter_base = tsm_iter_get_node(&iter_2);
                if (!p_iter_base) {
                    tklog_error("Somehow node is NULL\n");
                    return -1;
                }
                // it not found self then check if base is found as type in other node
                struct tsm_key other_key = { .key_union = p_iter_base->key_union, .key_is_number = p_iter_base->key_is_number };
                bool found_self = tsm_key_match(base_key, other_key);
                if (!found_self) {
                    struct tsm_key other_type_key = { .key_union = p_iter_base->type_key_union, .key_is_number = p_iter_base->type_key_is_number };
                    bool type_found_in_other_node = tsm_key_match(base_key, other_type_key);
                    if (type_found_in_other_node) {
                        found_in_other_node = true;
                        break;
                    }
                }
                iter_2_valid = tsm_iter_next(p_tsm_base, &iter_2);
            }
            // if node at this iteration isnt found in any other node then we can free it
            if (!found_in_other_node) {
                if (to_free_count >= 1024) {
                    tklog_error("Too many types to free in one pass\n");
                    return -1;
                }
                to_free[to_free_count++] = p_base;
            }
        }
        // Check for cycle or done
        if (to_free_count == 0) {
            uint32_t nodes_count = tsm_nodes_count(p_tsm_base);
            if (nodes_count > 0) {
                tklog_error("children nodes type dependencies are cyclical and hence in an invalid state\n");
                return -1;
            }
            break;
        }
        // Free collected
        for (size_t i = 0; i < to_free_count; ++i) {
            TSM_Result defer_free_result = tsm_node_defer_free(p_tsm_base, to_free[i]);
            if (defer_free_result != TSM_SUCCESS) {
                tklog_error("tsm_node_defer_free failed for child node. it is %d\n", defer_free_result);
                return -1;
            }
            freed_something = true;
        }
    }
    tklog_info("cleaning all children nodes for TSM completed. pointer: %p\n", p_tsm_base);
    return 1;
}
// this function must be called within a read section
static int32_t _tsm_tsm_type_free_children_2(struct tsm_base_node* p_tsm_base) {

    // first remove all types that arent a type
    struct cds_lfht_iter iter;
    tklog_scope(bool iter_valid = tsm_iter_first(p_tsm_base, &iter));
    while (iter_valid) {

        tklog_scope(struct tsm_base_node* p_base = tsm_iter_get_node(&iter));
        if (!p_base) {
            tklog_warning("somehow p_base is NULL\n");
            break;
        }

        // if not is not a type then its safe to delete right away
        if (!tsm_node_is_type(p_base)) {
            // try free callback
            tklog_scope(TSM_Result defer_free_result = tsm_node_defer_free(p_tsm_base, p_base));
            if (defer_free_result != TSM_SUCCESS) {
                tklog_error("tsm_node_defer_free failed for child node. it is %d\n", defer_free_result);
                return -1;
            }
        }
        
        tklog_scope(iter_valid = tsm_iter_next(p_tsm_base, &iter));
    }

    bool node_freed_since_iter_first = false; // if there is a cycle where no type node is removed, it means the types are cyclical and therefore impossible to remove
    tklog_scope(iter_valid = tsm_iter_first(p_tsm_base, &iter));
    while (iter_valid) {

        tklog_scope(struct tsm_base_node* p_base = tsm_iter_get_node(&iter));
        if (!p_base) {
            tklog_warning("somehow p_base is NULL\n");
            break;
        }
        if (!tsm_node_is_type(p_base)) {
            tklog_error("somehow found non type when there shouldnt be any more types left\n");
            return -1;
        }

        // since p_base is type then we want to check all nodes if this key is found as type key in any other node
        bool found_in_other_node = false;
        struct tsm_key base_key = { .key_union = p_base->key_union, .key_is_number = p_base->key_is_number };
        struct cds_lfht_iter iter_2;
        tklog_scope(bool iter_2_valid = tsm_iter_first(p_tsm_base, &iter_2));
        while (iter_2_valid) {

            tklog_scope(struct tsm_base_node* p_iter_base = tsm_iter_get_node(&iter_2));
            if (!p_iter_base) {
                tklog_error("Somehow node is NULL\n");
                return -1;
            }

            // it not found self then check if base is found as type in other node
            struct tsm_key other_key = { .key_union = p_iter_base->key_union, .key_is_number = p_iter_base->key_is_number };
            tklog_scope(bool found_self = tsm_key_match(base_key, other_key));
            if (!found_self) {
                struct tsm_key other_type_key = { .key_union = p_iter_base->type_key_union, .key_is_number = p_iter_base->type_key_is_number };
                tklog_scope(bool type_found_in_other_node = tsm_key_match(base_key, other_type_key));
                if (type_found_in_other_node) {
                    found_in_other_node = true;
                    break;
                }
            }

            tklog_scope(iter_2_valid = tsm_iter_next(p_tsm_base, &iter_2));
        }

        // if node at this iteration isnt found in any other node then we can free it
        // then break this while loop as well so that the first iterator can be reset 
        // because we need to exit rcu_read section so that the node is not found in the iteration again. IS THIS TRUE???
        if (!found_in_other_node) {
            // try free callback
            tklog_scope(int32_t defer_free_result = tsm_node_defer_free(p_tsm_base, p_base));
            if (defer_free_result != TSM_SUCCESS) {
                tklog_error("tsm_node_defer_free failed for child node. it is %d\n", defer_free_result);
                return -1;
            }
            node_freed_since_iter_first = true;
        }
        
        tklog_scope(iter_valid = tsm_iter_next(p_tsm_base, &iter));
        if (!iter_valid) {
            if (!node_freed_since_iter_first) {
                tklog_error("children nodes type dependencies are cyclical and hence in an invalid state\n");
                return -1;
            }
            tsm_iter_first(p_tsm_base, &iter);
            node_freed_since_iter_first = false;
            // if iter is still invalid then hash table is empty
            if (!iter_valid) {
                break;
            }
        }
    }
    
    tklog_info("cleaning all children nodes for TSM completed. pointer: %p\n", p_tsm_base);

    return 1;
}
static void _tsm_tsm_type_free_callback(struct rcu_head* rcu_head) {
    if (!rcu_head) {
        tklog_error("rcu_head is NULL\n");
        return;
    }

    struct tsm_base_node* p_base = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);

    rcu_read_lock();
    tklog_scope(_tsm_tsm_type_free_children(p_base));
    tklog_scope(uint32_t nodes_count = tsm_nodes_count(p_base));
    rcu_read_unlock();

    if (nodes_count > 0)
        tklog_error("tsm is not empty before freeing it\n");

    tsm_key_union_free(p_base->key_union, p_base->key_is_number);
    tsm_key_union_free(p_base->type_key_union, p_base->type_key_is_number);
    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    tklog_scope(bool free_path_result = tsm_path_free(p_tsm->path));
    if (!free_path_result)
        tklog_warning("tsm_free_path failed\n");
    cds_lfht_destroy(p_tsm->p_ht, NULL);
    free(p_base);
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
    tklog_scope(bool iter_valid = tsm_iter_first(p_base, p_iter));
    while (iter_valid) {
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
            p_iter = NULL;
            rcu_read_unlock();
            return false;
        }
        tklog_scope(iter_valid = tsm_iter_next(p_base, p_iter));
    }
    free(p_iter);
    p_iter = NULL;
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
    
    tklog_scope(tsm_path_print(p_tsm->path));

    rcu_read_lock();
    struct cds_lfht_iter* p_iter = calloc(1, sizeof(struct cds_lfht_iter));
    tklog_scope(tsm_iter_first(p_base, p_iter));
    bool iter_next = p_iter != NULL;
    while (iter_next) {
        tklog_scope(struct tsm_base_node* iter_node = tsm_iter_get_node(p_iter));
        if (!iter_node) {
            break;
        }
        tklog_scope(tsm_node_print(p_base, iter_node));
        tklog_scope(iter_next = tsm_iter_next(p_base, p_iter));
    }
    rcu_read_unlock();
    free(p_iter);
    p_iter = NULL;

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
            // number_key = rand() % 100000000;
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
            copied_string = NULL;
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
            key_union.string = NULL;
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
        tklog_info("key: %d\n", key.key_union.number);
    } else {
        tklog_info("key: %s\n", key.key_union.string);
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
    if (!p_base_copy) {
        tklog_warning("same p_base node no longer accessible\n");
        return false;
    }
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

    tklog_scope(bool is_removed = tsm_node_is_removed(p_base));
    if (is_removed) {
        tklog_debug("node is removed and therefore not valid\n");
        return false;
    }

    return true;
}
bool tsm_base_node_print(struct tsm_base_node* p_base) {

    tklog_scope(bool is_removed = tsm_node_is_removed(p_base));

    if (p_base->key_is_number) {
        tklog_info("key: %lld\n", p_base->key_union.number);
    } else {
        tklog_info("key: %s\n", p_base->key_union.string);
    }

    if (is_removed) {
        tklog_info("    is removed\n");
    } else {
        tklog_info("    not removed\n");
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
    p_base_type->fn_is_valid = fn_is_valid;
    p_base_type->fn_print = fn_print;
    p_base_type->type_size_bytes = type_size_bytes;

    return p_base;
}
// ==========================================================================================
// PATH
// ==========================================================================================
bool tsm_path_insert_key(struct tsm_path* p_path, struct tsm_key key, int32_t index) {
    if (!p_path) {
        tklog_error("p_path is NULL\n");
        return false;
    }
    int32_t length = (int32_t)p_path->length;
    if (index < -length - 1 || index > length) {
        tklog_error("index(%d) is out of bounds for path length(%d)\n", index, length);
        return false;
    }
    int32_t pos = index;
    if (pos < 0) {
        pos += length + 1;
    }
    struct tsm_key* new_array = realloc(p_path->from_gtsm_to_self, (length + 1) * sizeof(struct tsm_key));
    if (!new_array) {
        tklog_error("Failed to realloc path array\n");
        return false;
    }
    p_path->from_gtsm_to_self = new_array;
    memmove(&p_path->from_gtsm_to_self[pos + 1], &p_path->from_gtsm_to_self[pos], (length - pos) * sizeof(struct tsm_key));
    p_path->from_gtsm_to_self[pos] = key;
    p_path->length++;
    return true;
}
bool tsm_path_remove_key(struct tsm_path* p_path, int32_t index) {
    if (!p_path) {
        tklog_error("p_path is NULL\n");
        return false;
    }
    int32_t length = (int32_t)p_path->length;
    if (length == 0) {
        tklog_error("Cannot remove from empty path\n");
        return false;
    }
    if (index < -length || index >= length) {
        tklog_error("index(%d) is out of bounds for path length(%d)\n", index, length);
        return false;
    }
    int32_t pos = index;
    if (pos < 0) {
        pos += length;
    }
    tsm_key_free(&p_path->from_gtsm_to_self[pos]);
    memmove(&p_path->from_gtsm_to_self[pos], &p_path->from_gtsm_to_self[pos + 1], (length - pos - 1) * sizeof(struct tsm_key));
    p_path->length--;
    if (p_path->length > 0) {
        struct tsm_key* new_array = realloc(p_path->from_gtsm_to_self, p_path->length * sizeof(struct tsm_key));
        if (new_array) {
            p_path->from_gtsm_to_self = new_array;
        }
    } else {
        free(p_path->from_gtsm_to_self);
        p_path->from_gtsm_to_self = NULL;
    }
    return true;
}
struct tsm_path tsm_path_create(struct tsm_base_node* p_parent_tsm_base, struct tsm_base_node* p_base) {
    struct tsm_path null_path = { .from_gtsm_to_self = NULL, .length = 0 };
    if (!p_parent_tsm_base || !p_base) {
        tklog_error("p_parent_tsm_base or p_base is NULL\n");
        return null_path;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_parent_tsm_base));
    if (!is_tsm) {
        tklog_error("p_parent_tsm_base is not TSM\n");
        return null_path;
    }
#ifdef TSM_DEBUG
    if (!tsm_node_is_valid(p_parent_tsm_base, p_base)) {
        tklog_error("p_base is not valid\n");
        return null_path;
    }
#endif
    struct tsm* p_parent_tsm = caa_container_of(p_parent_tsm_base, struct tsm, base);
    struct tsm_path path = tsm_path_copy(p_parent_tsm->path);
    struct tsm_key base_key = { .key_union = p_base->key_union, .key_is_number = p_base->key_is_number };
    struct tsm_key base_key_copy = tsm_key_copy(base_key);
    if (!tsm_path_insert_key(&path, base_key_copy, -1)) {
        tklog_error("inserting key into path failed\n");
        tsm_key_free(&base_key_copy);
        tsm_path_free(path);
        return null_path;
    }
    return path;
}
bool tsm_path_free(struct tsm_path path) {
    if ((path.length == 0) != (path.from_gtsm_to_self == NULL)) {
        tklog_error("path.length(%u) inconsistent with from_gtsm_to_self(%p)\n", path.length, path.from_gtsm_to_self);
        return false;
    }
    tklog_scope(tsm_path_print(path));
    for (uint32_t i = 0; i < path.length; ++i) {
        tsm_key_free(&path.from_gtsm_to_self[i]);
    }
    if (path.from_gtsm_to_self) {
        tklog_info("freeing %p\n", path.from_gtsm_to_self);
        free(path.from_gtsm_to_self);
        path.from_gtsm_to_self = NULL;
    }
    return true;
}
bool tsm_path_print(struct tsm_path path) {
    if (path.length == 0) {
        tklog_info("path: (empty)\n");
        return true;
    }
    char full_str[512];
    char* ptr = full_str;
    size_t remaining = sizeof(full_str);
    bool result = true;
    for (uint32_t i = 0; i < path.length; ++i) {
        int len;
        if (path.from_gtsm_to_self[i].key_is_number) {
            len = snprintf(ptr, remaining, "%llu -> ", path.from_gtsm_to_self[i].key_union.number);
        } else {
            len = snprintf(ptr, remaining, "%s -> ", path.from_gtsm_to_self[i].key_union.string);
        }
        if (len < 0 || (size_t)len >= remaining) {
            result = false;
            break;
        }
        ptr += len;
        remaining -= len;
    }
    if (result && ptr >= full_str + 4) {
        ptr[-4] = '\0';  // Remove the last " -> "
    } else {
        result = false;
    }
    tklog_info("path: %s\n", full_str);
    return result;
}
struct tsm_path tsm_path_copy(struct tsm_path path_to_copy) {
    struct tsm_path null_path = { .from_gtsm_to_self = NULL, .length = 0 };
    if (path_to_copy.length == 0) {
        return null_path;
    }
    struct tsm_key* new_array = calloc(1, path_to_copy.length * sizeof(struct tsm_key));
    if (!new_array) {
        tklog_error("Failed to allocate memory for path copy\n");
        return null_path;
    }
    for (uint32_t i = 0; i < path_to_copy.length; ++i) {
        new_array[i] = tsm_key_copy(path_to_copy.from_gtsm_to_self[i]);
    }
    struct tsm_path copy = { .from_gtsm_to_self = new_array, .length = path_to_copy.length };
    return copy;
}
struct tsm_base_node* tsm_node_get_by_path(struct tsm_base_node* p_tsm_base, struct tsm_path path) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return NULL;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("p_tsm_base is not a TSM\n");
        return NULL;
    }
    struct tsm_base_node* current = p_tsm_base;
    for (uint32_t i = 0; i < path.length; ++i) {
        struct tsm_key key = path.from_gtsm_to_self[i];
        current = tsm_node_get(current, key);
        if (!current) {
            gtsm_print();
            tsm_path_print(path);
            tklog_error("Failed to get node at path index %u\n", i);
            return NULL;
        }
        tklog_scope(is_tsm = tsm_node_is_tsm(current));
        if (i < path.length - 1 && !is_tsm) {
            tklog_error("Intermediate node at path index %u is not a TSM\n", i);
            return NULL;
        }
    }
    return current;
}

// ==========================================================================================
// THREAD SAFE MAP
// ==========================================================================================
struct tsm_base_node* tsm_create(
    struct tsm_base_node* p_tsm_parent_base,
    struct tsm_key new_tsm_key) {

    // validating TSM
    if (!p_tsm_parent_base) {
        tklog_error("p_tsm_parent_base is NULL\n");
        return NULL;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_parent_base));
    if (!is_tsm) {
        tklog_warning("given tsm is not a TSM\n");
        return NULL;
    }
    struct tsm* p_tsm_parent = caa_container_of(p_tsm_parent_base, struct tsm, base);


    // create the new tsm node
    tklog_scope(struct tsm_key tsm_type_key_copy = tsm_key_copy(g_tsm_type_key));
    tklog_scope(struct tsm_base_node* p_new_tsm_base = tsm_base_node_create(new_tsm_key, tsm_type_key_copy, sizeof(struct tsm), false, true));
    if (!p_new_tsm_base) {
        tklog_error("Failed to create base node for new TSM\n");
        tsm_key_free(&tsm_type_key_copy);
        return NULL;
    }
    struct tsm* p_new_tsm = caa_container_of(p_new_tsm_base, struct tsm, base);
    p_new_tsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    if (!p_new_tsm->p_ht) {
        tklog_error("Failed to create lock free hash table\n");
        goto free_tsm;
    }

    tklog_scope(p_new_tsm->path = tsm_path_create(p_tsm_parent_base, p_new_tsm_base));

    // create base_type and insert it into new TSM
    tklog_scope(struct tsm_key base_type_key = tsm_key_copy(g_base_type_key));
    tklog_scope(struct tsm_base_node* p_new_base_type = tsm_base_type_node_create(
        base_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node)));
    if (!p_new_base_type) {
        tklog_error("Failed to create base_type node\n");
        tsm_base_node_free(p_new_base_type);
        goto free_tsm;
    }
    uint64_t hash = _tsm_hash_key(p_new_base_type->key_union, p_new_base_type->key_is_number);
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &base_type_key, &p_new_base_type->lfht_node));
    if (result != &p_new_base_type->lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        tsm_base_node_free(p_new_base_type);
        goto free_tsm;
    }


    // creating tsm_type now, as base_type is created, and insert it into new TSM
    tklog_scope(tsm_type_key_copy = tsm_key_copy(g_tsm_type_key));
    tklog_scope(struct tsm_base_node* p_new_tsm_type = tsm_base_type_node_create(
        tsm_type_key_copy, 
        sizeof(struct tsm_base_type_node),
        _tsm_tsm_type_free_callback,
        _tsm_tsm_type_is_valid,
        _tsm_tsm_type_print,
        sizeof(struct tsm)));
    if (!p_new_tsm_type) {
        tklog_error("Failed to create tsm_type\n");
        tsm_base_node_free(p_new_tsm_type);
        goto free_tsm;
    }
    hash = _tsm_hash_key(p_new_tsm_type->key_union, p_new_tsm_type->key_is_number);
    tklog_scope(result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &tsm_type_key_copy, &p_new_tsm_type->lfht_node));
    if (result != &p_new_tsm_type->lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        tsm_base_node_free(p_new_tsm_type);
        goto free_tsm;
    }
    
    #ifdef TSM_DEBUG
        tklog_scope(struct tsm_base_node* base_node = tsm_node_get(p_new_tsm_base, g_base_type_key));
        if (!base_node) {
            tklog_error("base_type node not found after insertion\n");
            goto free_tsm;
        }

        tklog_scope(struct tsm_base_node* tsm_node = tsm_node_get(p_new_tsm_base, g_tsm_type_key));
        if (!tsm_node) {
            tklog_error("tsm_type node not found after insertion\n");
            goto free_tsm;
        }
    #endif
    
    tklog_debug("Created new TSM and inserted base_type and tsm_type nodes inside it\n");

    return p_new_tsm_base;

    free_tsm:

    tklog_scope(TSM_Result defer_free_result = tsm_node_defer_free(p_tsm_parent_base, p_new_tsm_base));
    if (defer_free_result != TSM_SUCCESS) {
        tklog_error("tsm_node_defer_free for half created TSM failed\n");
    }
    return NULL;
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
TSM_Result tsm_node_insert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!new_node || !p_tsm_base) {
        tklog_error("new_node is NULL\n");
        return TSM_NULL_ARGUMENT;
    }
    if (new_node->key_union.number == 0) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("key number is 0\n");
        return TSM_KEY_INVALID;
    }
    if (new_node->type_key_union.number == 0) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("type_key number is 0\n");
        return TSM_KEY_INVALID;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("given p_tsm_base is not TSM\n");
        return TSM_NODE_NOT_TSM;
    }
    tklog_scope(is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_scope(tsm_node_print(p_tsm_base , new_node));
        tklog_error("given p_tsm_base is not TSM\n");
        return TSM_NODE_NOT_TSM;
    }

    struct tsm_key type_key = { .key_union = new_node->type_key_union, .key_is_number = new_node->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_node_base = tsm_node_get(p_tsm_base, type_key));

    if (!p_type_node_base) {
        tklog_scope(tsm_base_node_print(p_tsm_base));
        tklog_scope(tsm_base_node_print(new_node));
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("didnt find node_type");
        return TSM_NODE_NOT_FOUND;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_node_base, struct tsm_base_type_node, base);
    uint32_t type_size_bytes = p_type_node->type_size_bytes;

    if (type_size_bytes != new_node->this_size_bytes) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_scope(tsm_node_print(p_tsm_base, p_type_node_base));
        tklog_error("type_size_bytes(%d) != new_node->this_size_bytes(%d)", type_size_bytes, new_node->this_size_bytes);
        return TSM_NODE_SIZE_MISMATCH;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    uint64_t hash = _tsm_hash_key(new_node->key_union, new_node->key_is_number);
    struct tsm_key key = { .key_union = new_node->key_union, .key_is_number = new_node->key_is_number };
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_tsm->p_ht, hash, _tsm_key_match, &key, &new_node->lfht_node));
    
    if (result != &new_node->lfht_node) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_warning("Somehow node already exists\n");
        return TSM_NODE_ALREADY_EXISTS;
    }

    #ifdef TSM_DEBUG
        tklog_scope(bool is_valid_type_function_result = tsm_node_is_valid(p_tsm_base, new_node));
        if (!is_valid_type_function_result) {
            tklog_error("after inserting then running tsm_node_is_valid for the inserted node it returned false\n");
            return TSM_NODE_INVALID;
        }
    #endif

    if (new_node->key_is_number) {
        tklog_debug("Successfully inserted node with number key %llu\n", new_node->key_union.number);
    } else {
        tklog_debug("Successfully inserted node with string key %s\n", new_node->key_union.string);
    }

    return TSM_SUCCESS;
}
TSM_Result tsm_node_update(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return TSM_NULL_ARGUMENT;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return TSM_NULL_ARGUMENT;
    }
    if (new_node->key_union.number == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_KEY_INVALID;
    }
    if (new_node->type_key_union.number == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_KEY_INVALID;
    }

    tklog_scope(bool is_tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm_result) {
        tklog_error("given p_tsm_base node is not a TSM\n");
        return TSM_NODE_NOT_TSM;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Find the existing node
    uint64_t                hash = _tsm_hash_key(new_node->key_union, new_node->key_is_number);
    struct tsm_key key = { .key_union = new_node->key_union, .key_is_number = new_node->key_is_number};
    struct cds_lfht_iter    old_iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key, &old_iter);
    struct cds_lfht_node*   old_lfht_node = cds_lfht_iter_get_node(&old_iter);
    struct tsm_base_node*   old_node = caa_container_of(old_lfht_node, struct tsm_base_node, lfht_node);
    if (!old_node) {
        if (new_node->key_is_number)
            tklog_info("Cannot update - node with number key %llu not found\n", new_node->key_union.number);
        else
            tklog_info("Cannot update - node with string key %s not found\n", new_node->key_union.string);
        return TSM_NODE_NOT_FOUND;
    }
    if (old_node == new_node) {
        tklog_error("when replacing a node its not allowed to replace with the same node. given new_node == old_node\n");
        return TSM_NODE_REPLACING_SAME;
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
        return TSM_KEYS_NOT_SAME;
    }

    tklog_scope(struct tsm_base_node* p_type_base_node = tsm_node_get(p_tsm_base, old_type_key));
    if (!p_type_base_node) {
        tklog_error("Cannot find type node for existing node\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_TYPE_NOT_FOUND;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_type_base_node, struct tsm_base_type_node, base);
    
    // Verify data size matches expected type size 
    if (new_node->this_size_bytes != p_type->type_size_bytes) {
        tklog_error("new node size %u doesn't match current node size %u\n", new_node->this_size_bytes, p_type->type_size_bytes);
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_NODE_SIZE_MISMATCH;
    }

    // Get the free callback
    if (!p_type->fn_free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_NULL_FUNCTION_POINTER;
    }

    int32_t replace_result = cds_lfht_replace(p_tsm->p_ht, &old_iter, hash, _tsm_key_match, &key, &new_node->lfht_node);
    
    if (replace_result == -ENOENT) {
        tklog_notice("could not replace node because it was removed within the attempt of updating it\n");
        return TSM_NODE_ALREADY_REMOVED;
    } else if (replace_result != 0) {
        tklog_error("Failed to replace node in hash table\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_NODE_REPLACEMENT_FAILURE;
    }

    // Schedule old node for RCU cleanup
    tklog_scope(call_rcu(&old_node->rcu_head, p_type->fn_free_callback));
    if (new_node->key_is_number) {
        tklog_debug("Successfully updated node with number key %llu\n", new_node->key_union.number);
    } else {
        tklog_debug("Successfully updated node with string key %s\n", new_node->key_union.string);
    }
    return TSM_SUCCESS;
}
TSM_Result tsm_node_upsert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return TSM_NULL_ARGUMENT;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return TSM_NULL_ARGUMENT;
    }
    if (new_node->key_union.number == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_KEY_INVALID;
    }
    if (new_node->type_key_union.number == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_KEY_INVALID;
    }

    struct tsm_key key = { .key_union = new_node->key_union, .key_is_number = new_node->key_is_number };
    tklog_scope(struct tsm_base_node* old_node = tsm_node_get(p_tsm_base, key));

    if (old_node) {
        tklog_scope(TSM_Result update_result = tsm_node_update(p_tsm_base, new_node));
        if (update_result == TSM_NODE_NOT_FOUND) {
            tklog_warning("Node to update inside upsert function suddenly got deleted\n");
        } else if (update_result != TSM_SUCCESS && update_result != TSM_NODE_ALREADY_REMOVED) {
            tklog_error("update inside upsert function failed with code %d\n", update_result);
        }
        return update_result;
    } else {
        tklog_scope(TSM_Result insert_result = tsm_node_insert(p_tsm_base, new_node));
        if (insert_result != TSM_SUCCESS && insert_result != TSM_NODE_ALREADY_EXISTS) {
            tklog_error("insert function inside upsert function failed\n");
        }
        return insert_result;
    }
    return TSM_SUCCESS;
}
TSM_Result tsm_node_defer_free(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    if (!p_tsm_base || !p_base) {
        tklog_error("NULL arguments not valid\n");
        return TSM_NULL_ARGUMENT;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tsm_base_node_print(p_tsm_base);
        tklog_error("given p_tsm_base is not a TSM\n");
        TSM_NODE_NOT_TSM;
    }

    #ifdef TSM_DEBUG
        tklog_scope(bool is_valid = tsm_node_is_valid(p_base));
        if (!is_valid) {
            tklog_error("base node to defer free is not valid\n");
            return TSM_NODE_INVALID;
        }

        struct tsm_key base_key = { .key_union = p_base->key_union, .key_is_number = p_base->key_is_number };
        struct tsm_key base_type_key = { .key_union = p_base->type_key_union, .key_is_number = p_base->type_key_is_number };
        // if p_base is type and is not base type then we want to check all nodes if this key is found as type key in any other node
        if (tsm_node_is_type(p_base) && !tsm_key_match(base_key, base_type_key)) {
            struct cds_lfht_iter iter;
            tklog_scope(bool node_exists = tsm_iter_first(p_tsm_base, &iter));
            while (node_exists) {
                tklog_scope(struct tsm_base_node* p_iter_base = tsm_iter_get_node(&iter));
                if (!p_iter_base) {
                    tklog_error("Somehow node is NULL\n");
                    return TSM_NULL_ITER_NODE;
                }
                struct tsm_key other_key = { .key_union = p_iter_base->type_key_union, .key_is_number = p_iter_base->type_key_is_number };
                tklog_scope(bool type_found_in_other_node = tsm_key_match(base_key, other_key));
                if (type_found_in_other_node) {
                    tsm_node_print(p_tsm_base, p_base);
                    tsm_node_print(p_tsm_base, p_iter_base);
                    tklog_error("trying to free type node but it is found in other nodes\n");
                    return TSM_TSM_NOT_EMPTY;
                }
                tklog_scope(node_exists = tsm_iter_next(p_tsm_base, &iter));
            }
        }
    #endif

    // schdule node for free callback via call_rcu
    struct tsm_key type_key = { .key_union.string = p_base->type_key_union.string, .key_is_number = p_base->type_key_is_number };
    tklog_scope(struct tsm_base_node* p_type_base = tsm_node_get(p_tsm_base, type_key));
    if (!p_type_base) {
        gtsm_print();
        tsm_base_node_print(p_tsm_base);
        tsm_base_node_print(p_base);
        tklog_error("could not get type for node to free"); 
        return TSM_TYPE_NOT_FOUND;
    }
    struct tsm_base_type_node* p_type = caa_container_of(p_type_base, struct tsm_base_type_node, base);
    #ifdef TSM_DEBUG
        tklog_scope(bool is_valid = tsm_node_is_valid(p_type));
        if (!is_valid) {
            tklog_error("type node is not valid\n");
            return false;
        }
        if (!p_type->fn_free_callback) {
            tklog_scope(tsm_node_print(p_tsm_base, p_base));
            tklog_error("free_callback is NULL\n");
            return false;
        }
    #endif

    // Logically removing the node from the hashtable
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    int32_t del_result = cds_lfht_del(p_tsm->p_ht, &p_base->lfht_node);
    if (del_result == -ENOENT) {
        tklog_warning("node is already removed\n");
        return TSM_NODE_ALREADY_REMOVED;
    } else if (del_result != 0) {
        tklog_error("Failed to delete node thread safe map. del_result = %d\n", del_result);
        tklog_scope(tsm_node_print(p_tsm_base, p_base));
        return TSM_UNKNOWN; 
    }
    #ifdef TSM_DEBUG
        tklog_scope(bool is_removed = tsm_node_is_removed(p_base));
        if (!is_removed) {
            tklog_error("p_base was not removed\n");
            return TSM_NODE_NOT_REMOVED;
        }
    #endif

    // Schedule for RCU cleanup after successful removal
    call_rcu(&p_base->rcu_head, p_type->fn_free_callback);

    return TSM_SUCCESS;
}
bool tsm_node_is_removed(struct tsm_base_node* node) {
    if (!node) {
        tklog_warning("given node is NULL\n");
        return true; // NULL node is considered removed
    }
    tklog_scope(bool result = cds_lfht_is_node_deleted(&node->lfht_node));
    return result;
}
uint64_t tsm_nodes_count(struct tsm_base_node* p_tsm_base) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return 0;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return 0;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    long split_count_before, split_count_after;
    uint64_t count;
    
    // cds_lfht_count_nodes MUST be called from within read-side critical section
    // according to URCU specification
    cds_lfht_count_nodes(p_tsm->p_ht, &split_count_before, &count, &split_count_after);
    
    return count;
}
bool tsm_print(struct tsm_base_node* p_tsm_base) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return false;
    }
    tklog_scope(bool is_tsm = tsm_node_is_tsm(p_tsm_base));
    if (!is_tsm) {
        tklog_error("given p_tsm_base is not TSM\n");
        return false;
    }
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    struct cds_lfht_iter iter;
    tklog_scope(bool node_exists = tsm_iter_first(p_tsm_base, &iter));
    while (node_exists) {
        tklog_scope(struct tsm_base_node* p_base = tsm_iter_get_node(&iter));
        if (!p_base) {
            tklog_error("p_base is NULL\n");
            return false;
        }

        tklog_scope(tsm_node_print(p_tsm_base, p_base));

        tklog_scope(node_exists = tsm_iter_next(p_tsm_base, &iter));
    }

    return true;
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
    if (!p_tsm) {
        return false;
    }
    if (iter->node == NULL) {
        return false;
    }
    
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
    if (!iter) {
        tklog_error("iter became NULL which it never should be\n");
        return false;
    }
    if (iter->node == NULL) {
        return false;
    }
    
    return true;
}
struct tsm_base_node* tsm_iter_get_node(struct cds_lfht_iter* iter) {
    if (!iter) {
        tklog_error("tsm_iter_next called with NULL parameters or uninitialized system\n");
        return NULL;
    }
    if (iter->node == NULL) {
        tklog_warning("iter->node == NULL which means there is no node for this iter\n");
        return NULL;
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
TSM_Result gtsm_init() {

    rcu_read_lock();
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (GTSM_rcu) {
        tklog_info("GTSM is already initialized because its not NULL\n");
        rcu_read_unlock();
        return TSM_ALREADY_INITIALIZED;
    }
    rcu_read_unlock();

    // Register the node size function with urcu_safe  system
    urcu_safe_set_node_size_function(_tsm_get_node_size);
    // Register the node bounds functions with urcu_safe system
    urcu_safe_set_node_start_ptr_function(_tsm_get_node_start_ptr);

    // create the GTSM node
    tklog_scope(struct tsm_key gtsm_key = tsm_key_copy(g_gtsm_key));
    tklog_scope(struct tsm_key gtsm_type_key = tsm_key_copy(g_tsm_type_key));
    tklog_scope(struct tsm_base_node* p_new_gtsm_base = tsm_base_node_create(gtsm_key, gtsm_type_key, sizeof(struct tsm), false, true));
    if (!p_new_gtsm_base) {
        tsm_key_free(&gtsm_key);
        tsm_key_free(&gtsm_type_key);
        tklog_error("Failed to create base node for new TSM\n");
        return TSM_NODE_CREATION_FAILURE;
    }
    struct tsm* p_new_gtsm = caa_container_of(p_new_gtsm_base, struct tsm, base);
    p_new_gtsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    if (!p_new_gtsm->p_ht) {
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        tklog_error("Failed to create lock free hash table\n");
        return TSM_UNKNOWN;
    }
    p_new_gtsm->path.length = 0;
    p_new_gtsm->path.from_gtsm_to_self = NULL;


    // insert base_type into GTSM
    tklog_scope(struct tsm_key base_key = tsm_key_copy(g_base_type_key));
    tklog_scope(struct tsm_base_node* base_type_base = tsm_base_type_node_create(
        base_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node)));
    if (!base_type_base) {
        tsm_key_free(&base_key);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        tklog_error("Failed to create base_type node\n");
        rcu_read_unlock();
        return TSM_NODE_CREATION_FAILURE;
    }
    uint64_t hash = _tsm_hash_key(base_type_base->key_union, base_type_base->key_is_number);
    rcu_read_lock();
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_new_gtsm->p_ht, hash, _tsm_key_match, &base_key, &base_type_base->lfht_node));
    if (result != &base_type_base->lfht_node) {
        _tsm_base_type_node_free_callback(&base_type_base->rcu_head);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        tklog_error("Failed to insert base_type node into hash table\n");
        return TSM_NODE_INSERTION_FAILURE;
    }
    

    // insert tsm_type into GTSM
    tklog_scope(struct tsm_key tsm_type_key = tsm_key_copy(g_tsm_type_key));
    tklog_scope(struct tsm_base_node* tsm_type_base = tsm_base_type_node_create(
        tsm_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_tsm_type_free_callback,
        _tsm_tsm_type_is_valid,
        _tsm_tsm_type_print,
        sizeof(struct tsm)));
    if (!tsm_type_base) {
        tsm_key_free(&tsm_type_key);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        tklog_error("Failed to create base_type node\n");
        return TSM_NODE_CREATION_FAILURE;
    }
    hash = _tsm_hash_key(tsm_type_base->key_union, tsm_type_base->key_is_number);
    tklog_scope(result = cds_lfht_add_unique(p_new_gtsm->p_ht, hash, _tsm_key_match, &tsm_type_key, &tsm_type_base->lfht_node));
    if (result != &tsm_type_base->lfht_node) {
        _tsm_base_type_node_free_callback(&tsm_type_base->rcu_head);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        tklog_error("Failed to insert base_type node into hash table\n");
        return TSM_NODE_INSERTION_FAILURE;
    }

    // validating inserts
    #ifdef TSM_DEBUG
        tklog_scope(struct tsm_base_node* base_type_node = tsm_node_get(p_new_gtsm_base, g_base_type_key));
        if (!base_type_node) {
            _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
            rcu_read_unlock();
            tklog_error("base_type node not found after initialization\n");
            return TSM_NODE_NOT_FOUND;
        }

        tklog_scope(struct tsm_base_node* tsm_type_node = tsm_node_get(p_new_gtsm_base, g_tsm_type_key));
        if (!tsm_type_node) {
            _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
            rcu_read_unlock();
            tklog_error("base_type node not found after initialization\n");
            return TSM_NODE_NOT_FOUND;
        }
    #endif
        
    rcu_read_unlock();
    
    void* old_pointer = rcu_cmpxchg_pointer(&GTSM, NULL, p_new_gtsm_base);
    if (old_pointer != NULL) {
        tklog_notice("failed to swap pointer\n");
        return TSM_CMPXCHG_FAILURE;
    }

    tklog_debug("Created GTSM and inserted base_type and tsm_type into it\n");

    return TSM_SUCCESS;
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
    for (int32_t count = 0; count < 5; count++) {
        void* old_pointer = rcu_cmpxchg_pointer(&GTSM, GTSM_rcu, NULL);
        if (old_pointer == GTSM_rcu) { break; }
        if (count == 5) {
            tklog_notice("failed to swap pointer to NULL\n");
            return false;
        }
    }

    tklog_scope(bool is_tsm = tsm_node_is_tsm(GTSM_rcu));
    if (!is_tsm) {
        tklog_warning("GTSM is not a TSM\n");
        return false;
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

    tklog_info("Printing GTSM:\n");
    tklog_scope(tsm_base_node_print(GTSM_rcu));

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
        p_iter = NULL;
    }
    rcu_read_unlock();

    return true;
}