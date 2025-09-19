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
    struct tsm_key ctx_1 = { .key_union = base_node->key_union, .key_type = base_node->key_type };
    struct tsm_key* p_ctx_2 = (struct tsm_key *)key;
    return tsm_key_match(ctx_1, *p_ctx_2);
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
    tklog_scope(TSM_Result tsm_result = tsm_base_node_free(p_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("failed to free with code %d\n", tsm_result);
    }
}
static TSM_Result _tsm_base_type_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    if (!p_base || !p_tsm_base) {
        tklog_error("arguments is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("p_tsm_base is not TSM\n");
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_is_type(p_base)); 
    if (tsm_result != TSM_RESULT_NODE_IS_TYPE) {
        tklog_info("given base is not type which it should be for it to be valid\n");
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_base_node_is_valid(p_tsm_base, p_base));
    if (tsm_result != TSM_RESULT_NODE_IS_VALID) {
        tklog_info("base is not valid\n");
        return tsm_result;
    }

    // Basic validation - check if it's a node type node
    struct tsm_base_type_node* p_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    if (!p_type->fn_free_callback) {
        tklog_debug("p_type->fn_free_callback is NULL\n");
        return TSM_RESULT_NULL_FUNCTION_POINTER;
    }
    if (!p_type->fn_is_valid) {
        tklog_debug("p_type->fn_is_valid is NULL\n");
        return TSM_RESULT_NULL_FUNCTION_POINTER;
    }
    if (!p_type->fn_print) {
        tklog_debug("p_type->fn_print is NULL\n");
        return TSM_RESULT_NULL_FUNCTION_POINTER;
    }
    if (p_type->type_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_debug("node_type->type_size_bytes(%d) < sizeof(struct tsm_base_node)(%d)\n", p_type->type_size_bytes, sizeof(struct tsm_base_node));
        return TSM_RESULT_NODE_SIZE_MISMATCH;
    }

    return TSM_RESULT_NODE_IS_VALID;
}
static TSM_Result _tsm_base_type_node_print(struct tsm_base_node* p_base) {

    if (!p_base) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_type(p_base)); 
    if (tsm_result != TSM_RESULT_NODE_IS_TYPE) {
        tklog_info("given base is not type which it should be\n");
        return tsm_result;
    }
    tsm_result = tsm_base_node_print(p_base);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("failed to print base node\n");
        return tsm_result;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_base, struct tsm_base_type_node, base);
    (void)p_type_node;
    tklog_info("    fn_free_callback: %p\n", p_type_node->fn_free_callback);
    tklog_info("    fn_is_valid: %p\n", p_type_node->fn_is_valid);
    tklog_info("    fn_print: %p\n", p_type_node->fn_print);
    tklog_info("    size of the node this node is type for: %d bytes\n", p_type_node->type_size_bytes);

    return TSM_RESULT_SUCCESS;
}

// TSM Type
static TSM_Result _tsm_tsm_type_free_children(struct tsm_base_node* p_tsm_base) {

    if (!p_tsm_base) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }

    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("node is not TSM\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }

    // first remove all non-types
    struct cds_lfht_iter iter;
    TSM_Result iter_valid = tsm_iter_first(p_tsm_base, &iter);
    while (iter_valid == TSM_RESULT_SUCCESS) {
        struct tsm_base_node* p_base = NULL;
        tklog_scope(tsm_result = tsm_iter_get_node(&iter, &p_base));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            tklog_error("tsm_iter_get_node failed\n");
            return tsm_result;
        }
        // Advance BEFORE possible delete
        iter_valid = tsm_iter_next(p_tsm_base, &iter);
        if (!p_base) {
            tklog_warning("somehow p_base is NULL\n");
            continue;
        }
        tklog_scope(tsm_result = tsm_node_is_type(p_base));
        if (tsm_result != TSM_RESULT_NODE_IS_TYPE) {
            if (tsm_result != TSM_RESULT_NODE_NOT_TYPE) {
                tklog_error("tsm_node_is_type() failed with code %d\n", tsm_result);
                return tsm_result;
            }
            tklog_scope(tsm_result = tsm_node_defer_free(p_tsm_base, p_base));
            if (tsm_result != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_node_defer_free failed for child node. it is %d\n", tsm_result);
                return tsm_result;
            }
        }
    }
    if (iter_valid != TSM_RESULT_ITER_END) {
        tklog_error("after iter loop iter is not TSM_RESULT_ITER_END but rather %d\n", tsm_result);
        return tsm_result;
    }
    // now types - collect first to avoid iter invalidation
    while (true) {

        struct tsm_base_node* to_free[MAX_TYPES_IN_SAME_LAYER]; // Adjust size as needed or use dynamic allocation
        size_t to_free_count = 0;
        iter_valid = tsm_iter_first(p_tsm_base, &iter);
        while (iter_valid == TSM_RESULT_SUCCESS) {
            struct tsm_base_node* p_base = NULL;
            tklog_scope(tsm_result = tsm_iter_get_node(&iter, &p_base));
            if (tsm_result != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_iter_get_node() failed with code %d\n", tsm_result);
                return tsm_result;
            }
            iter_valid = tsm_iter_next(p_tsm_base, &iter);
            if (!p_base) {
                tklog_warning("somehow p_base is NULL\n");
                continue;
            }
            tklog_scope(tsm_result = tsm_node_is_type(p_base));
            if (tsm_result != TSM_RESULT_NODE_IS_TYPE) {
                tklog_error("somehow found non-type when there should be only types left\n");
                return TSM_RESULT_TSM_NON_TYPES_STILL_REMAINING;
            }
            // since p_base is type then we want to check all nodes if this key is found as type key in any other node
            bool found_in_other_node = false;
            struct tsm_key base_key = { .key_union = p_base->key_union, .key_type = p_base->key_type };
            struct cds_lfht_iter iter_2;
            TSM_Result iter_2_valid = tsm_iter_first(p_tsm_base, &iter_2);
            while (iter_2_valid == TSM_RESULT_SUCCESS) {
                struct tsm_base_node* p_iter_base = NULL;
                tklog_scope(tsm_result = tsm_iter_get_node(&iter_2, &p_iter_base));
                if (tsm_result != TSM_RESULT_SUCCESS) {
                    tklog_error("Somehow node is NULL\n");
                    return tsm_result;
                }
                // it not found self then check if base is found as type in other node
                struct tsm_key other_key = { .key_union = p_iter_base->key_union, .key_type = p_iter_base->key_type };
                bool found_self = tsm_key_match(base_key, other_key);
                if (!found_self) {
                    struct tsm_key other_type_key = { .key_union = p_iter_base->type_key_union, .key_type = p_iter_base->type_key_type };
                    bool type_found_in_other_node = tsm_key_match(base_key, other_type_key);
                    if (type_found_in_other_node) {
                        found_in_other_node = true;
                        break;
                    }
                }
                iter_2_valid = tsm_iter_next(p_tsm_base, &iter_2);
            }
            if (iter_2_valid != TSM_RESULT_SUCCESS && iter_2_valid != TSM_RESULT_ITER_END) {
                tklog_error("after iter loop iter is not TSM_RESULT_ITER_END but rather %d\n", iter_2_valid);
                return iter_2_valid;
            }
            // if node at this iteration isnt found in any other node then we can free it
            if (!found_in_other_node) {
                if (to_free_count >= MAX_TYPES_IN_SAME_LAYER) {
                    tklog_error("Too many types to free in one pass\n");
                    return TSM_RESULT_TSM_TOO_MANY_TYPES;
                }
                to_free[to_free_count++] = p_base;
            }
        }
        if (iter_valid != TSM_RESULT_ITER_END) {
            tklog_error("after iter loop iter is not TSM_RESULT_ITER_END but rather %d\n", tsm_result);
            return tsm_result;
        }

        // Check for cycle or done
        if (to_free_count == 0) {
            uint64_t nodes_count = 0;
            tklog_scope(tsm_result = tsm_nodes_count(p_tsm_base, &nodes_count));
            if (tsm_result != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_nodes_count() failed with code %d\n", tsm_result);
                return tsm_result;
            }
            if (nodes_count > 0) {
                tklog_error("children nodes type dependencies are cyclical and hence in an invalid state\n");
                return TSM_RESULT_TSM_CYCLICAL_TYPES;
            }
            break;
        }
        // Free collected
        for (size_t i = 0; i < to_free_count; ++i) {
            tklog_scope(TSM_Result defer_free_result = tsm_node_defer_free(p_tsm_base, to_free[i]));
            if (defer_free_result != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_node_defer_free failed for child node. it is %d\n", defer_free_result);
                return defer_free_result;
            }
        }
    }
    tklog_info("cleaning all children nodes for TSM completed. pointer: %p\n", p_tsm_base);
    return TSM_RESULT_SUCCESS;
}
static void _tsm_tsm_type_free_callback(struct rcu_head* rcu_head) {
    if (!rcu_head) {
        tklog_error("rcu_head is NULL\n");
        return;
    }

    struct tsm_base_node* p_base = caa_container_of(rcu_head, struct tsm_base_node, rcu_head);
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("in _tsm_tsm_type_free_callback node is not TSM with error code %d\n", tsm_result);
        return;
    }

    rcu_read_lock();
    tklog_scope(tsm_result = _tsm_tsm_type_free_children(p_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("in _tsm_tsm_type_free_callback _tsm_tsm_type_free_children failed with error code %d\n", tsm_result);
        rcu_read_unlock();
        return;
    }
    uint64_t nodes_count;
    tklog_scope(tsm_result = tsm_nodes_count(p_base, &nodes_count));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("in _tsm_tsm_type_free_callback tsm_nodes_count failed with error code %d\n", tsm_result);
        rcu_read_unlock();
        return;
    }
    rcu_read_unlock();

    if (nodes_count > 0) {
        tklog_error("tsm is not empty before freeing it\n");
    }

    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    tklog_scope(tsm_result = tsm_path_free(&p_tsm->path));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("tsm_free_path failed\n");
    }
    cds_lfht_destroy(p_tsm->p_ht, NULL);
    tklog_scope(tsm_result = tsm_base_node_free(p_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("tsm_base_node_free failed with code %d\n", tsm_result);
    }
}
static TSM_Result _tsm_tsm_type_is_valid(struct tsm_base_node* p_parent_tsm_base, struct tsm_base_node* p_tsm_base) {

    if (!p_tsm_base || !p_parent_tsm_base) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_parent_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("p_parent_tsm_base is not a TSM\n");
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("p_tsm_base is not a TSM\n");
        return tsm_result;
    }

    struct tsm_key tmp_key = { .key_union = p_tsm_base->key_union, .key_type = p_tsm_base->key_type };
    struct tsm_base_node* p_tmp_tsm_base = NULL; 
    tklog_scope(tsm_result = tsm_node_get(p_parent_tsm_base, tmp_key, &p_tmp_tsm_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_warning("given tsm doesnt exist in given parent tsm\n");
        return tsm_result;
    }

    struct cds_lfht_iter iter;
    tklog_scope(TSM_Result iter_valid = tsm_iter_first(p_tsm_base, &iter));
    while (iter_valid == TSM_RESULT_SUCCESS) {
        struct tsm_base_node* iter_node = NULL;
        tklog_scope(tsm_result = tsm_iter_get_node(&iter, &iter_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            return tsm_result;
        }
        if (!iter_node) {
            tklog_error("iter_node is NULL\n");
            break;
        }
        tklog_scope(TSM_Result is_valid = tsm_base_node_is_valid(p_tsm_base, iter_node));
        if (is_valid != TSM_RESULT_NODE_IS_VALID) {
            tklog_warning("This node is not valid: code %d\n", is_valid);
            tklog_scope(tsm_node_print(p_tsm_base, iter_node));
            return is_valid;
        }
        tklog_scope(iter_valid = tsm_iter_next(p_tsm_base, &iter));
    }
    if (iter_valid != TSM_RESULT_ITER_END) {
        tklog_error("iter_valid is invalid with code%d\n", iter_valid);
        return iter_valid;
    }

    return TSM_RESULT_NODE_IS_VALID;
}
static TSM_Result _tsm_tsm_type_print(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("p_base is not a TSM\n");
        return tsm_result;
    }

    tsm_base_node_print(p_base);
    struct tsm* p_tsm = caa_container_of(p_base, struct tsm, base);
    tsm_path_print(&p_tsm->path);

    struct cds_lfht_iter iter;
    tklog_scope(TSM_Result iter_valid = tsm_iter_first(p_base, &iter));
    while (iter_valid == TSM_RESULT_SUCCESS) {
        struct tsm_base_node* iter_node = NULL;
        tklog_scope(tsm_result = tsm_iter_get_node(&iter, &iter_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            return tsm_result;
        }
        if (!iter_node) {
            tklog_error("somehow iter_node is NULL\n");
            break;
        }
        tsm_result = tsm_node_print(p_base, iter_node);
        if (tsm_result != TSM_RESULT_SUCCESS) {
            return tsm_result;
        }
        iter_valid = tsm_iter_next(p_base, &iter);
    }
    if (iter_valid != TSM_RESULT_ITER_END) {
        tklog_error("iter_valid is invalid with code %d\n", iter_valid);
        return iter_valid;
    }

    return TSM_RESULT_SUCCESS;
}

// ==========================================================================================
// PUBLIC 
// ==========================================================================================

// ==========================================================================================
// KEY
// ==========================================================================================
TSM_Result tsm_key_union_string_create(const char* string_key, union tsm_key_union* p_output_key) {

    if (!string_key || !p_output_key) {
        tklog_error("NULL argumnets\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }

    // Validate and copy string key
    size_t key_len = strlen(string_key) + 1;
    if (key_len == 1) {
        tklog_error("string key is empty\n");
        return TSM_RESULT_KEY_STRING_EMPTY;
    }
    if (key_len > MAX_STRING_KEY_LEN) {
        tklog_error("string key too long (max %d characters)\n", MAX_STRING_KEY_LEN - 1);
        return TSM_RESULT_KEY_STRING_TOO_LARGE;
    }
    
    char* copied_string = calloc(1, key_len);
    if (!copied_string) {
        tklog_error("Failed to allocate memory for string key\n");
        return TSM_RESULT_ALLOCATION_FAILURE;
    }
    
    if (strncpy(copied_string, string_key, key_len - 1) == NULL) {
        tklog_error("Failed to copy string key\n");
        free(copied_string);
        return TSM_RESULT_KEY_STRING_COPY_FAILURE;
    }
    copied_string[key_len - 1] = '\0'; // Ensure null termination
    
    p_output_key->string = copied_string;

    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_key_union_uint64_create(uint64_t number_key, union tsm_key_union* p_output_key) {
    if (number_key == 0) {
        number_key = atomic_fetch_add(&g_key_counter, 1);
    }
    p_output_key->uint64 = number_key;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_key_union_free(union tsm_key_union key_union, uint8_t key_type) {
    if (key_union.uint64 == 0) {
        tklog_error("number key is 0\n");
        return TSM_RESULT_KEY_NOT_VALID;
    }
    if (key_type == TSM_KEY_TYPE_UINT64) {
        return TSM_RESULT_SUCCESS;
    } else {
        if (key_union.string) {
            free(key_union.string);
            key_union.string = NULL;
            return TSM_RESULT_SUCCESS;
        }
    }
    return TSM_RESULT_UNKNOWN;
}
TSM_Result tsm_key_uint64_create(uint64_t number_key, struct tsm_key* p_output_key) {
    if (!p_output_key) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_key_union_uint64_create(number_key, &p_output_key->key_union));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        p_output_key->key_type = TSM_KEY_TYPE_NONE;
        return tsm_result;
    }
    p_output_key->key_type = TSM_KEY_TYPE_UINT64;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_key_string_create(const char* p_string, struct tsm_key* p_output_key) {
    if (!p_string || !p_output_key) {
        tklog_error("p_string is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_key_union_string_create(p_string, &p_output_key->key_union));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        p_output_key->key_type = TSM_KEY_TYPE_NONE;
        return tsm_result;
    }
    p_output_key->key_type = TSM_KEY_TYPE_STRING;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_key_copy(struct tsm_key key, struct tsm_key* p_output_key) {
    if (!p_output_key) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (key.key_type == TSM_KEY_TYPE_UINT64) {
        if (key.key_union.uint64 == 0) {
            tklog_error("key uint64 value is 0 which is invalid in this context\n");
            return TSM_RESULT_KEY_UINT64_IS_ZERO;
        }
        tklog_scope(TSM_Result tsm_result = tsm_key_uint64_create(key.key_union.uint64, p_output_key));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            return tsm_result;
        }
    } else if (key.key_type == TSM_KEY_TYPE_STRING) {
        if (key.key_union.string == NULL) {
            tklog_error("key string value is NULL which is invalid in this context\n");
            return TSM_RESULT_KEY_STRING_IS_NULL;
        }
        tklog_scope(TSM_Result tsm_result = tsm_key_string_create(key.key_union.string, p_output_key));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            return tsm_result;
        }
    } else {
        tklog_error("key type is invlaid. the type is %d\n", key.key_type);
    }
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_key_free(struct tsm_key* key) {
    if (!key) {
        tklog_error("key is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_key_union_free(key->key_union, key->key_type));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("failed to free node with code %d\n", tsm_result);
        return tsm_result;
    }
    key->key_union.uint64 = 0;
    key->key_type = TSM_KEY_TYPE_NONE;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_key_match(struct tsm_key key_1, struct tsm_key key_2) {
    tklog_scope(TSM_Result is_valid = tsm_key_is_valid(&key_1));
    if (is_valid != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("key_1 invlaid with code %d\n", is_valid);
        return is_valid;
    }
    tklog_scope(is_valid = tsm_key_is_valid(&key_2));
    if (is_valid != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("key_2 invlaid with code %d\n", is_valid);
        return is_valid;
    }
    // if not both is string or both is number
    if (key_1.key_type != key_2.key_type)
        return TSM_RESULT_KEYS_DONT_MATCH;
    // if both are number
    if (key_1.key_type == TSM_KEY_TYPE_UINT64) 
        if (key_1.key_union.uint64 == key_2.key_union.uint64) {
            return TSM_RESULT_KEYS_MATCH;
        } else {
            return TSM_RESULT_KEYS_DONT_MATCH;
        }
    // if both are string
    else {
        const char* string_1 = key_1.key_union.string;
        const char* string_2 = key_2.key_union.string;
        uint64_t string_1_len = strlen(string_1);
        uint64_t string_2_len = strlen(string_2);
        (void)string_1_len;
        (void)string_2_len;
        if (strcmp(string_1, string_2) == 0) {
            return TSM_RESULT_KEYS_MATCH;
        } else {
            return TSM_RESULT_KEYS_DONT_MATCH;
        }
    }
}
TSM_Result tsm_key_print(struct tsm_key key) {
    TSM_Result is_valid = tsm_key_is_valid(&key);
    if (is_valid != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("key invlaid with code %d\n", is_valid);
        return is_valid;
    }
    if (key.key_type == TSM_KEY_TYPE_UINT64) {
        tklog_info("key: %lu\n", key.key_union.uint64);
    } else if (key.key_type == TSM_KEY_TYPE_STRING) {
        tklog_info("key: %s\n", key.key_union.string);
    } else {
        tklog_info("key is parent\n");
    }
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_key_is_valid(struct tsm_key* p_key) {
    if (!p_key) {
        tklog_notice("tsm_key_is_valid(): NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (p_key->key_type == TSM_KEY_TYPE_UINT64) {
        if (p_key->key_union.uint64 == 0) {
            tklog_notice("tsm_key_is_valid(): uint64 is 0 when type is uint64\n");
            return TSM_RESULT_KEY_NOT_VALID;
        }
    } else if (p_key->key_type == TSM_KEY_TYPE_STRING) {
        if (p_key->key_union.string == NULL) {
            tklog_notice("tsm_key_is_valid(): string is NULL\n");
            return TSM_RESULT_KEY_NOT_VALID;
        }
    } else if (p_key->key_type == TSM_KEY_TYPE_PARENT) {
        if (p_key->key_union.uint64 != 0) {
            tklog_notice("tsm_key_is_valid(): uint64 is not 0 when type is parent\n");
            return TSM_RESULT_KEY_NOT_VALID;
        }
    } else {
        tklog_notice("tsm_key_is_valid(): invalid key type %d\n", p_key->key_type);
        return TSM_RESULT_KEY_NOT_VALID;
    }
    return TSM_RESULT_KEY_IS_VALID;
}

// ==========================================================================================
// BASE NODE
// ==========================================================================================
TSM_Result tsm_base_node_create(
                struct tsm_key key, 
                struct tsm_key type_key, 
                uint32_t this_size_bytes, 
                bool this_is_type, 
                bool this_is_tsm, 
                struct tsm_base_node** pp_output_node) {

    tklog_scope(TSM_Result tsm_result = tsm_key_is_valid(&key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("key is invalid with code %d\n", tsm_result);
        *pp_output_node = NULL;
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_key_is_valid(&type_key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("type_key is invalid with code %d\n", tsm_result);
        *pp_output_node = NULL;
        return tsm_result;
    }
    if (this_size_bytes < sizeof(struct tsm_base_node)) {
        tklog_error("this_size_bytes is less than the size of the tsm_base_node\n");
        *pp_output_node = NULL;
        return TSM_RESULT_NODE_SIZE_TO_SMALL;
    }
    struct tsm_base_node* p_node = calloc(1, this_size_bytes);
    if (!p_node) {
        tklog_error("Failed to allocate memory for p_node\n");
        return TSM_RESULT_ALLOCATION_FAILURE;
    }
    cds_lfht_node_init(&p_node->lfht_node);
    
    // Handle key assignment - copy string keys, use number keys directly
    if (key.key_type == TSM_KEY_TYPE_UINT64) {
        p_node->key_union.uint64 = key.key_union.uint64;
    } else {
        // String key is already copied by tsm_key_union_create
        p_node->key_union.string = key.key_union.string;
    }
    
    // Handle type_key assignment - copy string keys, use number keys directly
    if (type_key.key_type == TSM_KEY_TYPE_UINT64) {
        p_node->type_key_union.uint64 = type_key.key_union.uint64;
    } else {
        // String type_key is already copied by tsm_key_union_create
        p_node->type_key_union.string = type_key.key_union.string;
    }
    
    p_node->key_type = key.key_type;
    p_node->type_key_type = type_key.key_type;
    p_node->this_size_bytes = this_size_bytes;
    p_node->this_is_type = this_is_type;
    p_node->this_is_tsm = this_is_tsm;
    *pp_output_node = p_node;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_base_node_free(struct tsm_base_node* p_base_node) {
    if (!p_base_node) {
        tklog_error("p_base_node is NULL when trying to free it");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result free_result = tsm_key_union_free(p_base_node->key_union, p_base_node->key_type));
    if (free_result != TSM_RESULT_SUCCESS) {
        tklog_error("failed to free key union with code %d\n", free_result);
        return free_result;
    }
    tklog_scope(free_result = tsm_key_union_free(p_base_node->type_key_union, p_base_node->type_key_type));
    if (free_result != TSM_RESULT_SUCCESS) {
        tklog_error("failed to free key union with code %d\n", free_result);
        return free_result;
    }
    free(p_base_node);
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_base_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    
    if (!p_base || !p_tsm_base) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("p_tsm_base is not TSM\n");
        return TSM_RESULT_NODE_NOT_TSM;
    }

    struct tsm_key key = { .key_union = p_base->key_union, .key_type = p_base->key_type};
    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type};
    tklog_scope(tsm_result = tsm_key_is_valid(&key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_key_is_valid(&type_key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        return tsm_result;
    }

    struct tsm_base_node* p_base_copy = NULL;
    tklog_scope(tsm_result = tsm_node_get(p_tsm_base, key, &p_base_copy));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    if (p_base != p_base_copy) {
        tklog_notice("tsm_base_node_is_valid: given p_base and p_base gotten from key in given p_base is not the same\n");
        tklog_scope(TSM_Result print_result = tsm_node_print(p_tsm_base, p_base));
        if (print_result != TSM_RESULT_SUCCESS) {
            tklog_error("failed to print p_base node with error code %d\n", print_result);
        }
        tklog_scope(print_result = tsm_node_print(p_tsm_base, p_base_copy));
        if (print_result != TSM_RESULT_SUCCESS) {
            tklog_error("failed to print p_base node with error code %d\n", print_result);
        }
        return TSM_RESULT_NODE_NOT_FOUND_SELF;
    }

    struct tsm_base_node* p_base_type = NULL;
    tklog_scope(tsm_result = tsm_node_get(p_tsm_base, type_key, &p_base_type));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_notice("tsm_base_node_is_valid: did not find type for given base node\n");
        tklog_scope(TSM_Result print_result = tsm_node_print(p_tsm_base, p_base));
        if (print_result != TSM_RESULT_SUCCESS) {
            tklog_error("failed to print p_base node with error code %d\n", print_result);
        }
        return tsm_result;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    if (p_base->this_size_bytes != p_type->type_size_bytes) {
        tklog_notice("tsm_base_node_is_valid: p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)", p_base->this_size_bytes, p_type->type_size_bytes);
        tklog_scope(TSM_Result print_result = tsm_node_print(p_tsm_base, p_base));
        if (print_result != TSM_RESULT_SUCCESS) {
            tklog_error("failed to print p_base node with error code %d\n", print_result);
        }
        return TSM_RESULT_NODE_SIZE_MISMATCH;
    }

    tklog_scope(TSM_Result is_removed = tsm_node_is_removed(p_base));
    if (is_removed != TSM_RESULT_NODE_NOT_REMOVED) {
        if (is_removed != TSM_RESULT_NODE_IS_REMOVED) {
            tklog_error("tsm_node_is_removed failed with code %d\n", is_removed);
        } else {
            tklog_notice("tsm_base_node_is_valid: node is removed and therefore not valid\n");
        }
        return is_removed;
    }

    return TSM_RESULT_NODE_IS_VALID;
}
TSM_Result tsm_base_node_print(struct tsm_base_node* p_base) {

    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }

    if (p_base->key_type == TSM_KEY_TYPE_UINT64) {
        tklog_info("key: %lu\n", p_base->key_union.uint64);
    } else {
        tklog_info("key: %s\n", p_base->key_union.string);
    }

    if (p_base->type_key_type == TSM_KEY_TYPE_UINT64) {
        tklog_info("    type_key: %lu\n", p_base->type_key_union.uint64);
    } else {
        tklog_info("    type_key: %s\n", p_base->type_key_union.string);
    }

    tklog_info("    size: %d bytes\n", p_base->this_size_bytes);

    return TSM_RESULT_SUCCESS;
}

// ==========================================================================================
// BASE TYPE
// ==========================================================================================
TSM_Result tsm_base_type_node_create(
    struct tsm_key key, 
    uint32_t this_size_bytes,
    void (*fn_free_callback)(struct rcu_head*),
    TSM_Result (*fn_is_valid)(struct tsm_base_node*, struct tsm_base_node*),
    TSM_Result (*fn_print)(struct tsm_base_node*),
    uint32_t type_size_bytes,
    struct tsm_base_node** pp_output_node) {

    *pp_output_node = NULL;
    if (this_size_bytes < sizeof(struct tsm_base_type_node)) {
        tklog_error("this_size_bytes(%zu) is less than sizeof(tsm_base_type_node)\n", sizeof(struct tsm_base_type_node));
        return TSM_RESULT_NODE_SIZE_TO_SMALL;
    }
    if (fn_free_callback == NULL) {
        tklog_error("fn_free_callback is NULL\n");
        return TSM_RESULT_NULL_FUNCTION_POINTER;
    }
    if (fn_is_valid == NULL) {
        tklog_error("fn_is_valid is NULL\n");
        return TSM_RESULT_NULL_FUNCTION_POINTER;
    }
    if (fn_print == NULL) {
        tklog_error("fn_print is NULL\n");
        return TSM_RESULT_NULL_FUNCTION_POINTER;
    }

    // Create proper copies of the string keys for the base_type node
    struct tsm_key base_type_key_copy = {0};
    tklog_scope(TSM_Result tsm_result = tsm_key_copy(g_base_type_key, &base_type_key_copy));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    
    struct tsm_base_node* p_base = NULL;
    tklog_scope(TSM_Result create_result = tsm_base_node_create(key, base_type_key_copy, sizeof(struct tsm_base_type_node), true, false, &p_base));
    if (create_result != TSM_RESULT_SUCCESS) {
        tklog_error("Failed to create base_type node\n");
        tsm_key_free(&base_type_key_copy);
        return create_result;
    }

    struct tsm_base_type_node* p_base_type = caa_container_of(p_base, struct tsm_base_type_node, base);

    p_base_type->fn_free_callback = fn_free_callback;
    p_base_type->fn_is_valid = fn_is_valid;
    p_base_type->fn_print = fn_print;
    p_base_type->type_size_bytes = type_size_bytes;

    *pp_output_node = p_base;

    return TSM_RESULT_SUCCESS;
}
// ==========================================================================================
// PATH
// ==========================================================================================
TSM_Result tsm_path_insert_key(struct tsm_path* p_path, struct tsm_key* p_key, int32_t index) {
    if (!p_path || !p_key) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result key_valid = tsm_key_is_valid(p_key));
    if (key_valid != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("key is not valid with code %d\n", key_valid);
        return key_valid;
    }
    int32_t length = (int32_t)p_path->length;
    if (index < -length - 1 || index > length) {
        tklog_error("index(%d) is out of bounds for path length(%d)\n", index, length);
        return TSM_RESULT_OUTSIDE_BOUNDS;
    }
    if (length == 0) {
        if (p_path->key_chain != NULL) {
            tklog_error("Inconsistent path: length=0 but key_chain not NULL\n");
            return TSM_RESULT_NULL_ARGUMENT;
        }
    } else {
        if (p_path->key_chain == NULL) {
            tklog_error("Inconsistent path: length>0 but key_chain NULL\n");
            return TSM_RESULT_NULL_ARGUMENT;
        }
    }
    int32_t pos = index;
    if (pos < 0) {
        pos += length + 1;
    }
    struct tsm_key* new_array = realloc(p_path->key_chain, (length + 1) * sizeof(struct tsm_key));
    if (!new_array) {
        tklog_error("Failed to realloc path array\n");
        return TSM_RESULT_ALLOCATION_FAILURE;
    }
    p_path->key_chain = new_array;
    memmove(&p_path->key_chain[pos + 1], &p_path->key_chain[pos], (length - pos) * sizeof(struct tsm_key));
    p_path->key_chain[pos] = *p_key;  // Copy struct (transfers ownership)
    // Invalidate source to prevent double-free
    p_key->key_union.uint64 = 0;
    p_key->key_type = TSM_KEY_TYPE_NONE;
    p_path->length++;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_path_remove_key(struct tsm_path* p_path, int32_t index) {
    if (!p_path) {
        tklog_error("p_path is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    int32_t length = (int32_t)p_path->length;
    if (index < -length || index >= length) {
        tklog_error("index(%d) is outside bounds for path length(%d)\n", index, length);
        return TSM_RESULT_OUTSIDE_BOUNDS;
    }
    int32_t pos = index;
    if (pos < 0) {
        pos += length;
    }
    tsm_key_free(&p_path->key_chain[pos]);
    memmove(&p_path->key_chain[pos], &p_path->key_chain[pos + 1], (length - pos - 1) * sizeof(struct tsm_key));
    p_path->length--;
    if (p_path->length > 0) {
        struct tsm_key* new_array = realloc(p_path->key_chain, p_path->length * sizeof(struct tsm_key));
        if (new_array) {
            p_path->key_chain = new_array;
        }
    } else {
        free(p_path->key_chain);
        p_path->key_chain = NULL;
    }
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_path_is_valid(struct tsm_path* p_path) {
    if (!p_path) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if ((p_path->length == 0) != (p_path->key_chain == NULL)) {
        tklog_info("p_path->length is %d and p_path->key_chain is %p\n", p_path->length, p_path->key_chain);
        return TSM_RESULT_PATH_INVALID;
    }
    for (uint32_t i = 0; i < p_path->length; ++i) {
        tklog_scope(TSM_Result key_valid = tsm_key_is_valid(&p_path->key_chain[i]));
        if (key_valid != TSM_RESULT_KEY_IS_VALID) {
            tklog_info("key in index %d is invalid\n", i);
            return key_valid;
        }
    }
    return TSM_RESULT_PATH_VALID;
}
TSM_Result tsm_path_free(struct tsm_path* p_path) {
    if (!p_path) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result path_valid = tsm_path_is_valid(p_path));
    if (path_valid != TSM_RESULT_PATH_VALID) {
        tklog_error("p_path is not valid before trying to free it with error code %d\n", path_valid);
        return path_valid;
    }
    tklog_scope(TSM_Result print_result = tsm_path_print(p_path));
    if (print_result != TSM_RESULT_SUCCESS) {
        tklog_error("printfing path failed with error code %d\n", print_result);
        return print_result;
    }
    for (uint32_t i = 0; i < p_path->length; ++i) {
        tklog_scope(TSM_Result free_result = tsm_key_free(&p_path->key_chain[i]));
        if (free_result != TSM_RESULT_SUCCESS) {
            tklog_error("freeing key failed with error code %d\n", free_result);
            return free_result;
        }
    }
    if (p_path->key_chain) {
        free(p_path->key_chain);
    }
    p_path->key_chain = NULL;
    p_path->length = 0;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_path_print(struct tsm_path* p_path) {
    if (!p_path) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result path_valid = tsm_path_is_valid(p_path));
    if (path_valid != TSM_RESULT_PATH_VALID)  {
        tklog_error("p_path is not valid with error code %d\n", path_valid);
        return path_valid;
    }
    if (p_path->length == 0) {
        tklog_info("p_path: (empty)\n");
        return TSM_RESULT_SUCCESS;
    }
    char full_str[512];
    char* ptr = full_str;
    size_t remaining = sizeof(full_str);
    TSM_Result result = TSM_RESULT_SUCCESS;
    for (uint32_t i = 0; i < p_path->length; ++i) {
        int len;
        if (p_path->key_chain[i].key_type == TSM_KEY_TYPE_UINT64) {
            len = snprintf(ptr, remaining, "%lu -> ", p_path->key_chain[i].key_union.uint64);
        } else {
            len = snprintf(ptr, remaining, "%s -> ", p_path->key_chain[i].key_union.string);
        }
        if (len < 0 || (size_t)len >= remaining) {
            result = TSM_RESULT_BUFFER_OVERFLOW;
            break;
        }
        ptr += len;
        remaining -= len;
    }
    if (ptr >= full_str + 4) {
        ptr[-4] = '\0';  // Remove the last " -> "
    } else {
        result = TSM_RESULT_UNKNOWN;
    }
    tklog_info("path: %s\n", full_str);
    return result;
}
TSM_Result tsm_path_copy(struct tsm_path* p_path_to_copy, struct tsm_path* p_output_path) {
    if (!p_path_to_copy || !p_output_path) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_path_is_valid(p_path_to_copy));
    if (tsm_result != TSM_RESULT_PATH_VALID) {
        return tsm_result;
    }
    p_output_path->key_chain = NULL;
    p_output_path->length = 0;
    if (p_path_to_copy->length == 0) {
        return TSM_RESULT_SUCCESS;
    }
    struct tsm_key* new_array = calloc(1, p_path_to_copy->length * sizeof(struct tsm_key));
    if (!new_array) {
        tklog_error("Failed to allocate memory for path copy\n");
        return TSM_RESULT_ALLOCATION_FAILURE;
    }
    for (uint32_t i = 0; i < p_path_to_copy->length; ++i) {
        tklog_scope(tsm_result = tsm_key_copy(p_path_to_copy->key_chain[i], &new_array[i]));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            free(new_array);
            return tsm_result;
        }
    }
    p_output_path->key_chain = new_array;
    p_output_path->length = p_path_to_copy->length;
    tklog_scope(tsm_result = tsm_path_is_valid(p_output_path));
    if (tsm_result != TSM_RESULT_PATH_VALID) {
        free(new_array);
        return tsm_result;
    }
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_path_get_key_ref(struct tsm_path* p_path, int32_t index, struct tsm_key** pp_key_ref) {
    if (!pp_key_ref || !p_path) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    *pp_key_ref = NULL;
    tklog_scope(TSM_Result path_valid = tsm_path_is_valid(p_path));
    if (path_valid != TSM_RESULT_PATH_VALID) {
        tklog_error("p_path is invalid with code %d\n", path_valid);
        return path_valid;
    }
    if (p_path->length == 0) {
        tklog_error("p_path is empty\n");
        return TSM_RESULT_OUTSIDE_BOUNDS;
    }
    int32_t pos = index;
    if (pos < 0) {
        pos += (int32_t)p_path->length;
    }
    if (pos < 0 || pos >= (int32_t)p_path->length) {
        tklog_error("index(%d) is out of bounds for p_path length(%u)\n", index, p_path->length);
        return TSM_RESULT_OUTSIDE_BOUNDS;
    }
    *pp_key_ref = &p_path->key_chain[pos];
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_path_create_between_paths(struct tsm_path* p_path_1, struct tsm_path* p_path_2, struct tsm_path* p_output_path) {
    if (!p_output_path || !p_path_1 || !p_path_2) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result valid_1 = tsm_path_is_valid(p_path_1));
    if (valid_1 != TSM_RESULT_PATH_VALID) {
        tklog_error("p_path_1 is invalid with code %d\n", valid_1);
        return valid_1;
    }
    tklog_scope(TSM_Result valid_2 = tsm_path_is_valid(p_path_2));
    if (valid_2 != TSM_RESULT_PATH_VALID) {
        tklog_error("p_path_2 is invalid with code %d\n", valid_2);
        return valid_2;
    }
    p_output_path->key_chain = NULL;
    p_output_path->length = 0;

    uint32_t len1 = p_path_1->length;
    uint32_t len2 = p_path_2->length;
    uint32_t common_len = 0;
    uint32_t min_len = len1 < len2 ? len1 : len2;
    while (common_len < min_len) {
        TSM_Result match = tsm_key_match(p_path_1->key_chain[common_len], p_path_2->key_chain[common_len]);
        if (match != TSM_RESULT_KEYS_MATCH) {
            break;
        }
        common_len++;
    }

    uint32_t ups_needed = len1 - common_len;
    uint32_t downs_needed = len2 - common_len;
    uint32_t total_keys = ups_needed + downs_needed;
    if (total_keys == 0) {
        // Same path, empty relative path
        return TSM_RESULT_SUCCESS;
    }

    struct tsm_key* new_array = calloc(1, total_keys * sizeof(struct tsm_key));
    if (!new_array) {
        tklog_error("Failed to allocate memory for between path\n");
        return TSM_RESULT_ALLOCATION_FAILURE;
    }

    uint32_t idx = 0;
    // Add ups (parent keys)
    for (uint32_t i = 0; i < ups_needed; ++i) {
        // Create a parent key: type PARENT, union uint64=0 (dummy)
        TSM_Result res = tsm_key_union_uint64_create(0, &new_array[idx].key_union);
        if (res != TSM_RESULT_SUCCESS) {
            // Note: tsm_key_union_uint64_create allows 0 for special cases
            new_array[idx].key_union.uint64 = 0;
        }
        new_array[idx].key_type = TSM_KEY_TYPE_PARENT;
        idx++;
    }
    // Add downs (suffix of p_path_2)
    for (uint32_t i = common_len; i < len2; ++i) {
        TSM_Result res = tsm_key_copy(p_path_2->key_chain[i], &new_array[idx]);
        if (res != TSM_RESULT_SUCCESS) {
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

    tklog_scope(TSM_Result valid_out = tsm_path_is_valid(p_output_path));
    if (valid_out != TSM_RESULT_PATH_VALID) {
        tsm_path_free(p_output_path);
        return valid_out;
    }
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_path_insert_path(struct tsm_path* p_dst_path, struct tsm_path* p_src_path, int32_t index) {
    if (!p_dst_path || !p_src_path) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result valid_src = tsm_path_is_valid(p_src_path));
    if (valid_src != TSM_RESULT_PATH_VALID) {
        tklog_error("p_src_path is invalid with code %d\n", valid_src);
        return valid_src;
    }
    tklog_scope(TSM_Result valid_dst = tsm_path_is_valid(p_dst_path));
    if (valid_dst != TSM_RESULT_PATH_VALID) {
        tklog_error("p_dst_path is invalid with code %d\n", valid_dst);
        return valid_dst;
    }

    uint32_t dst_len = p_dst_path->length;
    uint32_t src_len = p_src_path->length;
    if (src_len == 0) {
        // Inserting empty path does nothing
        return TSM_RESULT_SUCCESS;
    }
    if (index < -(int32_t)dst_len - 1 || index > (int32_t)dst_len) {
        tklog_error("index(%d) is out of bounds for dst_path length(%u)\n", index, dst_len);
        return TSM_RESULT_OUTSIDE_BOUNDS;
    }

    int32_t pos = index;
    if (pos < 0) {
        pos += dst_len + 1;
    }

    // Allocate new array for dst_len + src_len
    struct tsm_key* new_array = realloc(p_dst_path->key_chain, (dst_len + src_len) * sizeof(struct tsm_key));
    if (!new_array) {
        tklog_error("Failed to realloc dst_path array\n");
        return TSM_RESULT_ALLOCATION_FAILURE;
    }
    p_dst_path->key_chain = new_array;

    // Shift the tail
    memmove(&p_dst_path->key_chain[pos + src_len], &p_dst_path->key_chain[pos],
            (dst_len - pos) * sizeof(struct tsm_key));

    // Copy src keys into the gap
    for (uint32_t i = 0; i < src_len; ++i) {
        TSM_Result res = tsm_key_copy(p_src_path->key_chain[i], &p_dst_path->key_chain[pos + i]);
        if (res != TSM_RESULT_SUCCESS) {
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
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_path_length(struct tsm_path* p_path, uint32_t* p_output_length) {
    if (!p_path || !p_output_length) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    *p_output_length = p_path->length;
    return TSM_RESULT_SUCCESS;
}
// ==========================================================================================
// THREAD SAFE MAP
// ==========================================================================================
TSM_Result tsm_create(
    struct tsm_base_node* p_tsm_parent_base,
    struct tsm_key new_tsm_key,
    struct tsm_base_node** pp_output_node) {

    *pp_output_node = NULL;

    // validating TSM
    if (!p_tsm_parent_base) {
        tklog_error("p_tsm_parent_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_parent_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_info("tsm_node_is_tsm failed with code %d\n", tsm_result);
        return tsm_result;
    }

    tklog_timer_start();

    // create the new tsm node
    struct tsm_key tsm_type_key_copy = {0};
    tklog_scope(tsm_result = tsm_key_copy(g_tsm_type_key, &tsm_type_key_copy));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_info("tsm_key_copy failed with code %d\n", tsm_result);
        return tsm_result;
    }
    struct tsm_base_node* p_new_tsm_base = NULL;
    tklog_scope(tsm_result = tsm_base_node_create(new_tsm_key, tsm_type_key_copy, sizeof(struct tsm), false, true, &p_new_tsm_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_scope(tsm_key_free(&tsm_type_key_copy));
        tklog_timer_stop();
        tklog_info("tsm_base_node_create failed with code %d\n", tsm_result);
        return tsm_result;
    }
    struct tsm* p_new_tsm = caa_container_of(p_new_tsm_base, struct tsm, base);
    p_new_tsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    if (!p_new_tsm->p_ht) {
        tklog_error("Failed to create lock free hash table\n");
        tsm_result = TSM_RESULT_HASHTABLE_CREATION_FAILURE;
        goto free_tsm;
    }

    struct tsm* p_tsm_parent = caa_container_of(p_tsm_parent_base, struct tsm, base);
    tklog_scope(tsm_result = tsm_path_copy(&p_tsm_parent->path, &p_new_tsm->path));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("tsm_path_copy failed with code %d\n", tsm_result);
        return tsm_result;
    }
    struct tsm_key endpoint_key = { .key_union = p_new_tsm_base->key_union, .key_type = p_new_tsm_base->key_type };
    tklog_scope(tsm_result = tsm_path_insert_key(&p_new_tsm->path, &endpoint_key, -1));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("tsm_path_insert_key failed with code %d\n", tsm_result);
        return tsm_result;
    }

    // create base_type and insert it into new TSM
    struct tsm_key base_type_key = {0};
    tklog_scope(tsm_result = tsm_key_copy(g_base_type_key, &base_type_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_info("tsm_key_copy failed with code %d\n", tsm_result);
        return tsm_result;
    }
    struct tsm_base_node* p_new_base_type = NULL;
    tklog_scope(tsm_result = tsm_base_type_node_create(
        base_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node),
        &p_new_base_type));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("Failed to create base_type node\n");
        tklog_scope(tsm_base_node_free(p_new_base_type));
        goto free_tsm;
    }
    tklog_scope(tsm_base_node_print(p_new_base_type));
    uint64_t hash = _tsm_hash_key(p_new_base_type->key_union, p_new_base_type->key_type);
    tklog_scope(struct cds_lfht_node* add_unique_result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &base_type_key, &p_new_base_type->lfht_node));
    if (add_unique_result != &p_new_base_type->lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        tsm_base_node_free(p_new_base_type);
        tsm_result = TSM_RESULT_NODE_INSERTION_FAILURE;
        goto free_tsm;
    }

    // creating tsm_type now, as base_type is created, and insert it into new TSM
    tklog_scope(tsm_result = tsm_key_copy(g_tsm_type_key, &tsm_type_key_copy));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_info("tsm_key_copy failed with code %d\n", tsm_result);
        return tsm_result;
    }
    struct tsm_base_node* p_new_tsm_type = NULL;
    tklog_scope(tsm_result = tsm_base_type_node_create(
        tsm_type_key_copy, 
        sizeof(struct tsm_base_type_node),
        _tsm_tsm_type_free_callback,
        _tsm_tsm_type_is_valid,
        _tsm_tsm_type_print,
        sizeof(struct tsm),
        &p_new_tsm_type));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_error("Failed to create tsm_type\n");
        tsm_base_node_free(p_new_tsm_type);
        goto free_tsm;
    }
    hash = _tsm_hash_key(p_new_tsm_type->key_union, p_new_tsm_type->key_type);
    tklog_scope(add_unique_result = cds_lfht_add_unique(p_new_tsm->p_ht, hash, _tsm_key_match, &tsm_type_key_copy, &p_new_tsm_type->lfht_node));
    if (add_unique_result != &p_new_tsm_type->lfht_node) {
        tklog_error("Failed to insert base_type node into hash table\n");
        tsm_base_node_free(p_new_tsm_type);
        tsm_result = TSM_RESULT_NODE_INSERTION_FAILURE;
        goto free_tsm;
    }
    
    #ifdef TSM_DEBUG
        struct tsm_base_node* base_node = NULL;
        tklog_scope(tsm_result = tsm_node_get(p_new_tsm_base, g_base_type_key, &base_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            tklog_error("base_type node not found after insertion\n");
            goto free_tsm;
        }

        tklog_scope(tsm_result = tsm_node_get(p_new_tsm_base, g_tsm_type_key, &base_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            tklog_error("tsm_type node not found after insertion\n");
            goto free_tsm;
        }
    #endif
    
    tklog_debug("Created new TSM and inserted base_type and tsm_type nodes inside it\n");

    tklog_timer_stop();
    *pp_output_node = p_new_tsm_base;
    return TSM_RESULT_SUCCESS;

    free_tsm:
    tklog_scope(TSM_Result defer_free_result = tsm_node_defer_free(p_tsm_parent_base, p_new_tsm_base));
    if (defer_free_result != TSM_RESULT_SUCCESS) {
        tklog_error("tsm_node_defer_free for newly created TSM failed with code %d\n", defer_free_result);
    }
    tklog_timer_stop();
    return tsm_result;
}
TSM_Result tsm_node_get(struct tsm_base_node* p_tsm_base, struct tsm_key key, struct tsm_base_node** pp_output_node) {

    *pp_output_node = NULL;

    if (!p_tsm_base) {
        tklog_error("p_tsm is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_key_is_valid(&key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("key is invlaid\n");
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return tsm_result;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash internally
    uint64_t hash = _tsm_hash_key(key.key_union, key.key_type);
    struct cds_lfht_iter iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key, &iter);
    struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(&iter);
    
    if (!lfht_node) {
        tklog_info("node is not found because lfht_node = NULL");
        return TSM_RESULT_NODE_NOT_FOUND;
    }

    struct tsm_base_node* result = caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
    *pp_output_node = result;
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_node_get_by_path(struct tsm_base_node* p_tsm_base, struct tsm_path* p_path, struct tsm_base_node** pp_output_node) {
    if (!p_tsm_base || !p_path) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    *pp_output_node = NULL;
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("p_tsm_base is not a TSM\n");
        return tsm_result;
    }

    if ((p_path->key_chain == NULL) ^ (p_path->length == 0)) {
        tklog_error("p_path->key_chain == NULL ^ p_path->length == 0\n");
        return TSM_RESULT_PATH_INVALID;
    } 

    tklog_timer_start();
    struct tsm_base_node* current = p_tsm_base;
    for (uint32_t i = 0; i < p_path->length; ++i) {
        struct tsm_key key = p_path->key_chain[i];
        struct tsm_base_node* p_new = NULL;
        TSM_Result tsm_result = tsm_node_get(current, key, &p_new);
        if (tsm_result != TSM_RESULT_SUCCESS) {
            tklog_scope(TSM_Result print_result = tsm_path_print(p_path));
            if (print_result != TSM_RESULT_SUCCESS) {
                tklog_warning("tsm_path_print failed with code %d\n", print_result);
            }
            tklog_timer_stop();
            return tsm_result;
        }
        current = p_new;
        tklog_scope(tsm_result = tsm_node_is_tsm(current));
        if (i < p_path->length - 1 && tsm_result != TSM_RESULT_NODE_IS_TSM) {
            tklog_warning("Intermediate node at p_path index %u is not a TSM\n", i);
            tklog_timer_stop();
            return tsm_result;
        }
    }
    *pp_output_node = current;
    tklog_timer_stop();

    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_node_get_by_path_at_depth(struct tsm_base_node* p_tsm_base, struct tsm_path* p_path, int depth, struct tsm_base_node** pp_output_node) {
    if (!p_tsm_base || !p_path) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    *pp_output_node = NULL;
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("p_tsm_base is not a TSM\n");
        return tsm_result;
    }
    tklog_scope(TSM_Result path_valid = tsm_path_is_valid(p_path));
    if (path_valid != TSM_RESULT_PATH_VALID) {
        tklog_error("p_path is invalid with code %d\n", path_valid);
        return path_valid;
    }
    uint32_t path_len = p_path->length;  // Use uint32_t for consistency
    if (path_len == 0 && depth != 0) {
        tklog_error("depth(%d) is out of bounds for empty p_path\n", depth);
        return TSM_RESULT_OUTSIDE_BOUNDS;
    }
    int adjusted_depth = depth;
    if (depth < 0) {
        adjusted_depth += (int)path_len + 1;  // Matches original intent: -1 -> path_len
    }
    if (adjusted_depth < 0 || adjusted_depth > (int)path_len) {
        tklog_error("depth(%d) resolves to steps(%d) which is out of bounds for path length(%u)\n", depth, adjusted_depth, path_len);
        return TSM_RESULT_OUTSIDE_BOUNDS;
    }
    tklog_timer_start();
    struct tsm_base_node* current = p_tsm_base;
    uint32_t steps = (uint32_t)adjusted_depth;
    for (uint32_t i = 0; i < steps; ++i) {  // 0-based: i=0 to <steps
        struct tsm_key key = p_path->key_chain[i];
        struct tsm_base_node* p_new = NULL;
        TSM_Result tsm_result = tsm_node_get(current, key, &p_new);
        if (tsm_result != TSM_RESULT_SUCCESS) {
            tklog_scope(TSM_Result print_result = tsm_path_print(p_path));
            if (print_result != TSM_RESULT_SUCCESS) {
                tklog_warning("tsm_path_print failed with code %d\n", print_result);
            }
            tklog_timer_stop();
            return tsm_result;
        }
        current = p_new;
        tklog_scope(tsm_result = tsm_node_is_tsm(current));
        if (i < steps - 1 && tsm_result != TSM_RESULT_NODE_IS_TSM) {  // Align with first function
            tklog_error("Intermediate node at p_path index %u is not a TSM\n", i);  // Changed to error for consistency
            tklog_timer_stop();
            return tsm_result;
        }
    }
    *pp_output_node = current;
    tklog_timer_stop();
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_node_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    if (!p_base || !p_tsm_base) {
        tklog_notice("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return tsm_result;
    }

    tklog_timer_start();

    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type };
    struct tsm_base_node* p_base_type = NULL;
    tklog_scope(tsm_result = tsm_node_get(p_tsm_base, type_key, &p_base_type));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_notice("tried to get type node but got NULL for node: \n");
        tklog_timer_stop();
        return tsm_result;
    }

    tklog_scope(struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base));
    uint32_t size_1 = p_base->this_size_bytes;
    uint32_t size_2 = p_type->type_size_bytes;
    if (size_1 != size_2) {
        tklog_scope(p_type->fn_print(p_base));
        tklog_notice("p_base->this_size_bytes(%d) != p_type->type_size_bytes(%d)\n", p_base->this_size_bytes, p_type->type_size_bytes);
        tklog_timer_stop();
        return TSM_RESULT_NODE_SIZE_MISMATCH;
    }

    tklog_scope(tsm_result = p_type->fn_is_valid(p_tsm_base, p_base));
    if (tsm_result != TSM_RESULT_NODE_IS_VALID) {
        tklog_warning("fn_is_valid for node didnt return valid. code: %d\n", tsm_result);
        tklog_scope(tsm_node_print(p_tsm_base, p_base));
    }

    tklog_timer_stop();

    return tsm_result;
}
TSM_Result tsm_node_is_tsm(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (!p_base->this_is_tsm) {
        return TSM_RESULT_NODE_NOT_TSM;
    }
    return TSM_RESULT_NODE_IS_TSM;
}
TSM_Result tsm_node_is_type(struct tsm_base_node* p_base) {
    if (!p_base) {
        tklog_error("p_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (p_base->this_is_type) {
        return TSM_RESULT_NODE_IS_TYPE;;
    } else {
        tklog_debug("node is not a type node\n");
        return TSM_RESULT_NODE_NOT_TYPE;
    }
}
TSM_Result tsm_node_print(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {

    if (!p_tsm_base || !p_base) {
        tklog_error("p_base is NULL");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not a TSM\n");
        return tsm_result;
    }

    tklog_timer_start();

    struct tsm_key type_key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type };
    struct tsm_base_node* p_base_type = NULL;
    tklog_scope(tsm_result = tsm_node_get(p_tsm_base, type_key, &p_base_type));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_timer_stop();
        return TSM_RESULT_NODE_NOT_FOUND;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_base_type, struct tsm_base_type_node, base);

    tklog_scope(tsm_result = p_type->fn_print(p_base));

    tklog_timer_stop();

    return tsm_result;
}
TSM_Result tsm_node_insert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!new_node || !p_tsm_base) {
        tklog_error("new_node is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (new_node->key_union.uint64 == 0) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("key number is 0\n");
        return TSM_RESULT_KEY_NOT_VALID;
    }
    if (new_node->type_key_union.uint64 == 0) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("type_key number is 0\n");
        return TSM_RESULT_KEY_NOT_VALID;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("given p_tsm_base is not TSM\n");
        return tsm_result;
    }

    tklog_timer_start();

    struct tsm_key type_key = { .key_union = new_node->type_key_union, .key_type = new_node->type_key_type };
    struct tsm_base_node* p_type_node_base = NULL;
    tklog_scope(tsm_result = tsm_node_get(p_tsm_base, type_key, &p_type_node_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        gtsm_print();
        tsm_base_node_print(p_tsm_base);
        tsm_key_print(type_key);
        tklog_timer_stop();
        return tsm_result;
    }

    struct tsm_base_type_node* p_type_node = caa_container_of(p_type_node_base, struct tsm_base_type_node, base);
    uint32_t type_size_bytes = p_type_node->type_size_bytes;

    if (type_size_bytes != new_node->this_size_bytes) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_scope(tsm_node_print(p_tsm_base, p_type_node_base));
        tklog_error("type_size_bytes(%d) != new_node->this_size_bytes(%d)", type_size_bytes, new_node->this_size_bytes);
        tklog_timer_stop();
        return TSM_RESULT_NODE_SIZE_MISMATCH;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);

    uint64_t hash = _tsm_hash_key(new_node->key_union, new_node->key_type);
    struct tsm_key key = { .key_union = new_node->key_union, .key_type = new_node->key_type };
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_tsm->p_ht, hash, _tsm_key_match, &key, &new_node->lfht_node));
    
    if (result != &new_node->lfht_node) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_warning("node already exists\n");
        tklog_timer_stop();
        return TSM_RESULT_NODE_EXISTS;
    }

    #ifdef TSM_DEBUG
        tklog_scope(tsm_result = tsm_node_is_valid(p_tsm_base, new_node));
        if (tsm_result != TSM_RESULT_NODE_IS_VALID && tsm_result != TSM_RESULT_NODE_IS_REMOVED && tsm_result != TSM_RESULT_NODE_NOT_FOUND) {
            tklog_error("after inserting then running tsm_node_is_valid for the inserted node it returned code: %d\n", tsm_result);
            tklog_timer_stop();
            return tsm_result;
        }
    #endif

    if (new_node->key_type == TSM_KEY_TYPE_UINT64) {
        tklog_debug("Successfully inserted node with number key %llu\n", new_node->key_union.uint64);
    } else {
        tklog_debug("Successfully inserted node with string key %s\n", new_node->key_union.string);
    }

    tklog_timer_stop();

    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_node_update(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (new_node->key_union.uint64 == 0) {
        tklog_error("key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_RESULT_KEY_NOT_VALID;
    }
    if (new_node->type_key_union.uint64 == 0) {
        tklog_error("type_key number is 0\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        return TSM_RESULT_KEY_NOT_VALID;
    }

    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base node is not a TSM\n");
        return tsm_result;
    }

    tklog_timer_start();

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Find the existing node
    uint64_t                hash = _tsm_hash_key(new_node->key_union, new_node->key_type);
    struct tsm_key key = { .key_union = new_node->key_union, .key_type = new_node->key_type};
    struct cds_lfht_iter    old_iter = {0};
    cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key, &old_iter);
    struct cds_lfht_node*   old_lfht_node = cds_lfht_iter_get_node(&old_iter);
    struct tsm_base_node*   old_node = caa_container_of(old_lfht_node, struct tsm_base_node, lfht_node);
    if (!old_node) {
        if (new_node->key_type == TSM_KEY_TYPE_UINT64)
            tklog_info("Cannot update - node with number key %lu not found\n", new_node->key_union.uint64);
        else
            tklog_info("Cannot update - node with string key %s not found\n", new_node->key_union.string);
        tklog_timer_stop();
        return TSM_RESULT_NODE_NOT_FOUND;
    }
    if (old_node == new_node) {
        tklog_error("when replacing a node its not allowed to replace with the same node. given new_node == old_node\n");
        tklog_timer_stop();
        return TSM_RESULT_NODE_REPLACING_SAME;
    }
 
    // Get the type information
    struct tsm_key old_type_key = { .key_union = old_node->type_key_union, .key_type = old_node->type_key_type };
    struct tsm_key new_type_key = { .key_union = new_node->type_key_union, .key_type = new_node->type_key_type };
    tklog_scope(tsm_result = tsm_key_match(old_type_key, new_type_key));
    if (tsm_result != TSM_RESULT_KEYS_MATCH) {
        tklog_timer_stop();
        return tsm_result;
    }

    struct tsm_base_node* p_type_base_node = NULL;
    tklog_scope(tsm_result = tsm_node_get(p_tsm_base, old_type_key, &p_type_base_node));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_timer_stop();
        return tsm_result;
    }

    struct tsm_base_type_node* p_type = caa_container_of(p_type_base_node, struct tsm_base_type_node, base);
    
    // Verify data size matches expected type size 
    if (new_node->this_size_bytes != p_type->type_size_bytes) {
        tklog_error("new node size %u doesn't match current node size %u\n", new_node->this_size_bytes, p_type->type_size_bytes);
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_timer_stop();
        return TSM_RESULT_NODE_SIZE_MISMATCH;
    }

    // Get the free callback
    if (!p_type->fn_free_callback) {
        tklog_error("free_callback is NULL\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_timer_stop();
        return TSM_RESULT_NULL_FUNCTION_POINTER;
    }

    int32_t replace_result = cds_lfht_replace(p_tsm->p_ht, &old_iter, hash, _tsm_key_match, &key, &new_node->lfht_node);
    
    if (replace_result == -ENOENT) {
        tklog_notice("could not replace node because it was removed within the attempt of updating it\n");
        tklog_timer_stop();
        return TSM_RESULT_NODE_IS_REMOVED;
    } else if (replace_result != 0) {
        tklog_error("Failed to replace node in hash table\n");
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_timer_stop();
        return TSM_RESULT_NODE_REPLACEMENT_FAILURE;
    }

    // Schedule old node for RCU cleanup
    tklog_scope(call_rcu(&old_node->rcu_head, p_type->fn_free_callback));
    if (new_node->key_type == TSM_KEY_TYPE_UINT64) {
        tklog_debug("Successfully updated node with number key %llu\n", new_node->key_union.uint64);
    } else {
        tklog_debug("Successfully updated node with string key %s\n", new_node->key_union.string);
    }
    tklog_timer_stop();
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_node_upsert(struct tsm_base_node* p_tsm_base, struct tsm_base_node* new_node) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (!new_node) {
        tklog_error("new_node is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }

    tklog_timer_start();

    struct tsm_key key = { .key_union = new_node->key_union, .key_type = new_node->key_type };
    tklog_scope(TSM_Result tsm_result = tsm_key_is_valid(&key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("key is not valid. code: %d\n", tsm_result);
        tklog_timer_stop();
        return tsm_result;
    }
    struct tsm_key type_key = { .key_union = new_node->type_key_union, .key_type = new_node->type_key_type };
    tklog_scope(tsm_result = tsm_key_is_valid(&type_key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        tklog_scope(tsm_node_print(p_tsm_base, new_node));
        tklog_error("type_key is not valid. code: %d\n", tsm_result);
        tklog_timer_stop();
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not TSM\n");
        tklog_timer_stop();
        return tsm_result;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    uint64_t hash = _tsm_hash_key(new_node->key_union, new_node->key_type);
    struct cds_lfht_node *old_lfht_node = cds_lfht_add_replace(p_tsm->p_ht, hash, _tsm_key_match, &key, &new_node->lfht_node);
    struct tsm_base_node *old_node = NULL;
    if (old_lfht_node != NULL) {
        old_node = caa_container_of(old_lfht_node, struct tsm_base_node, lfht_node);
        struct tsm_key old_node_type_key = { .key_union = old_node->type_key_union, .key_type = old_node->type_key_type };
        struct tsm_base_node* p_type_old_node_base = NULL;
        tklog_scope(tsm_result = tsm_node_get(p_tsm_base, old_node_type_key, &p_type_old_node_base));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            tklog_timer_stop();
            return tsm_result;
        }
        struct tsm_base_type_node* p_type_node = caa_container_of(p_type_old_node_base, struct tsm_base_type_node, base);
        if (!p_type_node->fn_free_callback) {
            tklog_error("free_callback is NULL\n");
            tklog_scope(tsm_node_print(p_tsm_base, new_node));
            tklog_timer_stop();
            return TSM_RESULT_NULL_FUNCTION_POINTER;
        }
        // Use the type's free callback for old node (since types match)
        tklog_scope(call_rcu(&old_node->rcu_head, p_type_node->fn_free_callback));
        tklog_debug("Successfully updated node through upsert\n");
    } else {
        tklog_debug("Successfully inserted node through upsert\n");
    }

#ifdef TSM_DEBUG
    tklog_scope(tsm_result = tsm_node_is_valid(p_tsm_base, new_node));
    if (tsm_result != TSM_RESULT_NODE_IS_VALID) {
        tklog_warning("after upsert then running tsm_node_is_valid for the node it returned false. code: %d (possible concurrent removal)\n", tsm_result);
    }
#endif

    tklog_timer_stop();
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_node_defer_free(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    if (!p_tsm_base || !p_base) {
        tklog_error("NULL arguments not valid\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tsm_base_node_print(p_tsm_base);
        tklog_error("given p_tsm_base is not a TSM\n");
        return tsm_result;
    }

    tklog_timer_start();

    #ifdef TSM_DEBUG
        tklog_scope(tsm_result = tsm_node_is_valid(p_tsm_base, p_base));
        if (tsm_result == TSM_RESULT_NODE_NOT_FOUND_SELF || tsm_result == TSM_RESULT_NODE_NOT_FOUND || tsm_result == TSM_RESULT_NODE_IS_REMOVED) {
            tklog_warning("node is not found. Are you sure you're trying to defer free the correct node inside the correct TSM. If you are sure then another thread has removed it\n");
            tklog_timer_stop();
            return TSM_RESULT_SUCCESS;
        } else if (tsm_result != TSM_RESULT_NODE_IS_VALID) {
            tklog_error("base node to defer free is not valid with code %d\n", tsm_result);
            tklog_timer_stop();
            return tsm_result;
        }

        // if p_base is type and is not base type then we want to check all nodes if this key is found as type key in any other node
        struct tsm_key base_key = { .key_union = p_base->key_union, .key_type = p_base->key_type };
        struct tsm_key base_type_key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type };
        tklog_scope(tsm_result = tsm_key_match(base_key, base_type_key));
        if (tsm_result != TSM_RESULT_KEYS_MATCH) {
            if (tsm_result != TSM_RESULT_KEYS_DONT_MATCH) {
                tklog_error("tsm_key_match failed with code %d\n", tsm_result);
                tklog_timer_stop();
                return tsm_result;
            }
            tklog_scope(tsm_result = tsm_node_is_type(p_base));
            if (tsm_result == TSM_RESULT_NODE_IS_TYPE) {
                struct cds_lfht_iter iter;
                tklog_scope(bool node_exists = tsm_iter_first(p_tsm_base, &iter));
                while (node_exists) {
                    struct tsm_base_node* p_iter_base = NULL;
                    tklog_scope(tsm_result = tsm_iter_get_node(&iter, &p_iter_base));
                    if (tsm_result != TSM_RESULT_SUCCESS) {
                        tklog_timer_stop();
                        return tsm_result;
                    }
                    struct tsm_key other_key = { .key_union = p_iter_base->type_key_union, .key_type = p_iter_base->type_key_type };
                    tklog_scope(tsm_result = tsm_key_match(base_key, other_key));
                    if (tsm_result != TSM_RESULT_KEYS_MATCH) {
                        tsm_node_print(p_tsm_base, p_base);
                        tsm_node_print(p_tsm_base, p_iter_base);
                        tklog_timer_stop();
                        return tsm_result;
                    }
                    tklog_scope(node_exists = tsm_iter_next(p_tsm_base, &iter));
                }
            } else if (tsm_result == TSM_RESULT_NODE_NOT_TYPE) {
                tklog_info("p_base is not a type\n");
            } else {
                tklog_error("tsm_node_is_type() failed with code %d\n", tsm_result);
                tklog_timer_stop();
                return tsm_result;
            }
        }
    #endif

    // schdule node for free callback via call_rcu
    struct tsm_key type_key = { .key_union.string = p_base->type_key_union.string, .key_type = p_base->type_key_type };
    struct tsm_base_node* p_type_base = NULL;
    tklog_scope(tsm_result = tsm_node_get(p_tsm_base, type_key, &p_type_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_timer_stop();
        return tsm_result;
    }
    struct tsm_base_type_node* p_type = caa_container_of(p_type_base, struct tsm_base_type_node, base);
    #ifdef TSM_DEBUG
        tklog_scope(tsm_result = tsm_node_is_valid(p_tsm_base, p_type_base));
        if (tsm_result != TSM_RESULT_NODE_IS_VALID) {
            tklog_error("type node is not valid\n");
            tklog_timer_stop();
            return tsm_result;
        }
        if (!p_type->fn_free_callback) {
            tklog_scope(tsm_node_print(p_tsm_base, p_base));
            tklog_error("free_callback is NULL\n");
            tklog_timer_stop();
            return TSM_RESULT_NULL_FUNCTION_POINTER;
        }
    #endif

    // Logically removing the node from the hashtable
    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    int32_t del_result = cds_lfht_del(p_tsm->p_ht, &p_base->lfht_node);
    if (del_result == -ENOENT) {
        tklog_warning("node is already removed\n");
        tklog_timer_stop();
        return TSM_RESULT_NODE_IS_REMOVED;
    } else if (del_result != 0) {
        tklog_error("Failed to delete node thread safe map. del_result = %d\n", del_result);
        tklog_scope(tsm_node_print(p_tsm_base, p_base));
        tklog_timer_stop();
        return TSM_RESULT_UNKNOWN; 
    }
    #ifdef TSM_DEBUG
        tklog_scope(tsm_result = tsm_node_is_removed(p_base));
        if (tsm_result != TSM_RESULT_NODE_IS_REMOVED) {
            tklog_error("p_base was not removed\n");
            tklog_timer_stop();
            return tsm_result;
        }
    #endif

    // Schedule for RCU cleanup after successful removal
    call_rcu(&p_base->rcu_head, p_type->fn_free_callback);

    tklog_timer_stop();

    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_node_copy_key(struct tsm_base_node* p_base, struct tsm_key* p_output_key) {
    if (!p_base || !p_output_key) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    struct tsm_key key = { .key_union = p_base->key_union, .key_type = p_base->key_type };
    tklog_scope(TSM_Result res = tsm_key_copy(key, p_output_key));
    if (res != TSM_RESULT_SUCCESS) {
        tklog_error("tsm_key_copy failed with code %d\n", res);
        return res;
    }
    return TSM_RESULT_SUCCESS;
}  
TSM_Result tsm_node_copy_key_type(struct tsm_base_node* p_base, struct tsm_key* p_output_key) {
    if (!p_base || !p_output_key) {
        tklog_error("NULL arguments\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    struct tsm_key key = { .key_union = p_base->type_key_union, .key_type = p_base->type_key_type };
    tklog_scope(TSM_Result res = tsm_key_copy(key, p_output_key));
    if (res != TSM_RESULT_SUCCESS) {
        tklog_error("tsm_key_copy failed with code %d\n", res);
        return res;
    }
    return TSM_RESULT_SUCCESS;
}  
TSM_Result tsm_node_is_removed(struct tsm_base_node* node) {
    if (!node) {
        tklog_warning("given node is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT; // NULL node is considered removed
    }
    tklog_scope(bool result = cds_lfht_is_node_deleted(&node->lfht_node));
    if (!result) {
        return TSM_RESULT_NODE_NOT_REMOVED;
    }
    return TSM_RESULT_NODE_IS_REMOVED;
}
TSM_Result tsm_nodes_count(struct tsm_base_node* p_tsm_base, uint64_t* p_output_count) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return TSM_RESULT_SUCCESS;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not TSM\n");
        return tsm_result;
    }

    tklog_timer_start();

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    long split_count_before, split_count_after;
    // cds_lfht_count_nodes MUST be called from within read-side critical section
    // according to URCU specification
    cds_lfht_count_nodes(p_tsm->p_ht, &split_count_before, p_output_count, &split_count_after);

    tklog_timer_stop();
    
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_print(struct tsm_base_node* p_tsm_base) {
    if (!p_tsm_base) {
        tklog_error("p_tsm_base is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not TSM\n");
        return tsm_result;
    }

    tklog_timer_start();

    struct cds_lfht_iter iter;
    tklog_scope(TSM_Result iter_status = tsm_iter_first(p_tsm_base, &iter));
    while (iter_status == TSM_RESULT_SUCCESS) {
        struct tsm_base_node* p_base = NULL;
        tklog_scope(tsm_result = tsm_iter_get_node(&iter, &p_base));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            tklog_timer_stop();
            return tsm_result;
        }

        tklog_scope(tsm_node_print(p_tsm_base, p_base));

        tklog_scope(iter_status = tsm_iter_next(p_tsm_base, &iter));
    }
    if (iter_status != TSM_RESULT_ITER_END) {
        tklog_error("code %d\n", iter_status);
        tklog_timer_stop();
        return iter_status;
    }

    tklog_timer_stop();

    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_iter_first(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    if (!p_tsm_base) {
        tklog_error("tsm_iter_first called with NULL p_tsm_base\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (!iter) {
        tklog_error("iter is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not TSM\n");
        return tsm_result;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    tklog_scope(cds_lfht_first(p_tsm->p_ht, iter));
    if (iter->node == NULL) {
        return TSM_RESULT_ITER_END;
    }
    
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_iter_next(struct tsm_base_node* p_tsm_base, struct cds_lfht_iter* iter) {
    if (!p_tsm_base || !iter) {
        tklog_error("tsm_iter_next called with NULL parameters or uninitialized system\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not TSM\n");
        return TSM_RESULT_SUCCESS;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    tklog_scope(cds_lfht_next(p_tsm->p_ht, iter));
    if (!iter) {
        tklog_error("iter became NULL which it never should be\n");
        return TSM_RESULT_ITER_IS_NULL;
    }
    if (iter->node == NULL) {
        return TSM_RESULT_ITER_END;
    }
    
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_iter_get_node(struct cds_lfht_iter* iter, struct tsm_base_node** pp_output_node) {
    if (!iter || !pp_output_node) {
        tklog_error("tsm_iter_next called with NULL parameters or uninitialized system\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    if (iter->node == NULL) {
        tklog_warning("iter->node == NULL which means there is no node for this iter\n");
        return TSM_RESULT_ITER_END;
    }
    tklog_scope(struct cds_lfht_node* lfht_node = cds_lfht_iter_get_node(iter));
    if (!lfht_node) {
        return TSM_RESULT_NODE_NOT_FOUND;
    }
    *pp_output_node = caa_container_of(lfht_node, struct tsm_base_node, lfht_node);
    return TSM_RESULT_SUCCESS;
}
TSM_Result tsm_iter_lookup(struct tsm_base_node* p_tsm_base, struct tsm_key key, struct cds_lfht_iter* iter) {
    if (!p_tsm_base || !iter) {
        tklog_error("tsm_lookup_iter called with NULL parameters\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    tklog_scope(TSM_Result tsm_result = tsm_key_is_valid(&key));
    if (tsm_result != TSM_RESULT_KEY_IS_VALID) {
        tklog_error("number key is not valid with code %d\n", tsm_result);
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_is_tsm(p_tsm_base));
    if (tsm_result != TSM_RESULT_NODE_IS_TSM) {
        tklog_error("given p_tsm_base is not TSM\n");
        return tsm_result;
    }

    struct tsm* p_tsm = caa_container_of(p_tsm_base, struct tsm, base);
    
    // Calculate hash
    uint64_t hash = _tsm_hash_key(key.key_union, key.key_type);
    tklog_scope(cds_lfht_lookup(p_tsm->p_ht, hash, _tsm_key_match, &key, iter));
    
    return TSM_RESULT_SUCCESS;
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
        return TSM_RESULT_GTSM_ALREADY_INITIALIZED;
    }
    rcu_read_unlock();

    // Register the node size function with urcu_safe  system
    urcu_safe_set_node_size_function(_tsm_get_node_size);
    // Register the node bounds functions with urcu_safe system
    urcu_safe_set_node_start_ptr_function(_tsm_get_node_start_ptr);

    // create the GTSM node
    struct tsm_key gtsm_key = {0};
    tklog_scope(TSM_Result tsm_result = tsm_key_copy(g_gtsm_key, &gtsm_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct tsm_key gtsm_type_key = {0};
    tklog_scope(tsm_result = tsm_key_copy(g_tsm_type_key, &gtsm_type_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct tsm_base_node* p_new_gtsm_base = NULL;
    tklog_scope(tsm_result = tsm_base_node_create(gtsm_key, gtsm_type_key, sizeof(struct tsm), false, true, &p_new_gtsm_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tsm_key_free(&gtsm_key);
        tsm_key_free(&gtsm_type_key);
        tklog_error("Failed to create base node for new TSM\n");
        return tsm_result;
    }
    struct tsm* p_new_gtsm = caa_container_of(p_new_gtsm_base, struct tsm, base);
    p_new_gtsm->p_ht = cds_lfht_new(8,8,0,CDS_LFHT_AUTO_RESIZE,NULL);
    if (!p_new_gtsm->p_ht) {
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        tklog_error("Failed to create lock free hash table\n");
        return TSM_RESULT_UNKNOWN;
    }
    p_new_gtsm->path.length = 0;
    p_new_gtsm->path.key_chain = NULL;


    // insert base_type into GTSM
    struct tsm_key base_key = {0};
    tklog_scope(tsm_result = tsm_key_copy(g_base_type_key, &base_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct tsm_base_node* base_type_base = NULL;
    tklog_scope(tsm_result = tsm_base_type_node_create(
        base_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_base_type_node_free_callback,
        _tsm_base_type_node_is_valid,
        _tsm_base_type_node_print,
        sizeof(struct tsm_base_type_node),
        &base_type_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tsm_key_free(&base_key);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        tklog_error("Failed to create base_type node\n");
        return tsm_result;
    }
    uint64_t hash = _tsm_hash_key(base_type_base->key_union, base_type_base->key_type);
    rcu_read_lock();
    tklog_scope(struct cds_lfht_node* result = cds_lfht_add_unique(p_new_gtsm->p_ht, hash, _tsm_key_match, &base_key, &base_type_base->lfht_node));
    if (result != &base_type_base->lfht_node) {
        _tsm_base_type_node_free_callback(&base_type_base->rcu_head);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        tklog_error("Failed to insert base_type node into hash table\n");
        return TSM_RESULT_NODE_INSERTION_FAILURE;
    }
    

    // insert tsm_type into GTSM
    struct tsm_key tsm_type_key = {0};
    tklog_scope(tsm_result = tsm_key_copy(g_tsm_type_key, &tsm_type_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct tsm_base_node* tsm_type_base = NULL;
    tklog_scope(tsm_result = tsm_base_type_node_create(
        tsm_type_key, 
        sizeof(struct tsm_base_type_node),
        _tsm_tsm_type_free_callback,
        _tsm_tsm_type_is_valid,
        _tsm_tsm_type_print,
        sizeof(struct tsm),
        &tsm_type_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tsm_key_free(&tsm_type_key);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        return tsm_result;
    }
    hash = _tsm_hash_key(tsm_type_base->key_union, tsm_type_base->key_type);
    tklog_scope(result = cds_lfht_add_unique(p_new_gtsm->p_ht, hash, _tsm_key_match, &tsm_type_key, &tsm_type_base->lfht_node));
    if (result != &tsm_type_base->lfht_node) {
        _tsm_base_type_node_free_callback(&tsm_type_base->rcu_head);
        _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
        rcu_read_unlock();
        tklog_error("Failed to insert base_type node into hash table\n");
        return TSM_RESULT_NODE_INSERTION_FAILURE;
    }

    // validating inserts
    #ifdef TSM_DEBUG
        struct tsm_base_node* base_type_node = NULL;
        tklog_scope(tsm_result = tsm_node_get(p_new_gtsm_base, g_base_type_key, &base_type_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
            rcu_read_unlock();
            return tsm_result;
        }

        struct tsm_base_node* tsm_type_node = NULL;
        tklog_scope(tsm_result = tsm_node_get(p_new_gtsm_base, g_tsm_type_key, &tsm_type_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            _tsm_tsm_type_free_callback(&p_new_gtsm_base->rcu_head);
            rcu_read_unlock();
            return tsm_result;
        }
    #endif
        
    rcu_read_unlock();
    
    void* old_pointer = rcu_cmpxchg_pointer(&GTSM, NULL, p_new_gtsm_base);
    if (old_pointer != NULL) {
        tklog_notice("failed to swap pointer\n");
        return TSM_RESULT_CMPXCHG_FAILURE;
    }

    tklog_debug("Created GTSM and inserted base_type and tsm_type into it\n");

    return TSM_RESULT_SUCCESS;
}
struct tsm_base_node* gtsm_get() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    return GTSM_rcu;
}
TSM_Result gtsm_free() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (GTSM_rcu == NULL) {
        tklog_info("GTSM is NULL");
        return TSM_RESULT_GTSM_NOT_INITIALIZED;
    }

    // assigning NULL now so that no more nodes can be inserted into GTSM when we try to remove all nodes inside GTSM
    // actually even though GTSM is assigned to NULL threads that is inside a long read could still access the old GTSM
    // and insert into GTSM even after all nodes are scheduled to be removed. How HOW TO FIX THIS????
    for (int32_t count = 0; count < 5; count++) {
        void* old_pointer = rcu_cmpxchg_pointer(&GTSM, GTSM_rcu, NULL);
        if (old_pointer == GTSM_rcu) { break; }
        if (count == 5) {
            tklog_notice("failed to swap pointer to NULL\n");
            return TSM_RESULT_CMPXCHG_FAILURE;
        }
    }

    tklog_scope(TSM_Result is_tsm = tsm_node_is_tsm(GTSM_rcu));
    if (is_tsm != TSM_RESULT_NODE_IS_TSM) {
        tklog_warning("GTSM is not a TSM\n");
        return is_tsm;
    }
    
    // Schedule for RCU cleanup after successful removal
    call_rcu(&GTSM_rcu->rcu_head, _tsm_tsm_type_free_callback);

    return TSM_RESULT_SUCCESS;
}
TSM_Result gtsm_print() {
    struct tsm_base_node* GTSM_rcu = rcu_dereference(GTSM);
    if (!GTSM_rcu) {
        tklog_error("Global thread safe map not initialized\n");
        return TSM_RESULT_GTSM_NOT_INITIALIZED;
    }

    tklog_info("Printing GTSM:\n");
    tklog_scope(tsm_base_node_print(GTSM_rcu));

    struct cds_lfht_iter iter;
    tklog_scope(TSM_Result iter_valid = tsm_iter_first(GTSM_rcu, &iter));
    while (iter_valid == TSM_RESULT_SUCCESS) {
        struct tsm_base_node* iter_node = NULL;
        tklog_scope(TSM_Result tsm_result = tsm_iter_get_node(&iter, &iter_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            return tsm_result;
        }
        tklog_scope(tsm_result = tsm_node_print(GTSM_rcu, iter_node));
        if (tsm_result != TSM_RESULT_SUCCESS) {
            return tsm_result;
        }
        tklog_scope(iter_valid = tsm_iter_next(GTSM_rcu, &iter));
    }
    if (iter_valid != TSM_RESULT_ITER_END) {
        tklog_error("iter is unvalid after its used with code %d\n", iter_valid);
        return iter_valid;
    }

    return TSM_RESULT_SUCCESS;
}