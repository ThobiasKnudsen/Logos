#include "ast/regex_trie.h"
#include "ast/regex_literal_splitting.hpp"
#include <stddef.h> // For size_t, nullptr.
#include <stdlib.h> // For malloc, free.
#include <stdbool.h> // For bool.
#include <stdio.h> // For printf, fprintf.
#include <string.h> // For qsort? No, but for completeness.

// Define Verstable table type (prefixed to avoid conflict).
#define NAME internal_regex_trie
#define KEY_TY uint8_t
#define VAL_TY void* // For child pointers. allways regex_trie
#define HASH_FN vt_hash_integer
#define CMPR_FN vt_cmpr_integer
#include <verstable.h> // Or your _deps path.

struct regex_trie {
    internal_regex_trie trie_children;      // Verstable map.
    void*               p_leaf_value;         // nullptr if not leaf
    reflex::Matcher     matcher;            // RE/flex field (e.g., for in-memory regex scanning).
    std::vector<std::pair<regex_trie*, std::string>>
                        regexes;            // the strings are combined with '|' between for the matcher
    bool                is_leaf             // true if lead false if internal node
    bool                matcher_updated;    // if false the matcher has to be lazily recreated when used later
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
static void print_words_recursive(const regex_trie* p_trie, uint8_t* word_buffer, size_t* current_len, size_t buffer_size) {
    if (!p_trie) return;
    // Step 1: Check for EOW sentinel at this node.
    internal_regex_trie_itr end_itr = internal_regex_trie_get(&p_trie->trie_children, (uint8_t)'\0');
    if (!internal_regex_trie_is_end(end_itr) && end_itr.data->val == nullptr) {
        // Null-terminate and print the current word.
        if (*current_len < buffer_size - 1) {
            word_buffer[*current_len] = '\0';
            CM_LOG_NOTICE("%s\n", (const char*)word_buffer);
        } else {
            // Rare: Word too long; truncate and print.
            word_buffer[buffer_size - 1] = '\0';
            CM_LOG_NOTICE("%s... (truncated)\n", (const char*)word_buffer);
        }
    }
    // Step 2: Collect non-sentinel child keys for sorted iteration.
    enum { MAX_CHILDREN_PRINT = 1024 };
    uint8_t keys[MAX_CHILDREN_PRINT];
    size_t num_keys = 0;
    for (internal_regex_trie_itr itr = internal_regex_trie_first(&((regex_trie*)p_trie)->trie_children); !internal_regex_trie_is_end(itr); itr = internal_regex_trie_next(itr)) {
        uint8_t key = itr.data->key;
        void* val = itr.data->val;
        if (key != (uint8_t)'\0' && val != nullptr && num_keys < MAX_CHILDREN_PRINT) { // Non-sentinel child.
            keys[num_keys++] = key;
        } else if (num_keys >= MAX_CHILDREN_PRINT) {
            CM_LOG_WARNING("Warning: Node has >%d trie_children; truncating print.\n", MAX_CHILDREN_PRINT);
            break;
        }
    }
    // Sort keys lexicographically.
    qsort(keys, num_keys, sizeof(uint8_t), cmp_uint8);
    // Step 3: Recurse on sorted trie_children.
    for (size_t i = 0; i < num_keys; ++i) {
        uint8_t key = keys[i];
        if (*current_len >= buffer_size - 1) {
            CM_LOG_WARNING("Warning: Word buffer overflow; skipping branch.\n");
            continue;
        }
        // Append key to buffer.
        word_buffer[(*current_len)++] = key;
        // Recurse.
        internal_regex_trie_itr child_itr = internal_regex_trie_get(&((regex_trie*)p_trie)->trie_children, key);
        if (!internal_regex_trie_is_end(child_itr)) {
            print_words_recursive((const regex_trie*)child_itr.data->val, word_buffer, current_len, buffer_size);
        }
        // Pop: Backtrack.
        (*current_len)--;
    }
}

static CM_RES regex_trie_update_matcher(regex_trie* p_trie) {
    CM_ASSERT(p_trie);
    CM_ASSERT(!p_trie->matcher_updated);

    p_trie->matcher_updated = true;
    return CM_RES_SUCCESS;
}
static CM_RES regex_trie_update_matcher(regex_trie* p_trie) {
    CM_ASSERT(p_trie);
    CM_ASSERT(!p_trie->matcher_updated);

    if (p_trie->regexes.empty()) {
        // No alts: impossible match (e.g., reject immediately in use).
        p_trie->matcher = reflex::Matcher("^(?!x)x");  // Zero-width negative lookahead.
    } else {
        std::string combined;
        for (size_t i = 0; i < p_trie->regexes.size(); ++i) {
            if (i > 0) combined += "|";
            combined += p_trie->regexes[i].second;
        }
        p_trie->matcher = reflex::Matcher(combined.c_str());
    }
    p_trie->matcher_updated = true;
    return CM_RES_SUCCESS;
}

extern "C" {

// Create/initialize an empty root trie node.
CM_RES regex_trie_create(regex_trie** pp_output_trie) {
    CM_ASSERT(pp_output_trie);
    CM_ASSERT(*pp_output_trie == nullptr);
    *pp_output_trie = (regex_trie*)malloc(sizeof(regex_trie));
    CM_ASSERT(*pp_output_trie);
    internal_regex_trie_init(&(*pp_output_trie)->trie_children);
    (*pp_output_trie)->p_value = false;
    (*pp_output_trie)->p_leaf_value = nullptr;
    (*pp_output_trie)->regexes = {}
    (*pp_output_trie)->matcher = new refex::Matcher(""); // new with empty regex
    (*pp_output_trie)->matcher_updated = matcher_updated = true;
    return CM_RES_SUCCESS;
}

// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
CM_RES regex_trie_insert(regex_trie* p_trie, const uint8_t* p_regex, void* p_value) {
    CM_ASSERT(p_trie && p_regex);
    // CM_TIMER_START();
    std::string str(static_cast<const char*>(p_regex));
    std::vector<std::vector<Segment>> paths = regex_literal_splitting(str);
    for (std::vector<Segment> path : paths) {
        regex_trie* p_current_trie = p_trie; // start from the start for each path
        for (uint32_t seg = 0; seg < path.size(); seg++) {
            if (path[seg].is_lit) {
                for (size_t i = 0; i < path[seg].str.size(); ++i) {
                    uint8_t c = static_cast<uint8_t>(path[seg].str[i]);
                    internal_regex_trie_itr itr = internal_regex_trie_get(&p_current_trie->trie_children, c);
                    if (internal_regex_trie_is_end(itr)) {
                        // Create new child node.
                        regex_trie* p_new_trie = nullptr;
                        CM_RES create_res = regex_trie_create(&p_new_trie);
                        if (create_res != CM_RES_SUCCESS) {
                            // Updated: destroy frees self.
                            regex_trie_destroy(p_new_trie);
                            // CM_TIMER_STOP();
                            return create_res;
                        }
                        // Insert the new child.
                        itr = internal_regex_trie_insert(&p_current_trie->trie_children, c, (void*)p_new_trie);
                        if (internal_regex_trie_is_end(itr)) {
                            // Updated: destroy frees self; no extra free.
                            regex_trie_destroy(p_new_trie);
                            // CM_TIMER_STOP();
                            return CM_RES_ALLOCATION_FAILURE;
                        }
                    }
                    // Advance: Use itr.data->val for both new/existing cases.
                    p_current_trie = (regex_trie*)itr.data->val;
                    if (!p_current_trie) {
                        // CM_TIMER_STOP();
                        return CM_RES_NULL_ARGUMENT; // Corrupt: nullptr child pointer.
                    }
                }
            }
            // if non literal
            else {
                regex_trie* p_new_trie = nullptr;
                CM_RES create_res = regex_trie_create(&p_new_trie);
                if (create_res != CM_RES_SUCCESS) {
                    // Updated: destroy frees self.
                    regex_trie_destroy(p_new_trie);
                    // CM_TIMER_STOP();
                    return create_res;
                }
                bool found_existing = false;
                for (const auto& pr : p_current_trie->regexes) {
                    if (pr.second == path[seg].str) {
                        p_current_trie = pr.first;
                        found_existing = true;
                        break;
                    }
                }
                if (!found_existing) {
                    // sets to false because we will update the regexes under
                    p_new_trie->matcher_updated = false;
                    std::string str_copy = path[seg].str;
                    p_current_trie->regexes.push_back({p_new_trie, str_copy});
                    p_current_trie = p_new_trie;
                }
            }
        }
        // Always mark/overwrite end of word sentinel (idempotent).
        internal_regex_trie_itr end_itr = internal_regex_trie_insert(&p_current_trie->trie_children, (uint8_t)'\0', nullptr);
        if (internal_regex_trie_is_end(end_itr)) {
            // CM_TIMER_STOP();
            return CM_RES_ALLOCATION_FAILURE;
        }
        p_current_trie->is_leaf = true;
        p_current_trie->p_leaf_value = p_value;
    }
    // CM_TIMER_STOP();
    return CM_RES_SUCCESS;
}

// Search for a word; returns true if found (end-of-word marked).
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_regex, uint32_t* p_output_match_start, uint32_t* p_output_match_end, void** pp_output_value) {
    CM_ASSERT(p_trie && p_regex);
    // CM_TIMER_START();
    std::string str(static_cast<const char*>(p_regex));
    std::vector<std::vector<Segment>> paths = regex_literal_splitting(str);
    for (const auto& path : paths) { 
        regex_trie* p_current_trie = p_trie;
        for (uint32_t seg = 0; seg < path.size(); seg++) {
            if (path.is_lit)
                for (size_t i = 0; i < path.str.size(); ++i) {
                    uint8_t c = path.str[i];
                    internal_regex_trie_itr itr = internal_regex_trie_get(&p_current_trie->trie_children, c);
                    if (internal_regex_trie_is_end(itr)) {
                        // CM_TIMER_STOP();
                        return CM_RES_REGEX_TRIE_NODE_NOT_FOUND; // Missing child.
                    }
                    p_current_trie = (regex_trie*)itr.data->val;
                    if (!p_current_trie) {
                        // CM_TIMER_STOP();
                        return CM_RES_REGEX_TRIE_NODE_NOT_FOUND;
                    }
                }
            }
            // if regex and not literal
            else {
                if (!p_current_trie->matcher_updated) {
                    CM_ASSERT(CM_RES_SUCCESS == regex_trie_update_matcher(p_current_trie));
                }
            }
        }
    } 
    return CM_RES_REGEX_TRIE_NODE_NOT_FOUND;

    // Check sentinel.
    internal_regex_trie_itr end_itr = internal_regex_trie_get(&p_current_trie->trie_children, (uint8_t)'\0');
    // CM_TIMER_STOP();

    if (internal_regex_trie_is_end(end_itr) || end_itr.data->val != nullptr) {
        return CM_RES_REGEX_TRIE_NODE_NOT_FOUND;
    }
    return CM_RES_REGEX_TRIE_NODE_FOUND;
}

// Recursive destroy: Cleans trie_children and internals.
CM_RES regex_trie_destroy(regex_trie* p_trie) {
    CM_ASSERT(p_trie);
    // Iterate over all trie_children and recurse (skip nullptr values, e.g., sentinels).
    bool failed = false;
    for (internal_regex_trie_itr itr = internal_regex_trie_first(&p_trie->trie_children); !internal_regex_trie_is_end(itr); itr = internal_regex_trie_next(itr)) {
        void* val = itr.data->val;
        if (val != nullptr) { // Recurse only on actual child tries (sentinels are nullptr).
            if (CM_RES_SUCCESS != regex_trie_destroy((regex_trie*)val)) { // free trie_children.
                failed = true;
            }   
        }
    }
    for (std::pair<regex_trie*, std:string> regex : p_trie->regexes) {
        if (regex.first != nullptr) {
            if (CM_RES_SUCCESS != regex_trie_destroy(regex.first)) {
                failed = true;
            }
        }
    }
    internal_regex_trie_cleanup(&p_trie->trie_children);

    free(p_trie);
    if (failed) {
        return CM_RES_REGEX_TRIE_DESTROY_FAILED;
    }
    return CM_RES_SUCCESS;
}

// Print all words in the trie, one per line, in lexicographic order.
// (depth param ignored for this flat output.)
CM_RES regex_trie_print(const regex_trie* p_trie, int depth) {
    (void)depth; // Unused.
    CM_ASSERT(p_trie);
    enum { MAX_WORD_LEN = 1024 };
    uint8_t word_buffer[MAX_WORD_LEN];
    size_t current_len = 0;
    print_words_recursive(p_trie, word_buffer, &current_len, MAX_WORD_LEN);
    return CM_RES_SUCCESS;
}

CM_RES regex_trie_get_longest_prefix(const regex_trie* p_trie, const uint8_t* p_input, size_t input_len, size_t* p_matched_len, void** pp_value) {
    CM_ASSERT(p_trie && input_len != 0 && p_input && p_matched_len != 0 && pp_value);
    const regex_trie* current = p_trie;
    size_t matched = 0;
    void* last_value = nullptr;
    for (size_t i = 0; i < input_len; ++i) {
        uint8_t c = p_input[i];
        internal_regex_trie_itr itr = internal_regex_trie_get(&((regex_trie*)current)->trie_children, c);
        if (internal_regex_trie_is_end(itr)) break; // Mismatch.
        current = (const regex_trie*)itr.data->val;
        if (!current) break;
        // Check sentinel (EOW).
        internal_regex_trie_itr end_itr = internal_regex_trie_get(&((regex_trie*)current)->trie_children, (uint8_t)'\0');
        if (internal_regex_trie_is_end(end_itr) || end_itr.data->val != nullptr) {
            last_value = nullptr; // Or store if valued.
        }
        ++matched;
    }
    *p_matched_len = matched;
    *pp_value = last_value;
    return CM_RES_SUCCESS;
}

} // extern "C"