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
use std::hash::{Hash, Hasher};
use std::ptr;
use std::slice;
use std::time;

use crate::chunk::{Chunk, OpCode};
use crate::class_store::{self, CoreClassStore};
use crate::common;
use crate::compiler;
use crate::core;
use crate::debug;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, FnvHasher};
use crate::memory::{self, Gc, GcBoxPtr, GcManaged, Heap, Root};
use crate::object::{
    self, NativeFn, ObjClass, ObjClosure, ObjFunction, ObjNative, ObjRange, ObjString,
    ObjStringValueMap, ObjUpvalue,
};
use crate::stack::Stack;
use crate::utils;
use crate::value::Value;

const RANGE_CACHE_SIZE: usize = 8;

pub fn interpret(vm: &mut Vm, source: String) -> Result<Value, Error> {
    let compile_result = compiler::compile(vm, source);
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

struct ClassDef {
    name: Gc<ObjString>,
    metaclass_name: Gc<ObjString>,
    superclass: Gc<ObjClass>,
    methods: ObjStringValueMap,
    static_methods: ObjStringValueMap,
}

impl ClassDef {
    fn new(name: Gc<ObjString>, metaclass_name: Gc<ObjString>, superclass: Gc<ObjClass>) -> Self {
        ClassDef {
            name,
            metaclass_name,
            superclass,
            methods: object::new_obj_string_value_map(),
            static_methods: object::new_obj_string_value_map(),
        }
    }
}

impl GcManaged for ClassDef {
    fn mark(&self) {
        self.methods.mark();
        self.static_methods.mark();
    }

    fn blacken(&self) {
        self.methods.blacken();
        self.static_methods.blacken();
    }
}

pub struct Vm {
    ip: *const u8,
    active_chunk: Gc<Chunk>,
    frames: Vec<CallFrame>,
    stack: Stack<Value>,
    globals: ObjStringValueMap,
    open_upvalues: Vec<Gc<RefCell<ObjUpvalue>>>,
    init_string: Gc<ObjString>,
    next_string: Gc<ObjString>,
    pub(crate) class_store: CoreClassStore,
    chunks: Vec<Root<Chunk>>,
    core_chunks: Vec<Root<Chunk>>,
    string_class: Root<ObjClass>,
    string_store: HashMap<u64, Root<ObjString>, BuildPassThroughHasher>,
    range_cache: Vec<(Root<ObjRange>, time::Instant)>,
    working_class_def: Option<ClassDef>,
    pub(crate) heap: Heap,
}

impl Vm {
    pub fn new() -> Self {
        let heap = memory::Heap::new();
        // # Safety
        // We create some dangling GC pointers here. This is safe because the fields are
        // re-assigned immediately to valid GC pointers by init_heap_allocated_data.
        let mut vm = Vm {
            ip: ptr::null(),
            active_chunk: unsafe { Gc::dangling() },
            frames: Vec::with_capacity(common::FRAMES_MAX),
            stack: Stack::new(),
            globals: object::new_obj_string_value_map(),
            open_upvalues: Vec::new(),
            init_string: unsafe { Gc::dangling() },
            next_string: unsafe { Gc::dangling() },
            class_store: unsafe { CoreClassStore::new_empty() },
            chunks: Vec::new(),
            core_chunks: Vec::new(),
            string_class: unsafe { Root::dangling() },
            string_store: HashMap::with_hasher(BuildPassThroughHasher::default()),
            heap,
            range_cache: Vec::with_capacity(RANGE_CACHE_SIZE),
            working_class_def: None,
        };
        vm.init_heap_allocated_data();
        vm
    }

    pub fn with_built_ins() -> Self {
        let mut vm = Self::new();
        vm.init_built_in_globals();
        vm
    }

    pub fn execute(&mut self, function: Root<ObjFunction>, args: &[Value]) -> Result<Value, Error> {
        let closure = object::new_gc_obj_closure(self, function.as_gc());
        self.push(Value::ObjClosure(closure));
        self.stack.extend_from_slice(args);
        self.call_value(Value::ObjClosure(closure), args.len())?;
        match self.run() {
            Ok(value) => Ok(value),
            Err(mut error) => Err(self.runtime_error(&mut error)),
        }
    }

    pub fn get_global(&mut self, name: &str) -> Option<Value> {
        let name = self.new_gc_obj_string(name);
        self.globals.get(&name).copied()
    }

    pub fn set_global(&mut self, name: &str, value: Value) {
        let name = self.new_gc_obj_string(name);
        self.globals.insert(name, value);
    }

    pub fn define_native(&mut self, name: &str, function: NativeFn) {
        let native = object::new_root_obj_native(self, function);
        let name = self.new_gc_obj_string(name);
        self.globals.insert(name, Value::ObjNative(native.as_gc()));
    }

    pub fn new_gc_obj_string(&mut self, data: &str) -> Gc<ObjString> {
        let hash = {
            let mut hasher = FnvHasher::new();
            (*data).hash(&mut hasher);
            hasher.finish()
        };
        if let Some(gc_string) = self.string_store.get(&hash).map(|v| v.as_gc()) {
            return gc_string;
        }
        let ret = self.allocate(ObjString::new(self.string_class.as_gc(), data, hash));
        self.string_store.insert(hash, ret.as_root());
        ret
    }

    pub fn reset(&mut self) {
        self.reset_stack();
        self.chunks = self.core_chunks.clone();
        self.globals = object::new_obj_string_value_map();
        self.init_built_in_globals();
    }

    pub(crate) fn add_chunk(&mut self, chunk: Chunk) -> usize {
        let root = self.allocate_root(chunk);
        self.chunks.push(root);
        self.chunks.len() - 1
    }

    pub(crate) fn get_chunk(&self, index: usize) -> Gc<Chunk> {
        self.chunks[index].as_gc()
    }

    pub(crate) fn allocate_bare<T: 'static + GcManaged>(&mut self, data: T) -> GcBoxPtr<T> {
        let mut roots: Vec<&dyn GcManaged> = vec![
            &self.stack,
            &self.globals,
            &self.frames,
            &self.open_upvalues,
        ];
        if let Some(def) = self.working_class_def.as_ref() {
            roots.push(def);
        }
        self.heap.allocate_bare(&roots, data)
    }

    pub(crate) fn allocate<T: 'static + GcManaged>(&mut self, data: T) -> Gc<T> {
        self.allocate_root(data).as_gc()
    }

    pub(crate) fn allocate_root<T: 'static + GcManaged>(&mut self, data: T) -> Root<T> {
        let mut roots: Vec<&dyn GcManaged> = vec![
            &self.stack,
            &self.globals,
            &self.frames,
            &self.open_upvalues,
        ];
        if let Some(def) = self.working_class_def.as_ref() {
            roots.push(def);
        }
        self.heap.allocate_root(&roots, data)
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
                            return Err(error!(
                                ErrorKind::RuntimeError, "Binary operands must both be numbers."
                            ));
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
                let index = read_short!() as usize;
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
                println!("{}", self.stack);
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
                            return Err(error!(
                                ErrorKind::RuntimeError,
                                "Undefined variable '{}'.", *name
                            ));
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
                        return Err(error!(
                            ErrorKind::RuntimeError,
                            "Undefined variable '{}'.", *name
                        ));
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
                    let name = read_string!();

                    if let Some(instance) = self.peek(0).try_as_obj_instance() {
                        let borrowed_instance = instance.borrow();
                        if let Some(&property) = borrowed_instance.fields.get(&name) {
                            self.pop();
                            self.push(property);
                            continue;
                        }
                    }

                    let class = self.peek(0).get_class(&self.class_store);
                    self.bind_method(class, name)?;
                }

                byte if byte == OpCode::SetProperty as u8 => {
                    let instance = if let Some(ptr) = self.peek(1).try_as_obj_instance() {
                        ptr
                    } else {
                        return Err(error!(
                            ErrorKind::RuntimeError,
                            "Only instances have fields."
                        ));
                    };
                    let name = read_string!();
                    let value = *self.peek(0);
                    instance.borrow_mut().fields.insert(name, value);

                    self.pop();
                    self.pop();
                    self.push(value);
                }

                byte if byte == OpCode::GetClass as u8 => {
                    let value = *self.peek(0);
                    match value {
                        Value::ObjClass(_) => continue,
                        Value::ObjInstance(instance) => {
                            *self.peek_mut(0) = Value::ObjClass(instance.borrow().class);
                        }
                        _ => {
                            let class = value.get_class(&self.class_store);
                            *self.peek_mut(0) = Value::ObjClass(class);
                        }
                    }
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
                            let value = Value::ObjString(
                                self.new_gc_obj_string(format!("{}{}", *a, *b).as_str()),
                            );
                            self.stack.push(value)
                        }

                        (Value::Number(a), Value::Number(b)) => {
                            self.push(Value::Number(a + b));
                        }

                        _ => {
                            return Err(error!(
                                ErrorKind::RuntimeError,
                                "Binary operands must be two numbers or two strings.",
                            ));
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
                        return Err(error!(
                            ErrorKind::RuntimeError,
                            "Unary operand must be a number."
                        ));
                    }
                }

                byte if byte == OpCode::FormatString as u8 => {
                    let value = *self.peek(0);
                    if value.try_as_obj_string().is_some() {
                        continue;
                    }
                    let obj =
                        Value::ObjString(self.new_gc_obj_string(format!("{}", value).as_str()));
                    *self.peek_mut(0) = obj;
                }

                byte if byte == OpCode::BuildRange as u8 => {
                    let end = utils::validate_integer(self.pop())?;
                    let begin = utils::validate_integer(self.pop())?;
                    let range = self.build_range(begin, end);
                    self.push(Value::ObjRange(range));
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
                    let value = Value::ObjString(self.new_gc_obj_string(new_string.as_str()));
                    self.push(value);
                }

                byte if byte == OpCode::BuildVec as u8 => {
                    let num_operands = read_byte!() as usize;
                    let vec = object::new_root_obj_vec(self, self.class_store.get_obj_vec_class());
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
                    self.active_chunk = self.get_chunk(prev_chunk_index);
                    self.ip = prev_ip;

                    self.stack.truncate(prev_stack_size);
                    self.push(result);
                }

                byte if byte == OpCode::DeclareClass as u8 => {
                    let name = read_string!();
                    let metaclass_name = self.new_gc_obj_string(format!("{}Class", *name).as_str());
                    let superclass = self.class_store.get_object_class();
                    self.working_class_def = Some(ClassDef::new(name, metaclass_name, superclass));
                    let class = object::new_gc_obj_class(
                        self,
                        name,
                        self.class_store.get_base_metaclass(),
                        None,
                        object::new_obj_string_value_map(),
                    );
                    self.push(Value::ObjClass(class));
                }

                byte if byte == OpCode::DefineClass as u8 => {
                    let class_def = self.working_class_def.as_ref().expect("Expected ClassDef.");

                    let base_metaclass = self.class_store.get_base_metaclass();
                    let name = class_def.name;
                    let metaclass_name = class_def.metaclass_name;
                    let methods = class_def.methods.clone();
                    let static_methods = class_def.static_methods.clone();
                    let superclass = class_def.superclass;
                    let metaclass = object::new_root_obj_class(
                        self,
                        metaclass_name,
                        base_metaclass,
                        Some(self.class_store.get_object_class()),
                        static_methods,
                    );

                    let defined_class = object::new_root_obj_class(
                        self,
                        name,
                        metaclass.as_gc(),
                        Some(superclass),
                        methods,
                    );
                    self.working_class_def = None;
                    *self.peek_mut(0) = Value::ObjClass(defined_class.as_gc());
                }

                byte if byte == OpCode::Inherit as u8 => {
                    let superclass = if let Some(ptr) = self.peek(1).try_as_obj_class() {
                        ptr
                    } else {
                        return Err(error!(
                            ErrorKind::RuntimeError,
                            "Superclass must be a class."
                        ));
                    };
                    self.working_class_def.as_mut().unwrap().superclass = superclass;
                    self.pop();
                }

                byte if byte == OpCode::Method as u8 => {
                    let name = read_string!();
                    self.define_method(name, false)?;
                }

                byte if byte == OpCode::StaticMethod as u8 => {
                    let name = read_string!();
                    self.define_method(name, true)?;
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
                let instance = object::new_gc_obj_instance(self, class);
                *self.peek_mut(arg_count) = Value::ObjInstance(instance);

                let init = class.methods.get(&self.init_string);
                if let Some(Value::ObjClosure(initialiser)) = init {
                    return self.call_closure(*initialiser, arg_count);
                } else if let Some(Value::ObjNative(initialiser)) = init {
                    return self.call_native(*initialiser, arg_count);
                } else if arg_count != 0 {
                    return Err(error!(
                        ErrorKind::TypeError,
                        "Expected 0 arguments but found {}.", arg_count
                    ));
                }

                Ok(())
            }

            Value::ObjClosure(function) => self.call_closure(function, arg_count),

            Value::ObjNative(wrapped) => self.call_native(wrapped, arg_count),

            _ => Err(error!(
                ErrorKind::TypeError,
                "Can only call functions and classes."
            )),
        }
    }

    fn invoke_from_class(
        &mut self,
        class: Gc<ObjClass>,
        name: Gc<ObjString>,
        arg_count: usize,
    ) -> Result<(), Error> {
        if let Some(value) = class.methods.get(&name) {
            return match value {
                Value::ObjClosure(closure) => self.call_closure(*closure, arg_count),
                Value::ObjNative(native) => self.call_native(*native, arg_count),
                _ => unreachable!(),
            };
        }
        Err(error!(
            ErrorKind::AttributeError,
            "Undefined property '{}'.", *name
        ))
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
            _ => {
                let class = receiver.get_class(&self.class_store);
                self.invoke_from_class(class, name, arg_count)
            }
        }
    }

    fn call_closure(
        &mut self,
        closure: Gc<RefCell<ObjClosure>>,
        arg_count: usize,
    ) -> Result<(), Error> {
        if arg_count as u32 + 1 != closure.borrow().function.arity {
            return Err(error!(
                ErrorKind::TypeError,
                "Expected {} arguments but found {}.",
                closure.borrow().function.arity - 1,
                arg_count
            ));
        }

        if self.frames.len() == common::FRAMES_MAX {
            return Err(error!(ErrorKind::IndexError, "Stack overflow."));
        }

        let chunk_index = closure.borrow().function.chunk_index;
        self.active_chunk = self.get_chunk(chunk_index);
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
        let frame_end = self.stack.len();
        let frame_begin = frame_end - arg_count - 1;
        // # Safety
        // Native functions accept a mutable Vm instance _and_ a slice of Value objects representing
        // arguments to the native function. In general this is unsafe, but because the stack cannot
        // be mutated via the Vm object, it is safe to hand the native function a mutable Vm _and_
        // an immutable slice of stack data.
        let args =
            unsafe { slice::from_raw_parts(&self.stack[frame_begin], frame_end - frame_begin) };
        let result = function(self, args)?;
        self.stack.truncate(frame_begin + 1);
        *self.peek_mut(0) = result;
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
            let chunk = self.get_chunk(frame.closure.borrow().function.chunk_index);
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

    fn define_method(&mut self, name: Gc<ObjString>, is_static: bool) -> Result<(), Error> {
        let method = *self.peek(0);
        let class_def = self.working_class_def.as_mut().unwrap();
        class_def.methods.insert(name, method);
        if is_static {
            class_def.static_methods.insert(name, method);
        }
        self.pop();

        Ok(())
    }

    fn bind_method(&mut self, class: Gc<ObjClass>, name: Gc<ObjString>) -> Result<(), Error> {
        let instance = *self.peek(0);
        let bound = match class.methods.get(&name) {
            Some(Value::ObjClosure(ptr)) => {
                Value::ObjBoundMethod(object::new_gc_obj_bound_method(self, instance, *ptr))
            }
            Some(Value::ObjNative(ptr)) => {
                Value::ObjBoundNative(object::new_gc_obj_bound_method(self, instance, *ptr))
            }
            None => {
                return Err(error!(
                    ErrorKind::AttributeError,
                    "Undefined property '{}'.", *name
                ));
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

    fn build_range(&mut self, begin: isize, end: isize) -> Gc<ObjRange> {
        // Ranges are cached using a crude LRU cache. Since the cache size is small it's reasonable
        // to store the cache elements in a Vec and just iterate.
        let result = self
            .range_cache
            .iter()
            .find(|&(r, _)| r.begin == begin && r.end == end);

        if let Some((range, _)) = result {
            return range.as_gc();
        }

        // Cache miss! Create the range and cache it.

        let range =
            object::new_root_obj_range(self, self.class_store.get_obj_range_class(), begin, end);
        let range_gc = range.as_gc();

        // Check the cache size. If we're at the limit, evict the oldest element.
        if self.range_cache.len() >= RANGE_CACHE_SIZE {
            let stale_pos = self
                .range_cache
                .iter()
                .enumerate()
                .max_by(|first, second| first.1 .1.elapsed().cmp(&second.1 .1.elapsed()))
                .map(|e| e.0)
                .expect("Expect to find max given non-empty Vec.");

            self.range_cache[stale_pos] = (range, time::Instant::now());
        } else {
            self.range_cache.push((range, time::Instant::now()));
        }

        range_gc
    }

    fn init_heap_allocated_data(&mut self) {
        let mut base_metaclass_ptr = unsafe { class_store::new_base_metaclass(self) };
        let root_base_metaclass = Root::from(base_metaclass_ptr);
        let mut object_class_ptr = self.allocate_bare(ObjClass {
            name: None,
            metaclass: root_base_metaclass.as_gc(),
            superclass: None,
            methods: object::new_obj_string_value_map(),
        });
        let root_object_class = Root::from(object_class_ptr);
        let mut string_metaclass_ptr = self.allocate_bare(ObjClass::new(
            root_base_metaclass.as_gc(),
            Some(root_object_class.as_gc()),
        ));
        let root_string_metaclass = Root::from(string_metaclass_ptr);
        let mut string_class_ptr = self.allocate_bare(ObjClass::new(
            root_string_metaclass.as_gc(),
            Some(root_object_class.as_gc()),
        ));

        self.string_class = Root::from(string_class_ptr);
        let object_class_name = self.new_gc_obj_string("Object");
        let base_metaclass_name = self.new_gc_obj_string("Type");
        let string_metaclass_name = self.new_gc_obj_string("StringClass");
        let string_class_name = self.new_gc_obj_string("String");
        // # Safety
        // We're modifying data for which there are immutable references held by other data
        // structures (namely metaclass and class objects). Because the code is single-threaded and
        // the immutable references aren't being used to access the data at this point in time
        // (class names are only used by the Display trait, and superclass and methods are only
        // accessed once code is run), mutating the data here should be safe.
        unsafe {
            object_class_ptr.as_mut().data.name = Some(object_class_name);
            base_metaclass_ptr.as_mut().data.name = Some(base_metaclass_name);
            base_metaclass_ptr.as_mut().data.superclass = Some(root_object_class.as_gc());
            string_metaclass_ptr.as_mut().data.name = Some(string_metaclass_name);
            string_class_ptr.as_mut().data.name = Some(string_class_name);
            core::bind_object_class(self, &mut object_class_ptr);
            core::bind_type_class(self, &mut base_metaclass_ptr);
            core::bind_gc_obj_string_class(self, &mut string_class_ptr, &mut string_metaclass_ptr);
        }

        let empty_chunk = self.allocate(Chunk::new());
        let init_string = self.new_gc_obj_string("__init__");
        let next_string = self.new_gc_obj_string("__next__");
        self.active_chunk = empty_chunk;
        self.init_string = init_string;
        self.next_string = next_string;
        let class_store =
            CoreClassStore::new_with_built_ins(self, root_base_metaclass, root_object_class);
        self.core_chunks = self.chunks.clone();
        self.class_store = class_store;
    }

    fn init_built_in_globals(&mut self) {
        self.define_native("clock", core::clock);
        self.define_native("type", core::type_);
        self.define_native("print", core::print);
        self.define_native("sentinel", core::sentinel);
        let base_metaclass = self.class_store.get_base_metaclass();
        self.set_global("Type", Value::ObjClass(base_metaclass));
        let object_class = self.class_store.get_object_class();
        self.set_global("Object", Value::ObjClass(object_class));
        let nil_class = self.class_store.get_nil_class();
        self.set_global("Nil", Value::ObjClass(nil_class));
        let boolean_class = self.class_store.get_boolean_class();
        self.set_global("Bool", Value::ObjClass(boolean_class));
        let number_class = self.class_store.get_number_class();
        self.set_global("Num", Value::ObjClass(number_class));
        let sentinel_class = self.class_store.get_sentinel_class();
        self.set_global("Sentinel", Value::ObjClass(sentinel_class));
        let obj_closure_class = self.class_store.get_obj_closure_class();
        self.set_global("Func", Value::ObjClass(obj_closure_class));
        let obj_native_class = self.class_store.get_obj_native_class();
        self.set_global("BuiltIn", Value::ObjClass(obj_native_class));
        let obj_closure_method_class = self.class_store.get_obj_closure_method_class();
        self.set_global("Method", Value::ObjClass(obj_closure_method_class));
        let obj_native_method_class = self.class_store.get_obj_native_method_class();
        self.set_global("BuiltInMethod", Value::ObjClass(obj_native_method_class));
        let obj_string_class = self.string_class.as_gc();
        self.set_global("String", Value::ObjClass(obj_string_class));
        let obj_iter_class = self.class_store.get_obj_iter_class();
        self.set_global("Iter", Value::ObjClass(obj_iter_class));
        let obj_map_iter_class = self.class_store.get_obj_map_iter_class();
        self.set_global("MapIter", Value::ObjClass(obj_map_iter_class));
        let obj_filter_iter_class = self.class_store.get_obj_filter_iter_class();
        self.set_global("FilterIter", Value::ObjClass(obj_filter_iter_class));
        let obj_vec_class = self.class_store.get_obj_vec_class();
        self.set_global("Vec", Value::ObjClass(obj_vec_class));
        let obj_range_class = self.class_store.get_obj_range_class();
        self.set_global("Range", Value::ObjClass(obj_range_class));
    }

    fn frame(&self) -> &CallFrame {
        self.frames.last().expect("Call stack empty.")
    }

    fn peek(&self, depth: usize) -> &Value {
        self.stack.peek(depth)
    }

    fn peek_mut(&mut self, depth: usize) -> &mut Value {
        self.stack.peek_mut(depth)
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
        if let Some(def) = self.working_class_def.as_ref() {
            def.mark()
        }
    }

    fn blacken(&self) {
        self.stack.blacken();
        self.globals.blacken();
        self.frames.blacken();
        self.open_upvalues.blacken();
        if let Some(def) = self.working_class_def.as_ref() {
            def.blacken()
        }
    }
}
