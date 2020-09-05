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

use std::mem;

use crate::chunk;
use crate::common;
use crate::debug;
use crate::memory;
use crate::object;
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
            value if value == Precedence::Assignment as usize => Precedence::Assignment,
            value if value == Precedence::Or as usize => Precedence::Or,
            value if value == Precedence::And as usize => Precedence::And,
            value if value == Precedence::Equality as usize => Precedence::Equality,
            value if value == Precedence::Comparison as usize => Precedence::Comparison,
            value if value == Precedence::Term as usize => Precedence::Term,
            value if value == Precedence::Factor as usize => Precedence::Factor,
            value if value == Precedence::Unary as usize => Precedence::Unary,
            value if value == Precedence::Call as usize => Precedence::Call,
            value if value == Precedence::Primary as usize => Precedence::Primary,
            _ => panic!("Unknown precedence {}", value),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum FunctionKind {
    Function,
    Initialiser,
    Method,
    Script,
}

impl Default for FunctionKind {
    fn default() -> Self {
        FunctionKind::Script
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
    is_captured: bool,
}

#[derive(Default)]
struct Upvalue {
    index: u8,
    is_local: bool,
}

#[derive(Default)]
struct Compiler {
    function: memory::Root<object::ObjFunction>,
    kind: FunctionKind,

    locals: Vec<Local>,
    upvalues: Vec<Upvalue>,
    scope_depth: usize,
}

enum CompilerError {
    InvalidCompilerKind,
    LocalNotFound,
    ReadVarInInitialiser,
    TooManyClosureVars,
}

impl Compiler {
    fn new(kind: FunctionKind, name: String) -> Self {
        let name = memory::allocate(object::ObjString::new(name));
        Compiler {
            function: memory::allocate(object::ObjFunction::new(name.as_gc())),
            kind: kind,
            locals: vec![Local {
                name: scanner::Token::from_string(if kind != FunctionKind::Function {
                    "this"
                } else {
                    ""
                }),
                depth: Some(0),
                is_captured: false,
            }],
            upvalues: Vec::new(),
            scope_depth: 0,
        }
    }

    fn add_local(&mut self, name: &scanner::Token) -> bool {
        if self.locals.len() == common::LOCALS_MAX {
            return false;
        }

        self.locals.push(Local {
            name: name.clone(),
            depth: None,
            is_captured: false,
        });

        return true;
    }

    fn resolve_local(&self, name: &scanner::Token) -> Result<u8, CompilerError> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name.source == name.source {
                if local.depth.is_none() {
                    return Err(CompilerError::ReadVarInInitialiser);
                }
                return Ok(i as u8);
            }
        }

        Err(CompilerError::LocalNotFound)
    }

    fn add_upvalue(&mut self, index: u8, is_local: bool) -> Result<u8, CompilerError> {
        let upvalue_count = self.upvalues.len();

        for (i, upvalue) in self.upvalues.iter().enumerate() {
            if upvalue.index == index && upvalue.is_local == is_local {
                return Ok(i as u8);
            }
        }

        if upvalue_count == common::UPVALUES_MAX {
            return Err(CompilerError::TooManyClosureVars);
        }

        self.upvalues.push(Upvalue {
            index: index,
            is_local: is_local,
        });
        Ok(upvalue_count as u8)
    }
}

struct ClassCompiler {
    name: scanner::Token,
    has_superclass: bool,
}

pub fn compile(source: String) -> Option<memory::Root<object::ObjFunction>> {
    let mut scanner = scanner::Scanner::from_source(source);

    let mut parser = Parser::new(&mut scanner);
    parser.parse()
}

struct Parser<'a> {
    current: scanner::Token,
    previous: scanner::Token,
    had_error: bool,
    panic_mode: bool,
    scanner: &'a mut scanner::Scanner,
    compilers: Vec<Compiler>,
    class_compilers: Vec<ClassCompiler>,
    rules: [ParseRule; 40],
}

impl<'a> Parser<'a> {
    fn new(scanner: &'a mut scanner::Scanner) -> Parser<'a> {
        Parser {
            current: scanner::Token::new(),
            previous: scanner::Token::new(),
            had_error: false,
            panic_mode: false,
            scanner: scanner,
            compilers: vec![Compiler::new(FunctionKind::Script, String::from(""))],
            class_compilers: Vec::new(),
            rules: [
                // LeftParen
                ParseRule {
                    prefix: Some(Parser::grouping),
                    infix: Some(Parser::call),
                    precedence: Precedence::Call,
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
                    infix: Some(Parser::dot),
                    precedence: Precedence::Call,
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
                    prefix: Some(Parser::super_),
                    infix: None,
                    precedence: Precedence::None,
                },
                // This
                ParseRule {
                    prefix: Some(Parser::this),
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

    fn parse(&mut self) -> Option<memory::Root<object::ObjFunction>> {
        self.advance();

        while !self.match_token(scanner::TokenKind::Eof) {
            self.declaration();
        }

        if self.had_error {
            return None;
        }

        Some(self.finalise_compiler().0)
    }

    fn advance(&mut self) {
        self.previous = self.current.clone();

        loop {
            self.current = self.scanner.scan_token();
            if self.current.kind != scanner::TokenKind::Error {
                break;
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
        while !self.check(scanner::TokenKind::RightBrace) && !self.check(scanner::TokenKind::Eof) {
            self.declaration();
        }

        self.consume(scanner::TokenKind::RightBrace, "Expected '}' after block.");
    }

    fn new_compiler(&mut self, kind: FunctionKind) {
        self.compilers
            .push(Compiler::new(kind, self.previous.source.clone()));
    }

    fn finalise_compiler(&mut self) -> (memory::Root<object::ObjFunction>, Compiler) {
        self.emit_return();

        if cfg!(feature = "debug_bytecode") && !self.had_error {
            let func_name = format!("{}", *self.compiler().function);
            debug::disassemble_chunk(&self.compiler().function.chunk, func_name.as_str());
        }

        let upvalue_count = self.compiler().upvalues.len();
        self.compiler_mut().function.upvalue_count = upvalue_count;

        // TODO: Use a Vec of compilers here to simplify this logic.
        let mut function: memory::Root<object::ObjFunction> = Default::default();
        let mut compiler = self.compilers.pop().unwrap();
        mem::swap(&mut function, &mut compiler.function);

        (function, compiler)
    }

    fn function(&mut self, kind: FunctionKind) {
        self.new_compiler(kind);
        self.begin_scope();

        self.consume(
            scanner::TokenKind::LeftParen,
            "Expected '(' after function name.",
        );
        if !self.check(scanner::TokenKind::RightParen) {
            loop {
                self.compiler_mut().function.arity += 1;
                if self.compiler().function.arity > 255 {
                    self.error_at_current("Cannot have more than 255 parameters.");
                }

                let param_constant = self.parse_variable("Expected parameter name.");
                self.define_variable(param_constant);

                if !self.match_token(scanner::TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(
            scanner::TokenKind::RightParen,
            "Expected ')' after parameters.",
        );

        self.consume(
            scanner::TokenKind::LeftBrace,
            "Expected '{' before function body.",
        );
        self.block();

        let (function, compiler) = self.finalise_compiler();

        let constant = self.make_constant(value::Value::from(function.as_gc()));
        self.emit_bytes([chunk::OpCode::Closure as u8, constant]);

        for upvalue in compiler.upvalues.iter() {
            self.emit_byte(upvalue.is_local as u8);
            self.emit_byte(upvalue.index as u8);
        }
    }

    fn method(&mut self) {
        self.consume(scanner::TokenKind::Identifier, "Expected method name.");
        let previous = self.previous.clone();
        let constant = self.identifier_constant(&previous);

        let kind = if self.previous.source == "init" {
            FunctionKind::Initialiser
        } else {
            FunctionKind::Method
        };
        self.function(kind);
        self.emit_bytes([chunk::OpCode::Method as u8, constant]);
    }

    fn class_declaration(&mut self) {
        self.consume(scanner::TokenKind::Identifier, "Expected class name.");
        let name = self.previous.clone();
        let name_constant = self.identifier_constant(&name);
        self.declare_variable();

        self.emit_bytes([chunk::OpCode::Class as u8, name_constant]);
        self.define_variable(name_constant);

        self.class_compilers.push(ClassCompiler {
            name: self.previous.clone(),
            has_superclass: false,
        });

        if self.match_token(scanner::TokenKind::Less) {
            self.consume(scanner::TokenKind::Identifier, "Expected superclass name.");
            Parser::variable(self, false);

            if name.source == self.previous.source {
                self.error("A class cannot inherit from iteself.");
            }

            self.begin_scope();
            self.compiler_mut()
                .add_local(&scanner::Token::from_string("super"));
            self.define_variable(0);

            self.named_variable(name.clone(), false);
            self.emit_byte(chunk::OpCode::Inherit as u8);
            self.class_compilers.last_mut().unwrap().has_superclass = true;
        }

        self.named_variable(name, false);
        self.consume(
            scanner::TokenKind::LeftBrace,
            "Expected '{' before class body.",
        );
        while !self.check(scanner::TokenKind::RightBrace) && !self.check(scanner::TokenKind::Eof) {
            self.method();
        }
        self.consume(
            scanner::TokenKind::RightBrace,
            "Expected '}' after class body.",
        );
        self.emit_byte(chunk::OpCode::Pop as u8);

        if self.class_compilers.last().unwrap().has_superclass {
            self.end_scope();
        }

        self.class_compilers.pop();
    }

    fn fun_declaration(&mut self) {
        let global = self.parse_variable("Expected function name.");
        self.mark_initialised();
        self.function(FunctionKind::Function);
        self.define_variable(global);
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

        self.consume(scanner::TokenKind::LeftParen, "Expected '(' after 'for'.");
        if self.match_token(scanner::TokenKind::SemiColon) {
            // No initialiser
        } else if self.match_token(scanner::TokenKind::Var) {
            self.var_declaration();
        } else {
            self.expression_statement();
        }

        let mut loop_start = self.chunk().code.len();

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

            let increment_start = self.chunk().code.len();
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
        self.consume(scanner::TokenKind::SemiColon, "Expected ';' after value.");
        self.emit_byte(chunk::OpCode::Print as u8);
    }

    fn return_statement(&mut self) {
        if self.compiler().kind == FunctionKind::Script {
            self.error("Cannot return from top-level code.");
        }
        if self.match_token(scanner::TokenKind::SemiColon) {
            self.emit_return();
        } else {
            if self.compiler().kind == FunctionKind::Initialiser {
                self.error("Cannot return a value from an initialiser.");
            }
            self.expression();
            self.consume(
                scanner::TokenKind::SemiColon,
                "Expected ';' after return value.",
            );
            self.emit_byte(chunk::OpCode::Return as u8);
        }
    }

    fn while_statement(&mut self) {
        let loop_start = self.chunk().code.len();

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
        self.compiler_mut().scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.compiler_mut().scope_depth -= 1;

        loop {
            let scope_depth = self.compiler().scope_depth;
            let opcode = match self.compiler().locals.last() {
                Some(local) => {
                    if local.depth.unwrap() <= scope_depth {
                        return;
                    }
                    if local.is_captured {
                        chunk::OpCode::CloseUpvalue
                    } else {
                        chunk::OpCode::Pop
                    }
                }
                None => {
                    return;
                }
            };

            self.emit_byte(opcode as u8);
            self.compiler_mut().locals.pop();
        }
    }

    fn statement(&mut self) {
        if self.match_token(scanner::TokenKind::Print) {
            self.print_statement();
        } else if self.match_token(scanner::TokenKind::For) {
            self.for_statement();
        } else if self.match_token(scanner::TokenKind::If) {
            self.if_statement();
        } else if self.match_token(scanner::TokenKind::Return) {
            self.return_statement();
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
        if self.match_token(scanner::TokenKind::Class) {
            self.class_declaration();
        } else if self.match_token(scanner::TokenKind::Fun) {
            self.fun_declaration();
        } else if self.match_token(scanner::TokenKind::Var) {
            self.var_declaration();
        } else {
            self.statement();
        }

        if self.panic_mode {
            self.synchronise();
        }
    }

    fn emit_byte(&mut self, byte: u8) {
        let line = self.previous.line as i32;
        self.chunk().write(byte, line);
    }

    fn emit_bytes(&mut self, bytes: [u8; 2]) {
        self.emit_byte(bytes[0]);
        self.emit_byte(bytes[1]);
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.emit_byte(chunk::OpCode::Loop as u8);

        let offset = self.chunk().code.len() - loop_start + 2;
        if offset > common::JUMP_SIZE_MAX {
            self.error("Loop body too large.");
        }

        self.emit_byte(((offset >> 8) & 0xff) as u8);
        self.emit_byte((offset & 0xff) as u8);
    }

    fn emit_jump(&mut self, instruction: chunk::OpCode) -> usize {
        self.emit_byte(instruction as u8);
        self.emit_bytes([0xff, 0xff]);
        self.chunk().code.len() - 2
    }

    fn emit_return(&mut self) {
        if self.compiler().kind == FunctionKind::Initialiser {
            self.emit_bytes([chunk::OpCode::GetLocal as u8, 0]);
        } else {
            self.emit_byte(chunk::OpCode::Nil as u8);
        }
        self.emit_byte(chunk::OpCode::Return as u8);
    }

    fn make_constant(&mut self, value: value::Value) -> u8 {
        let constant = self.chunk().add_constant(value);
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
        let jump = self.chunk().code.len() - offset - 2;

        if jump > common::JUMP_SIZE_MAX {
            self.error("Too much code to jump over.");
        }

        self.chunk().code[offset] = ((jump >> 8) & 0xff) as u8;
        self.chunk().code[offset + 1] = (jump & 0xff) as u8;
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

        while precedence as usize <= self.get_rule(self.current.kind).precedence as usize {
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
        let scope_depth = self.compiler().scope_depth;
        if scope_depth == 0 {
            return;
        }

        let mut error_locations: Vec<scanner::Token> = Vec::new();

        for local in self.compilers.last().unwrap().locals.iter().rev() {
            match local.depth {
                Some(value) => {
                    if value < scope_depth {
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

        if !self.compilers.last_mut().unwrap().add_local(&self.previous) {
            self.error("Too many variables in function.");
        }
    }

    fn parse_variable(&mut self, error_message: &str) -> u8 {
        self.consume(scanner::TokenKind::Identifier, error_message);

        self.declare_variable();
        if self.compiler().scope_depth > 0 {
            return 0;
        }

        let name = self.previous.clone();
        self.identifier_constant(&name)
    }

    fn mark_initialised(&mut self) {
        if self.compiler().scope_depth == 0 {
            return;
        }
        self.compiler_mut().locals.last_mut().unwrap().depth = Some(self.compiler().scope_depth);
    }

    fn define_variable(&mut self, global: u8) {
        if self.compiler().scope_depth > 0 {
            self.mark_initialised();
            return;
        }

        self.emit_bytes([chunk::OpCode::DefineGlobal as u8, global]);
    }

    fn argument_list(&mut self) -> u8 {
        let mut arg_count: u8 = 0;
        if !self.check(scanner::TokenKind::RightParen) {
            loop {
                self.expression();
                if arg_count == 255 {
                    self.error("Cannot have more than 255 arguments.");
                }
                arg_count += 1;

                if !self.match_token(scanner::TokenKind::Comma) {
                    break;
                }
            }
        }

        self.consume(
            scanner::TokenKind::RightParen,
            "Expected ')' after arguments.",
        );
        arg_count
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

    fn compiler_error(&mut self, error: CompilerError) {
        match error {
            CompilerError::ReadVarInInitialiser => {
                self.error("Cannot read local variable in its own initialiser.");
            }
            CompilerError::TooManyClosureVars => {
                self.error("Too many closure variables in function.");
            }
            _ => {}
        }
    }

    fn resolve_local(&mut self, name: &scanner::Token) -> Option<u8> {
        match self.compiler_mut().resolve_local(name) {
            Ok(index) => Some(index),
            Err(error) => {
                self.compiler_error(error);
                None
            }
        }
    }

    fn resolve_upvalue(&mut self, name: &scanner::Token) -> Option<u8> {
        if self.compilers.len() < 2 {
            // If there's only one scope then we're not going to find an upvalue.
            self.compiler_error(CompilerError::InvalidCompilerKind);
            return None;
        }

        // Iterate through the compilers outwards from the active one.
        for enclosing in (0..self.compilers.len() - 1).rev() {
            let current = enclosing + 1;
            // Try and resolve the local in the enclosing compiler's scope.
            if let Ok(index) = self.compilers[enclosing].resolve_local(name) {
                // If we found it, mark as captured and propagate the upvalue to the compilers that
                // are enclosed by the current one.
                self.compilers[enclosing].locals[index as usize].is_captured = true;
                let mut index = index;
                for compiler in current..self.compilers.len() {
                    index = match self.compilers[compiler].add_upvalue(index, compiler == current) {
                        Ok(index) => index,
                        Err(error) => {
                            self.compiler_error(error);
                            return None;
                        }
                    };
                }
                return Some(index);
            }
        }
        None
    }

    fn named_variable(&mut self, name: scanner::Token, can_assign: bool) {
        let (get_op, set_op, arg): (chunk::OpCode, chunk::OpCode, u8) = (|| {
            if let Some(result) = self.resolve_local(&name) {
                return (chunk::OpCode::GetLocal, chunk::OpCode::SetLocal, result);
            } else if let Some(result) = self.resolve_upvalue(&name) {
                return (chunk::OpCode::GetUpvalue, chunk::OpCode::SetUpvalue, result);
            }
            return (
                chunk::OpCode::GetGlobal,
                chunk::OpCode::SetGlobal,
                self.identifier_constant(&name),
            );
        })();

        if can_assign && self.match_token(scanner::TokenKind::Equal) {
            self.expression();
            self.emit_bytes([set_op as u8, arg]);
        } else {
            self.emit_bytes([get_op as u8, arg]);
        }
    }

    fn compiler(&mut self) -> &Compiler {
        &self.compilers.last().unwrap()
    }

    fn compiler_mut(&mut self) -> &mut Compiler {
        self.compilers.last_mut().unwrap()
    }

    fn chunk(&mut self) -> &mut chunk::Chunk {
        &mut self.compiler_mut().function.chunk
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
            scanner::TokenKind::BangEqual => {
                s.emit_bytes([chunk::OpCode::Equal as u8, chunk::OpCode::Not as u8])
            }
            scanner::TokenKind::EqualEqual => s.emit_byte(chunk::OpCode::Equal as u8),
            scanner::TokenKind::Greater => s.emit_byte(chunk::OpCode::Greater as u8),
            scanner::TokenKind::GreaterEqual => {
                s.emit_bytes([chunk::OpCode::Less as u8, chunk::OpCode::Not as u8])
            }
            scanner::TokenKind::Less => s.emit_byte(chunk::OpCode::Less as u8),
            scanner::TokenKind::LessEqual => {
                s.emit_bytes([chunk::OpCode::Greater as u8, chunk::OpCode::Not as u8])
            }
            scanner::TokenKind::Plus => s.emit_byte(chunk::OpCode::Add as u8),
            scanner::TokenKind::Minus => s.emit_byte(chunk::OpCode::Subtract as u8),
            scanner::TokenKind::Star => s.emit_byte(chunk::OpCode::Multiply as u8),
            scanner::TokenKind::Slash => s.emit_byte(chunk::OpCode::Divide as u8),
            _ => {}
        }
    }

    fn call(s: &mut Parser, _can_assign: bool) {
        let arg_count = s.argument_list();
        s.emit_bytes([chunk::OpCode::Call as u8, arg_count]);
    }

    fn dot(s: &mut Parser, can_assign: bool) {
        s.consume(
            scanner::TokenKind::Identifier,
            "Expected property name after '.'.",
        );
        let previous = s.previous.clone();
        let name = s.identifier_constant(&previous);

        if can_assign && s.match_token(scanner::TokenKind::Equal) {
            s.expression();
            s.emit_bytes([chunk::OpCode::SetProperty as u8, name]);
        } else if s.match_token(scanner::TokenKind::LeftParen) {
            let arg_count = s.argument_list();
            s.emit_bytes([chunk::OpCode::Invoke as u8, name]);
            s.emit_byte(arg_count);
        } else {
            s.emit_bytes([chunk::OpCode::GetProperty as u8, name]);
        }
    }

    fn unary(s: &mut Parser, _can_assign: bool) {
        let operator_kind = s.previous.kind;
        s.parse_precedence(Precedence::Unary);

        match operator_kind {
            scanner::TokenKind::Minus => s.emit_byte(chunk::OpCode::Negate as u8),
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

    fn super_(s: &mut Parser, _can_assign: bool) {
        if s.class_compilers.is_empty() {
            s.error("Cannot use 'super' outside of a class.");
        } else if !s.class_compilers.last().unwrap().has_superclass {
            s.error("Cannot use 'super' in a class with no superclass.");
        }

        s.consume(scanner::TokenKind::Dot, "Expected '.' after 'super'.");
        s.consume(
            scanner::TokenKind::Identifier,
            "Expected superclass method name.",
        );
        let previous = s.previous.clone();
        let name = s.identifier_constant(&previous);

        s.named_variable(scanner::Token::from_string("this"), false);
        if s.match_token(scanner::TokenKind::LeftParen) {
            let arg_count = s.argument_list();
            s.named_variable(scanner::Token::from_string("super"), false);
            s.emit_bytes([chunk::OpCode::SuperInvoke as u8, name]);
            s.emit_byte(arg_count);
        } else {
            s.named_variable(scanner::Token::from_string("super"), false);
            s.emit_bytes([chunk::OpCode::GetSuper as u8, name]);
        }
    }

    fn this(s: &mut Parser, _can_assign: bool) {
        if s.class_compilers.is_empty() {
            s.error("Cannot use 'this' outside of a class.");
            return;
        }
        Parser::variable(s, false);
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
