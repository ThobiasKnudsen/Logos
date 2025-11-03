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
#include <reflex/matcher.h>
#include <exception> // For std::exception.
#include <utility> // For std::pair.
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
    reflex::Matcher* matcher; // Compiled combined RE/flex pattern (nullptr if not updated).
    std::vector<std::pair<regex_trie*, std::string>>
                        regexes; // the strings are combined with '|' between for the matcher
    bool matcher_updated; // if false the matcher has to be lazily recreated when used later
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
    if (p_trie->matcher) {
        delete p_trie->matcher;
        p_trie->matcher = nullptr;
    }
    std::string pattern_str;
    if (p_trie->regexes.empty()) {
        pattern_str = "^(?!)";
    } else {
        std::ostringstream oss;
        oss << "^";
        for (size_t i = 0; i < p_trie->regexes.size(); ++i) {
            if (i > 0) oss << "|";
            oss << "(" << p_trie->regexes[i].second << ")";
        }
        pattern_str = oss.str();
    }
    try {
        p_trie->matcher = new reflex::Matcher(pattern_str.c_str());
    } catch (const std::exception& e) {
        fprintf(stderr, "RE/flex compile fail: %s\n", e.what());
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
    p_new->matcher = nullptr;
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
// Longest prefix match (now handles literal + regex branches; updates on sentinel for exact words/delims).
// Returns FOUND if any EOW prefix (>0), else NOT_FOUND; sets matched=0 if none.
CM_RES regex_trie_get(regex_trie* p_trie, const uint8_t* p_string, size_t input_len, size_t* p_output_matched_total, void** pp_output_value) {
    CM_ASSERT(p_trie && p_string && p_output_matched_total && pp_output_value && input_len > 0);
    //CM_TIMER_START();
    *p_output_matched_total = 0;
    *pp_output_value = nullptr;
    regex_trie* current = p_trie;
    size_t pos = 0;
    size_t max_matched = 0;
    void* max_value = nullptr;
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
                    CM_TIMER_STOP();
                    return update_res; // Propagate failure.
                }
            }
            if (!current->matcher) {
                break; // Shouldn't happen post-update.
            }
            // Set input to remaining string.
            //CM_TIMER_START();
            reflex::Input rinput(static_cast<const char*>(static_cast<const void*>(p_string + pos)), input_len - pos);
            current->matcher->input(rinput);
            current->matcher->reset();
            //CM_TIMER_STOP();
            //CM_TIMER_START();
            // Match combined anchored pattern on remaining input.
            size_t accept_val = current->matcher->find();
            //CM_TIMER_STOP();
            //CM_TIMER_START();
            if (accept_val == 0) {
                break; // No match.
            }
            if (current->matcher->first() != 0 || current->matcher->size() == 0) {
                break; // Not anchored at start or empty match.
            }
            size_t regex_len = current->matcher->size();
            // Identify which alternative matched (loop over groups like PCRE2).
            size_t which = current->regexes.size(); // Invalid default.
            for (size_t g = 1; g <= current->regexes.size(); ++g) {
                std::pair<const char*, size_t> group = (*current->matcher)[g];
                if (group.second > 0) {
                    which = g - 1;
                    break; // Left-to-right: first matching alt.
                }
            }
            //CM_TIMER_STOP();
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
        *p_output_matched_total = max_matched;
        *pp_output_value = max_value;
        //CM_TIMER_STOP();
        return CM_RES_REGEX_TRIE_NODE_FOUND;
    }
    //CM_TIMER_STOP();
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
    if (p_trie->matcher) {
        delete p_trie->matcher;
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