// htrie_wchar.cpp
#include "ast/htrie_wchar.h"
#include "code_monitoring.h"
#include <tsl/htrie_map.h> // HAT-trie header
#include <string>
#include <codecvt>
#include <locale>
#include <cstdlib> // For malloc/free in error paths
// Internal UTF-8 conversion helpers.
static std::string wchar_to_utf8(const std::wstring& wstr) {
    std::wstring_convert<std::codecvt_utf8<wchar_t>, wchar_t> converter;
    return converter.to_bytes(wstr);
}
static std::wstring utf8_to_wchar(const std::string& utf8) {
    std::wstring_convert<std::codecvt_utf8<wchar_t>, wchar_t> converter;
    return converter.from_bytes(utf8);
}
// Opaque struct: Wraps HAT-trie.
struct htrie_wchar {
    tsl::htrie_map<char, void*> internal_trie;
};
// Opaque node struct: Represents a prefix position in the trie.
struct htrie_wchar_node {
    std::string prefix;
    const htrie_wchar* owner_trie;
};
CM_RES htrie_wchar_create(htrie_wchar** pp_output_trie) {
    CM_ASSERT(pp_output_trie);
    try {
        *pp_output_trie = new htrie_wchar{};
        return CM_RES_SUCCESS;
    } catch (...) {
        return CM_RES_HTRIE_INTERNAL_ERROR;
    }
}
CM_RES htrie_wchar_destroy(htrie_wchar* p_trie) {
    if (!p_trie) return CM_RES_SUCCESS;  // Safe for NULL per header
    delete p_trie;
    return CM_RES_SUCCESS;
}
CM_RES htrie_wchar_insert(htrie_wchar* p_trie, const wchar_t* p_key, uint64_t key_len, void* p_value) {
    CM_ASSERT(p_trie && ((p_key != nullptr) == (key_len != 0)));
    try {
        CM_TIMER_START();
        std::string utf8_key = "";
        if (key_len != 0) {
            std::wstring ws(p_key, p_key + key_len);
            utf8_key = wchar_to_utf8(ws);
        }
        CM_TIMER_STOP();
        CM_TIMER_START();
        p_trie->internal_trie.insert(utf8_key, p_value);
        CM_TIMER_STOP();
        return CM_RES_SUCCESS;
    } catch (...) {
        return CM_RES_ALLOCATION_FAILURE;
    }
}
CM_RES htrie_wchar_get(htrie_wchar* p_trie, const wchar_t* p_key, uint64_t key_len, void** pp_output_value) {
    CM_ASSERT(p_trie && ((p_key != nullptr) == (key_len != 0)) && pp_output_value);
    try {
        CM_TIMER_START();
        std::string utf8_key = "";
        if (key_len != 0) {
            std::wstring ws(p_key, p_key + key_len);
            utf8_key = wchar_to_utf8(ws);
        }
        auto it = p_trie->internal_trie.find(utf8_key);
        if (it == p_trie->internal_trie.end()) {
            CM_TIMER_STOP();
            return CM_RES_HTRIE_NODE_NOT_FOUND;
        }
        *pp_output_value = it.value();
        CM_TIMER_STOP();
        return CM_RES_HTRIE_NODE_FOUND;
    } catch (...) {
        CM_TIMER_STOP();
        return CM_RES_ALLOCATION_FAILURE;
    }
}
CM_RES htrie_wchar_size(const htrie_wchar* p_trie, uint64_t* p_output_size) {
    CM_ASSERT(p_trie && p_output_size);
    *p_output_size = static_cast<uint64_t>(p_trie->internal_trie.size());
    return CM_RES_SUCCESS;
}
CM_RES htrie_wchar_node_root(const htrie_wchar* p_trie, htrie_wchar_node** pp_output_root) {
    CM_ASSERT(p_trie && pp_output_root);
    try {
        *pp_output_root = new htrie_wchar_node{"", p_trie};
        return CM_RES_SUCCESS;
    } catch (...) {
        *pp_output_root = nullptr;
        return CM_RES_ALLOCATION_FAILURE;
    }
}
CM_RES htrie_wchar_node_next(const htrie_wchar_node* p_current_node, wchar_t ch, htrie_wchar_node** pp_output_node) {
    CM_ASSERT(p_current_node && pp_output_node);
    try {
        std::wstring wch(1, ch);
        std::string ch_utf8 = wchar_to_utf8(wch);
        std::string new_prefix = p_current_node->prefix + ch_utf8;
        auto& t = p_current_node->owner_trie->internal_trie;
        auto range = t.equal_prefix_range(new_prefix);
        bool found_local = (range.first != range.second);
        if (!found_local) {
            *pp_output_node = nullptr;
            return CM_RES_HTRIE_NODE_NOT_FOUND;
        }
        *pp_output_node = new htrie_wchar_node{new_prefix, p_current_node->owner_trie};
        return CM_RES_HTRIE_NODE_FOUND;
    } catch (...) {
        *pp_output_node = nullptr;
        return CM_RES_ALLOCATION_FAILURE;
    }
}
CM_RES htrie_wchar_node_destroy(htrie_wchar_node* p_node) {
    if (!p_node) return CM_RES_SUCCESS;  // Safe for NULL per header
    delete p_node;
    return CM_RES_SUCCESS;
}
CM_RES htrie_wchar_longest_prefix(const htrie_wchar* p_trie, const wchar_t* p_input, uint64_t input_len, uint64_t* p_matched_length, void** pp_value) {
    CM_ASSERT(p_trie && ((p_input != nullptr) == (input_len != 0)) && p_matched_length && pp_value);
    try {
        CM_TIMER_START();
        *p_matched_length = 0;
        *pp_value = nullptr;

        if (input_len == 0) {
            // Check for empty key at root.
            auto& t = p_trie->internal_trie;
            auto it = t.find("");
            if (it != t.end()) {
                *pp_value = it.value();
            }
            CM_TIMER_STOP();
            return CM_RES_SUCCESS;
        }

        std::wstring ws(p_input, p_input + input_len);
        std::string utf8_key = wchar_to_utf8(ws);

        auto& t = p_trie->internal_trie;
        auto it = t.longest_prefix(utf8_key);
        if (it == t.end()) {
            CM_TIMER_STOP();
            return CM_RES_SUCCESS;
        }

        // Convert matched UTF-8 prefix back to wchar_t to get char count.
        std::wstring matched_w = utf8_to_wchar(it.key());
        *p_matched_length = static_cast<uint64_t>(matched_w.size());
        *pp_value = it.value();
        CM_TIMER_STOP();
        return CM_RES_SUCCESS;
    } catch (...) {
        CM_TIMER_STOP();
        return CM_RES_ALLOCATION_FAILURE;
    }
}
CM_RES htrie_char_longest_prefix(const htrie_wchar* p_trie, const char* p_input, uint64_t max_input_len, uint64_t* p_matched_length, void** pp_value) {
    CM_ASSERT(p_trie && ((p_input != nullptr) == (max_input_len != 0)) && p_matched_length && pp_value);

    *p_matched_length = 0;
    *pp_value = nullptr;

    if (max_input_len == 0) { /* empty check */ }

    // Manual incremental walk: Build prefix incrementally, check existence via library.
    std::string current_prefix;
    current_prefix.reserve(64);  // Assume short matches.
    size_t pos = 0;
    CM_TIMER_START();
    while (pos < max_input_len) {
        current_prefix += p_input[pos];  // Append one byte (UTF-8 safe for prefixes).

        // Use library's equal_prefix_range to check if this prefix exists (O(1) hash of full current_prefix).
        auto& t = p_trie->internal_trie;
        auto range = t.equal_prefix_range(current_prefix);

        if (range.first == range.second) {
            // No match for this longer prefix → done.
            break;
        }

        // It's a match so far; check if terminal (full word/delimiter).
        auto it = t.find(current_prefix);
        if (it != t.end()) {
            *pp_value = it.value();
        }

        ++pos;
        ++(*p_matched_length);
    }
    CM_TIMER_STOP();

    return CM_RES_SUCCESS;
}