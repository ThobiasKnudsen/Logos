//! Parse state for a single tab session
//!
//! Manages lexing, parsing, and GLSL generation state for tab content.
//! Uses content hashing to avoid redundant re-parsing.

const std = @import("std");

/// Token from lexer - represents one lexical element
pub const Token = struct {
    /// The actual text of the token
    text: []const u8,

    /// Type classification for syntax highlighting and parsing
    token_type: TokenType,

    /// Byte offset in source text where token starts
    byte_start: usize,

    /// Byte offset in source text where token ends (exclusive)
    byte_end: usize,
};

/// Token type classification - matches types in RegexTrieValue
pub const TokenType = enum {
    keyword, // if, else, for, while, fn, let, etc.
    identifier, // variable/function names
    number, // integer and float literals
    operator, // +, -, *, /, ^, =, ==, etc.
    string, // "..." string literals
    comment, // // or /* */ comments
    punctuation, // (, ), {, }, [, ], ,, ;
    whitespace, // spaces, tabs, newlines
    unknown, // unrecognized characters
};

/// Parse error information for inline display
pub const ParseError = struct {
    /// Byte offset where error starts
    byte_start: usize,

    /// Byte offset where error ends (exclusive)
    byte_end: usize,

    /// Human-readable error message
    message: []const u8,

    /// Severity level for display
    severity: enum { err, warning, hint },
};

/// Result of parsing operation
pub const ParseResult = union(enum) {
    /// Successful parse with AST and GLSL
    success: struct {
        // ast: *AstNode,  // TODO: Add when AST is implemented
        // glsl: []const u8,  // TODO: Add when GLSL gen is implemented
    },

    /// Fail-fast: single error (initial implementation)
    fail_fast: ParseError,

    // Future extensions:
    // collected_errors: []ParseError,
    // partial: struct { ast: *AstNode, errors: []ParseError },
};

/// All parsing state for a tab session
pub const ParseState = struct {
    /// Hash of content when last lexed (for cache invalidation)
    content_hash: u64,

    /// Cached token stream from lexer (allocated in general allocator)
    tokens: ?[]Token,

    /// Parsed AST (allocated in parse_arena)
    // ast: ?*AstNode,  // TODO: Add when AST is implemented

    /// Generated GLSL shader code (allocated in parse_arena)
    // generated_glsl: ?[]const u8,  // TODO: Add when GLSL gen is implemented

    /// Parse errors for display (allocated in general allocator)
    errors: std.ArrayList(ParseError),

    /// Arena allocator for this parse cycle (reset on each new parse)
    parse_arena: std.heap.ArenaAllocator,

    /// General allocator (for tokens, errors, etc.)
    allocator: std.mem.Allocator,

    /// Current parsing status
    status: enum { idle, lexing, parsing, ready, err },

    /// Initialize a new parse state
    pub fn init(allocator: std.mem.Allocator) ParseState {
        return .{
            .content_hash = 0,
            .tokens = null,
            .errors = std.ArrayList(ParseError){
                .items = &.{},
                .capacity = 0,
            },
            .parse_arena = std.heap.ArenaAllocator.init(allocator),
            .allocator = allocator,
            .status = .idle,
        };
    }

    /// Check if content has changed and needs re-lexing
    pub fn shouldRelex(self: *ParseState, content: []const u8) bool {
        const new_hash = std.hash.Wyhash.hash(0, content);
        if (new_hash == self.content_hash) return false;
        self.content_hash = new_hash;
        return true;
    }

    /// Start a new parse cycle - resets arena and clears old data
    pub fn startNewParse(self: *ParseState) void {
        // Reset arena, freeing all previous AST nodes and GLSL strings
        _ = self.parse_arena.reset(.retain_capacity);

        // Clear parse products (AST and GLSL will be set to null when implemented)
        // self.ast = null;
        // self.generated_glsl = null;

        // Clear errors but retain capacity
        self.errors.clearRetainingCapacity();

        // Status back to idle
        self.status = .idle;
    }

    /// Free tokens from previous lex
    pub fn freeTokens(self: *ParseState) void {
        if (self.tokens) |tokens| {
            self.allocator.free(tokens);
            self.tokens = null;
        }
    }

    /// Deinitialize parse state
    pub fn deinit(self: *ParseState) void {
        self.freeTokens();
        self.errors.deinit(self.allocator);
        self.parse_arena.deinit();
    }
};
