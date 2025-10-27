#ifndef AST_REGEX_TRIE_H
#define AST_REGEX_TRIE_H
#include "code_monitoring.h"
#ifdef __cplusplus
#include <cstdint>
using std::uint8_t;
#else
#include <stdint.h>
#endif

// Opaque type for the trie node.
typedef struct regex_trie_1 regex_trie_1;

#ifdef __cplusplus
extern "C" {
#endif

// Create/initialize an empty root trie node.
CM_RES regex_trie_1_create(regex_trie_1** pp_output_trie);
// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
CM_RES regex_trie_1_insert(regex_trie_1* p_trie, const uint8_t* p_string);
// Search for a word; returns true if found (end-of-word marked).
bool regex_trie_1_get(regex_trie_1* p_trie, const uint8_t* p_string);
// Recursive destroy: Cleans children and internals.
void regex_trie_1_destroy(regex_trie_1* p_trie);
// Print all words in the trie, one per line, in lexicographic order.
void regex_trie_1_print(const regex_trie_1* p_trie, int depth);
CM_RES regex_trie_1_longest_prefix(const regex_trie_1* p_trie, const uint8_t* p_input, size_t input_len, size_t* p_matched_len, void** pp_value);

#ifdef __cplusplus
}
#endif
#endif // AST_REGEX_TRIE_H