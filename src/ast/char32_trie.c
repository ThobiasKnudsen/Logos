#include "ast/char32_trie.h" 
#include <stddef.h>  // For size_t, NULL.
#include <stdlib.h>  // For malloc, free.
#include <stdbool.h>  // For bool.

// Trichotomy comparator for qsort (sorting keys for print).
static int cmp_char32(const void* lhs, const void* rhs) {
    char32_t l = *(const char32_t*)lhs;
    char32_t r = *(const char32_t*)rhs;
    return (l > r) - (l < r);
}
static size_t char32len(const char32_t *s) {
    size_t len = 0;
    if (!s) return 0;  // Handle null pointer
    while (s[len] != 0) {
        ++len;
    }
    return len;
}
// Recursive helper to print words: Builds prefix in buffer, prints when EOW hit.
static void print_words_recursive(const char32_trie* p_trie, char32_t* word_buffer, size_t* current_len, size_t buffer_size) {
    if (!p_trie) return;

    // Step 1: Check for EOW sentinel at this node.
    char32_trie_itr end_itr = vt_get(p_trie, L'\0');
    if (!vt_is_end(end_itr) && end_itr.data->val == NULL) {
        // Null-terminate and print the current word.
        if (*current_len < buffer_size - 1) {
            word_buffer[*current_len] = L'\0';
            printf("%ls\n", word_buffer);
        } else {
            // Rare: Word too long; truncate and print.
            word_buffer[buffer_size - 1] = L'\0';
            printf("%ls... (truncated)\n", word_buffer);
        }
    }

    // Step 2: Collect non-sentinel child keys for sorted iteration.
    enum { MAX_CHILDREN_PRINT = 1024 };
    char32_t keys[MAX_CHILDREN_PRINT];
    size_t num_keys = 0;
    for (char32_trie_itr itr = vt_first((char32_trie*)p_trie); !vt_is_end(itr); itr = vt_next(itr)) {
        char32_t key = itr.data->key;
        void* val = itr.data->val;
        if (key != L'\0' && val != NULL && num_keys < MAX_CHILDREN_PRINT) {  // Non-sentinel child.
            keys[num_keys++] = key;
        } else if (num_keys >= MAX_CHILDREN_PRINT) {
            fprintf(stderr, "Warning: Node has >%d children; truncating print.\n", MAX_CHILDREN_PRINT);
            break;
        }
    }
    // Sort keys lexicographically.
    qsort(keys, num_keys, sizeof(char32_t), cmp_char32);

    // Step 3: Recurse on sorted children.
    for (size_t i = 0; i < num_keys; ++i) {
        char32_t key = keys[i];
        if (*current_len >= buffer_size - 1) {
            fprintf(stderr, "Warning: Word buffer overflow; skipping branch.\n");
            continue;
        }
        // Append key to buffer.
        word_buffer[(*current_len)++] = key;
        // Recurse.
        char32_trie_itr child_itr = vt_get((char32_trie*)p_trie, key);
        if (!vt_is_end(child_itr)) {
            print_words_recursive((const char32_trie*)child_itr.data->val, word_buffer, current_len, buffer_size);
        }
        // Pop: Backtrack.
        (*current_len)--;
    }
}

// Create/initialize an empty root trie node.
CM_RES trie_create(char32_trie** pp_output_trie) {
    CM_ASSERT(pp_output_trie);
    CM_ASSERT(*pp_output_trie == NULL);
    *pp_output_trie = malloc(sizeof(char32_trie));
    CM_ASSERT(*pp_output_trie);
    vt_init(*pp_output_trie);
    return CM_RES_SUCCESS;
}

// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
CM_RES trie_insert(char32_trie* p_trie, const char32_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    CM_TIMER_START();
    size_t string_length = char32len(p_string);
    char32_trie* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        char32_t c = p_string[i];
        char32_trie_itr itr = vt_get(p_current_trie, c);
        if (vt_is_end(itr)) {
            // Create new child node.
            char32_trie* p_new_trie = NULL;
            CM_RES create_res = trie_create(&p_new_trie);
            if (create_res != CM_RES_SUCCESS) {
                // Updated: destroy frees self.
                trie_destroy(p_new_trie);
                CM_TIMER_STOP();
                return create_res;
            }
            // Insert the new child.
            itr = vt_insert(p_current_trie, c, (void*)p_new_trie);
            if (vt_is_end(itr)) {
                // Updated: destroy frees self; no extra free.
                trie_destroy(p_new_trie);
                CM_TIMER_STOP();
                return CM_RES_ALLOCATION_FAILURE;
            }
        }
        // Advance: Use itr.data->val for both new/existing cases.
        p_current_trie = (char32_trie*)itr.data->val;
        if (!p_current_trie) {
            CM_TIMER_STOP();
            return CM_RES_NULL_ARGUMENT;  // Corrupt: NULL child pointer.
        }
    }
    // Always mark/overwrite end of word sentinel (idempotent).
    char32_trie_itr end_itr = vt_insert(p_current_trie, L'\0', NULL);
    if (vt_is_end(end_itr)) {
        CM_TIMER_STOP();
        return CM_RES_ALLOCATION_FAILURE;
    }
    CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}

// Search for a word; returns true if found (end-of-word marked).
bool trie_get(char32_trie* p_trie, const char32_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    CM_TIMER_START();
    size_t string_length = char32len(p_string);
    char32_trie* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        char32_t c = p_string[i];
        char32_trie_itr itr = vt_get(p_current_trie, c);
        if (vt_is_end(itr)) {
            CM_TIMER_STOP();
            return false;  // Missing child.
        }
        p_current_trie = (char32_trie*)itr.data->val;
        if (!p_current_trie) {
            CM_TIMER_STOP();
            return false;
        }
    }
    // Check sentinel.
    char32_trie_itr end_itr = vt_get(p_current_trie, L'\0');
    CM_TIMER_STOP();
    return !vt_is_end(end_itr) && end_itr.data->val == NULL;
}

// Recursive destroy: Cleans children and internals.
void trie_destroy(char32_trie* p_trie) {
    if (!p_trie) return;
    // Iterate over all children and recurse (skip NULL values, e.g., sentinels).
    for (char32_trie_itr itr = vt_first(p_trie); !vt_is_end(itr); itr = vt_next(itr)) {
        void* val = itr.data->val;
        if (val != NULL) {  // Recurse only on actual child tries (sentinels are NULL).
            trie_destroy((char32_trie*)val);  // Always free children.
        }
    }
    vt_cleanup(p_trie);
    free(p_trie);
}
// Print all words in the trie, one per line, in lexicographic order.
// (depth param ignored for this flat output.)
void trie_print(const char32_trie* p_trie, int depth) {
    (void)depth;  // Unused.
    if (!p_trie) {
        printf("(null trie)\n");
        return;
    }
    enum { MAX_WORD_LEN = 1024 };
    char32_t word_buffer[MAX_WORD_LEN];
    size_t current_len = 0;
    print_words_recursive(p_trie, word_buffer, &current_len, MAX_WORD_LEN);
}

CM_RES trie_longest_prefix(const char32_trie* p_trie, const char32_t* p_input, size_t input_len, size_t* p_matched_len, void** pp_value) {
    if (!p_trie || input_len == 0) { *p_matched_len = 0; *pp_value = NULL; return CM_RES_SUCCESS; }
    const char32_trie* current = p_trie;
    size_t matched = 0;
    void* last_value = NULL;
    for (size_t i = 0; i < input_len; ++i) {
        char32_t c = p_input[i];
        char32_trie_itr itr = vt_get((char32_trie*)current, c);
        if (vt_is_end(itr)) break;  // Mismatch.
        current = (const char32_trie*)itr.data->val;
        if (!current) break;
        // Check sentinel (EOW).
        char32_trie_itr end_itr = vt_get((char32_trie*)current, L'\0');
        if (!vt_is_end(end_itr) && end_itr.data->val == NULL) {
            last_value = NULL;  // Or store if valued.
        }
        ++matched;
    }
    *p_matched_len = matched;
    *pp_value = last_value;
    return CM_RES_SUCCESS;
}
CM_RES trie_longest_char_prefix(const char32_trie* p_trie, const char* p_input, uint64_t max_input_len, uint64_t* p_matched_length, void** pp_value) {
    CM_ASSERT(p_trie && p_input && p_matched_length && pp_value);
    *p_matched_length = 0;
    *pp_value = NULL;
    CM_TIMER_START();
    if (max_input_len == 0) {
        // Check empty key.
        char32_trie_itr end_itr = vt_get((char32_trie*)p_trie, 0); // L'\0' == 0U
        if (!vt_is_end(end_itr) && end_itr.data->val == NULL) {
            *pp_value = NULL;
        }
        CM_TIMER_STOP();
        return CM_RES_SUCCESS;
    }
    const char32_trie* current = p_trie;
    uint64_t byte_pos = 0;
    void* last_value = NULL;
    while (byte_pos < max_input_len) {
        // For ASCII/UTF-8 single-byte: next codepoint is 1 byte.
        // Future: Add UTF-8 decode (e.g., if (byte & 0x80) { multi-byte... } but skip for words.txt.
        char32_t c = (char32_t)p_input[byte_pos]; // Safe cast: ASCII 0-127 → valid codepoints.
        char32_trie_itr itr = vt_get((char32_trie*)current, c);
        if (vt_is_end(itr)) {
            break; // Mismatch.
        }
        current = (const char32_trie*)itr.data->val;
        if (!current) {
            break;
        }
        // Check EOW sentinel.
        char32_trie_itr end_itr = vt_get((char32_trie*)current, 0U);
        if (!vt_is_end(end_itr) && end_itr.data->val == NULL) {
            last_value = NULL; // Set-like; store value if needed.
        }
        ++byte_pos; // Advance 1 byte (ASCII assumption).
    }
    *p_matched_length = byte_pos; // Bytes matched.
    *pp_value = last_value;
    CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}