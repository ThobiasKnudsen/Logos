use super::ast::AstNode;
use super::token::{Token, TokenType};

/// Recursive-descent parser for the Logos math language.
///
/// Key differences from typical languages:
/// - `:` is the binding operator (like `=` in other languages)
/// - `=` is the equality operator (like `==` in other languages)
/// - Comma separates statements inside parenthesized blocks
/// - Newline separates statements at top level
///
/// Operator precedence (lowest to highest):
///   1. Logical OR
///   2. Logical AND
///   3. Comparison (=, !=, <, >, <=, >=)
///   4. Addition / Subtraction
///   5. Multiplication / Division / Modulo
///   6. Exponentiation (^, right-associative)
///   7. Unary (-, not)
///   8. Postfix (function call, indexing)
///   9. Primary (number, identifier, parenthesized expr/block, if, etc.)
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(&mut self) -> Result<AstNode, String> {
        self.skip_newlines();
        if self.at_end() {
            // Empty input — return a default zero
            return Ok(AstNode::Number(0.0));
        }

        let mut stmts = Vec::new();
        while !self.at_end() {
            self.skip_newlines();
            if self.at_end() {
                break;
            }
            stmts.push(self.parse_statement()?);
            self.skip_newlines();
        }

        if stmts.is_empty() {
            Ok(AstNode::Number(0.0))
        } else if stmts.len() == 1 {
            Ok(stmts.pop().unwrap())
        } else {
            Ok(AstNode::Block(stmts))
        }
    }

    fn parse_statement(&mut self) -> Result<AstNode, String> {
        // Check for function definition: name(params): body
        if let Some(func) = self.try_parse_function_def()? {
            return Ok(func);
        }

        // Check for binding: name: expr
        if let Some(binding) = self.try_parse_binding()? {
            return Ok(binding);
        }

        self.parse_expr()
    }

    fn try_parse_function_def(&mut self) -> Result<Option<AstNode>, String> {
        let save = self.pos;

        // name(params): body  OR  name(params) = body (for backward compat)
        let name = match &self.peek().ty {
            TokenType::Identifier(name) => name.clone(),
            _ => return Ok(None),
        };

        if self.peek_at(1).map(|t| &t.ty) != Some(&TokenType::LParen) {
            return Ok(None);
        }

        self.advance(); // consume identifier
        self.advance(); // consume '('

        let mut params = Vec::new();
        while self.peek().ty != TokenType::RParen {
            match &self.peek().ty {
                TokenType::Identifier(p) => {
                    params.push(p.clone());
                    self.advance();
                }
                TokenType::AxisVar(p) => {
                    params.push(p.clone());
                    self.advance();
                }
                _ => { self.pos = save; return Ok(None); }
            }
            if self.peek().ty == TokenType::Comma {
                self.advance();
            }
        }
        self.advance(); // consume ')'

        // Body follows `:` (primary) or `=` (backward compat)
        if self.peek().ty == TokenType::Colon {
            self.advance();
            let body = self.parse_expr()?;
            return Ok(Some(AstNode::FunctionDef {
                name,
                params,
                body: Box::new(body),
            }));
        }

        // If no colon follows, this isn't a function def — might be a function call
        self.pos = save;
        Ok(None)
    }

    fn try_parse_binding(&mut self) -> Result<Option<AstNode>, String> {
        // name: expr (using colon for binding)
        let name = match &self.peek().ty {
            TokenType::Identifier(name) => name.clone(),
            _ => return Ok(None),
        };

        if self.peek_at(1).map(|t| &t.ty) != Some(&TokenType::Colon) {
            return Ok(None);
        }

        self.advance(); // consume identifier
        self.advance(); // consume ':'
        let value = self.parse_expr()?;
        Ok(Some(AstNode::Binding {
            name,
            value: Box::new(value),
        }))
    }

    // --- Expression parsing with precedence climbing ---

    fn parse_expr(&mut self) -> Result<AstNode, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_and()?;
        while self.peek().ty == TokenType::Or {
            self.advance();
            let right = self.parse_and()?;
            left = AstNode::Apply {
                name: "or".to_string(),
                args: vec![left, right],
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_comparison()?;
        while self.peek().ty == TokenType::And {
            self.advance();
            let right = self.parse_comparison()?;
            left = AstNode::Apply {
                name: "and".to_string(),
                args: vec![left, right],
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_addition()?;
        loop {
            let op = match self.peek().ty {
                TokenType::Eq => "eq",
                TokenType::Neq => "neq",
                TokenType::Lt => "lt",
                TokenType::Gt => "gt",
                TokenType::Lte => "lte",
                TokenType::Gte => "gte",
                _ => break,
            };
            self.advance();
            let right = self.parse_addition()?;
            left = AstNode::Apply {
                name: op.to_string(),
                args: vec![left, right],
            };
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.peek().ty {
                TokenType::Plus => "add",
                TokenType::Minus => "sub",
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            left = AstNode::Apply {
                name: op.to_string(),
                args: vec![left, right],
            };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_exponent()?;
        loop {
            let op = match self.peek().ty {
                TokenType::Star => "mul",
                TokenType::Slash => "div",
                TokenType::Percent => "mod",
                _ => break,
            };
            self.advance();
            let right = self.parse_exponent()?;
            left = AstNode::Apply {
                name: op.to_string(),
                args: vec![left, right],
            };
        }
        Ok(left)
    }

    fn parse_exponent(&mut self) -> Result<AstNode, String> {
        let base = self.parse_unary()?;
        if self.peek().ty == TokenType::Caret {
            self.advance();
            // Right-associative: recurse into parse_exponent
            let exp = self.parse_exponent()?;
            Ok(AstNode::Apply {
                name: "pow".to_string(),
                args: vec![base, exp],
            })
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<AstNode, String> {
        if self.peek().ty == TokenType::Minus {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(AstNode::Apply {
                name: "neg".to_string(),
                args: vec![operand],
            });
        }
        if self.peek().ty == TokenType::Not {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(AstNode::Apply {
                name: "not".to_string(),
                args: vec![operand],
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<AstNode, String> {
        let mut expr = self.parse_primary()?;

        loop {
            match &self.peek().ty {
                // Function call: expr(args)
                TokenType::LParen => {
                    if let AstNode::Identifier(ref name) = expr {
                        let name = name.clone();
                        self.advance(); // consume '('
                        let args = self.parse_arg_list()?;
                        self.expect(TokenType::RParen)?;
                        expr = AstNode::Apply { name, args };
                    } else {
                        break;
                    }
                }
                // Indexing: expr[index]
                TokenType::LBracket => {
                    self.advance();
                    let _index = self.parse_expr()?;
                    self.expect(TokenType::RBracket)?;
                }
                // Unicode superscript square
                TokenType::Builtin(ref s) if s == "square" => {
                    self.advance();
                    expr = AstNode::Apply {
                        name: "mul".to_string(),
                        args: vec![expr.clone(), expr],
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<AstNode, String> {
        match self.peek().ty.clone() {
            TokenType::Number(n) => {
                self.advance();
                Ok(AstNode::Number(n))
            }
            TokenType::BoolLit(b) => {
                self.advance();
                Ok(AstNode::BoolLit(b))
            }
            TokenType::Identifier(name) => {
                self.advance();
                Ok(AstNode::Identifier(name))
            }
            TokenType::AxisVar(name) => {
                self.advance();
                Ok(AstNode::Identifier(name))
            }
            TokenType::Builtin(name) => {
                self.advance();
                if self.peek().ty == TokenType::LParen {
                    self.advance(); // consume '('
                    let args = self.parse_arg_list()?;
                    self.expect(TokenType::RParen)?;
                    Ok(AstNode::Apply { name, args })
                } else {
                    Ok(AstNode::Identifier(name))
                }
            }
            TokenType::LParen => {
                self.advance(); // consume '('
                self.skip_newlines();
                if self.peek().ty == TokenType::RParen {
                    self.advance();
                    return Ok(AstNode::Tuple(Vec::new()));
                }

                // Parse a block: comma-separated statements inside parens.
                // Could be: (expr), (a, b, c) tuple, or (binding, binding, result) block
                let first = self.parse_block_item()?;
                self.skip_newlines();

                if self.peek().ty == TokenType::Comma {
                    // Multiple items — could be tuple or block with bindings
                    let mut items = vec![first];
                    while self.peek().ty == TokenType::Comma {
                        self.advance();
                        self.skip_newlines();
                        if self.peek().ty == TokenType::RParen {
                            break; // trailing comma
                        }
                        items.push(self.parse_block_item()?);
                        self.skip_newlines();
                    }
                    self.expect(TokenType::RParen)?;

                    // If any item is a Binding or FunctionDef, it's a block
                    let has_bindings = items.iter().any(|item| {
                        matches!(item, AstNode::Binding { .. } | AstNode::FunctionDef { .. })
                    });
                    if has_bindings {
                        Ok(AstNode::Block(items))
                    } else {
                        Ok(AstNode::Tuple(items))
                    }
                } else {
                    // Single parenthesized expression
                    self.expect(TokenType::RParen)?;
                    Ok(first)
                }
            }
            TokenType::If => {
                self.advance(); // consume 'if'
                let has_paren = self.peek().ty == TokenType::LParen;
                if has_paren { self.advance(); }
                let condition = self.parse_expr()?;
                if has_paren { self.expect(TokenType::RParen)?; }

                self.skip_newlines();
                let then_branch = self.parse_expr()?;
                self.skip_newlines();

                let else_branch = if self.peek().ty == TokenType::Else {
                    self.advance();
                    self.skip_newlines();
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };

                Ok(AstNode::IfExpr {
                    condition: Box::new(condition),
                    then_branch: Box::new(then_branch),
                    else_branch,
                })
            }
            // Type names as cast functions: f32(expr), vec2(a, b), etc.
            TokenType::TypeF32 | TokenType::TypeF64 | TokenType::TypeI32 |
            TokenType::TypeVec2 | TokenType::TypeVec3 | TokenType::TypeVec4 => {
                let cast_name = match &self.peek().ty {
                    TokenType::TypeF32 => "f32",
                    TokenType::TypeF64 => "f64",
                    TokenType::TypeI32 => "i32",
                    TokenType::TypeVec2 => "vec2",
                    TokenType::TypeVec3 => "vec3",
                    TokenType::TypeVec4 => "vec4",
                    _ => unreachable!(),
                }.to_string();
                self.advance();

                if self.peek().ty == TokenType::LParen {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(TokenType::RParen)?;
                    Ok(AstNode::Apply { name: cast_name, args })
                } else {
                    Ok(AstNode::Identifier(cast_name))
                }
            }
            _ => {
                Err(format!("Unexpected token {:?} at position {}", self.peek().ty, self.peek().span.0))
            }
        }
    }

    /// Parse an item inside a parenthesized block (handles binding with colon).
    fn parse_block_item(&mut self) -> Result<AstNode, String> {
        // Try function def first
        if let Some(func) = self.try_parse_function_def()? {
            return Ok(func);
        }
        // Try binding
        if let Some(binding) = self.try_parse_binding()? {
            return Ok(binding);
        }
        self.parse_expr()
    }

    fn parse_arg_list(&mut self) -> Result<Vec<AstNode>, String> {
        let mut args = Vec::new();
        if self.peek().ty == TokenType::RParen {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        while self.peek().ty == TokenType::Comma {
            self.advance();
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }

    // --- Helpers ---

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&EOF_TOKEN)
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len() || self.peek().ty == TokenType::Eof
    }

    fn skip_newlines(&mut self) {
        while self.peek().ty == TokenType::Newline {
            self.advance();
        }
    }

    fn expect(&mut self, expected: TokenType) -> Result<(), String> {
        if std::mem::discriminant(&self.peek().ty) == std::mem::discriminant(&expected) {
            self.advance();
            Ok(())
        } else {
            Err(format!(
                "Expected {:?}, found {:?} at position {}",
                expected,
                self.peek().ty,
                self.peek().span.0,
            ))
        }
    }
}

const EOF_TOKEN: Token = Token {
    ty: TokenType::Eof,
    span: (0, 0),
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::lexer::Lexer;

    fn parse(input: &str) -> AstNode {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse().unwrap()
    }

    #[test]
    fn test_empty_input() {
        let ast = parse("");
        assert!(matches!(ast, AstNode::Number(n) if n == 0.0));
    }

    #[test]
    fn test_whitespace_only() {
        let ast = parse("   \t  ");
        assert!(matches!(ast, AstNode::Number(n) if n == 0.0));
    }

    #[test]
    fn test_single_number() {
        let ast = parse("42");
        assert!(matches!(ast, AstNode::Number(n) if n == 42.0));
    }

    #[test]
    fn test_single_variable() {
        let ast = parse("x");
        assert!(matches!(ast, AstNode::Identifier(ref s) if s == "x"));
    }

    #[test]
    fn test_simple_addition() {
        let ast = parse("x + y");
        match ast {
            AstNode::Apply { name, args } => {
                assert_eq!(name, "add");
                assert_eq!(args.len(), 2);
            }
            _ => panic!("Expected Apply node"),
        }
    }

    #[test]
    fn test_precedence() {
        // x + y * z  should parse as  x + (y * z)
        let ast = parse("x + y * z");
        match ast {
            AstNode::Apply { name, ref args } => {
                assert_eq!(name, "add");
                match &args[1] {
                    AstNode::Apply { name, .. } => assert_eq!(name, "mul"),
                    _ => panic!("Expected mul"),
                }
            }
            _ => panic!("Expected Apply"),
        }
    }

    #[test]
    fn test_function_call() {
        let ast = parse("sin(x)");
        match ast {
            AstNode::Apply { name, args } => {
                assert_eq!(name, "sin");
                assert_eq!(args.len(), 1);
            }
            _ => panic!("Expected Apply"),
        }
    }

    #[test]
    fn test_negation() {
        let ast = parse("-x");
        match ast {
            AstNode::Apply { name, args } => {
                assert_eq!(name, "neg");
                assert_eq!(args.len(), 1);
            }
            _ => panic!("Expected neg Apply"),
        }
    }

    #[test]
    fn test_power_right_assoc() {
        // x ^ y ^ z should parse as x ^ (y ^ z)
        let ast = parse("x ^ y ^ z");
        match ast {
            AstNode::Apply { name, ref args } => {
                assert_eq!(name, "pow");
                match &args[1] {
                    AstNode::Apply { name, .. } => assert_eq!(name, "pow"),
                    _ => panic!("Expected inner pow"),
                }
            }
            _ => panic!("Expected pow"),
        }
    }

    #[test]
    fn test_binding_with_colon() {
        // `:` is the binding operator in Logos
        let ast = parse("a: 5");
        match ast {
            AstNode::Binding { name, .. } => assert_eq!(name, "a"),
            _ => panic!("Expected Binding, got {:?}", ast),
        }
    }

    #[test]
    fn test_equality_with_equals() {
        // `=` is equality in Logos (not assignment)
        let ast = parse("x = 5");
        match ast {
            AstNode::Apply { name, args } => {
                assert_eq!(name, "eq");
                assert_eq!(args.len(), 2);
            }
            _ => panic!("Expected equality Apply, got {:?}", ast),
        }
    }

    #[test]
    fn test_function_def_with_colon() {
        let ast = parse("f(x, y): x + y");
        match ast {
            AstNode::FunctionDef { name, params, .. } => {
                assert_eq!(name, "f");
                assert_eq!(params, vec!["x", "y"]);
            }
            _ => panic!("Expected FunctionDef"),
        }
    }

    #[test]
    fn test_if_expr() {
        let ast = parse("if (x > 0) 1 else -1");
        match ast {
            AstNode::IfExpr { else_branch, .. } => {
                assert!(else_branch.is_some());
            }
            _ => panic!("Expected IfExpr"),
        }
    }

    #[test]
    fn test_multi_arg_builtin() {
        let ast = parse("clamp(x, 0, 1)");
        match ast {
            AstNode::Apply { name, args } => {
                assert_eq!(name, "clamp");
                assert_eq!(args.len(), 3);
            }
            _ => panic!("Expected Apply"),
        }
    }

    #[test]
    fn test_multiline_statements() {
        // Newline-separated statements
        let ast = parse("a: 5\nb: 10\na + b");
        match ast {
            AstNode::Block(stmts) => {
                assert_eq!(stmts.len(), 3);
                assert!(matches!(&stmts[0], AstNode::Binding { name, .. } if name == "a"));
                assert!(matches!(&stmts[1], AstNode::Binding { name, .. } if name == "b"));
            }
            _ => panic!("Expected Block with 3 stmts"),
        }
    }

    #[test]
    fn test_block_in_parens() {
        // Comma-separated block inside parens
        let ast = parse("(a: 5, b: 10, a + b)");
        match ast {
            AstNode::Block(stmts) => {
                assert_eq!(stmts.len(), 3);
            }
            _ => panic!("Expected Block, got {:?}", ast),
        }
    }

    #[test]
    fn test_tuple() {
        let ast = parse("(1, 2, 3)");
        match ast {
            AstNode::Tuple(items) => {
                assert_eq!(items.len(), 3);
            }
            _ => panic!("Expected Tuple"),
        }
    }

    #[test]
    fn test_complex_math_expr() {
        // sqrt(x*x + y*y) — common Logos pattern
        let ast = parse("sqrt(x*x + y*y)");
        match ast {
            AstNode::Apply { name, .. } => assert_eq!(name, "sqrt"),
            _ => panic!("Expected sqrt Apply"),
        }
    }

    #[test]
    fn test_function_def_with_block_body() {
        // Function with block body (comma-separated bindings)
        let ast = parse("f(a, b): (r: a + b, r * 2)");
        match ast {
            AstNode::FunctionDef { name, params, body } => {
                assert_eq!(name, "f");
                assert_eq!(params, vec!["a", "b"]);
                assert!(matches!(*body, AstNode::Block(_)));
            }
            _ => panic!("Expected FunctionDef, got {:?}", ast),
        }
    }

    #[test]
    fn test_nested_function_calls() {
        let ast = parse("log(0.5 * log(x) / log(2.0))");
        match ast {
            AstNode::Apply { name, .. } => assert_eq!(name, "log"),
            _ => panic!("Expected log Apply"),
        }
    }

    #[test]
    fn test_chained_comparison() {
        let ast = parse("x > 0 and x < 10");
        match ast {
            AstNode::Apply { name, .. } => assert_eq!(name, "and"),
            _ => panic!("Expected and Apply"),
        }
    }
}
