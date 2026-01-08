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

    /// Default lexer configuration for math expressions
    pub fn mathDefault() LexerConfig {
        const patterns = [_]TokenPattern{
            // Keywords (example - adjust for your language)
            .{ .pattern = "if", .token_type = .keyword },
            .{ .pattern = "else", .token_type = .keyword },
            .{ .pattern = "for", .token_type = .keyword },
            .{ .pattern = "while", .token_type = .keyword },
            .{ .pattern = "fn", .token_type = .keyword },
            .{ .pattern = "let", .token_type = .keyword },
            .{ .pattern = "return", .token_type = .keyword },

            // Numbers (integers and floats)
            .{ .pattern = "[0-9]+\\.[0-9]+", .token_type = .number }, // Float
            .{ .pattern = "[0-9]+", .token_type = .number }, // Integer

            // Identifiers (variable/function names)
            .{ .pattern = "[a-zA-Z_][a-zA-Z0-9_]*", .token_type = .identifier },

            // Operators
            .{ .pattern = "\\+", .token_type = .operator },
            .{ .pattern = "-", .token_type = .operator },
            .{ .pattern = "\\*", .token_type = .operator },
            .{ .pattern = "/", .token_type = .operator },
            .{ .pattern = "\\^", .token_type = .operator },
            .{ .pattern = "==", .token_type = .operator },
            .{ .pattern = "!=", .token_type = .operator },
            .{ .pattern = "<=", .token_type = .operator },
            .{ .pattern = ">=", .token_type = .operator },
            .{ .pattern = "<", .token_type = .operator },
            .{ .pattern = ">", .token_type = .operator },
            .{ .pattern = "=", .token_type = .operator },

            // Punctuation
            .{ .pattern = "\\(", .token_type = .punctuation },
            .{ .pattern = "\\)", .token_type = .punctuation },
            .{ .pattern = "\\{", .token_type = .punctuation },
            .{ .pattern = "\\}", .token_type = .punctuation },
            .{ .pattern = "\\[", .token_type = .punctuation },
            .{ .pattern = "\\]", .token_type = .punctuation },
            .{ .pattern = ",", .token_type = .punctuation },
            .{ .pattern = ";", .token_type = .punctuation },

            // Whitespace
            .{ .pattern = "[ \\t\\n\\r]+", .token_type = .whitespace },

            // Comments
            .{ .pattern = "//[^\\n]*", .token_type = .comment },
            .{ .pattern = "/\\*[\\s\\S]*?\\*/", .token_type = .comment },
        };
        return .{ .patterns = &patterns };
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

        var token_types = std.ArrayList(TokenType).init(allocator);
        errdefer token_types.deinit(allocator);

        // Insert all patterns into the trie with TokenType as data
        for (config.patterns) |pattern_entry| {
            // Store the token type in our list
            try token_types.append(allocator, pattern_entry.token_type);
            const token_type_ptr = &token_types.items[token_types.items.len - 1];

            // Create value with pointer to the token type
            const value = try regex_trie.RegexTrieValue.create(
                allocator,
                pattern_entry.pattern,
                @ptrCast(token_type_ptr),
            );
            errdefer value.deinit();

            try trie.insert(value);
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
        var tokens = std.ArrayList(Token).init(self.allocator);
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
            const token_type = if (match_result.value.data) |data_ptr|
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
