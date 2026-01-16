//! Parse state for a single tab session
//!
//! Manages lexing, parsing, and GLSL generation state for tab content.
//! Uses content hashing to avoid redundant re-parsing.

const std = @import("std");
const ast_node = @import("../lang/ast_node.zig");
const glsl_gen = @import("../lang/glsl_gen.zig");

pub const AstNode = ast_node.AstNode;
pub const GeneratedShader = glsl_gen.GeneratedShader;
pub const OutputType = glsl_gen.OutputType;

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
    keyword, // if, else, for, while, and, or, true, false
    identifier, // variable/function names
    number, // integer and float literals
    operator, // +, -, *, /, ^, =, ==, ², !, etc.
    string, // "..." string literals
    comment, // // or /* */ comments
    punctuation, // (, ), {, }, [, ], ,, :, .
    whitespace, // spaces, tabs, newlines
    builtin, // Built-in functions: sin, cos, log, etc.
    axis, // Reserved axis variables: axis1, axis2, axis3, x, y, z
    type_name, // Type names for casting: f32, f64, i32, etc.
    unknown, // unrecognized characters
};

/// Line and column position in source code
pub const SourceLocation = struct {
    line: usize, // 1-based line number
    column: usize, // 1-based column number

    pub fn format(self: SourceLocation, comptime _: []const u8, _: std.fmt.FormatOptions, writer: anytype) !void {
        try writer.print("Ln {d}, Col {d}", .{ self.line, self.column });
    }
};

/// Convert byte offset to line/column in source text
pub fn byteOffsetToLocation(content: []const u8, byte_offset: usize) SourceLocation {
    var line: usize = 1;
    var col: usize = 1;
    const safe_offset = @min(byte_offset, content.len);

    for (content[0..safe_offset]) |c| {
        if (c == '\n') {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    return .{ .line = line, .column = col };
}

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

    /// Get formatted error message with location info
    pub fn formatWithLocation(self: ParseError, content: []const u8, buf: []u8) []const u8 {
        const loc = byteOffsetToLocation(content, self.byte_start);
        return std.fmt.bufPrint(buf, "{s} (Ln {d}, Col {d})", .{
            self.message,
            loc.line,
            loc.column,
        }) catch self.message;
    }
};

/// Result of parsing operation
pub const ParseResult = union(enum) {
    /// Successful parse with AST
    success: struct {
        ast: *AstNode,
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
    ast: ?*AstNode,

    /// Generated GLSL fragment shaders (one per root output)
    generated_shaders: ?[]GeneratedShader,

    /// Parse errors for display (allocated in general allocator)
    errors: std.ArrayList(ParseError),

    /// Arena allocator for this parse cycle (reset on each new parse)
    parse_arena: std.heap.ArenaAllocator,

    /// General allocator (for tokens, errors, etc.)
    allocator: std.mem.Allocator,

    /// Current parsing status
    status: enum { idle, lexing, parsing, ready, err },

    /// Error message for display (set when status is .err)
    error_message: ?[]const u8,

    /// Initialize a new parse state
    pub fn init(allocator: std.mem.Allocator) ParseState {
        return .{
            .content_hash = 0,
            .tokens = null,
            .ast = null,
            .generated_shaders = null,
            .errors = std.ArrayList(ParseError){
                .items = &.{},
                .capacity = 0,
            },
            .parse_arena = std.heap.ArenaAllocator.init(allocator),
            .allocator = allocator,
            .status = .idle,
            .error_message = null,
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

        // Clear parse products
        self.ast = null;
        self.freeGeneratedShaders();

        // Clear errors but retain capacity
        self.errors.clearRetainingCapacity();
        self.error_message = null;

        // Status back to idle
        self.status = .idle;
    }

    /// Free generated shaders
    pub fn freeGeneratedShaders(self: *ParseState) void {
        if (self.generated_shaders) |shaders| {
            for (shaders) |shader| {
                self.allocator.free(shader.source);
            }
            self.allocator.free(shaders);
            self.generated_shaders = null;
        }
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
        self.freeGeneratedShaders();
        self.errors.deinit(self.allocator);
        self.parse_arena.deinit();
    }
};
