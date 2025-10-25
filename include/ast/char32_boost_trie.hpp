#ifndef AST_WCHAR_TRIE_H
#define AST_WCHAR_TRIE_H
#include "code_monitoring.h"

template <typename T>
class BoostTrie {

private:

    boost::unordered_flat_map<T, Char32Trie> children;
    void* p_value = nullptr; // Cannot be nullptr when leafnode as that is the marker for leaf node

public:

    // Create/initialize an empty root trie node.
    Char32BoostTrie();
    // Recursive destroy: Cleans children and internals.
    ~Char32BoostTrie();
    // Insert a string into the trie, creating nodes as needed.
    // Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
    CM_RES insert(const T* p_string);
    // Search for a word; returns true if found (end-of-word marked).
    CM_RES get(const T* p_string);
    // Print all words in the trie, one per line, in lexicographic order.
    CM_RES print(int depth);
    CM_RES get_longest_prefix(const T* p_input, size_t input_len, size_t* p_matched_len, void** pp_value);
};

#endif // AST_WCHAR_TRIE_H