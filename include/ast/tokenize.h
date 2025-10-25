// ast/tokenize.h
#ifndef AST_TOKENIZE_H
#define AST_TOKENIZE_H

#include "code_monitoring.h"
#include <wchar.h>
#include <stdint.h>

enum ast_token_type {
    AST_TOKEN_TYPE_NUMBER,
    AST_TOKEN_TYPE_IDENTIFIER
};

struct ast_token {
    uint32_t token_start_in_src_string; // pointer to first character in source string
    uint32_t token_length; // length of token
};

struct ast_tokens {
    struct ast_token *p_tokens;
    uint32_t tokens_length;
};

CM_RES ast_tokenize(const wchar_t *p_src_string, struct ast_tokens *p_output_tokens);
CM_RES ast_tokens_print(const wchar_t *p_src_string, const struct ast_tokens *p_tokens);

#endif // AST_TOKENIZE_H