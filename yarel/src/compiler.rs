/* Copyright 2020-2021 Matt Spraggs
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
use std::collections::HashMap;
use std::fmt::Write;
use std::mem;
use std::path::Path;

use crate::chunk::{Chunk, OpCode};
use crate::common;
use crate::debug;
use crate::error::{Error, ErrorKind};
use crate::memory::{Gc, Root};
use crate::object::{ObjFunction, ObjString};
use crate::scanner::{Scanner, Token, TokenKind};
use crate::value::{self, Value};
use crate::vm::Vm;

#[derive(Copy, Clone)]
enum Precedence {
    None,
    Assignment,
    Or,
    And,
    Equality,
    Comparison,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    BitShift,
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
            value if value == Precedence::BitwiseOr as usize => Precedence::BitwiseOr,
            value if value == Precedence::BitwiseXor as usize => Precedence::BitwiseXor,
            value if value == Precedence::BitwiseAnd as usize => Precedence::BitwiseAnd,
            value if value == Precedence::BitShift as usize => Precedence::BitShift,
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
    StaticMethod,
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
    function: ObjFunction,
    kind: FunctionKind,
    chunk: Chunk,
    locals: Vec<Local>,
    upvalues: Vec<Upvalue>,
    scope_depth: usize,
    lambda_count: usize,
    in_try_block: bool,
    loop_stack: Vec<(usize, usize)>,
    break_stack: Vec<Vec<usize>>,
}

enum CompilerError {
    InvalidCompilerKind,
    InvalidControlStatement,
    JumpTooLarge,
    LocalNotFound,
    ReadVarInInitialiser,
    TooManyClosureVars,
}

impl Compiler {
    fn new(kind: FunctionKind, name: Gc<ObjString>, module_path: Gc<ObjString>) -> Self {
        Compiler {
            function: ObjFunction::new(name, 1, 0, Gc::dangling(), module_path),
            kind,
            chunk: Chunk::new(),
            locals: vec![Local {
                name: if kind == FunctionKind::StaticMethod {
                    "Self"
                } else if kind != FunctionKind::Function {
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
            in_try_block: false,
            loop_stack: Vec::new(),
            break_stack: Vec::new(),
        }
    }

    fn allocate_function(&mut self, vm: &mut Vm) -> Root<ObjFunction> {
        let chunk = mem::replace(&mut self.chunk, Chunk::new());
        let chunk = vm.add_chunk(chunk);
        self.function.chunk = chunk;
        let function = mem::take(&mut self.function);
        Root::new(function)
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

    fn mark_initialised(&mut self, local: usize) {
        self.locals[local].depth = Some(self.scope_depth);
    }

    fn mark_last_initialised(&mut self) {
        self.locals.last_mut().unwrap().depth = Some(self.scope_depth);
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
        self.function.upvalue_count += 1;
        Ok(upvalue_count as u8)
    }

    fn patch_jump(&mut self, offset: usize) -> Result<(), CompilerError> {
        let jump = self.chunk.code.len() - offset - 2;

        if jump > common::JUMP_SIZE_MAX {
            return Err(CompilerError::JumpTooLarge);
        }

        let bytes = (jump as u16).to_ne_bytes();

        self.chunk.code[offset] = bytes[0];
        self.chunk.code[offset + 1] = bytes[1];
        Ok(())
    }

    fn push_loop(&mut self) {
        let loop_start = self.chunk.code.len();
        self.loop_stack.push((loop_start, self.scope_depth));
        self.break_stack.push(Vec::new());
    }

    fn push_break(&mut self, pos: usize) -> Result<(), CompilerError> {
        let breaks = self
            .break_stack
            .last_mut()
            .ok_or(CompilerError::InvalidControlStatement)?;
        breaks.push(pos);
        Ok(())
    }

    fn pop_loop(&mut self) -> Result<(), CompilerError> {
        self.loop_stack.pop();
        let break_points = self.break_stack.pop().expect("Expected Vec.");

        for &bp in &break_points {
            self.patch_jump(bp)?;
        }

        Ok(())
    }

    fn current_loop_header(&self) -> Option<(usize, usize)> {
        self.loop_stack.last().copied()
    }
}

struct ClassCompiler {
    has_superclass: bool,
}

pub fn compile(
    vm: &mut Vm,
    source: String,
    module_path: Option<&str>,
) -> Result<Root<ObjFunction>, Error> {
    let mut scanner = Scanner::from_source(source);
    let mut parser = Parser::new(vm, &mut scanner, module_path);
    parser.parse()
}

struct Attribute {
    name: Token,
    arguments: Vec<Token>,
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
    module_path: Gc<ObjString>,
    attributes: HashMap<String, Attribute>,
    attribute_opener: Option<Token>,
    vm: &'a mut Vm,
}

impl<'a> Parser<'a> {
    fn new(vm: &'a mut Vm, scanner: &'a mut Scanner, module_path: Option<&str>) -> Parser<'a> {
        let module_path = vm.new_gc_obj_string(module_path.unwrap_or("main"));
        let empty = vm.new_gc_obj_string("");
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
            module_path,
            attributes: HashMap::new(),
            attribute_opener: None,
            vm,
        };
        ret.new_compiler(FunctionKind::Script, empty, module_path);
        ret
    }

    fn parse(&mut self) -> Result<Root<ObjFunction>, Error> {
        self.advance();

        while !self.match_token(TokenKind::Eof) {
            self.declaration();
        }
        self.check_no_attributes();

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
            || self.match_token(TokenKind::AmpEqual)
            || self.match_token(TokenKind::BarEqual)
            || self.match_token(TokenKind::CaretEqual)
            || self.match_token(TokenKind::PercentEqual)
            || self.match_token(TokenKind::LessLessEqual)
            || self.match_token(TokenKind::GreaterGreaterEqual)
    }

    fn expression(&mut self) {
        let precedence = if self.single_target_mode {
            Precedence::BitwiseOr
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

    fn new_compiler(
        &mut self,
        kind: FunctionKind,
        name: Gc<ObjString>,
        module_path: Gc<ObjString>,
    ) {
        self.compilers.push(Compiler::new(kind, name, module_path));
    }

    fn finalise_compiler(&mut self) -> (Root<ObjFunction>, Vec<Upvalue>) {
        self.emit_return();

        let mut compiler = self.compilers.pop().expect("Compiler stack empty.");
        let function = compiler.allocate_function(self.vm);
        self.compiled_functions.push(function.clone());

        if cfg!(feature = "debug_bytecode") && self.errors.borrow().is_empty() {
            let chunk = function.chunk;
            let func_name = format!("{}", Value::ObjFunction(function.as_gc()));
            debug::disassemble_chunk(&chunk, &func_name);
        }

        (function, compiler.upvalues)
    }

    fn function(&mut self, kind: FunctionKind) {
        let name = self.previous.source.clone();
        let name = self.vm.new_gc_obj_string(name.as_str());
        self.new_compiler(kind, name, self.module_path);
        self.begin_scope();

        self.consume(TokenKind::LeftParen, "Expected '(' after function name.");
        if kind.is_bound() {
            self.consume(
                TokenKind::Self_,
                "Expected 'self' as first parameter in method.",
            );
            self.match_token(TokenKind::Comma);
        } else if self.match_token(TokenKind::Self_) {
            self.error("Expected parameter name.");
            self.match_token(TokenKind::Comma);
        }
        self.parameter_list(
            TokenKind::RightParen,
            "Cannot have more than 255 parameters.",
            "Expected parameter name.",
        );
        self.consume(TokenKind::RightParen, "Expected ')' after parameters.");

        self.consume(TokenKind::LeftBrace, "Expected '{' before function body.");
        if kind == FunctionKind::Initialiser {
            let arity = (self.compiler().function.arity - 1) as u8;
            self.emit_bytes([OpCode::Construct as u8, arity]);
        }
        self.block();

        let (function, upvalues) = self.finalise_compiler();

        let constant = self.make_constant(value::Value::ObjFunction(function.as_gc()));
        self.emit_constant_op(OpCode::Closure, constant);

        for upvalue in upvalues.iter() {
            self.emit_byte(upvalue.is_local as u8);
            self.emit_byte(upvalue.index as u8);
        }
    }

    fn method(&mut self) {
        if self.match_token(TokenKind::Hash) {
            self.attributes_declaration();
        }

        let static_attr = self.take_attribute("static", 0);
        let constructor_attr = self.take_attribute("constructor", 0);
        self.check_supported_attributes("method");

        self.consume(TokenKind::Fn, "Expected 'fn' before method name.");
        self.consume(TokenKind::Identifier, "Expected method name.");
        let previous = self.previous.clone();
        let constant = self.identifier_constant(&previous);

        let kind = if constructor_attr.is_some() {
            if let Some(attr) = static_attr {
                self.error_at(attr.name, "Constructors cannot be static.");
            }
            FunctionKind::Initialiser
        } else if static_attr.is_some() {
            FunctionKind::StaticMethod
        } else {
            FunctionKind::Method
        };
        self.function(kind);
        let opcode = if kind == FunctionKind::Method {
            OpCode::Method
        } else {
            OpCode::StaticMethod
        };
        self.emit_constant_op(opcode, constant);
    }

    fn initialiser(&mut self, name: Token) {
        let name_constant = self.identifier_constant(&name);
        let kind = FunctionKind::Initialiser;

        let name = self.vm.new_gc_obj_string(name.source.as_str());
        self.new_compiler(kind, name, self.module_path);
        self.begin_scope();
        self.emit_bytes([OpCode::Construct as u8, 0]);
        let (function, _) = self.finalise_compiler();

        let constant = self.make_constant(value::Value::ObjFunction(function.as_gc()));
        self.emit_constant_op(OpCode::Closure, constant);

        let opcode = OpCode::StaticMethod;
        self.emit_constant_op(opcode, name_constant);
    }

    fn class_declaration(&mut self) {
        let constructor_attr = self.take_attribute("constructor", 1);
        let constructor_name = constructor_attr.map(|a| a.arguments[0].clone());
        let superclass_attr = self.take_attribute("derive", 1);
        let superclass_name = superclass_attr.map(|a| a.arguments[0].clone());
        self.check_supported_attributes("class");

        self.consume(TokenKind::Identifier, "Expected class name.");
        let name = self.previous.clone();
        let name_constant = self.identifier_constant(&name);
        self.declare_variable();

        self.emit_constant_op(OpCode::DeclareClass, name_constant);
        self.define_variable(name_constant);

        self.class_compilers.push(ClassCompiler {
            has_superclass: false,
        });

        if let Some(superclass_name) = superclass_name {
            self.named_variable(superclass_name.clone(), false);

            if name.source == superclass_name.source {
                self.error("A class cannot inherit from itself.");
            }

            self.begin_scope();
            self.compiler_mut().add_local(&Token::from_string("super"));
            self.define_variable(0);

            self.named_variable(name.clone(), false);
            self.emit_byte_for_token(OpCode::Inherit as u8, superclass_name);
            self.class_compilers.last_mut().unwrap().has_superclass = true;
        }

        let (_, set_op, arg) = self.resolve_variable(&name);

        self.named_variable(name, false);
        self.consume(TokenKind::LeftBrace, "Expected '{' before class body.");

        if let Some(name) = constructor_name {
            self.initialiser(name);
        }

        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            self.method();
        }
        self.consume(TokenKind::RightBrace, "Expected '}' after class body.");
        self.emit_byte(OpCode::DefineClass as u8);
        self.emit_variable_op(set_op, arg);
        self.emit_byte(OpCode::Pop as u8);

        if self.class_compilers.last().unwrap().has_superclass {
            self.end_scope();
        }

        self.class_compilers.pop();
    }

    fn fn_declaration(&mut self) {
        self.check_supported_attributes("function");
        let global = self.parse_variable("Expected function name.");
        self.mark_initialised();
        self.function(FunctionKind::Function);
        self.define_variable(global);
    }

    fn take_attribute(&mut self, name: &str, num_args: usize) -> Option<Attribute> {
        let attr = self.attributes.remove(name);
        if let Some(attr) = attr {
            if attr.arguments.len() != num_args {
                let msg = format!(
                    "Expected {} argument{} to '{}' attribute.",
                    num_args,
                    if num_args != 1 { "s" } else { "" },
                    attr.name.source
                );
                self.error_at(attr.name, &msg);
                None
            } else {
                Some(attr)
            }
        } else {
            None
        }
    }

    fn attribute(&mut self) -> Option<Attribute> {
        if !self.match_token(TokenKind::Identifier) {
            return None;
        }
        let name = self.previous.clone();
        let mut arguments = Vec::new();

        if self.match_token(TokenKind::LeftParen) {
            loop {
                if !self.match_token(TokenKind::Identifier) {
                    self.error_at_current("Expected an attribute argument.");
                    return None;
                }
                arguments.push(self.previous.clone());

                if !self.match_token(TokenKind::Comma) {
                    break;
                }
            }

            if !self.match_token(TokenKind::RightParen) {
                self.error_at_current("Expected ')' after attribute arguments.");
                return None;
            }
        }

        Some(Attribute { name, arguments })
    }

    fn attributes_declaration(&mut self) {
        self.check_no_attributes();
        let opener = self.previous.clone();
        if !self.match_token(TokenKind::LeftBracket) {
            self.error_at_current("Expected '[' after '#'.");
            return;
        }
        let mut attributes = HashMap::new();

        while let Some(attribute) = self.attribute() {
            if attributes
                .insert(attribute.name.source.clone(), attribute)
                .is_some()
            {
                self.error(&format!("Duplicate attribute '{}'.", self.previous.source));
                break;
            }

            if !self.match_token(TokenKind::Comma) {
                break;
            }
        }
        if attributes.is_empty() {
            self.error_at_current("Expected at least one attribute.");
        }
        if !self.match_token(TokenKind::RightBracket) {
            self.error_at_current("Expected ']' after attribute list.");
            return;
        }
        self.attribute_opener = Some(opener);
        self.attributes = attributes;
    }

    fn var_declaration(&mut self) {
        self.check_no_attributes();
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

    fn import_statement(&mut self) {
        self.consume(TokenKind::Str, "Expected a module path.");
        let path = &self.previous.clone();
        if path.source == "main" {
            self.error("Cannot import top-level module.");
        }
        let path_constant = self.identifier_constant(&path);

        let name = if self.match_token(TokenKind::As) {
            self.consume(TokenKind::Identifier, "Expected module name.");
            self.previous.clone()
        } else {
            let result = (|| Some(Path::new(&path.source).file_name()?.to_str()?))();
            if let Some(filename) = result {
                Token::from_string_and_line(filename, self.current.line)
            } else {
                self.error("Expected a module path.");
                return;
            }
        };
        // Because the module path syntactically comes before the variable that refers to the
        // module, we have to inject the token that refers to the module here.
        self.previous = name.clone();
        self.declare_variable();
        self.emit_constant_op(OpCode::StartImport, path_constant);

        self.consume(TokenKind::SemiColon, "Expected ';' after module import.");

        self.emit_byte(OpCode::FinishImport as u8);

        let name_constant = self.identifier_constant(&name);
        self.define_variable(name_constant);
    }

    fn for_statement(&mut self) {
        self.begin_scope();

        let loop_iter_name = "... temp-iter-var ...";

        // For loops take the following form:
        // for v in [1, 2, 3] {
        //     ... loop body ...
        // }
        //
        // To support this we generate code equivalent to the following:
        // var v;
        // var it = [1, 2, 3].iter();
        // while !(v = it.next()).derives(StopIter) {
        //     ... loop body ...
        // }

        // Set up loop variable
        if !self.match_token(TokenKind::Identifier) {
            self.error_at_current("Expected loop variable name.");
            return;
        }
        self.declare_variable();
        let loop_var = self.compiler().locals.len() - 1;
        self.emit_byte(OpCode::Nil as u8);

        // Parse for loop syntax
        self.consume(TokenKind::In, "Expected 'in' after loop variable.");

        // Parse iterable object
        self.expression();

        self.compiler_mut().mark_initialised(loop_var);

        self.compiler_mut()
            .add_local(&Token::from_string(loop_iter_name));
        let iter_method_name = self.identifier_constant(&Token::from_string("iter"));
        // Fetch the iterator itself
        self.emit_constant_op(OpCode::Invoke, iter_method_name);
        self.emit_byte(0);
        self.mark_initialised();

        self.compiler_mut().push_loop();
        let (loop_start, _) = self
            .compiler()
            .current_loop_header()
            .expect("Expected usize.");
        self.emit_byte(OpCode::IterNext as u8);
        self.emit_bytes([OpCode::SetLocal as u8, loop_var as u8]);

        let exit_jump = self.emit_jump(OpCode::JumpIfStopIter);

        self.emit_byte(OpCode::Pop as u8);

        self.consume(TokenKind::LeftBrace, "Expected '{' after loop expression.");
        self.begin_scope();
        self.block();
        self.end_scope();

        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit_byte(OpCode::Pop as u8);
        match self.compiler_mut().pop_loop() {
            Ok(_) => {}
            Err(e) => self.compiler_error(e),
        }
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
            if self.compiler().in_try_block {
                self.emit_byte(OpCode::JumpFinally as u8);
            }
            self.emit_byte(OpCode::Return as u8);
        }
    }

    fn break_statement(&mut self) {
        let break_pos = self.emit_jump(OpCode::Jump);
        match self.compiler_mut().push_break(break_pos) {
            Ok(_) => {}
            Err(e) => {
                self.compiler_error(e);
                return;
            }
        }
        let scope_depth = self
            .compiler()
            .current_loop_header()
            .expect("Expected tuple.")
            .1;
        self.emit_scope_end(false, scope_depth);
        self.consume(TokenKind::SemiColon, "Expected ';' after 'break'.");
    }

    fn continue_statement(&mut self) {
        let (jump_target, scope_depth) = match self.compiler().current_loop_header() {
            Some((pos, depth)) => (pos, depth),
            None => {
                self.error("Cannot use 'continue' statement outside of loop body.");
                return;
            }
        };
        self.emit_scope_end(false, scope_depth);
        self.emit_loop(jump_target);
        self.consume(TokenKind::SemiColon, "Expected ';' after 'continue'.");
    }

    fn throw_statement(&mut self) {
        self.expression();
        self.consume(TokenKind::SemiColon, "Expected ';' after throw value.");
        self.emit_byte(OpCode::Throw as u8);
    }

    fn try_statement(&mut self) {
        let prev_in_try_block = self.compiler().in_try_block;
        self.compiler_mut().in_try_block = true;

        self.emit_byte(OpCode::PushExcHandler as u8);
        let handler_catch_arg_pos = self.chunk().code.len();
        self.emit_bytes([0xff, 0xff]);
        self.emit_bytes([0xff, 0xff]);
        let post_handler_args_ip_pos = self.chunk().code.len();

        self.consume(TokenKind::LeftBrace, "Expected '{' after 'try'.");
        self.begin_scope();
        self.block();
        self.end_scope();
        self.compiler_mut().in_try_block = prev_in_try_block;

        self.emit_byte(OpCode::PopExcHandler as u8);
        let catch_jump_pos = self.emit_jump(OpCode::Jump);
        self.patch_offset_at(handler_catch_arg_pos, post_handler_args_ip_pos);
        let catch_start_pos = self.chunk().code.len();

        let have_catch = self.match_token(TokenKind::Catch);

        if have_catch {
            self.emit_byte(OpCode::PopExcHandler as u8);
            if !self.match_token(TokenKind::Identifier) {
                self.error_at_current("Expected exception variable name.");
                return;
            }
            self.begin_scope();

            self.declare_variable();
            self.mark_initialised();

            self.consume(TokenKind::LeftBrace, "Expected '{' after variable.");

            self.block();
            self.end_scope();
        }

        self.patch_jump(catch_jump_pos);

        self.patch_offset_at(handler_catch_arg_pos + 2, catch_start_pos);
        let have_finally = self.match_token(TokenKind::Finally);

        if have_finally {
            self.consume(TokenKind::LeftBrace, "Expected '{' after 'finally'.");
            self.begin_scope();
            self.block();
            self.end_scope();
            self.emit_byte(OpCode::EndFinally as u8);
        }

        if !have_catch && !have_finally {
            self.error("Expected 'catch' or 'finally' after 'try' block.");
            return;
        }
    }

    fn while_statement(&mut self) {
        self.compiler_mut().push_loop();
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
        match self.compiler_mut().pop_loop() {
            Ok(_) => {}
            Err(e) => self.compiler_error(e),
        }
    }

    fn synchronise(&mut self) {
        self.panic_mode.set(false);

        while self.current.kind != TokenKind::Eof {
            if self.previous.kind == TokenKind::SemiColon {
                return;
            }

            match self.current.kind {
                TokenKind::Hash => return,
                TokenKind::Class => return,
                TokenKind::Fn => return,
                TokenKind::Var => return,
                TokenKind::For => return,
                TokenKind::If => return,
                TokenKind::While => return,
                TokenKind::Break => return,
                TokenKind::Continue => return,
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
        let scope_depth = self.compiler().scope_depth;
        self.emit_scope_end(true, scope_depth);
    }

    fn statement(&mut self) {
        self.check_no_attributes();
        if self.match_token(TokenKind::Import) {
            self.import_statement();
        } else if self.match_token(TokenKind::For) {
            self.for_statement();
        } else if self.match_token(TokenKind::If) {
            self.if_statement();
        } else if self.match_token(TokenKind::Return) {
            self.return_statement();
        } else if self.match_token(TokenKind::Break) {
            self.break_statement();
        } else if self.match_token(TokenKind::Continue) {
            self.continue_statement();
        } else if self.match_token(TokenKind::Throw) {
            self.throw_statement();
        } else if self.match_token(TokenKind::Try) {
            self.try_statement();
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
        } else if self.match_token(TokenKind::Hash) {
            self.attributes_declaration();
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

    fn emit_byte_for_token(&mut self, byte: u8, token: Token) {
        let line = token.line as i32;
        self.chunk().write(byte, line);
    }

    fn emit_constant_op(&mut self, opcode: OpCode, constant: u16) {
        self.emit_byte(opcode as u8);
        self.emit_bytes(constant.to_ne_bytes());
    }

    fn emit_variable_op(&mut self, opcode: OpCode, variable: u16) {
        if opcode.arg_sizes() == &[1] {
            self.emit_bytes([opcode as u8, variable as u8]);
        } else {
            self.emit_constant_op(opcode, variable);
        }
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
        if self.compiler().in_try_block {
            self.emit_byte(OpCode::JumpFinally as u8);
        }
        self.emit_byte(OpCode::Return as u8);
    }

    fn emit_scope_end(&mut self, pop_locals: bool, scope_depth: usize) {
        let mut opcodes = Vec::new();
        for local in self.compiler().locals.iter().rev() {
            if local.depth.unwrap() <= scope_depth {
                break;
            }
            let opcode = if local.is_captured {
                OpCode::CloseUpvalue
            } else {
                OpCode::Pop
            };
            opcodes.push(opcode as u8);
        }
        for &opcode in &opcodes {
            self.emit_byte(opcode);
            if pop_locals {
                self.compiler_mut().locals.pop();
            }
        }
    }

    fn make_constant(&mut self, value: value::Value) -> u16 {
        let constant = self.chunk().add_constant(value);
        if constant > u16::MAX as usize {
            self.error("Too many constants in one chunk.");
            return 0;
        }
        constant as u16
    }

    fn emit_constant(&mut self, value: value::Value) {
        let constant = self.make_constant(value);
        self.emit_byte(OpCode::Constant as u8);
        self.emit_bytes(constant.to_ne_bytes());
    }

    fn patch_jump(&mut self, offset: usize) {
        match self.compiler_mut().patch_jump(offset) {
            Ok(_) => {}
            Err(e) => self.compiler_error(e),
        }
    }

    fn patch_offset_at(&mut self, pos: usize, offset: usize) {
        let jump = self.chunk().code.len() - offset;
        if jump > common::JUMP_SIZE_MAX {
            self.error("Too much code in block.");
        }

        let bytes = (jump as u16).to_ne_bytes();

        self.chunk().code[pos] = bytes[0];
        self.chunk().code[pos + 1] = bytes[1];
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

    fn identifier_constant(&mut self, token: &Token) -> u16 {
        let value = Value::ObjString(self.vm.new_gc_obj_string(&token.source));
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

    fn parse_variable(&mut self, error_message: &str) -> u16 {
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
        self.compiler_mut().mark_last_initialised();
    }

    fn define_variable(&mut self, global: u16) {
        if self.compiler().scope_depth > 0 {
            self.mark_initialised();
            return;
        }

        self.emit_byte(OpCode::DefineGlobal as u8);
        self.emit_bytes(global.to_ne_bytes());
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
                self.compiler_mut().function.arity += 1;
                if self.compiler().function.arity > 256 {
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

        write!(
            error_string,
            "[module \"{}\", line {}] Error",
            self.module_path.as_str(),
            token.line
        )
        .unwrap();

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
            CompilerError::InvalidControlStatement => {
                let msg = format!(
                    "Cannot use '{}' statement outside of loop body.",
                    if self.previous.kind == TokenKind::Break {
                        "break"
                    } else {
                        "continue"
                    }
                );
                self.error(&msg);
            }
            CompilerError::JumpTooLarge => self.error("Too much code to jump over."),
            CompilerError::ReadVarInInitialiser => {
                self.error("Cannot read local variable in its own initialiser.");
            }
            CompilerError::TooManyClosureVars => {
                self.error("Too many closure variables in function.");
            }
            _ => {}
        }
    }

    fn check_no_attributes(&mut self) {
        if let Some(opener) = self.attribute_opener.take() {
            self.error_at(opener, "Unexpected attribute list.");
        }
        self.attributes.clear();
    }

    fn check_supported_attributes(&mut self, kind: &str) {
        for attr in self.attributes.values() {
            let msg = format!("Unsupported {} attribute '{}'.", kind, attr.name.source);
            self.error_at(attr.name.clone(), &msg);
        }
        self.attributes.clear();
        self.attribute_opener = None;
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

    fn binary_assign(&mut self, get_op: OpCode, variable: u16) {
        self.single_target_mode = true;
        let op_kind = self.previous.kind;
        self.emit_variable_op(get_op, variable);
        self.expression();
        match op_kind {
            TokenKind::MinusEqual => self.emit_byte(OpCode::Subtract as u8),
            TokenKind::PlusEqual => self.emit_byte(OpCode::Add as u8),
            TokenKind::SlashEqual => self.emit_byte(OpCode::Divide as u8),
            TokenKind::StarEqual => self.emit_byte(OpCode::Multiply as u8),
            TokenKind::AmpEqual => self.emit_byte(OpCode::BitwiseAnd as u8),
            TokenKind::BarEqual => self.emit_byte(OpCode::BitwiseOr as u8),
            TokenKind::CaretEqual => self.emit_byte(OpCode::BitwiseXor as u8),
            TokenKind::PercentEqual => self.emit_byte(OpCode::Modulo as u8),
            TokenKind::LessLessEqual => self.emit_byte(OpCode::BitShiftLeft as u8),
            TokenKind::GreaterGreaterEqual => self.emit_byte(OpCode::BitShiftRight as u8),
            _ => unreachable!(),
        }
        self.single_target_mode = false;
    }

    fn resolve_variable(&mut self, name: &Token) -> (OpCode, OpCode, u16) {
        if let Some(result) = self.resolve_local(&name) {
            (OpCode::GetLocal, OpCode::SetLocal, result as u16)
        } else if let Some(result) = self.resolve_upvalue(&name) {
            (OpCode::GetUpvalue, OpCode::SetUpvalue, result as u16)
        } else {
            (
                OpCode::GetGlobal,
                OpCode::SetGlobal,
                self.identifier_constant(&name),
            )
        }
    }

    fn named_variable(&mut self, name: Token, can_assign: bool) {
        let (get_op, set_op, arg) = self.resolve_variable(&name);

        if can_assign && self.match_token(TokenKind::Equal) {
            self.expression();
            self.emit_variable_op(set_op, arg);
        } else if can_assign && self.match_binary_assignment() {
            self.binary_assign(get_op, arg);
            self.emit_variable_op(set_op, arg);
        } else {
            if get_op.arg_sizes() == &[1] {
                self.emit_bytes([get_op as u8, arg as u8]);
            } else {
                self.emit_constant_op(get_op, arg);
            }
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
        let mut single_elem_tuple = false;
        let mut num_elems: usize = 0;
        if !s.check(TokenKind::RightParen) {
            loop {
                s.expression();
                if num_elems == 255 {
                    s.error("Cannot have more than 255 Tuple elements.");
                }
                num_elems += 1;

                if !s.match_token(TokenKind::Comma) {
                    break;
                }
                if num_elems == 1 && s.check(TokenKind::RightParen) {
                    single_elem_tuple = true;
                    break;
                }
            }
        }

        let is_tuple = num_elems != 1 || single_elem_tuple;
        if is_tuple {
            s.emit_bytes([OpCode::BuildTuple as u8, num_elems as u8]);
        }

        let msg = &format!(
            "Expected ')' after {}.",
            if is_tuple { "elements" } else { "expression" }
        );
        s.consume(TokenKind::RightParen, msg);
    }

    fn binary(s: &mut Parser, _can_assign: bool) {
        let operator_kind = s.previous.kind;
        let rule_precedence = s.get_rule(operator_kind).precedence;
        s.parse_precedence(Precedence::from(rule_precedence as usize + 1));

        match operator_kind {
            TokenKind::BangEqual => s.emit_bytes([OpCode::Equal as u8, OpCode::LogicalNot as u8]),
            TokenKind::EqualEqual => s.emit_byte(OpCode::Equal as u8),
            TokenKind::Greater => s.emit_byte(OpCode::Greater as u8),
            TokenKind::GreaterEqual => s.emit_bytes([OpCode::Less as u8, OpCode::LogicalNot as u8]),
            TokenKind::Less => s.emit_byte(OpCode::Less as u8),
            TokenKind::LessEqual => s.emit_bytes([OpCode::Greater as u8, OpCode::LogicalNot as u8]),
            TokenKind::Plus => s.emit_byte(OpCode::Add as u8),
            TokenKind::Minus => s.emit_byte(OpCode::Subtract as u8),
            TokenKind::Star => s.emit_byte(OpCode::Multiply as u8),
            TokenKind::Slash => s.emit_byte(OpCode::Divide as u8),
            TokenKind::Amp => s.emit_byte(OpCode::BitwiseAnd as u8),
            TokenKind::Bar => s.emit_byte(OpCode::BitwiseOr as u8),
            TokenKind::Caret => s.emit_byte(OpCode::BitwiseXor as u8),
            TokenKind::Percent => s.emit_byte(OpCode::Modulo as u8),
            TokenKind::LessLess => s.emit_byte(OpCode::BitShiftLeft as u8),
            TokenKind::GreaterGreater => s.emit_byte(OpCode::BitShiftRight as u8),
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
            s.emit_constant_op(OpCode::SetProperty, name);
        } else if can_assign && s.match_binary_assignment() {
            s.emit_byte(OpCode::CopyTop as u8);
            s.binary_assign(OpCode::GetProperty, name);
            s.emit_constant_op(OpCode::SetProperty, name);
        } else if s.match_token(TokenKind::LeftParen) {
            let arg_count = s.argument_list(
                TokenKind::RightParen,
                "Cannot have more than 255 arguments.",
                "Expected ')' after arguments.",
            );
            s.emit_constant_op(OpCode::Invoke, name);
            s.emit_byte(arg_count);
        } else {
            s.emit_constant_op(OpCode::GetProperty, name);
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
        s.emit_constant_op(OpCode::Invoke, name);
        s.emit_byte(num_args as u8);
    }

    fn lambda(s: &mut Parser, _can_assign: bool) {
        let lambda_count = s.compiler().lambda_count;
        s.compiler_mut().lambda_count += 1;
        let name =
            s.vm.new_gc_obj_string(format!("lambda-{}", lambda_count).as_str());
        s.new_compiler(FunctionKind::Function, name, s.module_path);
        s.begin_scope();

        if s.previous.kind == TokenKind::Bar {
            s.parameter_list(
                TokenKind::Bar,
                "Cannot have more than 255 parameters.",
                "Expected parameter name.",
            );
            s.consume(TokenKind::Bar, "Expected ')' after parameters.");
        }

        if s.match_token(TokenKind::LeftBrace) {
            s.block();
        } else {
            s.expression();
            s.emit_byte(OpCode::Return as u8);
        }

        let (function, upvalues) = s.finalise_compiler();

        let constant = s.make_constant(value::Value::ObjFunction(function.as_gc()));
        s.emit_constant_op(OpCode::Closure, constant);

        for upvalue in upvalues.iter() {
            s.emit_byte(upvalue.is_local as u8);
            s.emit_byte(upvalue.index as u8);
        }
    }

    fn hash_map(s: &mut Parser, _can_assign: bool) {
        let mut num_entries: usize = 0;
        if !s.check(TokenKind::RightBrace) {
            loop {
                s.expression();
                s.consume(TokenKind::Colon, "Expected ':' after key.");
                s.expression();

                if num_entries == 255 {
                    s.error("Cannot have more than 255 HashMap entries.");
                }
                num_entries += 1;

                if !s.match_token(TokenKind::Comma) {
                    break;
                }
            }
        }

        s.consume(TokenKind::RightBrace, "Expected '}' after elements.");
        s.emit_bytes([OpCode::BuildHashMap as u8, num_entries as u8]);
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
            TokenKind::Bang => s.emit_byte(OpCode::LogicalNot as u8),
            TokenKind::Tilde => s.emit_byte(OpCode::BitwiseNot as u8),
            _ => {}
        }
    }

    fn number(s: &mut Parser, _can_assign: bool) {
        let value = match s.previous.source.as_str().parse::<f64>() {
            Ok(n) => n,
            Err(_) => {
                s.error("Unable to parse number.");
                return;
            }
        };
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
        let value = Value::ObjString(s.vm.new_gc_obj_string(&s.previous.source));
        s.emit_constant(value);
    }

    fn interpolation(s: &mut Parser, _can_assign: bool) {
        let mut arg_count = 0;
        loop {
            if !s.previous.source.is_empty() {
                let value = Value::ObjString(s.vm.new_gc_obj_string(&s.previous.source));
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
            let value = Value::ObjString(s.vm.new_gc_obj_string(s.previous.source.as_str()));
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
        if s.compiler().kind == FunctionKind::StaticMethod {
            s.error("Cannot use 'self' in a static method.");
            return;
        }
        Parser::variable(s, false);
    }

    fn cap_self(s: &mut Parser, _can_assign: bool) {
        if s.class_compilers.is_empty() {
            s.error("Cannot use 'Self' outside of a class.");
            return;
        }
        // TODO: Optimise this access to generate a single opcode
        Parser::variable(s, false);
        s.emit_byte(OpCode::GetClass as u8);
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
            s.emit_constant_op(OpCode::SuperInvoke, name);
            s.emit_byte(arg_count);
        } else {
            s.named_variable(Token::from_string("super"), false);
            s.emit_constant_op(OpCode::GetSuper, name);
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

const RULES: [ParseRule; 72] = [
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
        prefix: Some(Parser::hash_map),
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
    // Colon
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
    // Amp
    ParseRule {
        prefix: None,
        infix: Some(Parser::binary),
        precedence: Precedence::BitwiseAnd,
    },
    // AmpEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Bar
    ParseRule {
        prefix: Some(Parser::lambda),
        infix: Some(Parser::binary),
        precedence: Precedence::BitwiseOr,
    },
    // BarEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Caret
    ParseRule {
        prefix: None,
        infix: Some(Parser::binary),
        precedence: Precedence::BitwiseXor,
    },
    // CaretEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Percent
    ParseRule {
        prefix: None,
        infix: Some(Parser::binary),
        precedence: Precedence::Factor,
    },
    // PercentEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // GreaterGreater
    ParseRule {
        prefix: None,
        infix: Some(Parser::binary),
        precedence: Precedence::BitShift,
    },
    // GreaterGreaterEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // LessLess
    ParseRule {
        prefix: None,
        infix: Some(Parser::binary),
        precedence: Precedence::BitShift,
    },
    // LessLessEqual
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // AmpAmp
    ParseRule {
        prefix: None,
        infix: Some(Parser::and),
        precedence: Precedence::And,
    },
    // BarBar
    ParseRule {
        prefix: Some(Parser::lambda),
        infix: Some(Parser::or),
        precedence: Precedence::Or,
    },
    // Tilde
    ParseRule {
        prefix: Some(Parser::unary),
        infix: None,
        precedence: Precedence::None,
    },
    // Hash
    ParseRule {
        prefix: None,
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
    // CapSelf
    ParseRule {
        prefix: Some(Parser::cap_self),
        infix: None,
        precedence: Precedence::None,
    },
    // Catch
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
        prefix: Some(Parser::literal),
        infix: None,
        precedence: Precedence::None,
    },
    // Finally
    ParseRule {
        prefix: None,
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
    // Import
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // As
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
    // Break
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Continue
    ParseRule {
        prefix: None,
        infix: None,
        precedence: Precedence::None,
    },
    // Throw
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
    // Try
    ParseRule {
        prefix: None,
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
