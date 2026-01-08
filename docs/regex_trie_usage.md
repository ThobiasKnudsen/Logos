# RegexTrie Generic Usage Guide

The `RegexTrie` is now fully generic - you can store any data type alongside your regex patterns.

## Basic Structure

```zig
pub const RegexTrieValue = struct {
    regex_key: []const u8,       // The regex pattern
    data: ?*anyopaque,            // Your generic data pointer
    allocator: std.mem.Allocator,
};
```

## Example 1: Storing Token Types (Lexer)

```zig
const TokenType = enum { keyword, identifier, number };

// Store token types in an ArrayList (so pointers remain stable)
var token_types = std.ArrayList(TokenType).init(allocator);
try token_types.append(allocator, .keyword);

// Get stable pointer to the token type
const token_type_ptr = &token_types.items[0];

// Create trie value with data pointer
const value = try RegexTrieValue.create(
    allocator,
    "if",                         // Pattern
    @ptrCast(token_type_ptr),    // Your data
);

var trie = try RegexTrie.init(allocator);
try trie.insert(value);

// Later, when matching:
const match = try trie.get("if (x > 0)");
const token_type = @as(*TokenType, @ptrCast(@alignCast(match.value.data))).*;
// token_type is now .keyword
```

## Example 2: Storing Arbitrary Structs

```zig
const SyntaxInfo = struct {
    color: [3]u8,
    style: enum { bold, italic, normal },
};

var syntax_info = try allocator.create(SyntaxInfo);
syntax_info.* = .{
    .color = .{255, 0, 0},  // Red
    .style = .bold,
};

const value = try RegexTrieValue.create(
    allocator,
    "[0-9]+",                    // Number pattern
    @ptrCast(syntax_info),      // Your struct
);

try trie.insert(value);

// Later:
const match = try trie.get("123 + 456");
const info = @as(*SyntaxInfo, @ptrCast(@alignCast(match.value.data))).*;
// info.color is [255, 0, 0], info.style is .bold
```

## Example 3: Storing Function Pointers

```zig
const HandlerFn = *const fn([]const u8) void;

fn handleKeyword(text: []const u8) void {
    std.debug.print("Found keyword: {s}\n", .{text});
}

const handler_ptr: HandlerFn = &handleKeyword;

const value = try RegexTrieValue.create(
    allocator,
    "fn|let|if|else",
    @ptrCast(@constCast(&handler_ptr)),
);

try trie.insert(value);

// Later:
const match = try trie.get("fn main()");
const handler = @as(*const HandlerFn, @ptrCast(@alignCast(match.value.data))).*;
handler(match.value.regex_key);  // Calls handleKeyword
```

## Memory Management Notes

1. **RegexTrieValue does NOT free the data** - you are responsible for freeing it
2. Keep your data alive as long as the trie exists
3. Use `ArrayList` to store data (guarantees stable pointers)
4. Alternative: allocate each data item individually and free them when cleaning up

## How the Lexer Uses It

The lexer stores `TokenType` values in an `ArrayList(TokenType)` and points to them:

```zig
pub const Lexer = struct {
    trie: *regex_trie.RegexTrie,
    token_types: std.ArrayList(TokenType),  // Keeps data alive

    pub fn init(allocator: Allocator, config: LexerConfig) !Lexer {
        var token_types = std.ArrayList(TokenType).init(allocator);

        for (config.patterns) |pattern| {
            try token_types.append(allocator, pattern.token_type);
            const ptr = &token_types.items[token_types.items.len - 1];

            const value = try RegexTrieValue.create(
                allocator,
                pattern.pattern,
                @ptrCast(ptr),  // Pointer to TokenType in ArrayList
            );
            try trie.insert(value);
        }
        // ArrayList keeps token types alive for the lifetime of the lexer
    }
};
```

This design keeps the RegexTrie generic while allowing type-safe storage of any data!
