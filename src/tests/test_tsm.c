#include "tklog.h"
#include "tsm.h"
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
    TSM_Result free_result = tsm_base_node_free(p_base);
    if (free_result != TSM_RESULT_SUCCESS) {
        tklog_error("failed to free node_1\n");
    }
}
static TSM_Result node_1_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    TSM_Result tsm_result = tsm_base_node_is_valid(p_tsm_base, p_base);
    if (tsm_result != TSM_RESULT_NODE_IS_VALID) {
        return tsm_result;
    }
    struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
    if (p_node_1->x < 0) {
        tklog_notice("rectangle is less than zero in x axis for some areas\n");
        return TSM_RESULT_NODE_NOT_VALID;
    }
    if (p_node_1->y < 0) {
        tklog_notice("rectangle is less than zero in y axis for some areas\n");
        return TSM_RESULT_NODE_NOT_VALID;
    }
    return TSM_RESULT_NODE_IS_VALID;
}
static TSM_Result node_1_print(struct tsm_base_node* p_base) {
    tklog_scope(TSM_Result tsm_result = tsm_base_node_print(p_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
    (void)p_node_1;
    tklog_info(" x y width height: %f %f %f %f\n", p_node_1->x, p_node_1->y, p_node_1->width, p_node_1->height);
    tklog_info(" red green blue alpha: %d %d %d %d\n", p_node_1->red, p_node_1->green, p_node_1->blue, p_node_1->alpha);
    return TSM_RESULT_SUCCESS;
}
TSM_Result node_1_create_in_tsm(
    struct tsm_base_node* p_tsm,
    float x, float y, float width, float height,
    unsigned char red, unsigned char green, unsigned char blue, unsigned char alpha,
    struct tsm_key* out_key) {
    if (!out_key) {
        tklog_error("out_key is NULL\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    struct tsm_key node_1_key = {0};
    TSM_Result tsm_result = tsm_key_uint64_create(0, &node_1_key);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_info("tsm_key_uint64_create failed with code%d\n", tsm_result);
        return tsm_result;
    }
    struct tsm_key node_1_type_key = {0};
    tsm_result = tsm_key_copy(g_node_1_type_key, &node_1_type_key);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_scope(tsm_key_free(&node_1_key));
        tklog_info("tsm_key_copy failed with code%d\n", tsm_result);
        return tsm_result;
    }
    struct tsm_base_node* p_new_node_1_base = NULL;
    tsm_result = tsm_base_node_create(node_1_key, node_1_type_key, sizeof(struct node_1), false, false, &p_new_node_1_base);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_scope(tsm_key_free(&node_1_key));
        tklog_scope(tsm_key_free(&node_1_type_key));
        tklog_info("tsm_base_node_create failed with code%d\n", tsm_result);
        return tsm_result;
    }
    struct node_1* p_node_1 = caa_container_of(p_new_node_1_base, struct node_1, base);
    p_node_1->x = x;
    p_node_1->y = y;
    p_node_1->width = width;
    p_node_1->height = height;
    p_node_1->red = red;
    p_node_1->green = green;
    p_node_1->blue = blue;
    p_node_1->alpha = alpha;
    tsm_result = tsm_node_insert(p_tsm, p_new_node_1_base);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        if (tsm_result != TSM_RESULT_NODE_IS_REMOVED) {
            tklog_scope(TSM_Result free_result = tsm_base_node_free(p_new_node_1_base));
            if (free_result != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_base_node_free failed with code %d\n", free_result);
            };
            tklog_info("tsm_node_insert failed with code %d\n", tsm_result);
        }
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_copy_key(p_new_node_1_base, out_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        rcu_read_unlock();
        tklog_info("tsm_key_copy failed with code %d\n", tsm_result);
        return tsm_result;
    }
    return TSM_RESULT_SUCCESS;
}
// Define another custom type for testing: simple_int_node
static struct tsm_key g_simple_int_type_key = { .key_union.string = "simple_int_type", .key_type = TSM_KEY_TYPE_STRING };
struct simple_int_node {
    struct tsm_base_node base;
    int value;
};
static void simple_int_free_callback(struct rcu_head* p_rcu_head) {
    struct tsm_base_node* p_base = caa_container_of(p_rcu_head, struct tsm_base_node, rcu_head);
    TSM_Result free_result = tsm_base_node_free(p_base);
    if (free_result != TSM_RESULT_SUCCESS) {
        tklog_error("failed to free simple_int_node\n");
    }
}
static TSM_Result simple_int_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    TSM_Result tsm_result = tsm_base_node_is_valid(p_tsm_base, p_base);
    if (tsm_result != TSM_RESULT_NODE_IS_VALID) {
        return tsm_result;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    if (p_node->value < 0) {
        tklog_notice("p_node->value(%d) is below 0\n", p_node->value);
        return TSM_RESULT_NODE_NOT_VALID;
    }
    return TSM_RESULT_NODE_IS_VALID;
}
static TSM_Result simple_int_print(struct tsm_base_node* p_base) {
    tklog_scope(TSM_Result tsm_result = tsm_base_node_print(p_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    (void)p_node;
    tklog_info(" value: %d\n", p_node->value);
    return TSM_RESULT_SUCCESS;
}
TSM_Result simple_int_create_in_tsm(struct tsm_base_node* p_tsm, int value, struct tsm_key* out_key) {
    if (!out_key) {
        tklog_error("NULL argument\n");
        return TSM_RESULT_NULL_ARGUMENT;
    }
    struct tsm_key key = {0};
    TSM_Result tsm_result = tsm_key_uint64_create(0, &key);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct tsm_key type_key = {0};
    tsm_result = tsm_key_copy(g_simple_int_type_key, &type_key);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_scope(tsm_key_free(&key));
        return tsm_result;
    }
    struct tsm_base_node* p_base = NULL;
    tsm_result = tsm_base_node_create(key, type_key, sizeof(struct simple_int_node), false, false, &p_base);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_scope(tsm_key_free(&key));
        tklog_scope(tsm_key_free(&type_key));
        return tsm_result;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    p_node->value = value;
    tklog_scope(tsm_result = tsm_node_insert(p_tsm, p_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        // tklog_critical("tsm_node_insert failed with code %d\n", tsm_result);
        if (tsm_result != TSM_RESULT_NODE_IS_REMOVED) {
            tklog_scope(TSM_Result free_result = tsm_base_node_free(p_base));
            if (free_result != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_node_defer_free failed with code %d\n", free_result);
            }
        }
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_copy_key(p_base, out_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    return TSM_RESULT_SUCCESS;
}
// Function to create and insert custom type nodes
TSM_Result create_custom_types(struct tsm_base_node* p_tsm_base) {
    // Create node_1_type
    struct tsm_key node1_type_key = {0};
    tklog_scope(TSM_Result tsm_result = tsm_key_copy(g_node_1_type_key, &node1_type_key));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct tsm_base_node* node1_type_base = NULL;
    tklog_scope(tsm_result = tsm_base_type_node_create( node1_type_key, sizeof(struct tsm_base_type_node),
                                            node_1_free_callback, node_1_is_valid, node_1_print,
                                            sizeof(struct node_1), &node1_type_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_scope(tsm_key_free(&node1_type_key));
        return tsm_result;
    }
    tklog_scope(tsm_result = tsm_node_insert(p_tsm_base, node1_type_base));
    if (tsm_result != TSM_RESULT_SUCCESS) {
        TSM_Result free_result = tsm_base_node_free(node1_type_base);
        if (free_result != TSM_RESULT_SUCCESS) {
            tklog_error("tsm_base_node_free failed with code %d\n", free_result);
        }
        return tsm_result;
    }
    // Create simple_int_type
    struct tsm_key simple_type_key = {0};
    tsm_result = tsm_key_copy(g_simple_int_type_key, &simple_type_key);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        return tsm_result;
    }
    struct tsm_base_node* simple_type_base = NULL;
    tsm_result = tsm_base_type_node_create( simple_type_key, sizeof(struct tsm_base_type_node),
                                            simple_int_free_callback, simple_int_is_valid, simple_int_print,
                                            sizeof(struct simple_int_node), &simple_type_base);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tklog_scope(tsm_key_free(&simple_type_key));
        return tsm_result;
    }
    tsm_result = tsm_node_insert(p_tsm_base, simple_type_base);
    if (tsm_result != TSM_RESULT_SUCCESS) {
        tsm_base_node_free(simple_type_base);
        return tsm_result;
    }
    return TSM_RESULT_SUCCESS;
}
// Add this declaration before the stress_thread function (global scope for __thread)
__thread struct tsm_path current_path;

// New path function to add to thread_safe_map.h and implement in the corresponding .c file
// TSM_Result tsm_path_get_parent(struct tsm_path path, struct tsm_path* p_output_path) {
//     if (path.length == 0) {
//         return TSM_RESULT_PATH_NOTHING_TO_REMOVE;
//     }
//     TSM_Result res = tsm_path_copy(path, p_output_path);
//     if (res != TSM_RESULT_SUCCESS) {
//         return res;
//     }
//     p_output_path->length--;
//     return TSM_RESULT_SUCCESS;
// }

// Modified stress_thread function
void* stress_thread(void* arg) {
    tklog_notice("start thread stress_thread function\n");
    rcu_register_thread();
    srand(time(NULL) ^ (intptr_t)pthread_self() ^ (intptr_t)arg);
    memset(&current_path, 0, sizeof(current_path)); // Start at GTSM root
    rcu_read_lock(); tklog_timer_start();
    for (int op = 0; op < 1000; op++) {
        rcu_read_unlock();
        rcu_read_lock(); tklog_timer_start();
        tklog_scope(TSM_Result res = tsm_path_is_valid(&current_path));
        if (res != TSM_RESULT_PATH_VALID) {
            tklog_error("path is not valid with code %d\n", res);
            tklog_timer_stop();
            break;
        }
        struct tsm_base_node* p_current_node = NULL;
        tklog_scope(res = tsm_node_get_by_path(gtsm_get(), &current_path, &p_current_node));
        if (res != TSM_RESULT_SUCCESS) {
            tklog_scope(res = tsm_path_free(&current_path));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_path_free failed with code %d\n", res);
            }
            tklog_timer_stop();
            continue;
        }
        int r = rand() % 100;
        if (r < 25) { // Insert (adjusted probability)
            tklog_info("op: %d insert\n", op);
            struct tsm_base_node* p_tsm = NULL;
            tklog_scope(res = tsm_node_is_tsm(p_current_node));
            if (res == TSM_RESULT_NODE_IS_TSM) {
                p_tsm = p_current_node;
            }
            else if (res == TSM_RESULT_NODE_NOT_TSM) {
                uint32_t length = 0;
                tklog_scope(res = tsm_path_length(&current_path, &length));
                if (length == 0) {
                    tklog_error("somehow the current node is not TSM and is also the ground TSM aka GTSM\n");
                }
                tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
                if (res == TSM_RESULT_NODE_NOT_FOUND) {
                    tklog_notice("parent TSM node was node found\n");
                    tklog_timer_stop();
                    continue;
                }
                else if (res != TSM_RESULT_SUCCESS || !p_tsm) {
                    tklog_error("tsm_node_get_by_path_at_depth failed with code %d for validate\n", res);
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = tsm_node_is_tsm(p_tsm));
                if (res == TSM_RESULT_NODE_NOT_TSM) {
                    tklog_warning("parent node is not a tsm which likely means it was upserted with a non TSM node\n");
                    tklog_timer_stop();
                    continue;
                }
                if (res != TSM_RESULT_NODE_IS_TSM) {
                    tklog_error("p_tsm is still not a TSM with code %d\n", res);
                    tklog_timer_stop();
                    continue;
                }
            }
            else {
                tklog_error("tsm_node_is_tsm failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            int type = rand() % 3;
            struct tsm_key k = {0};
            if (type == 0) {
                tklog_scope(res = node_1_create_in_tsm(p_tsm, (float)(rand() % 100), (float)(rand() % 100), (float)(rand() % 10 + 1), (float)(rand() % 10 + 1),
                                                   rand() % 256, rand() % 256, rand() % 256, rand() % 256, &k));
                if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_NODE_EXISTS && res != TSM_RESULT_NODE_IS_REMOVED) {
                    tklog_error("node_1_create_in_tsm failed with code %d\n", res);
                } 
            } else if (type == 1) {
                tklog_scope(res = simple_int_create_in_tsm(p_tsm, rand(), &k));
                if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_NODE_EXISTS && res != TSM_RESULT_NODE_IS_REMOVED) {
                    tklog_error("simple_int_create_in_tsm failed with code %d\n", res);
                } 
            } else {
                struct tsm_key temp_k = {0};
                tklog_scope(res = tsm_key_uint64_create(0, &temp_k));
                if (res != TSM_RESULT_SUCCESS) tklog_error("tsm_key_uint64_create failed with code %d\n", res);
                struct tsm_base_node* new_tsm = NULL;
                tklog_scope(res = tsm_create(p_tsm, temp_k, &new_tsm));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_create failed with code %d\n", res);
                    tsm_key_free(&temp_k);  // Free temp key on failure
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = create_custom_types(new_tsm));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("create_custom_types failed with code %d\n", res);
                    tsm_node_defer_free(p_tsm, new_tsm);
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = tsm_node_insert(p_tsm, new_tsm));
                if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_NODE_EXISTS && res != TSM_RESULT_NODE_IS_REMOVED) {
                    tklog_error("tsm_node_insert failed with code %d\n", res);
                    tklog_scope(tsm_node_defer_free(p_tsm, new_tsm));
                    tklog_timer_stop();
                    continue;
                } else if (res == TSM_RESULT_NODE_EXISTS) {
                    tklog_scope(res = tsm_node_defer_free(p_tsm, new_tsm));
                    if (res != TSM_RESULT_SUCCESS) {
                        tklog_error("failed to defer free uninserted TSM\n");
                    }
                }
            }
            tklog_timer_stop();
            continue;
        } else if (r < 50) { // Validate (adjusted)
            tklog_info("op: %d validate\n", op);
            if (current_path.length == 0) {
                tklog_timer_stop();
                continue;
            }
            struct tsm_base_node* p_parent_tsm = NULL;
            tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == TSM_RESULT_NODE_NOT_FOUND) {
                tklog_timer_stop();
                continue;
            } else if (res != TSM_RESULT_SUCCESS || !p_parent_tsm) {
                tklog_error("tsm_node_get_by_path_at_depth failed with code %d for validate\n", res);
            }
            tklog_scope(res = tsm_node_is_tsm(p_parent_tsm));
            if (res == TSM_RESULT_NODE_NOT_TSM) {
                tklog_notice("parent node is not TSM\n");
                tklog_timer_stop();
                continue;
            }
            else if (res != TSM_RESULT_NODE_IS_TSM) {
                tklog_error("somehow node is not TSM with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            if (res == TSM_RESULT_SUCCESS) {
                tklog_scope(res = tsm_node_is_valid(p_parent_tsm, p_current_node));
                if (res != TSM_RESULT_NODE_IS_VALID) {
                    tklog_error("tsm_node_is_valid failed with code %d\n", res);
                    tklog_timer_stop();
                    continue;
                }
            }
            tklog_timer_stop();
            continue;
        } else if (r < 60) { // Update (adjusted)
            tklog_info("op: %d update\n", op);
            if (current_path.length == 0) {
                tklog_timer_stop();
                continue;
            }
            struct tsm_base_node* p_parent_tsm = NULL;
            tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == TSM_RESULT_NODE_NOT_FOUND) {
                tklog_timer_stop();
                continue;
            } else if (res != TSM_RESULT_SUCCESS || !p_parent_tsm) {
                gtsm_print();
                tklog_scope(tsm_base_node_print(p_current_node));
                tklog_scope(tsm_path_print(&current_path));
                tklog_error("tsm_node_get_by_path_at_depth failed with code %d for update\n", res);
            }
            struct tsm_key uk = {0};
            tklog_scope(res = tsm_key_copy((struct tsm_key){.key_union = p_current_node->key_union, .key_type = p_current_node->key_type}, &uk));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_key_copy failed with code %d for update key\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_node_is_type(p_current_node));
            if (res == TSM_RESULT_NODE_IS_TYPE) {
                tklog_scope(tsm_key_free(&uk));
                tklog_timer_stop();
                continue;
            } else if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_NODE_NOT_TYPE) {
                tklog_error("tsm_node_is_type failed with code %d\n", res);
                tklog_scope(tsm_key_free(&uk));
                tklog_timer_stop();
                continue;
            }
            struct tsm_key type_key = { .key_union = p_current_node->type_key_union, .key_type = p_current_node->key_type };
            int type = -1;
            tklog_scope(res = tsm_key_match(type_key, g_node_1_type_key));
            if (res == TSM_RESULT_KEYS_MATCH) {
                type = 0;
            } else {
                tklog_scope(res = tsm_key_match(type_key, g_simple_int_type_key));
                if (res == TSM_RESULT_KEYS_MATCH) {
                    type = 1;
                }
            }
            tklog_scope(res = tsm_node_is_tsm(p_current_node));
            if (res == TSM_RESULT_NODE_IS_TSM) {
                type = 2;
            } else if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_NODE_NOT_TSM) {
                tklog_error("tsm_node_is_tsm failed with code %d\n", res);
            }
            if (type == -1 || type == 2) {
                tklog_scope(tsm_key_free(&uk));
                tklog_timer_stop();
                continue;
            }
            struct tsm_base_node* update_base = NULL;
            struct tsm_key update_type_key = {0};
            TSM_Result success = TSM_RESULT_UNKNOWN;
            if (type == 0) {
                tklog_scope(res = tsm_key_copy(g_node_1_type_key, &update_type_key));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_key_copy for node_1 type failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = tsm_base_node_create(uk, update_type_key, sizeof(struct node_1), false, false, &update_base));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_base_node_create for node_1 failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_scope(tsm_key_free(&update_type_key));
                    tklog_timer_stop();
                    continue;
                }
                struct node_1* un = caa_container_of(update_base, struct node_1, base);
                un->x = (float)(rand() % 100);
                un->y = (float)(rand() % 100);
                un->width = (float)(rand() % 10 + 1);
                un->height = (float)(rand() % 10 + 1);
                un->red = rand() % 256;
                un->green = rand() % 256;
                un->blue = rand() % 256;
                un->alpha = rand() % 256;
                tklog_scope(success = tsm_node_update(p_parent_tsm, update_base));
                if (success != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_node_update for node_1 failed with code %d\n", success);
                    tsm_base_node_free(update_base);
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(tsm_key_free(&update_type_key));
            } else if (type == 1) {
                tklog_scope(res = tsm_key_copy(g_simple_int_type_key, &update_type_key));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_key_copy for simple_int type failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = tsm_base_node_create(uk, update_type_key, sizeof(struct simple_int_node), false, false, &update_base));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_base_node_create for simple_int failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_scope(tsm_key_free(&update_type_key));
                    tklog_timer_stop();
                    continue;
                }
                struct simple_int_node* un = caa_container_of(update_base, struct simple_int_node, base);
                un->value = rand();
                tklog_scope(success = tsm_node_update(p_parent_tsm, update_base));
                if (success != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_node_update for simple_int failed with code %d\n", success);
                    tsm_base_node_free(update_base);
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(tsm_key_free(&update_type_key));
            }
            tklog_scope(tsm_key_free(&uk));
            tklog_timer_stop();
            continue;
        } else if (r < 70) { // Free (adjusted)
            tklog_info("op: %d free\n", op);
            if (current_path.length == 0) {
                tklog_timer_stop();
                continue;
            }
            
            struct tsm_base_node* p_parent_tsm = NULL;
            tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == TSM_RESULT_NODE_NOT_FOUND) {
                tklog_timer_stop();
                continue;
            }
            else if (res != TSM_RESULT_SUCCESS || !p_parent_tsm) {
                tklog_error("tsm_node_get_by_path_at_depth failed with code %d for validate\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_node_is_tsm(p_parent_tsm));
            if (res == TSM_RESULT_NODE_NOT_TSM) {
                tklog_notice("p_parent_node is not TSM\n");
                tklog_timer_stop();
                continue;
            }
            struct tsm_base_node* p_base = p_current_node;
            tklog_scope(res = tsm_node_is_type(p_base));
            if (res == TSM_RESULT_NODE_IS_TYPE) {
                tklog_timer_stop();
                continue;
            } else if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_NODE_NOT_TYPE) {
                tklog_error("tsm_node_is_type failed with code %d for free\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_node_defer_free(p_parent_tsm, p_base));
            if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_NODE_IS_REMOVED) {
                tklog_error("tsm_node_defer_free failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            // After free, sett current_path to parent path
            tklog_scope(res = tsm_path_remove_key(&current_path, -1));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("core %d\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_timer_stop();
            continue;
        } else if (r < 70) { // Count (adjusted)
            tklog_info("op: %d count\n", op);
            struct tsm_base_node* p_tsm = NULL;
            tklog_scope(TSM_Result res = tsm_node_is_tsm(p_current_node));
            if (res == TSM_RESULT_NODE_IS_TSM) {
                p_tsm = p_current_node;
            } else if (res == TSM_RESULT_NODE_NOT_TSM) {
                tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
                if (res == TSM_RESULT_NODE_NOT_FOUND) {
                    tklog_notice("parent node not found\n");
                    tklog_timer_stop();
                    continue;
                } else if (res != TSM_RESULT_SUCCESS || !p_tsm) {
                    tklog_error("tsm_node_get_by_path_at_depth failed with code %d for validate\n", res);
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(TSM_Result res = tsm_node_is_tsm(p_tsm));
                if (res != TSM_RESULT_NODE_IS_TSM) {
                    tklog_notice("parent node is not TSM\n");
                    tklog_timer_stop();
                    continue;
                }   
            } else {
                tklog_error("code %d\n", res);
            }
            uint64_t count = 0;
            tklog_scope(res = tsm_nodes_count(p_tsm, &count));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_nodes_count failed with code %d\n", res);
            } else {
                tklog_warning("op: %d count = %lu\n", op, count);
            }
            tklog_timer_stop();
            continue;
        } else if (r < 72) { // Iterate (adjusted)
            tklog_info("op: %d iterate\n", op);
            struct tsm_base_node* p_tsm = gtsm_get();
            if (current_path.length > 0) {
                struct tsm_base_node* p_tsm = NULL;
                tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
                if (res == TSM_RESULT_NODE_NOT_FOUND) {
                    tklog_notice("tsm_node_get_by_path_at_depth: node not found\n");
                    tklog_timer_stop();
                    continue;
                }
                if (res != TSM_RESULT_SUCCESS || !p_tsm) {
                    tklog_error("tsm_node_get_by_path_at_depth failed with code %d for validate\n", res);
                    tklog_timer_stop();
                    continue;
                }
            }
            struct cds_lfht_iter iter;
            tklog_scope(res = tsm_iter_first(p_tsm, &iter));
            if (res == TSM_RESULT_SUCCESS) {
                int iters = 0;
                TSM_Result next = TSM_RESULT_SUCCESS;
                while (next == TSM_RESULT_SUCCESS && iters < 100) { // Limit to avoid too long read lock
                    struct tsm_base_node* ip = NULL;
                    tklog_scope(res = tsm_iter_get_node(&iter, &ip));
                    if (res != TSM_RESULT_SUCCESS) {
                        tklog_error("tsm_iter_get_node failed with code %d\n", res);
                        break;
                    }
                    if (ip) {
                        // tklog_scope(tsm_node_print(p_tsm, ip));
                    }
                    tklog_scope(next = tsm_iter_next(p_tsm, &iter));
                    if (next != TSM_RESULT_SUCCESS && next != TSM_RESULT_ITER_END) {
                        tklog_error("tsm_iter_next failed with code %d\n", next);
                        break;
                    }
                    iters++;
                }
            } else {
                tklog_error("tsm_iter_first failed with code %d\n", res);
            }
            tklog_timer_stop();
            continue;
        } else if (r < 94) { // Navigation: first child (adjusted)
            tklog_info("op: %d nav first child\n", op);
            tklog_scope(res = tsm_node_is_tsm(p_current_node));
            if (res == TSM_RESULT_NODE_NOT_TSM) {
                r = 95;
            } else if (res != TSM_RESULT_NODE_IS_TSM) {
                tklog_error("tsm_node_is_tsm failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            } else {
                struct cds_lfht_iter iter;
                tklog_scope(res = tsm_iter_first(p_current_node, &iter));
                if (res == TSM_RESULT_SUCCESS) {
                    struct tsm_base_node* p_child = NULL;
                    tklog_scope(res = tsm_iter_get_node(&iter, &p_child));
                    if (res == TSM_RESULT_SUCCESS && p_child) {
                        struct tsm_key child_key;
                        tklog_scope(res = tsm_node_copy_key(p_child, &child_key));
                        if (res != TSM_RESULT_SUCCESS) {
                            tklog_error("tsm_node_copy_key failed with code %d\n", res);
                            tsm_key_free(&child_key);
                            tklog_timer_stop();
                            continue;
                        }
                        tsm_path_print(&current_path);
                        tklog_scope(res = tsm_path_insert_key(&current_path, &child_key, -1));
                        if (res != TSM_RESULT_SUCCESS) {
                            tklog_error("tsm_path_insert_key failed with code %d\n", res);
                            tsm_key_free(&child_key);
                            tklog_timer_stop();
                            continue;
                        }
                        tsm_path_print(&current_path);
                        tklog_scope(res = tsm_path_is_valid(&current_path));
                        if (res != TSM_RESULT_PATH_VALID) {
                            tklog_error("path is invalid with code %d\n", res);
                            tsm_path_free(&current_path);
                            tklog_timer_stop();
                            continue;
                        }
                    } else {
                        tklog_error("tsm_iter_get_node for first child failed with code %d\n", res);
                    }
                } else {
                    tklog_error("tsm_iter_first for first child failed with code %d\n", res);
                }
            }
        } if (r < 96) { // Navigation: next sibling
            tklog_info("op: %d nav next node\n", op);
            if (current_path.length == 0) {
                tklog_timer_stop();
                continue;
            }
            struct tsm_base_node* p_parent_tsm = NULL;
            tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_parent_tsm));
            if (res == TSM_RESULT_NODE_NOT_FOUND) {
                tklog_notice("tsm_node_get_by_path_at_depth node not found\n");
                tklog_timer_stop();
                continue;
            }
            if (res != TSM_RESULT_SUCCESS || !p_parent_tsm) {
                tklog_error("tsm_node_get_by_path for p_parent_tsm failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_node_is_tsm(p_parent_tsm));
            if (res == TSM_RESULT_NODE_NOT_TSM) {
                tklog_notice("parent is not a TSM\n");
                tklog_timer_stop();
                continue;
            }
            struct tsm_key* p_last_key = NULL;
            tklog_scope(res = tsm_path_get_key_ref(&current_path, -1, &p_last_key));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_key_copy for p_last_key failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            struct cds_lfht_iter iter;
            tklog_scope(res = tsm_iter_lookup(p_parent_tsm, p_last_key, &iter));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_iter_lookup failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_iter_next(p_parent_tsm, &iter));
            if (res != TSM_RESULT_SUCCESS && res != TSM_RESULT_ITER_END) {
                tklog_error("Navigation next sibling failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            struct tsm_base_node* next_node = NULL;
            tklog_scope(res = tsm_iter_get_node(&iter, &next_node));
            if (res == TSM_RESULT_ITER_END) {
                tklog_scope(res = tsm_iter_first(p_parent_tsm, &iter));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_iter_first failed with code %d\n", res);
                    tklog_timer_stop();
                    continue;
                }   
            }
            tklog_scope(res = tsm_iter_get_node(&iter, &next_node));
            if (res != TSM_RESULT_SUCCESS || !next_node) {
                tklog_error("tsm_iter_get_node failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            struct tsm_key next_key = {0};
            tklog_scope(res = tsm_node_copy_key(next_node, &next_key));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_key_copy for next_key failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_key_is_valid(&next_key));
            if (res != TSM_RESULT_KEY_IS_VALID) {
                tklog_error("tsm_key_is_valid failed with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_path_remove_key(&current_path, -1));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_path_remove_key failed with code %d\n", res);
                tklog_scope(tsm_key_free(&next_key));
                tklog_scope(tsm_path_free(&current_path));
            }
            tklog_scope(res = tsm_path_insert_key(&current_path, &next_key, -1));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_path_insert_key failed with code %d\n", res);
                tklog_scope(tsm_key_free(&next_key));
                tklog_scope(tsm_path_free(&current_path));
            }
            tklog_timer_stop();
        } else if (r < 97) { // Navigation: parent
            tklog_info("op: %d nav parent\n", op);
            struct tsm_base_node* current_node = p_current_node;
            if (!current_node) {
                tklog_timer_stop();
                continue;
            }
            // Goto Parent (except GTSM)
            if (current_path.length > 0) {
                tklog_scope(res = tsm_path_remove_key(&current_path, -1));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_scope(tsm_path_free(&current_path));
                    tklog_error("tsm_path_remove_key failed with code %d\n", res);
                } 
            }
            tklog_timer_stop();
        } else { // Upsert (adjusted, now covers the rest)
            tklog_info("op: %d upsert\n", op);
            uint32_t length = 0;
            tklog_scope(res = tsm_path_length(&current_path, &length));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_path_length failed\n");
                tklog_timer_stop();
                continue;
            }
            if (length == 0) {
                tklog_debug("current_path is 0 and upsert must therefore be skipped since we cant upsert the GTSM\n");
                tklog_timer_stop();
                continue;
            }
            struct tsm_base_node* p_tsm = NULL;
            tklog_scope(res = tsm_node_get_by_path_at_depth(gtsm_get(), &current_path, -2, &p_tsm));
            if (res == TSM_RESULT_NODE_NOT_FOUND) {
                tklog_timer_stop();
                continue;
            }
            else if (res != TSM_RESULT_SUCCESS || !p_tsm) {
                tklog_error("tsm_node_get_by_path_at_depth failed with code %d for validate\n", res);
                tklog_timer_stop();
                continue;
            }
            tklog_scope(res = tsm_node_is_tsm(p_tsm));
            if (res == TSM_RESULT_NODE_NOT_TSM) {
                tklog_notice("parent tsm is no longer a TSM\n");
                tklog_timer_stop();
                continue;
            }
            else if (res != TSM_RESULT_NODE_IS_TSM) {
                tklog_error("p_tsm is not a TSM with code %d\n", res);
                tklog_timer_stop();
                continue;
            }
            bool is_new = rand() % 2 == 0;
            int type = rand() % 3;
            struct tsm_key uk = {0};
            if (!is_new) {
                if (p_current_node) {
                    struct tsm_key key = {.key_union = p_current_node->key_union, .key_type = p_current_node->key_type};
                    tklog_scope(res = tsm_key_copy(key, &uk));
                    if (res == TSM_RESULT_SUCCESS) {
                        tklog_info("op: %d upsert p_current_node\n", op);
                    } else {
                        tklog_scope(res = tsm_key_print(key));
                        tklog_error("tsm_key_copy for p_current_node upsert failed with code %d\n", res);
                        tklog_timer_stop();
                        continue;
                    }
                } else {
                    tklog_error("p_current_node is NULL for p_current_node upsert\n");
                    tklog_timer_stop();
                    continue;
                }
            } else {
                tklog_scope(res = tsm_key_uint64_create(rand() % 50, &uk)); // Random non-zero uint64
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_key_uint64_create for new upsert failed with code %d\n", res);
                    tklog_timer_stop();
                    continue;
                }
                tklog_info("op: %d upsert maybe new\n", op);
            }
            struct tsm_base_node* upsert_base = NULL;
            struct tsm_key type_key = {0};
            struct tsm_base_node* p_existing_node = NULL;
            tklog_scope(res = tsm_node_get(p_tsm, uk, &p_existing_node));
            if (res == TSM_RESULT_SUCCESS) {
                tklog_scope(res = tsm_node_is_type(p_existing_node));
                if (res == TSM_RESULT_NODE_IS_TYPE) {
                    tklog_notice("will not upsert type node in test\n");
                    tklog_scope(tsm_key_free(&uk));
                    tklog_timer_stop();
                    continue;
                }
            } else if (res != TSM_RESULT_NODE_NOT_FOUND) {
                tklog_error("code %d\n", res);
            }
            if (type == 0) {
                tklog_scope(res = tsm_key_copy(g_node_1_type_key, &type_key));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_key_copy for node_1 upsert failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = tsm_base_node_create(uk, type_key, sizeof(struct node_1), false, false, &upsert_base));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_base_node_create for node_1 upsert failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_scope(tsm_key_free(&type_key));
                    tklog_timer_stop();
                    continue;
                }
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
                tklog_scope(res = tsm_key_copy(g_simple_int_type_key, &type_key));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_key_copy for simple_int upsert failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = tsm_base_node_create(uk, type_key, sizeof(struct simple_int_node), false, false, &upsert_base));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("tsm_base_node_create for simple_int upsert failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_scope(tsm_key_free(&type_key));
                    tklog_timer_stop();
                    continue;
                }
                struct simple_int_node* un = caa_container_of(upsert_base, struct simple_int_node, base);
                un->value = rand();
            }
            if (type == 2) {
                tklog_scope(res = tsm_create(p_tsm, uk, &upsert_base));
                if (res != TSM_RESULT_SUCCESS || !upsert_base) {
                    tklog_error("tsm_create for new TSM upsert failed with code %d\n", res);
                    tklog_scope(tsm_key_free(&uk));
                    tklog_timer_stop();
                    continue;
                }
                tklog_scope(res = create_custom_types(upsert_base));
                if (res != TSM_RESULT_SUCCESS) {
                    tklog_error("create_custom_types for new TSM failed with code %d\n", res);
                    tklog_scope(res = tsm_node_defer_free(p_tsm, upsert_base));
                    if (res != TSM_RESULT_SUCCESS) {
                        tklog_error("tsm_node_defer_free for cleanup failed with code %d\n", res);
                    }
                    tklog_timer_stop();
                    continue;
                }
            }
            // Upsert non-TSM node
            tklog_scope(res = tsm_node_upsert(p_tsm, upsert_base));
            if (res != TSM_RESULT_SUCCESS) {
                tklog_error("tsm_node_upsert failed with code %d for type %d\n", res, type);
                tsm_base_node_free(upsert_base);
            }
            tklog_timer_stop();
        }
        // Removed TSM-specific branch as per instructions
        if (op % 200 == 0) {
            usleep(rand() % 500); // Random small delay to increase concurrency contention
        }
    }
    tklog_scope(TSM_Result res = tsm_path_free(&current_path));
    if (res != TSM_RESULT_SUCCESS) {
        tklog_error("Final tsm_path_free failed with code %d\n", res);
    }
    rcu_read_unlock(); tklog_timer_stop();
    rcu_unregister_thread();
    //tklog_timer_print();
    tklog_timer_clear();
    return NULL;
}
void stress_test() {
    tklog_info("Starting incremental stress test\n");
    for (int nthreads = 1; nthreads <= 1; nthreads *= 2) {
        tklog_notice("Stress testing with %d threads ===========================================================================================\n", nthreads);

        pthread_t* threads = malloc(nthreads * sizeof(pthread_t));
        if (!threads) {
            tklog_error("Failed to allocate threads array\n");
            return;
        }

        tklog_scope(TSM_Result init_result = gtsm_init());
        if (init_result != TSM_RESULT_SUCCESS) {
            tklog_error("gtsm_init failed with code %d\n", init_result);
        }
        rcu_read_lock();
        tklog_scope(TSM_Result custom_type_creation_result = create_custom_types(gtsm_get()));
        if (custom_type_creation_result != TSM_RESULT_SUCCESS) {
            tklog_error("creating custom types failed. Code: %d\n", custom_type_creation_result);
        }
        rcu_read_unlock();

        for (int i = 0; i < nthreads; i++) {
            if (pthread_create(&threads[i], NULL, stress_thread, (void*)(intptr_t)i) != 0) {
                tklog_error("Failed to create thread\n");
            }
        }
        for (int i = 0; i < nthreads; i++) {
            pthread_join(threads[i], NULL);
            tklog_notice("Joining thread %d\n", i);
        }
        free(threads);
        // Cleanup remaining keys
        tklog_notice("Staring gtsm_free for %d threads\n", nthreads);
        rcu_read_lock();
        tklog_scope(TSM_Result free_result = gtsm_free());
        if (free_result != TSM_RESULT_SUCCESS) {
            tklog_error("gtsm_free failed\n");
        }
        rcu_read_unlock();

        rcu_barrier();
        rcu_barrier();
        rcu_barrier();
        rcu_barrier();
        rcu_barrier();
        rcu_barrier();
        rcu_barrier();
        rcu_barrier();
        rcu_barrier();

        tklog_notice("Completed stress test with %d threads\n", nthreads);
    }
    tklog_notice("Incremental stress test completed\n");
}
// TODO:
// create JSON function inside types.
// create node size calculation function inside types.
// create gtsm_is_valid
// add mimalloc to the project
int main() {
    tklog_notice("Comprehensive test_gtsm_tsm running\n");
    rcu_init();
    rcu_register_thread();
    // Add stress test after basic tests
    stress_test();
    // multiple because each callback can defer new callbacks
    rcu_barrier();
    rcu_barrier();
    rcu_barrier();
    rcu_barrier();
    rcu_barrier();
    rcu_barrier();
    rcu_barrier();
    rcu_barrier();
    rcu_barrier();
    rcu_unregister_thread();
    tklog_timer_clear();
    tklog_timer_print();
    tklog_info("Comprehensive test completed\n");
    return 0;
}