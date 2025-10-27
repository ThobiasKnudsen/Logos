// ast/regex_trie_2.h
#ifndef REGEX_TRIE_2_H
#define REGEX_TRIE_2_H

#include <stdbool.h>
#include <stddef.h> // For size_t.
#include "code_monitoring.h" // For CM_RES, CM_ASSERT, etc.

// Forward declaration.
typedef struct regex_trie_2 regex_trie_2;

// Create/initialize an empty root trie node.
CM_RES regex_trie_2_create(regex_trie_2** pp_output_trie);

// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
// Assumes p_string is null-terminated uint8_t (UTF-8/ASCII bytes).
CM_RES regex_trie_2_insert(regex_trie_2* p_trie, const uint8_t* p_string);

// Search for a word; returns true if found (end-of-word marked).
bool regex_trie_2_get(regex_trie_2* p_trie, const uint8_t* p_string);

// Recursive destroy: Cleans children and internals.
void regex_trie_2_destroy(regex_trie_2* p_trie);

// Print all words in the trie, one per line, in lexicographic order.
// (depth param ignored for this flat output.)
void regex_trie_2_print(const regex_trie_2* p_trie, int depth);

// Find the longest prefix match in the input buffer.
// Advances up to input_len bytes (or less if mismatch/EOS).
// Outputs matched byte length and associated value (nullptr for EOW sentinel).
// Assumes ASCII/UTF-8 single-byte for simplicity (multi-byte handling omitted).
CM_RES regex_trie_2_longest_prefix(const regex_trie_2* p_trie, const uint8_t* p_input, size_t input_len,
                                   size_t* p_matched_len, void** pp_value);

#endif // REGEX_TRIE_2_H