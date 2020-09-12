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
use std::time;

use crate::chunk;
use crate::common;
use crate::compiler;
use crate::debug;
use crate::memory;
use crate::object;
use crate::value;

const FRAMES_MAX: usize = 64;
const STACK_MAX: usize = common::LOCALS_MAX * FRAMES_MAX;

#[derive(Debug)]
pub enum VmError {
    AttributeError,
    CompileError,
    IndexError,
    RuntimeError,
    TypeError,
    ValueError,
}

pub fn interpret(vm: &mut Vm, source: String) -> Result<(), VmError> {
    let compile_result = compiler::compile(source);
    match compile_result {
        Some(function) => vm.interpret(function),
        None => Err(VmError::CompileError),
    }
}

struct CallFrame {
    closure: memory::Gc<RefCell<object::ObjClosure>>,
    ip: usize,
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
    frames: memory::UniqueRoot<Vec<CallFrame>>,
    stack: memory::UniqueRoot<Vec<value::Value>>,
    globals: memory::UniqueRoot<HashMap<String, value::Value>>,
    open_upvalues: memory::UniqueRoot<Vec<memory::Gc<RefCell<object::ObjUpvalue>>>>,
    init_string: memory::UniqueRoot<object::ObjString>,
}

impl Default for Vm {
    fn default() -> Self {
        Vm {
            frames: memory::allocate_unique(Vec::with_capacity(FRAMES_MAX)),
            stack: memory::allocate_unique(Vec::with_capacity(STACK_MAX)),
            globals: memory::allocate_unique(HashMap::new()),
            open_upvalues: memory::allocate_unique(Vec::new()),
            init_string: memory::allocate_unique(object::ObjString::new(String::from("init"))),
        }
    }
}

fn clock_native(_arg_count: usize, _args: &mut [value::Value]) -> value::Value {
    let duration = time::SystemTime::now()
        .duration_since(time::SystemTime::UNIX_EPOCH)
        .unwrap();
    let seconds = duration.as_secs_f64();
    let nanos = duration.subsec_nanos() as f64 / 1e9;
    value::Value::Number(seconds + nanos)
}

impl Vm {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn interpret(
        &mut self,
        function: memory::Root<object::ObjFunction>,
    ) -> Result<(), VmError> {
        self.define_native("clock", clock_native);
        self.push(value::Value::ObjFunction(function.as_gc()));

        let closure = memory::allocate(RefCell::new(object::ObjClosure::new(function.as_gc())));
        self.pop()?;
        self.push(value::Value::ObjClosure(closure.as_gc()));
        self.call_value(value::Value::ObjClosure(closure.as_gc()), 0)?;
        self.run()
    }

    fn run(&mut self) -> Result<(), VmError> {
        macro_rules! binary_op {
            ($value_type:expr, $op:tt) => {
                {
                    let second_value = self.pop()?;
                    let first_value = self.pop()?;
                    let (first, second) = match (first_value, second_value) {
                        (
                            value::Value::Number(first),
                            value::Value::Number(second)
                        ) => (first, second),
                        _ => {
                            self.runtime_error("Binary operands must both be numbers.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    self.push($value_type(first $op second));
                }
            };
        }

        macro_rules! read_byte {
            () => {{
                let ip = self.frame()?.ip;
                let ret = self.frame()?.closure.borrow().function.chunk.code[ip];
                self.frames.last_mut().ok_or(VmError::IndexError)?.ip += 1;
                ret
            }};
        }

        macro_rules! read_short {
            () => {{
                let ret = ((self.frame()?.closure.borrow().function.chunk.code[self.frame()?.ip]
                    as u16)
                    << 8)
                    | self.frame()?.closure.borrow().function.chunk.code[self.frame()?.ip + 1]
                        as u16;
                self.frame_mut()?.ip += 2;
                ret
            }};
        }

        macro_rules! read_constant {
            () => {{
                let index = read_byte!() as usize;
                self.frame()?.closure.borrow().function.chunk.constants[index]
            }};
        }

        macro_rules! read_string {
            () => {
                match read_constant!() {
                    value::Value::ObjString(s) => s,
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
                let ip = self.frame()?.ip;
                debug::disassemble_instruction(&self.frame()?.closure.borrow().function.chunk, ip);
            }
            let instruction = chunk::OpCode::from(read_byte!());

            match instruction {
                chunk::OpCode::Constant => {
                    let constant = read_constant!();
                    self.push(constant);
                }

                chunk::OpCode::Nil => {
                    self.push(value::Value::None);
                }

                chunk::OpCode::True => {
                    self.push(value::Value::Boolean(true));
                }

                chunk::OpCode::False => {
                    self.push(value::Value::Boolean(false));
                }

                chunk::OpCode::Pop => {
                    self.pop()?;
                }

                chunk::OpCode::GetLocal => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame()?.slot_base;
                    let value = self.stack[slot_base + slot];
                    self.push(value);
                }

                chunk::OpCode::SetLocal => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame()?.slot_base;
                    self.stack[slot_base + slot] = *self.peek(0);
                }

                chunk::OpCode::GetGlobal => {
                    let name = read_string!();
                    let value = match self.globals.get(&name.data) {
                        Some(value) => *value,
                        None => {
                            let msg = format!("Undefined variable '{}'.", name.data);
                            self.runtime_error(msg.as_str());
                            return Err(VmError::RuntimeError);
                        }
                    };
                    self.push(value);
                }

                chunk::OpCode::DefineGlobal => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    self.globals.insert(name.data.clone(), value);
                    self.pop()?;
                }

                chunk::OpCode::SetGlobal => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    let prev = self.globals.insert(name.data.clone(), value);
                    match prev {
                        Some(_) => {}
                        None => {
                            self.globals.remove(&name.data);
                            let msg = format!("Undefined variable '{}'.", name.data);
                            self.runtime_error(msg.as_str());
                            return Err(VmError::RuntimeError);
                        }
                    }
                }

                chunk::OpCode::GetUpvalue => {
                    let upvalue_index = read_byte!() as usize;
                    let upvalue =
                        match *self.frame()?.closure.borrow().upvalues[upvalue_index].borrow() {
                            object::ObjUpvalue::Open(slot) => self.stack[slot],
                            object::ObjUpvalue::Closed(value) => value,
                        };
                    self.push(upvalue);
                }

                chunk::OpCode::SetUpvalue => {
                    let upvalue_index = read_byte!() as usize;
                    let stack_value = *self.peek(0);
                    let closure = self.frame()?.closure;
                    match *closure.borrow_mut().upvalues[upvalue_index].borrow_mut() {
                        object::ObjUpvalue::Open(slot) => {
                            self.stack[slot] = stack_value;
                        }
                        object::ObjUpvalue::Closed(ref mut value) => {
                            *value = stack_value;
                        }
                    };
                }

                chunk::OpCode::GetProperty => {
                    let instance = match *self.peek(0) {
                        value::Value::ObjInstance(ptr) => ptr,
                        _ => {
                            self.runtime_error("Only instances have properties.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    let name = read_string!();

                    let borrowed_instance = instance.borrow();
                    if let Some(property) = borrowed_instance.fields.get(&name.data) {
                        self.pop()?;
                        self.push(*property);
                    } else {
                        self.bind_method(borrowed_instance.class, name)?;
                    }
                }

                chunk::OpCode::SetProperty => {
                    let instance = match *self.peek(1) {
                        value::Value::ObjInstance(ptr) => ptr,
                        _ => {
                            self.runtime_error("Only instances have fields.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    let name = read_string!();
                    let value = *self.peek(0);
                    instance
                        .borrow_mut()
                        .fields
                        .insert(name.data.clone(), value);

                    self.pop()?;
                    self.pop()?;
                    self.push(value);
                }

                chunk::OpCode::GetSuper => {
                    let name = read_string!();
                    let superclass = match self.pop()? {
                        value::Value::ObjClass(ptr) => ptr,
                        _ => unreachable!(),
                    };

                    self.bind_method(superclass, name)?;
                }

                chunk::OpCode::Equal => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(value::Value::Boolean(a == b));
                }

                chunk::OpCode::Greater => binary_op!(value::Value::Boolean, >),

                chunk::OpCode::Less => binary_op!(value::Value::Boolean, <),

                chunk::OpCode::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (value::Value::ObjString(a), value::Value::ObjString(b)) => self
                            .stack
                            .push(value::Value::from(format!("{}{}", a.data, b.data))),

                        (value::Value::Number(a), value::Value::Number(b)) => {
                            self.push(value::Value::Number(a + b));
                        }

                        _ => {
                            self.runtime_error(
                                "Binary operands must be two numbers or two strings.",
                            );
                            return Err(VmError::RuntimeError);
                        }
                    }
                }

                chunk::OpCode::Subtract => binary_op!(value::Value::Number, -),

                chunk::OpCode::Multiply => binary_op!(value::Value::Number, *),

                chunk::OpCode::Divide => binary_op!(value::Value::Number, /),

                chunk::OpCode::Not => {
                    let value = self.pop()?;
                    self.push(value::Value::Boolean(!value.as_bool()));
                }

                chunk::OpCode::Negate => {
                    let value = self.pop()?;
                    match value {
                        value::Value::Number(underlying) => {
                            self.push(value::Value::Number(-underlying));
                        }
                        _ => {
                            self.runtime_error("Unary operand must be a number.");
                            return Err(VmError::RuntimeError);
                        }
                    }
                }

                chunk::OpCode::Print => {
                    println!("{}", self.pop()?);
                }

                chunk::OpCode::Jump => {
                    let offset = read_short!();
                    self.frame_mut()?.ip += offset as usize;
                }

                chunk::OpCode::JumpIfFalse => {
                    let offset = read_short!();
                    if !self.peek(0).as_bool() {
                        self.frame_mut()?.ip += offset as usize;
                    }
                }

                chunk::OpCode::Loop => {
                    let offset = read_short!();
                    self.frame_mut()?.ip -= offset as usize;
                }

                chunk::OpCode::Call => {
                    let arg_count = read_byte!() as usize;
                    self.call_value(*self.peek(arg_count), arg_count)?;
                }

                chunk::OpCode::Invoke => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    self.invoke(method, arg_count)?;
                }

                chunk::OpCode::SuperInvoke => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    let superclass = match self.pop()? {
                        value::Value::ObjClass(ptr) => ptr,
                        _ => unreachable!(),
                    };
                    self.invoke_from_class(superclass, method, arg_count)?;
                }

                chunk::OpCode::Closure => {
                    let function = match read_constant!() {
                        value::Value::ObjFunction(underlying) => underlying,
                        _ => panic!("Expected ObjFunction."),
                    };

                    let upvalue_count = function.upvalue_count;

                    let closure = memory::allocate(RefCell::new(object::ObjClosure::new(function)));
                    self.push(value::Value::ObjClosure(closure.as_gc()));

                    for i in 0..upvalue_count {
                        let is_local = read_byte!() != 0;
                        let index = read_byte!() as usize;
                        let slot_base = self.frame()?.slot_base;
                        closure.borrow_mut().upvalues[i] = if is_local {
                            self.capture_upvalue(slot_base + index)
                        } else {
                            self.frame()?.closure.borrow().upvalues[index]
                        };
                    }
                }

                chunk::OpCode::CloseUpvalue => {
                    self.close_upvalues(self.stack.len() - 1, *self.peek(0));
                    self.pop()?;
                }

                chunk::OpCode::Return => {
                    let result = self.pop()?;
                    for i in self.frame()?.slot_base..self.stack.len() {
                        self.close_upvalues(i, self.stack[i])
                    }

                    let prev_stack_size = self.frame()?.slot_base;
                    self.frames.pop();
                    if self.frames.is_empty() {
                        self.pop()?;
                        return Ok(());
                    }

                    self.stack.truncate(prev_stack_size);
                    self.push(result);
                }

                chunk::OpCode::Class => {
                    let class =
                        memory::allocate(RefCell::new(object::ObjClass::new(read_string!())));
                    self.push(value::Value::ObjClass(class.as_gc()));
                }

                chunk::OpCode::Inherit => {
                    let superclass_pos = self.stack.len() - 2;
                    let superclass = match self.stack[superclass_pos] {
                        value::Value::ObjClass(ptr) => ptr,
                        _ => {
                            self.runtime_error("Superclass must be a class.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    let subclass = match self.peek(0) {
                        value::Value::ObjClass(ptr) => *ptr,
                        _ => unreachable!(),
                    };
                    for (name, value) in superclass.borrow().methods.iter() {
                        subclass.borrow_mut().methods.insert(name.clone(), *value);
                    }
                    self.pop()?;
                }

                chunk::OpCode::Method => {
                    let name = read_string!();
                    self.define_method(name)?;
                }
            }
        }
    }

    fn call_value(&mut self, value: value::Value, arg_count: usize) -> Result<(), VmError> {
        match value {
            value::Value::ObjBoundMethod(bound) => {
                *self.peek_mut(arg_count) = bound.borrow().receiver;
                self.call(bound.borrow().method, arg_count)
            }

            value::Value::ObjClass(class) => {
                let instance = memory::allocate(RefCell::new(object::ObjInstance::new(class)));
                *self.peek_mut(arg_count) = value::Value::ObjInstance(instance.as_gc());

                if let Some(value::Value::ObjClosure(initialiser)) =
                    class.borrow().methods.get(&self.init_string.data)
                {
                    return self.call(*initialiser, arg_count);
                } else if arg_count != 0 {
                    let msg = format!("Expected 0 arguments but got {}.", arg_count);
                    self.runtime_error(msg.as_str());
                    return Err(VmError::TypeError);
                }

                Ok(())
            }

            value::Value::ObjClosure(function) => {
                self.call(function, arg_count)
            }

            value::Value::ObjNative(wrapped) => {
                let function = wrapped.function.ok_or(VmError::ValueError)?;
                let frame_begin = self.stack.len() - arg_count - 1;
                let result = function(arg_count, &mut self.stack[frame_begin..]);
                self.stack.truncate(frame_begin);
                self.push(result);
                Ok(())
            }

            _ => {
                self.runtime_error("Can only call functions and classes.");
                Err(VmError::TypeError)
            }
        }
    }

    fn invoke_from_class(
        &mut self,
        class: memory::Gc<RefCell<object::ObjClass>>,
        name: memory::Gc<object::ObjString>,
        arg_count: usize,
    ) -> Result<(), VmError> {
        if let Some(value) = class.borrow().methods.get(&name.data) {
            return match value {
                value::Value::ObjClosure(closure) => self.call(*closure, arg_count),
                _ => unreachable!(),
            };
        }
        let msg = format!("Undefined property '{}'.", name.data);
        self.runtime_error(msg.as_str());
        Err(VmError::AttributeError)
    }

    fn invoke(
        &mut self,
        name: memory::Gc<object::ObjString>,
        arg_count: usize,
    ) -> Result<(), VmError> {
        let receiver = *self.peek(arg_count);
        match receiver {
            value::Value::ObjInstance(instance) => {
                if let Some(value) = instance.borrow().fields.get(&name.data) {
                    *self.peek_mut(arg_count) = *value;
                    return self.call_value(*value, arg_count);
                }

                self.invoke_from_class(instance.borrow().class, name, arg_count)
            }
            _ => {
                self.runtime_error("Only instances have methods.");
                Err(VmError::ValueError)
            }
        }
    }

    fn call(
        &mut self,
        closure: memory::Gc<RefCell<object::ObjClosure>>,
        arg_count: usize,
    ) -> Result<(), VmError> {
        if arg_count as u32 != closure.borrow().function.arity {
            let msg = format!(
                "Expected {} arguments but got {}.",
                closure.borrow().function.arity,
                arg_count
            );
            self.runtime_error(msg.as_str());
            return Err(VmError::TypeError);
        }

        if self.frames.len() == FRAMES_MAX {
            self.runtime_error("Stack overflow.");
            return Err(VmError::IndexError);
        }

        self.frames.push(CallFrame {
            closure,
            ip: 0,
            slot_base: self.stack.len() - arg_count - 1,
        });
        Ok(())
    }

    fn reset_stack(&mut self) {
        self.stack.clear();
        self.frames.clear();
    }

    fn runtime_error(&mut self, message: &str) {
        eprintln!("{}", message);

        for frame in self.frames.iter().rev() {
            let function = frame.closure.borrow().function;

            let instruction = frame.ip - 1;
            eprint!("[line {}] in ", function.chunk.lines[instruction]);
            if function.name.data.is_empty() {
                eprintln!("script");
            } else {
                eprintln!("{}()", function.name.data);
            }
        }

        self.reset_stack();
    }

    fn define_native(&mut self, name: &str, function: object::NativeFn) {
        self.push(value::Value::from(function));
        let value = *self.peek(0);
        self.globals.insert(String::from(name), value);
        self.pop().unwrap_or(value::Value::None);
    }

    fn define_method(&mut self, name: memory::Gc<object::ObjString>) -> Result<(), VmError> {
        let method = *self.peek(0);
        let class = match *self.peek(1) {
            value::Value::ObjClass(ptr) => ptr,
            _ => unreachable!(),
        };
        class.borrow_mut().methods.insert(name.data.clone(), method);
        self.pop().unwrap_or(value::Value::None);

        Ok(())
    }

    fn bind_method(
        &mut self,
        class: memory::Gc<RefCell<object::ObjClass>>,
        name: memory::Gc<object::ObjString>,
    ) -> Result<(), VmError> {
        let borrowed_class = class.borrow();
        let method = match borrowed_class.methods.get(&name.data) {
            Some(value::Value::ObjClosure(ptr)) => *ptr,
            None => {
                let msg = format!("Undefined property '{}'.", name.data);
                self.runtime_error(msg.as_str());
                return Err(VmError::AttributeError);
            }
            _ => unreachable!(),
        };

        let instance = *self.peek(0);
        let bound = memory::allocate(RefCell::new(object::ObjBoundMethod::new(instance, method)));
        self.pop()?;
        self.push(value::Value::ObjBoundMethod(bound.as_gc()));

        Ok(())
    }

    fn capture_upvalue(&mut self, location: usize) -> memory::Gc<RefCell<object::ObjUpvalue>> {
        let result = self
            .open_upvalues
            .iter()
            .find(|&u| u.borrow().is_open_with_index(location));

        let upvalue = if let Some(upvalue) = result {
            *upvalue
        } else {
            memory::allocate(RefCell::new(object::ObjUpvalue::new(location))).as_gc()
        };

        self.open_upvalues.push(upvalue);
        upvalue
    }

    fn close_upvalues(&mut self, last: usize, value: value::Value) {
        for upvalue in self.open_upvalues.iter() {
            if upvalue.borrow().is_open_with_index(last) {
                upvalue.borrow_mut().close(value);
            }
        }

        self.open_upvalues.retain(|u| u.borrow().is_open());
    }

    fn frame(&self) -> Result<&CallFrame, VmError> {
        self.frames.last().ok_or(VmError::IndexError)
    }

    fn frame_mut(&mut self) -> Result<&mut CallFrame, VmError> {
        self.frames.last_mut().ok_or(VmError::IndexError)
    }

    fn peek(&self, depth: usize) -> &value::Value {
        let stack_len = self.stack.len();
        &self.stack[stack_len - depth - 1]
    }

    fn peek_mut(&mut self, depth: usize) -> &mut value::Value {
        let stack_len = self.stack.len();
        &mut self.stack[stack_len - depth - 1]
    }

    fn push(&mut self, value: value::Value) {
        self.stack.push(value);
    }

    fn pop(&mut self) -> Result<value::Value, VmError> {
        self.stack.pop().ok_or(VmError::IndexError)
    }
}
