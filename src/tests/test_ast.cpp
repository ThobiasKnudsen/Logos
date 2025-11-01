#include "ast/tokenize.h"
#include "ast/regex_trie.h"
#include "code_monitoring.h"
#include "ast/regex_literal_splitting.hpp"
#include <locale.h>
#include <stdio.h> // For printf (if not included via others).
#include <stddef.h> // For size_t.
#include <wchar.h> // For fgetws, wcslen, etc. (unused now).
#include <uchar.h> // For char32_t.

// Test inserting many words from words.txt and printing the words.
static void test_regex_trie_many_words_print(void) {
    CM_TIMER_START();

    setlocale(LC_ALL, ""); // For proper printing.
    regex_trie* p_root = NULL;
    CM_ASSERT(CM_RES_SUCCESS == regex_trie_create(&p_root));
    // Insert "hello" for verification (uint8_t literal).
    CM_ASSERT(CM_RES_SUCCESS == regex_trie_insert(p_root, (const uint8_t*)"hello"));
    CM_ASSERT(CM_RES_SUCCESS == regex_trie_insert(p_root, (const uint8_t*)"\n"));
    CM_ASSERT(CM_RES_SUCCESS == regex_trie_insert(p_root, (const uint8_t*)"\r"));
    CM_ASSERT(CM_RES_SUCCESS == regex_trie_insert(p_root, (const uint8_t*)" "));
    // Open and read words.txt (assume one word per line, UTF-8/ASCII).
    FILE* fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    enum { MAX_WORD_LEN = 1024 };
    char line[MAX_WORD_LEN]; // Read as bytes.
    size_t num_words = 0;
    CM_TIMER_START(); // 815ms
    while (fgets(line, MAX_WORD_LEN, fp)) {
        // Trim trailing newline/whitespace.
        size_t len = strlen(line);
        if (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) {
            line[len - 1] = '\0';
            len--; // No strlen re-call.
        }
        // Skip empty lines.
        if (len == 0) continue;
        // Use bytes directly (ASCII-safe).
        CM_ASSERT(CM_RES_SUCCESS == regex_trie_insert(p_root, (const uint8_t*)line));
        num_words++;
    }
    CM_TIMER_STOP();
    fclose(fp);
    // Verify per-line.
    fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    num_words = 0;
    CM_TIMER_START(); // 548ms
    while (fgets(line, MAX_WORD_LEN, fp)) {
        size_t len = strlen(line);
        if (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) {
            line[len - 1] = '\0';
            len--;
        }
        if (len == 0) continue;
        // Use bytes directly (ASCII-safe).
        CM_ASSERT(CM_RES_REGEX_TRIE_NODE_FOUND == regex_trie_get(p_root, (const uint8_t*)line));
        num_words++;
    }
    CM_TIMER_STOP();
    fclose(fp);
    // Bulk verification using regex_trie_get_longest_prefix.
    fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    fseek(fp, 0, SEEK_END);
    long file_size = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    char* byte_buffer = (char*)malloc(file_size + 1);
    CM_ASSERT(byte_buffer);
    size_t read_bytes = fread(byte_buffer, 1, file_size, fp);
    byte_buffer[read_bytes] = '\0';
    fclose(fp);
    enum { MAX_TOKEN_LEN = 1024 };
    size_t bulk_num_words = 0;
    size_t byte_offset = 0;
    CM_TIMER_START(); // 692ms
    while (byte_offset < read_bytes) {
        size_t this_len = read_bytes - byte_offset;
        if (this_len > MAX_TOKEN_LEN) this_len = MAX_TOKEN_LEN;
        size_t matched_len;
        void* value_out = NULL;
        CM_ASSERT(CM_RES_SUCCESS == regex_trie_get_longest_prefix(p_root, (const uint8_t*)(byte_buffer + byte_offset), this_len, &matched_len, &value_out));
        if (matched_len == 0) {
            byte_offset += 1; // Skip unexpected chars.
            continue;
        } else {
            char temp_str[matched_len + 1];
            memcpy(temp_str, byte_buffer + byte_offset, matched_len);
            temp_str[matched_len] = '\0';
            // printf("%s\n", temp_str);
        }
        // Count as word if not a single-char delimiter.
        char first_char = byte_buffer[byte_offset];
        if (matched_len > 1 || (matched_len == 1 && first_char != '\n' && first_char != '\r' && first_char != ' ')) {
            bulk_num_words++;
        }
        byte_offset += matched_len;
    }
    CM_TIMER_STOP();
    free(byte_buffer);
    CM_ASSERT(bulk_num_words == num_words); // Now matches!
    CM_LOG_NOTICE("Bulk verified %zu words using regex_trie_get_longest_prefix.\n", bulk_num_words);
    CM_LOG_NOTICE("Regex Trie (Verstable) after inserting %zu words.\n", num_words);
    CM_TIMER_START(); // 182ms
    regex_trie_destroy(p_root);
    CM_TIMER_STOP();

    CM_TIMER_STOP();
}

int main() {
    test_regex_trie_many_words_print();
    CM_TIMER_PRINT();
    CM_TIMER_CLEAR();
    test_regex_trie_many_words_print();
    CM_TIMER_PRINT();
    CM_TIMER_CLEAR();

    regex_literal_splitting_test();
    // test_htrie_wchar_many_words();
    /*
    const wchar_t* test_string_1 = L"542.6752 542.6752/=ijv _grw7 573jiv int main() { int x=0; x = x + 1; if (x > 0) { printf(\"value: %d\", x); } char c = 'a'; int arr[10]; s.field = 42; a ? b : c; !x / x ^ x % x _valid; }";
    struct ast_tokens tokens_1 = {0};
    CM_ASSERT(CM_RES_SUCCESS == ast_tokenize(test_string_1, &tokens_1));
    CM_ASSERT(CM_RES_SUCCESS == ast_tokens_print(test_string_1, &tokens_1));
    */
    return 0;
}