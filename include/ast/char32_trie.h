#ifndef AST_WCHAR_TRIE_H
#define AST_WCHAR_TRIE_H
#include "code_monitoring.h"
#include <uchar.h>

// Hash function: Cast to uint64_t for 64-bit output (per Verstable docs).
static inline uint64_t wchar_hash(const char32_t key) {
    return (uint64_t)key;
}

// Equality function for Verstable: bool true if equal.
static inline bool wchar_eq(const char32_t lhs, const char32_t rhs) {
    return lhs == rhs;
}

#define NAME char32_trie
#define KEY_TY char32_t
#define VAL_TY void*
#define HASH_FN wchar_hash
#define CMPR_FN wchar_eq
#include "verstable.h"

// Create/initialize an empty root trie node.
CM_RES trie_create(char32_trie** pp_output_trie);
// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
CM_RES trie_insert(char32_trie* p_trie, const char32_t* p_string);
// Search for a word; returns true if found (end-of-word marked).
bool trie_get(char32_trie* p_trie, const char32_t* p_string);
// Recursive destroy: Cleans children and internals.
void trie_destroy(char32_trie* p_trie);
// Print all words in the trie, one per line, in lexicographic order.
void trie_print(const char32_trie* p_trie, int depth);

CM_RES trie_longest_prefix(const char32_trie* p_trie, const char32_t* p_input, size_t input_len, size_t* p_matched_len, void** pp_value);

CM_RES trie_longest_char_prefix(const char32_trie* p_trie, const char* p_input, uint64_t max_input_len, uint64_t* p_matched_length, void** pp_value);
#endif // AST_WCHAR_TRIE_H