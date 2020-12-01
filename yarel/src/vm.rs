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
use std::fmt::Write;
use std::ptr;
use std::rc::Rc;
use std::time;

use crate::chunk::{Chunk, ChunkStore, OpCode};
use crate::class_store::{self, CoreClassStore};
use crate::common;
use crate::compiler;
use crate::debug;
use crate::error::{Error, ErrorKind};
use crate::hash::BuildPassThroughHasher;
use crate::memory::{Gc, GcManaged, Heap, Root, UniqueRoot};
use crate::object::{
    self, NativeFn, ObjClass, ObjClosure, ObjFunction, ObjNative, ObjString, ObjUpvalue,
};
use crate::value::Value;

const FRAMES_MAX: usize = 64;
const STACK_MAX: usize = common::LOCALS_MAX * FRAMES_MAX;

pub fn interpret(vm: &mut Vm, source: String) -> Result<Value, Error> {
    let compile_result = compiler::compile(
        &mut vm.heap.borrow_mut(),
        &mut vm.chunk_store.borrow_mut(),
        source,
    );
    match compile_result {
        Ok(function) => vm.execute(function, &[]),
        Err(error) => Err(error),
    }
}

pub struct CallFrame {
    closure: Gc<RefCell<ObjClosure>>,
    prev_ip: *const u8,
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
    ip: *const u8,
    active_chunk: Gc<Chunk>,
    frames: Vec<CallFrame>,
    stack: Vec<Value>,
    globals: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
    open_upvalues: Vec<Gc<RefCell<ObjUpvalue>>>,
    init_string: Gc<ObjString>,
    next_string: Gc<ObjString>,
    class_store: Box<CoreClassStore>,
    chunk_store: Rc<RefCell<ChunkStore>>,
    heap: Rc<RefCell<Heap>>,
}

fn clock_native(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _args: &mut [Value],
) -> Result<Value, Error> {
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

fn default_print(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 2 {
        return error!(ErrorKind::RuntimeError, "Expected one argument to 'print'.");
    }
    println!("{}", args[1]);
    Ok(Value::None)
}

fn string(
    heap: &mut Heap,
    _class_store: &CoreClassStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 2 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected one argument to 'String'."
        );
    }
    Ok(Value::ObjString(object::new_gc_obj_string(
        heap,
        format!("{}", args[1]).as_str(),
    )))
}

fn sentinel(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected no arguments to 'sentinel'."
        );
    }
    Ok(Value::Sentinel)
}

pub fn new_root_vm(
    heap: Rc<RefCell<Heap>>,
    chunk_store: Rc<RefCell<ChunkStore>>,
) -> UniqueRoot<Vm> {
    let class_store = class_store::new_empty_class_store(&mut heap.borrow_mut());
    let vm = Vm::new(heap.clone(), chunk_store, class_store);
    heap.borrow_mut().allocate_unique_root(vm)
}

pub fn new_root_vm_with_built_ins(
    heap: Rc<RefCell<Heap>>,
    chunk_store: Rc<RefCell<ChunkStore>>,
    class_store: Box<CoreClassStore>,
) -> UniqueRoot<Vm> {
    let vm = Vm::new(heap.clone(), chunk_store, class_store);
    let mut vm = heap.borrow_mut().allocate_unique_root(vm);
    vm.define_native("clock", clock_native);
    vm.define_native("print", default_print);
    vm.define_native("String", string);
    vm.define_native("sentinel", sentinel);
    let obj_iter_class = vm.class_store.get_obj_iter_class();
    vm.set_global("Iter", Value::ObjClass(obj_iter_class));
    let obj_map_iter_class = vm.class_store.get_obj_map_iter_class();
    vm.set_global("MapIter", Value::ObjClass(obj_map_iter_class));
    let obj_vec_class = vm.class_store.get_obj_vec_class();
    vm.set_global("Vec", Value::ObjClass(obj_vec_class));
    let obj_range_class = vm.class_store.get_obj_range_class();
    vm.set_global("Range", Value::ObjClass(obj_range_class));
    vm
}

impl Vm {
    fn new(
        heap: Rc<RefCell<Heap>>,
        chunk_store: Rc<RefCell<ChunkStore>>,
        class_store: Box<CoreClassStore>,
    ) -> Self {
        let empty_chunk = heap.borrow_mut().allocate(Chunk::new());
        let init_string = object::new_gc_obj_string(&mut heap.borrow_mut(), "__init__");
        let next_string = object::new_gc_obj_string(&mut heap.borrow_mut(), "__next__");
        Vm {
            ip: ptr::null(),
            active_chunk: empty_chunk,
            frames: Vec::with_capacity(FRAMES_MAX),
            stack: Vec::with_capacity(STACK_MAX),
            globals: HashMap::with_hasher(BuildPassThroughHasher::default()),
            open_upvalues: Vec::new(),
            init_string,
            next_string,
            class_store,
            chunk_store,
            heap,
        }
    }

    pub fn execute(&mut self, function: Root<ObjFunction>, args: &[Value]) -> Result<Value, Error> {
        let closure = object::new_gc_obj_closure(&mut self.heap.borrow_mut(), function.as_gc());
        self.push(Value::ObjClosure(closure));
        self.stack.extend_from_slice(args);
        self.call_value(Value::ObjClosure(closure), args.len())?;
        match self.run() {
            Ok(value) => Ok(value),
            Err(mut error) => Err(self.runtime_error(&mut error)),
        }
    }

    pub fn get_global(&self, name: &str) -> Option<Value> {
        let name = object::new_gc_obj_string(&mut self.heap.borrow_mut(), name);
        self.globals.get(&name).copied()
    }

    pub fn set_global(&mut self, name: &str, value: Value) {
        let name = object::new_gc_obj_string(&mut self.heap.borrow_mut(), name);
        self.globals.insert(name, value);
    }

    pub fn define_native(&mut self, name: &str, function: NativeFn) {
        let native = object::new_root_obj_native(&mut self.heap.borrow_mut(), function);
        let name = object::new_gc_obj_string(&mut self.heap.borrow_mut(), name);
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
                unsafe {
                    let ret = *self.ip;
                    self.ip = self.ip.offset(1);
                    ret
                }
            }};
        }

        macro_rules! read_short {
            () => {{
                unsafe {
                    let ret = u16::from_ne_bytes([*self.ip, *self.ip.offset(1)]);
                    self.ip = self.ip.offset(2);
                    ret
                }
            }};
        }

        macro_rules! read_constant {
            () => {{
                let index = read_byte!() as usize;
                self.active_chunk.constants[index]
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
                let offset = self.active_chunk.code_offset(self.ip);
                debug::disassemble_instruction(&self.active_chunk, offset);
            }
            let byte = read_byte!();

            match byte {
                byte if byte == OpCode::Constant as u8 => {
                    let constant = read_constant!();
                    self.push(constant);
                }

                byte if byte == OpCode::Nil as u8 => {
                    self.push(Value::None);
                }

                byte if byte == OpCode::True as u8 => {
                    self.push(Value::Boolean(true));
                }

                byte if byte == OpCode::False as u8 => {
                    self.push(Value::Boolean(false));
                }

                byte if byte == OpCode::Pop as u8 => {
                    self.pop();
                }

                byte if byte == OpCode::CopyTop as u8 => {
                    let top = *self.peek(0);
                    self.push(top);
                }

                byte if byte == OpCode::GetLocal as u8 => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame().slot_base;
                    let value = self.stack[slot_base + slot];
                    self.push(value);
                }

                byte if byte == OpCode::SetLocal as u8 => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame().slot_base;
                    self.stack[slot_base + slot] = *self.peek(0);
                }

                byte if byte == OpCode::GetGlobal as u8 => {
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

                byte if byte == OpCode::DefineGlobal as u8 => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    self.globals.insert(name, value);
                    self.pop();
                }

                byte if byte == OpCode::SetGlobal as u8 => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    let prev = self.globals.insert(name, value);
                    if prev.is_none() {
                        self.globals.remove(&name);
                        return error!(ErrorKind::RuntimeError, "Undefined variable '{}'.", *name);
                    }
                }

                byte if byte == OpCode::GetUpvalue as u8 => {
                    let upvalue_index = read_byte!() as usize;
                    let upvalue =
                        match *self.frame().closure.borrow().upvalues[upvalue_index].borrow() {
                            ObjUpvalue::Open(slot) => self.stack[slot],
                            ObjUpvalue::Closed(value) => value,
                        };
                    self.push(upvalue);
                }

                byte if byte == OpCode::SetUpvalue as u8 => {
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

                byte if byte == OpCode::GetProperty as u8 => {
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

                byte if byte == OpCode::SetProperty as u8 => {
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

                byte if byte == OpCode::GetSuper as u8 => {
                    let name = read_string!();
                    let superclass = self.pop().try_as_obj_class().expect("Expected ObjClass.");

                    self.bind_method(superclass, name)?;
                }

                byte if byte == OpCode::Equal as u8 => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::Boolean(a == b));
                }

                byte if byte == OpCode::Greater as u8 => binary_op!(Value::Boolean, >),

                byte if byte == OpCode::Less as u8 => binary_op!(Value::Boolean, <),

                byte if byte == OpCode::Add as u8 => {
                    let b = self.pop();
                    let a = self.pop();
                    match (a, b) {
                        (Value::ObjString(a), Value::ObjString(b)) => {
                            let value = Value::ObjString(object::new_gc_obj_string(
                                &mut self.heap.borrow_mut(),
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

                byte if byte == OpCode::Subtract as u8 => binary_op!(Value::Number, -),

                byte if byte == OpCode::Multiply as u8 => binary_op!(Value::Number, *),

                byte if byte == OpCode::Divide as u8 => binary_op!(Value::Number, /),

                byte if byte == OpCode::Not as u8 => {
                    let value = self.pop();
                    self.push(Value::Boolean(!value.as_bool()));
                }

                byte if byte == OpCode::Negate as u8 => {
                    let value = self.pop();
                    if let Some(num) = value.try_as_number() {
                        self.push(Value::Number(-num));
                    } else {
                        return error!(ErrorKind::RuntimeError, "Unary operand must be a number.",);
                    }
                }

                byte if byte == OpCode::FormatString as u8 => {
                    let value = self.peek(0);
                    if value.try_as_obj_string().is_some() {
                        continue;
                    }
                    let obj = Value::ObjString(object::new_gc_obj_string(
                        &mut self.heap.borrow_mut(),
                        format!("{}", value).as_str(),
                    ));
                    *self.peek_mut(0) = obj;
                }

                byte if byte == OpCode::BuildRange as u8 => {
                    let end = object::validate_integer(self.pop())?;
                    let begin = object::validate_integer(self.pop())?;
                    let range = object::new_root_obj_range(
                        &mut self.heap.borrow_mut(),
                        self.class_store.get_obj_range_class(),
                        begin,
                        end,
                    );
                    self.push(Value::ObjRange(range.as_gc()));
                }

                byte if byte == OpCode::BuildString as u8 => {
                    let num_operands = read_byte!() as usize;
                    if num_operands == 1 {
                        continue;
                    }
                    let mut new_string = String::new();
                    for pos in (0..num_operands).rev() {
                        new_string.push_str(self.peek(pos).try_as_obj_string().unwrap().as_str())
                    }
                    let new_stack_size = self.stack.len() - num_operands;
                    self.stack.truncate(new_stack_size);
                    let value = Value::ObjString(object::new_gc_obj_string(
                        &mut self.heap.borrow_mut(),
                        new_string.as_str(),
                    ));
                    self.push(value);
                }

                byte if byte == OpCode::BuildVec as u8 => {
                    let num_operands = read_byte!() as usize;
                    let vec = object::new_root_obj_vec(
                        &mut self.heap.borrow_mut(),
                        self.class_store.get_obj_vec_class(),
                    );
                    let begin = self.stack.len() - num_operands;
                    let end = self.stack.len();
                    vec.borrow_mut().elements = self.stack[begin..end].iter().copied().collect();
                    self.stack.truncate(begin);
                    self.push(Value::ObjVec(vec.as_gc()));
                }

                byte if byte == OpCode::IterNext as u8 => {
                    let iter = *self.peek(0);
                    self.push(iter);
                    self.invoke(self.next_string, 0)?;
                }

                byte if byte == OpCode::Jump as u8 => {
                    let offset = read_short!();
                    self.ip = unsafe { self.ip.offset(offset as isize) };
                }

                byte if byte == OpCode::JumpIfFalse as u8 => {
                    let offset = read_short!();
                    if !self.peek(0).as_bool() {
                        self.ip = unsafe { self.ip.offset(offset as isize) };
                    }
                }

                byte if byte == OpCode::JumpIfSentinel as u8 => {
                    let offset = read_short!();
                    if let Value::Sentinel = self.peek(0) {
                        self.ip = unsafe { self.ip.offset(offset as isize) };
                    }
                }

                byte if byte == OpCode::Loop as u8 => {
                    let offset = read_short!();
                    self.ip = unsafe { self.ip.offset(-(offset as isize)) };
                }

                byte if byte == OpCode::Call as u8 => {
                    let arg_count = read_byte!() as usize;
                    self.call_value(*self.peek(arg_count), arg_count)?;
                }

                byte if byte == OpCode::Invoke as u8 => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    self.invoke(method, arg_count)?;
                }

                byte if byte == OpCode::SuperInvoke as u8 => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    let superclass = match self.pop() {
                        Value::ObjClass(ptr) => ptr,
                        _ => unreachable!(),
                    };
                    self.invoke_from_class(superclass, method, arg_count)?;
                }

                byte if byte == OpCode::Closure as u8 => {
                    let function = match read_constant!() {
                        Value::ObjFunction(underlying) => underlying,
                        _ => panic!("Expected ObjFunction."),
                    };

                    let upvalue_count = function.upvalue_count;

                    let closure = object::new_gc_obj_closure(&mut self.heap.borrow_mut(), function);
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

                byte if byte == OpCode::CloseUpvalue as u8 => {
                    self.close_upvalues(self.stack.len() - 1, *self.peek(0));
                    self.pop();
                }

                byte if byte == OpCode::Return as u8 => {
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
                    self.active_chunk = self.chunk_store.borrow().get_chunk(prev_chunk_index);
                    self.ip = prev_ip;

                    self.stack.truncate(prev_stack_size);
                    self.push(result);
                }

                byte if byte == OpCode::Class as u8 => {
                    let string = read_string!();
                    let class = object::new_gc_obj_class(&mut self.heap.borrow_mut(), string);
                    self.push(Value::ObjClass(class));
                }

                byte if byte == OpCode::Inherit as u8 => {
                    let superclass = if let Some(ptr) = self.peek(1).try_as_obj_class() {
                        ptr
                    } else {
                        return error!(ErrorKind::RuntimeError, "Superclass must be a class.");
                    };
                    let subclass = self.peek(0).try_as_obj_class().expect("Expected ObjClass.");
                    subclass.borrow_mut().add_superclass(superclass);
                    self.pop();
                }

                byte if byte == OpCode::Method as u8 => {
                    let name = read_string!();
                    self.define_method(name)?;
                }

                _ => {
                    panic!("Unknown opcode {}", byte);
                }
            }
        }
    }

    fn call_value(&mut self, value: Value, arg_count: usize) -> Result<(), Error> {
        match value {
            Value::ObjBoundMethod(bound) => {
                *self.peek_mut(arg_count) = bound.borrow().receiver;
                self.call_closure(bound.borrow().method, arg_count)
            }

            Value::ObjBoundNative(bound) => {
                *self.peek_mut(arg_count) = bound.borrow().receiver;
                self.call_native(bound.borrow().method, arg_count)
            }

            Value::ObjClass(class) => {
                let instance = object::new_gc_obj_instance(&mut self.heap.borrow_mut(), class);
                *self.peek_mut(arg_count) = Value::ObjInstance(instance);

                let borrowed_class = class.borrow();
                let init = borrowed_class.methods.get(&self.init_string);
                if let Some(Value::ObjClosure(initialiser)) = init {
                    return self.call_closure(*initialiser, arg_count);
                } else if let Some(Value::ObjNative(initialiser)) = init {
                    return self.call_native(*initialiser, arg_count);
                } else if arg_count != 0 {
                    return error!(
                        ErrorKind::TypeError,
                        "Expected 0 arguments but got {}.", arg_count
                    );
                }

                Ok(())
            }

            Value::ObjClosure(function) => self.call_closure(function, arg_count),

            Value::ObjNative(wrapped) => self.call_native(wrapped, arg_count),

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
                Value::ObjClosure(closure) => self.call_closure(*closure, arg_count),
                Value::ObjNative(native) => self.call_native(*native, arg_count),
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
                let class = vec.borrow().class;
                self.invoke_from_class(class, name, arg_count)
            }
            Value::ObjVecIter(iter) => {
                let class = iter.borrow().class;
                self.invoke_from_class(class, name, arg_count)
            }
            Value::ObjRange(range) => {
                let class = range.class;
                self.invoke_from_class(class, name, arg_count)
            }
            Value::ObjRangeIter(iter) => {
                let class = iter.borrow().class;
                self.invoke_from_class(class, name, arg_count)
            }
            _ => error!(ErrorKind::ValueError, "Only instances have methods."),
        }
    }

    fn call_closure(
        &mut self,
        closure: Gc<RefCell<ObjClosure>>,
        arg_count: usize,
    ) -> Result<(), Error> {
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

        let chunk_index = closure.borrow().function.chunk_index;
        self.active_chunk = self.chunk_store.borrow().get_chunk(chunk_index);
        self.frames.push(CallFrame {
            closure,
            prev_ip: self.ip,
            slot_base: self.stack.len() - arg_count - 1,
        });
        self.ip = &self.active_chunk.code[0];
        Ok(())
    }

    fn call_native(&mut self, native: Gc<ObjNative>, arg_count: usize) -> Result<(), Error> {
        let function = native.function;
        let frame_begin = self.stack.len() - arg_count - 1;
        let result = function(
            &mut self.heap.borrow_mut(),
            &self.class_store,
            &mut self.stack[frame_begin..frame_begin + arg_count + 1],
        )?;
        self.stack.truncate(frame_begin);
        self.push(result);
        Ok(())
    }

    fn reset_stack(&mut self) {
        self.stack.clear();
        self.frames.clear();
    }

    fn runtime_error(&mut self, error: &mut Error) -> Error {
        let mut ips: Vec<*const u8> = self.frames.iter().skip(1).map(|f| f.prev_ip).collect();
        ips.push(self.ip);

        for (i, frame) in self.frames.iter().enumerate().rev() {
            let function = frame.closure.borrow().function;

            let mut new_msg = String::new();
            let chunk = self
                .chunk_store
                .borrow()
                .get_chunk(frame.closure.borrow().function.chunk_index);
            let instruction = chunk.code_offset(ips[i]) - 1;
            write!(new_msg, "[line {}] in ", chunk.lines[instruction])
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
        let bound =
            match borrowed_class.methods.get(&name) {
                Some(Value::ObjClosure(ptr)) => Value::ObjBoundMethod(
                    object::new_gc_obj_bound_method(&mut self.heap.borrow_mut(), instance, *ptr),
                ),
                Some(Value::ObjNative(ptr)) => Value::ObjBoundNative(
                    object::new_gc_obj_bound_method(&mut self.heap.borrow_mut(), instance, *ptr),
                ),
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
            object::new_gc_obj_upvalue(&mut self.heap.borrow_mut(), location)
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
        self.stack.mark();
        self.globals.mark();
        self.frames.mark();
        self.open_upvalues.mark();
    }

    fn blacken(&self) {
        self.stack.blacken();
        self.globals.blacken();
        self.frames.blacken();
        self.open_upvalues.blacken();
    }
}
