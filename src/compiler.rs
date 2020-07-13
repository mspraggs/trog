/* Copyright 2020 Matt Spraggs
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::chunk;
use crate::common;
use crate::debug;
use crate::scanner;
use crate::value;

#[derive(Copy, Clone)]
enum Precedence {
    None,
    Assignment,
    Or,
    And,
    Equality,
    Comparison,
    Term,
    Factor,
    Unary,
    Call,
    Primary,
}

impl From<usize> for Precedence {
    fn from(value: usize) -> Self {
        match value {
            value if value == Precedence::None as usize => Precedence::None,
            value if value == Precedence::Assignment as usize => {
                Precedence::Assignment
            }
            value if value == Precedence::Or as usize => Precedence::Or,
            value if value == Precedence::And as usize => Precedence::And,
            value if value == Precedence::Equality as usize => {
                Precedence::Equality
            }
            value if value == Precedence::Comparison as usize => {
                Precedence::Comparison
            }
            value if value == Precedence::Term as usize => Precedence::Term,
            value if value == Precedence::Factor as usize => Precedence::Factor,
            value if value == Precedence::Unary as usize => Precedence::Unary,
            value if value == Precedence::Call as usize => Precedence::Call,
            value if value == Precedence::Primary as usize => {
                Precedence::Primary
            }
            _ => panic!("Unknown precedence {}", value),
        }
    }
}

type ParseFn = fn(&mut Parser, bool) -> ();

#[derive(Copy, Clone)]
struct ParseRule {
    prefix: Option<ParseFn>,
    infix: Option<ParseFn>,
    precedence: Precedence,
}

#[derive(Default)]
struct Local {
    name: scanner::Token,
    depth: Option<usize>,
}

#[derive(Default)]
struct Compiler {
    locals: Vec<Local>,
    scope_depth: usize,
}

impl Compiler {
    fn new() -> Self {
        Default::default()
    }

    fn add_local(&mut self, name: &scanner::Token) -> bool {
        if self.locals.len() == common::MAX_LOCALS {
            return false;
        }

        self.locals.push(Local {
            name: name.clone(),
            depth: None,
        });

        return true;
    }
}

pub fn compile(source: String) -> Option<chunk::Chunk> {
    let mut chunk = chunk::Chunk::new();
    let compiler = Box::new(Compiler::new());
    let mut scanner = scanner::Scanner::from_source(source);

    let mut parser = Parser::new(&mut scanner, compiler, &mut chunk);
    if parser.parse() {
        if cfg!(debug_assertions) {
            debug::disassemble_chunk(&chunk, "code");
        }
        return Some(chunk);
    }

    None
}

struct Parser<'a> {
    current: scanner::Token,
    previous: scanner::Token,
    had_error: bool,
    panic_mode: bool,
    scanner: &'a mut scanner::Scanner,
    compiler: Box<Compiler>,
    chunk: &'a mut chunk::Chunk,
    rules: [ParseRule; 40],
}

impl<'a> Parser<'a> {
    fn new(
        scanner: &'a mut scanner::Scanner,
        compiler: Box<Compiler>,
        chunk: &'a mut chunk::Chunk,
    ) -> Parser<'a> {
        Parser {
            current: scanner::Token::new(),
            previous: scanner::Token::new(),
            had_error: false,
            panic_mode: false,
            scanner: scanner,
            compiler: compiler,
            chunk: chunk,
            rules: [
                // LeftParen
                ParseRule {
                    prefix: Some(Parser::grouping),
                    infix: None,
                    precedence: Precedence::None,
                },
                // RightParen
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // LeftBrace
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // RightBrace
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Comma
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Dot
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Minus
                ParseRule {
                    prefix: Some(Parser::unary),
                    infix: Some(Parser::binary),
                    precedence: Precedence::Term,
                },
                // Plus
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Term,
                },
                // SemiColon
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Slash
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Factor,
                },
                // Star
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Factor,
                },
                // Bang
                ParseRule {
                    prefix: Some(Parser::unary),
                    infix: None,
                    precedence: Precedence::None,
                },
                // BangEqual
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Equality,
                },
                // Equal
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // EqualEqual
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Equality,
                },
                // Greater
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Comparison,
                },
                // GreaterEqual
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Comparison,
                },
                // Less
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Comparison,
                },
                // LessEqual
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::binary),
                    precedence: Precedence::Comparison,
                },
                // Identifier
                ParseRule {
                    prefix: Some(Parser::variable),
                    infix: None,
                    precedence: Precedence::None,
                },
                // Str
                ParseRule {
                    prefix: Some(Parser::string),
                    infix: None,
                    precedence: Precedence::None,
                },
                // Number
                ParseRule {
                    prefix: Some(Parser::number),
                    infix: None,
                    precedence: Precedence::None,
                },
                // And
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::and),
                    precedence: Precedence::And,
                },
                // Class
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Else
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // False
                ParseRule {
                    prefix: Some(Parser::literal),
                    infix: None,
                    precedence: Precedence::None,
                },
                // For
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Fun
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // If
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Nil
                ParseRule {
                    prefix: Some(Parser::literal),
                    infix: None,
                    precedence: Precedence::None,
                },
                // Or
                ParseRule {
                    prefix: None,
                    infix: Some(Parser::or),
                    precedence: Precedence::Or,
                },
                // Print
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Return
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Super
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // This
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // True
                ParseRule {
                    prefix: Some(Parser::literal),
                    infix: None,
                    precedence: Precedence::None,
                },
                // Var
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // While
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Error
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
                // Eof
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
                },
            ],
        }
    }

    fn parse(&mut self) -> bool {
        self.advance();

        while !self.match_token(scanner::TokenKind::Eof) {
            self.declaration();
        }

        self.emit_return();
        !self.had_error
    }

    fn advance(&mut self) {
        self.previous = self.current.clone();

        loop {
            self.current = self.scanner.scan_token();
            match self.current.kind {
                scanner::TokenKind::Error => {}
                _ => {
                    break;
                }
            }

            let msg = self.current.source.clone();
            self.error_at_current(msg.as_str());
        }
    }

    fn consume(&mut self, kind: scanner::TokenKind, message: &str) {
        if self.current.kind == kind {
            self.advance();
            return;
        }
        self.error_at_current(message);
    }

    fn check(&self, kind: scanner::TokenKind) -> bool {
        self.current.kind == kind
    }

    fn match_token(&mut self, kind: scanner::TokenKind) -> bool {
        if !self.check(kind) {
            return false;
        }
        self.advance();
        true
    }

    fn expression(&mut self) {
        self.parse_precedence(Precedence::Assignment);
    }

    fn block(&mut self) {
        while !self.check(scanner::TokenKind::RightBrace)
            && !self.check(scanner::TokenKind::Eof)
        {
            self.declaration();
        }

        self.consume(
            scanner::TokenKind::RightBrace,
            "Expected '}' after block.",
        );
    }

    fn var_declaration(&mut self) {
        let global = self.parse_variable("Expected variable name.");

        if self.match_token(scanner::TokenKind::Equal) {
            self.expression();
        } else {
            self.emit_byte(chunk::OpCode::Nil as u8);
        }
        self.consume(
            scanner::TokenKind::SemiColon,
            "Expected ';' after variable declaration.",
        );

        self.define_variable(global);
    }

    fn expression_statement(&mut self) {
        self.expression();
        self.consume(
            scanner::TokenKind::SemiColon,
            "Expected ';' after expression.",
        );
        self.emit_byte(chunk::OpCode::Pop as u8);
    }

    fn for_statement(&mut self) {
        self.begin_scope();

        self.consume(
            scanner::TokenKind::LeftParen,
            "Expected '(' after 'for'.",
        );
        if self.match_token(scanner::TokenKind::SemiColon) {
            // No initialiser
        } else if self.match_token(scanner::TokenKind::Var) {
            self.var_declaration();
        } else {
            self.expression_statement();
        }

        let mut loop_start = self.chunk.code.len();

        let mut exit_jump: Option<usize> = None;

        if !self.match_token(scanner::TokenKind::SemiColon) {
            self.expression();
            self.consume(
                scanner::TokenKind::SemiColon,
                "Expected ';' after loop condition.",
            );

            // We'll need to jump out of the loop if the condition is false, so
            // we add a conditional jump here.
            exit_jump = Some(self.emit_jump(chunk::OpCode::JumpIfFalse));
            self.emit_byte(chunk::OpCode::Pop as u8);
        }

        if !self.match_token(scanner::TokenKind::RightParen) {
            let body_jump = self.emit_jump(chunk::OpCode::Jump);

            let increment_start = self.chunk.code.len();
            self.expression();
            self.emit_byte(chunk::OpCode::Pop as u8);
            self.consume(
                scanner::TokenKind::RightParen,
                "Expected ')' after for clauses.",
            );

            self.emit_loop(loop_start);
            loop_start = increment_start;
            self.patch_jump(body_jump);
        }

        self.statement();

        self.emit_loop(loop_start);

        if let Some(offset) = exit_jump {
            self.patch_jump(offset);
            self.emit_byte(chunk::OpCode::Pop as u8);
        }

        self.end_scope();
    }

    fn if_statement(&mut self) {
        self.consume(scanner::TokenKind::LeftParen, "Expected '(' after 'if'.");
        self.expression();
        self.consume(
            scanner::TokenKind::RightParen,
            "Expected ')' after condition.",
        );

        let then_jump = self.emit_jump(chunk::OpCode::JumpIfFalse);
        self.emit_byte(chunk::OpCode::Pop as u8);
        self.statement();

        let else_jump = self.emit_jump(chunk::OpCode::Jump);

        self.patch_jump(then_jump);
        self.emit_byte(chunk::OpCode::Pop as u8);

        if self.match_token(scanner::TokenKind::Else) {
            self.statement();
        }
        self.patch_jump(else_jump);
    }

    fn print_statement(&mut self) {
        self.expression();
        self.consume(
            scanner::TokenKind::SemiColon,
            "Expected ';' after value.",
        );
        self.emit_byte(chunk::OpCode::Print as u8);
    }

    fn while_statement(&mut self) {
        let loop_start = self.chunk.code.len();

        self.consume(
            scanner::TokenKind::LeftParen,
            "'Expected '(' after 'while'.",
        );
        self.expression();
        self.consume(
            scanner::TokenKind::RightParen,
            "'Expected ')' after expression.",
        );

        let exit_jump = self.emit_jump(chunk::OpCode::JumpIfFalse);

        self.emit_byte(chunk::OpCode::Pop as u8);
        self.statement();

        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit_byte(chunk::OpCode::Pop as u8);
    }

    fn synchronise(&mut self) {
        self.panic_mode = false;

        while self.current.kind != scanner::TokenKind::Eof {
            if self.previous.kind == scanner::TokenKind::SemiColon {
                return;
            }

            match self.current.kind {
                scanner::TokenKind::Class => return,
                scanner::TokenKind::Fun => return,
                scanner::TokenKind::Var => return,
                scanner::TokenKind::For => return,
                scanner::TokenKind::If => return,
                scanner::TokenKind::While => return,
                scanner::TokenKind::Print => return,
                scanner::TokenKind::Return => return,
                _ => {}
            }

            self.advance();
        }
    }

    fn begin_scope(&mut self) {
        self.compiler.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.compiler.scope_depth -= 1;

        loop {
            match self.compiler.locals.last() {
                Some(local) => {
                    if local.depth.unwrap() <= self.compiler.scope_depth {
                        break;
                    }
                }
                None => {
                    break;
                }
            }

            self.emit_byte(chunk::OpCode::Pop as u8);
            self.compiler.locals.pop();
        }
    }

    fn statement(&mut self) {
        if self.match_token(scanner::TokenKind::Print) {
            self.print_statement();
        } else if self.match_token(scanner::TokenKind::For) {
            self.for_statement();
        } else if self.match_token(scanner::TokenKind::If) {
            self.if_statement();
        } else if self.match_token(scanner::TokenKind::While) {
            self.while_statement();
        } else if self.match_token(scanner::TokenKind::LeftBrace) {
            self.begin_scope();
            self.block();
            self.end_scope();
        } else {
            self.expression_statement();
        }
    }

    fn declaration(&mut self) {
        if self.match_token(scanner::TokenKind::Var) {
            self.var_declaration();
        } else {
            self.statement();
        }

        if self.panic_mode {
            self.synchronise();
        }
    }

    fn emit_byte(&mut self, byte: u8) {
        self.chunk.write(byte, self.previous.line as i32);
    }

    fn emit_bytes(&mut self, bytes: [u8; 2]) {
        self.emit_byte(bytes[0]);
        self.emit_byte(bytes[1]);
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.emit_byte(chunk::OpCode::Loop as u8);

        let offset = self.chunk.code.len() - loop_start + 2;
        if offset > common::MAX_JUMP_SIZE {
            self.error("Loop body too large.");
        }

        self.emit_byte(((offset >> 8) & 0xff) as u8);
        self.emit_byte((offset & 0xff) as u8);
    }

    fn emit_jump(&mut self, instruction: chunk::OpCode) -> usize {
        self.emit_byte(instruction as u8);
        self.emit_bytes([0xff, 0xff]);
        self.chunk.code.len() - 2
    }

    fn emit_return(&mut self) {
        self.emit_byte(chunk::OpCode::Return as u8);
    }

    fn make_constant(&mut self, value: value::Value) -> u8 {
        let constant = self.chunk.add_constant(value);
        if constant > u8::MAX as usize {
            self.error("Too many constants in one chunk.");
            return 0;
        }
        constant as u8
    }

    fn emit_constant(&mut self, value: value::Value) {
        let constant = self.make_constant(value);
        self.emit_bytes([chunk::OpCode::Constant as u8, constant]);
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump = self.chunk.code.len() - offset - 2;

        if jump > common::MAX_JUMP_SIZE {
            self.error("Too much code to jump over.");
        }

        self.chunk.code[offset] = ((jump >> 8) & 0xff) as u8;
        self.chunk.code[offset + 1] = (jump & 0xff) as u8;
    }

    fn parse_precedence(&mut self, precedence: Precedence) {
        self.advance();
        let kind = self.previous.kind;
        let prefix_rule = self.get_rule(kind).prefix;
        let can_assign = precedence as usize <= Precedence::Assignment as usize;

        match prefix_rule {
            Some(ref handler) => handler(self, can_assign),
            None => {
                self.error("Expected expression.");
                return;
            }
        }

        while precedence as usize
            <= self.get_rule(self.current.kind).precedence as usize
        {
            self.advance();
            let infix_rule = self.get_rule(self.previous.kind).infix;
            infix_rule.unwrap()(self, can_assign);
        }

        if can_assign && self.match_token(scanner::TokenKind::Equal) {
            self.error("Invalid assignment target.");
        }
    }

    fn identifier_constant(&mut self, token: &scanner::Token) -> u8 {
        self.make_constant(value::Value::from(token.source.clone()))
    }

    fn declare_variable(&mut self) {
        if self.compiler.scope_depth == 0 {
            return;
        }

        let mut error_locations: Vec<scanner::Token> = Vec::new();

        for local in self.compiler.locals.iter().rev() {
            match local.depth {
                Some(value) => {
                    if value < self.compiler.scope_depth {
                        break;
                    }
                }
                None => {}
            }

            if self.previous.source == local.name.source {
                error_locations.push(self.previous.clone());
            }
        }

        for token in error_locations {
            self.error_at(
                token,
                "Variable with this name already declared in this scope.",
            );
        }

        if !self.compiler.add_local(&self.previous) {
            self.error("Too many variables in function.");
        }
    }

    fn parse_variable(&mut self, error_message: &str) -> u8 {
        self.consume(scanner::TokenKind::Identifier, error_message);

        self.declare_variable();
        if self.compiler.scope_depth > 0 {
            return 0;
        }

        let name = self.previous.clone();
        self.identifier_constant(&name)
    }

    fn mark_initialised(&mut self) {
        self.compiler.locals.last_mut().unwrap().depth =
            Some(self.compiler.scope_depth);
    }

    fn define_variable(&mut self, global: u8) {
        if self.compiler.scope_depth > 0 {
            self.mark_initialised();
            return;
        }

        self.emit_bytes([chunk::OpCode::DefineGlobal as u8, global]);
    }

    fn get_rule(&self, kind: scanner::TokenKind) -> &ParseRule {
        &self.rules[kind as usize]
    }

    fn error_at_current(&mut self, message: &str) {
        self.error_at(self.current.clone(), message);
    }

    fn error(&mut self, message: &str) {
        self.error_at(self.previous.clone(), message);
    }

    fn error_at(&mut self, token: scanner::Token, message: &str) {
        if self.panic_mode {
            return;
        }
        self.panic_mode = true;

        eprint!("[line {}] Error", token.line);

        match token.kind {
            scanner::TokenKind::Eof => eprint!(" at end"),
            scanner::TokenKind::Error => {}
            _ => eprint!(" at '{}'", token.source),
        };

        eprintln!(": {}", message);
        self.had_error = true;
    }

    fn resolve_local(&mut self, name: &scanner::Token) -> Option<u8> {
        for (i, local) in self.compiler.locals.iter().enumerate().rev() {
            if local.name.source == name.source {
                if local.depth.is_none() {
                    self.error("Cannot read variable in its own initialiser.");
                }
                return Some(i as u8);
            }
        }

        None
    }

    fn named_variable(&mut self, name: scanner::Token, can_assign: bool) {
        let (get_op, set_op, arg): (chunk::OpCode, chunk::OpCode, u8) =
            (|| {
                if let Some(result) = self.resolve_local(&name) {
                    return (
                        chunk::OpCode::GetLocal,
                        chunk::OpCode::SetLocal,
                        result,
                    );
                } else {
                    return (
                        chunk::OpCode::GetGlobal,
                        chunk::OpCode::SetGlobal,
                        self.identifier_constant(&name),
                    );
                }
            })();

        if can_assign && self.match_token(scanner::TokenKind::Equal) {
            self.expression();
            self.emit_bytes([set_op as u8, arg]);
        } else {
            self.emit_bytes([get_op as u8, arg]);
        }
    }

    fn grouping(s: &mut Parser, _can_assign: bool) {
        s.expression();
        s.consume(
            scanner::TokenKind::RightParen,
            "Expected ')' after expression.",
        );
    }

    fn binary(s: &mut Parser, _can_assign: bool) {
        let operator_kind = s.previous.kind;
        let rule_precedence = s.get_rule(operator_kind).precedence;
        s.parse_precedence(Precedence::from(rule_precedence as usize + 1));

        match operator_kind {
            scanner::TokenKind::BangEqual => s.emit_bytes([
                chunk::OpCode::Equal as u8,
                chunk::OpCode::Not as u8,
            ]),
            scanner::TokenKind::EqualEqual => {
                s.emit_byte(chunk::OpCode::Equal as u8)
            }
            scanner::TokenKind::Greater => {
                s.emit_byte(chunk::OpCode::Greater as u8)
            }
            scanner::TokenKind::GreaterEqual => s.emit_bytes([
                chunk::OpCode::Less as u8,
                chunk::OpCode::Not as u8,
            ]),
            scanner::TokenKind::Less => s.emit_byte(chunk::OpCode::Less as u8),
            scanner::TokenKind::LessEqual => s.emit_bytes([
                chunk::OpCode::Greater as u8,
                chunk::OpCode::Not as u8,
            ]),
            scanner::TokenKind::Plus => s.emit_byte(chunk::OpCode::Add as u8),
            scanner::TokenKind::Minus => {
                s.emit_byte(chunk::OpCode::Subtract as u8)
            }
            scanner::TokenKind::Star => {
                s.emit_byte(chunk::OpCode::Multiply as u8)
            }
            scanner::TokenKind::Slash => {
                s.emit_byte(chunk::OpCode::Divide as u8)
            }
            _ => {}
        }
    }

    fn unary(s: &mut Parser, _can_assign: bool) {
        let operator_kind = s.previous.kind;
        s.parse_precedence(Precedence::Unary);

        match operator_kind {
            scanner::TokenKind::Minus => {
                s.emit_byte(chunk::OpCode::Negate as u8)
            }
            scanner::TokenKind::Bang => s.emit_byte(chunk::OpCode::Not as u8),
            _ => {}
        }
    }

    fn number(s: &mut Parser, _can_assign: bool) {
        let value = s.previous.source.as_str().parse::<f64>().unwrap();
        s.emit_constant(value::Value::Number(value));
    }

    fn literal(s: &mut Parser, _can_assign: bool) {
        match s.previous.kind {
            scanner::TokenKind::False => {
                s.emit_byte(chunk::OpCode::False as u8);
            }
            scanner::TokenKind::Nil => {
                s.emit_byte(chunk::OpCode::Nil as u8);
            }
            scanner::TokenKind::True => {
                s.emit_byte(chunk::OpCode::True as u8);
            }
            _ => {}
        }
    }

    fn string(s: &mut Parser, _can_assign: bool) {
        s.emit_constant(value::Value::from(
            &s.previous.source[1..s.previous.source.len() - 1],
        ));
    }

    fn variable(s: &mut Parser, can_assign: bool) {
        s.named_variable(s.previous.clone(), can_assign);
    }

    fn and(s: &mut Parser, _can_assign: bool) {
        let end_jump = s.emit_jump(chunk::OpCode::JumpIfFalse);

        s.emit_byte(chunk::OpCode::Pop as u8);
        s.parse_precedence(Precedence::And);

        s.patch_jump(end_jump);
    }

    fn or(s: &mut Parser, _can_assign: bool) {
        let else_jump = s.emit_jump(chunk::OpCode::JumpIfFalse);
        let end_jump = s.emit_jump(chunk::OpCode::Jump);

        s.patch_jump(else_jump);
        s.emit_byte(chunk::OpCode::Pop as u8);

        s.parse_precedence(Precedence::Or);
        s.patch_jump(end_jump);
    }
}
