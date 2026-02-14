const std = @import("std");
const lexer = @import("src/lang/lexer.zig");
const parser = @import("src/lang/parser.zig");

pub fn main() !void {
    const allocator = std.heap.page_allocator;

    // Test the user's problematic code
    const source = "a(x,y): (x+y), a(x,y) = 9";

    // Tokenize
    var lex = try lexer.Lexer.init(allocator);
    defer lex.deinit();

    const tokens = try lex.tokenize(source);
    defer allocator.free(tokens);

    std.debug.print("Tokens:\n", .{});
    for (tokens, 0..) |tok, i| {
        std.debug.print("  {}: {s} ({s})\n", .{ i, tok.text, @tagName(tok.token_type) });
    }
    std.debug.print("\n", .{});

    // Parse
    const result = try parser.parseTokens(allocator, tokens);
    defer {
        if (result.errors.len == 0) {
            result.ast.deinit(allocator);
        }
        allocator.free(result.errors);
    }

    if (result.errors.len > 0) {
        std.debug.print("Parse errors:\n", .{});
        for (result.errors) |err| {
            std.debug.print("  [{}-{}]: {s}\n", .{ err.byte_start, err.byte_end, err.message });
        }
    } else {
        std.debug.print("Parse successful!\n", .{});
        std.debug.print("AST: {}\n", .{result.ast.data});
    }
}
