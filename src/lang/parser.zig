//! Recursive descent parser for the Logos language
//!
//! Parses a token stream into an AST. Supports:
//! - Bindings: `name: expr` or `(a, b, c): expr`
//! - Function definitions: `name(params): (body)`
//! - Control flow: if/else, while, for
//! - Expressions: binary ops, unary ops, function calls, property access
//! - Literals: numbers, identifiers, booleans, tuples

const std = @import("std");
const parse_state = @import("../session/parse_state.zig");
const ast = @import("ast_node.zig");

pub const Token = parse_state.Token;
pub const TokenType = parse_state.TokenType;
pub const ParseError = parse_state.ParseError;
pub const AstNode = ast.AstNode;
pub const BinaryOp = ast.BinaryOp;
pub const UnaryOp = ast.UnaryOp;
pub const PrimitiveType = ast.PrimitiveType;
pub const BindingPattern = ast.BindingPattern;
pub const SourceSpan = ast.SourceSpan;

/// Error type for parsing operations
pub const Error = error{
    ParseError,
    OutOfMemory,
};

/// Parser for the Logos language
pub const Parser = struct {
    tokens: []const Token,
    pos: usize,
    allocator: std.mem.Allocator,
    errors: std.ArrayList(ParseError),

    /// Initialize a new parser
    pub fn init(allocator: std.mem.Allocator, tokens: []const Token) Parser {
        return .{
            .tokens = tokens,
            .pos = 0,
            .allocator = allocator,
            .errors = .{ .items = &.{}, .capacity = 0 },
        };
    }

    /// Deinitialize the parser
    pub fn deinit(self: *Parser) void {
        self.errors.deinit(self.allocator);
    }

    /// Parse all tokens into a block of statements
    pub fn parse(self: *Parser) Error!*AstNode {
        var statements: std.ArrayList(*AstNode) = .{ .items = &.{}, .capacity = 0 };
        errdefer {
            for (statements.items) |stmt| {
                self.allocator.destroy(stmt);
            }
            statements.deinit(self.allocator);
        }

        while (!self.isAtEnd()) {
            // Skip whitespace and comments at top level
            self.skipWhitespaceAndComments();
            if (self.isAtEnd()) break;

            const stmt = try self.parseStatement();
            try statements.append(self.allocator, stmt);

            // Skip optional comma separator between top-level statements
            self.skipWhitespaceAndComments();
            if (self.checkText(",")) {
                _ = self.advance();
            }
        }

        const span = if (statements.items.len > 0)
            statements.items[0].span.merge(statements.items[statements.items.len - 1].span)
        else
            SourceSpan{ .start = 0, .end = 0 };

        return AstNode.blockNode(self.allocator, try statements.toOwnedSlice(self.allocator), span);
    }

    /// Parse a single statement (binding, expression, or control flow)
    fn parseStatement(self: *Parser) Error!*AstNode {
        self.skipWhitespaceAndComments();

        // Check for keywords first
        if (self.checkText("if")) {
            return self.parseIf();
        }
        if (self.checkText("while")) {
            return self.parseWhile();
        }
        if (self.checkText("for")) {
            return self.parseFor();
        }

        // Try to parse as binding (identifier/tuple followed by colon)
        // But also need to handle function definitions: name(params): body
        const checkpoint = self.pos;

        // Check for tuple pattern: (a, b, c):
        if (self.checkText("(")) {
            const maybe_binding = self.tryParseBindingPattern();
            if (maybe_binding) |pattern| {
                self.skipWhitespaceAndComments();
                if (self.checkText(":")) {
                    _ = self.advance(); // consume ':'
                    self.skipWhitespaceAndComments();
                    const value = try self.parseExpression();
                    return AstNode.bindingNode(self.allocator, pattern, value, SourceSpan{
                        .start = checkpoint,
                        .end = value.span.end,
                    });
                }
            }
            // Restore and parse as expression
            self.pos = checkpoint;
        }

        // Check for identifier binding or function definition
        if (self.check(.identifier) or self.check(.axis)) {
            const name_token = self.peek() orelse return self.unexpectedEndError();
            _ = self.advance();
            self.skipWhitespaceAndComments();

            // Function definition: name(params): body
            if (self.checkText("(")) {
                const params_result = self.tryParseParamList();
                if (params_result) |params| {
                    self.skipWhitespaceAndComments();
                    if (self.checkText(":")) {
                        _ = self.advance(); // consume ':'
                        self.skipWhitespaceAndComments();
                        const body = try self.parseExpression();
                        return AstNode.functionDef(self.allocator, name_token.text, params, body, SourceSpan{
                            .start = name_token.byte_start,
                            .end = body.span.end,
                        });
                    }
                }
                // Not a function def, restore and continue
                self.pos = checkpoint;
            } else if (self.checkText(":")) {
                // Simple binding: name: value
                _ = self.advance(); // consume ':'
                self.skipWhitespaceAndComments();
                const value = try self.parseExpression();
                return AstNode.bindingNode(self.allocator, .{ .single = name_token.text }, value, SourceSpan{
                    .start = name_token.byte_start,
                    .end = value.span.end,
                });
            } else {
                // Not a binding, restore and parse as expression
                self.pos = checkpoint;
            }
        }

        // Otherwise, parse as expression
        return self.parseExpression();
    }

    /// Parse an expression (handles operator precedence)
    fn parseExpression(self: *Parser) Error!*AstNode {
        return self.parseOr();
    }

    /// Parse logical OR: a or b
    fn parseOr(self: *Parser) Error!*AstNode {
        var left = try self.parseAnd();

        while (true) {
            self.skipWhitespaceAndComments();
            if (self.checkText("or")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const right = try self.parseAnd();
                left = try AstNode.binaryOp(self.allocator, .@"or", left, right);
            } else {
                break;
            }
        }
        return left;
    }

    /// Parse logical AND: a and b
    fn parseAnd(self: *Parser) Error!*AstNode {
        var left = try self.parseComparison();

        while (true) {
            self.skipWhitespaceAndComments();
            if (self.checkText("and")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const right = try self.parseComparison();
                left = try AstNode.binaryOp(self.allocator, .@"and", left, right);
            } else {
                break;
            }
        }
        return left;
    }

    /// Parse comparisons: =, !=, <, >, <=, >=
    fn parseComparison(self: *Parser) Error!*AstNode {
        var left = try self.parseAddSub();

        while (true) {
            self.skipWhitespaceAndComments();
            const op = self.matchComparisonOp() orelse break;
            self.skipWhitespaceAndComments();
            const right = try self.parseAddSub();
            left = try AstNode.binaryOp(self.allocator, op, left, right);
        }
        return left;
    }

    /// Parse addition and subtraction: a + b, a - b
    fn parseAddSub(self: *Parser) Error!*AstNode {
        var left = try self.parseMulDiv();

        while (true) {
            self.skipWhitespaceAndComments();
            if (self.checkText("+")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const right = try self.parseMulDiv();
                left = try AstNode.binaryOp(self.allocator, .add, left, right);
            } else if (self.checkText("-")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const right = try self.parseMulDiv();
                left = try AstNode.binaryOp(self.allocator, .sub, left, right);
            } else {
                break;
            }
        }
        return left;
    }

    /// Parse multiplication and division: a * b, a / b
    fn parseMulDiv(self: *Parser) Error!*AstNode {
        var left = try self.parsePower();

        while (true) {
            self.skipWhitespaceAndComments();
            if (self.checkText("*")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const right = try self.parsePower();
                left = try AstNode.binaryOp(self.allocator, .mul, left, right);
            } else if (self.checkText("/")) {
                // Check it's not a comment
                if (self.pos + 1 < self.tokens.len) {
                    const next = self.tokens[self.pos + 1];
                    if (std.mem.startsWith(u8, next.text, "/") or std.mem.startsWith(u8, next.text, "*")) {
                        break; // It's a comment, stop
                    }
                }
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const right = try self.parsePower();
                left = try AstNode.binaryOp(self.allocator, .div, left, right);
            } else {
                break;
            }
        }
        return left;
    }

    /// Parse power/exponentiation: a ^ b (right associative)
    fn parsePower(self: *Parser) Error!*AstNode {
        const base = try self.parseUnary();

        self.skipWhitespaceAndComments();
        if (self.checkText("^")) {
            _ = self.advance();
            self.skipWhitespaceAndComments();
            const exp = try self.parsePower(); // Right associative
            return AstNode.binaryOp(self.allocator, .pow, base, exp);
        }
        return base;
    }

    /// Parse unary operators: -x, !x
    fn parseUnary(self: *Parser) Error!*AstNode {
        self.skipWhitespaceAndComments();

        if (self.checkText("-")) {
            const op_token = self.advance().?;
            self.skipWhitespaceAndComments();
            const operand = try self.parseUnary();
            return AstNode.unaryOp(self.allocator, .neg, operand, SourceSpan{
                .start = op_token.byte_start,
                .end = operand.span.end,
            });
        }
        if (self.checkText("!")) {
            const op_token = self.advance().?;
            self.skipWhitespaceAndComments();
            const operand = try self.parseUnary();
            return AstNode.unaryOp(self.allocator, .not, operand, SourceSpan{
                .start = op_token.byte_start,
                .end = operand.span.end,
            });
        }

        return self.parsePostfix();
    }

    /// Parse postfix operators: x², x.prop, x[i], x(args)
    fn parsePostfix(self: *Parser) Error!*AstNode {
        var node = try self.parsePrimary();

        while (true) {
            self.skipWhitespaceAndComments();

            // Square operator: x²
            if (self.checkText("²") or self.checkText("\xc2\xb2")) {
                const op_token = self.advance().?;
                node = try AstNode.unaryOp(self.allocator, .square, node, SourceSpan{
                    .start = node.span.start,
                    .end = op_token.byte_end,
                });
            }
            // Property access: x.prop
            else if (self.checkText(".")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                if (self.check(.identifier) or self.check(.axis)) {
                    const prop_token = self.advance().?;
                    node = try AstNode.propertyAccess(self.allocator, node, prop_token.text, SourceSpan{
                        .start = node.span.start,
                        .end = prop_token.byte_end,
                    });
                } else {
                    return self.addError("Expected property name after '.'");
                }
            }
            // Index access: x[i]
            else if (self.checkText("[")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const index = try self.parseExpression();
                self.skipWhitespaceAndComments();
                const close = try self.expectText("]");
                const idx_node = try self.allocator.create(AstNode);
                idx_node.* = .{
                    .data = .{ .index = .{ .base = node, .index_expr = index } },
                    .span = SourceSpan{ .start = node.span.start, .end = close.byte_end },
                };
                node = idx_node;
            }
            // Function call: x(args) - but only if node is an identifier or type name
            else if (self.checkText("(")) {
                // Only parse as function call if base is an identifier, builtin, or type
                const is_callable = switch (node.data) {
                    .identifier => true,
                    else => false,
                };
                if (!is_callable) break;

                _ = self.advance(); // consume '('
                self.skipWhitespaceAndComments();

                var args: std.ArrayList(*AstNode) = .{ .items = &.{}, .capacity = 0 };
                errdefer args.deinit(self.allocator);

                // Parse arguments
                if (!self.checkText(")")) {
                    const first_arg = try self.parseExpression();
                    try args.append(self.allocator, first_arg);

                    while (true) {
                        self.skipWhitespaceAndComments();
                        if (self.checkText(",")) {
                            _ = self.advance();
                            self.skipWhitespaceAndComments();
                            const arg = try self.parseExpression();
                            try args.append(self.allocator, arg);
                        } else {
                            break;
                        }
                    }
                }

                self.skipWhitespaceAndComments();
                const close = try self.expectText(")");
                node = try AstNode.functionCall(self.allocator, node, try args.toOwnedSlice(self.allocator), SourceSpan{
                    .start = node.span.start,
                    .end = close.byte_end,
                });
            } else {
                break;
            }
        }

        return node;
    }

    /// Parse primary expressions: literals, identifiers, tuples, casts
    fn parsePrimary(self: *Parser) Error!*AstNode {
        self.skipWhitespaceAndComments();

        const token = self.peek() orelse return self.unexpectedEndError();

        // Number literal
        if (token.token_type == .number) {
            _ = self.advance();
            const value = std.fmt.parseFloat(f64, token.text) catch 0.0;
            return AstNode.number(self.allocator, value, SourceSpan{
                .start = token.byte_start,
                .end = token.byte_end,
            });
        }

        // Boolean literals
        if (std.mem.eql(u8, token.text, "true")) {
            _ = self.advance();
            return AstNode.boolLit(self.allocator, true, SourceSpan{
                .start = token.byte_start,
                .end = token.byte_end,
            });
        }
        if (std.mem.eql(u8, token.text, "false")) {
            _ = self.advance();
            return AstNode.boolLit(self.allocator, false, SourceSpan{
                .start = token.byte_start,
                .end = token.byte_end,
            });
        }

        // Type cast: f32(x), i32(x), etc.
        if (token.token_type == .type_name) {
            const type_token = self.advance().?;
            self.skipWhitespaceAndComments();

            if (self.checkText("(")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                const operand = try self.parseExpression();
                self.skipWhitespaceAndComments();
                const close = try self.expectText(")");

                const target_type = stringToPrimitiveType(type_token.text) orelse .f32;
                return AstNode.castExpr(self.allocator, target_type, operand, SourceSpan{
                    .start = type_token.byte_start,
                    .end = close.byte_end,
                });
            } else {
                // Just an identifier that happens to be a type name
                return AstNode.identifier(self.allocator, type_token.text, SourceSpan{
                    .start = type_token.byte_start,
                    .end = type_token.byte_end,
                });
            }
        }

        // Identifier (including axis, builtin)
        if (token.token_type == .identifier or token.token_type == .axis or token.token_type == .builtin) {
            _ = self.advance();
            return AstNode.identifier(self.allocator, token.text, SourceSpan{
                .start = token.byte_start,
                .end = token.byte_end,
            });
        }

        // Parenthesized expression or tuple
        if (std.mem.eql(u8, token.text, "(")) {
            return self.parseParenOrTuple();
        }

        // Unknown token
        return self.unexpectedTokenError();
    }

    /// Parse parenthesized expression or tuple
    fn parseParenOrTuple(self: *Parser) Error!*AstNode {
        const open = self.advance().?; // consume '('
        self.skipWhitespaceAndComments();

        // Empty tuple
        if (self.checkText(")")) {
            const close = self.advance().?;
            const empty_tuple: []*AstNode = &.{};
            return AstNode.tupleLit(self.allocator, empty_tuple, SourceSpan{
                .start = open.byte_start,
                .end = close.byte_end,
            });
        }

        // Parse first expression
        const first = try self.parseExpression();
        self.skipWhitespaceAndComments();

        // If there's a comma, this is a tuple
        if (self.checkText(",")) {
            var elements: std.ArrayList(*AstNode) = .{ .items = &.{}, .capacity = 0 };
            errdefer elements.deinit(self.allocator);
            try elements.append(self.allocator, first);

            while (self.checkText(",")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();

                // Allow trailing comma before )
                if (self.checkText(")")) break;

                const elem = try self.parseExpression();
                try elements.append(self.allocator, elem);
                self.skipWhitespaceAndComments();
            }

            const close = try self.expectText(")");
            return AstNode.tupleLit(self.allocator, try elements.toOwnedSlice(self.allocator), SourceSpan{
                .start = open.byte_start,
                .end = close.byte_end,
            });
        }

        // Just a parenthesized expression
        _ = try self.expectText(")");
        return first;
    }

    /// Parse if expression
    fn parseIf(self: *Parser) Error!*AstNode {
        const if_token = self.advance().?; // consume 'if'
        self.skipWhitespaceAndComments();

        _ = try self.expectText("(");
        self.skipWhitespaceAndComments();
        const condition = try self.parseExpression();
        self.skipWhitespaceAndComments();
        _ = try self.expectText(")");

        self.skipWhitespaceAndComments();
        const then_branch = try self.parseExpression();

        self.skipWhitespaceAndComments();
        var else_branch: ?*AstNode = null;
        var end_pos = then_branch.span.end;

        if (self.checkText("else")) {
            _ = self.advance();
            self.skipWhitespaceAndComments();
            else_branch = try self.parseExpression();
            end_pos = else_branch.?.span.end;
        }

        return AstNode.ifExpr(self.allocator, condition, then_branch, else_branch, SourceSpan{
            .start = if_token.byte_start,
            .end = end_pos,
        });
    }

    /// Parse while loop
    fn parseWhile(self: *Parser) Error!*AstNode {
        const while_token = self.advance().?; // consume 'while'
        self.skipWhitespaceAndComments();

        _ = try self.expectText("(");
        self.skipWhitespaceAndComments();
        const condition = try self.parseExpression();
        self.skipWhitespaceAndComments();
        _ = try self.expectText(")");

        self.skipWhitespaceAndComments();
        const body = try self.parseExpression();

        return AstNode.whileLoop(self.allocator, condition, body, SourceSpan{
            .start = while_token.byte_start,
            .end = body.span.end,
        });
    }

    /// Parse for loop
    fn parseFor(self: *Parser) Error!*AstNode {
        const for_token = self.advance().?; // consume 'for'
        self.skipWhitespaceAndComments();

        _ = try self.expectText("(");
        self.skipWhitespaceAndComments();
        const init_stmt = try self.parseStatement();
        self.skipWhitespaceAndComments();
        _ = try self.expectText(",");
        self.skipWhitespaceAndComments();
        const condition = try self.parseExpression();
        self.skipWhitespaceAndComments();
        _ = try self.expectText(",");
        self.skipWhitespaceAndComments();
        const update = try self.parseStatement();
        self.skipWhitespaceAndComments();
        _ = try self.expectText(")");

        self.skipWhitespaceAndComments();
        const body = try self.parseExpression();

        return AstNode.forLoop(self.allocator, init_stmt, condition, update, body, SourceSpan{
            .start = for_token.byte_start,
            .end = body.span.end,
        });
    }

    // ============ Helper Methods ============

    fn skipWhitespaceAndComments(self: *Parser) void {
        while (self.pos < self.tokens.len) {
            const tok = self.tokens[self.pos];
            if (tok.token_type == .whitespace or tok.token_type == .comment) {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn isAtEnd(self: *Parser) bool {
        return self.pos >= self.tokens.len;
    }

    fn peek(self: *Parser) ?Token {
        if (self.pos >= self.tokens.len) return null;
        return self.tokens[self.pos];
    }

    fn advance(self: *Parser) ?Token {
        if (self.pos >= self.tokens.len) return null;
        const tok = self.tokens[self.pos];
        self.pos += 1;
        return tok;
    }

    fn check(self: *Parser, token_type: TokenType) bool {
        if (self.pos >= self.tokens.len) return false;
        return self.tokens[self.pos].token_type == token_type;
    }

    fn checkText(self: *Parser, text: []const u8) bool {
        if (self.pos >= self.tokens.len) return false;
        return std.mem.eql(u8, self.tokens[self.pos].text, text);
    }

    fn expectText(self: *Parser, text: []const u8) Error!Token {
        if (self.pos >= self.tokens.len) {
            // Choose descriptive message based on what we expected
            if (std.mem.eql(u8, text, ")")) {
                return self.addError("Missing closing ')'");
            } else if (std.mem.eql(u8, text, "(")) {
                return self.addError("Missing opening '('");
            } else if (std.mem.eql(u8, text, "]")) {
                return self.addError("Missing closing ']'");
            } else if (std.mem.eql(u8, text, ":")) {
                return self.addError("Missing ':' in binding");
            } else if (std.mem.eql(u8, text, ",")) {
                return self.addError("Missing ','");
            }
            return self.unexpectedEndError();
        }
        if (!std.mem.eql(u8, self.tokens[self.pos].text, text)) {
            // Choose descriptive message based on what we expected
            if (std.mem.eql(u8, text, ")")) {
                return self.addError("Expected ')' to close expression");
            } else if (std.mem.eql(u8, text, "(")) {
                return self.addError("Expected '(' for function call or grouping");
            } else if (std.mem.eql(u8, text, "]")) {
                return self.addError("Expected ']' to close index");
            } else if (std.mem.eql(u8, text, ":")) {
                return self.addError("Expected ':' after name in binding");
            } else if (std.mem.eql(u8, text, ",")) {
                return self.addError("Expected ',' between arguments");
            }
            return self.addExpectedError(text);
        }
        return self.advance().?;
    }

    fn matchComparisonOp(self: *Parser) ?BinaryOp {
        if (self.pos >= self.tokens.len) return null;
        const text = self.tokens[self.pos].text;

        const op: ?BinaryOp = if (std.mem.eql(u8, text, "!="))
            .neq
        else if (std.mem.eql(u8, text, "<="))
            .lte
        else if (std.mem.eql(u8, text, ">="))
            .gte
        else if (std.mem.eql(u8, text, "="))
            .eq
        else if (std.mem.eql(u8, text, "<"))
            .lt
        else if (std.mem.eql(u8, text, ">"))
            .gt
        else
            null;

        if (op != null) {
            self.pos += 1;
        }
        return op;
    }

    fn tryParseBindingPattern(self: *Parser) ?BindingPattern {
        const start = self.pos;

        if (!self.checkText("(")) return null;
        _ = self.advance();
        self.skipWhitespaceAndComments();

        var names: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
        defer names.deinit(self.allocator);

        // First name
        if (self.check(.identifier) or self.check(.axis)) {
            const name_tok = self.advance().?;
            names.append(self.allocator, name_tok.text) catch {
                self.pos = start;
                return null;
            };
        } else {
            self.pos = start;
            return null;
        }

        // More names
        while (true) {
            self.skipWhitespaceAndComments();
            if (self.checkText(",")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                if (self.check(.identifier) or self.check(.axis)) {
                    const name_tok = self.advance().?;
                    names.append(self.allocator, name_tok.text) catch {
                        self.pos = start;
                        return null;
                    };
                } else {
                    self.pos = start;
                    return null;
                }
            } else {
                break;
            }
        }

        self.skipWhitespaceAndComments();
        if (!self.checkText(")")) {
            self.pos = start;
            return null;
        }
        _ = self.advance();

        return .{ .tuple = names.toOwnedSlice(self.allocator) catch {
            self.pos = start;
            return null;
        } };
    }

    fn tryParseParamList(self: *Parser) ?[]const []const u8 {
        const start = self.pos;

        if (!self.checkText("(")) return null;
        _ = self.advance();
        self.skipWhitespaceAndComments();

        var params: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
        defer params.deinit(self.allocator);

        // Empty param list
        if (self.checkText(")")) {
            _ = self.advance();
            return params.toOwnedSlice(self.allocator) catch {
                self.pos = start;
                return null;
            };
        }

        // First param
        if (self.check(.identifier)) {
            const name_tok = self.advance().?;
            params.append(self.allocator, name_tok.text) catch {
                self.pos = start;
                return null;
            };
        } else {
            self.pos = start;
            return null;
        }

        // More params
        while (true) {
            self.skipWhitespaceAndComments();
            if (self.checkText(",")) {
                _ = self.advance();
                self.skipWhitespaceAndComments();
                if (self.check(.identifier)) {
                    const name_tok = self.advance().?;
                    params.append(self.allocator, name_tok.text) catch {
                        self.pos = start;
                        return null;
                    };
                } else {
                    self.pos = start;
                    return null;
                }
            } else {
                break;
            }
        }

        self.skipWhitespaceAndComments();
        if (!self.checkText(")")) {
            self.pos = start;
            return null;
        }
        _ = self.advance();

        return params.toOwnedSlice(self.allocator) catch {
            self.pos = start;
            return null;
        };
    }

    fn addError(self: *Parser, message: []const u8) Error {
        const token = self.peek() orelse Token{
            .text = "",
            .token_type = .unknown,
            .byte_start = if (self.tokens.len > 0) self.tokens[self.tokens.len - 1].byte_end else 0,
            .byte_end = if (self.tokens.len > 0) self.tokens[self.tokens.len - 1].byte_end else 0,
        };
        self.errors.append(self.allocator, .{
            .byte_start = token.byte_start,
            .byte_end = token.byte_end,
            .message = message,
            .severity = .err,
        }) catch {};
        return error.ParseError;
    }

    /// Add error for expected token - uses simple static messages for reliability
    fn addExpectedError(self: *Parser, expected: []const u8) Error {
        const token = self.peek();
        // Use the expected text directly since dynamic formatting has lifetime issues
        // The UI can show the token at the error location for context
        _ = expected;
        if (token) |tok| {
            // Choose a static message based on what we got
            const msg = switch (tok.token_type) {
                .punctuation => "Expected different punctuation",
                .operator => "Unexpected operator",
                .number => "Unexpected number",
                .identifier => "Unexpected identifier",
                .keyword => "Unexpected keyword",
                .whitespace => "Unexpected whitespace",
                else => "Syntax error",
            };
            return self.addError(msg);
        }
        return self.addError("Unexpected end of input");
    }

    fn unexpectedEndError(self: *Parser) Error {
        return self.addError("Unexpected end of input");
    }

    fn unexpectedTokenError(self: *Parser) Error {
        const token = self.peek();
        if (token) |tok| {
            // Use static messages based on token type for reliability
            const msg = switch (tok.token_type) {
                .unknown => "Unknown/invalid character",
                .punctuation => "Unexpected punctuation",
                .operator => "Unexpected operator here",
                .number => "Unexpected number",
                .identifier => "Unexpected identifier",
                .keyword => "Unexpected keyword",
                .comment => "Unexpected comment",
                .whitespace => "Unexpected whitespace",
                else => "Unexpected token",
            };
            return self.addError(msg);
        }
        return self.addError("Unexpected token");
    }
};

/// Convert type name string to PrimitiveType
fn stringToPrimitiveType(text: []const u8) ?PrimitiveType {
    if (std.mem.eql(u8, text, "f32")) return .f32;
    if (std.mem.eql(u8, text, "f64")) return .f64;
    if (std.mem.eql(u8, text, "i32")) return .i32;
    if (std.mem.eql(u8, text, "i64")) return .i64;
    if (std.mem.eql(u8, text, "bool")) return .bool;
    if (std.mem.eql(u8, text, "vec2")) return .vec2;
    if (std.mem.eql(u8, text, "vec3")) return .vec3;
    if (std.mem.eql(u8, text, "vec4")) return .vec4;
    return null;
}

/// Convenience function to parse tokens and return result
pub fn parseTokens(allocator: std.mem.Allocator, tokens: []const Token) !struct { ast: *AstNode, errors: []ParseError } {
    var parser = Parser.init(allocator, tokens);
    defer parser.deinit();

    const ast_result = parser.parse() catch |err| {
        if (err == error.ParseError) {
            return .{
                .ast = undefined,
                .errors = try parser.errors.toOwnedSlice(allocator),
            };
        }
        return err;
    };

    return .{
        .ast = ast_result,
        .errors = try parser.errors.toOwnedSlice(allocator),
    };
}

