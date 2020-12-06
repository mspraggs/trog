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

use std::cell::{Cell, RefCell};
use std::fmt::Write;
use std::mem;

use crate::chunk::{Chunk, ChunkStore, OpCode};
use crate::common;
use crate::debug;
use crate::error::{Error, ErrorKind};
use crate::memory::{Heap, Root};
use crate::object::{self, ObjFunction, ObjStringStore};
use crate::scanner::{Scanner, Token, TokenKind};
use crate::value::{self, Value};

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
    Range,
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
            value if value == Precedence::Range as usize => Precedence::Range,
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

impl FunctionKind {
    fn is_bound(&self) -> bool {
        match self {
            FunctionKind::Initialiser => true,
            FunctionKind::Method => true,
            _ => false,
        }
    }
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
    name: String,
    depth: Option<usize>,
    is_captured: bool,
}

#[derive(Default)]
struct Upvalue {
    index: u8,
    is_local: bool,
}

struct Compiler {
    kind: FunctionKind,
    func_arity: u32,
    func_name: String,
    chunk: Chunk,

    locals: Vec<Local>,
    upvalues: Vec<Upvalue>,
    scope_depth: usize,
    lambda_count: usize,
}

enum CompilerError {
    InvalidCompilerKind,
    LocalNotFound,
    ReadVarInInitialiser,
    TooManyClosureVars,
}

impl Compiler {
    fn new(kind: FunctionKind, name: &str) -> Self {
        Compiler {
            kind,
            func_arity: 1,
            func_name: name.to_owned(),
            chunk: Chunk::new(),
            locals: vec![Local {
                name: if kind != FunctionKind::Function {
                    "self"
                } else {
                    ""
                }
                .to_owned(),
                depth: Some(0),
                is_captured: false,
            }],
            upvalues: Vec::new(),
            scope_depth: 0,
            lambda_count: 0,
        }
    }

    fn make_function(
        &mut self,
        heap: &mut Heap,
        string_store: &mut ObjStringStore,
        chunk_store: &mut ChunkStore,
    ) -> Root<ObjFunction> {
        let name = string_store.new_root_obj_string(heap, self.func_name.as_str());
        let num_upvalues = self.upvalues.len();
        let chunk = mem::replace(&mut self.chunk, Chunk::new());
        let chunk_index = chunk_store.add_chunk(heap, chunk);
        object::new_root_obj_function(
            heap,
            name.as_gc(),
            self.func_arity,
            num_upvalues,
            chunk_index,
        )
    }

    fn add_local(&mut self, name: &Token) -> bool {
        if self.locals.len() == common::LOCALS_MAX {
            return false;
        }

        self.locals.push(Local {
            name: name.source.clone(),
            depth: None,
            is_captured: false,
        });

        true
    }

    fn resolve_local(&self, name: &Token) -> Result<u8, CompilerError> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name == name.source {
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

        self.upvalues.push(Upvalue { index, is_local });
        Ok(upvalue_count as u8)
    }
}

struct ClassCompiler {
    has_superclass: bool,
}

pub fn compile(
    heap: &mut Heap,
    chunk_store: &mut ChunkStore,
    string_store: &mut ObjStringStore,
    source: String,
) -> Result<Root<ObjFunction>, Error> {
    let mut scanner = Scanner::from_source(source);

    let mut parser = Parser::new(heap, &mut scanner, chunk_store, string_store);
    parser.parse()
}

struct Parser<'a> {
    current: Token,
    previous: Token,
    panic_mode: Cell<bool>,
    single_target_mode: bool,
    scanner: &'a mut Scanner,
    compilers: Vec<Compiler>,
    class_compilers: Vec<ClassCompiler>,
    errors: RefCell<Vec<String>>,
    compiled_functions: Vec<Root<ObjFunction>>,
    chunk_store: &'a mut ChunkStore,
    string_store: &'a mut ObjStringStore,
    heap: &'a mut Heap,
}

impl<'a> Parser<'a> {
    fn new(
        heap: &'a mut Heap,
        scanner: &'a mut Scanner,
        chunk_store: &'a mut ChunkStore,
        string_store: &'a mut ObjStringStore,
    ) -> Parser<'a> {
        let mut ret = Parser {
            current: Token::new(),
            previous: Token::new(),
            panic_mode: Cell::new(false),
            single_target_mode: false,
            scanner,
            compilers: Vec::new(),
            class_compilers: Vec::new(),
            errors: RefCell::new(Vec::new()),
            compiled_functions: Vec::new(),
            chunk_store,
            string_store,
            heap,
        };
        ret.new_compiler(FunctionKind::Script, "");
        ret
    }

    fn parse(&mut self) -> Result<Root<ObjFunction>, Error> {
        self.advance();

        while !self.match_token(TokenKind::Eof) {
            self.declaration();
        }

        let had_error = !self.errors.borrow().is_empty();
        if had_error {
            return Err(Error::with_messages(
                ErrorKind::CompileError,
                &self
                    .errors
                    .borrow_mut()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            ));
        }

        Ok(self.finalise_compiler().0)
    }

    fn advance(&mut self) {
        self.previous = self.current.clone();

        loop {
            self.current = self.scanner.scan_token();
            if self.current.kind != TokenKind::Error {
                break;
            }

            let msg = self.current.source.clone();
            self.error_at_current(msg.as_str());
        }
    }

    fn consume(&mut self, kind: TokenKind, message: &str) {
        if self.current.kind == kind {
            self.advance();
            return;
        }
        self.error_at_current(message);
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.current.kind == kind
    }

    fn check_any(&self, kinds: &[TokenKind]) -> bool {
        kinds.iter().any(|k| self.check(*k))
    }

    fn match_token(&mut self, kind: TokenKind) -> bool {
        if !self.check(kind) {
            return false;
        }
        self.advance();
        true
    }

    fn match_binary_assignment(&mut self) -> bool {
        self.match_token(TokenKind::MinusEqual)
            || self.match_token(TokenKind::PlusEqual)
            || self.match_token(TokenKind::SlashEqual)
            || self.match_token(TokenKind::StarEqual)
    }

    fn expression(&mut self) {
        let precedence = if self.single_target_mode {
            Precedence::Term
        } else {
            Precedence::Assignment
        };
        self.parse_precedence(precedence);
    }

    fn block(&mut self) {
        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            self.declaration();
        }

        self.consume(TokenKind::RightBrace, "Expected '}' after block.");
    }

    fn new_compiler(&mut self, kind: FunctionKind, name: &str) {
        self.compilers.push(Compiler::new(kind, name));
    }

    fn finalise_compiler(&mut self) -> (Root<ObjFunction>, Vec<Upvalue>) {
        self.emit_return();

        if cfg!(feature = "debug_bytecode") && self.errors.borrow().is_empty() {
            let compiler = self.compiler();
            debug::disassemble_chunk(&compiler.chunk, compiler.func_name.as_str());
        }

        let mut compiler = self.compilers.pop().expect("Compiler stack empty.");
        let function = compiler.make_function(self.heap, self.string_store, self.chunk_store);
        self.compiled_functions.push(function.clone());

        (function, compiler.upvalues)
    }

    fn function(&mut self, kind: FunctionKind) {
        let name = self.previous.source.clone();
        self.new_compiler(kind, name.as_str());
        self.begin_scope();

        self.consume(TokenKind::LeftParen, "Expected '(' after function name.");
        if kind.is_bound() {
            self.consume(
                TokenKind::Self_,
                "Expected 'self' as first parameter in method.",
            );
            self.match_token(TokenKind::Comma);
        }
        self.parameter_list(
            TokenKind::RightParen,
            "Cannot have more than 255 parameters.",
            "Expected parameter name.",
        );
        self.consume(TokenKind::RightParen, "Expected ')' after parameters.");

        self.consume(TokenKind::LeftBrace, "Expected '{' before function body.");
        self.block();

        let (function, upvalues) = self.finalise_compiler();

        let constant = self.make_constant(value::Value::ObjFunction(function.as_gc()));
        self.emit_bytes([OpCode::Closure as u8, constant]);

        for upvalue in upvalues.iter() {
            self.emit_byte(upvalue.is_local as u8);
            self.emit_byte(upvalue.index as u8);
        }
    }

    fn method(&mut self) {
        self.consume(TokenKind::Fn, "Expected 'fn' before method name.");
        self.consume(TokenKind::Identifier, "Expected method name.");
        let previous = self.previous.clone();
        let constant = self.identifier_constant(&previous);

        let kind = if self.previous.source == "__init__" {
            FunctionKind::Initialiser
        } else {
            FunctionKind::Method
        };
        self.function(kind);
        self.emit_bytes([OpCode::Method as u8, constant]);
    }

    fn class_declaration(&mut self) {
        self.consume(TokenKind::Identifier, "Expected class name.");
        let name = self.previous.clone();
        let name_constant = self.identifier_constant(&name);
        self.declare_variable();

        self.emit_bytes([OpCode::Class as u8, name_constant]);
        self.define_variable(name_constant);

        self.class_compilers.push(ClassCompiler {
            has_superclass: false,
        });

        if self.match_token(TokenKind::Less) {
            self.consume(TokenKind::Identifier, "Expected superclass name.");
            Parser::variable(self, false);

            if name.source == self.previous.source {
                self.error("A class cannot inherit from iteself.");
            }

            self.begin_scope();
            self.compiler_mut().add_local(&Token::from_string("super"));
            self.define_variable(0);

            self.named_variable(name.clone(), false);
            self.emit_byte(OpCode::Inherit as u8);
            self.class_compilers.last_mut().unwrap().has_superclass = true;
        }

        self.named_variable(name, false);
        self.consume(TokenKind::LeftBrace, "Expected '{' before class body.");
        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            self.method();
        }
        self.consume(TokenKind::RightBrace, "Expected '}' after class body.");
        self.emit_byte(OpCode::Pop as u8);

        if self.class_compilers.last().unwrap().has_superclass {
            self.end_scope();
        }

        self.class_compilers.pop();
    }

    fn fn_declaration(&mut self) {
        let global = self.parse_variable("Expected function name.");
        self.mark_initialised();
        self.function(FunctionKind::Function);
        self.define_variable(global);
    }

    fn var_declaration(&mut self) {
        let global = self.parse_variable("Expected variable name.");

        if self.match_token(TokenKind::Equal) {
            self.expression();
        } else {
            self.emit_byte(OpCode::Nil as u8);
        }
        self.consume(
            TokenKind::SemiColon,
            "Expected ';' after variable declaration.",
        );

        self.define_variable(global);
    }

    fn expression_statement(&mut self) {
        self.expression();
        self.consume(TokenKind::SemiColon, "Expected ';' after expression.");
        self.emit_byte(OpCode::Pop as u8);
    }

    fn for_statement(&mut self) {
        self.begin_scope();

        let loop_iter_name = "... temp-iter-var ...";

        // For loops take the following form:
        // for (v in [1, 2, 3]) {
        //     ... loop body ...
        // }
        //
        // To support this we generate code equivalent to the following:
        // var v;
        // var it = [1, 2, 3].__iter__();
        // while ((v = it.__next__()) != <sentinel>) {
        //     ... loop body ...
        // }

        // Set up loop variable
        self.consume(TokenKind::Identifier, "Expected loop variable name.");
        self.declare_variable();
        self.emit_byte(OpCode::Nil as u8);
        self.mark_initialised();

        let loop_var = self
            .resolve_local(&Token::from_string(self.previous.source.as_str()))
            .expect("Expected to resolve local.");

        // Parse for loop syntax
        self.consume(TokenKind::In, "Expected 'in' after loop variable.");

        // Parse iterable object
        self.expression();
        self.compiler_mut()
            .add_local(&Token::from_string(loop_iter_name));
        let iter_method_name = self.identifier_constant(&Token::from_string("__iter__"));
        // Fetch the iterator itself
        self.emit_bytes([OpCode::Invoke as u8, iter_method_name]);
        self.emit_byte(0);
        self.mark_initialised();

        let loop_start = self.chunk().code.len();
        self.emit_byte(OpCode::IterNext as u8);
        self.emit_bytes([OpCode::SetLocal as u8, loop_var]);

        let exit_jump = self.emit_jump(OpCode::JumpIfSentinel);

        self.emit_byte(OpCode::Pop as u8);

        self.consume(TokenKind::LeftBrace, "Expected '{' after loop expression.");
        self.begin_scope();
        self.block();
        self.end_scope();

        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit_byte(OpCode::Pop as u8);
        self.end_scope();
    }

    fn if_statement(&mut self) {
        self.expression();

        let then_jump = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_byte(OpCode::Pop as u8);

        self.consume(TokenKind::LeftBrace, "Expected '{' after condition.");
        self.begin_scope();
        self.block();
        self.end_scope();

        let else_jump = self.emit_jump(OpCode::Jump);

        self.patch_jump(then_jump);
        self.emit_byte(OpCode::Pop as u8);

        if self.match_token(TokenKind::Else) {
            if !self.check_any(&[TokenKind::If, TokenKind::LeftBrace]) {
                self.error_at_current("Expected '{' after 'else'.");
            }
            self.statement();
        }
        self.patch_jump(else_jump);
    }

    fn return_statement(&mut self) {
        if self.compiler().kind == FunctionKind::Script {
            self.error("Cannot return from top-level code.");
        }
        if self.match_token(TokenKind::SemiColon) {
            self.emit_return();
        } else {
            if self.compiler().kind == FunctionKind::Initialiser {
                self.error("Cannot return a value from an initialiser.");
            }
            self.expression();
            self.consume(TokenKind::SemiColon, "Expected ';' after return value.");
            self.emit_byte(OpCode::Return as u8);
        }
    }

    fn while_statement(&mut self) {
        let loop_start = self.chunk().code.len();

        self.expression();

        let exit_jump = self.emit_jump(OpCode::JumpIfFalse);

        self.emit_byte(OpCode::Pop as u8);

        self.consume(TokenKind::LeftBrace, "Expected '{' after condition.");
        self.begin_scope();
        self.block();
        self.end_scope();

        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit_byte(OpCode::Pop as u8);
    }

    fn synchronise(&mut self) {
        self.panic_mode.set(false);

        while self.current.kind != TokenKind::Eof {
            if self.previous.kind == TokenKind::SemiColon {
                return;
            }

            match self.current.kind {
                TokenKind::Class => return,
                TokenKind::Fn => return,
                TokenKind::Var => return,
                TokenKind::For => return,
                TokenKind::If => return,
                TokenKind::While => return,
                TokenKind::Return => return,
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
                        OpCode::CloseUpvalue
                    } else {
                        OpCode::Pop
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
        if self.match_token(TokenKind::For) {
            self.for_statement();
        } else if self.match_token(TokenKind::If) {
            self.if_statement();
        } else if self.match_token(TokenKind::Return) {
            self.return_statement();
        } else if self.match_token(TokenKind::While) {
            self.while_statement();
        } else if self.match_token(TokenKind::LeftBrace) {
            self.begin_scope();
            self.block();
            self.end_scope();
        } else {
            self.expression_statement();
        }
    }

    fn declaration(&mut self) {
        if self.match_token(TokenKind::Class) {
            self.class_declaration();
        } else if self.match_token(TokenKind::Fn) {
            self.fn_declaration();
        } else if self.match_token(TokenKind::Var) {
            self.var_declaration();
        } else {
            self.statement();
        }

        if self.panic_mode.get() {
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
        self.emit_byte(OpCode::Loop as u8);

        let offset = self.chunk().code.len() - loop_start + 2;
        if offset > common::JUMP_SIZE_MAX {
            self.error("Loop body too large.");
        }

        let bytes = (offset as u16).to_ne_bytes();

        self.emit_byte(bytes[0]);
        self.emit_byte(bytes[1]);
    }

    fn emit_jump(&mut self, instruction: OpCode) -> usize {
        self.emit_byte(instruction as u8);
        self.emit_bytes([0xff, 0xff]);
        self.chunk().code.len() - 2
    }

    fn emit_return(&mut self) {
        if self.compiler().kind == FunctionKind::Initialiser {
            self.emit_bytes([OpCode::GetLocal as u8, 0]);
        } else {
            self.emit_byte(OpCode::Nil as u8);
        }
        self.emit_byte(OpCode::Return as u8);
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
        self.emit_bytes([OpCode::Constant as u8, constant]);
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump = self.chunk().code.len() - offset - 2;

        if jump > common::JUMP_SIZE_MAX {
            self.error("Too much code to jump over.");
        }

        let bytes = (jump as u16).to_ne_bytes();

        self.chunk().code[offset] = bytes[0];
        self.chunk().code[offset + 1] = bytes[1];
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

        if can_assign && self.match_token(TokenKind::Equal) {
            self.error("Invalid assignment target.");
        }
    }

    fn identifier_constant(&mut self, token: &Token) -> u8 {
        let value = Value::ObjString(
            self.string_store
                .new_gc_obj_string(self.heap, &token.source),
        );
        self.make_constant(value)
    }

    fn declare_variable(&mut self) {
        let scope_depth = self.compiler().scope_depth;
        if scope_depth == 0 {
            return;
        }

        for local in self.compilers.last().unwrap().locals.iter().rev() {
            if let Some(value) = local.depth {
                if value < scope_depth {
                    break;
                }
            }

            if self.previous.source == local.name {
                self.error("Variable with this name already declared in this scope.");
            }
        }

        if !self.compilers.last_mut().unwrap().add_local(&self.previous) {
            self.error("Too many variables in function.");
        }
    }

    fn parse_variable(&mut self, error_message: &str) -> u8 {
        self.consume(TokenKind::Identifier, error_message);

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

        self.emit_bytes([OpCode::DefineGlobal as u8, global]);
    }

    fn argument_list(&mut self, right_delim: TokenKind, count_msg: &str, delim_msg: &str) -> u8 {
        let mut arg_count: usize = 0;
        if !self.check(right_delim) {
            loop {
                self.expression();
                if arg_count == 255 {
                    self.error(count_msg);
                }
                arg_count += 1;

                if !self.match_token(TokenKind::Comma) {
                    break;
                }
            }
        }

        self.consume(right_delim, delim_msg);
        arg_count as u8
    }

    fn parameter_list(&mut self, right_delim: TokenKind, count_msg: &str, param_msg: &str) {
        if !self.check(right_delim) {
            loop {
                self.compiler_mut().func_arity += 1;
                if self.compiler().func_arity > 256 {
                    self.error_at_current(count_msg);
                }

                let param_constant = self.parse_variable(param_msg);
                self.define_variable(param_constant);

                if !self.match_token(TokenKind::Comma) {
                    break;
                }
            }
        }
    }

    fn get_rule(&self, kind: TokenKind) -> &ParseRule {
        &RULES[kind as usize]
    }

    fn error_at_current(&self, message: &str) {
        self.error_at(self.current.clone(), message);
    }

    fn error(&self, message: &str) {
        self.error_at(self.previous.clone(), message);
    }

    fn error_at(&self, token: Token, message: &str) {
        if self.panic_mode.get() {
            return;
        }
        self.panic_mode.set(true);

        let mut error_string = String::new();

        write!(error_string, "[line {}] Error", token.line).unwrap();

        match token.kind {
            TokenKind::Eof => write!(error_string, " at end").unwrap(),
            TokenKind::Error => {}
            _ => write!(error_string, " at '{}'", token.source).unwrap(),
        };

        write!(error_string, ": {}", message).unwrap();
        self.errors.borrow_mut().push(error_string);
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

    fn resolve_local(&mut self, name: &Token) -> Option<u8> {
        match self.compiler_mut().resolve_local(name) {
            Ok(index) => Some(index),
            Err(error) => {
                self.compiler_error(error);
                None
            }
        }
    }

    fn resolve_upvalue(&mut self, name: &Token) -> Option<u8> {
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

    fn binary_assign(&mut self, get_op: OpCode, variable: u8) {
        self.single_target_mode = true;
        let op_kind = self.previous.kind;
        self.emit_bytes([get_op as u8, variable]);
        self.expression();
        match op_kind {
            TokenKind::MinusEqual => self.emit_byte(OpCode::Subtract as u8),
            TokenKind::PlusEqual => self.emit_byte(OpCode::Add as u8),
            TokenKind::SlashEqual => self.emit_byte(OpCode::Divide as u8),
            TokenKind::StarEqual => self.emit_byte(OpCode::Multiply as u8),
            _ => unreachable!(),
        }
        self.single_target_mode = false;
    }

    fn named_variable(&mut self, name: Token, can_assign: bool) {
        let (get_op, set_op, arg) = if let Some(result) = self.resolve_local(&name) {
            (OpCode::GetLocal, OpCode::SetLocal, result)
        } else if let Some(result) = self.resolve_upvalue(&name) {
            (OpCode::GetUpvalue, OpCode::SetUpvalue, result)
        } else {
            (
                OpCode::GetGlobal,
                OpCode::SetGlobal,
                self.identifier_constant(&name),
            )
        };

        if can_assign && self.match_token(TokenKind::Equal) {
            self.expression();
            self.emit_bytes([set_op as u8, arg]);
        } else if can_assign && self.match_binary_assignment() {
            self.binary_assign(get_op, arg);
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

    fn chunk(&mut self) -> &mut Chunk {
        &mut self.compiler_mut().chunk
    }

    fn grouping(s: &mut Parser, _can_assign: bool) {
        s.expression();
        s.consume(TokenKind::RightParen, "Expected ')' after expression.");
    }

    fn binary(s: &mut Parser, _can_assign: bool) {
        let operator_kind = s.previous.kind;
        let rule_precedence = s.get_rule(operator_kind).precedence;
        s.parse_precedence(Precedence::from(rule_precedence as usize + 1));

        match operator_kind {
            TokenKind::BangEqual => s.emit_bytes([OpCode::Equal as u8, OpCode::Not as u8]),
            TokenKind::EqualEqual => s.emit_byte(OpCode::Equal as u8),
            TokenKind::Greater => s.emit_byte(OpCode::Greater as u8),
            TokenKind::GreaterEqual => s.emit_bytes([OpCode::Less as u8, OpCode::Not as u8]),
            TokenKind::Less => s.emit_byte(OpCode::Less as u8),
            TokenKind::LessEqual => s.emit_bytes([OpCode::Greater as u8, OpCode::Not as u8]),
            TokenKind::Plus => s.emit_byte(OpCode::Add as u8),
            TokenKind::Minus => s.emit_byte(OpCode::Subtract as u8),
            TokenKind::Star => s.emit_byte(OpCode::Multiply as u8),
            TokenKind::Slash => s.emit_byte(OpCode::Divide as u8),
            _ => {}
        }
    }

    fn call(s: &mut Parser, _can_assign: bool) {
        let arg_count = s.argument_list(
            TokenKind::RightParen,
            "Cannot have more than 255 arguments.",
            "Expected ')' after arguments.",
        );
        s.emit_bytes([OpCode::Call as u8, arg_count]);
    }

    fn dot(s: &mut Parser, can_assign: bool) {
        s.consume(TokenKind::Identifier, "Expected property name after '.'.");
        let previous = s.previous.clone();
        let name = s.identifier_constant(&previous);

        if can_assign && s.match_token(TokenKind::Equal) {
            s.expression();
            s.emit_bytes([OpCode::SetProperty as u8, name]);
        } else if can_assign && s.match_binary_assignment() {
            s.emit_byte(OpCode::CopyTop as u8);
            s.binary_assign(OpCode::GetProperty, name);
            s.emit_bytes([OpCode::SetProperty as u8, name]);
        } else if s.match_token(TokenKind::LeftParen) {
            let arg_count = s.argument_list(
                TokenKind::RightParen,
                "Cannot have more than 255 arguments.",
                "Expected ')' after arguments.",
            );
            s.emit_bytes([OpCode::Invoke as u8, name]);
            s.emit_byte(arg_count);
        } else {
            s.emit_bytes([OpCode::GetProperty as u8, name]);
        }
    }

    fn dotdot(s: &mut Parser, _can_assign: bool) {
        s.parse_precedence(Precedence::Unary);
        s.emit_byte(OpCode::BuildRange as u8);
    }

    fn index(s: &mut Parser, can_assign: bool) {
        s.expression();
        s.consume(TokenKind::RightBracket, "Expected ']' after index.");

        let (name, num_args) = if can_assign && s.match_token(TokenKind::Equal) {
            s.expression();
            (s.identifier_constant(&Token::from_string("__setitem__")), 2)
        } else {
            (s.identifier_constant(&Token::from_string("__getitem__")), 1)
        };
        s.emit_bytes([OpCode::Invoke as u8, name]);
        s.emit_byte(num_args as u8);
    }

    fn lambda(s: &mut Parser, _can_assign: bool) {
        let lambda_count = s.compiler().lambda_count;
        s.compiler_mut().lambda_count += 1;
        s.new_compiler(
            FunctionKind::Function,
            format!("lambda-{}", lambda_count).as_str(),
        );
        s.begin_scope();

        s.parameter_list(
            TokenKind::Bar,
            "Cannot have more than 255 parameters.",
            "Expected parameter name.",
        );
        s.consume(TokenKind::Bar, "Expected ')' after parameters.");

        if s.match_token(TokenKind::LeftBrace) {
            s.block();
        } else {
            s.expression();
            s.emit_byte(OpCode::Return as u8);
        }

        let (function, upvalues) = s.finalise_compiler();

        let constant = s.make_constant(value::Value::ObjFunction(function.as_gc()));
        s.emit_bytes([OpCode::Closure as u8, constant]);

        for upvalue in upvalues.iter() {
            s.emit_byte(upvalue.is_local as u8);
            s.emit_byte(upvalue.index as u8);
        }
    }

    fn vector(s: &mut Parser, _can_assign: bool) {
        let num_elems = s.argument_list(
            TokenKind::RightBracket,
            "Cannot have more than 255 Vec elements.",
            "Expected ']' after elements.",
        );

        s.emit_bytes([OpCode::BuildVec as u8, num_elems as u8]);
    }

    fn unary(s: &mut Parser, _can_assign: bool) {
        let operator_kind = s.previous.kind;
        s.parse_precedence(Precedence::Unary);

        match operator_kind {
            TokenKind::Minus => s.emit_byte(OpCode::Negate as u8),
            TokenKind::Bang => s.emit_byte(OpCode::Not as u8),
            _ => {}
        }
    }

    fn number(s: &mut Parser, _can_assign: bool) {
        let value = s.previous.source.as_str().parse::<f64>().unwrap();
        s.emit_constant(value::Value::Number(value));
    }

    fn literal(s: &mut Parser, _can_assign: bool) {
        match s.previous.kind {
            TokenKind::False => {
                s.emit_byte(OpCode::False as u8);
            }
            TokenKind::Nil => {
                s.emit_byte(OpCode::Nil as u8);
            }
            TokenKind::True => {
                s.emit_byte(OpCode::True as u8);
            }
            _ => {}
        }
    }

    fn string(s: &mut Parser, _can_assign: bool) {
        let value = Value::ObjString(s.string_store.new_gc_obj_string(s.heap, &s.previous.source));
        s.emit_constant(value);
    }

    fn interpolation(s: &mut Parser, _can_assign: bool) {
        let mut arg_count = 0;
        loop {
            if !s.previous.source.is_empty() {
                let value =
                    Value::ObjString(s.string_store.new_gc_obj_string(s.heap, &s.previous.source));
                s.emit_constant(value);
                arg_count += 1;
            }
            s.expression();
            s.emit_byte(OpCode::FormatString as u8);
            arg_count += 1;
            if !s.match_token(TokenKind::Interpolation) {
                break;
            }
        }

        s.advance();
        if !s.previous.source.is_empty() {
            let value = Value::ObjString(
                s.string_store
                    .new_gc_obj_string(s.heap, s.previous.source.as_str()),
            );
            s.emit_constant(value);
            arg_count += 1;
        }

        s.emit_bytes([OpCode::BuildString as u8, arg_count as u8]);
    }

    fn variable(s: &mut Parser, can_assign: bool) {
        s.named_variable(s.previous.clone(), can_assign);
    }

    fn self_(s: &mut Parser, _can_assign: bool) {
        if s.class_compilers.is_empty() {
            s.error("Cannot use 'self' outside of a class.");
            return;
        }
        Parser::variable(s, false);
    }

    fn super_(s: &mut Parser, _can_assign: bool) {
        if s.class_compilers.is_empty() {
            s.error("Cannot use 'super' outside of a class.");
        } else if !s.class_compilers.last().unwrap().has_superclass {
            s.error("Cannot use 'super' in a class with no superclass.");
        }

        s.consume(TokenKind::Dot, "Expected '.' after 'super'.");
        s.consume(TokenKind::Identifier, "Expected superclass method name.");
        let previous = s.previous.clone();
        let name = s.identifier_constant(&previous);

        let instance_local_name = s.compiler().locals[0].name.clone();
        s.named_variable(Token::from_string(instance_local_name.as_str()), false);
        if s.match_token(TokenKind::LeftParen) {
            let arg_count = s.argument_list(
                TokenKind::RightParen,
                "Cannot have more than 255 arguments.",
                "Expected ')' after arguments.",
            );
            s.named_variable(Token::from_string("super"), false);
            s.emit_bytes([OpCode::SuperInvoke as u8, name]);
            s.emit_byte(arg_count);
        } else {
            s.named_variable(Token::from_string("super"), false);
            s.emit_bytes([OpCode::GetSuper as u8, name]);
        }
    }

    fn and(s: &mut Parser, _can_assign: bool) {
        let end_jump = s.emit_jump(OpCode::JumpIfFalse);

        s.emit_byte(OpCode::Pop as u8);
        s.parse_precedence(Precedence::And);

        s.patch_jump(end_jump);
    }

    fn or(s: &mut Parser, _can_assign: bool) {
        let else_jump = s.emit_jump(OpCode::JumpIfFalse);
        let end_jump = s.emit_jump(OpCode::Jump);

        s.patch_jump(else_jump);
        s.emit_byte(OpCode::Pop as u8);

        s.parse_precedence(Precedence::Or);
        s.patch_jump(end_jump);
    }
}

const RULES: [ParseRule; 49] = [
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
    // LeftBracket
    ParseRule {
        prefix: Some(Parser::vector),
        infix: Some(Parser::index),
        precedence: Precedence::Call,
    },
    // RightBracket
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
    // DotDot
    ParseRule {
        prefix: None,
        infix: Some(Parser::dotdot),
        precedence: Precedence::Range,
    },
    // Minus
    ParseRule {
        prefix: Some(Parser::unary),
        infix: Some(Parser::binary),
        precedence: Precedence::Term,
    },
    // MinusEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Plus
    ParseRule {
        prefix: None,
        infix: Some(Parser::binary),
        precedence: Precedence::Term,
    },
    // PlusEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
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
    // SlashEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Star
    ParseRule {
        prefix: None,
        infix: Some(Parser::binary),
        precedence: Precedence::Factor,
    },
    // StarEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
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
    // Bar
    ParseRule {
        prefix: Some(Parser::lambda),
        infix: None,
        precedence: Precedence::None,
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
    // Interpolation
    ParseRule {
        prefix: Some(Parser::interpolation),
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
    // Fn
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
    // In
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
    // Return
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Self
    ParseRule {
        prefix: Some(Parser::self_),
        infix: None,
        precedence: Precedence::None,
    },
    // Super
    ParseRule {
        prefix: Some(Parser::super_),
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
];
