#include "tsm.h"
#include "code_monitoring.h"
#include <pthread.h>
#include <unistd.h>
#include <stdlib.h>
#include <time.h>
#include <stdatomic.h>
#include <stdio.h>

#define MAX_KEYS 10000
// Define node_1 as in the original
static struct tsm_key g_node_1_type_key = { .key_union.string = "node_1_type", .key_type = TSM_KEY_TYPE_STRING };
struct node_1 {
    struct tsm_base_node base;
    float x;
    float y;
    float width;
    float height;
    unsigned char red;
    unsigned char green;
    unsigned char blue;
    unsigned char alpha;
};
static void node_1_free_callback(struct rcu_head* p_rcu_head) {
    struct tsm_base_node* p_base = caa_container_of(p_rcu_head, struct tsm_base_node, rcu_head);
    CM_RES free_result = tsm_base_node_free(p_base);
    if (free_result != CM_RES_SUCCESS) {
        CM_LOG_ERROR("failed to free node_1\n");
    }
}
static CM_RES node_1_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    CM_RES tsm_result = tsm_base_node_is_valid(p_tsm_base, p_base);
    if (tsm_result != CM_RES_TSM_NODE_IS_VALID) {
        return tsm_result;
    }
    struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
    if (p_node_1->x < 0) {
        CM_LOG_NOTICE("rectangle is less than zero in x axis for some areas\n");
        return CM_RES_TSM_NODE_NOT_VALID;
    }
    if (p_node_1->y < 0) {
        CM_LOG_NOTICE("rectangle is less than zero in y axis for some areas\n");
        return CM_RES_TSM_NODE_NOT_VALID;
    }
    return CM_RES_TSM_NODE_IS_VALID;
}
static CM_RES node_1_print(const struct tsm_base_node* p_base) {
    CM_SCOPE(CM_RES tsm_result = tsm_base_node_print(p_base));
    if (tsm_result != CM_RES_SUCCESS) {
        return tsm_result;
    }
    struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
    (void)p_node_1;
    CM_LOG_INFO(" x y width height: %f %f %f %f\n", p_node_1->x, p_node_1->y, p_node_1->width, p_node_1->height);
    CM_LOG_INFO(" red green blue alpha: %d %d %d %d\n", p_node_1->red, p_node_1->green, p_node_1->blue, p_node_1->alpha);
    return CM_RES_SUCCESS;
}
CM_RES node_1_create_in_tsm(
    const struct tsm_base_node* p_tsm,
    float x, float y, float width, float height,
    unsigned char red, unsigned char green, unsigned char blue, unsigned char alpha,
    struct tsm_key* p_out_key) {

    CM_ASSERT(p_out_key && p_tsm);
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, p_out_key));
    struct tsm_base_node* p_new_node_1_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(p_out_key, &g_node_1_type_key, sizeof(struct node_1), &p_new_node_1_base));
    struct node_1* p_node_1 = caa_container_of(p_new_node_1_base, struct node_1, base);
    p_node_1->x = x;
    p_node_1->y = y;
    p_node_1->width = width;
    p_node_1->height = height;
    p_node_1->red = red;
    p_node_1->green = green;
    p_node_1->blue = blue;
    p_node_1->alpha = alpha;
    CM_SCOPE(CM_RES tsm_result = tsm_node_insert(p_tsm, p_new_node_1_base));
    if (tsm_result != CM_RES_SUCCESS) {
        if (tsm_result != CM_RES_TSM_NODE_IS_REMOVED) {
            CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(p_new_node_1_base));
            CM_LOG_INFO("tsm_node_insert failed with code %d\n", tsm_result);
        }
        return tsm_result;
    }
    return CM_RES_SUCCESS;
}
// Define another custom type for testing: simple_int_node
static struct tsm_key g_simple_int_type_key = { .key_union.string = "simple_int_type", .key_type = TSM_KEY_TYPE_STRING };
struct simple_int_node {
    struct tsm_base_node base;
    int value;
};
static void simple_int_free_callback(struct rcu_head* p_rcu_head) {
    struct tsm_base_node* p_base = caa_container_of(p_rcu_head, struct tsm_base_node, rcu_head);
    CM_RES free_result = tsm_base_node_free(p_base);
    if (free_result != CM_RES_SUCCESS) {
        CM_LOG_ERROR("failed to free simple_int_node\n");
    }
}
static CM_RES simple_int_is_valid(const struct tsm_base_node* p_tsm_base, const struct tsm_base_node* p_base) {
    CM_RES tsm_result = tsm_base_node_is_valid(p_tsm_base, p_base);
    if (tsm_result != CM_RES_TSM_NODE_IS_VALID) {
        return tsm_result;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    if (p_node->value < 0) {
        CM_LOG_NOTICE("p_node->value(%d) is below 0\n", p_node->value);
        return CM_RES_TSM_NODE_NOT_VALID;
    }
    return CM_RES_TSM_NODE_IS_VALID;
}
static CM_RES simple_int_print(const struct tsm_base_node* p_base) {
    CM_SCOPE(CM_RES tsm_result = tsm_base_node_print(p_base));
    if (tsm_result != CM_RES_SUCCESS) {
        return tsm_result;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    (void)p_node;
    CM_LOG_INFO(" value: %d\n", p_node->value);
    return CM_RES_SUCCESS;
}
CM_RES simple_int_create_in_tsm(const struct tsm_base_node* p_tsm, int value, struct tsm_key* p_out_key) {
    CM_ASSERT(p_out_key && p_tsm);
    CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(0, p_out_key));
    struct tsm_base_node* p_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(p_out_key, &g_simple_int_type_key, sizeof(struct simple_int_node), &p_base));
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    p_node->value = value;
    CM_SCOPE(CM_RES tsm_result = tsm_node_insert(p_tsm, p_base));
    if (tsm_result != CM_RES_SUCCESS) {
        if (tsm_result != CM_RES_TSM_NODE_IS_REMOVED) {
            CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(p_base));
        }
        return tsm_result;
    }
    CM_SCOPE(tsm_result = tsm_node_copy_key(p_base, p_out_key));
    if (tsm_result != CM_RES_SUCCESS) {
        return tsm_result;
    }
    return CM_RES_SUCCESS;
}
// Function to create and insert custom type nodes
CM_RES create_custom_types(const struct tsm_base_node* p_tsm_base) {
    // Create node_1_type
    struct tsm_base_node* node1_type_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create(&g_node_1_type_key, sizeof(struct tsm_base_type_node),
                                            node_1_free_callback, node_1_is_valid, node_1_print,
                                            sizeof(struct node_1), &node1_type_base));
    CM_SCOPE(CM_RES tsm_result = tsm_node_insert(p_tsm_base, node1_type_base));
    if (tsm_result != CM_RES_SUCCESS) {
        CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_free(node1_type_base));
        return tsm_result;
    }
    // Create simple_int_type
    struct tsm_base_node* simple_type_base = NULL;
    CM_ASSERT(CM_RES_SUCCESS == tsm_base_type_node_create( &g_simple_int_type_key, sizeof(struct tsm_base_type_node),
                                            simple_int_free_callback, simple_int_is_valid, simple_int_print,
                                            sizeof(struct simple_int_node), &simple_type_base));
    tsm_result = tsm_node_insert(p_tsm_base, simple_type_base);
    if (tsm_result != CM_RES_SUCCESS) {
        tsm_base_node_free(simple_type_base);
        return tsm_result;
    }
    return CM_RES_SUCCESS;
}
// Add this declaration before the stress_thread function (global scope for __thread)
__thread struct tsm_path current_path;

// New path function to add to thread_safe_map.h and implement in the corresponding .c file
// CM_RES tsm_path_get_parent(struct tsm_path path, struct tsm_path* p_output_path) {
//     if (path.length == 0) {
//         return CM_RES_TSM_PATH_NOTHING_TO_REMOVE;
//     }
//     CM_RES res = tsm_path_copy(path, p_output_path);
//     if (res != CM_RES_SUCCESS) {
//         return res;
//     }
//     p_output_path->length--;
//     return CM_RES_SUCCESS;
// }

// Modified stress_thread function
void* stress_thread(void* arg) {
    CM_LOG_NOTICE("start thread stress_thread function\n");
    rcu_register_thread();
    srand(time(NULL) ^ (intptr_t)pthread_self() ^ (intptr_t)arg);
    memset(&current_path, 0, sizeof(current_path)); // Start at GTSM root
    rcu_read_lock(); CM_TIMER_START();
    for (int op = 0; op < 70000; op++) {
        CM_TIMER_START();
        rcu_read_unlock();
        rcu_read_lock(); 
        CM_TIMER_STOP();
        CM_TIMER_START();
        CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(&current_path));
        const struct tsm_base_node* p_current_node = NULL;
        CM_SCOPE(CM_RES res = tsm_node_get_by_path(gtsm_get(), &current_path, &p_current_node));
        if (res != CM_RES_SUCCESS) {
            CM_ASSERT(CM_RES_SUCCESS == tsm_path_free(&current_path));
            CM_TIMER_STOP();
            continue;
        }
        int r = rand() % 100;
        if (r < 25) { // Insert (adjusted probability)
            CM_LOG_DEBUG("op: %d insert\n", op);
            const struct tsm_base_node* p_tsm = NULL;
            CM_SCOPE(res = tsm_node_is_tsm(p_current_node));
            if (res == CM_RES_TSM_NODE_IS_TSM) {
                p_tsm = p_current_node;
            } else if (res == CM_RES_TSM_NODE_NOT_TSM) {
                uint32_t length = 0;
                CM_ASSERT(CM_RES_SUCCESS == tsm_path_length(&current_path, &length));
                CM_ASSERT(length > 0);
                CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
                if (res == CM_RES_TSM_NODE_NOT_FOUND) {
                    CM_LOG_NOTICE("parent TSM node was node found\n");
                    CM_TIMER_STOP();
                    continue;
                }
                CM_ASSERT(res == CM_RES_SUCCESS && p_tsm);
                CM_SCOPE(res = tsm_node_is_tsm(p_tsm));
                if (res == CM_RES_TSM_NODE_NOT_TSM) {
                    CM_LOG_WARNING("parent node is not a tsm which likely means it was upserted with a non TSM node\n");
                    CM_TIMER_STOP();
                    continue;
                }
                CM_ASSERT(res == CM_RES_TSM_NODE_IS_TSM);
            } else {
                CM_LOG_ERROR("tsm_node_is_tsm failed with code %d\n", res);
            }
            int type = rand() % 3;
            struct tsm_key k = {0};
            if (type == 0) {
                CM_SCOPE(res = node_1_create_in_tsm(p_tsm, (float)(rand() % 100), (float)(rand() % 100), (float)(rand() % 10 + 1), (float)(rand() % 10 + 1),
                                                   rand() % 256, rand() % 256, rand() % 256, rand() % 256, &k));
                CM_ASSERT(res == CM_RES_SUCCESS || res == CM_RES_TSM_NODE_EXISTS || res == CM_RES_TSM_NODE_IS_REMOVED);
            } else if (type == 1) {
                CM_SCOPE(res = simple_int_create_in_tsm(p_tsm, rand(), &k));
                CM_ASSERT(res == CM_RES_SUCCESS || res == CM_RES_TSM_NODE_EXISTS || res == CM_RES_TSM_NODE_IS_REMOVED);
            } else {
                struct tsm_key temp_k = {0};
                CM_SCOPE(res = tsm_key_uint64_create(0, &temp_k));
                CM_ASSERT(res == CM_RES_SUCCESS);
                struct tsm_base_node* new_tsm = NULL;
                CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_tsm, &temp_k, &new_tsm));
                CM_ASSERT(CM_RES_SUCCESS == create_custom_types(new_tsm));
                CM_SCOPE(res = tsm_node_insert(p_tsm, new_tsm));
                CM_ASSERT(res == CM_RES_SUCCESS || res == CM_RES_TSM_NODE_EXISTS || res == CM_RES_TSM_NODE_IS_REMOVED);
                if (res == CM_RES_TSM_NODE_EXISTS) {
                    CM_ASSERT(CM_RES_SUCCESS == tsm_node_defer_free(p_tsm, new_tsm));
                }
            }
            CM_TIMER_STOP();
            continue;
        } else if (r < 50) { // Validate (adjusted)
            CM_LOG_DEBUG("op: %d validate\n", op);
            if (current_path.length == 0) {
                CM_TIMER_STOP();
                continue;
            }
            const struct tsm_base_node* p_parent_tsm = NULL;
            CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == CM_RES_TSM_NODE_NOT_FOUND) {
                CM_TIMER_STOP();
                continue;
            } 
            CM_ASSERT(res == CM_RES_SUCCESS && p_parent_tsm);
            CM_SCOPE(res = tsm_node_is_tsm(p_parent_tsm));
            if (res == CM_RES_TSM_NODE_NOT_TSM) {
                CM_LOG_NOTICE("parent node is not TSM\n");
                CM_TIMER_STOP();
                continue;
            }
            CM_ASSERT(res == CM_RES_TSM_NODE_IS_TSM);
            if (res == CM_RES_SUCCESS) {
                CM_ASSERT(CM_RES_TSM_NODE_IS_VALID == tsm_node_is_valid(p_parent_tsm, p_current_node));
            }
            CM_TIMER_STOP();
            continue;
        } else if (r < 60) { // Update (adjusted)
            CM_LOG_DEBUG("op: %d update\n", op);
            if (current_path.length == 0) { 
                CM_TIMER_STOP(); continue; 
            }
            const struct tsm_base_node* p_parent_tsm = NULL;
            CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == CM_RES_TSM_NODE_NOT_FOUND) { 
                CM_TIMER_STOP(); continue; 
            }
            CM_ASSERT(res == CM_RES_SUCCESS);
            struct tsm_key key = {.key_union = p_current_node->key_union, .key_type = p_current_node->key_type};
            CM_SCOPE(res = tsm_node_is_type(p_current_node));
            if (res == CM_RES_TSM_NODE_IS_TYPE) { 
                CM_TIMER_STOP(); continue; 
            }
            CM_ASSERT(res == CM_RES_SUCCESS || res == CM_RES_TSM_NODE_NOT_TYPE);
            struct tsm_key type_key = { .key_union = p_current_node->type_key_union, .key_type = p_current_node->key_type };
            int type = -1;
            CM_SCOPE(res = tsm_key_match(&type_key, &g_node_1_type_key));
            if (res == CM_RES_TSM_KEYS_MATCH) {
                type = 0;
            } else {
                CM_SCOPE(res = tsm_key_match(&type_key, &g_simple_int_type_key));
                if (res == CM_RES_TSM_KEYS_MATCH) {
                    type = 1;
                }
            }
            CM_SCOPE(res = tsm_node_is_tsm(p_current_node));
            if (res == CM_RES_TSM_NODE_IS_TSM) {
                type = 2;
            } else if (res != CM_RES_TSM_NODE_NOT_TSM) {
                CM_LOG_ERROR("tsm_node_is_tsm failed with code %d\n", res);
            }
            if (type == -1 || type == 2) { 
                CM_TIMER_STOP(); continue; 
            }
            struct tsm_base_node* update_base = NULL;
            if (type == 0) {
                CM_ASSERT(res == CM_RES_SUCCESS);
                CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(&key, &g_node_1_type_key, sizeof(struct node_1), &update_base));
                struct node_1* un = caa_container_of(update_base, struct node_1, base);
                un->x = (float)(rand() % 100);
                un->y = (float)(rand() % 100);
                un->width = (float)(rand() % 10 + 1);
                un->height = (float)(rand() % 10 + 1);
                un->red = rand() % 256;
                un->green = rand() % 256;
                un->blue = rand() % 256;
                un->alpha = rand() % 256;
                CM_ASSERT(CM_RES_SUCCESS == tsm_node_update(p_parent_tsm, update_base));
            } else if (type == 1) {
                CM_ASSERT(res == CM_RES_SUCCESS);
                CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(&key, &g_node_1_type_key, sizeof(struct simple_int_node), &update_base));
                struct simple_int_node* un = caa_container_of(update_base, struct simple_int_node, base);
                un->value = rand();
                CM_ASSERT(CM_RES_SUCCESS == tsm_node_update(p_parent_tsm, update_base));
            }
            CM_TIMER_STOP();
            continue;
        } else if (r < 70) { // Free (adjusted)
            CM_LOG_DEBUG("op: %d free\n", op);
            if (current_path.length == 0) {
                CM_TIMER_STOP();
                continue;
            }
            const struct tsm_base_node* p_parent_tsm = NULL;
            CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == CM_RES_TSM_NODE_NOT_FOUND) {
                CM_TIMER_STOP();
                continue;
            }
            CM_ASSERT(res == CM_RES_SUCCESS);
            CM_SCOPE(res = tsm_node_is_tsm(p_parent_tsm));
            if (res == CM_RES_TSM_NODE_NOT_TSM) {
                CM_LOG_NOTICE("p_parent_node is not TSM\n");
                CM_TIMER_STOP();
                continue;
            }
            const struct tsm_base_node* p_base = p_current_node;
            CM_SCOPE(res = tsm_node_is_type(p_base));
            if (res == CM_RES_TSM_NODE_IS_TYPE) {
                CM_TIMER_STOP();
                continue;
            } 
            CM_ASSERT(res == CM_RES_SUCCESS || res == CM_RES_TSM_NODE_NOT_TYPE);
            CM_SCOPE(res = tsm_node_defer_free(p_parent_tsm, p_base));
            CM_ASSERT(res == CM_RES_SUCCESS || res == CM_RES_TSM_NODE_IS_REMOVED);
            // After free, sett current_path to parent path
            CM_ASSERT(CM_RES_SUCCESS == tsm_path_remove_key(&current_path, -1));
            CM_TIMER_STOP();
            continue;
        } else if (r < 70) { // Count (adjusted)
            CM_LOG_DEBUG("op: %d count\n", op);
            const struct tsm_base_node* p_tsm = NULL;
            CM_SCOPE(CM_RES res = tsm_node_is_tsm(p_current_node));
            if (res == CM_RES_TSM_NODE_IS_TSM) {
                p_tsm = p_current_node;
            } else if (res == CM_RES_TSM_NODE_NOT_TSM) {
                CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
                if (res == CM_RES_TSM_NODE_NOT_FOUND) {
                    CM_LOG_NOTICE("parent node not found\n");
                    CM_TIMER_STOP();
                    continue;
                } 
                CM_ASSERT(res == CM_RES_SUCCESS);
                CM_SCOPE(CM_RES res = tsm_node_is_tsm(p_tsm));
                if (res != CM_RES_TSM_NODE_IS_TSM) {
                    CM_LOG_NOTICE("parent node is not TSM\n");
                    CM_TIMER_STOP();
                    continue;
                }   
            } else {
                CM_LOG_ERROR("code %d\n", res);
            }
            uint64_t count = 0;
            CM_ASSERT(CM_RES_SUCCESS == tsm_nodes_count(p_tsm, &count));
            CM_LOG_WARNING("op: %d count = %lu\n", op, count);
            CM_TIMER_STOP();
            continue;
        } else if (r < 72) { // Iterate (adjusted)
            CM_LOG_DEBUG("op: %d iterate\n", op);
            const struct tsm_base_node* p_tsm = gtsm_get();
            if (current_path.length > 0) {
                const struct tsm_base_node* p_tsm = NULL;
                CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
                if (res == CM_RES_TSM_NODE_NOT_FOUND) {
                    CM_LOG_NOTICE("tsm_node_get_by_path_at_depth: node not found\n");
                    CM_TIMER_STOP();
                    continue;
                }
                CM_ASSERT(res == CM_RES_SUCCESS);
            }
            struct cds_lfht_iter iter;
            CM_ASSERT(CM_RES_SUCCESS == tsm_iter_first(p_tsm, &iter));
            int iters = 0;
            CM_RES next = CM_RES_SUCCESS;
            while (next == CM_RES_SUCCESS && iters < 100) { // Limit to avoid too long read lock
                const struct tsm_base_node* ip = NULL;
                CM_ASSERT(CM_RES_SUCCESS == tsm_iter_get_node(&iter, &ip));
                CM_SCOPE(next = tsm_iter_next(p_tsm, &iter));
                CM_ASSERT(next == CM_RES_SUCCESS || next == CM_RES_TSM_ITER_END);
                iters++;
            }
            CM_TIMER_STOP();
            continue;
        } else if (r < 73) { // Navigation: first child (adjusted)
            CM_LOG_DEBUG("op: %d nav first child\n", op);
            CM_SCOPE(res = tsm_node_is_tsm(p_current_node));
            if (res == CM_RES_TSM_NODE_NOT_TSM) {
                r = 95;
            } else if (res != CM_RES_TSM_NODE_IS_TSM) {
                CM_LOG_ERROR("tsm_node_is_tsm failed with code %d\n", res);
                CM_TIMER_STOP();
                continue;
            } else {
                struct cds_lfht_iter iter;
                CM_ASSERT(CM_RES_SUCCESS == tsm_iter_first(p_current_node, &iter));
                const struct tsm_base_node* p_child = NULL;
                CM_SCOPE(res = tsm_iter_get_node(&iter, &p_child));
                CM_ASSERT(res == CM_RES_SUCCESS && p_child);
                struct tsm_key child_key;
                CM_ASSERT(CM_RES_SUCCESS == tsm_node_copy_key(p_child, &child_key));
                CM_ASSERT(CM_RES_SUCCESS == tsm_path_insert_key(&current_path, &child_key, -1));
                tsm_key_free(&child_key);
                CM_ASSERT(CM_RES_TSM_PATH_VALID == tsm_path_is_valid(&current_path));
            }
        } else if (r < 74) { // Navigation: next sibling
            CM_LOG_DEBUG("op: %d nav next node\n", op);
            if (current_path.length == 0) {
                CM_TIMER_STOP();
                continue;
            }
            const struct tsm_base_node* p_parent_tsm = NULL;
            CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == CM_RES_TSM_NODE_NOT_FOUND) {
                CM_LOG_NOTICE("tsm_node_get_by_path_at_depth node not found\n");
                CM_TIMER_STOP();
                continue;
            }
            CM_ASSERT(res == CM_RES_SUCCESS && p_parent_tsm);
            CM_SCOPE(res = tsm_node_is_tsm(p_parent_tsm));
            if (res == CM_RES_TSM_NODE_NOT_TSM) {
                CM_LOG_NOTICE("parent is not a TSM\n");
                CM_TIMER_STOP();
                continue;
            }
            const struct tsm_key* p_last_key = NULL;
            CM_ASSERT(CM_RES_SUCCESS == tsm_path_get_key_ref(&current_path, -1, &p_last_key));
            struct cds_lfht_iter iter;
            CM_ASSERT(CM_RES_SUCCESS == tsm_iter_lookup(p_parent_tsm, p_last_key, &iter));
            CM_SCOPE(res = tsm_iter_next(p_parent_tsm, &iter));
            CM_ASSERT(res == CM_RES_SUCCESS || res == CM_RES_TSM_ITER_END);
            const struct tsm_base_node* next_node = NULL;
            CM_SCOPE(res = tsm_iter_get_node(&iter, &next_node));
            if (res == CM_RES_TSM_ITER_END) {
                CM_ASSERT(CM_RES_SUCCESS == tsm_iter_first(p_parent_tsm, &iter));
            }
            CM_SCOPE(res = tsm_iter_get_node(&iter, &next_node));
            CM_ASSERT(res == CM_RES_SUCCESS && next_node);
            struct tsm_key next_key = {0};
            CM_ASSERT(CM_RES_SUCCESS == tsm_node_copy_key(next_node, &next_key));
            CM_ASSERT(CM_RES_TSM_KEY_IS_VALID == tsm_key_is_valid(&next_key));
            CM_ASSERT(CM_RES_SUCCESS == tsm_path_remove_key(&current_path, -1));
            CM_ASSERT(CM_RES_SUCCESS == tsm_path_insert_key(&current_path, &next_key, -1));
            CM_ASSERT(CM_RES_SUCCESS == tsm_key_free(&next_key));
            CM_TIMER_STOP();
        } else if (r < 97) { // Navigation: parent
            CM_LOG_NOTICE("op: %d nav parent\n", op);
            const struct tsm_base_node* current_node = p_current_node;
            if (!current_node) {
                CM_TIMER_STOP();
                continue;
            }
            // Goto Parent (except GTSM)
            if (current_path.length > 0) {
                CM_ASSERT(CM_RES_SUCCESS == tsm_path_remove_key(&current_path, -1));
            }
            CM_TIMER_STOP();
        } else { // Upsert (adjusted, now covers the rest)
            CM_LOG_DEBUG("op: %d upsert\n", op);
            uint32_t length = 0;
            CM_ASSERT(CM_RES_SUCCESS == tsm_path_length(&current_path, &length));
            if (length == 0) {
                CM_LOG_DEBUG("current_path is 0 and upsert must therefore be skipped since we cant upsert the GTSM\n");
                CM_TIMER_STOP();
                continue;
            }
            const struct tsm_base_node* p_tsm = NULL;
            CM_SCOPE(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
            if (res == CM_RES_TSM_NODE_NOT_FOUND) {
                CM_TIMER_STOP();
                continue;
            }
            CM_ASSERT(res == CM_RES_SUCCESS && p_tsm);
            CM_SCOPE(res = tsm_node_is_tsm(p_tsm));
            CM_ASSERT(res == CM_RES_TSM_NODE_NOT_TSM || res == CM_RES_TSM_NODE_IS_TSM);
            if (res == CM_RES_TSM_NODE_NOT_TSM) {
                CM_LOG_NOTICE("parent tsm is no longer a TSM\n");
                CM_TIMER_STOP();
                continue;
            }
            bool is_new = rand() % 2 == 0;
            int type = rand() % 3;
            struct tsm_key uk = {0};
            if (!is_new) {
                CM_ASSERT(p_current_node);
                struct tsm_key key = {.key_union = p_current_node->key_union, .key_type = p_current_node->key_type};
                CM_ASSERT(CM_RES_SUCCESS == tsm_key_copy(&key, &uk));
                CM_LOG_DEBUG("op: %d upsert p_current_node\n", op);
            } else {
                CM_ASSERT(CM_RES_SUCCESS == tsm_key_uint64_create(rand() % 50, &uk)); // Random non-zero uint64
                CM_LOG_DEBUG("op: %d upsert maybe new\n", op);
            }
            struct tsm_base_node* upsert_base = NULL;
            const struct tsm_base_node* p_existing_node = NULL;
            CM_SCOPE(res = tsm_node_get(p_tsm, &uk, &p_existing_node));
            if (res == CM_RES_SUCCESS) {
                CM_SCOPE(res = tsm_node_is_type(p_existing_node));
                if (res == CM_RES_TSM_NODE_IS_TYPE) {
                    CM_LOG_NOTICE("will not upsert type node in test\n");
                    CM_SCOPE(tsm_key_free(&uk));
                    CM_TIMER_STOP();
                    continue;
                }
            } else if (res != CM_RES_TSM_NODE_NOT_FOUND) {
                CM_LOG_ERROR("code %d\n", res);
            }
            if (type == 0) {
                CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(&uk, &g_node_1_type_key, sizeof(struct node_1), &upsert_base));
                struct node_1* un = caa_container_of(upsert_base, struct node_1, base);
                un->x = (float)(rand() % 100);
                un->y = (float)(rand() % 100);
                un->width = (float)(rand() % 10 + 1);
                un->height = (float)(rand() % 10 + 1);
                un->red = rand() % 256;
                un->green = rand() % 256;
                un->blue = rand() % 256;
                un->alpha = rand() % 256;
            } else if (type == 1) {
                CM_ASSERT(CM_RES_SUCCESS == tsm_base_node_create(&uk, &g_simple_int_type_key, sizeof(struct simple_int_node), &upsert_base));
                struct simple_int_node* un = caa_container_of(upsert_base, struct simple_int_node, base);
                un->value = rand();
            }
            if (type == 2) {
                CM_ASSERT(CM_RES_SUCCESS == tsm_create(p_tsm, &uk, &upsert_base));
                CM_ASSERT(CM_RES_SUCCESS == create_custom_types(upsert_base));
            }
            // freeing key as it is deepcopied in its usage
            CM_SCOPE(tsm_key_free(&uk));
            // Upsert non-TSM node
            CM_ASSERT(CM_RES_SUCCESS == tsm_node_upsert(p_tsm, upsert_base));
            CM_TIMER_STOP();
        }
        // Removed TSM-specific branch as per instructions
        if (op % 200 == 0) {
            usleep(rand() % 500); // Random small delay to increase concurrency contention
        }
    }
    CM_ASSERT(CM_RES_SUCCESS == tsm_path_free(&current_path));
    rcu_read_unlock(); 
    CM_TIMER_STOP();
    rcu_unregister_thread();
    CM_TIMER_PRINT();
    CM_TIMER_CLEAR();
    return NULL;
}
void stress_test() {
    CM_LOG_INFO("Starting incremental stress test\n");
    for (int nthreads = 1; nthreads <= 8; nthreads *= 2) {
        CM_LOG_NOTICE("Stress testing with %d threads ===========================================================================================\n", nthreads);

        pthread_t* threads = malloc(nthreads * sizeof(pthread_t));
        CM_ASSERT(threads);

        CM_ASSERT(CM_RES_SUCCESS == gtsm_init());
        rcu_read_lock();
        CM_SCOPE(CM_RES custom_type_creation_result = create_custom_types(gtsm_get()));
        CM_ASSERT(custom_type_creation_result == CM_RES_SUCCESS);
        rcu_read_unlock();

        for (int i = 0; i < nthreads; i++) {
            CM_ASSERT(pthread_create(&threads[i], NULL, stress_thread, (void*)(intptr_t)i) == 0);
        }
        for (int i = 0; i < nthreads; i++) {
            pthread_join(threads[i], NULL);
            CM_LOG_NOTICE("Joining thread %d\n", i);
        }
        free(threads);
        // Cleanup remaining keys
        CM_LOG_NOTICE("Staring gtsm_free for %d threads\n", nthreads);
        rcu_read_lock();
        CM_ASSERT(CM_RES_SUCCESS == gtsm_free());
        rcu_read_unlock();

        rcu_barrier();

        CM_LOG_NOTICE("Completed stress test with %d threads\n", nthreads);
    }
    CM_LOG_NOTICE("Incremental stress test completed\n");
}
// TODO:
// create JSON function inside types.
// create node size calculation function inside types.
// create gtsm_is_valid
// add mimalloc to the project
int main() {
    CM_LOG_NOTICE("Comprehensive test_tsm running\n");
    rcu_init();
    rcu_register_thread();
    // Add stress test after basic tests
    stress_test();
    // multiple because each callback can defer new callbacks
    rcu_barrier();
    rcu_unregister_thread();
    CM_TIMER_CLEAR();
    CM_TIMER_PRINT();
    CM_LOG_INFO("Comprehensive test completed\n");
    return 0;
}