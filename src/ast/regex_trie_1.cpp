// ast/regex_trie_1.cpp
#include "ast/regex_trie_1.h"
#include <boost/unordered/unordered_flat_map.hpp>
#include <cstddef> // For size_t, nullptr.
#include <cstdlib> // For malloc, free, qsort.
#include <cstdio> // For printf, fprintf.
#include <cstring> // For strcmp if needed, but not.
#include <vector> // For collecting keys temporarily.
#include <algorithm> // For std::sort.
#include <cstdint> // For uint64_t, uint32_t, uint8_t.

// Type alias for the map (avoids template parsing issues in destructor call)
using MapType = boost::unordered_flat_map<uint8_t, void*>;

// Struct definition (must be before any functions that use it).
struct regex_trie_1 {
    MapType children;
};

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
static void print_words_recursive(const regex_trie_1* p_trie, uint8_t* word_buffer, size_t* current_len, size_t buffer_size) {
    if (!p_trie) return;
    // Step 1: Check for EOW sentinel at this node.
    auto end_it = p_trie->children.find(0);
    if (end_it != p_trie->children.end() && end_it->second == nullptr) {
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
    // Step 2: Collect non-sentinel child keys for sorted iteration.
    enum { MAX_CHILDREN_PRINT = 1024 };
    uint8_t keys[MAX_CHILDREN_PRINT];
    size_t num_keys = 0;
    for (const auto& p : p_trie->children) {
        uint8_t key = p.first;
        void* val = p.second;
        if (key != 0 && val != nullptr && num_keys < MAX_CHILDREN_PRINT) { // Non-sentinel child.
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
        auto child_it = p_trie->children.find(key);
        if (child_it != p_trie->children.end()) {
            print_words_recursive(static_cast<const regex_trie_1*>(child_it->second), word_buffer, current_len, buffer_size);
        }
        // Pop: Backtrack.
        (*current_len)--;
    }
}

#ifdef __cplusplus
extern "C" {
#endif

// Create/initialize an empty root trie node.
CM_RES regex_trie_1_create(regex_trie_1** pp_output_trie) {
    CM_ASSERT(pp_output_trie);
    CM_ASSERT(*pp_output_trie == nullptr);
    *pp_output_trie = static_cast<regex_trie_1*>(malloc(sizeof(regex_trie_1)));
    CM_ASSERT(*pp_output_trie);
    if (*pp_output_trie) {
        new (&(*pp_output_trie)->children) MapType();
    }
    return CM_RES_SUCCESS;
}

// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
CM_RES regex_trie_1_insert(regex_trie_1* p_trie, const uint8_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    // CM_TIMER_START();
    size_t string_length = uint8len(p_string);
    regex_trie_1* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        uint8_t c = p_string[i];
        auto it = p_current_trie->children.find(c);
        if (it == p_current_trie->children.end()) {
            // Create new child node.
            regex_trie_1* p_new_trie = nullptr;
            CM_RES create_res = regex_trie_1_create(&p_new_trie);
            if (create_res != CM_RES_SUCCESS) {
                // Updated: destroy frees self.
                regex_trie_1_destroy(p_new_trie);
                // CM_TIMER_STOP();
                return create_res;
            }
            // Insert the new child.
            auto res = p_current_trie->children.emplace(c, static_cast<void*>(p_new_trie));
            if (!res.second) {
                // Should not happen; destroy frees self.
                regex_trie_1_destroy(p_new_trie);
                // CM_TIMER_STOP();
                return CM_RES_ALLOCATION_FAILURE;
            }
            it = res.first;
        }
        // Advance: Use it->second for both new/existing cases.
        p_current_trie = static_cast<regex_trie_1*>(it->second);
        if (!p_current_trie) {
            // CM_TIMER_STOP();
            return CM_RES_NULL_ARGUMENT; // Corrupt: NULL child pointer.
        }
    }
    // Always mark/overwrite end of word sentinel (idempotent).
    p_current_trie->children[0] = nullptr;
    // CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}

// Search for a word; returns true if found (end-of-word marked).
bool regex_trie_1_get(regex_trie_1* p_trie, const uint8_t* p_string) {
    CM_ASSERT(p_trie && p_string);
    // CM_TIMER_START();
    size_t string_length = uint8len(p_string);
    regex_trie_1* p_current_trie = p_trie;
    for (size_t i = 0; i < string_length; ++i) {
        uint8_t c = p_string[i];
        auto it = p_current_trie->children.find(c);
        if (it == p_current_trie->children.end()) {
            // CM_TIMER_STOP();
            return false; // Missing child.
        }
        p_current_trie = static_cast<regex_trie_1*>(it->second);
        if (!p_current_trie) {
            // CM_TIMER_STOP();
            return false;
        }
    }
    // Check sentinel.
    auto end_it = p_current_trie->children.find(0);
    // CM_TIMER_STOP();
    return (end_it != p_current_trie->children.end() && end_it->second == nullptr);
}

// Recursive destroy: Cleans children and internals.
void regex_trie_1_destroy(regex_trie_1* p_trie) {
    if (!p_trie) return;
    // Iterate over all children and recurse (skip NULL values, e.g., sentinels).
    for (auto& p : p_trie->children) {
        void* val = p.second;
        if (val != nullptr) { // Recurse only on actual child tries (sentinels are NULL).
            regex_trie_1_destroy(static_cast<regex_trie_1*>(val)); // Always free children.
        }
    }
    p_trie->children.clear();
    p_trie->children.~MapType();  // Now simple and valid
    free(p_trie);
}

// Print all words in the trie, one per line, in lexicographic order.
// (depth param ignored for this flat output.)
void regex_trie_1_print(const regex_trie_1* p_trie, int depth) {
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

CM_RES regex_trie_1_longest_prefix(const regex_trie_1* p_trie, const uint8_t* p_input, size_t input_len, size_t* p_matched_len, void** pp_value) {
    if (!p_trie || input_len == 0) { *p_matched_len = 0; *pp_value = nullptr; return CM_RES_SUCCESS; }
    // CM_TIMER_START();
    const regex_trie_1* current = p_trie;
    size_t matched = 0;
    void* last_value = nullptr;
    for (size_t i = 0; i < input_len; ++i) {
        uint8_t c = p_input[i];
        auto it = current->children.find(c);
        if (it == current->children.end()) break; // Mismatch.
        current = static_cast<const regex_trie_1*>(it->second);
        if (!current) break;
        // Check sentinel (EOW).
        auto end_it = current->children.find(0);
        if (end_it != current->children.end() && end_it->second == nullptr) {
            last_value = nullptr; // Or store if valued.
        }
        ++matched;
    }
    *p_matched_len = matched;
    *pp_value = last_value;
    // CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}

#ifdef __cplusplus
}
#endif