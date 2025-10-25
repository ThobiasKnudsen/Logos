#include "ast/tokenize.h"
#include "ast/char32_trie.h"
#include "ast/htrie_wchar.h"
#include "code_monitoring.h"
#include <locale.h>
#include <stdio.h> // For printf (if not included via others).
#include <stddef.h> // For uint64_t.
#include <wchar.h> // For fgetws, wcslen, etc. (unused now).
#include <uchar.h> // For char32_t.

// Test inserting many words from words.txt and printing the words.
static void test_trie_many_words_print(void) {
    setlocale(LC_ALL, ""); // For proper printing.
    char32_trie* p_root = NULL;
    CM_ASSERT(CM_RES_SUCCESS == trie_create(&p_root));
    // Insert "hello" for verification (char32_t literal).
    CM_ASSERT(CM_RES_SUCCESS == trie_insert(p_root, U"hello"));
    CM_ASSERT(CM_RES_SUCCESS == trie_insert(p_root, U"\n"));
    CM_ASSERT(CM_RES_SUCCESS == trie_insert(p_root, U"\r"));
    CM_ASSERT(CM_RES_SUCCESS == trie_insert(p_root, U" "));
    // Open and read words.txt (assume one word per line, UTF-8/ASCII).
    FILE* fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    enum { MAX_WORD_LEN = 1024 };
    char line[MAX_WORD_LEN]; // Read as bytes.
    uint64_t num_words = 0;
    while (fgets(line, MAX_WORD_LEN, fp)) {
        // Trim trailing newline/whitespace.
        size_t len = strlen(line);
        if (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) {
            line[len - 1] = '\0';
            len--; // No strlen re-call.
        }
        // Skip empty lines.
        if (len == 0) continue;
        // Convert to char32_t (ASCII-safe).
        char32_t str32[MAX_WORD_LEN + 1];
        for (size_t i = 0; i < len; ++i) {
            str32[i] = (char32_t)(unsigned char)line[i]; // Unsigned to avoid sign-ext.
        }
        str32[len] = 0;
        CM_ASSERT(CM_RES_SUCCESS == trie_insert(p_root, str32));
        num_words++;
    }
    fclose(fp);
    // Verify per-line.
    fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    num_words = 0;
    while (fgets(line, MAX_WORD_LEN, fp)) {
        size_t len = strlen(line);
        if (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) {
            line[len - 1] = '\0';
            len--;
        }
        if (len == 0) continue;
        char32_t str32[MAX_WORD_LEN + 1];
        for (size_t i = 0; i < len; ++i) {
            str32[i] = (char32_t)(unsigned char)line[i];
        }
        str32[len] = 0;
        // Manual len for trie_get (fix wcslen bug).
        size_t str32_len = len; // 1:1 for ASCII.
        // But if trie_get uses wcslen, replace with loop:
        // size_t str32_len = 0; while (str32[str32_len]) ++str32_len;
        CM_ASSERT(trie_get(p_root, str32) == true);
        num_words++;
    }
    fclose(fp);
    // Bulk verification using trie_char_longest_prefix.
    fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    fseek(fp, 0, SEEK_END);
    long file_size = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    char* byte_buffer = malloc(file_size + 1);
    CM_ASSERT(byte_buffer);
    size_t read_bytes = fread(byte_buffer, 1, file_size, fp);
    byte_buffer[read_bytes] = '\0';
    fclose(fp);
    enum { MAX_TOKEN_LEN = 1024 };
    uint64_t bulk_num_words = 0;
    uint64_t byte_offset = 0;
    while (byte_offset < read_bytes) {
        uint64_t this_len = read_bytes - byte_offset;
        if (this_len > MAX_TOKEN_LEN) this_len = MAX_TOKEN_LEN;
        uint64_t matched_len;
        void* value_out = NULL;
        CM_ASSERT(CM_RES_SUCCESS == trie_longest_char_prefix(p_root, byte_buffer + byte_offset, this_len, &matched_len, &value_out));
        if (matched_len == 0) {
            byte_offset += 1; // Skip unexpected chars.
            continue;
        } else {
            char temp_str[matched_len + 1];
            memcpy(temp_str, byte_buffer + byte_offset, matched_len);
            temp_str[matched_len] = '\0';
            printf("%s\n", temp_str);
        }
        // Count as word if not a single-char delimiter.
        char first_char = byte_buffer[byte_offset];
        if (matched_len > 1 || (matched_len == 1 && first_char != '\n' && first_char != '\r' && first_char != ' ')) {
            bulk_num_words++;
        }
        byte_offset += matched_len;
    }
    free(byte_buffer);
    CM_ASSERT(bulk_num_words == num_words); // Now matches!
    CM_LOG_NOTICE("Bulk verified %zu words using trie_char_longest_prefix.\n", bulk_num_words);
    CM_LOG_NOTICE("Char32 Trie after inserting %zu words.\n", num_words);
    trie_destroy(p_root);
}
// New test: inserting many words from words.txt using htrie_wchar.
static void test_htrie_wchar_many_words(void) {
    setlocale(LC_ALL, ""); // For proper wchar_t printing.
    htrie_wchar* p_trie = NULL;
    CM_ASSERT(CM_RES_SUCCESS == htrie_wchar_create(&p_trie));
    CM_ASSERT(p_trie);
    // Insert "hello" for verification (with NULL value for set-like).
    CM_ASSERT(CM_RES_SUCCESS == htrie_wchar_insert(p_trie, L"hello", 5, NULL));
    CM_ASSERT(CM_RES_SUCCESS == htrie_wchar_insert(p_trie, L"\n", 1, NULL));
    CM_ASSERT(CM_RES_SUCCESS == htrie_wchar_insert(p_trie, L"\r", 1, NULL));
    CM_ASSERT(CM_RES_SUCCESS == htrie_wchar_insert(p_trie, L" ", 1, NULL));
    // Open and read words.txt (assume one word per line, UTF-8/ASCII).
    FILE* fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    enum { MAX_WORD_LEN = 1024 };
    wchar_t line[MAX_WORD_LEN];
    uint64_t num_words = 0;
    while (fgetws(line, MAX_WORD_LEN, fp)) {
        uint64_t len = wcslen(line);
        // Skip empty lines (including just \n).
        if (len <= 1 && (len == 0 || line[0] == L'\n')) continue;
        if (len > 0 && (line[len - 1] == L'\n' || line[len - 1] == L'\r')) {
            line[len - 1] = L'\0';
            len = wcslen(line); // Update after trim.
        }
        CM_ASSERT(CM_RES_SUCCESS == htrie_wchar_insert(p_trie, line, len, NULL));
        num_words++;
    }
    fclose(fp);
    // Verify (per-line).
    fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    num_words = 0;
    while (fgetws(line, MAX_WORD_LEN, fp)) {
        uint64_t len = wcslen(line);
        if (len <= 1 && (len == 0 || line[0] == L'\n')) continue;
        if (len > 0 && (line[len - 1] == L'\n' || line[len - 1] == L'\r')) {
            line[len - 1] = L'\0';
            len = wcslen(line);
        }
        void* value_out;
        CM_ASSERT(CM_RES_HTRIE_NODE_FOUND == htrie_wchar_get(p_trie, line, len, &value_out));
        num_words++;
    }
    fclose(fp);
   
    // Verify (bulk version using char).
    fp = fopen("../words.txt", "r");
    CM_ASSERT(fp);
    fseek(fp, 0, SEEK_END);
    long file_size = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    char* byte_buffer = malloc(file_size + 1);
    CM_ASSERT(byte_buffer);
    size_t read_bytes = fread(byte_buffer, 1, file_size, fp);
    byte_buffer[read_bytes] = '\0';
    fclose(fp);
    enum { MAX_TOKEN_LEN = 1024 };
    uint64_t bulk_num_words = 0;
    uint64_t byte_offset = 0;
    while (byte_offset < read_bytes) {
        uint64_t this_len = read_bytes - byte_offset;
        if (this_len > MAX_TOKEN_LEN) this_len = MAX_TOKEN_LEN;
        uint64_t matched_len;
        void* value_out = NULL;
        CM_RES res = htrie_char_longest_prefix(p_trie, byte_buffer + byte_offset, this_len, &matched_len, &value_out);
        CM_ASSERT(CM_RES_SUCCESS == res);
        if (matched_len == 0) {
            byte_offset += 1; // Skip unexpected chars.
            continue;
        }
        // Count as word if not a single-char delimiter.
        char first_char = byte_buffer[byte_offset];
        if (matched_len > 1 || (matched_len == 1 && first_char != '\n' && first_char != '\r' && first_char != ' ')) {
            bulk_num_words++;
        }
        byte_offset += matched_len;
    }
    free(byte_buffer);
    CM_ASSERT(bulk_num_words == num_words); // Ensure bulk matches insertion count.
    CM_LOG_NOTICE("Bulk verified %zu words.\n", bulk_num_words);
   
    uint64_t trie_size = 0;
    CM_ASSERT(CM_RES_SUCCESS == htrie_wchar_size(p_trie, &trie_size));
    CM_LOG_NOTICE("HTrie WChar trie after inserting %zu words (size: %zu):\n", num_words, trie_size);
    htrie_wchar_destroy(p_trie);
}
int main() {
    test_trie_many_words_print();
    test_htrie_wchar_many_words();
    CM_TIMER_PRINT();
    /*
    const wchar_t* test_string_1 = L"542.6752 542.6752/=ijv _grw7 573jiv int main() { int x=0; x = x + 1; if (x > 0) { printf(\"value: %d\", x); } char c = 'a'; int arr[10]; s.field = 42; a ? b : c; !x / x ^ x % x _valid; }";
    struct ast_tokens tokens_1 = {0};
    CM_ASSERT(CM_RES_SUCCESS == ast_tokenize(test_string_1, &tokens_1));
    CM_ASSERT(CM_RES_SUCCESS == ast_tokens_print(test_string_1, &tokens_1));
    */
    return 0;
}