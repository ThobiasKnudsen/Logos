#include "global_data/core.h"
#include "global_data/type.h"
#include "tklog.h"
#include "xxhash.h"

#include <stdatomic.h>

static struct cds_lfht *g_p_ht = NULL;
static _Atomic uint64_t g_key_counter = 1; // 0 is invalid. 1 is the first valid key

// Wrapper structure for key matching
struct gd_key_match_ctx {
    const void* key;
    bool key_is_number;
};

uint64_t _murmur3_64(const char *key, uint32_t len, uint64_t seed) {
    const uint64_t c1 = 0x87c37b91114253d5ULL;
    const uint64_t c2 = 0x4cf5ad432745937fULL;
    const int r1 = 31;
    const int r2 = 27;
    const int r3 = 33;
    const uint64_t m = 5;
    const uint64_t n1 = 0x52dce729;
    const uint64_t n2 = 0x38495ab5;

    uint64_t hash = seed;

    const int nblocks = len / 8;
    const uint64_t *blocks = (const uint64_t *) key;
    
    for (int i = 0; i < nblocks; i++) {
        uint64_t k = blocks[i];
        k *= c1;
        k = (k << r1) | (k >> (64 - r1));
        k *= c2;

        hash ^= k;
        hash = ((hash << r2) | (hash >> (64 - r2))) * m + n1;
    }

    const uint8_t *tail = (const uint8_t *) (key + nblocks * 8);
    uint64_t k1 = 0;

    switch (len & 7) {
    case 7: k1 ^= ((uint64_t)tail[6]) << 48;
    case 6: k1 ^= ((uint64_t)tail[5]) << 40;
    case 5: k1 ^= ((uint64_t)tail[4]) << 32;
    case 4: k1 ^= ((uint64_t)tail[3]) << 24;
    case 3: k1 ^= ((uint64_t)tail[2]) << 16;
    case 2: k1 ^= ((uint64_t)tail[1]) << 8;
    case 1: k1 ^= ((uint64_t)tail[0]);
        k1 *= c1;
        k1 = (k1 << r1) | (k1 >> (64 - r1));
        k1 *= c2;
        hash ^= k1;
    }

    hash ^= len;
    hash ^= (hash >> r3);
    hash *= 0xff51afd7ed558ccdULL;
    hash ^= (hash >> r3);
    hash *= 0xc4ceb9fe1a85ec53ULL;
    hash ^= (hash >> r3);

    return hash;
}

// Unified key matching function that handles both number and string keys
int _gd_key_match(struct cds_lfht_node *node, const void *key_ctx) {
    struct gd_base_node *base_node = caa_container_of(node, struct gd_base_node, lfht_node);
    struct gd_key_match_ctx *ctx = (struct gd_key_match_ctx *)key_ctx;
    
    // Check if key types match
    if (base_node->key_is_number != ctx->key_is_number) {
        return 0; // No match if key types don't match
    }
    
    if (ctx->key_is_number) {
        // Number key comparison
        uint64_t node_key = *(uint64_t*)base_node->key;
        uint64_t lookup_key = *(const uint64_t*)ctx->key;
        tklog_debug("Comparing number key: node_key=%llu, lookup_key=%llu\n", node_key, lookup_key);
        return node_key == lookup_key;
    } else {
        // String key comparison
        const char* node_key = (const char*)base_node->key;
        const char* lookup_key = (const char*)ctx->key;
        tklog_debug("Comparing string key: node_key=%s, lookup_key=%s\n", node_key, lookup_key);
        return strcmp(node_key, lookup_key) == 0;
    }
}

uint64_t _gd_hash_string(const char* string) {
    return _murmur3_64(string, strlen(string), 0);
}

uint64_t _gd_hash_u64(uint64_t key) {
    return XXH3_64bits(&key, sizeof(uint64_t));
}

// Initialize the global data system
bool gd_init(void) {
    if (g_p_ht != NULL) {
        tklog_warning("Global data system already initialized\n");
        return true;
    }
    
    g_p_ht = cds_lfht_new(8, 8, 0, CDS_LFHT_AUTO_RESIZE, NULL);
    if (!g_p_ht) {
        tklog_error("Failed to create global hash table\n");
        return false;
    }
    
    // Initialize the fundamental type node first
    if (!_gd_init_fundamental_type()) {
        tklog_error("Failed to initialize fundamental type node\n");
        cds_lfht_destroy(g_p_ht, NULL);
        g_p_ht = NULL;
        return false;
    }
    
    tklog_info("Global data system initialized\n");
    return true;
}

// Cleanup the global data system
void gd_cleanup(void) {
    if (g_p_ht) {
        // TODO: Properly cleanup all remaining nodes
        cds_lfht_destroy(g_p_ht, NULL);
        g_p_ht = NULL;
    }
}

// Get key counter for internal use
uint64_t _gd_get_next_key(void) {
    return atomic_fetch_add(&g_key_counter, 1);
}

// Get hash table for internal use
struct cds_lfht* _gd_get_hash_table(void) {
    return g_p_ht;
}

// UNIFIED FUNCTIONS

// must call rcu_read_lock before and rcu_read_unlock when no longer using this data
void* gd_get_unsafe(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return NULL;
    }
    if (!key) {
        tklog_error("key is NULL\n");
        return NULL;
    }
    if (key_is_number && *(const uint64_t*)key == 0) {
        tklog_error("number key is 0\n");
        return NULL;
    }

    uint64_t key_hash;
    if (key_is_number) {
        key_hash = _gd_hash_u64(*(const uint64_t*)key);
        tklog_debug("Looking up number key %llu with hash %llu\n", *(const uint64_t*)key, key_hash);
    } else {
        key_hash = _gd_hash_string((const char*)key);
        tklog_debug("Looking up string key %s with hash %llu\n", (const char*)key, key_hash);
    }
    
    struct gd_key_match_ctx ctx = { .key = key, .key_is_number = key_is_number };
    struct cds_lfht_iter iter;
    cds_lfht_lookup(g_p_ht, key_hash, _gd_key_match, &ctx, &iter);
    struct cds_lfht_node* p_lfht_node = cds_lfht_iter_get_node(&iter);

    if (!p_lfht_node) {
        if (key_is_number) {
            tklog_debug("Node for number key %llu not found\n", *(const uint64_t*)key);
        } else {
            tklog_debug("Node for string key %s not found\n", (const char*)key);
        }
        return NULL;
    }
    
    struct gd_base_node* p_base = caa_container_of(p_lfht_node, struct gd_base_node, lfht_node);
    
    return (void*)p_base;
}

void* gd_get_copy(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number) {
    // This function would need more implementation to actually copy data
    // For now, just return the unsafe version
    return gd_get_unsafe(key, key_is_number, type_key, type_key_is_number);
}

// returns 1 on success for string keys, the newly created numeric key for auto-generated keys, or 0 on failure
uint64_t gd_create_node(const void* key, bool key_is_number, const void* type_key, bool type_key_is_number) {
    if (!type_key) {
        tklog_error("type_key is NULL\n");
        return 0;
    }
    if (type_key_is_number && *(const uint64_t*)type_key == 0) {
        tklog_error("type_key number is 0\n");
        return 0;
    }
    if (!key_is_number && !key) {
        tklog_error("string key is NULL\n");
        return 0;
    }

    // Look up the type node
    uint64_t type_key_hash;
    if (type_key_is_number) {
        type_key_hash = _gd_hash_u64(*(const uint64_t*)type_key);
    } else {
        type_key_hash = _gd_hash_string((const char*)type_key);
    }

    rcu_read_lock();

    struct gd_key_match_ctx type_ctx = { .key = type_key, .key_is_number = type_key_is_number };
    struct cds_lfht_iter iter;
    cds_lfht_lookup(g_p_ht, type_key_hash, _gd_key_match, &type_ctx, &iter);
    struct cds_lfht_node* p_lfht_type_node = cds_lfht_iter_get_node(&iter);

    if (!p_lfht_type_node) {
        if (type_key_is_number) {
            tklog_notice("didnt find type_node for given type_key: %lld\n", *(const uint64_t*)type_key);
        } else {
            tklog_notice("didnt find type_node for given type_key: %s\n", (const char*)type_key);
        }
        rcu_read_unlock();
        return 0;
    }

    struct gd_base_node* p_base_type = caa_container_of(p_lfht_type_node, struct gd_base_node, lfht_node);
    struct gd_type_node* p_type_node = caa_container_of(p_base_type, struct gd_type_node, base);
    struct gd_base_node* p_new_node = malloc(p_type_node->type_size);

    rcu_read_unlock();

    if (!p_new_node) {
        tklog_error("malloc failed to allocate %d bytes\n", p_type_node->type_size);
        return 0;
    }

    cds_lfht_node_init(&p_new_node->lfht_node);
    p_new_node->key_is_number = key_is_number;
    p_new_node->type_key_is_number = type_key_is_number;
    
    // Set up the key
    uint64_t actual_key = 0;
    if (key_is_number) {
        if (key) {
            // Use provided numeric key
            actual_key = *(const uint64_t*)key;
        } else {
            // Auto-generate numeric key
            actual_key = atomic_fetch_add(&g_key_counter, 1);
        }
        p_new_node->key = malloc(sizeof(uint64_t));
        if (!p_new_node->key) {
            tklog_error("malloc failed to allocate numeric key\n");
            free(p_new_node);
            return 0;
        }
        *(uint64_t*)p_new_node->key = actual_key;
    } else {
        // String key
        const char* str_key = (const char*)key;
        p_new_node->key = malloc(strlen(str_key) + 1);
        if (!p_new_node->key) {
            tklog_error("malloc failed to allocate string key\n");
            free(p_new_node);
            return 0;
        }
        strcpy((char*)p_new_node->key, str_key);
    }
    
    // Set up the type key
    if (type_key_is_number) {
        p_new_node->type_key = malloc(sizeof(uint64_t));
        if (!p_new_node->type_key) {
            tklog_error("malloc failed to allocate numeric type key\n");
            free(p_new_node->key);
            free(p_new_node);
            return 0;
        }
        *(uint64_t*)p_new_node->type_key = *(const uint64_t*)type_key;
    } else {
        const char* str_type_key = (const char*)type_key;
        p_new_node->type_key = malloc(strlen(str_type_key) + 1);
        if (!p_new_node->type_key) {
            tklog_error("malloc failed to allocate string type key\n");
            free(p_new_node->key);
            free(p_new_node);
            return 0;
        }
        strcpy((char*)p_new_node->type_key, str_type_key);
    }

    // Calculate hash and insert
    uint64_t new_key_hash;
    if (key_is_number) {
        new_key_hash = _gd_hash_u64(actual_key);
        tklog_debug("Creating node with number key %llu, hash %llu\n", actual_key, new_key_hash);
    } else {
        new_key_hash = _gd_hash_string((const char*)key);
        tklog_debug("Creating node with string key %s, hash %llu\n", (const char*)key, new_key_hash);
    }

    // Insert the new node into the hash table
    rcu_read_lock();
    struct gd_key_match_ctx new_ctx = { .key = p_new_node->key, .key_is_number = key_is_number };
    struct cds_lfht_node* p_same_lfht_node = cds_lfht_add_unique(g_p_ht, 
                                                                new_key_hash, _gd_key_match, 
                                                                &new_ctx, &p_new_node->lfht_node);
    rcu_read_unlock();
    
    if (p_same_lfht_node != &p_new_node->lfht_node) {
        if (key_is_number) {
            tklog_critical("Somehow node with number key %llu already exists\n", actual_key);
        } else {
            tklog_critical("Somehow node with string key %s already exists\n", (const char*)key);
        }
        free(p_new_node->key);
        free(p_new_node->type_key);
        free(p_new_node);
        return 0;
    }

    if (key_is_number) {
        tklog_debug("Successfully inserted node with number key %llu\n", actual_key);
        return actual_key;
    } else {
        tklog_debug("Successfully inserted node with string key %s\n", (const char*)key);
        return 1; // Success for string keys
    }
}

bool gd_free_node(const void* key, bool key_is_number) {
    if (!g_p_ht) {
        tklog_error("Global data system not initialized\n");
        return false;
    }
    if (!key) {
        tklog_error("key is NULL\n");
        return false;
    }
    if (key_is_number && *(const uint64_t*)key == 0) {
        tklog_error("trying to delete node by number key which is 0\n");
        return false;
    }

    uint64_t key_hash;
    if (key_is_number) {
        key_hash = _gd_hash_u64(*(const uint64_t*)key);
    } else {
        key_hash = _gd_hash_string((const char*)key);
    }

    rcu_read_lock();
    struct gd_key_match_ctx ctx = { .key = key, .key_is_number = key_is_number };
    struct cds_lfht_iter iter;
    cds_lfht_lookup(g_p_ht, key_hash, _gd_key_match, &ctx, &iter);
    struct cds_lfht_node* p_lfht_node = cds_lfht_iter_get_node(&iter);

    if (!p_lfht_node) {
        if (key_is_number) {
            tklog_debug("trying to delete node with number key %lld, which doesnt exist\n", *(const uint64_t*)key);
        } else {
            tklog_debug("trying to delete node with string key %s, which doesnt exist\n", (const char*)key);
        }
        rcu_read_unlock();
        return false;
    }

    struct gd_base_node* p_base = caa_container_of(p_lfht_node, struct gd_base_node, lfht_node);

    // Look up the type node to get the free callback
    uint64_t type_key_hash;
    if (p_base->type_key_is_number) {
        type_key_hash = _gd_hash_u64(*(uint64_t*)p_base->type_key);
    } else {
        type_key_hash = _gd_hash_string((const char*)p_base->type_key);
    }

    struct gd_key_match_ctx type_ctx = { .key = p_base->type_key, .key_is_number = p_base->type_key_is_number };
    cds_lfht_lookup(g_p_ht, type_key_hash, _gd_key_match, &type_ctx, &iter);
    struct cds_lfht_node* p_lfht_type_node = cds_lfht_iter_get_node(&iter);

    if (!p_lfht_type_node) {
        if (key_is_number) {
            tklog_critical("found node by given number key %lld, but didnt find type_node\n", *(const uint64_t*)key);
        } else {
            tklog_critical("found node by given string key %s, but didnt find type_node\n", (const char*)key);
        }
        rcu_read_unlock();
        return false;
    }

    // Remove from hash table
    if (cds_lfht_del(g_p_ht, p_lfht_node) != 0) {
        if (key_is_number) {
            tklog_error("Failed to delete node with number key %llu from hash table\n", *(const uint64_t*)key);
        } else {
            tklog_error("Failed to delete node with string key %s from hash table\n", (const char*)key);
        }
        rcu_read_unlock();
        return false;
    }

    struct gd_base_node* p_type_base_node = caa_container_of(p_lfht_type_node, struct gd_base_node, lfht_node);
    struct gd_type_node* p_type_node = caa_container_of(p_type_base_node, struct gd_type_node, base);

    call_rcu(&p_base->rcu_head, p_type_node->fn_free_node_callback);

    rcu_read_unlock();
    return true;
}

// CONVENIENCE FUNCTIONS FOR BACKWARD COMPATIBILITY

void* gd_get_by_number_unsafe(uint64_t key) {
    return gd_get_unsafe(&key, true, NULL, false);
}

void* gd_get_by_string_unsafe(const char* key) {
    return gd_get_unsafe(key, false, NULL, false);
}

uint64_t gd_create_node_number(uint64_t type_key) {
    return gd_create_node(NULL, true, &type_key, true);
}

uint64_t gd_create_node_string(const char* key, uint64_t type_key) {
    return gd_create_node(key, false, &type_key, true);
}

bool gd_free_node_number(uint64_t key) {
    return gd_free_node(&key, true);
}

bool gd_free_node_string(const char* key) {
    return gd_free_node(key, false);
}
