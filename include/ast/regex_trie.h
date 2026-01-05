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

typedef struct regex_trie_value {
	uint8_t* p_regex_key;
} regex_trie_value;

/**
 * p_regex_key is a string which is copied and read only inside the function
 * value_size_bytes is the size in bytes you want to allocate for the new value
 * value_size_bytes has to be at least sizeof(regex_trie_value)
 */
regex_trie_value* regex_trie_value_create(const char* p_regex_key, size_t regex_len, size_t value_size);

// Your custom functions (no conflict now!).
CM_RES regex_trie_create(regex_trie** pp_output_trie);
CM_RES regex_trie_destroy(regex_trie* p_trie);
/**
 * p_base_value.p_regex_key must be a valid regex string which is the key to access this same value
 */
CM_RES regex_trie_insert(regex_trie* p_trie, regex_trie_value* p_base_value);  // String-based.
/**
 * remove value by its regex
 */
CM_RES regex_trie_remove(regex_trie* p_trie, const char* p_regex_key, regex_trie_value** pp_output_value);
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_string, size_t input_len, size_t* p_output_matched_total, regex_trie_value** pp_output_value);
CM_RES regex_trie_print(const regex_trie* p_trie);
// Add: CM_RES regex_trie_longest_char_prefix(...) if needed for byte buffer.

#ifdef __cplusplus
}
#endif

#endif