
#include "ast/core.h"
#include "code_monitoring.h"

enum ast_token_type {
	AST_TOKEN_TYPE_STRING,
	AST_TOKEN_TYPE_OPEN_BRACKET ,
	AST_TOKEN_TYPE_CLOSE_BRACKET ,
	AST_TOKEN_TYPE ,
	AST_TOKEN_TYPE ,
	AST_TOKEN_TYPE ,
	AST_TOKEN_TYPE ,
	AST_TOKEN_TYPE ,
	AST_TOKEN_TYPE 
};

enum ast_node_type {
	AST_NODE_TYPE_VAR,
	AST_NODE_TYPE_IF,
	AST_NODE_TYPE_ELSE,
	AST_NODE_TYPE_ELSE_IF,
	AST_NODE_TYPE_FOR,
	AST_NODE_TYPE_WHILE,
	AST_NODE_TYPE_ASSIGN, // a = b
	AST_NODE_TYPE_
};

struct ast_node {

}



struct ast {

};

CM_RES ast_tokenize(const char* str, )

CM_RES ast_parse(const char* str, ) {
	CM_ASSERT(str);
	uint32_t str_len = strlen(str);

}