#ifndef HTRIE_WCHAR_H
#define HTRIE_WCHAR_H
   
#include "code_monitoring.h"
#include <wchar.h> // For wchar_t
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Opaque handle to a wide-character (wchar_t) HAT-trie key-value store.
 * 
 * This structure wraps an internal HAT-trie implementation for efficient prefix-based
 * storage and lookup of null-terminated wchar_t strings mapped to void* values.
 * Keys are stored internally as UTF-8 for compatibility with the underlying library.
 * Values are user-managed and not automatically freed on destruction.
 * 
 * Supports exact-match insertion/retrieval and incremental traversal for lexing use cases.
 * Thread-safety is not guaranteed; synchronize externally if needed.
 */
typedef struct htrie_wchar htrie_wchar;

/**
 * @brief Opaque handle to a node (prefix position) in the wide-character HAT-trie.
 * 
 * Represents a specific prefix path in the trie, obtained via root or next operations.
 * Used for incremental traversal during lexing or prefix matching.
 * Nodes are lightweight but must be explicitly destroyed after use.
 * Invalidated if the owning trie is modified (e.g., via insert).
 * Not thread-safe.
 */
typedef struct htrie_wchar_node htrie_wchar_node;

/**
 * @brief Creates a new empty HAT-trie instance.
 * 
 * Allocates and initializes a new htrie_wchar structure with no entries.
 * 
 * @param pp_output_trie [out] Pointer to receive the new trie handle. Caller must destroy it when done.
 * @return CM_RES_SUCCESS on success, or an error code (e.g., CM_RES_HTRIE_INTERNAL_ERROR) on failure.
 */
CM_RES htrie_wchar_create(htrie_wchar** pp_output_trie);

/**
 * @brief Destroys the HAT-trie and frees all associated resources.
 * 
 * Releases the internal trie structure and any allocated memory. Does not free user-provided values.
 * All node handles from this trie become invalid after destruction.
 * 
 * @param trie [in] The trie handle to destroy. Pass NULL to no-op (but avoid).
 * @return CM_RES_SUCCESS on success, or an error code on failure.
 */
CM_RES htrie_wchar_destroy(htrie_wchar* p_trie);

/**
 * @brief Inserts or updates a key-value pair in the trie.
 * 
 * If the key already exists, overwrites the existing value. Supports Unicode via wchar_t (internally converted to UTF-8).
 * 
 * @param p_trie [in,out] The trie to insert into.
 * @param key [in] Pointer to wchar_t key characters. Must not be NULL.
 * @param key_len [in] Length of the key in wchar_t characters (excluding any null terminator).
 * @param value [in] User-managed void* value to associate with the key. Not freed by the trie.
 * @return CM_RES_SUCCESS on success, or an error code (e.g., CM_RES_HTRIE_INVALID_KEY, CM_RES_ALLOCATION_FAILURE).
 */
CM_RES htrie_wchar_insert(htrie_wchar* p_trie, const wchar_t* key, uint64_t key_len, void* value);

/**
 * @brief Retrieves the value associated with a key, if it exists.
 * 
 * Performs an exact-match lookup. On success:
 * - If found, returns CM_RES_HTRIE_NODE_FOUND and sets *pp_output_value to the value (user must manage/free if needed).
 * - If not found, returns CM_RES_HTRIE_NODE_NOT_FOUND and leaves *pp_output_value untouched.
 * 
 * @param p_trie [in] The trie to search.
 * @param key [in] Pointer to wchar_t key characters. Must not be NULL.
 * @param key_len [in] Length of the key in wchar_t characters (excluding any null terminator).
 * @param pp_output_value [out] Pointer to receive the value on found. Untouched on not found.
 * @return CM_RES_HTRIE_NODE_FOUND if key exists, CM_RES_HTRIE_NODE_NOT_FOUND if not, or an error code (e.g., CM_RES_ALLOCATION_FAILURE).
 */
CM_RES htrie_wchar_get(htrie_wchar* p_trie, const wchar_t* key, uint64_t key_len, void** pp_output_value);

/**
 * @brief Gets the total number of key-value entries in the trie.
 * 
 * @param trie [in] The trie to query.
 * @param p_output_size [out] Pointer to receive the entry count. Must not be NULL.
 * @return CM_RES_SUCCESS on success, or an error code.
 */
CM_RES htrie_wchar_size(const htrie_wchar* p_trie, uint64_t* p_output_size);

/**
 * @brief Obtains a node handle for the root of the trie (empty prefix).
 * 
 * Always succeeds for a valid trie, even if empty. The root node can be used as the starting point for traversal.
 * 
 * @param trie [in] The trie to get the root from.
 * @param pp_output_root [out] Pointer to receive the root node handle. Caller must destroy it when done.
 * @return CM_RES_SUCCESS on success, or an error code (e.g., CM_RES_ALLOCATION_FAILURE).
 */
CM_RES htrie_wchar_node_root(const htrie_wchar* p_trie, htrie_wchar_node** pp_output_root);

/**
 * @brief Advances from the current node to the child node for the given character, if it exists.
 * 
 * Performs a single-step prefix traversal. Handles Unicode characters via wchar_t (multi-byte UTF-8 internally).
 * 
 * @param current_node [in] The starting node handle. Must not be NULL.
 * @param ch [in] The wchar_t character to match for the next edge.
 * @param pp_output_node [out] Pointer to receive the next node handle if found. NULL if not found. Caller must destroy if non-NULL.
 * @param p_found [out] Set to 1 if the child exists (and *pp_output_node is valid), 0 otherwise. Must not be NULL.
 * @return CM_RES_SUCCESS on success, or an error code (e.g., CM_RES_ALLOCATION_FAILURE).
 */
CM_RES htrie_wchar_node_next(const htrie_wchar_node* p_current_node, wchar_t ch, htrie_wchar_node** pp_output_node, int* p_found);

/**
 * @brief Frees a node handle and its resources.
 * 
 * Must be called on every node obtained from root or next, even if traversal failed.
 * Safe to call on NULL.
 * 
 * @param p_node [in] The node handle to destroy.
 * @return CM_RES_SUCCESS on success, or an error code.
 */
CM_RES htrie_wchar_node_destroy(htrie_wchar_node* p_node);

/**
 * @brief Finds the longest prefix of the input that matches an exact key in the trie.
 * 
 * Traverses the input character-by-character, tracking the deepest prefix that ends at a valid key.
 * Useful for lexers to detect complete tokens (e.g., keywords) with maximal length.
 * If no matching prefix exists, sets *p_matched_length to 0 and *pp_value to NULL.
 * Input need not be null-terminated; length is provided explicitly.
 * 
 * @param trie [in] The trie to search.
 * @param input [in] Pointer to the input wchar_t buffer.
 * @param input_len [in] Number of wchar_t characters in the input to consider.
 * @param p_matched_length [out] Pointer to receive the length (in wchar_t chars) of the longest matching prefix that is a full key. 0 if none.
 * @param pp_value [out] Pointer to receive the value of the matching key, or NULL if none. User-managed.
 * @return CM_RES_SUCCESS if traversal completed (even if no match), or an error code (e.g., CM_RES_ALLOCATION_FAILURE).
 */
CM_RES htrie_wchar_longest_prefix(const htrie_wchar* p_trie, const wchar_t* p_input, uint64_t input_len, uint64_t* p_matched_length, void** pp_value);

CM_RES htrie_char_longest_prefix(const htrie_wchar* p_trie, const char* p_input, uint64_t max_input_len, uint64_t* p_matched_length, void** pp_value);

#ifdef __cplusplus
}
#endif
#endif // HTRIE_WCHAR_H