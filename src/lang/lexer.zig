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
    /// Keywords/types/builtins use (?![a-zA-Z0-9_]) negative lookahead for word boundaries
    pub fn logosPatterns() LexerConfig {
        // Word boundary: ensures keyword doesn't match if followed by identifier char
        const wb = "(?![a-zA-Z0-9_])";

        const patterns = [_]TokenPattern{
            // ============ Comments (highest priority) ============
            .{ .pattern = "//[^\\n]*", .token_type = .comment },
            .{ .pattern = "/\\*[\\s\\S]*?\\*/", .token_type = .comment },

            // ============ Type names (for casting) ============
            .{ .pattern = "f32" ++ wb, .token_type = .type_name },
            .{ .pattern = "f64" ++ wb, .token_type = .type_name },
            .{ .pattern = "i32" ++ wb, .token_type = .type_name },
            .{ .pattern = "i64" ++ wb, .token_type = .type_name },
            .{ .pattern = "bool" ++ wb, .token_type = .type_name },
            .{ .pattern = "vec2" ++ wb, .token_type = .type_name },
            .{ .pattern = "vec3" ++ wb, .token_type = .type_name },
            .{ .pattern = "vec4" ++ wb, .token_type = .type_name },

            // ============ Keywords ============
            .{ .pattern = "if" ++ wb, .token_type = .keyword },
            .{ .pattern = "else" ++ wb, .token_type = .keyword },
            .{ .pattern = "for" ++ wb, .token_type = .keyword },
            .{ .pattern = "while" ++ wb, .token_type = .keyword },
            .{ .pattern = "and" ++ wb, .token_type = .keyword },
            .{ .pattern = "or" ++ wb, .token_type = .keyword },
            .{ .pattern = "true" ++ wb, .token_type = .keyword },
            .{ .pattern = "false" ++ wb, .token_type = .keyword },

            // ============ Built-in functions ============
            .{ .pattern = "sin" ++ wb, .token_type = .builtin },
            .{ .pattern = "cos" ++ wb, .token_type = .builtin },
            .{ .pattern = "tan" ++ wb, .token_type = .builtin },
            .{ .pattern = "asin" ++ wb, .token_type = .builtin },
            .{ .pattern = "acos" ++ wb, .token_type = .builtin },
            .{ .pattern = "atan" ++ wb, .token_type = .builtin },
            .{ .pattern = "sinh" ++ wb, .token_type = .builtin },
            .{ .pattern = "cosh" ++ wb, .token_type = .builtin },
            .{ .pattern = "tanh" ++ wb, .token_type = .builtin },
            .{ .pattern = "log" ++ wb, .token_type = .builtin },
            .{ .pattern = "log2" ++ wb, .token_type = .builtin },
            .{ .pattern = "log10" ++ wb, .token_type = .builtin },
            .{ .pattern = "exp" ++ wb, .token_type = .builtin },
            .{ .pattern = "exp2" ++ wb, .token_type = .builtin },
            .{ .pattern = "sqrt" ++ wb, .token_type = .builtin },
            .{ .pattern = "pow" ++ wb, .token_type = .builtin },
            .{ .pattern = "abs" ++ wb, .token_type = .builtin },
            .{ .pattern = "sign" ++ wb, .token_type = .builtin },
            .{ .pattern = "floor" ++ wb, .token_type = .builtin },
            .{ .pattern = "ceil" ++ wb, .token_type = .builtin },
            .{ .pattern = "round" ++ wb, .token_type = .builtin },
            .{ .pattern = "fract" ++ wb, .token_type = .builtin },
            .{ .pattern = "mod" ++ wb, .token_type = .builtin },
            .{ .pattern = "min" ++ wb, .token_type = .builtin },
            .{ .pattern = "max" ++ wb, .token_type = .builtin },
            .{ .pattern = "clamp" ++ wb, .token_type = .builtin },
            .{ .pattern = "mix" ++ wb, .token_type = .builtin },
            .{ .pattern = "step" ++ wb, .token_type = .builtin },
            .{ .pattern = "smoothstep" ++ wb, .token_type = .builtin },
            .{ .pattern = "len" ++ wb, .token_type = .builtin },
            .{ .pattern = "length" ++ wb, .token_type = .builtin },
            .{ .pattern = "normalize" ++ wb, .token_type = .builtin },
            .{ .pattern = "dot" ++ wb, .token_type = .builtin },
            .{ .pattern = "cross" ++ wb, .token_type = .builtin },

            // ============ Reserved axis/output variables ============
            .{ .pattern = "axis1" ++ wb, .token_type = .axis },
            .{ .pattern = "axis2" ++ wb, .token_type = .axis },
            .{ .pattern = "axis3" ++ wb, .token_type = .axis },
            .{ .pattern = "time" ++ wb, .token_type = .axis },
            .{ .pattern = "red" ++ wb, .token_type = .axis },
            .{ .pattern = "green" ++ wb, .token_type = .axis },
            .{ .pattern = "blue" ++ wb, .token_type = .axis },
            .{ .pattern = "alpha" ++ wb, .token_type = .axis },

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
            .{ .pattern = "%", .token_type = .operator },
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
                    // No match - consume the full UTF-8 codepoint (not just 1 byte)
                    // to avoid splitting multi-byte characters like π (0xCF 0x80) across tokens.
                    const byte_len = std.unicode.utf8ByteSequenceLength(content[pos]) catch 1;
                    const end = @min(pos + byte_len, content.len);
                    try tokens.append(self.allocator, .{
                        .text = content[pos..end],
                        .token_type = .unknown,
                        .byte_start = pos,
                        .byte_end = end,
                    });
                    pos = end;
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
