#include "ast/regex_trie.h"
#include <stddef.h> // For size_t, NULL.
#include <stdlib.h> // For malloc, free.
#include <stdbool.h> // For bool.
#include <stdio.h>   // For printf, fprintf.
#include <string.h>  // For qsort? No, but for completeness.

// Trichotomy comparator for qsort (sorting keys for print).
static int cmp_uint8(const void* lhs, const void* rhs) {
    uint8_t l = *(const uint8_t*)lhs;
    uint8_t r = *(const uint8_t*)rhs;
    return (l > r) - (l < r);
}

static size_t uint8len(const uint8_t *s) {
    size_t len = 0;
    if (!s) return 0; // Handle null pointer
    while (s[len] != 0) {
        ++len;
    }
    return len;
}

// Recursive helper to print words: Builds prefix in buffer, prints when EOW hit.
static void print_words_recursive(const regex_trie* p_trie, uint8_t* word_buffer, size_t* current_len, size_t buffer_size) {
    if (!p_trie) return;
    // Step 1: Check for EOW sentinel at this node.
    internal_regex_trie_itr end_itr = internal_regex_trie_get(&p_trie->children, (uint8_t)'\0');
    if (!internal_regex_trie_is_end(end_itr) && end_itr.data->val == NULL) {
        // Null-terminate and print the current word.
        if (*current_len < buffer_size - 1) {
            word_buffer[*current_len] = '\0';
            printf("%s\n", (const char*)word_buffer);
        } else {
            // Rare: Word too long; truncate and print.
            word_buffer[buffer_size - 1] = '\0';
            printf("%s... (truncated)\n", (const char*)word_buffer);
        }
    }
    // Step 2: Collect non-sentinel child keys for sorted iteration.
    enum { MAX_CHILDREN_PRINT = 1024 };
    uint8_t keys[MAX_CHILDREN_PRINT];
    size_t num_keys = 0;
    for (internal_regex_trie_itr itr = internal_regex_trie_first(&((regex_trie*)p_trie)->children); !internal_regex_trie_is_end(itr); itr = internal_regex_trie_next(itr)) {
        uint8_t key = itr.data->key;
        void* val = itr.data->val;
        if (key != (uint8_t)'\0' && val != NULL && num_keys < MAX_CHILDREN_PRINT) { // Non-sentinel child.
            keys[num_keys++] = key;
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
        internal_regex_trie_itr child_itr = internal_regex_trie_get(&((regex_trie*)p_trie)->children, key);
        if (!internal_regex_trie_is_end(child_itr)) {
            print_words_recursive((const regex_trie*)child_itr.data->val, word_buffer, current_len, buffer_size);
        }
        // Pop: Backtrack.
        (*current_len)--;
    }
}

// Create/initialize an empty root trie node.
CM_RES regex_trie_create(regex_trie** pp_output_trie) {
    CM_ASSERT(pp_output_trie);
    CM_ASSERT(*pp_output_trie == NULL);
    *pp_output_trie = malloc(sizeof(regex_trie));
    CM_ASSERT(*pp_output_trie);
    internal_regex_trie_init(&(*pp_output_trie)->children);
    return CM_RES_SUCCESS;
}

// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
CM_RES regex_trie_insert(regex_trie* p_trie, const uint8_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    // CM_TIMER_START();
    size_t string_length = uint8len(p_string);
    regex_trie* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        uint8_t c = p_string[i];
        internal_regex_trie_itr itr = internal_regex_trie_get(&p_current_trie->children, c);
        if (internal_regex_trie_is_end(itr)) {
            // Create new child node.
            regex_trie* p_new_trie = NULL;
            CM_RES create_res = regex_trie_create(&p_new_trie);
            if (create_res != CM_RES_SUCCESS) {
                // Updated: destroy frees self.
                regex_trie_destroy(p_new_trie);
                // CM_TIMER_STOP();
                return create_res;
            }
            // Insert the new child.
            itr = internal_regex_trie_insert(&p_current_trie->children, c, (void*)p_new_trie);
            if (internal_regex_trie_is_end(itr)) {
                // Updated: destroy frees self; no extra free.
                regex_trie_destroy(p_new_trie);
                // CM_TIMER_STOP();
                return CM_RES_ALLOCATION_FAILURE;
            }
        }
        // Advance: Use itr.data->val for both new/existing cases.
        p_current_trie = (regex_trie*)itr.data->val;
        if (!p_current_trie) {
            // CM_TIMER_STOP();
            return CM_RES_NULL_ARGUMENT; // Corrupt: NULL child pointer.
        }
    }
    // Always mark/overwrite end of word sentinel (idempotent).
    internal_regex_trie_itr end_itr = internal_regex_trie_insert(&p_current_trie->children, (uint8_t)'\0', NULL);
    if (internal_regex_trie_is_end(end_itr)) {
        // CM_TIMER_STOP();
        return CM_RES_ALLOCATION_FAILURE;
    }
    // CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}

// Search for a word; returns true if found (end-of-word marked).
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    // CM_TIMER_START();
    size_t string_length = uint8len(p_string);
    regex_trie* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        uint8_t c = p_string[i];
        internal_regex_trie_itr itr = internal_regex_trie_get(&p_current_trie->children, c);
        if (internal_regex_trie_is_end(itr)) {
            // CM_TIMER_STOP();
            return CM_RES_REGEX_TRIE_NODE_NOT_FOUND; // Missing child.
        }
        p_current_trie = (regex_trie*)itr.data->val;
        if (!p_current_trie) {
            // CM_TIMER_STOP();
            return CM_RES_REGEX_TRIE_NODE_NOT_FOUND;
        }
    }
    // Check sentinel.
    internal_regex_trie_itr end_itr = internal_regex_trie_get(&p_current_trie->children, (uint8_t)'\0');
    // CM_TIMER_STOP();

    if (internal_regex_trie_is_end(end_itr) && end_itr.data->val == NULL) {
        return CM_RES_REGEX_TRIE_NODE_NOT_FOUND;
    }
    return CM_RES_REGEX_TRIE_NODE_FOUND;
}

// Recursive destroy: Cleans children and internals.
CM_RES regex_trie_destroy(regex_trie* p_trie) {
    CM_ASSERT(p_trie);
    // Iterate over all children and recurse (skip NULL values, e.g., sentinels).
    bool failed = false;
    for (internal_regex_trie_itr itr = internal_regex_trie_first(&p_trie->children); !internal_regex_trie_is_end(itr); itr = internal_regex_trie_next(itr)) {
        void* val = itr.data->val;
        if (val != NULL) { // Recurse only on actual child tries (sentinels are NULL).
            if (CM_RES_SUCCESS != regex_trie_destroy((regex_trie*)val)) { // free children.
                failed = true;
            }   
        }
    }
    internal_regex_trie_cleanup(&p_trie->children);
    free(p_trie);
    if (failed) {
        return CM_RES_REGEX_TRIE_DESTROY_FAILED;
    }
    return CM_RES_SUCCESS;
}

// Print all words in the trie, one per line, in lexicographic order.
// (depth param ignored for this flat output.)
CM_RES regex_trie_print(const regex_trie* p_trie, int depth) {
    (void)depth; // Unused.
    CM_ASSERT(p_trie);
    enum { MAX_WORD_LEN = 1024 };
    uint8_t word_buffer[MAX_WORD_LEN];
    size_t current_len = 0;
    print_words_recursive(p_trie, word_buffer, &current_len, MAX_WORD_LEN);
}

CM_RES regex_trie_get_longest_prefix(const regex_trie* p_trie, const uint8_t* p_input, size_t input_len, size_t* p_matched_len, void** pp_value) {
    CM_ASSERT(p_trie && input_len != 0);
    const regex_trie* current = p_trie;
    size_t matched = 0;
    void* last_value = NULL;
    for (size_t i = 0; i < input_len; ++i) {
        uint8_t c = p_input[i];
        internal_regex_trie_itr itr = internal_regex_trie_get(&((regex_trie*)current)->children, c);
        if (internal_regex_trie_is_end(itr)) break; // Mismatch.
        current = (const regex_trie*)itr.data->val;
        if (!current) break;
        // Check sentinel (EOW).
        internal_regex_trie_itr end_itr = internal_regex_trie_get(&((regex_trie*)current)->children, (uint8_t)'\0');
        if (!internal_regex_trie_is_end(end_itr) && end_itr.data->val == NULL) {
            last_value = NULL; // Or store if valued.
        }
        ++matched;
    }
    *p_matched_len = matched;
    *pp_value = last_value;
    return CM_RES_SUCCESS;
}