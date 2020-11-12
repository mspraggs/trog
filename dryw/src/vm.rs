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
use std::fmt::Write;
use std::time;

use crate::chunk::{Chunk, OpCode};
use crate::common;
use crate::compiler;
use crate::debug;
use crate::error::{Error, ErrorKind};
use crate::hash::BuildPassThroughHasher;
use crate::memory::{self, Gc, GcManaged, Root};
use crate::object::{self, NativeFn, ObjClass, ObjClosure, ObjFunction, ObjString, ObjUpvalue};
use crate::value::Value;

const FRAMES_MAX: usize = 64;
const STACK_MAX: usize = common::LOCALS_MAX * FRAMES_MAX;

pub fn interpret(vm: &mut Vm, source: String) -> Result<Value, Error> {
    let compile_result = compiler::compile(vm, source);
    match compile_result {
        Ok(function) => vm.execute(function, &[]),
        Err(error) => Err(error),
    }
}

pub struct CallFrame {
    closure: Gc<RefCell<ObjClosure>>,
    prev_ip: usize,
    slot_base: usize,
}

impl GcManaged for CallFrame {
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
    init_string: Gc<ObjString>,
}

impl Default for Vm {
    fn default() -> Self {
        Vm {
            ip: 0,
            active_chunk_index: 0,
            chunks: Vec::new(),
            frames: Vec::with_capacity(FRAMES_MAX),
            stack: Vec::with_capacity(STACK_MAX),
            globals: HashMap::with_hasher(BuildPassThroughHasher::default()),
            open_upvalues: Vec::new(),
            init_string: object::new_gc_obj_string("init"),
        }
    }
}

fn clock_native(_args: &mut [Value]) -> Result<Value, Error> {
    let duration = match time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH) {
        Ok(value) => value,
        Err(_) => {
            return error!(ErrorKind::RuntimeError, "Error calling native function.");
        }
    };
    let seconds = duration.as_secs_f64();
    let nanos = duration.subsec_nanos() as f64 / 1e9;
    Ok(Value::Number(seconds + nanos))
}

fn default_print(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 2 {
        return error!(ErrorKind::RuntimeError, "Expected one argument to 'print'.");
    }
    println!("{}", args[1]);
    Ok(Value::None)
}

pub fn new_root_vm() -> Root<Vm> {
    let mut vm = memory::allocate_root(Vm::new());
    vm.define_native("clock", Box::new(clock_native));
    vm.define_native("print", Box::new(default_print));
    let obj_vec_class = object::new_root_obj_vec_class();
    vm.set_global("Vec", Value::ObjClass(obj_vec_class.as_gc()));
    vm
}

impl Vm {
    fn new() -> Self {
        Default::default()
    }

    pub fn execute(&mut self, function: Root<ObjFunction>, args: &[Value]) -> Result<Value, Error> {
        let closure = object::new_gc_obj_closure(function.as_gc());
        self.push(Value::ObjClosure(closure));
        self.stack.extend_from_slice(args);
        self.call_value(Value::ObjClosure(closure), args.len())?;
        match self.run() {
            Ok(value) => Ok(value),
            Err(mut error) => Err(self.runtime_error(&mut error)),
        }
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

    pub fn get_global(&self, name: &str) -> Option<Value> {
        let name = object::new_gc_obj_string(name);
        self.globals.get(&name).map(|v| *v)
    }

    pub fn set_global(&mut self, name: &str, value: Value) {
        let name = object::new_gc_obj_string(name);
        self.globals.insert(name, value);
    }

    pub fn define_native(&mut self, name: &str, function: NativeFn) {
        let native = object::new_root_obj_native(function);
        let name = object::new_gc_obj_string(name);
        self.globals.insert(name, Value::ObjNative(native.as_gc()));
    }

    fn run(&mut self) -> Result<Value, Error> {
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
                            return error!(
                                ErrorKind::RuntimeError, "Binary operands must both be numbers."
                            );
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
                read_constant!()
                    .try_as_obj_string()
                    .expect("Expected variable name.")
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
                            return error!(
                                ErrorKind::RuntimeError,
                                "Undefined variable '{}'.", *name
                            );
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
                    if prev.is_none() {
                        self.globals.remove(&name);
                        return error!(ErrorKind::RuntimeError, "Undefined variable '{}'.", *name);
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
                    if let Value::ObjVec(vec) = *self.peek(0) {
                        let name = read_string!();
                        self.bind_method(vec.borrow().class, name)?;
                        continue;
                    }
                    let instance = if let Some(ptr) = self.peek(0).try_as_obj_instance() {
                        ptr
                    } else {
                        return error!(ErrorKind::RuntimeError, "Only instances have properties.",);
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
                    let instance = if let Some(ptr) = self.peek(1).try_as_obj_instance() {
                        ptr
                    } else {
                        return error!(ErrorKind::RuntimeError, "Only instances have fields.");
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
                    let superclass = self.pop().try_as_obj_class().expect("Expected ObjClass.");

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
                                format!("{}{}", *a, *b).as_str(),
                            ));
                            self.stack.push(value)
                        }

                        (Value::Number(a), Value::Number(b)) => {
                            self.push(Value::Number(a + b));
                        }

                        _ => {
                            return error!(
                                ErrorKind::RuntimeError,
                                "Binary operands must be two numbers or two strings.",
                            );
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
                    if let Some(num) = value.try_as_number() {
                        self.push(Value::Number(-num));
                    } else {
                        return error!(ErrorKind::RuntimeError, "Unary operand must be a number.",);
                    }
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

                    let closure = object::new_gc_obj_closure(function);
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
                        return Ok(self.pop());
                    }
                    let prev_chunk_index = self.frame().closure.borrow().function.chunk_index;
                    self.active_chunk_index = prev_chunk_index;
                    self.ip = prev_ip;

                    self.stack.truncate(prev_stack_size);
                    self.push(result);
                }

                OpCode::Class => {
                    let string = read_string!();
                    let class = object::new_gc_obj_class(string);
                    self.push(Value::ObjClass(class));
                }

                OpCode::Inherit => {
                    let superclass = if let Some(ptr) = self.peek(1).try_as_obj_class() {
                        ptr
                    } else {
                        return error!(ErrorKind::RuntimeError, "Superclass must be a class.");
                    };
                    let subclass = self.peek(0).try_as_obj_class().expect("Expected ObjClass.");
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

    fn call_value(&mut self, value: Value, arg_count: usize) -> Result<(), Error> {
        match value {
            Value::ObjBoundMethod(bound) => {
                *self.peek_mut(arg_count) = bound.borrow().receiver;
                self.call(bound.borrow().method, arg_count)
            }

            Value::ObjBoundNative(bound) => {
                *self.peek_mut(arg_count) = bound.borrow().receiver;
                self.call_value(Value::ObjNative(bound.borrow().method), arg_count)
            }

            Value::ObjClass(class) => {
                let instance = object::new_gc_obj_instance(class);
                *self.peek_mut(arg_count) = Value::ObjInstance(instance);

                let borrowed_class = class.borrow();
                let init = borrowed_class.methods.get(&self.init_string);
                if let Some(Value::ObjClosure(initialiser)) = init {
                    return self.call(*initialiser, arg_count);
                } else if let Some(Value::ObjNative(initialiser)) = init {
                    return self.call_value(Value::ObjNative(*initialiser), arg_count);
                } else if arg_count != 0 {
                    return error!(
                        ErrorKind::TypeError,
                        "Expected 0 arguments but got {}.", arg_count
                    );
                }

                Ok(())
            }

            Value::ObjClosure(function) => self.call(function, arg_count),

            Value::ObjNative(mut wrapped) => {
                let function = wrapped.function.as_mut();
                let frame_begin = self.stack.len() - arg_count - 1;
                let result = function(&mut self.stack[frame_begin..frame_begin + arg_count + 1])?;
                self.stack.truncate(frame_begin);
                self.push(result);
                Ok(())
            }

            _ => error!(ErrorKind::TypeError, "Can only call functions and classes."),
        }
    }

    fn invoke_from_class(
        &mut self,
        class: Gc<RefCell<ObjClass>>,
        name: Gc<ObjString>,
        arg_count: usize,
    ) -> Result<(), Error> {
        if let Some(value) = class.borrow().methods.get(&name) {
            return match value {
                Value::ObjClosure(closure) => self.call(*closure, arg_count),
                Value::ObjNative(native) => self.call_value(Value::ObjNative(*native), arg_count),
                _ => unreachable!(),
            };
        }
        error!(ErrorKind::AttributeError, "Undefined property '{}'.", *name)
    }

    fn invoke(&mut self, name: Gc<ObjString>, arg_count: usize) -> Result<(), Error> {
        let receiver = *self.peek(arg_count);
        match receiver {
            Value::ObjInstance(instance) => {
                if let Some(value) = instance.borrow().fields.get(&name) {
                    *self.peek_mut(arg_count) = *value;
                    return self.call_value(*value, arg_count);
                }

                self.invoke_from_class(instance.borrow().class, name, arg_count)
            }
            Value::ObjVec(vec) => {
                let class = { vec.borrow().class };
                self.invoke_from_class(class, name, arg_count)
            }
            _ => error!(ErrorKind::ValueError, "Only instances have methods."),
        }
    }

    fn call(&mut self, closure: Gc<RefCell<ObjClosure>>, arg_count: usize) -> Result<(), Error> {
        if arg_count as u32 + 1 != closure.borrow().function.arity {
            return error!(
                ErrorKind::TypeError,
                "Expected {} arguments but got {}.",
                closure.borrow().function.arity - 1,
                arg_count
            );
        }

        if self.frames.len() == FRAMES_MAX {
            return error!(ErrorKind::IndexError, "Stack overflow.");
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

    fn runtime_error(&mut self, error: &mut Error) -> Error {
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
            error.add_message(new_msg.as_str());
        }

        self.reset_stack();

        error.clone()
    }

    fn define_method(&mut self, name: Gc<ObjString>) -> Result<(), Error> {
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
    ) -> Result<(), Error> {
        let borrowed_class = class.borrow();
        let instance = *self.peek(0);
        let bound = match borrowed_class.methods.get(&name) {
            Some(Value::ObjClosure(ptr)) => {
                Value::ObjBoundMethod(object::new_gc_obj_bound_method(instance, *ptr))
            }
            Some(Value::ObjNative(ptr)) => {
                Value::ObjBoundNative(object::new_gc_obj_bound_method(instance, *ptr))
            }
            None => {
                return error!(ErrorKind::AttributeError, "Undefined property '{}'.", *name);
            }
            _ => unreachable!(),
        };
        self.pop();
        self.push(bound);
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
            object::new_gc_obj_upvalue(location)
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

impl GcManaged for Vm {
    fn mark(&self) {
        self.chunks.mark();
        self.stack.mark();
        self.globals.mark();
        self.frames.mark();
        self.open_upvalues.mark();
    }

    fn blacken(&self) {
        self.chunks.blacken();
        self.stack.blacken();
        self.globals.blacken();
        self.frames.blacken();
        self.open_upvalues.blacken();
    }
}
