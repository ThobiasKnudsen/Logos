#include "ast/regex_trie.h"
#include "ast/regex_literal_splitting.hpp"
#include <stddef.h> // For size_t, nullptr.
#include <stdlib.h> // For malloc, free.
#include <stdbool.h> // For bool.
#include <stdio.h> // For printf, fprintf.
#include <vector>
#include <string>
#include <sstream> // For ostringstream.
#include <algorithm> // For std::sort.
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
    regex_trie_value*   p_leaf_value; // nullptr if not leaf
    pcre2_code*         compiled_regex; // Compiled combined PCRE2 pattern (nullptr if not updated).
    pcre2_match_data*   match_data; // Reusable match data.
    std::vector<std::pair<regex_trie*, std::string>>
                        regexes; // the strings are combined with '|' between for the matcher
    bool                matcher_updated; // if false the matcher has to be lazily recreated when used later
};

struct ChainInfo {
    std::string label;
    const regex_trie* target_node;
};

static bool check_eow(const regex_trie* p_trie) {
    CM_ASSERT(p_trie);
    internal_regex_trie_itr end_itr = internal_regex_trie_get(&((regex_trie*)p_trie)->trie_children, (uint8_t)'\0');
    return !internal_regex_trie_is_end(end_itr) && end_itr.data->val == nullptr;
}
static bool has_children(const regex_trie* p_trie) {
    CM_ASSERT(p_trie);
    size_t num_lit = 0;
    for (internal_regex_trie_itr itr = internal_regex_trie_first(&((regex_trie*)p_trie)->trie_children);
         !internal_regex_trie_is_end(itr);
         itr = internal_regex_trie_next(itr)) {
        uint8_t key = itr.data->key;
        if (key != (uint8_t)'\0' && itr.data->val != nullptr) {
            num_lit++;
        }
    }
    return num_lit > 0 || !p_trie->regexes.empty();
}
static ChainInfo get_literal_chain(const regex_trie* node) {
    ChainInfo ci;
    ci.label = "";
    const regex_trie* curr = node;
    while (true) {
        std::vector<uint8_t> keys;
        for (internal_regex_trie_itr itr = internal_regex_trie_first(&((regex_trie*)curr)->trie_children);
             !internal_regex_trie_is_end(itr);
             itr = internal_regex_trie_next(itr)) {
            uint8_t key = itr.data->key;
            if (key != (uint8_t)'\0' && itr.data->val != nullptr) {
                keys.push_back(key);
            }
        }
        if (keys.size() != 1) break;
        if (!curr->regexes.empty()) break;
        uint8_t c = keys[0];
        ci.label += static_cast<char>(c);
        internal_regex_trie_itr child_itr = internal_regex_trie_get(&((regex_trie*)curr)->trie_children, c);
        if (internal_regex_trie_is_end(child_itr)) break;
        curr = static_cast<const regex_trie*>(child_itr.data->val);
        if (!curr) break;
    }
    ci.target_node = curr;
    return ci;
}
static void print_branches(const regex_trie* p_trie, size_t indent);
static void print_trie_recursive(const regex_trie* p_trie, size_t indent) {
    if (!p_trie) return;
    ChainInfo ci = get_literal_chain(p_trie);
    if (!ci.label.empty()) {
        for (size_t i = 0; i < indent * 4; ++i) printf(" ");
        printf("%s (lit)", ci.label.c_str());
        bool target_eow = check_eow(ci.target_node);
        bool target_leaf = !has_children(ci.target_node);
        if (target_eow && target_leaf) {
            printf(" (EOW)\n");
            return;
        }
        printf("\n");
        print_trie_recursive(ci.target_node, indent + 1);
        return;
    }
    if (check_eow(p_trie)) {
        for (size_t i = 0; i < indent * 4; ++i) printf(" ");
        printf("(EOW)\n");
    }
    print_branches(p_trie, indent);
}
static void print_branches(const regex_trie* p_trie, size_t indent) {
    if (!p_trie) return;
    // Regex branches first, sorted.
    if (!p_trie->regexes.empty()) {
        std::vector<std::pair<std::string, const regex_trie*>> regex_list;
        for (const auto& pr : p_trie->regexes) {
            regex_list.emplace_back(pr.second, pr.first);
        }
        std::sort(regex_list.begin(), regex_list.end());
        for (const auto& r : regex_list) {
            const std::string& rstr = r.first;
            const regex_trie* rchild = r.second;
            for (size_t i = 0; i < (indent + 1) * 4; ++i) printf(" ");
            printf("%s (regex)", rstr.c_str());
            bool child_eow = check_eow(rchild);
            bool child_leaf = !has_children(rchild);
            if (child_eow && child_leaf) {
                printf(" (EOW)\n");
            } else {
                printf("\n");
                print_trie_recursive(rchild, indent + 2);
            }
        }
    }
    // Literal branches, sorted.
    std::vector<uint8_t> lit_keys;
    for (internal_regex_trie_itr itr = internal_regex_trie_first(&((regex_trie*)p_trie)->trie_children);
         !internal_regex_trie_is_end(itr);
         itr = internal_regex_trie_next(itr)) {
        uint8_t key = itr.data->key;
        if (key != (uint8_t)'\0' && itr.data->val != nullptr) {
            lit_keys.push_back(key);
        }
    }
    std::sort(lit_keys.begin(), lit_keys.end());
    for (uint8_t c : lit_keys) {
        internal_regex_trie_itr c_itr = internal_regex_trie_get(&((regex_trie*)p_trie)->trie_children, c);
        if (internal_regex_trie_is_end(c_itr)) continue;
        const regex_trie* child = static_cast<const regex_trie*>(c_itr.data->val);
        ChainInfo ci = get_literal_chain(child);
        std::string full_label = std::string(1, static_cast<char>(c)) + ci.label;
        const regex_trie* target = ci.target_node;
        for (size_t i = 0; i < (indent + 1) * 4; ++i) printf(" ");
        printf("%s (lit)", full_label.c_str());
        bool t_eow = check_eow(target);
        bool t_leaf = !has_children(target);
        if (t_eow && t_leaf) {
            printf(" (EOW)\n");
        } else {
            printf("\n");
            print_trie_recursive(target, indent + 2);
        }
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

regex_trie_value* regex_trie_value_create(const char* p_regex_key, size_t regex_len, size_t value_size_bytes) {
    CM_ASSERT(p_regex_key);
    CM_ASSERT(value_size_bytes >= sizeof(regex_trie_value));
    CM_ASSERT(strlen(p_regex_key) >= regex_len);
    regex_trie_value* p_new_value = (regex_trie_value*)malloc(value_size_bytes);
    p_new_value->p_regex_key = NULL;
    p_new_value->p_regex_key = (uint8_t*)malloc(regex_len+1);
    CM_ASSERT(p_new_value->p_regex_key);
    memcpy((void*)p_new_value->p_regex_key, (void*)p_regex_key, regex_len);
    p_new_value->p_regex_key[regex_len] = '\0';
    return p_new_value;
}
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
CM_RES regex_trie_insert(regex_trie* p_trie, regex_trie_value* p_key_value) {
    CM_ASSERT(p_trie && p_key_value);
    CM_ASSERT(p_key_value->p_regex_key);
    //CM_TIMER_START();
    std::string str(reinterpret_cast<const char*>(p_key_value->p_regex_key));
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
        p_current_trie->p_leaf_value = p_key_value;
    }
    return CM_RES_SUCCESS;
}
struct PathStep {
    regex_trie* node; // The parent node
    bool is_literal;
    uint8_t lit_key;
    size_t regex_index;
};
CM_RES regex_trie_remove(regex_trie* p_trie, const char* p_regex_key, regex_trie_value** pp_output_value) {
    CM_ASSERT(p_trie);
    CM_ASSERT(p_regex_key);
    CM_ASSERT(pp_output_value);
    *pp_output_value = nullptr;
    std::string str(p_regex_key);
    std::vector<std::vector<Segment>> paths = regex_literal_splitting(str);
    bool all_removed = true;
    regex_trie_value* removed_value = nullptr;
    for (const auto& path : paths) {
        std::vector<PathStep> stack;
        regex_trie* current = p_trie;
        bool path_found = true;
        for (size_t seg = 0; seg < path.size() && path_found; ++seg) {
            const Segment& s = path[seg];
            if (s.is_lit) {
                for (size_t i = 0; i < s.str.size() && path_found; ++i) {
                    uint8_t c = static_cast<uint8_t>(s.str[i]);
                    internal_regex_trie_itr itr = internal_regex_trie_get(&current->trie_children, c);
                    if (internal_regex_trie_is_end(itr)) {
                        path_found = false;
                    } else {
                        regex_trie* child = static_cast<regex_trie*>(itr.data->val);
                        stack.push_back({current, true, c, size_t(-1)});
                        current = child;
                    }
                }
            } else {
                size_t found_idx = size_t(-1);
                for (size_t j = 0; j < current->regexes.size(); ++j) {
                    if (current->regexes[j].second == s.str) {
                        found_idx = j;
                        break;
                    }
                }
                if (found_idx == size_t(-1)) {
                    path_found = false;
                } else {
                    regex_trie* child = current->regexes[found_idx].first;
                    stack.push_back({current, false, uint8_t(0), found_idx});
                    current = child;
                }
            }
        }
        if (!path_found || !check_eow(current)) {
            all_removed = false;
            continue;
        }
        if (current->p_leaf_value == nullptr || strcmp(reinterpret_cast<const char*>(current->p_leaf_value->p_regex_key), p_regex_key) != 0) {
            all_removed = false;
            continue;
        }
        // Save the value (assume same for all paths)
        if (removed_value == nullptr) {
            removed_value = current->p_leaf_value;
        } else {
            // Check pointer is the same for every endpoint
            if (removed_value != current->p_leaf_value) {
                all_removed = false;
                continue;
            }
        }
        // Remove the leaf marker
        current->p_leaf_value = nullptr;
        internal_regex_trie_erase(&current->trie_children, static_cast<uint8_t>('\0'));
        // Prune upwards if possible
        while (!stack.empty()) {
            if (has_children(current) || current->p_leaf_value != nullptr || check_eow(current)) {
                break; // Node is not prunable
            }
            PathStep ps = stack.back();
            stack.pop_back();
            regex_trie* parent = ps.node;
            if (ps.is_literal) {
                internal_regex_trie_erase(&parent->trie_children, ps.lit_key);
            } else {
                parent->matcher_updated = false;
                parent->regexes.erase(parent->regexes.begin() + ps.regex_index);
            }
            // Destroy the pruned node
            regex_trie_destroy(current);
            // Move up
            current = parent;
        }
    }
    if (!all_removed) {
        return CM_RES_REGEX_TRIE_NODE_NOT_FOUND;
    }
    *pp_output_value = removed_value;
    return CM_RES_SUCCESS;
}
// Longest prefix match (now handles literal + regex branches; updates on sentinel for exact words/delims).
// Returns FOUND if any EOW prefix (>0), else NOT_FOUND; sets matched=0 if none.
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_string, size_t input_len, size_t* p_output_matched_total, regex_trie_value** pp_output_value) {
    CM_ASSERT(p_trie && p_string && p_output_matched_total && pp_output_value && input_len > 0);
    *p_output_matched_total = 0;
    *pp_output_value = nullptr;
    regex_trie* current = p_trie;
    size_t pos = 0;
    size_t max_matched = 0;
    regex_trie_value* max_value = nullptr;
    while (pos < input_len) {
        uint8_t c = p_string[pos];
        // Try literal child first.
        internal_regex_trie_itr lit_itr = internal_regex_trie_get(&current->trie_children, c);
        bool advanced = false;
        size_t advance_len = 0;
        if (!internal_regex_trie_is_end(lit_itr)) {
            regex_trie* next_current = (regex_trie*)lit_itr.data->val;
            if (next_current) {
                current = next_current;
                advance_len = 1;
                advanced = true;
            }
        }
        if (!advanced) {
            // No literal: Try regex branches.
            if (current->regexes.empty()) {
                break;
            }
            if (!current->matcher_updated) {
                CM_RES update_res = regex_trie_update_matcher(current);
                if (update_res != CM_RES_SUCCESS) {
                    return update_res; // Propagate failure.
                }
            }
            CM_ASSERT(current->compiled_regex);
            // Match combined anchored pattern on remaining input.
            int rc = pcre2_match(current->compiled_regex,
                                 (PCRE2_SPTR)(p_string + pos),
                                 input_len - pos,
                                 0, // Start offset.
                                 0, // Options.
                                 current->match_data,
                                 NULL);
            if (rc < 0 || rc == PCRE2_ERROR_NOMATCH) {
                break; // No match or error.
            }
            PCRE2_SIZE* ovector = pcre2_get_ovector_pointer(current->match_data);
            if (ovector[0] != 0 || ovector[1] == 0) {
                break; // Not anchored at start or empty match.
            }
            size_t regex_len = (size_t)ovector[1];
            // Identify which alternative matched (via first set capturing group).
            size_t which = current->regexes.size(); // Invalid default.
            for (size_t g = 1; g <= current->regexes.size(); ++g) {
                PCRE2_SIZE group_start = ovector[2 * g];
                if (group_start != PCRE2_UNSET) {
                    which = g - 1;
                    break; // Left-to-right: first matching alt.
                }
            }
            if (which >= current->regexes.size()) {
                break; // Matched but no group? Rare/corrupt.
            }
            current = current->regexes[which].first;
            advance_len = regex_len;
            advanced = true;
        }
        if (!advanced) {
            break;
        }
        pos += advance_len;
        // Check EOW sentinel after advance.
        internal_regex_trie_itr end_itr = internal_regex_trie_get(&current->trie_children, (uint8_t)'\0');
        if (!internal_regex_trie_is_end(end_itr) && end_itr.data->val == nullptr) {
            max_matched = pos;
            max_value = current->p_leaf_value;
        }
    }
    if (max_matched > 0) {
        CM_ASSERT(max_value);
        CM_ASSERT(max_value->p_regex_key);
        bool is_same = true;
        size_t regex_key_len = strlen((const char*)max_value->p_regex_key);
        CM_ASSERT(regex_key_len <= input_len);
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
CM_RES regex_trie_print(const regex_trie* p_trie) {
    CM_ASSERT(p_trie);
    print_trie_recursive(p_trie, 0);
    return CM_RES_SUCCESS;
}
} // extern "C"