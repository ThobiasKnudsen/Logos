// ast/regex_trie_2.c
#include "ast/regex_trie_2.h"
#include <stdlib.h> // For malloc, free, qsort.
#include <stdio.h> // For printf, fprintf.
#include <string.h> // For memcpy.
#include <stdbool.h> // For bool.

// Fixed-size array implementation for uint8_t keys (0-255).
// Null children[i] means no branch; is_end marks EOW (no slot 0 waste).
struct regex_trie_2 {
    void* children[256];
    bool is_end;
    uint8_t num_cached;      // 0-8: How many in cached_kids.
    uint8_t cached_kids[8];  // Pre-sorted hot keys (a-z bias).
    void* cached_ptrs[8];    // Parallel pointers.
};

// Helper: Length of null-terminated uint8_t string.
static size_t uint8len(const uint8_t *s) {
    size_t len = 0;
    if (!s) return 0;
    while (s[len] != 0) {
        ++len;
    }
    return len;
}

// Trichotomy comparator for qsort (sorting uint8_t keys for print).
static int cmp_uint8(const void* lhs, const void* rhs) {
    uint8_t l = *(const uint8_t*)lhs;
    uint8_t r = *(const uint8_t*)rhs;
    return (l > r) - (l < r);
}

// In regex_trie_2.c: Helper to update cache after insert (call on new child).
static void update_cache(regex_trie_2* node, uint8_t new_key) {
    // Simple: If <8 kids total, add to cache. Else, insert if "hot" (a-z,0-9).
    if (node->num_cached < 8) {
        // Bubble-sort insert for lex order (rare, small N).
        for (int i = node->num_cached; i > 0; --i) {
            if (new_key < node->cached_kids[i-1]) {
                node->cached_kids[i] = node->cached_kids[i-1];
                node->cached_ptrs[i] = node->cached_ptrs[i-1];
            } else {
                break;
            }
        }
        node->cached_kids[node->num_cached] = new_key;
        node->cached_ptrs[node->num_cached] = node->children[new_key];
        ++node->num_cached;
    } else if (new_key >= 'a' && new_key <= 'z') {  // Heuristic: Prioritize letters.
        // Evict least-recent (simple shift; or LRU if profiled).
        memmove(node->cached_kids, node->cached_kids + 1, 7 * sizeof(uint8_t));
        memmove(node->cached_ptrs, node->cached_ptrs + 1, 7 * sizeof(void*));
        node->cached_kids[0] = new_key;
        node->cached_ptrs[0] = node->children[new_key];
    }
}

// Recursive helper to print words: Builds prefix in buffer, prints when EOW hit.
static void print_words_recursive(const regex_trie_2* p_trie, uint8_t* word_buffer, size_t* current_len, size_t buffer_size) {
    if (!p_trie) return;

    // Step 1: Check for EOW at this node.
    if (p_trie->is_end) {
        // Null-terminate and print the current word.
        if (*current_len < buffer_size - 1) {
            word_buffer[*current_len] = 0;
            printf("%s\n", (const char*)word_buffer);
        } else {
            // Rare: Word too long; truncate and print.
            word_buffer[buffer_size - 1] = 0;
            printf("%s... (truncated)\n", (const char*)word_buffer);
        }
    }

    // Step 2: Collect non-null child keys for sorted iteration.
    enum { MAX_CHILDREN_PRINT = 1024 };
    uint8_t keys[MAX_CHILDREN_PRINT];
    size_t num_keys = 0;
    for (int i = 0; i < 256; ++i) {
        void* val = p_trie->children[i];
        if (val != NULL && num_keys < MAX_CHILDREN_PRINT) {
            keys[num_keys++] = (uint8_t)i;
        } else if (num_keys >= MAX_CHILDREN_PRINT) {
            fprintf(stderr, "Warning: Node has >%d children; truncating print.\n", MAX_CHILDREN_PRINT);
            break;
        }
    }

    // Sort keys lexicographically.
    qsort(keys, num_keys, sizeof(uint8_t), cmp_uint8);

    // Step 3: Recurse on sorted children.
    for (size_t i = 0; i < num_keys; ++i) {
        uint8_t key = keys[i];
        if (*current_len >= buffer_size - 1) {
            fprintf(stderr, "Warning: Word buffer overflow; skipping branch.\n");
            continue;
        }
        // Append key to buffer.
        word_buffer[(*current_len)++] = key;
        // Recurse.
        print_words_recursive((const regex_trie_2*)p_trie->children[key],
                              word_buffer, current_len, buffer_size);
        // Pop: Backtrack.
        (*current_len)--;
    }
}

// Create/initialize an empty root trie node.
CM_RES regex_trie_2_create(regex_trie_2** pp_output_trie) {
    CM_ASSERT(pp_output_trie);
    CM_ASSERT(*pp_output_trie == NULL);
    *pp_output_trie = (regex_trie_2*)malloc(sizeof(regex_trie_2));
    if (!*pp_output_trie) {
        return CM_RES_ALLOCATION_FAILURE;
    }
    // Zero the array and is_end.
    memset((*pp_output_trie)->children, 0, sizeof((*pp_output_trie)->children));
    (*pp_output_trie)->is_end = false;
    (*pp_output_trie)->num_cached = 0;
    memset((*pp_output_trie)->cached_kids, 0, sizeof((*pp_output_trie)->cached_kids));
    memset((*pp_output_trie)->cached_ptrs, 0, sizeof((*pp_output_trie)->cached_ptrs));
    return CM_RES_SUCCESS;
}

// Updated insert: Check cache first (linear scan on 8 = negligible).
CM_RES regex_trie_2_insert(regex_trie_2* p_trie, const uint8_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    CM_TIMER_START();
    size_t string_length = uint8len(p_string);
    regex_trie_2* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        uint8_t c = p_string[i];
        void* child = NULL;

        // Fast path: Scan tiny cache first (80% hit rate est.).
        for (uint8_t j = 0; j < p_current_trie->num_cached; ++j) {
            if (p_current_trie->cached_kids[j] == c) {
                child = p_current_trie->cached_ptrs[j];
                break;
            }
        }

        // Slow path: Full array if miss.
        if (!child) {
            child = p_current_trie->children[c];
        }

        if (!child) {
            // Create new.
            regex_trie_2* p_new_trie = NULL;
            CM_RES create_res = regex_trie_2_create(&p_new_trie);
            if (create_res != CM_RES_SUCCESS) {
                regex_trie_2_destroy(p_new_trie);
                CM_TIMER_STOP();
                return create_res;
            }
            p_current_trie->children[c] = p_new_trie;
            child = p_new_trie;
            // Update cache (bias to common keys).
            update_cache(p_current_trie, c);
        }
        p_current_trie = (regex_trie_2*)child;
        if (!p_current_trie) {
            CM_TIMER_STOP();
            return CM_RES_NULL_ARGUMENT;
        }
    }
    p_current_trie->is_end = true;
    CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}

// Search for a word; returns true if found (end-of-word marked).
bool regex_trie_2_get(regex_trie_2* p_trie, const uint8_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    // CM_TIMER_START();
    size_t string_length = uint8len(p_string);
    regex_trie_2* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        uint8_t c = p_string[i];
        if (!p_current_trie->children[c]) {
            // CM_TIMER_STOP();
            return false; // Missing child.
        }
        p_current_trie = (regex_trie_2*)p_current_trie->children[c];
        if (!p_current_trie) {
            // CM_TIMER_STOP();
            return false;
        }
    }
    // CM_TIMER_STOP();
    return p_current_trie->is_end;
}

// Recursive destroy: Cleans children and internals.
void regex_trie_2_destroy(regex_trie_2* p_trie) {
    if (!p_trie) return;
    // Recurse on all non-null children.
    for (int i = 0; i < 256; ++i) {
        void* val = p_trie->children[i];
        if (val != NULL) {
            regex_trie_2_destroy((regex_trie_2*)val);
        }
    }
    free(p_trie);
}

// Print all words in the trie, one per line, in lexicographic order.
// (depth param ignored for this flat output.)
void regex_trie_2_print(const regex_trie_2* p_trie, int depth) {
    (void)depth; // Unused.
    if (!p_trie) {
        printf("(null trie)\n");
        return;
    }
    enum { MAX_WORD_LEN = 1024 };
    uint8_t word_buffer[MAX_WORD_LEN];
    size_t current_len = 0;
    print_words_recursive(p_trie, word_buffer, &current_len, MAX_WORD_LEN);
}

// Find the longest prefix match in the input buffer.
// Outputs matched byte length and associated value (nullptr for EOW sentinel).
CM_RES regex_trie_2_longest_prefix(const regex_trie_2* p_trie, const uint8_t* p_input, size_t input_len,
                                   size_t* p_matched_len, void** pp_value) {
    if (!p_trie || input_len == 0) {
        if (p_matched_len) *p_matched_len = 0;
        if (pp_value) *pp_value = NULL;
        return CM_RES_SUCCESS;
    }
    // CM_TIMER_START();
    const regex_trie_2* current = p_trie;
    size_t matched = 0;
    void* last_value = NULL;
    for (size_t i = 0; i < input_len; ++i) {
        uint8_t c = p_input[i];
        if (!current->children[c]) break; // Mismatch.
        current = (const regex_trie_2*)current->children[c];
        if (!current) break;
        // Check EOW (update last_value if needed; here nullptr for set-like).
        if (current->is_end) {
            last_value = NULL;
        }
        ++matched;
    }
    if (p_matched_len) *p_matched_len = matched;
    if (pp_value) *pp_value = last_value;
    // CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}
