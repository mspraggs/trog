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

use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{self, Write};
use std::time;

use crate::chunk::{Chunk, OpCode};
use crate::common;
use crate::compiler;
use crate::debug;
use crate::hash::BuildPassThroughHasher;
use crate::memory::{self, Gc, GcManaged};
use crate::object::{self, NativeFn, ObjClass, ObjClosure, ObjFunction, ObjString, ObjUpvalue};
use crate::value::Value;

const FRAMES_MAX: usize = 64;
const STACK_MAX: usize = common::LOCALS_MAX * FRAMES_MAX;

#[derive(Clone, Debug)]
pub enum VmError {
    AttributeError(Vec<String>),
    CompileError(Vec<String>),
    IndexError(Vec<String>),
    RuntimeError(Vec<String>),
    TypeError(Vec<String>),
    ValueError(Vec<String>),
}

impl VmError {
    pub fn add_message(&mut self, message: String) {
        match self {
            VmError::AttributeError(msgs) => msgs.push(message),
            VmError::CompileError(msgs) => msgs.push(message),
            VmError::IndexError(msgs) => msgs.push(message),
            VmError::RuntimeError(msgs) => msgs.push(message),
            VmError::TypeError(msgs) => msgs.push(message),
            VmError::ValueError(msgs) => msgs.push(message),
        }
    }
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let messages = match self {
            VmError::AttributeError(msgs) => msgs,
            VmError::CompileError(msgs) => msgs,
            VmError::IndexError(msgs) => msgs,
            VmError::RuntimeError(msgs) => msgs,
            VmError::TypeError(msgs) => msgs,
            VmError::ValueError(msgs) => msgs,
        };

        for msg in messages {
            match writeln!(f, "{}", msg) {
                Ok(()) => {}
                Err(error) => {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

pub fn interpret(vm: &mut Vm, source: String) -> Result<(), VmError> {
    let compile_result = compiler::compile(vm, source);
    match compile_result {
        Ok(function) => vm.interpret(function),
        Err(errors) => Err(VmError::CompileError(errors)),
    }
}

pub struct CallFrame {
    closure: Gc<RefCell<ObjClosure>>,
    prev_ip: usize,
    slot_base: usize,
}

impl memory::GcManaged for CallFrame {
    fn mark(&self) {
        self.closure.mark();
    }

    fn blacken(&self) {
        self.closure.blacken();
    }
}

pub struct Vm {
    ip: usize,
    active_chunk_index: usize,
    chunks: Vec<Chunk>,
    frames: Vec<CallFrame>,
    stack: Vec<Value>,
    globals: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
    open_upvalues: Vec<Gc<RefCell<ObjUpvalue>>>,
    ephemeral_roots: Vec<Gc<dyn GcManaged>>,
    strings: HashMap<u64, Gc<ObjString>>,
    init_string: Gc<ObjString>,
}

impl Default for Vm {
    fn default() -> Self {
        let mut vm = Vm {
            ip: 0,
            active_chunk_index: 0,
            chunks: Vec::new(),
            frames: Vec::with_capacity(FRAMES_MAX),
            stack: Vec::with_capacity(STACK_MAX),
            globals: HashMap::with_hasher(BuildPassThroughHasher::default()),
            open_upvalues: Vec::new(),
            ephemeral_roots: Vec::new(),
            strings: HashMap::new(),
            init_string: Gc::dangling(),
        };
        vm.init_string = object::new_gc_obj_string(&mut vm, "init");
        vm
    }
}

fn clock_native(_arg_count: usize, _args: &mut [Value]) -> Result<Value, VmError> {
    let duration = match time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH) {
        Ok(value) => value,
        Err(_) => {
            return Err(VmError::RuntimeError(vec![
                "Error calling native function.".to_owned(),
            ]));
        }
    };
    let seconds = duration.as_secs_f64();
    let nanos = duration.subsec_nanos() as f64 / 1e9;
    Ok(Value::Number(seconds + nanos))
}

impl Vm {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn interpret(&mut self, function: Gc<ObjFunction>) -> Result<(), VmError> {
        self.push_ephemeral_root(function.as_base());
        self.define_native("clock", clock_native);
        let closure = object::new_gc_obj_closure(self, function);
        self.pop_ephemeral_root();
        self.push(Value::ObjClosure(closure));
        self.call_value(Value::ObjClosure(closure), 0)?;
        match self.run() {
            Ok(()) => Ok(()),
            Err(mut error) => Err(self.runtime_error(&mut error)),
        }
    }

    pub fn mark_roots(&mut self) {
        self.chunks.mark();
        self.stack.mark();
        self.globals.mark();
        self.frames.mark();
        self.open_upvalues.mark();
        self.ephemeral_roots.mark();
        self.strings.mark();
    }

    pub fn push_ephemeral_root(&mut self, root: Gc<dyn GcManaged>) {
        self.ephemeral_roots.push(root);
    }

    pub fn pop_ephemeral_root(&mut self) -> Gc<dyn GcManaged> {
        self.ephemeral_roots.pop().expect("Ephemeral roots empty.")
    }

    pub fn new_chunk(&mut self) -> usize {
        self.chunks.push(Chunk::new());
        self.chunks.len() - 1
    }

    pub fn get_chunk(&self, index: usize) -> &Chunk {
        &self.chunks[index]
    }

    pub fn get_chunk_mut(&mut self, index: usize) -> &mut Chunk {
        &mut self.chunks[index]
    }

    pub fn get_string(&self, hash: u64) -> Option<&Gc<ObjString>> {
        self.strings.get(&hash)
    }

    pub fn add_string(&mut self, string: Gc<ObjString>) {
        self.strings.insert(string.hash, string);
    }

    fn run(&mut self) -> Result<(), VmError> {
        macro_rules! binary_op {
            ($value_type:expr, $op:tt) => {
                {
                    let second_value = self.pop();
                    let first_value = self.pop();
                    let (first, second) = match (first_value, second_value) {
                        (
                            Value::Number(first),
                            Value::Number(second)
                        ) => (first, second),
                        _ => {
                            return Err(VmError::RuntimeError(
                                vec!["Binary operands must both be numbers.".to_owned()]
                            ));
                        }
                    };
                    self.push($value_type(first $op second));
                }
            };
        }

        macro_rules! read_byte {
            () => {{
                let ip = self.ip;
                let ret = self.active_chunk().code[ip];
                self.ip += 1;
                ret
            }};
        }

        macro_rules! read_short {
            () => {{
                let ret = u16::from_ne_bytes(
                    (&self.active_chunk().code[self.ip..self.ip + 2])
                        .try_into()
                        .unwrap(),
                );
                self.ip += 2;
                ret
            }};
        }

        macro_rules! read_constant {
            () => {{
                let index = read_byte!() as usize;
                self.active_chunk().constants[index]
            }};
        }

        macro_rules! read_string {
            () => {
                match read_constant!() {
                    Value::ObjString(s) => s,
                    _ => panic!("Expected variable name."),
                }
            };
        }

        loop {
            if cfg!(feature = "debug_trace") {
                print!("          ");
                for v in self.stack.iter() {
                    print!("[ {} ]", v);
                }
                println!();
                let ip = self.ip;
                debug::disassemble_instruction(self.active_chunk(), ip);
            }
            let instruction = OpCode::from(read_byte!());

            match instruction {
                OpCode::Constant => {
                    let constant = read_constant!();
                    self.push(constant);
                }

                OpCode::Nil => {
                    self.push(Value::None);
                }

                OpCode::True => {
                    self.push(Value::Boolean(true));
                }

                OpCode::False => {
                    self.push(Value::Boolean(false));
                }

                OpCode::Pop => {
                    self.pop();
                }

                OpCode::GetLocal => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame().slot_base;
                    let value = self.stack[slot_base + slot];
                    self.push(value);
                }

                OpCode::SetLocal => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame().slot_base;
                    self.stack[slot_base + slot] = *self.peek(0);
                }

                OpCode::GetGlobal => {
                    let name = read_string!();
                    let value = match self.globals.get(&name) {
                        Some(value) => *value,
                        None => {
                            let msg = format!("Undefined variable '{}'.", *name);
                            return Err(VmError::RuntimeError(vec![msg]));
                        }
                    };
                    self.push(value);
                }

                OpCode::DefineGlobal => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    self.globals.insert(name, value);
                    self.pop();
                }

                OpCode::SetGlobal => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    let prev = self.globals.insert(name, value);
                    match prev {
                        Some(_) => {}
                        None => {
                            self.globals.remove(&name);
                            let msg = format!("Undefined variable '{}'.", *name);
                            return Err(VmError::RuntimeError(vec![msg]));
                        }
                    }
                }

                OpCode::GetUpvalue => {
                    let upvalue_index = read_byte!() as usize;
                    let upvalue =
                        match *self.frame().closure.borrow().upvalues[upvalue_index].borrow() {
                            ObjUpvalue::Open(slot) => self.stack[slot],
                            ObjUpvalue::Closed(value) => value,
                        };
                    self.push(upvalue);
                }

                OpCode::SetUpvalue => {
                    let upvalue_index = read_byte!() as usize;
                    let stack_value = *self.peek(0);
                    let closure = self.frame().closure;
                    match *closure.borrow_mut().upvalues[upvalue_index].borrow_mut() {
                        ObjUpvalue::Open(slot) => {
                            self.stack[slot] = stack_value;
                        }
                        ObjUpvalue::Closed(ref mut value) => {
                            *value = stack_value;
                        }
                    };
                }

                OpCode::GetProperty => {
                    let instance = match *self.peek(0) {
                        Value::ObjInstance(ptr) => ptr,
                        _ => {
                            return Err(VmError::RuntimeError(vec![
                                "Only instances have properties.".to_owned(),
                            ]));
                        }
                    };
                    let name = read_string!();

                    let borrowed_instance = instance.borrow();
                    if let Some(property) = borrowed_instance.fields.get(&name) {
                        self.pop();
                        self.push(*property);
                    } else {
                        self.bind_method(borrowed_instance.class, name)?;
                    }
                }

                OpCode::SetProperty => {
                    let instance = match *self.peek(1) {
                        Value::ObjInstance(ptr) => ptr,
                        _ => {
                            return Err(VmError::RuntimeError(vec![
                                "Only instances have fields.".to_owned()
                            ]));
                        }
                    };
                    let name = read_string!();
                    let value = *self.peek(0);
                    instance.borrow_mut().fields.insert(name, value);

                    self.pop();
                    self.pop();
                    self.push(value);
                }

                OpCode::GetSuper => {
                    let name = read_string!();
                    let superclass = match self.pop() {
                        Value::ObjClass(ptr) => ptr,
                        _ => unreachable!(),
                    };

                    self.bind_method(superclass, name)?;
                }

                OpCode::Equal => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::Boolean(a == b));
                }

                OpCode::Greater => binary_op!(Value::Boolean, >),

                OpCode::Less => binary_op!(Value::Boolean, <),

                OpCode::Add => {
                    let b = self.pop();
                    let a = self.pop();
                    match (a, b) {
                        (Value::ObjString(a), Value::ObjString(b)) => {
                            let value = Value::ObjString(object::new_gc_obj_string(
                                self,
                                format!("{}{}", *a, *b).as_str(),
                            ));
                            self.stack.push(value)
                        }

                        (Value::Number(a), Value::Number(b)) => {
                            self.push(Value::Number(a + b));
                        }

                        _ => {
                            return Err(VmError::RuntimeError(vec![
                                "Binary operands must be two numbers or two strings.".to_owned(),
                            ]));
                        }
                    }
                }

                OpCode::Subtract => binary_op!(Value::Number, -),

                OpCode::Multiply => binary_op!(Value::Number, *),

                OpCode::Divide => binary_op!(Value::Number, /),

                OpCode::Not => {
                    let value = self.pop();
                    self.push(Value::Boolean(!value.as_bool()));
                }

                OpCode::Negate => {
                    let value = self.pop();
                    match value {
                        Value::Number(underlying) => {
                            self.push(Value::Number(-underlying));
                        }
                        _ => {
                            return Err(VmError::RuntimeError(vec![
                                "Unary operand must be a number.".to_owned(),
                            ]));
                        }
                    }
                }

                OpCode::Print => {
                    println!("{}", self.pop());
                }

                OpCode::Jump => {
                    let offset = read_short!();
                    self.ip += offset as usize;
                }

                OpCode::JumpIfFalse => {
                    let offset = read_short!();
                    if !self.peek(0).as_bool() {
                        self.ip += offset as usize;
                    }
                }

                OpCode::Loop => {
                    let offset = read_short!();
                    self.ip -= offset as usize;
                }

                OpCode::Call => {
                    let arg_count = read_byte!() as usize;
                    self.call_value(*self.peek(arg_count), arg_count)?;
                }

                OpCode::Invoke => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    self.invoke(method, arg_count)?;
                }

                OpCode::SuperInvoke => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    let superclass = match self.pop() {
                        Value::ObjClass(ptr) => ptr,
                        _ => unreachable!(),
                    };
                    self.invoke_from_class(superclass, method, arg_count)?;
                }

                OpCode::Closure => {
                    let function = match read_constant!() {
                        Value::ObjFunction(underlying) => underlying,
                        _ => panic!("Expected ObjFunction."),
                    };

                    let upvalue_count = function.upvalue_count;

                    let closure = object::new_gc_obj_closure(self, function);
                    self.push(Value::ObjClosure(closure));

                    for i in 0..upvalue_count {
                        let is_local = read_byte!() != 0;
                        let index = read_byte!() as usize;
                        let slot_base = self.frame().slot_base;
                        closure.borrow_mut().upvalues[i] = if is_local {
                            self.capture_upvalue(slot_base + index)
                        } else {
                            self.frame().closure.borrow().upvalues[index]
                        };
                    }
                }

                OpCode::CloseUpvalue => {
                    self.close_upvalues(self.stack.len() - 1, *self.peek(0));
                    self.pop();
                }

                OpCode::Return => {
                    let result = self.pop();
                    for i in self.frame().slot_base..self.stack.len() {
                        self.close_upvalues(i, self.stack[i])
                    }

                    let prev_stack_size = self.frame().slot_base;
                    let prev_ip = self.frame().prev_ip;
                    self.frames.pop();
                    if self.frames.is_empty() {
                        self.pop();
                        return Ok(());
                    }
                    let prev_chunk_index = self.frame().closure.borrow().function.chunk_index;
                    self.active_chunk_index = prev_chunk_index;
                    self.ip = prev_ip;

                    self.stack.truncate(prev_stack_size);
                    self.push(result);
                }

                OpCode::Class => {
                    let string = read_string!();
                    let class = object::new_gc_obj_class(self, string);
                    self.push(Value::ObjClass(class));
                }

                OpCode::Inherit => {
                    let superclass_pos = self.stack.len() - 2;
                    let superclass = match self.stack[superclass_pos] {
                        Value::ObjClass(ptr) => ptr,
                        _ => {
                            return Err(VmError::RuntimeError(vec![
                                "Superclass must be a class.".to_owned()
                            ]));
                        }
                    };
                    let subclass = match self.peek(0) {
                        Value::ObjClass(ptr) => *ptr,
                        _ => unreachable!(),
                    };
                    for (name, value) in superclass.borrow().methods.iter() {
                        subclass.borrow_mut().methods.insert(name.clone(), *value);
                    }
                    self.pop();
                }

                OpCode::Method => {
                    let name = read_string!();
                    self.define_method(name)?;
                }
            }
        }
    }

    fn call_value(&mut self, value: Value, arg_count: usize) -> Result<(), VmError> {
        match value {
            Value::ObjBoundMethod(bound) => {
                *self.peek_mut(arg_count) = bound.borrow().receiver;
                self.call(bound.borrow().method, arg_count)
            }

            Value::ObjClass(class) => {
                let instance = object::new_gc_obj_instance(self, class);
                *self.peek_mut(arg_count) = Value::ObjInstance(instance);

                if let Some(Value::ObjClosure(initialiser)) =
                    class.borrow().methods.get(&self.init_string)
                {
                    return self.call(*initialiser, arg_count);
                } else if arg_count != 0 {
                    let msg = format!("Expected 0 arguments but got {}.", arg_count);
                    return Err(VmError::TypeError(vec![msg]));
                }

                Ok(())
            }

            Value::ObjClosure(function) => self.call(function, arg_count),

            Value::ObjNative(wrapped) => {
                let function = wrapped.function.ok_or(VmError::ValueError(vec![
                    "Expected native function.".to_owned(),
                ]))?;
                let frame_begin = self.stack.len() - arg_count - 1;
                let result = function(
                    arg_count,
                    &mut self.stack[frame_begin..frame_begin + arg_count],
                )?;
                self.stack.truncate(frame_begin);
                self.push(result);
                Ok(())
            }

            _ => Err(VmError::TypeError(vec![
                "Can only call functions and classes.".to_owned(),
            ])),
        }
    }

    fn invoke_from_class(
        &mut self,
        class: Gc<RefCell<ObjClass>>,
        name: Gc<ObjString>,
        arg_count: usize,
    ) -> Result<(), VmError> {
        if let Some(value) = class.borrow().methods.get(&name) {
            return match value {
                Value::ObjClosure(closure) => self.call(*closure, arg_count),
                _ => unreachable!(),
            };
        }
        let msg = format!("Undefined property '{}'.", *name);
        Err(VmError::AttributeError(vec![msg]))
    }

    fn invoke(&mut self, name: Gc<ObjString>, arg_count: usize) -> Result<(), VmError> {
        let receiver = *self.peek(arg_count);
        match receiver {
            Value::ObjInstance(instance) => {
                if let Some(value) = instance.borrow().fields.get(&name) {
                    *self.peek_mut(arg_count) = *value;
                    return self.call_value(*value, arg_count);
                }

                self.invoke_from_class(instance.borrow().class, name, arg_count)
            }
            _ => Err(VmError::ValueError(vec![
                "Only instances have methods.".to_owned()
            ])),
        }
    }

    fn call(&mut self, closure: Gc<RefCell<ObjClosure>>, arg_count: usize) -> Result<(), VmError> {
        if arg_count as u32 != closure.borrow().function.arity {
            let msg = format!(
                "Expected {} arguments but got {}.",
                closure.borrow().function.arity,
                arg_count
            );
            return Err(VmError::TypeError(vec![msg]));
        }

        if self.frames.len() == FRAMES_MAX {
            return Err(VmError::IndexError(vec!["Stack overflow.".to_owned()]));
        }

        self.active_chunk_index = closure.borrow().function.chunk_index;
        self.frames.push(CallFrame {
            closure,
            prev_ip: self.ip,
            slot_base: self.stack.len() - arg_count - 1,
        });
        self.ip = 0;
        Ok(())
    }

    fn reset_stack(&mut self) {
        self.stack.clear();
        self.frames.clear();
    }

    fn runtime_error(&mut self, error: &mut VmError) -> VmError {
        let mut ips: Vec<usize> = self.frames.iter().skip(1).map(|f| f.prev_ip).collect();
        ips.push(self.ip);

        for (i, frame) in self.frames.iter().enumerate().rev() {
            let function = frame.closure.borrow().function;

            let mut new_msg = String::new();
            let instruction = ips[i] - 1;
            let chunk_index = frame.closure.borrow().function.chunk_index;
            write!(
                new_msg,
                "[line {}] in ",
                self.chunks[chunk_index].lines[instruction]
            )
            .expect("Unable to write error to buffer.");
            if function.name.is_empty() {
                write!(new_msg, "script").expect("Unable to write error to buffer.");
            } else {
                write!(new_msg, "{}()", *function.name).expect("Unable to write error to buffer.");
            }
            error.add_message(new_msg);
        }

        self.reset_stack();

        error.clone()
    }

    fn define_native(&mut self, name: &str, function: NativeFn) {
        let value = Value::ObjNative(object::new_gc_obj_native(self, function));
        self.push(value);
        let value = *self.peek(0);
        let name = object::new_gc_obj_string(self, name);
        self.globals.insert(name, value);
        self.pop();
    }

    fn define_method(&mut self, name: Gc<ObjString>) -> Result<(), VmError> {
        let method = *self.peek(0);
        let class = match *self.peek(1) {
            Value::ObjClass(ptr) => ptr,
            _ => unreachable!(),
        };
        class.borrow_mut().methods.insert(name, method);
        self.pop();

        Ok(())
    }

    fn bind_method(
        &mut self,
        class: Gc<RefCell<ObjClass>>,
        name: Gc<ObjString>,
    ) -> Result<(), VmError> {
        let borrowed_class = class.borrow();
        let method = match borrowed_class.methods.get(&name) {
            Some(Value::ObjClosure(ptr)) => *ptr,
            None => {
                let msg = format!("Undefined property '{}'.", *name);
                return Err(VmError::AttributeError(vec![msg]));
            }
            _ => unreachable!(),
        };

        let instance = *self.peek(0);
        let bound = object::new_gc_obj_bound_method(self, instance, method);
        self.pop();
        self.push(Value::ObjBoundMethod(bound));

        Ok(())
    }

    fn capture_upvalue(&mut self, location: usize) -> Gc<RefCell<ObjUpvalue>> {
        let result = self
            .open_upvalues
            .iter()
            .find(|&u| u.borrow().is_open_with_index(location));

        let upvalue = if let Some(upvalue) = result {
            *upvalue
        } else {
            object::new_gc_obj_upvalue(self, location)
        };

        self.open_upvalues.push(upvalue);
        upvalue
    }

    fn close_upvalues(&mut self, last: usize, value: Value) {
        for upvalue in self.open_upvalues.iter() {
            if upvalue.borrow().is_open_with_index(last) {
                upvalue.borrow_mut().close(value);
            }
        }

        self.open_upvalues.retain(|u| u.borrow().is_open());
    }

    fn active_chunk(&self) -> &Chunk {
        &self.chunks[self.active_chunk_index]
    }

    fn frame(&self) -> &CallFrame {
        self.frames.last().expect("Call stack empty.")
    }

    fn peek(&self, depth: usize) -> &Value {
        let stack_len = self.stack.len();
        &self.stack[stack_len - depth - 1]
    }

    fn peek_mut(&mut self, depth: usize) -> &mut Value {
        let stack_len = self.stack.len();
        &mut self.stack[stack_len - depth - 1]
    }

    fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("Stack empty.")
    }
}
