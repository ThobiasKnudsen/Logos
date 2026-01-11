# RegexTrie Usage Guide

The `RegexTrie` is a high-performance trie data structure that supports both literal strings and regex patterns. It uses PCRE2 with JIT compilation for fast regex matching.

## Basic API

### Initialization

```zig
const RegexTrie = @import("regex_trie").RegexTrie;

var trie = try RegexTrie.init(allocator);
defer trie.deinit();
```

### Inserting Patterns

```zig
// Insert a pattern with associated data
pub fn insert(
    self: *RegexTrie,
    allocator: std.mem.Allocator,
    key: []const u8,           // Pattern (literal or regex)
    data: ?*anyopaque,         // Your data pointer
) RegexTrieError!void
```

### Matching

```zig
// Returns the longest match at the start of the string
pub fn get(self: *RegexTrie, string: []const u8) RegexTrieError!MatchResult

// Find all possible matches (slower, explores all paths)
pub fn getAllMatches(
    self: *RegexTrie,
    string: []const u8,
    allocator: std.mem.Allocator,
) RegexTrieError!std.ArrayList(MatchResult)
```

### Match Result

```zig
pub const MatchResult = struct {
    matched: usize,            // Number of bytes matched
    regex_key: []const u8,     // The pattern that matched
    data: ?*anyopaque,         // User data pointer (cast to your type)
};
```

### Removing Patterns

```zig
// Returns the data pointer that was associated with the pattern
pub fn remove(self: *RegexTrie, regex_key: []const u8) RegexTrieError!?*anyopaque
```

## Example 1: Storing Token Types (Lexer)

```zig
const TokenType = enum { keyword, identifier, number };

// Store token types in an ArrayList (stable pointers)
var token_types = std.ArrayList(TokenType).init(allocator);
try token_types.append(.keyword);

// Get stable pointer to the token type
const token_type_ptr = &token_types.items[0];

var trie = try RegexTrie.init(allocator);
defer trie.deinit();

// Insert pattern with data pointer
try trie.insert(allocator, "if", @ptrCast(token_type_ptr));

// Later, when matching:
const match = try trie.get("if (x > 0)");
const token_type = @as(*TokenType, @ptrCast(@alignCast(match.data))).*;
// token_type is now .keyword
// match.matched is 2 (length of "if")
```

## Example 2: Storing Arbitrary Structs

```zig
const SyntaxInfo = struct {
    color: [3]u8,
    style: enum { bold, italic, normal },
};

var syntax_info = try allocator.create(SyntaxInfo);
syntax_info.* = .{
    .color = .{ 255, 0, 0 },  // Red
    .style = .bold,
};

try trie.insert(allocator, "[0-9]+", @ptrCast(syntax_info));

// Later:
const match = try trie.get("123 + 456");
const info = @as(*SyntaxInfo, @ptrCast(@alignCast(match.data))).*;
// info.color is [255, 0, 0], info.style is .bold
// match.matched is 3 (length of "123")
```

## Example 3: Storing Function Pointers

```zig
const HandlerFn = *const fn([]const u8) void;

fn handleKeyword(text: []const u8) void {
    std.debug.print("Found keyword: {s}\n", .{text});
}

const handler_ptr: HandlerFn = &handleKeyword;

try trie.insert(
    allocator,
    "fn|let|if|else",
    @ptrCast(@constCast(&handler_ptr)),
);

// Later:
const match = try trie.get("fn main()");
const handler = @as(*const HandlerFn, @ptrCast(@alignCast(match.data))).*;
handler(match.regex_key);  // Calls handleKeyword
```

## Example 4: Finding All Matches

```zig
try trie.insert(allocator, "a", null);
try trie.insert(allocator, "ab", null);
try trie.insert(allocator, "abc", null);

var matches = try trie.getAllMatches("abcd", allocator);
defer matches.deinit(allocator);

for (matches.items) |m| {
    std.debug.print("Matched {d} chars with pattern '{s}'\n", .{ m.matched, m.regex_key });
}
// Output:
// Matched 1 chars with pattern 'a'
// Matched 2 chars with pattern 'ab'
// Matched 3 chars with pattern 'abc'
```

## Memory Management

1. **The trie copies and owns the pattern key** - you don't need to keep it alive
2. **The trie does NOT own your data pointer** - you are responsible for freeing it
3. **Use `ArrayList` for data storage** - guarantees stable pointers when appending
4. **`remove()` returns the data pointer** - so you can clean it up

```zig
// Inserting
const my_data = try allocator.create(MyType);
try trie.insert(allocator, "pattern", @ptrCast(my_data));

// Removing - get data back for cleanup
if (trie.remove("pattern")) |data_ptr| {
    const my_data = @as(*MyType, @ptrCast(@alignCast(data_ptr)));
    allocator.destroy(my_data);
} else |_| {
    // Pattern not found
}
```

## Error Handling

```zig
pub const RegexTrieError = error{
    AllocationFailure,
    NullArgument,
    NodeNotFound,       // No match found in get()
    NodeFound,
    DestroyFailed,
    DuplicateLeafValue, // Pattern already exists
};
```

## Performance Notes

- **Literals are fast** - O(1) lookup per character via index array
- **Regex patterns use PCRE2 JIT** - compiled once, reused for all matches
- **`get()` returns longest match** - single traversal, very efficient
- **`getAllMatches()` is slower** - explores all possible paths
- **Pure literal patterns bypass regex splitting** - extra fast path

