// In ast/regex_trie.h
#ifndef REGEX_TRIE_H
#define REGEX_TRIE_H

#include <stdint.h>  // For uint8_t (ensures it's defined before Verstable).
#include <stdbool.h>
#include "code_monitoring.h"  // Your CM_RES, etc.

// Define Verstable table type (prefixed to avoid conflict).
#define NAME internal_regex_trie
#define KEY_TY uint8_t
#define VAL_TY void*  // For child pointers.
#include <verstable.h>  // Or your _deps path.

// Now define your trie struct using the prefixed map.
typedef struct regex_trie {
    internal_regex_trie children;  // Verstable map for {uint8_t key -> void* child}.
} regex_trie;

// Your custom functions (no conflict now!).
CM_RES regex_trie_create(regex_trie** pp_output_trie);
CM_RES regex_trie_destroy(regex_trie* p_trie);
CM_RES regex_trie_insert(regex_trie* p_trie, const uint8_t* p_string);  // String-based.
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_string);
CM_RES regex_trie_get_longest_prefix(const regex_trie* p_trie, const uint8_t* p_input, size_t input_len,
                                 size_t* p_matched_len, void** pp_value);
CM_RES regex_trie_print(const regex_trie* p_trie, int depth);
// Add: CM_RES regex_trie_longest_char_prefix(...) if needed for byte buffer.

#endif