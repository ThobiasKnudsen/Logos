#include "ast/ast.h"
#include "ast/regex_trie.h"
#include "code_monitoring.h"

enum ast_node_type {
	AST_NODE_TYPE_NULL,
	AST_NODE_TYPE_UNDEFINED,
	AST_NODE_TYPE_FUNCTION,
	AST_NODE_TYPE_VARIABLE,
	AST_NODE_TYPE_BLOCK,
	AST_NODE_TYPE_OPERATION,
	AST_NODE_TYPE_LITERAL,
	AST_NODE_TYPE_FOR_LOOP,
	AST_NODE_TYPE_WHILE_LOOP,
	AST_NODE_TYPE_IF,
	AST_NODE_TYPE_ELSE,
	AST_NODE_TYPE_IMPORT, // imporing other files as relative path in string. will just insert it as a block
};

enum ast_data_type {
	AST_DATA_TYPE_VOID,
	AST_DATA_TYPE_UNDEFINED,
	AST_DATA_TYPE_VARIABLE, // for "var" in synax 
	AST_DATA_TYPE_F32,
	AST_DATA_TYPE_F64,
	AST_DATA_TYPE_I8,
	AST_DATA_TYPE_I16,
	AST_DATA_TYPE_I32,
	AST_DATA_TYPE_I64,
	AST_DATA_TYPE_U8,
	AST_DATA_TYPE_U16,
	AST_DATA_TYPE_U32,
	AST_DATA_TYPE_U64,
	AST_DATA_TYPE_BOOL,
	AST_DATA_TYPE_TUPLE,
};

enum ast_operation_type {
	AST_OP_TYPE_NULL,
	AST_OP_TYPE_UNDEFINED,

	AST_OP_TYPE_ASSIGN, // (a: 77) or (expr: (b: 7, b+77)

	AST_OP_TYPE_ADD,
	AST_OP_TYPE_SUB,
	AST_OP_TYPE_MUL,
	AST_OP_TYPE_DIV,
	AST_OP_TYPE_EXP,
	AST_OP_TYPE_MODULO,

	// these are for casting the value to the right
	AST_OP_TYPE_F64,
	AST_OP_TYPE_F32,
	AST_OP_TYPE_I8,
	AST_OP_TYPE_I16,
	AST_OP_TYPE_I32,
	AST_OP_TYPE_I64,
	AST_OP_TYPE_U8,
	AST_OP_TYPE_U16,
	AST_OP_TYPE_U32,
	AST_OP_TYPE_U64,
	AST_OP_TYPE_BOOL,

	AST_OP_TYPE_AND,
	AST_OP_TYPE_OR,
	AST_OP_TYPE_XOR,
	AST_OP_TYPE_IMPLY,
	AST_OP_TYPE_NOT,

	AST_OP_TYPE_EQUAL,
	AST_OP_TYPE_NOT_EQUAL,
	AST_OP_TYPE_GREATER_THAN,
	AST_OP_TYPE_LESS_THAN,
	AST_OP_TYPE_GREATER_THAN_OR_EQUAL,
	AST_OP_TYPE_LESS_THAN_OR_EQUAL,
};

typedef struct ast_node_null {

} ast_node_null;

typedef struct ast_node_undefined {

} ast_node_undefined;

typedef struct ast_node_function {
	uint32_t argument_block; // NULL if an expression
	uint32_t content_block; // content of the function
} ast_node_function;

typedef struct ast_node_variable {
	enum ast_data_type data_type;
} ast_node_variable;

typedef struct ast_node_block {
	uint32_t* p_elements;
	uint16_t elements_count;
	uint32_t parent_block;
} ast_node_block;

typedef struct ast_node_operation {
	enum ast_operation_type op_type;
	uint32_t left_argument; // 0 if unary and the other is not 0
	uint32_t right_argument; // 0 if unary and the other is not 0
	uint32_t precedence;
	bool pars_direction; // true for parsing right, false for parsing left
} ast_node_operation;

typedef struct ast_node_literal {
	enum ast_data_type data_type;
} ast_node_literal;

typedef struct ast_node_for_loop {
	uint32_t argument_block; // must be exactly 3 elements in block
	uint32_t content_block;
} ast_node_for_loop;

typedef struct ast_node_while_loop {
	uint32_t argument_block; // must be exactly 1 elements in block
	uint32_t content_block;
} ast_node_while_loop;

typedef struct ast_node_if {
	uint32_t eval_block;
	uint32_t content_block;
	uint32_t else_node; // NULL if no else
} ast_node_if;

typedef struct ast_node_else {
	uint32_t content_block_or_if; // content block of "if" node if "else if"
	uint32_t else_node; // NULL if no else
} ast_node_else;

typedef struct ast_node {
	uint32_t 		string_index;
	enum ast_node_type 	node_type;
	union {
		ast_node_null 		null;
		ast_node_undefined 	undefined;
		ast_node_function 	function;
		ast_node_variable 	variable;
		ast_node_block 		block;
		ast_node_operation 	operation;
		ast_node_literal 	literal;
		ast_node_for_loop 	for_loop;
		ast_node_while_loop while_loop;
		ast_node_if 		if_;
		ast_node_else 		else_;
	} data;
} ast_node;

typedef struct ast_ctx {
	const char* file_path;
	uint32_t root_block;
	regex_trie* regex_trie_ctx;
} ast_ctx;

typedef struct ast_regex_trie_value {
	regex_trie_value 		base;
	enum ast_node_type 		node_type;
	enum ast_data_type  	return_type;
	enum ast_operation_type	op_type; // if node type is opreation
} ast_regex_trie_value;

static CM_RES ast_regex_trie_insert(
    regex_trie* p_regex_trie,
    const char* p_regex,
    enum ast_node_type node_type,
    enum ast_data_type return_type,
    enum ast_operation_type op_type)
{
    CM_ASSERT(p_regex_trie != NULL);  // Guard trie
    CM_ASSERT(p_regex != NULL);       // Basic null check
    // If OPERATION, op_type must be valid (non-NULL)
    CM_ASSERT(node_type != AST_NODE_TYPE_OPERATION || op_type != AST_OP_TYPE_NULL);
    // If leaf (OP/LIT/VAR), return_type must be non-void (value-producing)
    CM_ASSERT(!((node_type == AST_NODE_TYPE_OPERATION) ||
                (node_type == AST_NODE_TYPE_LITERAL) ||
                (node_type == AST_NODE_TYPE_VARIABLE)) ||
              return_type != AST_DATA_TYPE_VOID);
    // If control flow (FOR/WHILE/IF/ELSE/IMPORT), return_type must be void (statement-like)
    CM_ASSERT(!((node_type == AST_NODE_TYPE_FOR_LOOP) ||
                (node_type == AST_NODE_TYPE_WHILE_LOOP) ||
                (node_type == AST_NODE_TYPE_IF) ||
                (node_type == AST_NODE_TYPE_ELSE) ||
                (node_type == AST_NODE_TYPE_IMPORT)) ||
              return_type == AST_DATA_TYPE_VOID);

    size_t regex_length = strlen(p_regex);
    ast_regex_trie_value* p_value = (ast_regex_trie_value*)regex_trie_value_create(p_regex, regex_length, sizeof(ast_regex_trie_value));
    if (p_value == NULL) {
        return CM_RES_ALLOCATION_FAILURE;  // Alloc fail
    }

    p_value->node_type = node_type;
    p_value->return_type = return_type;
    p_value->op_type = op_type;

    CM_RES insert_res = regex_trie_insert(p_regex_trie, (regex_trie_value*)p_value);
    if (CM_RES_SUCCESS != insert_res) {
        // Cleanup on failure (assume destroy fn exists)
        regex_trie_value* p_output_value = NULL;
        regex_trie_remove(p_regex_trie, p_value->base.p_regex_key, &p_output_value);
        return insert_res;
    }

    return CM_RES_SUCCESS;
}
CM_RES ast_ctx_init(const char* file_path, ast_ctx** pp_output_ast_ctx) {
    CM_ASSERT(file_path && pp_output_ast_ctx);
    CM_ASSERT(*pp_output_ast_ctx == NULL);
    ast_ctx* p_new_ctx = malloc(sizeof(ast_ctx));
    if (p_new_ctx == NULL) {
        return CM_RES_ALLOCATION_FAILURE;
    }
    p_new_ctx->file_path = file_path;
    p_new_ctx->root_block = 0; // Initial root block index (e.g., module-level)
    CM_RES trie_res = regex_trie_create(&p_new_ctx->regex_trie_ctx);
    if (CM_RES_SUCCESS != trie_res) {
        free(p_new_ctx);
        return trie_res;
    }
    // Insert token patterns into the regex trie for lexing
    // Identifiers map to UNDEFINED nodes
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "[a-zA-Z_][a-zA-Z0-9_]*", AST_NODE_TYPE_UNDEFINED, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_UNDEFINED));
    // Identifiers map to decimal number nodes
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "-?\\d+(\\.\\d+)?", AST_NODE_TYPE_LITERAL, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    // Identifiers map to whole number nodes 
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "-?\\d+", AST_NODE_TYPE_LITERAL, AST_DATA_TYPE_I32, AST_OP_TYPE_NULL));
    // Identifier map to assign operation node
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, ":", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_VOID, AST_OP_TYPE_ASSIGN));
    // Arithmetic operators
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "\\+", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_ADD));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "-", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_SUB));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "\\*", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_MUL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "\\/", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_DIV));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "\\^", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_EXP));
    // Cast operations (treated as unary operations)
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "f64", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_F64));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "f32", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_F32));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "i8", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_I8));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "i16", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_I16));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "i32", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_I32));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "i64", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_I64));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "u8", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_U8));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "u16", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_U16));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "u32", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_U32));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "u64", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_U64));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "bool", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_BOOL));
    // Logical operators
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "and", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_AND));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "or", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_OR));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "xor", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_XOR));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "imply", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_IMPLY));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "not", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_NOT));
    // Comparison operators
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "=", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_EQUAL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "!=", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_NOT_EQUAL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, ">", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_GREATER_THAN));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "<", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_LESS_THAN));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, ">=", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_GREATER_THAN_OR_EQUAL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "<=", AST_NODE_TYPE_OPERATION, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_LESS_THAN_OR_EQUAL));
    // Grouping symbols (treated as undefined for now; parser will handle grouping/blocks)
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "\\(", AST_NODE_TYPE_UNDEFINED, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_UNDEFINED));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "\\)", AST_NODE_TYPE_UNDEFINED, AST_DATA_TYPE_UNDEFINED, AST_OP_TYPE_UNDEFINED));
    // Control flow and declaration keywords
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "for", AST_NODE_TYPE_FOR_LOOP, AST_DATA_TYPE_VOID, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "while", AST_NODE_TYPE_WHILE_LOOP, AST_DATA_TYPE_VOID, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "if", AST_NODE_TYPE_IF, AST_DATA_TYPE_VOID, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "else", AST_NODE_TYPE_ELSE, AST_DATA_TYPE_VOID, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "import", AST_NODE_TYPE_IMPORT, AST_DATA_TYPE_VOID, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "var", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_VARIABLE, AST_OP_TYPE_NULL));
    // Predefined variables (axes, time, colors) - using short forms from example syntax; types inferred as F32/F64
    // Axis1 and properties
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis1", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis1\\.min", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis1\\.max", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis1\\.res", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    // Axis2 and properties
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis2", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis2\\.min", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis2\\.max", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis2\\.res", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    // Axis3 and properties
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis3", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis3\\.min", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis3\\.max", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "axis3\\.res", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    // Time properties (short forms from example; F64 for precision)
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "time\\.s", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F64, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "time\\.ms", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F64, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "time\\.us", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F64, AST_OP_TYPE_NULL));
    // Colors (F32 for 0.0-1.0 range)
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "red", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "green", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "blue", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
    CM_ASSERT(CM_RES_SUCCESS == ast_regex_trie_insert(p_new_ctx->regex_trie_ctx, "alpha", AST_NODE_TYPE_VARIABLE, AST_DATA_TYPE_F32, AST_OP_TYPE_NULL));
   
    *pp_output_ast_ctx = p_new_ctx;
    return CM_RES_SUCCESS;
}