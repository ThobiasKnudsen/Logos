// ast/tokenize.c
#include "ast/tokenize.h"
#include <stdbool.h>
#include <wchar.h>
#include <wctype.h>  // For iswalpha, iswdigit (optional Unicode support)
#include "code_monitoring.h"

static const wchar_t punktation_chars[] = {L' ', L'\r', L'\t', L'\n', L'.', L',', L':', L';', L'(', L')', L'[', L']', L'{', L'}', L'"', L'\''};
static const size_t num_punktation_chars = sizeof(punktation_chars) / sizeof(wchar_t);
static const wchar_t operator_chars[] = {L'=', L'!', L'<', L'>', L'?', L'/', L'*', L'+', L'-', L'^', L'%', L'&', L'|'};
static const size_t num_operator_chars = sizeof(operator_chars) / sizeof(wchar_t);
static const wchar_t *valid_operators[] = {L"=", L"/=", L"*=", L"-=", L"+=", L"==", L"!=", L">=", L"<=", L">", L"<", L"*", L"/", L"+", L"-", L"^", L"%", L"&&", L"||", L"!", L"?"};
static const size_t num_valid_operators = sizeof(valid_operators) / sizeof(valid_operators[0]);
static const wchar_t valid_but_unused_chars[] = {L' ', L'\r', L'\t', L'\n'};
static const size_t num_valid_but_unused_chars = sizeof(valid_but_unused_chars) / sizeof(wchar_t);

static bool is_operator_char(wchar_t ch) {
    for (size_t j = 0; j < num_operator_chars; j++) {
        if (ch == operator_chars[j]) {
            return true;
        }
    }
    return false;
}

static bool is_valid_operator(const wchar_t *p_src_string, size_t start, size_t length) {
    if (length == 0) return false;
    // Check if all chars are operator chars
    for (size_t k = 0; k < length; k++) {
        if (!is_operator_char(p_src_string[start + k])) {
            return false;
        }
    }
    // Check against valid list
    for (size_t j = 0; j < num_valid_operators; j++) {
        size_t op_len = wcslen(valid_operators[j]);
        if (op_len == length && wmemcmp(&p_src_string[start], valid_operators[j], length) == 0) {
            return true;
        }
    }
    return false;
}

static CM_RES add_token(struct ast_tokens *p_output_tokens, size_t start_index, size_t token_length) {
    CM_ASSERT(p_output_tokens != NULL);
    if (token_length == 0) {
        return CM_RES_SUCCESS;  // No-op for zero-length
    }
    size_t new_length = p_output_tokens->tokens_length + 1;
    uint32_t start_u32 = (uint32_t)start_index;
    uint32_t length_u32 = (uint32_t)token_length;
    struct ast_token *new_tokens = realloc(p_output_tokens->p_tokens, sizeof(struct ast_token) * new_length);
    if (new_tokens == NULL) {
        CM_LOG_ERROR("realloc failure for tokens array");
    }
    p_output_tokens->p_tokens = new_tokens;
    p_output_tokens->p_tokens[new_length - 1] = (struct ast_token) {
        .token_start_in_src_string = start_u32,
        .token_length = length_u32
    };
    p_output_tokens->tokens_length = new_length;
    return CM_RES_SUCCESS;
}

CM_RES ast_tokenize(const wchar_t *p_src_string, struct ast_tokens *p_output_tokens) {
    CM_ASSERT(p_src_string != NULL && p_output_tokens != NULL);
    CM_ASSERT(p_output_tokens->p_tokens == NULL);
    p_output_tokens->tokens_length = 0;
    p_output_tokens->p_tokens = NULL;
    size_t src_string_length = wcslen(p_src_string);
    size_t start_index = 0;
    size_t end_index = 0;
    bool token_is_number = false;
    bool token_contains_dot = false;
    bool token_is_identifier = false;
    bool token_is_operator = false;

    for (size_t i = 0; i < src_string_length; i++) {
        wchar_t ch = p_src_string[i];
        bool found_single_token_case = false;
        for (size_t j = 0; j < num_punktation_chars; j++) {
            if (ch == punktation_chars[j]) {
                found_single_token_case = true;
                break;
            }
        }
        if (found_single_token_case) {
            if (ch == L'.' && token_is_number) {
                if (token_contains_dot) {
                    CM_LOG_ERROR("number cannot contain more than one '.'\n");
                }
                token_contains_dot = true;
                end_index = i + 1;
            } else {
                // Flush previous token if any
                size_t token_length = end_index - start_index;
                if (token_length != 0) {
                    CM_RES res = add_token(p_output_tokens, start_index, token_length);
                    if (res != CM_RES_SUCCESS) {
                        free(p_output_tokens->p_tokens);
                        p_output_tokens->p_tokens = NULL;
                        p_output_tokens->tokens_length = 0;
                        return res;
                    }
                    // Validate if operator
                    if (token_is_operator && !is_valid_operator(p_src_string, start_index, token_length)) {
                        CM_LOG_ERROR("invalid operator at index %zu: %.*ls\n", start_index, (int)token_length, &p_src_string[start_index]);
                    }
                }
                // Reset flags
                token_is_number = false;
                token_contains_dot = false;
                token_is_identifier = false;
                token_is_operator = false;

                // Check if whitespace to skip
                bool is_non_token_valid_character = false;
                for (size_t j = 0; j < num_valid_but_unused_chars; j++) {
                    if (ch == valid_but_unused_chars[j]) {
                        is_non_token_valid_character = true;
                        break;
                    }
                }
                if (!is_non_token_valid_character) {
                    CM_RES res = add_token(p_output_tokens, i, 1);
                    if (res != CM_RES_SUCCESS) {
                        free(p_output_tokens->p_tokens);
                        p_output_tokens->p_tokens = NULL;
                        p_output_tokens->tokens_length = 0;
                        return res;
                    }
                }
                start_index = i + 1;
                end_index = i + 1;
            }
            continue;
        }

        // Non-punctuation
        bool char_is_identifier = iswalpha(ch) || (ch == L'_');  // Using iswalpha for better Unicode support
        bool char_is_number = iswdigit(ch);
        bool char_is_operator_char = !char_is_identifier && !char_is_number && is_operator_char(ch);

        if (!char_is_operator_char && !char_is_identifier && !char_is_number) {
            CM_LOG_ERROR("char %lc at index %zu is neither identifier, number, nor operator\n", ch, i);
        }

        if (!token_is_number && !token_is_identifier && !token_is_operator) {
            // Start new token
            if (char_is_identifier) token_is_identifier = true;
            if (char_is_number) token_is_number = true;
            if (char_is_operator_char) token_is_operator = true;
        } else if (char_is_identifier && token_is_number) {
            // Switch number to identifier (e.g., 3g)
            if (token_contains_dot) {
                CM_LOG_ERROR("letter or underscore comes right after '[number].' which is not allowed\n");
            }
            token_is_number = false;
            token_is_identifier = true;
        } else if ((char_is_identifier || char_is_number) && token_is_operator) {
            // Transition: operator to id/num
            size_t token_length = end_index - start_index;
            if (token_length == 0) {
                CM_LOG_ERROR("somehow collected chars is operator but char length is 0?\n");
            }
            CM_RES res = add_token(p_output_tokens, start_index, token_length);
            if (res != CM_RES_SUCCESS) {
                free(p_output_tokens->p_tokens);
                p_output_tokens->p_tokens = NULL;
                p_output_tokens->tokens_length = 0;
                return res;
            }
            if (!is_valid_operator(p_src_string, start_index, token_length)) {
                CM_LOG_ERROR("invalid operator at index %zu: %.*ls\n", start_index, (int)token_length, &p_src_string[start_index]);
            }
            token_is_operator = false;
            start_index = i;
            end_index = i + 1;
            // Start new token with current char
            if (char_is_identifier) token_is_identifier = true;
            if (char_is_number) token_is_number = true;
        } else if ((token_is_identifier || token_is_number) && char_is_operator_char) {
            // Transition: id/num to operator
            size_t token_length = end_index - start_index;
            if (token_length == 0) {
                CM_LOG_ERROR("somehow collected chars is identifier or number but char length is 0?\n");
            }
            CM_RES res = add_token(p_output_tokens, start_index, token_length);
            if (res != CM_RES_SUCCESS) {
                free(p_output_tokens->p_tokens);
                p_output_tokens->p_tokens = NULL;
                p_output_tokens->tokens_length = 0;
                return res;
            }
            token_is_identifier = false;
            token_is_number = false;
            token_contains_dot = false;
            start_index = i;
            end_index = i + 1;
            token_is_operator = true;
        }
        // Continue accumulating
        end_index = i + 1;
    }

    // Flush final token if any
    size_t token_length = end_index - start_index;
    if (token_length != 0) {
        CM_RES res = add_token(p_output_tokens, start_index, token_length);
        if (res != CM_RES_SUCCESS) {
            free(p_output_tokens->p_tokens);
            p_output_tokens->p_tokens = NULL;
            p_output_tokens->tokens_length = 0;
            return res;
        }
        if (token_is_operator && !is_valid_operator(p_src_string, start_index, token_length)) {
            CM_LOG_ERROR("invalid operator at index %zu: %.*ls\n", start_index, (int)token_length, &p_src_string[start_index]);
        }
    }

    return CM_RES_SUCCESS;
}

CM_RES ast_tokens_print(const wchar_t *p_src_string, const struct ast_tokens *p_tokens) {
    CM_ASSERT(p_src_string != NULL && p_tokens != NULL);
    CM_ASSERT((p_tokens->tokens_length == 0 && p_tokens->p_tokens == NULL) ||
              (p_tokens->tokens_length > 0 && p_tokens->p_tokens != NULL));
    for (uint32_t i = 0; i < p_tokens->tokens_length; i++) {
        struct ast_token token = p_tokens->p_tokens[i];
        uint32_t token_start = token.token_start_in_src_string;
        uint32_t token_length = token.token_length;
        printf("%.*ls ", token_length, &p_src_string[token_start]);
    }
    return CM_RES_SUCCESS;
}