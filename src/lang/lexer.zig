//! Lexer for tokenizing source code
//!
//! Wraps a generic RegexTrie and associates patterns with TokenTypes
//! for syntax highlighting and parsing.

const std = @import("std");
const regex_trie = @import("regex_trie.zig");
const parse_state = @import("../session/parse_state.zig");

pub const Token = parse_state.Token;
pub const TokenType = parse_state.TokenType;

/// Pattern-to-TokenType mapping entry
pub const TokenPattern = struct {
    pattern: []const u8,
    token_type: TokenType,
};

/// Lexer configuration with pattern definitions
pub const LexerConfig = struct {
    patterns: []const TokenPattern,

    /// Logos language lexer configuration
    /// Order matters: longer/more specific patterns should come first
    pub fn logosPatterns() LexerConfig {
        const patterns = [_]TokenPattern{
            // ============ Comments (highest priority) ============
            .{ .pattern = "//[^\\n]*", .token_type = .comment },
            .{ .pattern = "/\\*[\\s\\S]*?\\*/", .token_type = .comment },

            // ============ Type names (for casting) ============
            // Must come before identifiers to match first
            .{ .pattern = "f32", .token_type = .type_name },
            .{ .pattern = "f64", .token_type = .type_name },
            .{ .pattern = "i32", .token_type = .type_name },
            .{ .pattern = "i64", .token_type = .type_name },
            .{ .pattern = "bool", .token_type = .type_name },
            .{ .pattern = "vec2", .token_type = .type_name },
            .{ .pattern = "vec3", .token_type = .type_name },
            .{ .pattern = "vec4", .token_type = .type_name },

            // ============ Keywords ============
            .{ .pattern = "if", .token_type = .keyword },
            .{ .pattern = "else", .token_type = .keyword },
            .{ .pattern = "for", .token_type = .keyword },
            .{ .pattern = "while", .token_type = .keyword },
            .{ .pattern = "and", .token_type = .keyword },
            .{ .pattern = "or", .token_type = .keyword },
            .{ .pattern = "true", .token_type = .keyword },
            .{ .pattern = "false", .token_type = .keyword },

            // ============ Built-in functions ============
            .{ .pattern = "sin", .token_type = .builtin },
            .{ .pattern = "cos", .token_type = .builtin },
            .{ .pattern = "tan", .token_type = .builtin },
            .{ .pattern = "asin", .token_type = .builtin },
            .{ .pattern = "acos", .token_type = .builtin },
            .{ .pattern = "atan", .token_type = .builtin },
            .{ .pattern = "sinh", .token_type = .builtin },
            .{ .pattern = "cosh", .token_type = .builtin },
            .{ .pattern = "tanh", .token_type = .builtin },
            .{ .pattern = "log", .token_type = .builtin },
            .{ .pattern = "log2", .token_type = .builtin },
            .{ .pattern = "log10", .token_type = .builtin },
            .{ .pattern = "exp", .token_type = .builtin },
            .{ .pattern = "exp2", .token_type = .builtin },
            .{ .pattern = "sqrt", .token_type = .builtin },
            .{ .pattern = "pow", .token_type = .builtin },
            .{ .pattern = "abs", .token_type = .builtin },
            .{ .pattern = "sign", .token_type = .builtin },
            .{ .pattern = "floor", .token_type = .builtin },
            .{ .pattern = "ceil", .token_type = .builtin },
            .{ .pattern = "round", .token_type = .builtin },
            .{ .pattern = "fract", .token_type = .builtin },
            .{ .pattern = "mod", .token_type = .builtin },
            .{ .pattern = "min", .token_type = .builtin },
            .{ .pattern = "max", .token_type = .builtin },
            .{ .pattern = "clamp", .token_type = .builtin },
            .{ .pattern = "mix", .token_type = .builtin },
            .{ .pattern = "step", .token_type = .builtin },
            .{ .pattern = "smoothstep", .token_type = .builtin },
            .{ .pattern = "len", .token_type = .builtin },
            .{ .pattern = "length", .token_type = .builtin },
            .{ .pattern = "normalize", .token_type = .builtin },
            .{ .pattern = "dot", .token_type = .builtin },
            .{ .pattern = "cross", .token_type = .builtin },

            // ============ Reserved axis/output variables ============
            // These get special highlighting
            .{ .pattern = "axis1", .token_type = .axis },
            .{ .pattern = "axis2", .token_type = .axis },
            .{ .pattern = "axis3", .token_type = .axis },
            .{ .pattern = "time", .token_type = .axis },
            .{ .pattern = "red", .token_type = .axis },
            .{ .pattern = "green", .token_type = .axis },
            .{ .pattern = "blue", .token_type = .axis },
            .{ .pattern = "alpha", .token_type = .axis },

            // ============ Numbers ============
            // Scientific notation
            .{ .pattern = "[0-9]+\\.?[0-9]*[eE][+-]?[0-9]+", .token_type = .number },
            // Float with decimal
            .{ .pattern = "[0-9]+\\.[0-9]+", .token_type = .number },
            // Integer
            .{ .pattern = "[0-9]+", .token_type = .number },

            // ============ Identifiers ============
            // Must come after keywords/builtins to allow them to match first
            .{ .pattern = "[a-zA-Z_][a-zA-Z0-9_]*", .token_type = .identifier },

            // ============ Multi-character operators ============
            .{ .pattern = "!=", .token_type = .operator },
            .{ .pattern = "<=", .token_type = .operator },
            .{ .pattern = ">=", .token_type = .operator },

            // ============ Single-character operators ============
            .{ .pattern = "\\+", .token_type = .operator },
            .{ .pattern = "-", .token_type = .operator },
            .{ .pattern = "\\*", .token_type = .operator },
            .{ .pattern = "/", .token_type = .operator },
            .{ .pattern = "\\^", .token_type = .operator },
            .{ .pattern = "<", .token_type = .operator },
            .{ .pattern = ">", .token_type = .operator },
            .{ .pattern = "=", .token_type = .operator },
            .{ .pattern = "!", .token_type = .operator },

            // Unicode superscript for square (²)
            .{ .pattern = "\xc2\xb2", .token_type = .operator }, // UTF-8 encoding of ²

            // ============ Punctuation ============
            .{ .pattern = "\\(", .token_type = .punctuation },
            .{ .pattern = "\\)", .token_type = .punctuation },
            .{ .pattern = "\\[", .token_type = .punctuation },
            .{ .pattern = "\\]", .token_type = .punctuation },
            .{ .pattern = ",", .token_type = .punctuation },
            .{ .pattern = ":", .token_type = .punctuation },
            .{ .pattern = "\\.", .token_type = .punctuation },

            // ============ Whitespace ============
            .{ .pattern = "[ \\t\\n\\r]+", .token_type = .whitespace },
        };
        return .{ .patterns = &patterns };
    }

    /// Legacy/default lexer configuration (generic)
    pub fn syntaxTokenPatterns() LexerConfig {
        return logosPatterns();
    }
};

/// Lexer wrapping a RegexTrie
pub const Lexer = struct {
    trie: *regex_trie.RegexTrie,
    /// Token type storage (owned by lexer, pointed to by RegexTrieValue.data)
    token_types: std.ArrayList(TokenType),
    allocator: std.mem.Allocator,

    /// Initialize a lexer with the given configuration
    pub fn init(allocator: std.mem.Allocator, config: LexerConfig) !Lexer {
        var trie = try regex_trie.RegexTrie.init(allocator);
        errdefer trie.deinit();

        var token_types = try std.ArrayList(TokenType).initCapacity(allocator, config.patterns.len);
        errdefer token_types.deinit(allocator);

        // Insert all patterns into the trie with TokenType as data
        for (config.patterns) |pattern_entry| {
            // Store the token type in our list
            try token_types.append(allocator, pattern_entry.token_type);
            const token_type_ptr = &token_types.items[token_types.items.len - 1];

            // Insert with pointer to the token type as data
            try trie.insert(allocator, pattern_entry.pattern, @ptrCast(token_type_ptr));
        }

        return .{
            .trie = trie,
            .token_types = token_types,
            .allocator = allocator,
        };
    }

    /// Deinitialize the lexer
    pub fn deinit(self: *Lexer) void {
        self.trie.deinit();
        self.token_types.deinit(self.allocator);
    }

    /// Tokenize input text into a token array
    /// Caller owns the returned slice and must free it
    pub fn tokenize(self: *Lexer, content: []const u8) ![]Token {
        var tokens = try std.ArrayList(Token).initCapacity(self.allocator, 0);
        errdefer tokens.deinit(self.allocator);

        var pos: usize = 0;
        while (pos < content.len) {
            const remaining = content[pos..];

            // Try to match a token
            const match_result = self.trie.get(remaining) catch |err| {
                if (err == regex_trie.RegexTrieError.NodeNotFound) {
                    // No match - treat as unknown single character
                    try tokens.append(self.allocator, .{
                        .text = content[pos .. pos + 1],
                        .token_type = .unknown,
                        .byte_start = pos,
                        .byte_end = pos + 1,
                    });
                    pos += 1;
                    continue;
                }
                return err;
            };

            // Extract TokenType from the data field
            const token_type = if (match_result.data) |data_ptr|
                @as(*TokenType, @ptrCast(@alignCast(data_ptr))).*
            else
                .unknown;

            const matched_len = match_result.matched;

            try tokens.append(self.allocator, .{
                .text = content[pos .. pos + matched_len],
                .token_type = token_type,
                .byte_start = pos,
                .byte_end = pos + matched_len,
            });

            pos += matched_len;
        }

        return try tokens.toOwnedSlice(self.allocator);
    }

    /// Check if content needs re-lexing by comparing with cached hash
    pub fn needsRelex(state: *parse_state.ParseState, content: []const u8) bool {
        return state.shouldRelex(content);
    }
};
