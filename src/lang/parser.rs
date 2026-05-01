//! Recursive-descent parser for the Logos math language.
//!
//! Key differences from typical languages:
//! - `:=` is the binding operator (like `=` in other languages)
//! - `=` is the equality operator (like `==` in other languages)
//! - Comma separates statements inside parenthesized blocks
//! - Newline separates statements at top level
//!
//! Operator precedence (lowest to highest):
//!
//!   1. Logical OR
//!   2. Logical AND
//!   3. Comparison (=, !=, <, >, <=, >=)
//!   4. Addition / Subtraction
//!   5. Multiplication / Division / Modulo
//!   6. Exponentiation (^, right-associative)
//!   7. Unary (-, not)
//!   8. Postfix (function call, indexing)
//!   9. Primary (number, identifier, parenthesized expr/block, if, etc.)

use super::ast::AstNode;
use super::token::{Token, TokenType};

/// Maximum recursion depth for nested expressions / parentheses. Guards
/// against stack overflow on pathological input (e.g. `((((...))))`).
/// Each level descends through ~10 mutually recursive parse fns, so this
/// translates to roughly 10x the stack frames.
const MAX_RECURSION_DEPTH: u32 = 64;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    source: String,
    /// When false, bare identifiers followed by `(` are NOT parsed as function
    /// calls in parse_postfix. Used to prevent the `(body)` of `parallel for`
    /// from being consumed as a call on the last range token.
    allow_ident_call: bool,
    /// Current recursion depth (incremented on each call to parse_primary/expr).
    depth: u32,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, source: String) -> Self {
        Self {
            tokens,
            pos: 0,
            source,
            allow_ident_call: true,
            depth: 0,
        }
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_RECURSION_DEPTH {
            return Err(format!(
                "expression nested too deeply (max {} levels)",
                MAX_RECURSION_DEPTH
            ));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
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
            // Skip optional comma separator between top-level statements
            self.skip_newlines();
            if self.peek().ty == TokenType::Comma {
                self.advance();
            }
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
        // Check for tuple destructuring binding: (a, b) := expr
        if let Some(tb) = self.try_parse_tuple_binding()? {
            return Ok(tb);
        }

        // Check for function definition: name(params) := body
        if let Some(func) = self.try_parse_function_def()? {
            return Ok(func);
        }

        // Check for binding: name := expr
        if let Some(binding) = self.try_parse_binding()? {
            return Ok(binding);
        }

        let expr = self.parse_expr()?;
        self.try_index_assign(expr)
    }

    fn try_parse_function_def(&mut self) -> Result<Option<AstNode>, String> {
        let save = self.pos;

        // name(params) := body  OR  name(params) = body (for backward compat)
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
                _ => {
                    self.pos = save;
                    return Ok(None);
                }
            }
            if self.peek().ty == TokenType::Comma {
                self.advance();
            }
        }
        self.advance(); // consume ')'

        // Body follows `:=` (primary) or `=` (backward compat)
        if self.peek().ty == TokenType::Colon {
            self.advance();
            let body = self.parse_expr()?;
            return Ok(Some(AstNode::FunctionDef {
                name,
                params,
                body: Box::new(body),
            }));
        }

        // If no := follows, this isn't a function def — might be a function call
        self.pos = save;
        Ok(None)
    }

    fn try_parse_binding(&mut self) -> Result<Option<AstNode>, String> {
        // name := expr (using := for binding)
        let name = match &self.peek().ty {
            TokenType::Identifier(name) => name.clone(),
            _ => return Ok(None),
        };

        if self.peek_at(1).map(|t| &t.ty) != Some(&TokenType::Colon) {
            return Ok(None);
        }

        self.advance(); // consume identifier
        self.advance(); // consume ':='
        let value = self.parse_expr()?;
        Ok(Some(AstNode::Binding {
            name,
            value: Box::new(value),
        }))
    }

    /// Try to parse a tuple destructuring binding: `(a, b) := expr`
    fn try_parse_tuple_binding(&mut self) -> Result<Option<AstNode>, String> {
        if self.peek().ty != TokenType::LParen {
            return Ok(None);
        }

        let save = self.pos;
        self.advance(); // consume '('

        let mut names = Vec::new();
        // First name
        match &self.peek().ty {
            TokenType::Identifier(n) | TokenType::AxisVar(n) => {
                names.push(n.clone());
                self.advance();
            }
            _ => {
                self.pos = save;
                return Ok(None);
            }
        }

        // More names separated by commas
        while self.peek().ty == TokenType::Comma {
            self.advance(); // consume ','
            match &self.peek().ty {
                TokenType::Identifier(n) | TokenType::AxisVar(n) => {
                    names.push(n.clone());
                    self.advance();
                }
                _ => {
                    self.pos = save;
                    return Ok(None);
                }
            }
        }

        if self.peek().ty != TokenType::RParen {
            self.pos = save;
            return Ok(None);
        }
        self.advance(); // consume ')'

        // Must be followed by ':='
        if self.peek().ty != TokenType::Colon {
            self.pos = save;
            return Ok(None);
        }
        self.advance(); // consume ':='

        let value = self.parse_expr()?;
        Ok(Some(AstNode::TupleBinding {
            names,
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
        let mut left = self.parse_range()?;
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

    fn parse_range(&mut self) -> Result<AstNode, String> {
        let left = self.parse_addition()?;
        if self.peek().ty == TokenType::DotDot {
            self.advance();
            let right = self.parse_addition()?;
            Ok(AstNode::Range {
                start: Box::new(left),
                end: Box::new(right),
            })
        } else {
            Ok(left)
        }
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
                    if !self.allow_ident_call {
                        break;
                    }
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
                    self.advance(); // consume '['
                    let index = self.parse_expr()?;
                    self.expect(TokenType::RBracket)?;
                    expr = AstNode::IndexAccess {
                        array: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                // Property access: expr.prop
                TokenType::Dot => {
                    self.advance(); // consume '.'
                    match self.peek().ty.clone() {
                        TokenType::Identifier(prop)
                        | TokenType::AxisVar(prop)
                        | TokenType::Builtin(prop) => {
                            self.advance();
                            expr = AstNode::PropertyAccess {
                                object: Box::new(expr),
                                property: prop,
                            };
                        }
                        _ => {
                            return Err(super::format_error_at(
                                &self.source,
                                self.peek().span.0,
                                &format!(
                                    "Expected property name after '.', found {}",
                                    self.peek().ty
                                ),
                            ))
                        }
                    }
                }
                // Unicode superscript exponents (², ³, ⁰-⁹)
                TokenType::Builtin(ref s) if is_superscript_token(s) => {
                    let exp = superscript_exponent(s);
                    self.advance();
                    expr = AstNode::Apply {
                        name: "pow".to_string(),
                        args: vec![expr, AstNode::Number(exp)],
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<AstNode, String> {
        self.enter()?;
        let result = self.parse_primary_inner();
        self.leave();
        result
    }

    fn parse_primary_inner(&mut self) -> Result<AstNode, String> {
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

                // Parse a block: comma or newline separated statements inside parens.
                // Could be: (expr), (a, b, c) tuple, or (binding, binding, result) block
                let mut items = vec![self.parse_block_item()?];
                self.skip_newlines();

                // Continue parsing items separated by commas and/or newlines
                while self.peek().ty != TokenType::RParen && !self.at_end() {
                    // Consume optional comma separator
                    if self.peek().ty == TokenType::Comma {
                        self.advance();
                    }
                    self.skip_newlines();
                    if self.peek().ty == TokenType::RParen {
                        break; // trailing comma/newline
                    }
                    items.push(self.parse_block_item()?);
                    self.skip_newlines();
                }
                self.expect(TokenType::RParen)?;

                if items.len() == 1 {
                    // Single parenthesized expression
                    Ok(items.pop().unwrap())
                } else {
                    // If any item is a Binding, FunctionDef, WhileLoop, IndexAssign, or TupleBinding, it's a block
                    let has_block_items = items.iter().any(|item| {
                        matches!(
                            item,
                            AstNode::Binding { .. }
                                | AstNode::FunctionDef { .. }
                                | AstNode::WhileLoop { .. }
                                | AstNode::IndexAssign { .. }
                                | AstNode::TupleBinding { .. }
                        )
                    });
                    if has_block_items {
                        Ok(AstNode::Block(items))
                    } else {
                        Ok(AstNode::Tuple(items))
                    }
                }
            }
            TokenType::If => {
                self.advance(); // consume 'if'
                let has_paren = self.peek().ty == TokenType::LParen;
                if has_paren {
                    self.advance();
                }
                let condition = self.parse_expr()?;
                if has_paren {
                    self.expect(TokenType::RParen)?;
                }

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
            // While loop: while(condition) body
            TokenType::While => {
                self.advance(); // consume 'while'
                self.expect(TokenType::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(TokenType::RParen)?;
                self.skip_newlines();
                let body = self.parse_expr()?;
                Ok(AstNode::WhileLoop {
                    condition: Box::new(condition),
                    body: Box::new(body),
                })
            }
            // For loop: for var in range (body) — sequential CPU
            TokenType::For => {
                self.advance(); // consume 'for'
                let var = match &self.peek().ty {
                    TokenType::Identifier(name) | TokenType::AxisVar(name) => {
                        let name = name.clone();
                        self.advance();
                        name
                    }
                    _ => {
                        return Err(super::format_error_at(
                            &self.source,
                            self.peek().span.0,
                            &format!(
                                "Expected loop variable after 'for', found {}",
                                self.peek().ty
                            ),
                        ))
                    }
                };
                self.expect(TokenType::In)?;
                self.allow_ident_call = false;
                let range = self.parse_expr()?;
                self.allow_ident_call = true;
                self.skip_newlines();
                self.expect(TokenType::LParen)?;
                self.skip_newlines();
                let mut stmts = Vec::new();
                while self.peek().ty != TokenType::RParen && !self.at_end() {
                    stmts.push(self.parse_block_item()?);
                    self.skip_newlines();
                    if self.peek().ty == TokenType::Comma {
                        self.advance();
                    }
                    self.skip_newlines();
                }
                self.expect(TokenType::RParen)?;
                let body = if stmts.len() == 1 {
                    stmts.pop().unwrap()
                } else {
                    AstNode::Block(stmts)
                };
                Ok(AstNode::ForLoop {
                    var,
                    range: Box::new(range),
                    body: Box::new(body),
                })
            }
            // Array literal: [expr, expr, ...]
            TokenType::LBracket => {
                self.advance(); // consume '['
                let mut elements = Vec::new();
                self.skip_newlines();
                if self.peek().ty != TokenType::RBracket {
                    elements.push(self.parse_expr()?);
                    while self.peek().ty == TokenType::Comma {
                        self.advance();
                        self.skip_newlines();
                        if self.peek().ty == TokenType::RBracket {
                            break; // trailing comma
                        }
                        elements.push(self.parse_expr()?);
                    }
                }
                self.expect(TokenType::RBracket)?;
                Ok(AstNode::ArrayLiteral(elements))
            }
            // Parallel for: parallel for var in range (body)
            TokenType::Parallel => {
                self.advance(); // consume 'parallel'
                self.expect(TokenType::For)?;
                let var = match &self.peek().ty {
                    TokenType::Identifier(name) | TokenType::AxisVar(name) => {
                        let name = name.clone();
                        self.advance();
                        name
                    }
                    _ => {
                        return Err(super::format_error_at(
                            &self.source,
                            self.peek().span.0,
                            &format!(
                                "Expected loop variable after 'parallel for', found {}",
                                self.peek().ty
                            ),
                        ))
                    }
                };
                self.expect(TokenType::In)?;
                // Disable identifier function calls so the body `(` isn't
                // consumed as a call on the last range token.
                self.allow_ident_call = false;
                let range = self.parse_expr()?;
                self.allow_ident_call = true;
                self.skip_newlines();
                // Body is a parenthesized block of statements
                self.expect(TokenType::LParen)?;
                self.skip_newlines();
                let mut stmts = Vec::new();
                while self.peek().ty != TokenType::RParen && !self.at_end() {
                    stmts.push(self.parse_block_item()?);
                    self.skip_newlines();
                    if self.peek().ty == TokenType::Comma {
                        self.advance();
                    }
                    self.skip_newlines();
                }
                self.expect(TokenType::RParen)?;
                let body = if stmts.len() == 1 {
                    stmts.pop().unwrap()
                } else {
                    AstNode::Block(stmts)
                };
                Ok(AstNode::ParallelFor {
                    var,
                    range: Box::new(range),
                    body: Box::new(body),
                })
            }
            // Type names as cast functions: f32(expr), vec2(a, b), etc.
            TokenType::TypeF32
            | TokenType::TypeF64
            | TokenType::TypeI32
            | TokenType::TypeVec2
            | TokenType::TypeVec3
            | TokenType::TypeVec4 => {
                let cast_name = match &self.peek().ty {
                    TokenType::TypeF32 => "f32",
                    TokenType::TypeF64 => "f64",
                    TokenType::TypeI32 => "i32",
                    TokenType::TypeVec2 => "vec2",
                    TokenType::TypeVec3 => "vec3",
                    TokenType::TypeVec4 => "vec4",
                    _ => unreachable!(),
                }
                .to_string();
                self.advance();

                if self.peek().ty == TokenType::LParen {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(TokenType::RParen)?;
                    Ok(AstNode::Apply {
                        name: cast_name,
                        args,
                    })
                } else {
                    Ok(AstNode::Identifier(cast_name))
                }
            }
            _ => Err(super::format_error_at(
                &self.source,
                self.peek().span.0,
                &format!("Unexpected token {}", self.peek().ty),
            )),
        }
    }

    /// Parse an item inside a parenthesized block (handles binding with `:=`).
    fn parse_block_item(&mut self) -> Result<AstNode, String> {
        // Try tuple destructuring binding: (a, b): expr
        if let Some(tb) = self.try_parse_tuple_binding()? {
            return Ok(tb);
        }
        // Try function def first
        if let Some(func) = self.try_parse_function_def()? {
            return Ok(func);
        }
        // Try binding
        if let Some(binding) = self.try_parse_binding()? {
            return Ok(binding);
        }
        let expr = self.parse_expr()?;
        self.try_index_assign(expr)
    }

    /// If `expr` is an IndexAccess and next token is `:=`, parse as IndexAssign.
    fn try_index_assign(&mut self, expr: AstNode) -> Result<AstNode, String> {
        if let AstNode::IndexAccess {
            ref array,
            ref index,
        } = expr
        {
            if self.peek().ty == TokenType::Colon {
                self.advance(); // consume ':='
                let value = self.parse_expr()?;
                return Ok(AstNode::IndexAssign {
                    array: array.clone(),
                    index: index.clone(),
                    value: Box::new(value),
                });
            }
        }
        Ok(expr)
    }

    fn parse_arg_list(&mut self) -> Result<Vec<AstNode>, String> {
        let mut args = Vec::new();
        self.skip_newlines();
        if self.peek().ty == TokenType::RParen {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        self.skip_newlines();
        while self.peek().ty == TokenType::Comma {
            self.advance();
            self.skip_newlines();
            args.push(self.parse_expr()?);
            self.skip_newlines();
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
            Err(super::format_error_at(
                &self.source,
                self.peek().span.0,
                &format!("Expected {}, found {}", expected, self.peek().ty),
            ))
        }
    }
}

const EOF_TOKEN: Token = Token {
    ty: TokenType::Eof,
    span: (0, 0),
};

/// Check if a Builtin token name is a Unicode superscript exponent.
fn is_superscript_token(name: &str) -> bool {
    matches!(
        name,
        "square" | "cube" | "pow0" | "pow1" | "pow4" | "pow5" | "pow6" | "pow7" | "pow8" | "pow9"
    )
}

/// Map a superscript Builtin token name to its numeric exponent.
fn superscript_exponent(name: &str) -> f64 {
    match name {
        "square" => 2.0,
        "cube" => 3.0,
        "pow0" => 0.0,
        "pow1" => 1.0,
        "pow4" => 4.0,
        "pow5" => 5.0,
        "pow6" => 6.0,
        "pow7" => 7.0,
        "pow8" => 8.0,
        "pow9" => 9.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::lexer::Lexer;

    fn parse(input: &str) -> AstNode {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input.to_string());
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
        // `:=` is the binding operator in Logos
        let ast = parse("a := 5");
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
        let ast = parse("f(x, y) := x + y");
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
        let ast = parse("a := 5\nb := 10\na + b");
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
        let ast = parse("(a := 5, b := 10, a + b)");
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
        let ast = parse("f(a, b) := (r := a + b, r * 2)");
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

    // -----------------------------------------------------------------------
    // Error-recovery tests — these inputs must NOT panic; they must return Err.
    // -----------------------------------------------------------------------

    fn try_parse(source: &str) -> Result<AstNode, String> {
        let mut lex = crate::lang::lexer::Lexer::new(source);
        let tokens = lex.tokenize()?;
        let mut parser = Parser::new(tokens, source.to_string());
        parser.parse()
    }

    #[test]
    fn err_unclosed_paren() {
        let err = try_parse("(1 + 2").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn err_unexpected_extra_token() {
        // Two consecutive numbers with no operator should fail.
        let err = try_parse("1 2 +").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn err_dangling_operator() {
        let err = try_parse("3 +").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn err_pathologically_deep_parens_does_not_overflow_stack() {
        // 1000 nested parens — should hit MAX_RECURSION_DEPTH and return Err
        // rather than stack-overflowing.
        let source = format!("{}{}{}", "(".repeat(1000), "1", ")".repeat(1000));
        let err = try_parse(&source).unwrap_err();
        assert!(err.contains("nested too deeply"), "got: {}", err);
    }

    #[test]
    fn err_function_def_missing_body() {
        let err = try_parse("f(x) :=").unwrap_err();
        assert!(!err.is_empty());
    }
}
