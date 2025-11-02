#include "ast/regex_trie.h"
#include "ast/regex_literal_splitting.hpp"
#include <stddef.h> // For size_t, nullptr.
#include <stdlib.h> // For malloc, free.
#include <stdbool.h> // For bool.
#include <stdio.h> // For printf, fprintf.
#include <vector>
#include <string>
#include <sstream> // For ostringstream.
#define PCRE2_CODE_UNIT_WIDTH 8
#include <pcre2.h> // PCRE2 header.

// Define Verstable table type (prefixed to avoid conflict).
#define NAME internal_regex_trie
#define KEY_TY uint8_t
#define VAL_TY void* // For child pointers. Always regex_trie*.
#define HASH_FN vt_hash_integer
#define CMPR_FN vt_cmpr_integer
#include <verstable.h> // Or your _deps path.

struct regex_trie {
    // leaf nodes will be identified by '\0' character
    internal_regex_trie trie_children; // Verstable map.
    void* p_leaf_value; // nullptr if not leaf
    pcre2_code* compiled_regex; // Compiled combined PCRE2 pattern (nullptr if not updated).
    pcre2_match_data* match_data; // Reusable match data.
    std::vector<std::pair<regex_trie*, std::string>>
                        regexes; // the strings are combined with '|' between for the matcher
    bool matcher_updated; // if false the matcher has to be lazily recreated when used later
};

// Trichotomy comparator for qsort (sorting keys for print).
static int cmp_uint8(const void* lhs, const void* rhs) {
    uint8_t l = *(const uint8_t*)lhs;
    uint8_t r = *(const uint8_t*)rhs;
    return (l > r) - (l < r);
}

// Recursive helper to print words: Builds prefix in buffer, prints when EOW hit.
static void print_words_recursive(const regex_trie* p_trie, uint8_t* word_buffer, size_t* current_len, size_t buffer_size) {
    if (!p_trie) return;
    // Step 1: Check for EOW sentinel at this node.
    internal_regex_trie_itr end_itr = internal_regex_trie_get(&((regex_trie*)p_trie)->trie_children, (uint8_t)'\0');
    if (!internal_regex_trie_is_end(end_itr) && end_itr.data->val == nullptr) {
        // Null-terminate and print the current word.
        if (*current_len < buffer_size - 1) {
            word_buffer[*current_len] = '\0';
            printf("%s\n", (const char*)word_buffer);
        } else {
            // Rare: Word too long; truncate and print.
            word_buffer[buffer_size - 1] = '\0';
            printf("%s... (truncated)\n", (const char*)word_buffer);
        }
    }
    // Step 2: Collect non-sentinel child keys for sorted iteration.
    enum { MAX_CHILDREN_PRINT = 1024 };
    uint8_t keys[MAX_CHILDREN_PRINT];
    size_t num_keys = 0;
    for (internal_regex_trie_itr itr = internal_regex_trie_first(&((regex_trie*)p_trie)->trie_children);
         !internal_regex_trie_is_end(itr);
         itr = internal_regex_trie_next(itr))
    {
        uint8_t key = itr.data->key;
        void* val = itr.data->val;
        if (key != (uint8_t)'\0' && val != nullptr && num_keys < MAX_CHILDREN_PRINT) { // Non-sentinel child.
            keys[num_keys++] = key;
        } else if (num_keys >= MAX_CHILDREN_PRINT) {
            fprintf(stderr, "Warning: Node has >%d trie_children; truncating print.\n", MAX_CHILDREN_PRINT);
            break;
        }
    }
    // Sort keys lexicographically.
    qsort(keys, num_keys, sizeof(uint8_t), cmp_uint8);
    // Step 3: Recurse on sorted trie_children.
    for (size_t i = 0; i < num_keys; ++i) {
        uint8_t key = keys[i];
        if (*current_len >= buffer_size - 1) {
            fprintf(stderr, "Warning: Word buffer overflow; skipping branch.\n");
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
    // Free existing if any.
    if (p_trie->compiled_regex) {
        pcre2_code_free(p_trie->compiled_regex);
        p_trie->compiled_regex = nullptr;
    }
    if (p_trie->match_data) {
        pcre2_match_data_free(p_trie->match_data);
        p_trie->match_data = nullptr;
    }
    if (p_trie->regexes.empty()) {
        int errcode;
        PCRE2_SIZE erroffset;
        p_trie->compiled_regex = pcre2_compile(PCRE2_SPTR("^(?!)"), PCRE2_ZERO_TERMINATED, PCRE2_UTF | PCRE2_UCP, &errcode, &erroffset, NULL);
        if (!p_trie->compiled_regex) {
            // Log error; for now, return fail.
            PCRE2_UCHAR errbuf[256];
            pcre2_get_error_message(errcode, errbuf, sizeof(errbuf));
            fprintf(stderr, "PCRE2 compile fail: %s\n", (char*)errbuf);
            return CM_RES_ALLOCATION_FAILURE; // Or custom RES.
        }
    } else {
        std::ostringstream oss;
        oss << "^";
        for (size_t i = 0; i < p_trie->regexes.size(); ++i) {
            if (i > 0) oss << "|";
            oss << "(" << p_trie->regexes[i].second << ")";
        }
        std::string pattern_str = oss.str();
        int errcode;
        PCRE2_SIZE erroffset;
        p_trie->compiled_regex = pcre2_compile(reinterpret_cast<PCRE2_SPTR>(pattern_str.c_str()), pattern_str.length(), PCRE2_UTF | PCRE2_UCP, &errcode, &erroffset, NULL);
        if (!p_trie->compiled_regex) {
            PCRE2_UCHAR errbuf[256];
            pcre2_get_error_message(errcode, errbuf, sizeof(errbuf));
            fprintf(stderr, "PCRE2 compile fail: %s at offset %zu\n", (char*)errbuf, erroffset);
            return CM_RES_ALLOCATION_FAILURE;
        }
        // Optional: JIT for speed.
        int jit_rc = pcre2_jit_compile(p_trie->compiled_regex, 0);
        if (jit_rc < 0) {
            fprintf(stderr, "PCRE2 JIT compile fail: %d\n", jit_rc);
        }
    }
    p_trie->match_data = pcre2_match_data_create_from_pattern(p_trie->compiled_regex, NULL);
    if (!p_trie->match_data) {
        pcre2_code_free(p_trie->compiled_regex);
        p_trie->compiled_regex = nullptr;
        return CM_RES_ALLOCATION_FAILURE;
    }
    p_trie->matcher_updated = true;
    return CM_RES_SUCCESS;
}

extern "C" {
// Create/initialize an empty root trie node.
CM_RES regex_trie_create(regex_trie** pp_output_trie) {
    CM_ASSERT(pp_output_trie);
    CM_ASSERT(*pp_output_trie == nullptr);
    regex_trie* p_new = (regex_trie*)malloc(sizeof(regex_trie));
    if (!p_new) {
        return CM_RES_ALLOCATION_FAILURE;
    }
    // Placement new to construct C++ members (vector defaults to empty).
    new (p_new) regex_trie();
    // Explicitly init C/POD members (vector already handled).
    internal_regex_trie_init(&p_new->trie_children);
    p_new->p_leaf_value = nullptr;
    p_new->compiled_regex = nullptr;
    p_new->match_data = nullptr;
    p_new->matcher_updated = true; // Initially "updated" (empty).
    *pp_output_trie = p_new;
    return CM_RES_SUCCESS;
}

// Insert a string into the trie, creating nodes as needed.
// Returns CM_RES_SUCCESS on success, or error (e.g., out of memory).
CM_RES regex_trie_insert(regex_trie* p_trie, const uint8_t* p_regex, void* p_value) {
    CM_ASSERT(p_trie && p_regex);
    //CM_TIMER_START();
    std::string str(reinterpret_cast<const char*>(p_regex));
    std::vector<std::vector<Segment>> paths = regex_literal_splitting(str);
    //CM_TIMER_STOP();
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
                            if (p_new_trie) regex_trie_destroy(p_new_trie);
                            return create_res;
                        }
                        // Insert the new child.
                        itr = internal_regex_trie_insert(&p_current_trie->trie_children, c, (void*)p_new_trie);
                        if (internal_regex_trie_is_end(itr)) {
                            regex_trie_destroy(p_new_trie);
                            return CM_RES_ALLOCATION_FAILURE;
                        }
                    }
                    // Advance: Use itr.data->val for both new/existing cases.
                    p_current_trie = (regex_trie*)itr.data->val;
                    if (!p_current_trie) {
                        return CM_RES_NULL_ARGUMENT; // Corrupt: nullptr child pointer.
                    }
                }
            }
            // if non literal
            else {
                bool found_existing = false;
                for (const auto& pr : p_current_trie->regexes) {
                    if (pr.second == path[seg].str) {
                        p_current_trie = pr.first;
                        found_existing = true;
                        break;
                    }
                }
                // if leaf node already exists then it invalidates the previous leaf_node.
                // Must therefore assert for that case
                if (found_existing && seg == path.size() - 1) {
                    CM_ASSERT(!p_current_trie->p_leaf_value); // Conflict.
                }
                if (!found_existing) {
                    regex_trie* p_new_trie = nullptr;
                    CM_RES create_res = regex_trie_create(&p_new_trie);
                    if (create_res != CM_RES_SUCCESS) {
                        if (p_new_trie) regex_trie_destroy(p_new_trie);
                        return create_res;
                    }
                    std::string str_copy = path[seg].str;
                    // Invalidate branch node's matcher (since new alt added)
                    p_current_trie->matcher_updated = false;
                    p_current_trie->regexes.push_back({p_new_trie, str_copy});
                    p_current_trie = p_new_trie;
                }
            }
        }
        // Always mark/overwrite end of word sentinel (idempotent).
        internal_regex_trie_itr end_itr = internal_regex_trie_insert(&p_current_trie->trie_children, (uint8_t)'\0', nullptr);
        if (internal_regex_trie_is_end(end_itr)) {
            return CM_RES_ALLOCATION_FAILURE;
        }
        p_current_trie->p_leaf_value = p_value;
    }
    return CM_RES_SUCCESS;
}

// Longest prefix match (literal-only; updates on sentinel for exact words/delims).
// Returns FOUND if any EOW prefix (>0), else NOT_FOUND; sets matched=0 if none.
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_string, size_t input_len, size_t* p_output_matched_total, void** pp_output_value) {
    CM_ASSERT(p_trie && p_string && p_output_matched_total && pp_output_value && input_len > 0);
    *p_output_matched_total = 0;
    *pp_output_value = nullptr;
    regex_trie* current = p_trie;  // Non-const for verstable access.
    size_t max_matched = 0;
    void* max_value = nullptr;
    for (size_t i = 0; i < input_len; ++i) {
        uint8_t c = p_string[i];
        internal_regex_trie_itr itr = internal_regex_trie_get(&current->trie_children, c);
        if (internal_regex_trie_is_end(itr)) {
            break;  // No further literal path.
        }
        current = (regex_trie*)itr.data->val;
        if (!current) {
            break;  // Corrupt.
        }
        // Check EOW sentinel after advance (update max only if hit).
        internal_regex_trie_itr end_itr = internal_regex_trie_get(&current->trie_children, (uint8_t)'\0');
        if (!internal_regex_trie_is_end(end_itr) && end_itr.data->val == nullptr) {
            max_matched = i + 1;
            max_value = current->p_leaf_value;
        }
    }
    if (max_matched > 0) {
        *p_output_matched_total = max_matched;
        *pp_output_value = max_value;
        return CM_RES_REGEX_TRIE_NODE_FOUND;
    }
    return CM_RES_REGEX_TRIE_NODE_NOT_FOUND;
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
    for (const auto& regex : p_trie->regexes) {
        if (regex.first != nullptr) {
            if (CM_RES_SUCCESS != regex_trie_destroy(regex.first)) {
                failed = true;
            }
        }
    }
    if (p_trie->compiled_regex) {
        pcre2_code_free(p_trie->compiled_regex);
    }
    if (p_trie->match_data) {
        pcre2_match_data_free(p_trie->match_data);
    }
    internal_regex_trie_cleanup(&p_trie->trie_children);
    // Destruct C++ members before free (cleans vector).
    p_trie->~regex_trie();
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
} // extern "C"