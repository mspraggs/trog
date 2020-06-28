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

type ParseFn = fn(&mut Compiler, bool) -> ();

#[derive(Copy, Clone)]
struct ParseRule {
    prefix: Option<ParseFn>,
    infix: Option<ParseFn>,
    precedence: Precedence,
}

pub fn compile(source: String) -> Option<chunk::Chunk> {
    let mut chunk = chunk::Chunk::new();
    let mut scanner = scanner::Scanner::from_source(source);

    let mut compiler =
        Compiler::with_scanner_and_chunk(&mut scanner, &mut chunk);
    if compiler.compile() {
        if cfg!(debug_assertions) {
            debug::disassemble_chunk(&chunk, "code");
        }
        return Some(chunk);
    }

    None
}

struct Compiler<'a> {
    current: scanner::Token,
    previous: scanner::Token,
    had_error: bool,
    panic_mode: bool,
    scanner: &'a mut scanner::Scanner,
    chunk: &'a mut chunk::Chunk,
    rules: [ParseRule; 40],
}

impl<'a> Compiler<'a> {
    fn with_scanner_and_chunk(
        scanner: &'a mut scanner::Scanner,
        chunk: &'a mut chunk::Chunk,
    ) -> Compiler<'a> {
        Compiler {
            current: scanner::Token::new(),
            previous: scanner::Token::new(),
            had_error: false,
            panic_mode: false,
            scanner: scanner,
            chunk: chunk,
            rules: [
                // LeftParen
                ParseRule {
                    prefix: Some(Compiler::grouping),
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
                    prefix: Some(Compiler::unary),
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Term,
                },
                // Plus
                ParseRule {
                    prefix: None,
                    infix: Some(Compiler::binary),
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
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Factor,
                },
                // Star
                ParseRule {
                    prefix: None,
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Factor,
                },
                // Bang
                ParseRule {
                    prefix: Some(Compiler::unary),
                    infix: None,
                    precedence: Precedence::None,
                },
                // BangEqual
                ParseRule {
                    prefix: None,
                    infix: Some(Compiler::binary),
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
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Equality,
                },
                // Greater
                ParseRule {
                    prefix: None,
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Comparison,
                },
                // GreaterEqual
                ParseRule {
                    prefix: None,
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Comparison,
                },
                // Less
                ParseRule {
                    prefix: None,
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Comparison,
                },
                // LessEqual
                ParseRule {
                    prefix: None,
                    infix: Some(Compiler::binary),
                    precedence: Precedence::Comparison,
                },
                // Identifier
                ParseRule {
                    prefix: Some(Compiler::variable),
                    infix: None,
                    precedence: Precedence::None,
                },
                // Str
                ParseRule {
                    prefix: Some(Compiler::string),
                    infix: None,
                    precedence: Precedence::None,
                },
                // Number
                ParseRule {
                    prefix: Some(Compiler::number),
                    infix: None,
                    precedence: Precedence::None,
                },
                // And
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
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
                    prefix: Some(Compiler::literal),
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
                    prefix: Some(Compiler::literal),
                    infix: None,
                    precedence: Precedence::None,
                },
                // Or
                ParseRule {
                    prefix: None,
                    infix: None,
                    precedence: Precedence::None,
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
                    prefix: Some(Compiler::literal),
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

    fn compile(&mut self) -> bool {
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

    fn print_statement(&mut self) {
        self.expression();
        self.consume(
            scanner::TokenKind::SemiColon,
            "Expected ';' after value.",
        );
        self.emit_byte(chunk::OpCode::Print as u8);
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

    fn statement(&mut self) {
        if self.match_token(scanner::TokenKind::Print) {
            self.print_statement();
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

    fn parse_variable(&mut self, error_message: &str) -> u8 {
        self.consume(scanner::TokenKind::Identifier, error_message);
        let name = self.previous.clone();
        self.identifier_constant(&name)
    }

    fn define_variable(&mut self, global: u8) {
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

    fn named_variable(&mut self, name: scanner::Token, can_assign: bool) {
        let arg = self.identifier_constant(&name);
        if can_assign && self.match_token(scanner::TokenKind::Equal) {
            self.expression();
            self.emit_bytes([chunk::OpCode::SetGlobal as u8, arg]);
        } else {
            self.emit_bytes([chunk::OpCode::GetGlobal as u8, arg]);
        }
    }

    fn grouping(s: &mut Compiler, _can_assign: bool) {
        s.expression();
        s.consume(
            scanner::TokenKind::RightParen,
            "Expected ')' after expression.",
        );
    }

    fn binary(s: &mut Compiler, _can_assign: bool) {
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

    fn unary(s: &mut Compiler, _can_assign: bool) {
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

    fn number(s: &mut Compiler, _can_assign: bool) {
        let value = s.previous.source.as_str().parse::<f64>().unwrap();
        s.emit_constant(value::Value::Number(value));
    }

    fn literal(s: &mut Compiler, _can_assign: bool) {
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

    fn string(s: &mut Compiler, _can_assign: bool) {
        s.emit_constant(value::Value::from(
            &s.previous.source[1..s.previous.source.len() - 1],
        ));
    }

    fn variable(s: &mut Compiler, can_assign: bool) {
        s.named_variable(s.previous.clone(), can_assign);
    }
}
