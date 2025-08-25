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
static struct tsm_key g_node_1_type_key = { .key_union.string = "node_1_type", .key_is_number = false };
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
    if (!tsm_base_node_free(p_base)) {
        tklog_error("failed to free node_1\n");
    }
}
static int node_1_try_free_callback(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    if (!p_tsm_base || !p_base) {
        tklog_error("NULL arguments\n");
        return -1;
    }
    if (!tsm_base_node_free_callback(p_tsm_base, p_base)) {
        tklog_error("tsm_base_node_free_callback failed\n");
        return -1;
    }
    return 1;
}
static bool node_1_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    if (!tsm_base_node_is_valid(p_tsm_base, p_base)) {
        tklog_notice("base node is not valid\n");
        return false;
    }
    struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
    if (p_node_1->x < 0) {
        tklog_notice("rectangle is less than zero in x axis for some areas\n");
        return false;
    }
    if (p_node_1->y < 0) {
        tklog_notice("rectangle is less than zero in y axis for some areas\n");
        return false;
    }
    return true;
}
static bool node_1_print(struct tsm_base_node* p_base) {
    if (!tsm_base_node_print(p_base)) {
        return false;
    }
    struct node_1* p_node_1 = caa_container_of(p_base, struct node_1, base);
    tklog_info(" x y width height: %f %f %f %f\n", p_node_1->x, p_node_1->y, p_node_1->width, p_node_1->height);
    tklog_info(" red green blue alpha: %d %d %d %d\n", p_node_1->red, p_node_1->green, p_node_1->blue, p_node_1->alpha);
    return true;
}
bool node_1_create_in_tsm(
    struct tsm_base_node* p_tsm,
    float x, float y, float width, float height,
    unsigned char red, unsigned char green, unsigned char blue, unsigned char alpha,
    struct tsm_key* out_key) {
    tklog_scope(struct tsm_key node_1_key = tsm_key_create(0, NULL, true));
    tklog_scope(struct tsm_key node_1_type_key = tsm_key_copy(g_node_1_type_key));
    tklog_scope(struct tsm_base_node* p_new_node_1_base = tsm_base_node_create(node_1_key, node_1_type_key, sizeof(struct node_1), false, false));
    if (!p_new_node_1_base) {
        tsm_key_free(&node_1_key);
        tsm_key_free(&node_1_type_key);
        return false;
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
    rcu_read_lock();
    tklog_scope(bool insert_result = tsm_node_insert(p_tsm, p_new_node_1_base));
    if (!insert_result) {
        tklog_scope(node_1_try_free_callback(p_tsm, p_new_node_1_base));
        gtsm_print();
        rcu_read_unlock();
        return false;
    }
    if (out_key) {
        tklog_scope(*out_key = tsm_key_copy((struct tsm_key){.key_union = p_new_node_1_base->key_union, .key_is_number = p_new_node_1_base->key_is_number}));
    }
    rcu_read_unlock();
    return true;
}
// Define another custom type for testing: simple_int_node
static struct tsm_key g_simple_int_type_key = { .key_union.string = "simple_int_type", .key_is_number = false };
struct simple_int_node {
    struct tsm_base_node base;
    int value;
};
static void simple_int_free_callback(struct rcu_head* p_rcu_head) {
    tklog_warning("simple_int_free_callback\n");
    struct tsm_base_node* p_base = caa_container_of(p_rcu_head, struct tsm_base_node, rcu_head);
    tklog_scope(bool free_result = tsm_base_node_free(p_base));
    if (!free_result) {
        tklog_error("failed to free simple_int_node\n");
    }
}
static int simple_int_try_free_callback(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
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
static bool simple_int_is_valid(struct tsm_base_node* p_tsm_base, struct tsm_base_node* p_base) {
    tklog_scope(bool is_valid = tsm_base_node_is_valid(p_tsm_base, p_base));
    if (!is_valid) {
        return false;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    return p_node->value >= 0;
}
static bool simple_int_print(struct tsm_base_node* p_base) {
    tklog_scope(bool print_result = tsm_base_node_print(p_base));
    if (!print_result) {
        return false;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    tklog_info(" value: %d\n", p_node->value);
    return true;
}
bool simple_int_create_in_tsm(struct tsm_base_node* p_tsm, int value, struct tsm_key* out_key) {
    tklog_scope(struct tsm_key key = tsm_key_create(0, NULL, true));
    tklog_scope(struct tsm_key type_key = tsm_key_copy(g_simple_int_type_key));
    tklog_scope(struct tsm_base_node* p_base = tsm_base_node_create(key, type_key, sizeof(struct simple_int_node), false, false));
    if (!p_base) {
        tsm_key_free(&key);
        tsm_key_free(&type_key);
        return false;
    }
    struct simple_int_node* p_node = caa_container_of(p_base, struct simple_int_node, base);
    p_node->value = value;
    rcu_read_lock();
    tklog_scope(bool insert_result = tsm_node_insert(p_tsm, p_base));
    if (!insert_result) {
        tklog_scope(simple_int_try_free_callback(p_tsm, p_base));
        rcu_read_unlock();
        return false;
    }
    if (out_key) {
        tklog_scope(*out_key = tsm_key_copy((struct tsm_key){.key_union = p_base->key_union, .key_is_number = p_base->key_is_number}));
    }
    rcu_read_unlock();
    return true;
}
// Helper to free non-inserted nodes
static bool tsm_base_node_free_non_inserted(struct tsm_base_node* p_base) {
    if (!p_base) return false;
    if (p_base->this_is_tsm) {
        struct tsm* p_tsm = (struct tsm*)p_base;
        cds_lfht_destroy(p_tsm->p_ht, NULL);
        if (p_tsm->path_length > 0) free(p_tsm->path_from_global_to_parent_tsm);
    }
    tsm_key_union_free(p_base->key_union, p_base->key_is_number);
    tsm_key_union_free(p_base->type_key_union, p_base->type_key_is_number);
    free(p_base);
    return true;
}
// Function to create and insert custom type nodes
bool create_custom_types(struct tsm_base_node* p_tsm_base) {
    struct tsm_base_node* p_gtsm = gtsm_get();
    // Create node_1_type
    struct tsm_key base_type_key = tsm_key_create(0, "base_type", false);
    struct tsm_key node1_type_key = tsm_key_copy(g_node_1_type_key);
    struct tsm_base_node* node1_type_base = tsm_base_type_node_create(node1_type_key, sizeof(struct tsm_base_type_node),
                                                                      node_1_free_callback, node_1_try_free_callback,
                                                                      node_1_is_valid, node_1_print, sizeof(struct node_1));
    if (!node1_type_base) {
        tsm_key_free(&base_type_key);
        tsm_key_free(&node1_type_key);
        return false;
    }
    bool insert_node1_type = tsm_node_insert(p_tsm_base, node1_type_base);
    if (!insert_node1_type) {
        tsm_base_node_free_non_inserted(node1_type_base);
        tsm_key_free(&base_type_key);
        return false;
    }
    // Create simple_int_type
    struct tsm_key simple_type_key = tsm_key_copy(g_simple_int_type_key);
    struct tsm_base_node* simple_type_base = tsm_base_type_node_create(simple_type_key, sizeof(struct tsm_base_type_node),
                                                                       simple_int_free_callback, simple_int_try_free_callback,
                                                                       simple_int_is_valid, simple_int_print, sizeof(struct simple_int_node));
    if (!simple_type_base) {
        tsm_key_free(&base_type_key);
        tsm_key_free(&simple_type_key);
        return false;
    }
    bool insert_simple_type = tsm_node_insert(p_tsm_base, simple_type_base);
    if (!insert_simple_type) {
        tsm_base_node_free_non_inserted(simple_type_base);
        tsm_key_free(&base_type_key);
        return false;
    }
    tsm_key_free(&base_type_key);
    return true;
}
static const unsigned long g_types_count = 4;
static bool filter_content(struct tsm_base_node* node) {
    return !node->this_is_type;
}
static bool filter_tsm(struct tsm_base_node* node) {
    return node->this_is_tsm;
}
static bool pick_random_key(struct tsm_key* out_key, bool (*filter)(struct tsm_base_node*)) {
    unsigned long count = tsm_nodes_count(gtsm_get());
    if (count == 0) return false;
    int attempts = 0;
    while (attempts < 20) {
        unsigned long r = (unsigned long)rand() % count;
        struct cds_lfht_iter iter;
        bool found = tsm_iter_first(gtsm_get(), &iter);
        if (found) {
            for (unsigned long i = 0; i < r; i++) {
                if (!tsm_iter_next(gtsm_get(), &iter)) {
                    break;
                }
            }
            struct tsm_base_node* node = tsm_iter_get_node(&iter);
            if (node && filter(node)) {
                *out_key = tsm_key_copy((struct tsm_key){.key_union = node->key_union, .key_is_number = node->key_is_number});
                return true;
            }
        }
        attempts++;
    }
    return false;
}
// Stress thread function
void* stress_thread(void* arg) {
    rcu_register_thread();
    srand(time(NULL) ^ (intptr_t)pthread_self() ^ (intptr_t)arg);
    for (int op = 0; op < 5000; op++) {
        int r = rand() % 100;
        rcu_read_lock();
        unsigned long total = tsm_nodes_count(gtsm_get());
        rcu_read_unlock();
        unsigned long content_count = total > g_types_count ? total - g_types_count : 0;
        if (r < 30 || content_count == 0) { // Prefer inserts if no keys or 30% chance
            int type = rand() % 3;
            struct tsm_key k;
            bool success = false;
            rcu_read_lock();
            tklog_scope(struct tsm_base_node* p_gtsm = gtsm_get());
            if (type == 0) {
                tklog_scope(success = node_1_create_in_tsm(p_gtsm, (float)(rand() % 100), (float)(rand() % 100), (float)(rand() % 10 + 1), (float)(rand() % 10 + 1),
                                               rand() % 256, rand() % 256, rand() % 256, rand() % 256, &k));
            } else if (type == 1) {
                tklog_scope(success = simple_int_create_in_tsm(p_gtsm, rand(), &k));
            } else {
                tklog_scope(struct tsm_key temp_k = tsm_key_create(0, NULL, true));
                tklog_scope(struct tsm_base_node* new_tsm = tsm_create_and_insert(p_gtsm, temp_k));
                if (new_tsm) {
                    tklog_scope(k = tsm_key_copy((struct tsm_key){.key_union = new_tsm->key_union, .key_is_number = new_tsm->key_is_number}));
                    tklog_scope(bool custom_type_creation_result = create_custom_types(new_tsm));
                    if (!custom_type_creation_result) {
                        tklog_error("creating custom types failed\n");
                        int result = tsm_node_try_free_callback(p_gtsm, new_tsm);
                        if (result != 1) {
                            tklog_error("free_callback failed\n");
                            rcu_read_unlock();
                            return NULL;
                        }
                    }
                    success = true;
                } else {
                    tklog_error("tsm_create_and_insert failed\n");
                    tklog_scope(tsm_key_free(&temp_k));
                }
            }
            rcu_read_unlock();
            if (success) {
                tklog_scope(tsm_key_free(&k));
            }
        } else if (r < 50) { // Get
            rcu_read_lock();
            struct tsm_key k;
            if (!pick_random_key(&k, filter_content)) {
                rcu_read_unlock();
                continue;
            }
            tklog_scope(struct tsm_base_node* node = tsm_node_get(gtsm_get(), k));
            if (node) {
                // Test additional functions if tsm
                tklog_scope(bool is_tsm = tsm_node_is_tsm(node));
                if (is_tsm) {
                    tklog_scope(tsm_nodes_count(node));
                    tklog_scope(tsm_get_parent_tsm(node));
                }
            }
            rcu_read_unlock();
            tklog_scope(tsm_key_free(&k));
        } else if (r < 60) { // Validate
            rcu_read_lock();
            struct tsm_key k;
            if (!pick_random_key(&k, filter_content)) {
                rcu_read_unlock();
                continue;
            }
            tklog_scope(struct tsm_base_node* p_base_k = tsm_node_get(gtsm_get(), k));
            tklog_scope(bool valid = tsm_node_is_valid(gtsm_get(), p_base_k));
            rcu_read_unlock();
            if (!valid) {
                tklog_notice("Node validation failed\n");
            }
            tklog_scope(tsm_key_free(&k));
        } else if (r < 70) { // Update
            rcu_read_lock();
            struct tsm_key uk;
            if (!pick_random_key(&uk, filter_content)) {
                rcu_read_unlock();
                continue;
            }
            struct tsm_base_node* p_base = tsm_node_get(gtsm_get(), uk);
            if (!p_base) {
                rcu_read_unlock();
                tsm_key_free(&uk);
                continue;
            }
            if (tsm_node_is_type(p_base)) {
                rcu_read_unlock();
                tsm_key_free(&uk);
                continue;
            }
            struct tsm_key type_key = {p_base->type_key_union, p_base->type_key_is_number};
            int type = -1;
            if (tsm_key_match(type_key, g_node_1_type_key)) {
                type = 0;
            } else if (tsm_key_match(type_key, g_simple_int_type_key)) {
                type = 1;
            } else if (p_base->this_is_tsm) {
                type = 2;
            }
            rcu_read_unlock();
            if (type == -1 || type == 2) {
                tsm_key_free(&uk);
                continue;
            }
            struct tsm_base_node* update_base = NULL;
            bool success = false;
            if (type == 0) {
                tklog_scope(type_key = tsm_key_copy(g_node_1_type_key));
                tklog_scope(update_base = tsm_base_node_create(uk, type_key, sizeof(struct node_1), false, false));
                if (update_base) {
                    struct node_1* un = caa_container_of(update_base, struct node_1, base);
                    un->x = (float)(rand() % 100);
                    un->y = (float)(rand() % 100);
                    un->width = (float)(rand() % 10 + 1);
                    un->height = (float)(rand() % 10 + 1);
                    un->red = rand() % 256;
                    un->green = rand() % 256;
                    un->blue = rand() % 256;
                    un->alpha = rand() % 256;
                    rcu_read_lock();
                    tklog_scope(success = tsm_node_update(gtsm_get(), update_base));
                    rcu_read_unlock();
                    if (!success) {
                        tklog_scope(tsm_base_node_free_non_inserted(update_base));
                    }
                }
            } else if (type == 1) {
                tklog_scope(type_key = tsm_key_copy(g_simple_int_type_key));
                tklog_scope(update_base = tsm_base_node_create(uk, type_key, sizeof(struct simple_int_node), false, false));
                if (update_base) {
                    struct simple_int_node* un = caa_container_of(update_base, struct simple_int_node, base);
                    un->value = rand();
                    rcu_read_lock();
                    tklog_scope(success = tsm_node_update(gtsm_get(), update_base));
                    rcu_read_unlock();
                    if (!success) {
                        tklog_scope(tsm_base_node_free_non_inserted(update_base));
                    }
                }
            } // No update for tsm type in this context
            if (!update_base) {
                tklog_scope(tsm_key_free(&uk));
                if (type == 0 || type == 1) {
                    tklog_scope(tsm_key_free(&type_key));
                }
            }
        } else if (r < 80) { // Free
            rcu_read_lock();
            struct tsm_key fk;
            if (!pick_random_key(&fk, filter_content)) {
                rcu_read_unlock();
                continue;
            }
            tklog_scope(struct tsm_base_node* p_base = tsm_node_get(gtsm_get(), fk));
            if (p_base) {
                if (tsm_node_is_type(p_base)) {
                    tsm_key_free(&fk);
                    rcu_read_unlock();
                    continue;
                }
                tklog_scope(int res = tsm_node_try_free_callback(gtsm_get(), p_base));
                while (res == 0) {
                    rcu_read_unlock();
                    rcu_barrier();
                    rcu_read_lock();
                    tklog_scope(p_base = tsm_node_get(gtsm_get(), fk));
                    tklog_scope(res = tsm_node_try_free_callback(gtsm_get(), p_base));
                }
                if (res == -1) {
                    tklog_error("try free failed\n");
                }
            }
            rcu_read_unlock();
            tklog_scope(tsm_key_free(&fk));
        } else if (r < 85) { // Count
            rcu_read_lock();
            tklog_scope(tsm_nodes_count(gtsm_get()));
            rcu_read_unlock();
        } else if (r < 90) { // Iterate
            rcu_read_lock();
            struct cds_lfht_iter iter;
            tklog_scope(bool iter_first_result = tsm_iter_first(gtsm_get(), &iter));
            if (iter_first_result) {
                int iters = 0;
                bool next = true;
                while (next && iters < 100) { // Limit to avoid too long read lock
                    tklog_scope(tsm_iter_get_node(&iter));
                    tklog_scope(next = tsm_iter_next(gtsm_get(), &iter));
                    iters++;
                }
            }
            rcu_read_unlock();
        } else if (r < 95) { // Upsert
            rcu_read_lock();
            bool is_new = rand() % 2 == 0;
            int type = rand() % 3;
            struct tsm_key uk;
            if (!is_new) {
                if (!pick_random_key(&uk, filter_content)) {
                    rcu_read_unlock();
                    continue;
                }
            } else {
                tklog_scope(uk = tsm_key_create(0, NULL, true));
            }
            struct tsm_base_node* upsert_base = NULL;
            struct tsm_key type_key;
            bool success = false;
            tklog_scope(struct tsm_base_node* p_gtsm = gtsm_get());
            if (!is_new) {
                struct tsm_base_node* existing = tsm_node_get(gtsm_get(), uk);
                if (!existing) {
                    rcu_read_unlock();
                    tsm_key_free(&uk);
                    continue;
                }
                if (tsm_node_is_type(existing)) {
                    rcu_read_unlock();
                    tsm_key_free(&uk);
                    continue;
                }
                type_key = (struct tsm_key){.key_union = existing->type_key_union, .key_is_number = existing->type_key_is_number};
                if (tsm_key_match(type_key, g_node_1_type_key)) {
                    type = 0;
                } else if (tsm_key_match(type_key, g_simple_int_type_key)) {
                    type = 1;
                } else if (existing->this_is_tsm) {
                    type = 2;
                } else {
                    rcu_read_unlock();
                    tsm_key_free(&uk);
                    continue;
                }
            }
            if (type == 2 && !is_new) {
                rcu_read_unlock();
                tsm_key_free(&uk);
                continue;
            }
            if (type == 0) {
                tklog_scope(type_key = tsm_key_copy(g_node_1_type_key));
                tklog_scope(upsert_base = tsm_base_node_create(uk, type_key, sizeof(struct node_1), false, false));
                if (upsert_base) {
                    struct node_1* un = caa_container_of(upsert_base, struct node_1, base);
                    un->x = (float)(rand() % 100);
                    un->y = (float)(rand() % 100);
                    un->width = (float)(rand() % 10 + 1);
                    un->height = (float)(rand() % 10 + 1);
                    un->red = rand() % 256;
                    un->green = rand() % 256;
                    un->blue = rand() % 256;
                    un->alpha = rand() % 256;
                    tklog_scope(success = tsm_node_upsert(gtsm_get(), upsert_base));
                    if (!success) {
                        tklog_scope(tsm_base_node_free_non_inserted(upsert_base));
                    }
                }
            } else if (type == 1) {
                tklog_scope(type_key = tsm_key_copy(g_simple_int_type_key));
                tklog_scope(upsert_base = tsm_base_node_create(uk, type_key, sizeof(struct simple_int_node), false, false));
                if (upsert_base) {
                    struct simple_int_node* un = caa_container_of(upsert_base, struct simple_int_node, base);
                    un->value = rand();
                    tklog_scope(success = tsm_node_upsert(gtsm_get(), upsert_base));
                    if (!success) {
                        tklog_scope(tsm_base_node_free_non_inserted(upsert_base));
                    }
                }
            } else if (type == 2) {
                tklog_scope(upsert_base = tsm_create_and_insert(p_gtsm, uk));
                if (upsert_base) {
                    tklog_scope(bool custom_type_creation_result = create_custom_types(upsert_base));
                    if (!custom_type_creation_result) {
                        tklog_error("creating custom types failed\n");
                        int result = tsm_node_try_free_callback(p_gtsm, upsert_base);
                        if (result != 1) {
                            tklog_error("free_callback failed\n");
                            rcu_read_unlock();
                            return NULL;
                        }
                        success = false;
                    }
                } else {
                    tklog_error("tsm_create_and_insert failed\n");
                }
            }
            rcu_read_unlock();
            if (!upsert_base) {
                tklog_scope(tsm_key_free(&uk));
                if (type == 0 || type == 1) {
                    tklog_scope(tsm_key_free(&type_key));
                }
            }
        } else { // TSM-specific tests
            rcu_read_lock();
            struct tsm_key tk;
            if (!pick_random_key(&tk, filter_tsm)) {
                rcu_read_unlock();
                continue;
            }
            tklog_scope(struct tsm_base_node* p_tsm = tsm_node_get(gtsm_get(), tk));
            if (p_tsm) {
                tklog_scope(tsm_node_is_tsm(p_tsm));
                tklog_scope(tsm_node_is_type(p_tsm));
                tklog_scope(tsm_nodes_count(p_tsm));
                tklog_scope(tsm_get_parent_tsm(p_tsm));
                // Insert a random sub-node (recurse if tsm chosen, but choose non-tsm for content)
                int sub_type = rand() % 3;
                struct tsm_key sub_k;
                bool success = false;
                struct tsm_base_node* sub_base = NULL;
                if (sub_type == 2) {
                    // Create tsm inside
                    tklog_scope(struct tsm_key temp_sub_k = tsm_key_create(0, NULL, true));
                    sub_base = tsm_create_and_insert(p_tsm, temp_sub_k);
                    if (sub_base) {
                        tklog_scope(bool custom_type_creation_result = create_custom_types(sub_base));
                        if (!custom_type_creation_result) {
                            tklog_error("creating custom types failed\n");
                            int result = tsm_node_try_free_callback(p_tsm, sub_base);
                            if (result != 1) {
                                tklog_error("free_callback failed\n");
                                return NULL;
                            }
                            success = false;
                        } else {
                            tklog_scope(sub_k = tsm_key_copy((struct tsm_key){.key_union = sub_base->key_union, .key_is_number = sub_base->key_is_number}));
                            success = true;
                            // Recursively add a non-tsm content to avoid infinite recursion
                            int content_type = rand() % 2;
                            bool content_success;
                            if (content_type == 0) {
                                tklog_scope(content_success = node_1_create_in_tsm(sub_base, (float)(rand() % 100), (float)(rand() % 100), (float)(rand() % 10 + 1), (float)(rand() % 10 + 1),
                                                     rand() % 256, rand() % 256, rand() % 256, rand() % 256, NULL));
                            } else {
                                tklog_scope(content_success = simple_int_create_in_tsm(sub_base, rand(), NULL));
                            }
                            if (!content_success) {
                                tklog_error("Failed to add content to sub-TSM\n");
                            }
                        }
                    } else {
                        tklog_error("tsm_create_and_insert failed\n");
                        tklog_scope(tsm_key_free(&temp_sub_k));
                    }
                } else if (sub_type == 0) {
                    tklog_scope(success = node_1_create_in_tsm(p_tsm, (float)(rand() % 100), (float)(rand() % 100), (float)(rand() % 10 + 1), (float)(rand() % 10 + 1),
                                                   rand() % 256, rand() % 256, rand() % 256, rand() % 256, &sub_k));
                } else {
                    tklog_scope(success = simple_int_create_in_tsm(p_tsm, rand(), &sub_k));
                }
                if (success) {
                    tklog_scope(struct tsm_base_node* sub_p = tsm_node_get(p_tsm, sub_k));
                    if (sub_p) {
                        tklog_scope(tsm_node_is_valid(p_tsm, sub_p));
                        tklog_scope(tsm_node_print(p_tsm, sub_p));
                    }
                    // Iterate
                    struct cds_lfht_iter iter;
                    tklog_scope(bool iter_first_result = tsm_iter_first(p_tsm, &iter));
                    if (iter_first_result) {
                        bool next = true;
                        while (next) {
                            tklog_scope(struct tsm_base_node* ip = tsm_iter_get_node(&iter));
                            if (ip) {
                                tklog_scope(tsm_node_print(p_tsm, ip));
                            }
                            tklog_scope(next = tsm_iter_next(p_tsm, &iter));
                        }
                    }
                    // Lookup
                    tklog_scope(tsm_iter_lookup(p_tsm, sub_k, &iter));
                    tklog_scope(tsm_iter_get_node(&iter));
                    // Free sub
                    tklog_scope(int res = tsm_node_try_free_callback(p_tsm, sub_p));
                    while (res == 0) {
                        tklog_scope(res = tsm_node_try_free_callback(p_tsm, sub_p));
                    }
                    if (res == -1)
                        tklog_error("try free sub failed\n");
                    // Check is_deleted after synchronize
                    rcu_read_unlock();
                    rcu_barrier();
                    rcu_read_lock();
                    tklog_scope(sub_p = tsm_node_get(p_tsm, sub_k));
                    if (sub_p) {
                        tklog_error("node not deleted after free\n");
                    }
                    tklog_scope(tsm_key_free(&sub_k));
                }
            }
            rcu_read_unlock();
            tklog_scope(tsm_key_free(&tk));
        }
        if (op % 200 == 0) {
            usleep(rand() % 500); // Random small delay to increase concurrency contention
        }
    }
    rcu_unregister_thread();
    return NULL;
}
void stress_test() {
    tklog_info("Starting incremental stress test\n");
    for (int nthreads = 1; nthreads <= 1; nthreads *= 2) {
        tklog_info("Stress testing with %d threads\n", nthreads);
        pthread_t* threads = malloc(nthreads * sizeof(pthread_t));
        if (!threads) {
            tklog_error("Failed to allocate threads array\n");
            return;
        }
        for (int i = 0; i < nthreads; i++) {
            if (pthread_create(&threads[i], NULL, stress_thread, (void*)(intptr_t)i) != 0) {
                tklog_error("Failed to create thread\n");
            }
        }
        for (int i = 0; i < nthreads; i++) {
            pthread_join(threads[i], NULL);
        }
        free(threads);
        // Cleanup remaining keys
        tklog_scope( rcu_read_lock(); gtsm_free(); rcu_read_unlock(); );
        rcu_barrier();
        tklog_scope( rcu_read_lock(); gtsm_init(); rcu_read_unlock(); );
        tklog_info("Completed stress test with %d threads\n", nthreads);
    }
    tklog_info("Incremental stress test completed\n");
}

// TODO:
//      create JSON function inside types. 
//      create node size calculation function inside types. 
//      create gtsm_is_valid
//      add mimalloc to the project


int main() {
    tklog_info("Comprehensive test_gtsm_tsm running\n");
    tklog_scope(rcu_init());
    rcu_register_thread();
    // Test 1: Initialization
    tklog_scope(bool init_result = gtsm_init());
    if (!init_result) {
        tklog_error("gtsm_init failed\n");
        rcu_unregister_thread();
        return -1;
    }
    rcu_read_lock();
    tklog_scope(bool custom_type_creation_result = create_custom_types(gtsm_get()));
    rcu_read_unlock();
    if (!custom_type_creation_result) {
        tklog_error("creating custom types failed\n");
        return -1;
    }
  
    // Add stress test after basic tests
    rcu_barrier();
    stress_test();
    rcu_barrier();
    rcu_read_lock();
    gtsm_print();
    gtsm_free();
    rcu_read_unlock();
    rcu_barrier();
    rcu_unregister_thread();
    tklog_info("Comprehensive test completed\n");
    return 0;
}