// In ast/regex_trie.h
#ifndef REGEX_TRIE_H
#define REGEX_TRIE_H

#include <stdint.h>  // For uint8_t (ensures it's defined before Verstable).
#include <stdbool.h>
#include "code_monitoring.h"  // Your CM_RES, etc.

// Now define your trie struct using the prefixed map.
typedef struct regex_trie regex_trie;

#ifdef __cplusplus
extern "C" {
#endif

// Your custom functions (no conflict now!).
CM_RES regex_trie_create(regex_trie** pp_output_trie);
CM_RES regex_trie_destroy(regex_trie* p_trie);
CM_RES regex_trie_insert(regex_trie* p_trie, const uint8_t* p_regex, void* p_value);  // String-based.
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_string, size_t input_len, size_t* p_output_matched_total, void** pp_output_value);
CM_RES regex_trie_print(const regex_trie* p_trie, int depth);
// Add: CM_RES regex_trie_longest_char_prefix(...) if needed for byte buffer.

#ifdef __cplusplus
}
#endif

#endif