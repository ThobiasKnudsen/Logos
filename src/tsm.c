#include "tsm.h"
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
#define MAX_TYPES_IN_SAME_LAYER 256
#define TSM_DEBUG
static _Atomic uint64_t g_key_counter = 1; // 0 is invalid. 1 is the first valid key
static struct tsm_base_node* GTSM = NULL;
static const struct tsm_key g_gtsm_key = { .key_union.string = "gtsm", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_base_type_key = { .key_union.string = "base_type", .key_type = TSM_KEY_TYPE_STRING };
static const struct tsm_key g_tsm_type_key  = { .key_union.string = "tsm_type",  .key_type = TSM_KEY_TYPE_STRING };
// ==========================================================================================
// THREAD SAFE MAP
// ==========================================================================================

static int32_t _tsm_key_match(struct cds_lfht_node *node, const void *key) {
    struct tsm_base_node *base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    const struct tsm_key ctx_1 = { .key_union = base_node->key_union, .key_type = base_node->key_type };
    const struct tsm_key* p_ctx_2 = (const struct tsm_key *)key;
    return tsm_key_match(&ctx_1, p_ctx_2);
}
static uint64_t _tsm_hash_key(union tsm_key_union key_union, uint8_t key_type) {
    if (key_type == TSM_KEY_TYPE_UINT64) {
        return XXH3_64bits(&key_union.uint64, sizeof(uint64_t));
    } else {
        const char* string = key_union.string;
        uint64_t string_len = strlen(string);
        return XXH3_64bits(string, string_len);
    }
}
static uint64_t _tsm_get_node_size(struct cds_lfht_node* node) {
    CM_ASSERT(node != NULL);
    struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    return base_node->this_size_bytes;
}
static void* _tsm_get_node_start_ptr(struct cds_lfht_node* node) {
    CM_ASSERT(node != NULL);
    struct tsm_base_node* base_node = caa_container_of(node, struct tsm_base_node, lfht_node);
    return (void*)base_node;
}

// Base Type
static void _tsm_base_type_node_free_callback(struct rcu_head* rcu_head) {
    CM_ASSERT(rcu_head != NULL);
    struct tsm_base_node* p_base = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(p_base));
}
static CM_RES _tsm_base_type_node_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {

    CM_ASSERT(p_base!=NULL && p_tsm_base!=NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));
    CM_ASSERT(CM_RES_TSM_NODE_IS_TYPE == tsm_node_is_type(p_base));
    CM_ASSERT(CM_RES_TSM_NODE_IS_VALID == tsm_base_node_is_valid(p_tsm_base, p_base));

    // Basic validation - check if it's a node type node
    struct tsm_base_type_node* p_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    if (!p_type->fn_free_callback) {
        CM_LOG_DEBUG("p_type->fn_free_callback is NULL\n");
        return CM_RES_NULL_FUNCTION_POINTER;
    }
    if (!p_type->fn_is_valid) {
        CM_LOG_DEBUG("p_type->fn_is_valid is NULL\n");
        return CM_RES_NULL_FUNCTION_POINTER;
    }
    if (!p_type->fn_print) {
        CM_LOG_DEBUG("p_type->fn_print is NULL\n");
        return CM_RES_NULL_FUNCTION_POINTER;
    }
    if (p_type->type_size_bytes < sizeof(struct tsm_base_node)) {
        CM_LOG_DEBUG("node_type->type_size_bytes(%d) < sizeof(struct tsm_base_node)(%ld)\n", p_type->type_size_bytes, sizeof(struct tsm_base_node));
        return CM_RES_TSM_NODE_SIZE_MISMATCH;
    }

    return CM_RES_TSM_NODE_IS_VALID;
}
static CM_RES _tsm_base_type_node_print(const struct tsm_base_node* p_base) {

    CM_ASSERT(p_base != NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TYPE == tsm_node_is_type(p_base)); 
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_print(p_base));

    struct tsm_base_type_node* p_type_node = caa_container_of(p_base, struct tsm_base_type_node, base);
    (void)p_type_node;
    CM_LOG_TSM_PRINT("    fn_free_callback: %p\n", p_type_node->fn_free_callback);
    CM_LOG_TSM_PRINT("    fn_is_valid: %p\n", p_type_node->fn_is_valid);
    CM_LOG_TSM_PRINT("    fn_print: %p\n", p_type_node->fn_print);
    CM_LOG_TSM_PRINT("    size of the node this node is type for: %d bytes\n", p_type_node->type_size_bytes);

    return CM_RES_SUCCESS;
}

// TSM Type
static CM_RES _tsm_tsm_type_free_children(const struct tsm_base_node* p_tsm_base);
static CM_RES _tsm_tsm_type_free_children(const struct tsm_base_node* p_tsm_base) {
    CM_ASSERT(p_tsm_base != NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    
    CM_SCOPE(tsm_print(p_tsm_base));

    // first remove all TSMs
    struct cds_lfht_iter iter;
    CM_SCOPE(CM_RES iter_valid = tsm_iter_first(p_tsm_base, &iter));
    while (iter_valid == CM_RES_SUCCESS) {
        const struct tsm_base_node* p_base = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_iter_get_node(&iter, &p_base));
        // Advance BEFORE possible delete
        CM_SCOPE(iter_valid = tsm_iter_next(p_tsm_base, &iter));
        CM_ASSERT(p_base != NULL);
        CM_SCOPE(CM_RES cm_is_tsm = tsm_node_is_tsm(p_base));
        if (cm_is_tsm == CM_RES_TSM_NODE_IS_TSM) {
            CM_ASSERT(CM_RES_SUCCESS == _tsm_tsm_type_free_children(p_base));
            CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm_base, p_base));
        }
        else if (cm_is_tsm != CM_RES_TSM_NODE_NOT_TSM) {
            CM_LOG_ERROR("tsm_node_is_tsm() failed with code %d\n", cm_is_tsm);
            return cm_is_tsm;
        }
    }
    CM_ASSERT(iter_valid == CM_RES_TSM_ITER_END);

    // Then remove all non-types and non-TSMs
    CM_SCOPE(iter_valid = tsm_iter_first(p_tsm_base, &iter));
    while (iter_valid == CM_RES_SUCCESS) {
        const struct tsm_base_node* p_base = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_iter_get_node(&iter, &p_base));
        // Advance BEFORE possible delete
        CM_SCOPE(iter_valid = tsm_iter_next(p_tsm_base, &iter));
        CM_ASSERT(p_base != NULL);
        CM_SCOPE(CM_RES cm_res = tsm_node_is_type(p_base));
        if (cm_res != CM_RES_TSM_NODE_IS_TYPE) {
            CM_ASSERT(cm_res == CM_RES_TSM_NODE_NOT_TYPE);
            CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm_base, p_base));
        }
    }
    CM_ASSERT(iter_valid == CM_RES_TSM_ITER_END);
    // now types - collect first to avoid iter invalidation
    while (true) {
        const struct tsm_base_node* to_free[MAX_TYPES_IN_SAME_LAYER]; // Adjust size as needed or use dynamic allocation
        size_t to_free_count = 0;
        CM_SCOPE(iter_valid = tsm_iter_first(p_tsm_base, &iter));
        while (iter_valid == CM_RES_SUCCESS) {
            const struct tsm_base_node* p_base = NULL;
            CM_ASSERT(CM_RES_SUCCESS == tsm_iter_get_node(&iter, &p_base));
            CM_SCOPE(iter_valid = tsm_iter_next(p_tsm_base, &iter));
            CM_ASSERT(p_base != NULL);
            CM_ASSERT(CM_RES_TSM_NODE_IS_TYPE == tsm_node_is_type(p_base));
            // since p_base is type then we want to check all nodes if this key is found as type key in any other node
            bool found_in_other_node = false;
            struct tsm_key base_key = { .key_union = p_base->key_union, .key_type = p_base->key_type };
            struct cds_lfht_iter iter_2;
            CM_SCOPE(CM_RES iter_2_valid = tsm_iter_first(p_tsm_base, &iter_2));
            while (iter_2_valid == CM_RES_SUCCESS) {
                const struct tsm_base_node* p_iter_base = NULL;
                CM_ASSERT(CM_RES_SUCCESS == tsm_iter_get_node(&iter_2, &p_iter_base));
                CM_SCOPE(tsm_base_node_print(p_iter_base));
                // it not found self then check if base is found as type in other node
                struct tsm_key other_key = { .key_union = p_iter_base->key_union, .key_type = p_iter_base->key_type };
                bool found_self = CM_RES_TSM_KEYS_MATCH == tsm_key_match(&base_key, &other_key);
                if (!found_self) {
                    struct tsm_key other_type_key = { .key_union = p_iter_base->type_key_union, .key_type = p_iter_base->type_key_type };
                    bool type_found_in_other_node = CM_RES_TSM_KEYS_MATCH == tsm_key_match(&base_key, &other_type_key);
                    if (type_found_in_other_node) {
                        found_in_other_node = true;
                        break;
                    }
                }
                CM_SCOPE(iter_2_valid = tsm_iter_next(p_tsm_base, &iter_2));
            }
            CM_ASSERT(iter_2_valid == CM_RES_SUCCESS || iter_2_valid == CM_RES_TSM_ITER_END);
            // if node at this iteration isnt found in any other node then we can free it
            if (!found_in_other_node) {
                CM_ASSERT(to_free_count < MAX_TYPES_IN_SAME_LAYER);
                to_free[to_free_count++] = p_base;
            }
        }
        CM_ASSERT(iter_valid == CM_RES_TSM_ITER_END);

        // Check for cycle or done
        if (to_free_count == 0) {
            uint64_t nodes_count = 0;
            CM_ASSERT(CM_RES_SUCCESS == tsm_nodes_count(p_tsm_base, &nodes_count));
            CM_ASSERT(nodes_count == 0);
            break;
        }
        // Free collected
        for (size_t i = 0; i < to_free_count; ++i) {
            CM_SCOPE(tsm_base_node_print(to_free[i]));
            CM_SCOPE(tsm_print(p_tsm_base));
            const struct tsm_key maybe_base_type_key = { .key_union = to_free[i]->key_union, .key_type = to_free[i]->key_type };
            CM_SCOPE(CM_RES cm_res = tsm_key_match(&maybe_base_type_key, &g_base_type_key));
            if (cm_res == CM_RES_TSM_KEYS_MATCH) {
                CM_ASSERT(to_free_count == 1); // should only one node left to free which is base type
                CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm_base, to_free[i])); // remove the last node
                uint64_t nodes_count = 0; // check that there is no nodes left
                CM_ASSERT(CM_RES_SUCCESS == tsm_nodes_count(p_tsm_base, &nodes_count));
                CM_ASSERT(nodes_count == 0);
            } else if (CM_RES_TSM_KEYS_DONT_MATCH) {
                CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm_base, to_free[i]));   
            } else {
                CM_LOG_ERROR("tsm_key_match failed with code %d\n", cm_res);
            }
        }
    }
    CM_LOG_DEBUG("cleaning all children nodes for TSM completed. pointer: %p\n", p_tsm_base);
    return CM_RES_SUCCESS;
}
static void _tsm_tsm_type_free_callback(struct rcu_head* rcu_head) {
    CM_ASSERT(rcu_head != NULL);

    struct tsm_base_node* p_base = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_base));

    rcu_read_lock();
    CM_ASSERT(CM_RES_SUCCESS == _tsm_tsm_type_free_children(p_base));
    uint64_t nodes_count;
    CM_ASSERT(CM_RES_SUCCESS == tsm_nodes_count(p_base, &nodes_count));
    CM_ASSERT(nodes_count == 0);
    rcu_read_unlock();

    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    CM_ASSERT(CM_RES_SUCCESS == tsm_path_free(&p_tsm->path));
    CM_ASSERT(0 == cds_lfht_destroy(p_tsm->p_ht, NULL));
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(p_base));
}
static CM_RES _tsm_tsm_type_is_valid(const struct tsm_base_node* p_parent_tsm_base, const struct tsm_base_node* p_tsm_base) {
    CM_ASSERT(p_tsm_base!=NULL && p_parent_tsm_base!=NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_parent_tsm_base));
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm_key tmp_key = { .key_union = p_tsm_base->key_union, .key_type = p_tsm_base->key_type };
    const struct tsm_base_node* p_tmp_tsm_base = NULL; 
    CM_SCOPE(CM_RES cm_res = tsm_node_get(p_parent_tsm_base, &tmp_key, &p_tmp_tsm_base));
    if (cm_res != CM_RES_SUCCESS) {
        CM_LOG_WARNING("tsm_node_get returned code %d\n", cm_res);
        return cm_res;
    }

    struct cds_lfht_iter iter;
    CM_SCOPE(CM_RES iter_valid = tsm_iter_first(p_tsm_base, &iter));
    while (iter_valid == CM_RES_SUCCESS) {
        const struct tsm_base_node* iter_node = NULL;
        CM_SCOPE(CM_RES cm_res = tsm_iter_get_node(&iter, &iter_node));
        if (cm_res != CM_RES_SUCCESS)
            return cm_res;
        CM_ASSERT(iter_node != NULL);
        CM_SCOPE(CM_RES is_valid = tsm_node_is_valid(p_tsm_base, iter_node));
        CM_ASSERT(  is_valid == CM_RES_TSM_NODE_IS_VALID ||
                    is_valid == CM_RES_TSM_NODE_NOT_FOUND ||
                    is_valid == CM_RES_TSM_NODE_NOT_FOUND_SELF ||
                    is_valid == CM_RES_TSM_NODE_IS_REMOVED);
        CM_SCOPE(iter_valid = tsm_iter_next(p_tsm_base, &iter));
    }
    CM_ASSERT(iter_valid == CM_RES_TSM_ITER_END);

    return CM_RES_TSM_NODE_IS_VALID;
}
static CM_RES _tsm_tsm_type_print(const struct tsm_base_node* p_base) {
    CM_ASSERT(p_base != NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_base));

    tsm_base_node_print(p_base); // this is 
    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    tsm_path_print(&p_tsm->path);

    struct cds_lfht_iter iter;
    CM_SCOPE(CM_RES iter_valid = tsm_iter_first(p_base, &iter));
    while (iter_valid == CM_RES_SUCCESS) {
        const struct tsm_base_node* iter_node = NULL;
        CM_SCOPE(CM_RES cm_res = tsm_iter_get_node(&iter, &iter_node));
        if (cm_res != CM_RES_SUCCESS) {
            return cm_res;
        }
        CM_ASSERT(iter_node != NULL);
        cm_res = tsm_node_print(p_base, iter_node);
        if (cm_res != CM_RES_SUCCESS) {
            return cm_res;
        }
        iter_valid = tsm_iter_next(p_base, &iter);
    }
    CM_ASSERT(iter_valid == CM_RES_TSM_ITER_END);

    return CM_RES_SUCCESS;
}

// ==========================================================================================
// PUBLIC 
// ==========================================================================================

// ==========================================================================================
// KEY
// ==========================================================================================
CM_RES tsm_key_union_string_create(const char* string_key, union tsm_key_union* p_output_key) {
    CM_ASSERT(string_key != NULL && p_output_key != NULL);

    // Validate and copy string key
    size_t key_len = strlen(string_key) + 1;
    CM_ASSERT(key_len > 1);
    CM_ASSERT(key_len <= MAX_STRING_KEY_LEN);

    char* copied_string = calloc(1, key_len);
    CM_ASSERT(copied_string != NULL);

    CM_ASSERT(strncpy(copied_string, string_key, key_len - 1) != NULL);
    copied_string[key_len - 1] = '\0'; // Ensure null termination

    p_output_key->string = copied_string;
    return CM_RES_SUCCESS;
}
CM_RES tsm_key_union_uint64_create(uint64_t number_key, union tsm_key_union* p_output_key) {
    CM_ASSERT(p_output_key != NULL);
    if (number_key == 0) {
        number_key = atomic_fetch_add(&g_key_counter, 1);
    }
    p_output_key->uint64 = number_key;
    return CM_RES_SUCCESS;
}
CM_RES tsm_key_union_free(union tsm_key_union key_union, uint8_t key_type) {
    CM_ASSERT(key_union.uint64 != 0);
    if (key_type == TSM_KEY_TYPE_UINT64) {
        return CM_RES_SUCCESS;
    } else {
        CM_ASSERT(key_union.string != NULL);
        free(key_union.string);
        key_union.string = NULL;
        return CM_RES_SUCCESS;
    }
}
CM_RES tsm_key_uint64_create(uint64_t number_key, struct tsm_key* p_output_key) {
    CM_ASSERT(p_output_key != NULL);
    CM_SCOPE(CM_RES cm_res = tsm_key_union_uint64_create(number_key, &p_output_key->key_union));
    if (cm_res != CM_RES_SUCCESS) {
        p_output_key->key_type = TSM_KEY_TYPE_NONE;
        return cm_res;
    }
    p_output_key->key_type = TSM_KEY_TYPE_UINT64;
    return CM_RES_SUCCESS;
}
CM_RES tsm_key_string_create(const char* p_string, struct tsm_key* p_output_key) {
    CM_ASSERT(p_string != NULL && p_output_key != NULL);
    CM_SCOPE(CM_RES cm_res = tsm_key_union_string_create(p_string, &p_output_key->key_union));
    if (cm_res != CM_RES_SUCCESS) {
        p_output_key->key_type = TSM_KEY_TYPE_NONE;
        return cm_res;
    }
    p_output_key->key_type = TSM_KEY_TYPE_STRING;
    return CM_RES_SUCCESS;
}
CM_RES tsm_key_copy(const struct tsm_key* p_key, struct tsm_key* p_output_key) {
    CM_ASSERT(p_key != NULL && p_output_key != NULL);
    if (p_key->key_type == TSM_KEY_TYPE_UINT64) {
        CM_ASSERT(p_key->key_union.uint64 != 0);
        CM_SCOPE(CM_RES cm_res = tsm_key_uint64_create(p_key->key_union.uint64, p_output_key));
        if (cm_res != CM_RES_SUCCESS)
            return cm_res;
    } else if (p_key->key_type == TSM_KEY_TYPE_STRING) {
        CM_ASSERT(p_key->key_union.string != NULL);
        CM_SCOPE(CM_RES cm_res = tsm_key_string_create(p_key->key_union.string, p_output_key));
        if (cm_res != CM_RES_SUCCESS)
            return cm_res;
    } else {
        CM_LOG_ERROR("key type is invlaid. the type is %d\n", p_key->key_type);
    }
    return CM_RES_SUCCESS;
}
CM_RES tsm_key_free(struct tsm_key* key) {
    CM_ASSERT(key != NULL);
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_union_free(key->key_union, key->key_type));
    key->key_union.uint64 = 0;
    key->key_type = TSM_KEY_TYPE_NONE;
    return CM_RES_SUCCESS;
}
CM_RES tsm_key_match(const struct tsm_key* p_key_1, const struct tsm_key* p_key_2) {
    CM_ASSERT(p_key_1 != NULL && p_key_2 != NULL);
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key_1));
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key_2));
    // if not both is string or both is number
    if (p_key_1->key_type != p_key_2->key_type)
        return CM_RES_TSM_KEYS_DONT_MATCH;
    // if both are number
    if (p_key_1->key_type == TSM_KEY_TYPE_UINT64) 
        if (p_key_1->key_union.uint64 == p_key_2->key_union.uint64) {
            return CM_RES_TSM_KEYS_MATCH;
        } else {
            return CM_RES_TSM_KEYS_DONT_MATCH;
        }
    // if both are string
    else {
        const char* string_1 = p_key_1->key_union.string;
        const char* string_2 = p_key_2->key_union.string;
        if (strcmp(string_1, string_2) == 0) {
            return CM_RES_TSM_KEYS_MATCH;
        } else {
            return CM_RES_TSM_KEYS_DONT_MATCH;
        }
    }
}
CM_RES tsm_key_print(const struct tsm_key* p_key) {
    CM_ASSERT(p_key != NULL);
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    if (p_key->key_type == TSM_KEY_TYPE_UINT64) {
        CM_LOG_TSM_PRINT("key: %lu\n", p_key->key_union.uint64);
    } else if (p_key->key_type == TSM_KEY_TYPE_STRING) {
        CM_LOG_TSM_PRINT("key: %s\n", p_key->key_union.string);
    } else {
        CM_LOG_TSM_PRINT("key is parent\n");
    }
    return CM_RES_SUCCESS;
}
CM_RES tsm_key_is_valid(const struct tsm_key* p_key) {
    if (!p_key) {
        CM_LOG_NOTICE("tsm_key_is_valid(): NULL argument\n");
        return CM_RES_NULL_ARGUMENT;
    }
    if (p_key->key_type == TSM_KEY_TYPE_UINT64) {
        if (p_key->key_union.uint64 == 0) {
            CM_LOG_NOTICE("tsm_key_is_valid(): uint64 is 0 when type is uint64\n");
            return CM_RES_TSM_KEY_NOT_VALID;
        }
    } else if (p_key->key_type == TSM_KEY_TYPE_STRING) {
        if (p_key->key_union.string == NULL) {
            CM_LOG_NOTICE("tsm_key_is_valid(): string is NULL\n");
            return CM_RES_TSM_KEY_NOT_VALID;
        }
    } else if (p_key->key_type == TSM_KEY_TYPE_PARENT) {
        if (p_key->key_union.uint64 != 0) {
            CM_LOG_NOTICE("tsm_key_is_valid(): uint64 is not 0 when type is parent\n");
            return CM_RES_TSM_KEY_NOT_VALID;
        }
    } else {
        CM_LOG_NOTICE("tsm_key_is_valid(): invalid key type %d\n", p_key->key_type);
        return CM_RES_TSM_KEY_NOT_VALID;
    }
    return CM_RES_TSM_KEY_IS_VALID;
}

// ==========================================================================================
// BASE NODE
// ==========================================================================================
CM_RES _tsm_base_node_create(
                const struct tsm_key* p_key, 
                const struct tsm_key* p_type_key, 
                uint32_t this_size_bytes, 
                bool this_is_type, 
                bool this_is_tsm, 
                struct tsm_base_node** pp_output_node) {

    CM_ASSERT(p_key != NULL && p_type_key != NULL && pp_output_node != NULL);
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_type_key));
    CM_ASSERT(this_size_bytes >= sizeof(struct tsm_base_node));
    struct tsm_base_node* p_node = calloc(1, this_size_bytes);
    CM_ASSERT(p_node != NULL);
    cds_lfht_node_init(&p_node->lfht_node);

    struct tsm_key key_copy = {0};
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(p_key, &key_copy));
    struct tsm_key type_key_copy = {0};
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(p_type_key, &type_key_copy));
    
    // Handle key assignment - copy string keys, use number keys directly
    if (key_copy.key_type == TSM_KEY_TYPE_UINT64) {
        p_node->key_union.uint64 = key_copy.key_union.uint64;
    } else {
        // String key is already copied by tsm_key_union_create
        p_node->key_union.string = key_copy.key_union.string;
    }
    
    // Handle type_key assignment - copy string keys, use number keys directly
    if (type_key_copy.key_type == TSM_KEY_TYPE_UINT64) {
        p_node->type_key_union.uint64 = type_key_copy.key_union.uint64;
    } else {
        // String type_key is already copied by tsm_key_union_create
        p_node->type_key_union.string = type_key_copy.key_union.string;
    }
    
    p_node->key_type = key_copy.key_type;
    p_node->type_key_type = type_key_copy.key_type;
    p_node->this_size_bytes = this_size_bytes;
    p_node->this_is_type = this_is_type;
    p_node->this_is_tsm = this_is_tsm;
    *pp_output_node = p_node;
    return CM_RES_SUCCESS;
}
CM_RES tsm_base_node_create(
                const struct tsm_key* p_key, 
                const struct tsm_key* p_type_key, 
                uint32_t this_size_bytes, 
                struct tsm_base_node** pp_output_node) {
    CM_SCOPE(CM_RES res = _tsm_base_node_create(p_key, p_type_key, this_size_bytes, false, false, pp_output_node));
    return res;
}  
CM_RES tsm_base_node_free(struct tsm_base_node* p_base_node) {
    CM_ASSERT(p_base_node != NULL);
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_union_free(p_base_node->key_union, p_base_node->key_type));
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_union_free(p_base_node->type_key_union, p_base_node->type_key_type));
    memset(p_base_node, 0, sizeof(struct tsm_base_node));
    free(p_base_node);

    return CM_RES_SUCCESS;
}
CM_RES tsm_base_node_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    
    CM_ASSERT(p_base != NULL && p_tsm_base != NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm_key key = { .key_union = p_base->key_union, .key_type = p_base->key_type};
    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type};
    CM_SCOPE(CM_RES cm_res = tsm_key_is_valid(&key));
    if (cm_res != CM_RES_TSM_KEY_IS_VALID)
        return cm_res;
    CM_SCOPE(cm_res = tsm_key_is_valid(&type_key));
    if (cm_res != CM_RES_TSM_KEY_IS_VALID)
        return cm_res;

    const struct tsm_base_node* p_base_copy = NULL;
    CM_SCOPE(cm_res = tsm_node_get(p_tsm_base, &key, &p_base_copy));
    if (cm_res != CM_RES_SUCCESS)
        return cm_res;
    if (p_base != p_base_copy) {
        CM_LOG_NOTICE("tsm_base_node_is_valid: given p_base and p_base gotten from key in given p_base is not the same\n");
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_print(p_tsm_base, p_base));
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_print(p_tsm_base, p_base_copy));
        return CM_RES_TSM_NODE_NOT_FOUND_SELF;
    }

    const struct tsm_base_node* p_base_type = NULL;
    CM_SCOPE(cm_res = tsm_node_get(p_tsm_base, &type_key, &p_base_type));
    if (cm_res != CM_RES_SUCCESS) {
        CM_LOG_NOTICE("tsm_base_node_is_valid: did not find type for given base node\n");
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_print(p_tsm_base, p_base));
        return cm_res;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    if (p_base->this_size_bytes != p_type->type_size_bytes) {
        CM_LOG_NOTICE("tsm_base_node_is_valid: p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)", p_base->this_size_bytes, p_type->type_size_bytes);
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_print(p_tsm_base, p_base));
        return CM_RES_TSM_NODE_SIZE_MISMATCH;
    }

    CM_SCOPE(CM_RES is_removed = tsm_node_is_removed(p_base));
    if (is_removed != CM_RES_TSM_NODE_NOT_REMOVED) {
        CM_ASSERT(is_removed == CM_RES_TSM_NODE_IS_REMOVED);
        CM_LOG_NOTICE("tsm_base_node_is_valid: node is removed and therefore not valid\n");
        return is_removed;
    }

    return CM_RES_TSM_NODE_IS_VALID;
}
CM_RES tsm_base_node_print(const struct tsm_base_node* p_base) {
    CM_ASSERT(p_base != NULL);

    if (p_base->key_type == TSM_KEY_TYPE_UINT64) {
        CM_LOG_TSM_PRINT("key: %lu\n", p_base->key_union.uint64);
    } else {
        CM_LOG_TSM_PRINT("key: %s\n", p_base->key_union.string);
    }

    if (p_base->type_key_type == TSM_KEY_TYPE_UINT64) {
        CM_LOG_TSM_PRINT("    type_key: %lu\n", p_base->type_key_union.uint64);
    } else {
        CM_LOG_TSM_PRINT("    type_key: %s\n", p_base->type_key_union.string);
    }

    CM_LOG_TSM_PRINT("    size: %d bytes\n", p_base->this_size_bytes);

    return CM_RES_SUCCESS;
}

// ==========================================================================================
// BASE TYPE
// ==========================================================================================
CM_RES tsm_base_type_node_create(
    const struct tsm_key* p_key, 
    uint32_t this_size_bytes,
    void (*fn_free_callback)(struct rcu_head*),
    CM_RES (*fn_is_valid)(const struct tsm_base_node*, const struct tsm_base_node*),
    CM_RES (*fn_print)(const struct tsm_base_node*),
    uint32_t type_size_bytes,
    struct tsm_base_node** pp_output_node) {

    CM_ASSERT(p_key && fn_free_callback && fn_is_valid && fn_print && pp_output_node);
    *pp_output_node = NULL;
    CM_ASSERT(this_size_bytes >= sizeof(struct tsm_base_type_node));
    CM_ASSERT(fn_free_callback != NULL);
    CM_ASSERT(fn_is_valid != NULL);
    CM_ASSERT(fn_print != NULL);
    
    struct tsm_base_node* p_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == _tsm_base_node_create(p_key, &g_base_type_key, sizeof(struct tsm_base_type_node), true, false, &p_base));

    struct tsm_base_type_node* p_base_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    p_base_type->fn_free_callback = fn_free_callback;
    p_base_type->fn_is_valid = fn_is_valid;
    p_base_type->fn_print = fn_print;
    p_base_type->type_size_bytes = type_size_bytes;

    *pp_output_node = p_base;

    return CM_RES_SUCCESS;
}
// ==========================================================================================
// PATH
// ==========================================================================================
CM_RES tsm_path_insert_key(struct tsm_path* p_path, const struct tsm_key* p_key, int32_t index) {
    CM_ASSERT(p_path && p_key);
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    int32_t length = (int32_t)p_path->length;
    CM_ASSERT(index >= -length - 1 && index <= length);
    if (length == 0) {
        CM_ASSERT(p_path->key_chain == NULL);
    } else {
        CM_ASSERT(p_path->key_chain != NULL);
    }
    int32_t pos = index;
    if (pos < 0) {
        pos += length + 1;
    }
    struct tsm_key* new_array = realloc(p_path->key_chain, (length + 1) * sizeof(struct tsm_key));
    CM_ASSERT(new_array != NULL);
    p_path->key_chain = new_array;
    memmove(&p_path->key_chain[pos + 1], &p_path->key_chain[pos], (length - pos) * sizeof(struct tsm_key));
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(p_key, &p_path->key_chain[pos])); // copy key so that ownership isnt moved
    p_path->length++;
    return CM_RES_SUCCESS;
}
CM_RES tsm_path_remove_key(struct tsm_path* p_path, int32_t index) {
    CM_ASSERT(p_path != NULL);
    int32_t length = (int32_t)p_path->length;
    CM_ASSERT(index >= -length && index < length);

    int32_t pos = index;
    if (pos < 0) {
        pos += length;
    }
    tsm_key_free(&p_path->key_chain[pos]);
    memmove(&p_path->key_chain[pos], &p_path->key_chain[pos + 1], (length - pos - 1) * sizeof(struct tsm_key));
    p_path->length--;
    if (p_path->length > 0) {
        struct tsm_key* new_array = realloc(p_path->key_chain, p_path->length * sizeof(struct tsm_key));
        CM_ASSERT(new_array != NULL);
        p_path->key_chain = new_array;
    } else {
        free(p_path->key_chain);
        p_path->key_chain = NULL;
    }
    return CM_RES_SUCCESS;
}
CM_RES tsm_path_is_valid(const struct tsm_path* p_path) {
    CM_ASSERT(p_path != NULL);
    if ((p_path->length == 0) != (p_path->key_chain == NULL)) {
        CM_LOG_INFO("p_path->length is %d and p_path->key_chain is %p\n", p_path->length, p_path->key_chain);
        return CM_RES_TSM_PATH_INVALID;
    }
    for (uint32_t i = 0; i < p_path->length; ++i) {
        CM_SCOPE(CM_RES key_valid = tsm_key_is_valid(&p_path->key_chain[i]));
        if (key_valid != CM_RES_TSM_KEY_IS_VALID) {
            CM_LOG_INFO("key in index %d is invalid\n", i);
            return key_valid;
        }
    }
    return CM_RES_TSM_PATH_VALID;
}
CM_RES tsm_path_free(struct tsm_path* p_path) {
    CM_ASSERT(p_path != NULL);
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_path));
    CM_ASSERT(CM_RES_SUCCESS == tsm_path_print(p_path));
    for (uint32_t i = 0; i < p_path->length; ++i) {
        CM_ASSERT(CM_RES_SUCCESS == tsm_key_free(&p_path->key_chain[i]));
    }
    if (p_path->key_chain) {
        free(p_path->key_chain);
    }
    p_path->key_chain = NULL;
    p_path->length = 0;
    return CM_RES_SUCCESS;
}
CM_RES tsm_path_print(const struct tsm_path* p_path) {
    CM_ASSERT(p_path != NULL);
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_path));
    if (p_path->length == 0) {
        CM_LOG_INFO("p_path: (empty)\n");
        return CM_RES_SUCCESS;
    }
    char full_str[1024];
    char* ptr = full_str;
    size_t remaining = sizeof(full_str);
    CM_RES result = CM_RES_SUCCESS;
    for (uint32_t i = 0; i < p_path->length; ++i) {
        int len;
        if (p_path->key_chain[i].key_type == TSM_KEY_TYPE_UINT64) {
            len = snprintf(ptr, remaining, "%lu -> ", p_path->key_chain[i].key_union.uint64);
        } else {
            len = snprintf(ptr, remaining, "%s -> ", p_path->key_chain[i].key_union.string);
        }
        if (len < 0 || (size_t)len >= remaining) {
            result = CM_RES_BUFFER_OVERFLOW;
            break;
        }
        ptr += len;
        remaining -= len;
    }
    if (ptr >= full_str + 4) {
        ptr[-4] = '\0';  // Remove the last " -> "
    } else {
        result = CM_RES_UNKNOWN;
    }
    CM_LOG_DEBUG("path: %s\n", full_str);
    return result;
}
CM_RES tsm_path_copy(const struct tsm_path* p_path_to_copy, struct tsm_path* p_output_path) {
    CM_ASSERT(p_path_to_copy != NULL && p_output_path != NULL);
    CM_SCOPE(CM_RES cm_res = tsm_path_is_valid(p_path_to_copy));
    if (cm_res != CM_RES_TSM_PATH_VALID) {
        return cm_res;
    }
    p_output_path->key_chain = NULL;
    p_output_path->length = 0;
    if (p_path_to_copy->length == 0) {
        return CM_RES_SUCCESS;
    }
    struct tsm_key* new_array = calloc(1, p_path_to_copy->length * sizeof(struct tsm_key));
    CM_ASSERT(new_array != NULL);
    for (uint32_t i = 0; i < p_path_to_copy->length; ++i) {
        CM_SCOPE(cm_res = tsm_key_copy(&p_path_to_copy->key_chain[i], &new_array[i]));
        if (cm_res != CM_RES_SUCCESS) {
            free(new_array);
            return cm_res;
        }
    }
    p_output_path->key_chain = new_array;
    p_output_path->length = p_path_to_copy->length;
    CM_SCOPE(cm_res = tsm_path_is_valid(p_output_path));
    if (cm_res != CM_RES_TSM_PATH_VALID) {
        free(new_array);
        return cm_res;
    }
    return CM_RES_SUCCESS;
}
CM_RES tsm_path_get_key_ref(const struct tsm_path* p_path, int32_t index, const struct tsm_key** pp_key_ref) {
    CM_ASSERT(pp_key_ref && p_path);
    *pp_key_ref = NULL;
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_path));
    CM_ASSERT(p_path->length != 0);
    int32_t pos = index;
    if (pos < 0) {
        pos += (int32_t)p_path->length;
    }
    CM_ASSERT(pos >= 0 && pos < (int32_t)p_path->length);
    *pp_key_ref = &p_path->key_chain[pos];
    return CM_RES_SUCCESS;
}
CM_RES tsm_path_create_between_paths(const struct tsm_path* p_path_1, const struct tsm_path* p_path_2, struct tsm_path* p_output_path) {
    CM_ASSERT(p_output_path && p_path_1 && p_path_2);
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_path_1));
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_path_2));
    p_output_path->key_chain = NULL;
    p_output_path->length = 0;

    uint32_t len1 = p_path_1->length;
    uint32_t len2 = p_path_2->length;
    uint32_t common_len = 0;
    uint32_t min_len = len1 < len2 ? len1 : len2;
    while (common_len < min_len) {
        CM_RES match = tsm_key_match(&p_path_1->key_chain[common_len], &p_path_2->key_chain[common_len]);
        if (match != CM_RES_TSM_KEYS_MATCH) {
            break;
        }
        common_len++;
    }

    uint32_t ups_needed = len1 - common_len;
    uint32_t downs_needed = len2 - common_len;
    uint32_t total_keys = ups_needed + downs_needed;
    if (total_keys == 0) {
        // Same path, empty relative path
        return CM_RES_SUCCESS;
    }

    struct tsm_key* new_array = calloc(1, total_keys * sizeof(struct tsm_key));
    CM_ASSERT(new_array != NULL);

    uint32_t idx = 0;
    // Add ups (parent keys)
    for (uint32_t i = 0; i < ups_needed; ++i) {
        // Create a parent key: type PARENT, union uint64=0 (dummy)
        CM_RES res = tsm_key_union_uint64_create(0, &new_array[idx].key_union);
        if (res != CM_RES_SUCCESS) {
            // Note: tsm_key_union_uint64_create allows 0 for special cases
            new_array[idx].key_union.uint64 = 0;
        }
        new_array[idx].key_type = TSM_KEY_TYPE_PARENT;
        idx++;
    }
    // Add downs (suffix of p_path_2)
    for (uint32_t i = common_len; i < len2; ++i) {
        CM_RES res = tsm_key_copy(&p_path_2->key_chain[i], &new_array[idx]);
        if (res != CM_RES_SUCCESS) {
            for (uint32_t j = 0; j < idx; ++j) {
                tsm_key_free(&new_array[j]);
            }
            free(new_array);
            return res;
        }
        idx++;
    }

    p_output_path->key_chain = new_array;
    p_output_path->length = total_keys;

    CM_SCOPE(CM_RES valid_out = tsm_path_is_valid(p_output_path));
    if (valid_out != CM_RES_TSM_PATH_VALID) {
        tsm_path_free(p_output_path);
        return valid_out;
    }
    return CM_RES_SUCCESS;
}
CM_RES tsm_path_insert_path(const struct tsm_path* p_src_path, struct tsm_path* p_dst_path, int32_t index) {
    CM_ASSERT(p_dst_path && p_src_path);
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_src_path));
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_dst_path));

    uint32_t dst_len = p_dst_path->length;
    uint32_t src_len = p_src_path->length;
    if (src_len == 0) {
        // Inserting empty path does nothing
        return CM_RES_SUCCESS;
    }
    CM_ASSERT(index >= -(int32_t)dst_len - 1 && index <= (int32_t)dst_len);

    int32_t pos = index;
    if (pos < 0) {
        pos += dst_len + 1;
    }

    // Allocate new array for dst_len + src_len
    struct tsm_key* new_array = realloc(p_dst_path->key_chain, (dst_len + src_len) * sizeof(struct tsm_key));
    CM_ASSERT(new_array != NULL);
    p_dst_path->key_chain = new_array;

    // Shift the tail
    memmove(&p_dst_path->key_chain[pos + src_len], &p_dst_path->key_chain[pos],
            (dst_len - pos) * sizeof(struct tsm_key));

    // Copy src keys into the gap
    for (uint32_t i = 0; i < src_len; ++i) {
        CM_RES res = tsm_key_copy(&p_src_path->key_chain[i], &p_dst_path->key_chain[pos + i]);
        if (res != CM_RES_SUCCESS) {
            // Rollback: shift back and free new keys
            for (uint32_t j = 0; j < i; ++j) {
                tsm_key_free(&p_dst_path->key_chain[pos + j]);
            }
            memmove(&p_dst_path->key_chain[pos], &p_dst_path->key_chain[pos + src_len],
                    (dst_len - pos) * sizeof(struct tsm_key));
            // Realloc back if needed, but for simplicity, leave it (size will be larger, but ok)
            p_dst_path->length = dst_len;
            return res;
        }
    }

    p_dst_path->length += src_len;
    return CM_RES_SUCCESS;
}
CM_RES tsm_path_length(const struct tsm_path* p_path, uint32_t* p_output_length) {
    CM_ASSERT(p_path && p_output_length);
    *p_output_length = p_path->length;
    return CM_RES_SUCCESS;
}
// ==========================================================================================
// NODE
// ==========================================================================================
CM_RES tsm_node_get(const struct tsm_base_node* p_tsm_base, const struct tsm_key* p_key, const struct tsm_base_node** pp_output_node) {


    CM_ASSERT(p_tsm_base && p_key && pp_output_node);
    *pp_output_node = NULL;
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash internally
    uint64_t hash = _tsm_hash_key(p_key->key_union, p_key->key_type);
    CM_TIMER_START();
    struct cds_lfht_iter iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, p_key, &iter);
    struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(&iter);
    CM_TIMER_STOP();
    
    if (!lfht_node) {
        CM_LOG_INFO("node is not found because lfht_node = NULL");
        return CM_RES_TSM_NODE_NOT_FOUND;
    }

    struct tsm_base_node* result = caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
    *pp_output_node = result;
    return CM_RES_SUCCESS;
}
CM_RES _tsm_node_get_mutable(const struct tsm_base_node* p_tsm_base, const struct tsm_key* p_key, struct tsm_base_node** pp_output_node) {
    CM_ASSERT(p_tsm_base && p_key && pp_output_node);
    *pp_output_node = NULL;
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash internally
    uint64_t hash = _tsm_hash_key(p_key->key_union, p_key->key_type);
    struct cds_lfht_iter iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, p_key, &iter);
    struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(&iter);
    
    if (!lfht_node) {
        CM_LOG_INFO("node is not found because lfht_node = NULL");
        return CM_RES_TSM_NODE_NOT_FOUND;
    }

    struct tsm_base_node* result = caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
    *pp_output_node = result;
    return CM_RES_SUCCESS;
}
CM_RES tsm_node_get_by_path(const struct tsm_base_node* p_tsm_base, const struct tsm_path* p_path, const struct tsm_base_node** pp_output_node) {
    CM_ASSERT(p_tsm_base && p_path && pp_output_node);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));
    CM_ASSERT(!((p_path->key_chain == NULL) ^ (p_path->length == 0)));

    CM_TIMER_START();
    const struct tsm_base_node* current = p_tsm_base;
    for (uint32_t i = 0; i < p_path->length; ++i) {
        struct tsm_key key = p_path->key_chain[i];
        const struct tsm_base_node* p_new = NULL;
        CM_RES cm_res = tsm_node_get(current, &key, &p_new);
        if (cm_res != CM_RES_SUCCESS) {
            CM_SCOPE(CM_RES print_result = tsm_path_print(p_path));
            if (print_result != CM_RES_SUCCESS) {
                CM_LOG_WARNING("tsm_path_print failed with code %d\n", print_result);
            }
            CM_TIMER_STOP();
            return cm_res;
        }
        current = p_new;
        CM_SCOPE(cm_res = tsm_node_is_tsm(current));
        if (i < p_path->length - 1 && cm_res != CM_RES_TSM_NODE_IS_TSM) {
            CM_LOG_WARNING("Intermediate node at p_path index %u is not a TSM\n", i);
            CM_TIMER_STOP();
            return cm_res;
        }
    }
    *pp_output_node = current;
    CM_TIMER_STOP();

    return CM_RES_SUCCESS;
}
CM_RES tsm_node_get_by_path_at_depth(const struct tsm_base_node* p_tsm_base, const struct tsm_path* p_path, int depth, const struct tsm_base_node** pp_output_node) {
    CM_ASSERT(p_tsm_base && p_path && pp_output_node);
    *pp_output_node = NULL;
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));
    CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(p_path));
    uint32_t path_len = p_path->length;  // Use uint32_t for consistency
    CM_ASSERT(path_len != 0 || depth == 0);
    if (depth < 0) {
        depth += (int)path_len + 1;  // Matches original intent: -1 -> path_len
    }
    CM_ASSERT(depth <= (int)path_len);
    CM_TIMER_START();
    const struct tsm_base_node* current = p_tsm_base;
    uint32_t steps = (uint32_t)depth;
    for (uint32_t i = 0; i < steps; ++i) {  // 0-based: i=0 to <steps
        struct tsm_key key = p_path->key_chain[i];
        const struct tsm_base_node* p_new = NULL;
        CM_SCOPE(CM_RES cm_res = tsm_node_get(current, &key, &p_new));
        if (cm_res != CM_RES_SUCCESS) {
            CM_SCOPE(CM_RES print_result = tsm_path_print(p_path));
            if (print_result != CM_RES_SUCCESS) {
                CM_LOG_WARNING("tsm_path_print failed with code %d\n", print_result);
            }
            CM_TIMER_STOP();
            return cm_res;
        }
        CM_SCOPE(cm_res = tsm_node_is_tsm(p_new));
        if (i < steps - 1 && cm_res != CM_RES_TSM_NODE_IS_TSM) {  // Align with first function
            CM_LOG_WARNING("Intermediate node at p_path index %u is not a TSM\n", i);  // Changed to error for consistency
            CM_TIMER_STOP();
            return CM_RES_TSM_NODE_NOT_FOUND;
        }
        current = p_new;
    }
    *pp_output_node = current;
    CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}
CM_RES tsm_node_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    CM_ASSERT(p_base && p_tsm_base);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));
    CM_TIMER_START();

    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type };
    const struct tsm_base_node* p_base_type = NULL;
    CM_SCOPE(CM_RES cm_res = tsm_node_get(p_tsm_base, &type_key, &p_base_type));
    if (cm_res != CM_RES_SUCCESS) {
        CM_LOG_NOTICE("tried to get type node but got NULL for node: \n");
        CM_TIMER_STOP();
        return cm_res;
    }

    CM_SCOPE(struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base));
    uint32_t size_1 = p_base->this_size_bytes;
    uint32_t size_2 = p_type->type_size_bytes;
    if (size_1 != size_2) {
        CM_SCOPE(p_type->fn_print(p_base));
        CM_LOG_NOTICE("p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)\n", p_base->this_size_bytes, p_type->type_size_bytes);
        CM_TIMER_STOP();
        return CM_RES_TSM_NODE_SIZE_MISMATCH;
    }

    CM_SCOPE(cm_res = p_type->fn_is_valid(p_tsm_base, p_base));
    if (cm_res != CM_RES_TSM_NODE_IS_VALID) {
        CM_LOG_WARNING("fn_is_valid for node didnt return valid. code: %d\n", cm_res);
        CM_SCOPE(tsm_node_print(p_tsm_base, p_base));
    }

    CM_TIMER_STOP();
    return cm_res;
}
CM_RES tsm_node_is_tsm(const struct tsm_base_node* p_base) {
    CM_ASSERT(p_base);
    if (!p_base->this_is_tsm) {
        return CM_RES_TSM_NODE_NOT_TSM;
    }
    return CM_RES_TSM_NODE_IS_TSM;
}
CM_RES tsm_node_is_type(const struct tsm_base_node* p_base) {
    CM_ASSERT(p_base)
    if (p_base->this_is_type) {
        return CM_RES_TSM_NODE_IS_TYPE;;
    } else {
        CM_LOG_DEBUG("node is not a type node\n");
        return CM_RES_TSM_NODE_NOT_TYPE;
    }
}
CM_RES tsm_node_print(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    CM_ASSERT(p_tsm_base && p_base);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    CM_TIMER_START();

    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type };
    const struct tsm_base_node* p_base_type = NULL;
    CM_SCOPE(CM_RES cm_res = tsm_node_get(p_tsm_base, &type_key, &p_base_type));
    if (cm_res != CM_RES_SUCCESS) {
        CM_TIMER_STOP();
        return CM_RES_TSM_NODE_NOT_FOUND;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    CM_SCOPE(cm_res = p_type->fn_print(p_base));

    CM_TIMER_STOP();

    return cm_res;
}
CM_RES tsm_node_insert(const struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    CM_ASSERT(new_node && p_tsm_base);
    CM_ASSERT(new_node->key_union.uint64 != 0);
    CM_ASSERT(new_node->type_key_union.uint64 != 0);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    CM_TIMER_START();

    struct tsm_key type_key = { .key_union = new_node->type_key_union, .key_type = new_node->type_key_type };
    const struct tsm_base_node* p_type_node_base = NULL;
    CM_SCOPE(CM_RES cm_res = tsm_node_get(p_tsm_base, &type_key, &p_type_node_base));
    if (cm_res != CM_RES_SUCCESS) {
        tsm_print(p_tsm_base);
        tsm_base_node_print(p_tsm_base);
        tsm_key_print(&type_key);
        CM_TIMER_STOP();
        return cm_res;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_node_base, struct tsm_base_type_node, base);
    uint32_t type_size_bytes = p_type_node->type_size_bytes;
    CM_ASSERT(type_size_bytes == new_node->this_size_bytes);

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    uint64_t hash = _tsm_hash_key(new_node->key_union, new_node->key_type);
    struct tsm_key key = { .key_union = new_node->key_union, .key_type = new_node->key_type };
    CM_SCOPE(struct cds_lfht_node* result = cds_lfht_add_unique(p_tsm->p_ht, hash, _tsm_key_match, &key, &new_node->lfht_node));
    
    if (result != &new_node->lfht_node) {
        CM_SCOPE(tsm_node_print(p_tsm_base, new_node));
        CM_LOG_WARNING("node already exists\n");
        CM_TIMER_STOP();
        return CM_RES_TSM_NODE_EXISTS;
    }

    #ifdef TSM_DEBUG
        CM_SCOPE(cm_res = tsm_node_is_valid(p_tsm_base, new_node));
        if (cm_res == CM_RES_TSM_NODE_IS_REMOVED || cm_res == CM_RES_TSM_NODE_NOT_FOUND || cm_res == CM_RES_TSM_NODE_NOT_FOUND_SELF) {
            CM_LOG_WARNING("Node inserted then immedietaly removed\n");
            return CM_RES_TSM_NODE_IS_REMOVED;
        }
        CM_ASSERT(cm_res == CM_RES_TSM_NODE_IS_VALID);
    #endif

    if (new_node->key_type == TSM_KEY_TYPE_UINT64) {
        CM_LOG_DEBUG("Successfully inserted node with number key %lu\n", new_node->key_union.uint64);
    } else {
        CM_LOG_DEBUG("Successfully inserted node with string key %s\n", new_node->key_union.string);
    }

    CM_TIMER_STOP();

    return CM_RES_SUCCESS;
}
CM_RES tsm_node_update(const struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    CM_ASSERT(p_tsm_base && new_node);
    CM_ASSERT(new_node->key_union.uint64 != 0);
    CM_ASSERT(new_node->type_key_union.uint64 != 0);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    CM_TIMER_START();

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Find the existing node
    uint64_t                hash = _tsm_hash_key(new_node->key_union, new_node->key_type);
    struct tsm_key key = { .key_union = new_node->key_union, .key_type = new_node->key_type};
    struct cds_lfht_iter    old_iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key, &old_iter);
    struct cds_lfht_node*   old_lfht_node = cds_lfht_iter_get_node(&old_iter);
    struct tsm_base_node*   old_node = caa_container_of(old_lfht_node, struct tsm_base_node, lfht_node);
    if (!old_node) {
        if (new_node->key_type == TSM_KEY_TYPE_UINT64) {
            CM_LOG_INFO("Cannot update - node with number key %lu not found\n", new_node->key_union.uint64);
        } else {
            CM_LOG_INFO("Cannot update - node with string key %s not found\n", new_node->key_union.string);
        }
        CM_TIMER_STOP();
        return CM_RES_TSM_NODE_NOT_FOUND;
    }
    CM_ASSERT(old_node != new_node);
 
    // Get the type information
    struct tsm_key old_type_key = { .key_union = old_node->type_key_union, .key_type = old_node->type_key_type };
    struct tsm_key new_type_key = { .key_union = new_node->type_key_union, .key_type = new_node->type_key_type };
    CM_SCOPE(CM_RES cm_res = tsm_key_match(&old_type_key, &new_type_key));
    if (cm_res != CM_RES_TSM_KEYS_MATCH) {
        CM_TIMER_STOP();
        return cm_res;
    }

    const struct tsm_base_node* p_type_base_node = NULL;
    CM_SCOPE(cm_res = tsm_node_get(p_tsm_base, &old_type_key, &p_type_base_node));
    if (cm_res != CM_RES_SUCCESS) {
        CM_TIMER_STOP();
        return cm_res;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_type_base_node, struct tsm_base_type_node, base);
    
    CM_ASSERT(new_node->this_size_bytes == p_type->type_size_bytes); // Verify data size matches expected type size 
    CM_ASSERT(p_type->fn_free_callback); // Get the free callback

    int32_t replace_result = cds_lfht_replace(p_tsm->p_ht, &old_iter, hash, _tsm_key_match, &key, &new_node->lfht_node);
    
    if (replace_result == -ENOENT) {
        CM_LOG_NOTICE("could not replace node because it was removed within the attempt of updating it\n");
        CM_TIMER_STOP();
        return CM_RES_TSM_NODE_IS_REMOVED;
    } else if (replace_result != 0) {
        CM_LOG_ERROR("Failed to replace node in hash table\n");
        return CM_RES_TSM_NODE_REPLACEMENT_FAILURE;
    }

    // Schedule old node for RCU cleanup
    CM_SCOPE(call_rcu(&old_node->rcu_head, p_type->fn_free_callback));
    if (new_node->key_type == TSM_KEY_TYPE_UINT64) {
        CM_LOG_DEBUG("Successfully updated node with number key %lu\n", new_node->key_union.uint64);
    } else {
        CM_LOG_DEBUG("Successfully updated node with string key %s\n", new_node->key_union.string);
    }
    CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}
CM_RES tsm_node_upsert(const struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    CM_ASSERT(p_tsm_base && new_node);

    CM_TIMER_START();

    struct tsm_key key = { .key_union = new_node->key_union, .key_type = new_node->key_type };
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(&key));
    struct tsm_key type_key = { .key_union = new_node->type_key_union, .key_type = new_node->type_key_type };
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(&type_key));
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    uint64_t hash = _tsm_hash_key(new_node->key_union, new_node->key_type);
    struct cds_lfht_node *old_lfht_node = cds_lfht_add_replace(p_tsm->p_ht, hash, _tsm_key_match, &key, &new_node->lfht_node);
    struct tsm_base_node *old_node = NULL;
    if (old_lfht_node != NULL) {
        old_node = caa_container_of(old_lfht_node, struct tsm_base_node, lfht_node);
        struct tsm_key old_node_type_key = { .key_union = old_node->type_key_union, .key_type = old_node->type_key_type };
        const struct tsm_base_node* p_type_old_node_base = NULL;
        CM_SCOPE(CM_RES cm_res = tsm_node_get(p_tsm_base, &old_node_type_key, &p_type_old_node_base));
        if (cm_res != CM_RES_SUCCESS) {
            CM_TIMER_STOP();
            return cm_res;
        }
        struct tsm_base_type_node* p_type_node = caa_container_of(p_type_old_node_base, struct tsm_base_type_node, base);
        CM_ASSERT(p_type_node->fn_free_callback);
        // Use the type's free callback for old node (since types match)
        CM_SCOPE(call_rcu(&old_node->rcu_head, p_type_node->fn_free_callback));
        CM_LOG_DEBUG("Successfully updated node through upsert\n");
    } else {
        CM_LOG_DEBUG("Successfully inserted node through upsert\n");
    }

#ifdef TSM_DEBUG
    CM_SCOPE(CM_RES cm_res = tsm_node_is_valid(p_tsm_base, new_node));
    if (cm_res != CM_RES_TSM_NODE_IS_VALID) {
        CM_LOG_WARNING("after upsert then running tsm_node_is_valid for the node it returned false. code: %d (possible concurrent removal)\n", cm_res);
    }
#endif

    CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}
CM_RES tsm_node_defer_free(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    CM_ASSERT(p_tsm_base && p_base);

    CM_TIMER_START();

    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm_key key_base = {0};
    CM_ASSERT(CM_RES_SUCCESS == tsm_node_copy_key(p_base, &key_base));
    struct tsm_base_node* p_base_mutable = NULL;
    CM_SCOPE(CM_RES cm_res = _tsm_node_get_mutable(p_tsm_base, &key_base, &p_base_mutable));
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_free(&key_base));
    if (cm_res == CM_RES_TSM_NODE_NOT_FOUND) {
        CM_LOG_NOTICE("tsm_node_defer_free: node no longer found\n");
        return CM_RES_SUCCESS;
    }
    CM_ASSERT(cm_res == CM_RES_SUCCESS);

    #ifdef TSM_DEBUG
        CM_SCOPE(cm_res = tsm_node_is_valid(p_tsm_base, p_base_mutable));
        if (cm_res == CM_RES_TSM_NODE_NOT_FOUND_SELF || cm_res == CM_RES_TSM_NODE_NOT_FOUND || cm_res == CM_RES_TSM_NODE_IS_REMOVED) {
            CM_LOG_WARNING("CM_RES: %d: node is not found. Are you sure you're trying to defer free the correct node inside the correct TSM. If you are sure then another thread has removed it\n", cm_res);
            CM_TIMER_STOP();
            return CM_RES_SUCCESS;
        } else if (cm_res != CM_RES_TSM_NODE_IS_VALID) {
            CM_LOG_ERROR("base node to defer free is not valid with code %d\n", cm_res);
            CM_TIMER_STOP();
            return cm_res;
        }

        // if p_base_mutable is type and is not base type then we want to check all nodes if this key is found as type key in any other node
        struct tsm_key base_key = { .key_union = p_base_mutable->key_union, .key_type = p_base_mutable->key_type };
        struct tsm_key base_type_key = { .key_union = p_base_mutable->type_key_union, .key_type = p_base_mutable->type_key_type };
        CM_SCOPE(cm_res = tsm_key_match(&base_key, &base_type_key));
        if (cm_res != CM_RES_TSM_KEYS_MATCH) {
            CM_ASSERT(cm_res == CM_RES_TSM_KEYS_DONT_MATCH);
            CM_SCOPE(cm_res = tsm_node_is_type(p_base_mutable));
            if (cm_res == CM_RES_TSM_NODE_IS_TYPE) {
                struct cds_lfht_iter iter;
                CM_SCOPE(bool node_exists = tsm_iter_first(p_tsm_base, &iter));
                while (node_exists) {
                    const struct tsm_base_node* p_iter_base = NULL;
                    CM_SCOPE(cm_res = tsm_iter_get_node(&iter, &p_iter_base));
                    if (cm_res != CM_RES_SUCCESS) {
                        CM_TIMER_STOP();
                        return cm_res;
                    }
                    struct tsm_key other_key = { .key_union = p_iter_base->type_key_union, .key_type = p_iter_base->type_key_type };
                    CM_SCOPE(cm_res = tsm_key_match(&base_key, &other_key));
                    if (cm_res != CM_RES_TSM_KEYS_MATCH) {
                        tsm_node_print(p_tsm_base, p_base_mutable);
                        tsm_node_print(p_tsm_base, p_iter_base);
                        CM_TIMER_STOP();
                        return cm_res;
                    }
                    CM_SCOPE(node_exists = tsm_iter_next(p_tsm_base, &iter));
                }
            } else if (cm_res != CM_RES_TSM_NODE_NOT_TYPE) {
                CM_LOG_ERROR("tsm_node_is_type() failed with code %d\n", cm_res);
                CM_TIMER_STOP();
                return cm_res;
            }
        }
    #endif

    // schdule node for free callback via call_rcu
    struct tsm_key type_key = { .key_union.string = p_base_mutable->type_key_union.string, .key_type = p_base_mutable->type_key_type };
    const struct tsm_base_node* p_type_base = NULL;
    CM_SCOPE(cm_res = tsm_node_get(p_tsm_base, &type_key, &p_type_base));
    if (cm_res != CM_RES_SUCCESS) {
        CM_TIMER_STOP();
        return cm_res;
    }
    struct tsm_base_type_node* p_type = caa_container_of(p_type_base, struct tsm_base_type_node, base);
    #ifdef TSM_DEBUG
        CM_ASSERT(CM_RES_TSM_NODE_IS_VALID == tsm_node_is_valid(p_tsm_base, p_type_base));
        CM_ASSERT(p_type->fn_free_callback);
    #endif

    // Logically removing the node from the hashtable
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    int32_t del_result = cds_lfht_del(p_tsm->p_ht, &p_base_mutable->lfht_node);
    if (del_result == -ENOENT) {
        CM_LOG_WARNING("node is already removed\n");
        CM_TIMER_STOP();
        return CM_RES_TSM_NODE_IS_REMOVED;
    } else if (del_result != 0) {
        CM_LOG_ERROR("Failed to delete node thread safe map. del_result = %d\n", del_result);
        CM_SCOPE(tsm_node_print(p_tsm_base, p_base_mutable));
        CM_TIMER_STOP();
        return CM_RES_UNKNOWN; 
    }
    #ifdef TSM_DEBUG
        CM_ASSERT(CM_RES_TSM_NODE_IS_REMOVED == tsm_node_is_removed(p_base_mutable));
    #endif

    // Schedule for RCU cleanup after successful removal
    call_rcu(&p_base_mutable->rcu_head, p_type->fn_free_callback);

    CM_TIMER_STOP();

    return CM_RES_SUCCESS;
}
CM_RES tsm_node_copy_key(const struct tsm_base_node* p_base, struct tsm_key* p_output_key) {
    CM_ASSERT(p_base && p_output_key);
    struct tsm_key key = { .key_union = p_base->key_union, .key_type = p_base->key_type };
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(&key, p_output_key));
    return CM_RES_SUCCESS;
}  
CM_RES tsm_node_copy_key_type(const struct tsm_base_node* p_base, struct tsm_key* p_output_key) {
    CM_ASSERT(p_base && p_output_key);
    struct tsm_key key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type };
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(&key, p_output_key));
    return CM_RES_SUCCESS;
}  
CM_RES tsm_node_is_removed(const struct tsm_base_node* node) {
    CM_ASSERT(node)
    CM_SCOPE(bool result = cds_lfht_is_node_deleted(&node->lfht_node));
    if (!result) {
        return CM_RES_TSM_NODE_NOT_REMOVED;
    }
    return CM_RES_TSM_NODE_IS_REMOVED;
}
// ==========================================================================================
// TSM NODE ITER
// ==========================================================================================
CM_RES tsm_iter_first(const struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    CM_ASSERT(p_tsm_base && iter);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    CM_SCOPE(cds_lfht_first(p_tsm->p_ht, iter));
    if (iter->node == NULL) {
        return CM_RES_TSM_ITER_END;
    }
    
    return CM_RES_SUCCESS;
}
CM_RES tsm_iter_next(const struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    CM_ASSERT(p_tsm_base && iter);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    CM_SCOPE(cds_lfht_next(p_tsm->p_ht, iter));
    CM_ASSERT(iter);
    if (iter->node == NULL) {
        return CM_RES_TSM_ITER_END;
    }
    
    return CM_RES_SUCCESS;
}
CM_RES tsm_iter_get_node(struct cds_lfht_iter* iter, const struct tsm_base_node** pp_output_node) {
    CM_ASSERT(iter && pp_output_node);
    if (iter->node == NULL) {
        CM_LOG_NOTICE("iter->node == NULL which means there is no node for this iter\n");
        return CM_RES_TSM_ITER_END;
    }
    CM_SCOPE(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    if (!lfht_node) {
        return CM_RES_TSM_NODE_NOT_FOUND;
    }
    *pp_output_node = caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
    return CM_RES_SUCCESS;
}
CM_RES tsm_iter_lookup(const struct tsm_base_node* p_tsm_base, const struct tsm_key* p_key, struct cds_lfht_iter* iter) {
    CM_ASSERT(p_tsm_base && iter && p_key);
    CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(p_key));
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    uint64_t hash = _tsm_hash_key(p_key->key_union, p_key->key_type); // Calculate hash
    CM_SCOPE(cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, p_key, iter));
    
    return CM_RES_SUCCESS;
}

// ==========================================================================================
// THREAD SAFE MAP
// ==========================================================================================
CM_RES tsm_create(const struct tsm_base_node* p_tsm_parent_base, const struct tsm_key* p_new_tsm_key, struct tsm_base_node** pp_output_node) {

    *pp_output_node = NULL;

    // validating TSM
    CM_ASSERT(p_tsm_parent_base && p_new_tsm_key && pp_output_node);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_parent_base));

    CM_TIMER_START();

    // create the new tsm node
    struct tsm_base_node* p_new_tsm_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == _tsm_base_node_create(p_new_tsm_key, &g_tsm_type_key, sizeof(struct tsm), false, true, &p_new_tsm_base));
    struct tsm* p_new_tsm = caa_container_of(p_new_tsm_base, struct tsm, base);
    p_new_tsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    CM_ASSERT(p_new_tsm->p_ht != NULL);

    struct tsm* p_tsm_parent = caa_container_of(p_tsm_parent_base, struct tsm, base);
    CM_ASSERT(CM_RES_SUCCESS == tsm_path_copy(&p_tsm_parent->path, &p_new_tsm->path));
    struct tsm_key key_copy = {0};
    CM_ASSERT(CM_RES_SUCCESS == tsm_node_copy_key(p_new_tsm_base, &key_copy));
    CM_ASSERT(CM_RES_SUCCESS == tsm_path_insert_key(&p_new_tsm->path, &key_copy, -1));
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_free(&key_copy));

    // create base_type and insert it into new TSM
    struct tsm_base_node* p_new_base_type = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
        &g_base_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node),
        &p_new_base_type));

    CM_SCOPE(tsm_base_node_print(p_new_base_type));
    uint64_t hash = _tsm_hash_key(p_new_base_type->key_union, p_new_base_type->key_type);
    CM_SCOPE(struct cds_lfht_node* add_unique_result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &g_base_type_key, &p_new_base_type->lfht_node));
    CM_ASSERT(add_unique_result == &p_new_base_type->lfht_node);

    // creating tsm_type now, as base_type is created, and insert it into new TSM
    struct tsm_base_node* p_new_tsm_type = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
        &g_tsm_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_tsm_type_free_callback,
        _tsm_tsm_type_is_valid,
        _tsm_tsm_type_print,
        sizeof(struct tsm),
        &p_new_tsm_type));
    hash = _tsm_hash_key(p_new_tsm_type->key_union, p_new_tsm_type->key_type);
    CM_SCOPE(add_unique_result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &g_tsm_type_key, &p_new_tsm_type->lfht_node));
    CM_ASSERT(add_unique_result == &p_new_tsm_type->lfht_node);
    
    #ifdef TSM_DEBUG
        const struct tsm_base_node* base_node = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_new_tsm_base, &g_base_type_key, &base_node));

        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_new_tsm_base, &g_tsm_type_key, &base_node));
    #endif
    
    CM_LOG_DEBUG("Created new TSM and inserted base_type and tsm_type nodes inside it\n");

    CM_TIMER_STOP();
    *pp_output_node = p_new_tsm_base;
    
    return CM_RES_SUCCESS;
}
CM_RES tsm_nodes_count(const struct tsm_base_node* p_tsm_base, uint64_t* p_output_count) {
    CM_ASSERT(p_tsm_base != NULL && p_output_count != NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    CM_TIMER_START();

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    long split_count_before, split_count_after;
    // cds_lfht_count_nodes MUST be called from within read-side critical section
    // according to URCU specification
    cds_lfht_count_nodes(p_tsm->p_ht, &split_count_before, p_output_count, &split_count_after);

    CM_TIMER_STOP();
    
    return CM_RES_SUCCESS;
}
CM_RES tsm_print(const struct tsm_base_node* p_tsm_base) {
    CM_ASSERT(p_tsm_base != NULL);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));

    CM_TIMER_START();

    struct cds_lfht_iter iter;
    CM_SCOPE(CM_RES iter_status = tsm_iter_first(p_tsm_base, &iter));
    while (iter_status == CM_RES_SUCCESS) {
        const struct tsm_base_node* p_base = NULL;
        CM_SCOPE(CM_RES cm_res = tsm_iter_get_node(&iter, &p_base));
        if (cm_res != CM_RES_SUCCESS) {
            CM_TIMER_STOP();
            return cm_res;
        }
        CM_SCOPE(tsm_node_print(p_tsm_base, p_base));
        CM_SCOPE(iter_status = tsm_iter_next(p_tsm_base, &iter));
    }
    CM_ASSERT(iter_status == CM_RES_TSM_ITER_END);

    CM_TIMER_STOP();

    return CM_RES_SUCCESS;
}
CM_RES tsm_copy_path(const struct tsm_base_node* p_tsm_base, struct tsm_path* p_output_path) {
    CM_ASSERT(p_tsm_base && p_output_path);
    CM_ASSERT(CM_RES_TSM_NODE_IS_TSM == tsm_node_is_tsm(p_tsm_base));
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    CM_ASSERT(CM_RES_SUCCESS == tsm_path_copy(&p_tsm->path, p_output_path));
    return CM_RES_SUCCESS;
}
// ==========================================================================================
// GLOBAL THREAD SAFE MAP
// ==========================================================================================
CM_RES gtsm_init() {

    rcu_read_lock();
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (GTSM_rcu) {
        CM_LOG_INFO("GTSM is already initialized because its not NULL\n");
        rcu_read_unlock();
        return CM_RES_GTSM_ALREADY_INITIALIZED;
    }
    rcu_read_unlock();

    // Register the node size function with urcu_safe  system
    urcu_safe_set_node_size_function(_tsm_get_node_size);
    // Register the node bounds functions with urcu_safe system
    urcu_safe_set_node_start_ptr_function(_tsm_get_node_start_ptr);

    // create the GTSM node
    struct tsm_base_node* p_new_gtsm_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == _tsm_base_node_create(&g_gtsm_key, &g_tsm_type_key, sizeof(struct tsm), false, true, &p_new_gtsm_base));

    struct tsm* p_new_gtsm = caa_container_of(p_new_gtsm_base, struct tsm, base);
    p_new_gtsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    CM_ASSERT(NULL != p_new_gtsm->p_ht);

    p_new_gtsm->path.length = 0;
    p_new_gtsm->path.key_chain = NULL;


    // insert base_type into GTSM
    struct tsm_base_node* base_type_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
        &g_base_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node),
        &base_type_base));

    uint64_t hash = _tsm_hash_key(base_type_base->key_union, base_type_base->key_type);
    rcu_read_lock();
    CM_SCOPE(struct cds_lfht_node* result = cds_lfht_add_unique(p_new_gtsm->p_ht, hash, _tsm_key_match, &g_base_type_key, &base_type_base->lfht_node));
    CM_ASSERT(result == &base_type_base->lfht_node);

    // insert tsm_type into GTSM
    struct tsm_base_node* tsm_type_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(
        &g_tsm_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_tsm_type_free_callback,
        _tsm_tsm_type_is_valid,
        _tsm_tsm_type_print,
        sizeof(struct tsm),
        &tsm_type_base));

    hash = _tsm_hash_key(tsm_type_base->key_union, tsm_type_base->key_type);
    CM_SCOPE(result = cds_lfht_add_unique(p_new_gtsm->p_ht, hash, _tsm_key_match, &g_tsm_type_key, &tsm_type_base->lfht_node));
    CM_ASSERT(result == &tsm_type_base->lfht_node);

    // validating inserts
    #ifdef TSM_DEBUG
        const struct tsm_base_node* base_type_node = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_new_gtsm_base, &g_base_type_key, &base_type_node));

        const struct tsm_base_node* tsm_type_node = NULL;
        CM_ASSERT(CM_RES_SUCCESS == tsm_node_get(p_new_gtsm_base, &g_tsm_type_key, &tsm_type_node));
    #endif
        
    rcu_read_unlock();
    
    void* old_pointer = rcu_cmpxchg_pointer(&GTSM, NULL, p_new_gtsm_base);
    if (old_pointer != NULL) {
        CM_LOG_NOTICE("failed to swap pointer\n");
        return CM_RES_CMPXCHG_FAILURE;
    }

    CM_LOG_DEBUG("Created GTSM and inserted base_type and tsm_type into it\n");

    return CM_RES_SUCCESS;
}
const struct tsm_base_node* gtsm_get() {
    const struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (GTSM_rcu == NULL) {
        CM_LOG_ERROR("GMTS_rcu is NULL\n");
    }
    return GTSM_rcu;
}
CM_RES gtsm_free() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    CM_ASSERT(GTSM_rcu != NULL);

    // assigning NULL now so that no more nodes can be inserted into GTSM when we try to remove all nodes inside GTSM
    // actually even though GTSM is assigned to NULL threads that is inside a long read could still access the old GTSM
    // and insert into GTSM even after all nodes are scheduled to be removed. How HOW TO FIX THIS????
    for (int32_t count = 0; count < 5; count++) {
        void* old_pointer = rcu_cmpxchg_pointer(&GTSM, GTSM_rcu, NULL);
        if (old_pointer == GTSM_rcu) { break; }
        if (count == 5) {
            CM_LOG_NOTICE("failed to swap pointer to NULL\n");
            return CM_RES_CMPXCHG_FAILURE;
        }
    }

    CM_SCOPE(CM_RES is_tsm = tsm_node_is_tsm(GTSM_rcu));
    if (is_tsm != CM_RES_TSM_NODE_IS_TSM) {
        CM_LOG_WARNING("GTSM is not a TSM\n");
        return is_tsm;
    }
    
    // Schedule for RCU cleanup after successful removal
    call_rcu(&GTSM_rcu->rcu_head, _tsm_tsm_type_free_callback);

    return CM_RES_SUCCESS;
}
CM_RES gtsm_print() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    CM_ASSERT(GTSM_rcu != NULL);

    CM_LOG_INFO("Printing GTSM:\n");
    CM_SCOPE(tsm_base_node_print(GTSM_rcu));

    struct cds_lfht_iter iter;
    CM_SCOPE(CM_RES iter_valid = tsm_iter_first(GTSM_rcu, &iter));
    while (iter_valid == CM_RES_SUCCESS) {
        const struct tsm_base_node* iter_node = NULL;
        CM_SCOPE(CM_RES cm_res = tsm_iter_get_node(&iter, &iter_node));
        if (cm_res != CM_RES_SUCCESS) {
            return cm_res;
        }
        CM_SCOPE(cm_res = tsm_node_print(GTSM_rcu, iter_node));
        if (cm_res != CM_RES_SUCCESS) {
            return cm_res;
        }
        CM_SCOPE(iter_valid = tsm_iter_next(GTSM_rcu, &iter));
    }
    CM_ASSERT(iter_valid == CM_RES_TSM_ITER_END);

    return CM_RES_SUCCESS;
}