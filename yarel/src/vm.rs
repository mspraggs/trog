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

#[allow(unused_imports)]
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use std::hash::{Hash, Hasher};
use std::hint;
use std::io;
use std::path::Path;
use std::ptr;
use std::time;

use crate::chunk::{Chunk, OpCode};
use crate::class_store::CoreClassStore;
use crate::common;
use crate::compiler;
use crate::core;
use crate::debug;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, FnvHasher};
use crate::memory::{self, Gc, Root, UniqueRoot};
use crate::object::{
    self, NativeFn, ObjBoundMethod, ObjClass, ObjClosure, ObjFiber, ObjFunction, ObjHashMap,
    ObjInstance, ObjModule, ObjNative, ObjRange, ObjRangeIter, ObjString, ObjStringIter,
    ObjStringValueMap, ObjTuple, ObjTupleIter, ObjUpvalue, ObjVec, ObjVecIter,
};
use crate::utils;
use crate::value::Value;

const RANGE_CACHE_SIZE: usize = 8;

type LoadModuleFn = fn(&str) -> Result<String, Error>;

pub fn interpret(vm: &mut Vm, source: String, module_path: Option<&str>) -> Result<Value, Error> {
    let compile_result = compiler::compile(vm, source, module_path);
    match compile_result {
        Ok(function) => vm.execute(function, &[]),
        Err(error) => Err(error),
    }
}

struct ClassDef {
    class: UniqueRoot<ObjClass>,
    metaclass: UniqueRoot<ObjClass>,
}

impl ClassDef {
    fn new(class: UniqueRoot<ObjClass>, metaclass: UniqueRoot<ObjClass>) -> Self {
        ClassDef { class, metaclass }
    }
}

fn default_read_module_source(path: &str) -> Result<String, Error> {
    let path = Path::new(path).with_extension("yl");
    let filename = match path.as_path().to_str() {
        Some(p) => p,
        None => {
            return Err(error!(
                ErrorKind::RuntimeError,
                "Error converting module path to string."
            ));
        }
    };

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            let reason = match e.kind() {
                io::ErrorKind::NotFound => "file not found",
                io::ErrorKind::PermissionDenied => "permission denied",
                io::ErrorKind::ConnectionRefused => "connection refused",
                io::ErrorKind::ConnectionReset => "connection reset",
                io::ErrorKind::ConnectionAborted => "connection aborted",
                io::ErrorKind::NotConnected => "not connected",
                io::ErrorKind::AddrInUse => "address in use",
                io::ErrorKind::AddrNotAvailable => "address not available",
                io::ErrorKind::BrokenPipe => "broken pipe",
                io::ErrorKind::AlreadyExists => "already exists",
                io::ErrorKind::WouldBlock => "would block",
                io::ErrorKind::InvalidInput => "invalid input",
                io::ErrorKind::InvalidData => "invalid data",
                io::ErrorKind::TimedOut => "timed out",
                io::ErrorKind::WriteZero => "write zero",
                io::ErrorKind::Interrupted => "interrupted",
                io::ErrorKind::Other => "other",
                io::ErrorKind::UnexpectedEof => "unexpected end-of-file",
                _ => "other",
            };
            return Err(error!(
                crate::error::ErrorKind::ImportError,
                "Unable to read file '{}' ({}).", filename, reason
            ));
        }
    };

    Ok(source)
}

pub struct Vm {
    ip: *const u8,
    active_module: Gc<RefCell<ObjModule>>,
    active_chunk: Gc<Chunk>,
    fiber: Option<Root<RefCell<ObjFiber>>>,
    unsafe_fiber: *mut ObjFiber,
    next_string: Gc<ObjString>,
    class_store: CoreClassStore,
    chunks: Vec<Root<Chunk>>,
    modules: HashMap<Gc<ObjString>, Root<RefCell<ObjModule>>, BuildPassThroughHasher>,
    core_chunks: Vec<Root<Chunk>>,
    string_class: Option<Root<ObjClass>>,
    string_store: string_store::ObjStringStore,
    range_cache: Vec<(Root<ObjRange>, time::Instant)>,
    working_class_def: Option<ClassDef>,
    module_loader: LoadModuleFn,
    printer: NativeFn,
    handling_exception: bool,
}

impl Vm {
    pub fn new() -> Self {
        // # Safety
        // We create some dangling GC pointers here. This is safe because the fields are
        // re-assigned immediately to valid GC pointers by init_heap_allocated_data.
        let mut vm = Vm {
            ip: ptr::null(),
            active_module: Gc::dangling(),
            active_chunk: Gc::dangling(),
            fiber: None,
            unsafe_fiber: ptr::null_mut(),
            next_string: Gc::dangling(),
            class_store: CoreClassStore::new_empty(),
            chunks: Vec::new(),
            modules: HashMap::with_hasher(BuildPassThroughHasher::default()),
            core_chunks: Vec::new(),
            string_class: None,
            string_store: string_store::ObjStringStore::new(),
            range_cache: Vec::with_capacity(RANGE_CACHE_SIZE),
            module_loader: default_read_module_source,
            printer: core::print,
            working_class_def: None,
            handling_exception: false,
        };
        vm.init_heap_allocated_data();
        vm
    }

    pub fn with_built_ins() -> Self {
        let mut vm = Self::new();
        vm.init_built_in_globals("main");
        vm
    }

    pub fn set_printer(&mut self, printer: NativeFn) {
        self.printer = printer;
        self.define_native("main", "print", self.printer);
    }

    pub fn set_module_loader(&mut self, loader: fn(&str) -> Result<String, Error>) {
        self.module_loader = loader;
    }

    pub fn execute(&mut self, function: Root<ObjFunction>, args: &[Value]) -> Result<Value, Error> {
        self.ip = ptr::null();
        self.fiber = None;
        let module = self.module(&function.module_path);
        let closure = self.new_root_obj_closure(function.as_gc(), module);
        let fiber = self.new_root_obj_fiber(closure.as_gc());
        let arity = closure.function.arity - 1;
        if arity != args.len() {
            return Err(error!(
                ErrorKind::TypeError,
                "Expected {} arguments but found {}.",
                arity,
                args.len()
            ));
        }
        self.load_fiber(fiber.as_gc(), None)?;
        for &arg in args {
            self.push(arg);
        }
        match self.run() {
            Ok(value) => Ok(value),
            Err(mut error) => Err(self.runtime_error(&mut error)),
        }
    }

    pub fn global(&mut self, module_name: &str, var_name: &str) -> Option<Value> {
        let var_name = self.new_gc_obj_string(var_name);
        self.module(module_name)
            .borrow()
            .attributes
            .get(&var_name)
            .copied()
    }

    pub fn set_global(&mut self, module_name: &str, var_name: &str, value: Value) {
        let var_name = self.new_gc_obj_string(var_name);
        self.module(module_name)
            .borrow_mut()
            .attributes
            .insert(var_name, value);
    }

    pub fn define_native(&mut self, module_name: &str, var_name: &str, function: NativeFn) {
        let var_name = self.new_gc_obj_string(var_name);
        let native = self.new_root_obj_native(var_name, function);
        self.module(module_name)
            .borrow_mut()
            .attributes
            .insert(var_name, Value::ObjNative(native.as_gc()));
    }

    pub fn get_class(&self, value: Value) -> Gc<ObjClass> {
        match value {
            Value::Boolean(_) => self.class_store.boolean_class(),
            Value::Number(_) => self.class_store.num_class(),
            Value::ObjString(string) => string.class,
            Value::ObjStringIter(iter) => iter.borrow().class,
            Value::ObjFunction(_) => {
                if cfg!(any(debug_assertions, feature = "safe_class_lookup")) {
                    unreachable!()
                } else {
                    unsafe { hint::unreachable_unchecked() }
                }
            }
            Value::ObjNative(_) => self.class_store.native_class(),
            Value::ObjClosure(_) => self.class_store.closure_class(),
            Value::ObjClass(class) => class.metaclass,
            Value::ObjInstance(instance) => instance.borrow().class,
            Value::ObjBoundMethod(_) => self.class_store.closure_method_class(),
            Value::ObjBoundNative(_) => self.class_store.native_method_class(),
            Value::ObjTuple(tuple) => tuple.class,
            Value::ObjTupleIter(iter) => iter.borrow().class,
            Value::ObjVec(vec) => vec.borrow().class,
            Value::ObjVecIter(iter) => iter.borrow().class,
            Value::ObjRange(range) => range.class,
            Value::ObjRangeIter(iter) => iter.borrow().class,
            Value::ObjHashMap(hash_map) => hash_map.borrow().class,
            Value::ObjModule(module) => module.borrow().class,
            Value::ObjFiber(fiber) => fiber.borrow().class,
            Value::None => self.class_store.nil_class(),
        }
    }

    pub fn new_gc_obj_string(&mut self, data: &str) -> Gc<ObjString> {
        let hash = {
            let mut hasher = FnvHasher::new();
            (*data).hash(&mut hasher);
            hasher.finish()
        };
        let key = (hash, data);
        if let Some(string) = self.string_store.get(key) {
            return string.as_gc();
        }
        let string = Root::new(ObjString::new(
            self.string_class.as_ref().expect("Expected Root.").as_gc(),
            data,
            hash,
        ));
        let ret = string.as_gc();
        self.string_store.insert(string);
        ret
    }

    pub fn new_root_obj_upvalue(&mut self, value: &mut Value) -> Root<RefCell<ObjUpvalue>> {
        Root::new(RefCell::new(ObjUpvalue::new(value)))
    }

    pub fn new_root_obj_function(
        &mut self,
        name: Gc<ObjString>,
        arity: usize,
        upvalue_count: usize,
        chunk: Gc<Chunk>,
        module_path: Gc<ObjString>,
    ) -> Root<ObjFunction> {
        Root::new(ObjFunction::new(
            name,
            arity,
            upvalue_count,
            chunk,
            module_path,
        ))
    }

    pub fn new_root_obj_native(
        &mut self,
        name: Gc<ObjString>,
        function: NativeFn,
    ) -> Root<ObjNative> {
        Root::new(ObjNative::new(name, function))
    }

    pub fn new_root_obj_closure(
        &mut self,
        function: Gc<ObjFunction>,
        module: Gc<RefCell<ObjModule>>,
    ) -> Root<ObjClosure> {
        let upvalue_roots: Vec<Root<RefCell<ObjUpvalue>>> = (0..function.upvalue_count)
            .map(|_| Root::new(RefCell::new(ObjUpvalue::new(ptr::null_mut()))))
            .collect();
        let upvalues = upvalue_roots.iter().map(|u| u.as_gc()).collect();
        Root::new(ObjClosure::new(function, upvalues, module))
    }

    pub fn new_root_obj_class(
        &mut self,
        name: Gc<ObjString>,
        metaclass: Gc<ObjClass>,
        superclass: Option<Gc<ObjClass>>,
        methods: ObjStringValueMap,
    ) -> Root<ObjClass> {
        Root::new(ObjClass::new(name, metaclass, superclass, methods))
    }

    pub fn new_root_obj_instance(&mut self, class: Gc<ObjClass>) -> Root<RefCell<ObjInstance>> {
        Root::new(RefCell::new(ObjInstance::new(class)))
    }

    pub fn new_root_obj_bound_method<T: 'static + memory::GcManaged>(
        &mut self,
        receiver: Value,
        method: Gc<T>,
    ) -> Root<RefCell<ObjBoundMethod<T>>> {
        Root::new(RefCell::new(ObjBoundMethod::new(receiver, method)))
    }

    pub fn new_root_obj_string_iter(
        &mut self,
        string: Gc<ObjString>,
    ) -> Root<RefCell<ObjStringIter>> {
        let class = self.class_store.string_iter_class();
        Root::new(RefCell::new(ObjStringIter::new(class, string)))
    }

    pub fn new_root_obj_hash_map(&mut self) -> Root<RefCell<ObjHashMap>> {
        let class = self.class_store.hash_map_class();
        Root::new(RefCell::new(ObjHashMap::new(class)))
    }

    pub fn new_root_obj_range(&mut self, begin: isize, end: isize) -> Root<ObjRange> {
        self.build_range(begin, end).as_root()
    }

    pub fn new_root_obj_range_iter(&mut self, range: Gc<ObjRange>) -> Root<RefCell<ObjRangeIter>> {
        let class = self.class_store.range_iter_class();
        Root::new(RefCell::new(ObjRangeIter::new(class, range)))
    }

    pub fn new_root_obj_tuple(&mut self, elements: Vec<Value>) -> Root<ObjTuple> {
        let class = self.class_store.tuple_class();
        Root::new(ObjTuple::new(class, elements))
    }

    pub fn new_root_obj_tuple_iter(&mut self, tuple: Gc<ObjTuple>) -> Root<RefCell<ObjTupleIter>> {
        let class = self.class_store.tuple_iter_class();
        Root::new(RefCell::new(ObjTupleIter::new(class, tuple)))
    }

    pub fn new_root_obj_vec(&mut self) -> Root<RefCell<ObjVec>> {
        let class = self.class_store.vec_class();
        Root::new(RefCell::new(ObjVec::new(class)))
    }

    pub fn new_root_obj_vec_iter(&mut self, vec: Gc<RefCell<ObjVec>>) -> Root<RefCell<ObjVecIter>> {
        let class = self.class_store.vec_iter_class();
        Root::new(RefCell::new(ObjVecIter::new(class, vec)))
    }

    pub fn new_root_obj_module(
        &mut self,
        class: Gc<ObjClass>,
        path: Gc<ObjString>,
    ) -> Root<RefCell<ObjModule>> {
        Root::new(RefCell::new(ObjModule::new(class, path)))
    }

    pub fn new_root_obj_err(&mut self, context: Value) -> Root<RefCell<ObjInstance>> {
        let class = self.class_store.error_class();
        self.new_root_obj_err_with_class(class, context)
    }

    pub fn new_root_obj_stop_iter(&mut self) -> Root<RefCell<ObjInstance>> {
        let class = self.class_store.stop_iter_class();
        self.new_root_obj_err_with_class(class, Value::None)
    }

    pub(crate) fn new_root_obj_fiber(
        &mut self,
        closure: Gc<ObjClosure>,
    ) -> Root<RefCell<ObjFiber>> {
        let class = self.class_store.fiber_class();
        Root::new(RefCell::new(ObjFiber::new(class, closure)))
    }

    pub fn reset(&mut self) {
        self.reset_stack();
        self.chunks = self.core_chunks.clone();
        self.modules.retain(|&k, _| k.as_str() == "main");
        self.active_module = self.module("main");
        self.active_module.borrow_mut().attributes = object::new_obj_string_value_map();
        self.init_built_in_globals("main");
    }

    pub(crate) fn module(&mut self, path: &str) -> Gc<RefCell<ObjModule>> {
        let path = self.new_gc_obj_string(path);
        if let Some(module) = self.modules.get(&path) {
            return module.as_gc();
        }
        let module = Root::new(RefCell::new(ObjModule::new(
            self.class_store.module_class(),
            path,
        )));
        let gc_module = module.as_gc();
        self.modules.insert(path, module);
        gc_module
    }

    pub fn peek(&self, depth: usize) -> Value {
        *self.active_fiber().stack.peek(depth)
    }

    pub(crate) fn push(&mut self, value: Value) {
        self.active_fiber_mut().stack.push(value)
    }

    pub(crate) fn pop(&mut self) -> Value {
        self.active_fiber_mut()
            .stack
            .pop()
            .expect("Expected Value.")
    }

    pub(crate) fn add_chunk(&mut self, chunk: Chunk) -> Gc<Chunk> {
        let root = Root::new(chunk);
        let ret = root.as_gc();
        self.chunks.push(root);
        ret
    }

    pub(crate) fn load_fiber(
        &mut self,
        fiber: Gc<RefCell<ObjFiber>>,
        arg: Option<Value>,
    ) -> Result<(), Error> {
        {
            let borrowed_fiber = fiber.borrow();
            if borrowed_fiber.has_finished() {
                return Err(error!(
                    ErrorKind::RuntimeError,
                    "Cannot call a finished fiber."
                ));
            }
            if borrowed_fiber.caller.is_some() {
                return Err(error!(
                    ErrorKind::RuntimeError,
                    "Cannot call a fiber that has already been called.",
                ));
            }
        }
        if self.fiber.is_some() {
            self.active_fiber_mut().current_frame_mut().unwrap().ip = self.ip;
        }

        self.unsafe_fiber = (*fiber).as_ptr();
        let caller = self.fiber.replace(fiber.as_root());
        self.active_fiber_mut().caller = caller.map(|p| p.as_gc());

        if self.active_fiber().is_new() {
            let closure = self.active_fiber().frames[0].closure;
            self.push(Value::ObjClosure(closure));
            if let Some(arg) = arg {
                self.push(Value::None);
                self.push(arg);
            }
        } else if let Some(arg) = arg {
            self.push(Value::None);
            self.poke(0, arg);
        }

        self.load_frame();
        Ok(())
    }

    pub(crate) fn unload_fiber(&mut self, arg: Option<Value>) -> Result<(), Error> {
        if !self.active_fiber().has_finished() {
            self.active_fiber_mut().current_frame_mut().unwrap().ip = self.ip;
        }
        let caller = self.active_fiber().caller;
        if let Some(caller) = caller {
            let mut current = self.fiber.replace(caller.as_root());
            self.unsafe_fiber = (*caller).as_ptr();
            current.as_mut().unwrap().borrow_mut().caller = None;
        } else {
            return Err(error!(
                ErrorKind::RuntimeError,
                "Cannot yield from module-level code."
            ));
        }
        if let Some(arg) = arg {
            self.poke(0, arg);
        } else {
            self.pop();
            self.poke(0, Value::None);
        }
        self.load_frame();
        Ok(())
    }

    fn run(&mut self) -> Result<Value, Error> {
        debug_assert!(self.modules.len() == 1);

        loop {
            if cfg!(feature = "debug_trace") {
                println!("          {}", self.active_fiber().stack);
                let offset = self.active_chunk.code_offset(self.ip);
                debug::disassemble_instruction(&self.active_chunk, offset);
            }
            let byte = self.read_byte();

            match byte {
                byte if byte == OpCode::Constant as u8 => {
                    let constant = self.read_constant();
                    self.push(constant);
                }
                byte if byte == OpCode::Nil as u8 => self.push(Value::None),
                byte if byte == OpCode::True as u8 => self.push(Value::Boolean(true)),
                byte if byte == OpCode::False as u8 => self.push(Value::Boolean(false)),
                byte if byte == OpCode::Pop as u8 => {
                    self.pop();
                }
                byte if byte == OpCode::CopyTop as u8 => {
                    let top = self.peek(0);
                    self.push(top);
                }
                byte if byte == OpCode::GetLocal as u8 => self.get_local_impl(),
                byte if byte == OpCode::SetLocal as u8 => self.set_local_impl(),
                byte if byte == OpCode::GetGlobal as u8 => self.get_global_impl()?,
                byte if byte == OpCode::DefineGlobal as u8 => self.define_global_impl(),
                byte if byte == OpCode::SetGlobal as u8 => self.set_global_impl()?,
                byte if byte == OpCode::GetUpvalue as u8 => self.get_upvalue_impl(),
                byte if byte == OpCode::SetUpvalue as u8 => self.set_upvalue_impl(),
                byte if byte == OpCode::GetProperty as u8 => self.get_property_impl()?,
                byte if byte == OpCode::SetProperty as u8 => self.set_property_impl()?,
                byte if byte == OpCode::GetClass as u8 => self.get_class_impl(),
                byte if byte == OpCode::GetSuper as u8 => self.get_super_impl()?,
                byte if byte == OpCode::Equal as u8 => self.equal_impl(),
                byte if byte == OpCode::Greater as u8 => {
                    self.binary_op_impl(|a, b| Value::Boolean(a > b))?;
                }
                byte if byte == OpCode::Less as u8 => {
                    self.binary_op_impl(|a, b| Value::Boolean(a < b))?;
                }
                byte if byte == OpCode::Add as u8 => self.add_impl()?,
                byte if byte == OpCode::Subtract as u8 => {
                    self.binary_op_impl(|a, b| Value::Number(a - b))?
                }
                byte if byte == OpCode::Multiply as u8 => {
                    self.binary_op_impl(|a, b| Value::Number(a * b))?
                }
                byte if byte == OpCode::Divide as u8 => {
                    self.binary_op_impl(|a, b| Value::Number(a / b))?
                }
                byte if byte == OpCode::BitwiseAnd as u8 => {
                    self.binary_op_impl(|a, b| Value::Number(((a as i64) & (b as i64)) as f64))?;
                }
                byte if byte == OpCode::BitwiseOr as u8 => {
                    self.binary_op_impl(|a, b| Value::Number(((a as i64) | (b as i64)) as f64))?;
                }
                byte if byte == OpCode::BitwiseXor as u8 => {
                    self.binary_op_impl(|a, b| Value::Number(((a as i64) ^ (b as i64)) as f64))?;
                }
                byte if byte == OpCode::Modulo as u8 => {
                    self.binary_op_impl(|a, b| Value::Number(a % b))?;
                }
                byte if byte == OpCode::LogicalNot as u8 => self.logical_not_impl(),
                byte if byte == OpCode::BitwiseNot as u8 => self.bitwise_not_impl()?,
                byte if byte == OpCode::BitShiftLeft as u8 => {
                    self.binary_op_impl(|a, b| {
                        Value::Number((a as i64).checked_shl(b as u32).unwrap_or_default() as f64)
                    })?;
                }
                byte if byte == OpCode::BitShiftRight as u8 => {
                    self.binary_op_impl(|a, b| {
                        Value::Number((a as i64).checked_shr(b as u32).unwrap_or_default() as f64)
                    })?;
                }
                byte if byte == OpCode::Negate as u8 => self.negate_impl()?,
                byte if byte == OpCode::FormatString as u8 => self.format_string_impl(),
                byte if byte == OpCode::BuildHashMap as u8 => self.build_hash_map_impl()?,
                byte if byte == OpCode::BuildRange as u8 => self.build_range_impl()?,
                byte if byte == OpCode::BuildString as u8 => self.build_string_impl(),
                byte if byte == OpCode::BuildTuple as u8 => self.build_tuple_impl(),
                byte if byte == OpCode::BuildVec as u8 => self.build_vec_impl(),
                byte if byte == OpCode::IterNext as u8 => self.iter_next_impl()?,
                byte if byte == OpCode::Jump as u8 => self.jump_impl(),
                byte if byte == OpCode::JumpIfFalse as u8 => self.jump_if_false_impl(),
                byte if byte == OpCode::JumpIfStopIter as u8 => self.jump_if_stop_iter(),
                byte if byte == OpCode::Loop as u8 => self.loop_impl(),
                byte if byte == OpCode::JumpFinally as u8 => self.jump_finally_impl(),
                byte if byte == OpCode::EndFinally as u8 => self.end_finally_impl()?,
                byte if byte == OpCode::PushExcHandler as u8 => self.push_exc_handler_impl(),
                byte if byte == OpCode::PopExcHandler as u8 => self.pop_exc_handler_impl(),
                byte if byte == OpCode::Throw as u8 => self.throw_impl()?,
                byte if byte == OpCode::Call as u8 => self.call_impl()?,
                byte if byte == OpCode::Construct as u8 => self.construct_impl(),
                byte if byte == OpCode::Invoke as u8 => self.invoke_impl()?,
                byte if byte == OpCode::SuperInvoke as u8 => self.super_invoke_impl()?,
                byte if byte == OpCode::Closure as u8 => self.closure_impl(),
                byte if byte == OpCode::CloseUpvalue as u8 => self.close_upvalue_impl(),
                byte if byte == OpCode::Return as u8 => {
                    if let Some(value) = self.return_impl()? {
                        return Ok(value);
                    }
                }
                byte if byte == OpCode::DeclareClass as u8 => self.declare_class_impl(),
                byte if byte == OpCode::DefineClass as u8 => self.define_class_impl(),
                byte if byte == OpCode::Inherit as u8 => self.inherit_impl()?,
                byte if byte == OpCode::Method as u8 => self.method_impl()?,
                byte if byte == OpCode::StaticMethod as u8 => self.static_method_impl()?,
                byte if byte == OpCode::StartImport as u8 => self.start_import_impl()?,
                byte if byte == OpCode::FinishImport as u8 => self.finish_import_impl(),
                _ => {
                    if cfg!(any(debug_assertions, feature = "safe_vm_opcodes")) {
                        panic!("Unknown opcode {}", byte);
                    } else {
                        unsafe {
                            hint::unreachable_unchecked();
                        }
                    }
                }
            }
        }
    }

    fn read_byte(&mut self) -> u8 {
        unsafe {
            let ret = *self.ip;
            self.ip = self.ip.offset(1);
            ret
        }
    }

    fn read_short(&mut self) -> u16 {
        unsafe {
            let ret = u16::from_ne_bytes([*self.ip, *self.ip.offset(1)]);
            self.ip = self.ip.offset(2);
            ret
        }
    }

    fn read_constant(&mut self) -> Value {
        let index = self.read_short() as usize;
        self.active_chunk.constants[index]
    }

    fn read_string(&mut self) -> Gc<ObjString> {
        self.read_constant()
            .try_as_obj_string()
            .expect("Expected variable name.")
    }

    fn get_local_impl(&mut self) {
        let slot = self.read_byte() as usize;
        let slot_base = self.active_fiber().current_frame().unwrap().slot_base;
        let value = self.active_fiber().stack[slot_base + slot];
        self.push(value);
    }

    fn set_local_impl(&mut self) {
        let slot = self.read_byte() as usize;
        let slot_base = self.active_fiber().current_frame().unwrap().slot_base;
        self.active_fiber_mut().stack[slot_base + slot] = self.peek(0);
    }

    fn get_global_impl(&mut self) -> Result<(), Error> {
        let name = self.read_string();
        let value = self
            .active_module
            .borrow()
            .attributes
            .get(&name)
            .map(|&v| v);
        if let Some(value) = value {
            self.push(value);
        } else {
            let err = error!(ErrorKind::NameError, "Undefined variable '{}'.", *name);
            self.try_handle_error(err)?;
        }
        Ok(())
    }

    fn define_global_impl(&mut self) {
        let name = self.read_string();
        let value = self.peek(0);
        self.active_module
            .borrow_mut()
            .attributes
            .insert(name, value);
        self.pop();
    }

    fn set_global_impl(&mut self) -> Result<(), Error> {
        let name = self.read_string();
        let value = self.peek(0);
        let global_is_undefined = {
            let globals = &mut self.active_module.borrow_mut().attributes;
            let prev = globals.insert(name, value);
            if prev.is_none() {
                globals.remove(&name);
            }
            prev.is_none()
        };
        if global_is_undefined {
            let err = error!(ErrorKind::NameError, "Undefined variable '{}'.", *name);
            self.try_handle_error(err)?;
        }
        Ok(())
    }

    fn get_upvalue_impl(&mut self) {
        let upvalue_index = self.read_byte() as usize;
        let upvalue = self
            .active_fiber()
            .current_frame()
            .unwrap()
            .closure
            .upvalues
            .borrow()[upvalue_index]
            .borrow()
            .get();
        self.push(upvalue);
    }

    fn set_upvalue_impl(&mut self) {
        let upvalue_index = self.read_byte() as usize;
        let stack_value = self.peek(0);
        let closure = self.active_fiber().current_frame().unwrap().closure;
        closure.upvalues.borrow_mut()[upvalue_index]
            .borrow_mut()
            .set(stack_value);
    }

    fn get_property_impl(&mut self) -> Result<(), Error> {
        let name = self.read_string();

        if let Some(instance) = self.peek(0).try_as_obj_instance() {
            let borrowed_instance = instance.borrow();
            if let Some(&property) = borrowed_instance.fields.get(&name) {
                self.pop();
                self.push(property);
                return Ok(());
            }
        }
        if let Some(module) = self.peek(0).try_as_obj_module() {
            if let Some(&property) = module.borrow().attributes.get(&name) {
                self.pop();
                self.push(property);
                return Ok(());
            }
        }

        let class = self.get_class(self.peek(0));
        self.bind_method(class, name)
    }

    fn set_property_impl(&mut self) -> Result<(), Error> {
        if let Some(module) = self.peek(1).try_as_obj_module() {
            let name = self.read_string();
            let value = self.peek(0);
            module.borrow_mut().attributes.insert(name, value);
            self.pop();
            self.pop();
            self.push(value);
            return Ok(());
        }
        let instance = if let Some(ptr) = self.peek(1).try_as_obj_instance() {
            ptr
        } else {
            let err = error!(ErrorKind::AttributeError, "Only instances have fields.");
            return self.try_handle_error(err);
        };
        let name = self.read_string();
        let value = self.peek(0);
        instance.borrow_mut().fields.insert(name, value);

        self.pop();
        self.pop();
        self.push(value);
        Ok(())
    }

    fn get_class_impl(&mut self) {
        let value = self.peek(0);
        match value {
            Value::ObjClass(_) => {}
            Value::ObjInstance(instance) => {
                self.poke(0, Value::ObjClass(instance.borrow().class));
            }
            _ => {
                let class = self.get_class(value);
                self.poke(0, Value::ObjClass(class));
            }
        }
    }

    fn get_super_impl(&mut self) -> Result<(), Error> {
        let name = self.read_string();
        let superclass = self.pop().try_as_obj_class().expect("Expected ObjClass.");

        self.bind_method(superclass, name)
    }

    fn equal_impl(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.push(Value::Boolean(a == b));
    }

    fn binary_op_impl(&mut self, op: fn(f64, f64) -> Value) -> Result<(), Error> {
        let second_value = self.pop();
        let first_value = self.pop();
        let (first, second) = match (first_value, second_value) {
            (Value::Number(first), Value::Number(second)) => (first, second),
            _ => {
                let err = error!(
                    ErrorKind::TypeError,
                    "Binary operands must both be numbers."
                );
                return self.try_handle_error(err);
            }
        };
        self.push(op(first, second));
        Ok(())
    }

    fn add_impl(&mut self) -> Result<(), Error> {
        let b = self.pop();
        let a = self.pop();
        match (a, b) {
            (Value::ObjString(a), Value::ObjString(b)) => {
                let value =
                    Value::ObjString(self.new_gc_obj_string(format!("{}{}", *a, *b).as_str()));
                self.push(value)
            }

            (Value::Number(a), Value::Number(b)) => {
                self.push(Value::Number(a + b));
            }

            _ => {
                let err = error!(
                    ErrorKind::TypeError,
                    "Binary operands must be two numbers or two strings.",
                );
                self.try_handle_error(err)?;
            }
        }
        Ok(())
    }

    fn logical_not_impl(&mut self) {
        let value = self.pop();
        self.push(Value::Boolean(!value.as_bool()));
    }

    fn bitwise_not_impl(&mut self) -> Result<(), Error> {
        let value = self.pop();
        if let Some(num) = value.try_as_number() {
            self.push(Value::Number(!(num as i64) as f64));
        } else {
            let err = error!(ErrorKind::TypeError, "Unary operand must be a number.");
            self.try_handle_error(err)?;
        }
        Ok(())
    }

    fn negate_impl(&mut self) -> Result<(), Error> {
        let value = self.pop();
        if let Some(num) = value.try_as_number() {
            self.push(Value::Number(-num));
        } else {
            let err = error!(ErrorKind::TypeError, "Unary operand must be a number.");
            self.try_handle_error(err)?;
        }
        Ok(())
    }

    fn format_string_impl(&mut self) {
        let value = self.peek(0);
        if value.try_as_obj_string().is_some() {
            return;
        }
        let obj = Value::ObjString(self.new_gc_obj_string(format!("{}", value).as_str()));
        self.poke(0, obj);
    }

    fn build_hash_map_impl(&mut self) -> Result<(), Error> {
        let num_elements = self.read_byte() as usize;
        match self.build_hash_map(num_elements) {
            Ok(map) => {
                self.push(Value::ObjHashMap(map.as_gc()));
            }
            Err(e) => {
                self.try_handle_error(e)?;
            }
        }
        Ok(())
    }

    fn build_range_impl(&mut self) -> Result<(), Error> {
        macro_rules! pop_integer {
            () => {
                match utils::validate_integer(self.pop()) {
                    Ok(i) => i,
                    Err(e) => {
                        return self.try_handle_error(e);
                    }
                }
            };
        }
        let end = pop_integer!();
        let begin = pop_integer!();
        let range = self.build_range(begin, end);
        self.push(Value::ObjRange(range));
        Ok(())
    }

    fn build_string_impl(&mut self) {
        let num_operands = self.read_byte() as usize;
        if num_operands == 1 {
            return;
        }
        let mut new_string = String::new();
        for pos in (0..num_operands).rev() {
            new_string.push_str(self.peek(pos).try_as_obj_string().unwrap().as_str())
        }
        self.discard(num_operands);
        let value = Value::ObjString(self.new_gc_obj_string(new_string.as_str()));
        self.push(value);
    }

    fn build_tuple_impl(&mut self) {
        let num_operands = self.read_byte() as usize;
        let begin = self.stack_size() - num_operands;
        let end = self.stack_size();
        let elements = self.active_fiber().stack[begin..end]
            .iter()
            .copied()
            .collect();
        let tuple = self.new_root_obj_tuple(elements);
        self.discard(num_operands);
        self.push(Value::ObjTuple(tuple.as_gc()));
    }

    fn build_vec_impl(&mut self) {
        let num_operands = self.read_byte() as usize;
        let vec = self.new_root_obj_vec();
        let begin = self.stack_size() - num_operands;
        let end = self.stack_size();
        vec.borrow_mut().elements = self.active_fiber().stack[begin..end]
            .iter()
            .copied()
            .collect();
        self.discard(num_operands);
        self.push(Value::ObjVec(vec.as_gc()));
    }

    fn iter_next_impl(&mut self) -> Result<(), Error> {
        let iter = self.peek(0);
        self.push(iter);
        self.invoke(self.next_string, 0)
    }

    fn jump_impl(&mut self) {
        let offset = self.read_short();
        self.ip = unsafe { self.ip.offset(offset as isize) };
    }

    fn jump_if_false_impl(&mut self) {
        let offset = self.read_short();
        if !self.peek(0).as_bool() {
            self.ip = unsafe { self.ip.offset(offset as isize) };
        }
    }

    fn jump_if_stop_iter(&mut self) {
        let offset = self.read_short();
        let stop_iter_class = self.class_store.stop_iter_class();
        if let Some(instance) = self.peek(0).try_as_obj_instance() {
            if instance.borrow().class == stop_iter_class {
                self.ip = unsafe { self.ip.offset(offset as isize) };
            }
        }
    }

    fn loop_impl(&mut self) {
        let offset = self.read_short();
        self.ip = unsafe { self.ip.offset(-(offset as isize)) };
    }

    fn jump_finally_impl(&mut self) {
        let return_value = self.peek(0);
        self.active_fiber_mut().return_ip = Some(self.ip);
        self.active_fiber_mut().return_value = return_value;
        self.pop();
        let (new_ip, init_stack_size) = {
            let handler = self
                .active_fiber_mut()
                .pop_exc_handler()
                .expect("Expected ExcHandler.");
            (handler.finally_ip, handler.init_stack_size)
        };
        self.active_fiber_mut().stack.truncate(init_stack_size);
        self.ip = new_ip;
    }

    fn end_finally_impl(&mut self) -> Result<(), Error> {
        if self.handling_exception {
            self.unwind_stack()?;
        }
        let return_data = self.active_fiber_mut().take_return_data();
        if let Some((value, ip)) = return_data {
            self.push(value);
            self.ip = ip;
        }
        Ok(())
    }

    fn push_exc_handler_impl(&mut self) {
        let try_size = self.read_short() as usize;
        let catch_size = self.read_short() as usize;

        let catch_ip = unsafe { self.ip.offset(try_size as isize) };
        let finally_ip = unsafe { self.ip.offset((try_size + catch_size) as isize) };

        self.active_fiber_mut()
            .push_exc_handler(catch_ip, finally_ip);
    }

    fn pop_exc_handler_impl(&mut self) {
        self.active_fiber_mut().pop_exc_handler();
    }

    fn throw_impl(&mut self) -> Result<(), Error> {
        self.handling_exception = true;
        self.active_fiber_mut().error_ip = Some(self.ip);
        self.unwind_stack()
    }

    fn call_impl(&mut self) -> Result<(), Error> {
        let arg_count = self.read_byte() as usize;
        self.call_value(self.peek(arg_count), arg_count)
    }

    fn construct_impl(&mut self) {
        let arg_count = self.read_byte() as usize;
        let value = self.peek(arg_count);
        if let Some(class) = value.try_as_obj_class() {
            let instance = self.new_root_obj_instance(class);
            self.poke(arg_count, Value::ObjInstance(instance.as_gc()));
        }
    }

    fn invoke_impl(&mut self) -> Result<(), Error> {
        let method = self.read_string();
        let arg_count = self.read_byte() as usize;
        self.invoke(method, arg_count)
    }

    fn super_invoke_impl(&mut self) -> Result<(), Error> {
        let method = self.read_string();
        let arg_count = self.read_byte() as usize;
        let superclass = match self.pop() {
            Value::ObjClass(ptr) => ptr,
            _ => unreachable!(),
        };
        self.invoke_from_class(superclass, method, arg_count)
    }

    fn closure_impl(&mut self) {
        let function = match self.read_constant() {
            Value::ObjFunction(underlying) => underlying,
            _ => panic!("Expected ObjFunction."),
        };

        let upvalue_count = function.upvalue_count;

        let closure = self.new_root_obj_closure(function, self.active_module);
        self.push(Value::ObjClosure(closure.as_gc()));

        for i in 0..upvalue_count {
            let is_local = self.read_byte() != 0;
            let index = self.read_byte() as usize;
            let slot_base = self.active_fiber().current_frame().unwrap().slot_base;
            closure.upvalues.borrow_mut()[i] = if is_local {
                self.capture_upvalue(slot_base + index)
            } else {
                self.active_fiber()
                    .current_frame()
                    .unwrap()
                    .closure
                    .upvalues
                    .borrow()[index]
            };
        }
    }

    fn close_upvalue_impl(&mut self) {
        let stack_size = self.stack_size();
        self.active_fiber_mut().close_upvalues(stack_size - 1);
        self.pop();
    }

    fn return_impl(&mut self) -> Result<Option<Value>, Error> {
        let result = self.pop();
        self.active_fiber_mut().close_upvalues_for_frame();

        let prev_stack_size = self.active_fiber().current_frame().unwrap().slot_base;
        self.active_fiber_mut().frames.pop();
        if self.active_fiber().has_finished() {
            if self.active_fiber().caller.is_some() {
                self.unload_fiber(None)?;
                self.poke(0, result);
                return Ok(None);
            }
            return Ok(Some(self.pop()));
        }
        self.load_frame();
        self.active_fiber_mut().stack.truncate(prev_stack_size);
        self.push(result);
        Ok(None)
    }

    fn declare_class_impl(&mut self) {
        let name = self.read_string();
        let metaclass_name = self.new_gc_obj_string(format!("{}Class", *name).as_str());
        let metaclass = UniqueRoot::new(ObjClass::new(
            metaclass_name,
            self.class_store.base_metaclass(),
            Some(self.class_store.object_class()),
            object::new_obj_string_value_map(),
        ));
        let class = UniqueRoot::new(ObjClass::new(
            name,
            self.class_store.base_metaclass(),
            Some(self.class_store.object_class()),
            object::new_obj_string_value_map(),
        ));
        self.working_class_def = Some(ClassDef::new(class, metaclass));
        self.push(Value::None);
    }

    fn define_class_impl(&mut self) {
        let mut class_def = self.working_class_def.take().expect("Expected ClassDef.");

        let defined_metaclass: Root<ObjClass> = class_def.metaclass.into();
        class_def.class.metaclass = defined_metaclass.as_gc();
        let defined_class: Root<ObjClass> = class_def.class.into();

        self.poke(0, Value::ObjClass(defined_class.as_gc()));
    }

    fn inherit_impl(&mut self) -> Result<(), Error> {
        let superclass = if let Some(ptr) = self.peek(1).try_as_obj_class() {
            ptr
        } else {
            let err = error!(ErrorKind::RuntimeError, "Superclass must be a class.");
            return self.try_handle_error(err);
        };
        self.working_class_def.as_mut().unwrap().class.superclass = Some(superclass);
        for (name, method) in &superclass.methods {
            self.working_class_def
                .as_mut()
                .unwrap()
                .class
                .methods
                .insert(*name, *method);
        }
        self.pop();
        Ok(())
    }

    fn method_impl(&mut self) -> Result<(), Error> {
        let name = self.read_string();
        self.define_method(name, false)
    }

    fn static_method_impl(&mut self) -> Result<(), Error> {
        let name = self.read_string();
        self.define_method(name, true)
    }

    fn start_import_impl(&mut self) -> Result<(), Error> {
        let path = self.read_string();

        if let Some(module) = self.modules.get(&path).map(|m| m.as_gc()) {
            if module.borrow().imported {
                self.push(Value::ObjModule(module));
                self.push(Value::None);
            } else {
                let err = error!(
                    ErrorKind::ImportError,
                    "Circular dependency encountered when importing module '{}'.",
                    path.as_str()
                );
                self.try_handle_error(err)?;
            }
            return Ok(());
        }

        let source = match (self.module_loader)(&path) {
            Ok(s) => s,
            Err(e) => {
                return self.try_handle_error(e);
            }
        };

        let function = match compiler::compile(self, source, Some(&path)) {
            Ok(f) => f,
            Err(e) => {
                let mut error = error!(ErrorKind::ImportError, "Error compiling module:");
                for msg in e.messages() {
                    error.add_message(&format!("    {}", msg));
                }
                return self.try_handle_error(error);
            }
        };

        let module = self.module(&path);
        self.push(Value::ObjModule(module));

        let closure = self.new_root_obj_closure(function.as_gc(), module);
        self.push(Value::ObjClosure(closure.as_gc()));

        self.call_value(self.peek(0), 0)?;
        let active_module_path = self.active_module.borrow().path;
        self.init_built_in_globals(&active_module_path);
        Ok(())
    }

    fn finish_import_impl(&mut self) {
        self.pop();
        let module = self
            .peek(0)
            .try_as_obj_module()
            .expect("Expected ObjModule.");
        module.borrow_mut().imported = true;
    }

    #[inline(always)]
    fn call_value(&mut self, value: Value, arg_count: usize) -> Result<(), Error> {
        match value {
            Value::ObjBoundMethod(bound) => {
                self.poke(arg_count, bound.borrow().receiver);
                self.call_closure(bound.borrow().method, arg_count)
            }

            Value::ObjBoundNative(bound) => {
                self.poke(arg_count, bound.borrow().receiver);
                self.call_native(bound.borrow().method, arg_count)
            }

            Value::ObjClosure(function) => self.call_closure(function, arg_count),

            Value::ObjNative(wrapped) => self.call_native(wrapped, arg_count),

            _ => {
                let err = error!(ErrorKind::TypeError, "Can only call functions and methods.");
                self.try_handle_error(err)
            }
        }
    }

    #[inline(always)]
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
        let err = error!(ErrorKind::AttributeError, "Undefined property '{}'.", *name);
        self.try_handle_error(err)
    }

    #[inline(always)]
    fn invoke(&mut self, name: Gc<ObjString>, arg_count: usize) -> Result<(), Error> {
        let receiver = self.peek(arg_count);
        let class = match receiver {
            Value::ObjInstance(instance) => {
                if let Some(value) = instance.borrow().fields.get(&name) {
                    self.poke(arg_count, *value);
                    return self.call_value(*value, arg_count);
                }
                instance.borrow().class
            }
            Value::ObjModule(module) => {
                let global = module.borrow().attributes.get(&name).copied();
                if let Some(value) = global {
                    self.poke(arg_count, value);
                    return self.call_value(value, arg_count);
                }
                module.borrow().class
            }
            _ => self.get_class(receiver),
        };
        self.invoke_from_class(class, name, arg_count)
    }

    #[inline(always)]
    pub fn call_closure(&mut self, closure: Gc<ObjClosure>, arg_count: usize) -> Result<(), Error> {
        let arity = closure.function.arity - 1;
        let err = if arg_count != arity {
            Some(error!(
                ErrorKind::TypeError,
                "Expected {} arguments but found {}.", arity, arg_count
            ))
        } else if self.active_fiber().frames.len() == common::FRAMES_MAX {
            Some(error!(ErrorKind::IndexError, "Stack overflow."))
        } else {
            None
        };

        if let Some(err) = err {
            return self.try_handle_error(err);
        }

        self.active_fiber_mut().current_frame_mut().unwrap().ip = self.ip;
        self.active_fiber_mut().push_call_frame(closure);
        self.load_frame();
        Ok(())
    }

    #[inline(always)]
    fn call_native(&mut self, native: Gc<ObjNative>, arg_count: usize) -> Result<(), Error> {
        let function = native.function;
        let result = function(self, arg_count);
        self.discard(arg_count);
        match result {
            Ok(value) => {
                self.poke(0, value);
            }
            Err(error) => {
                let exc_object = self.new_root_obj_err_from_error(error);
                self.poke(0, Value::ObjInstance(exc_object.as_gc()));
                self.unwind_stack()?;
            }
        }
        Ok(())
    }

    fn unwind_stack(&mut self) -> Result<(), Error> {
        let exc_object = self.peek(0);

        let exc_handler = self.active_fiber_mut().pop_exc_handler();
        let handler = if let Some(h) = exc_handler {
            h
        } else {
            return Err(self.new_error_from_value(exc_object));
        };

        self.active_fiber_mut()
            .stack
            .truncate(handler.init_stack_size);
        self.push(exc_object);
        self.active_fiber_mut().frames.truncate(handler.frame_count);
        self.handling_exception = handler.has_catch_block();
        self.active_fiber_mut().current_frame_mut().unwrap().ip = handler.catch_ip;
        self.load_frame();

        Ok(())
    }

    fn reset_stack(&mut self) {
        if let Some(fiber) = self.fiber.as_ref() {
            let mut borrowed_fiber = fiber.borrow_mut();
            borrowed_fiber.stack.clear();
            borrowed_fiber.frames.clear();
        }
    }

    fn runtime_error(&mut self, error: &mut Error) -> Error {
        let ip = self.ip;
        self.active_fiber_mut().store_error_ip_or(ip);
        for frame in self.active_fiber().frames.iter().rev() {
            let (function, module) = (frame.closure.function, frame.closure.module);

            let mut new_msg = String::new();
            let chunk = frame.closure.function.chunk;
            let instruction = chunk.code_offset(frame.ip) - 1;
            write!(
                new_msg,
                "[{}, line {}] in ",
                *module.borrow(),
                chunk.lines[instruction]
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

    fn define_method(&mut self, name: Gc<ObjString>, is_static: bool) -> Result<(), Error> {
        let method = self.peek(0);
        let class_def = self.working_class_def.as_mut().unwrap();
        class_def.class.methods.insert(name, method);
        if is_static {
            class_def.metaclass.methods.insert(name, method);
        } else {
            class_def.metaclass.methods.remove(&name);
        }
        self.pop();

        Ok(())
    }

    fn bind_method(&mut self, class: Gc<ObjClass>, name: Gc<ObjString>) -> Result<(), Error> {
        let instance = self.peek(0);
        let bound = match class.methods.get(&name) {
            Some(Value::ObjClosure(ptr)) => {
                Value::ObjBoundMethod(self.new_root_obj_bound_method(instance, *ptr).as_gc())
            }
            Some(Value::ObjNative(ptr)) => {
                Value::ObjBoundNative(self.new_root_obj_bound_method(instance, *ptr).as_gc())
            }
            None => {
                let err = error!(ErrorKind::AttributeError, "Undefined property '{}'.", *name);
                return self.try_handle_error(err);
            }
            _ => unreachable!(),
        };
        self.pop();
        self.push(bound);
        Ok(())
    }

    fn capture_upvalue(&mut self, location: usize) -> Gc<RefCell<ObjUpvalue>> {
        let loc_addr = unsafe { self.active_fiber().stack.as_ptr().offset(location as isize) };
        let predicate = |v| v > loc_addr;
        let mut prev_upvalue = None;
        let mut upvalue = self.active_fiber().open_upvalues;

        while upvalue.is_some() && upvalue.unwrap().borrow().is_open_with_pred(predicate) {
            prev_upvalue = upvalue;
            upvalue = upvalue.unwrap().borrow().next;
        }

        if let Some(upvalue) = upvalue {
            if upvalue.borrow().is_open_with_pred(|v| v == loc_addr) {
                return upvalue;
            }
        }

        let created_upvalue = Root::new(RefCell::new(ObjUpvalue::new(loc_addr as *mut _)));
        if let Some(uv) = prev_upvalue {
            uv.borrow_mut().next = Some(created_upvalue.as_gc());
        } else {
            self.active_fiber_mut().open_upvalues = Some(created_upvalue.as_gc());
        }

        created_upvalue.borrow_mut().next = upvalue;
        created_upvalue.as_gc()
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

        let class = self.class_store.range_class();
        let range = Root::new(ObjRange::new(class, begin, end));
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

    fn new_root_obj_err_with_class(
        &mut self,
        class: Gc<ObjClass>,
        context: Value,
    ) -> Root<RefCell<ObjInstance>> {
        let context_string = self.new_gc_obj_string("context");
        let instance = self.new_root_obj_instance(class);
        instance.borrow_mut().fields.insert(context_string, context);
        instance
    }

    fn new_root_obj_err_from_error(&mut self, error: Error) -> Root<RefCell<ObjInstance>> {
        let msg = self.new_gc_obj_string(&error.messages().join("\n"));
        let class = match error.kind() {
            ErrorKind::AttributeError => self.class_store.attribute_error_class(),
            ErrorKind::CompileError => self.class_store.runtime_error_class(),
            ErrorKind::ImportError => self.class_store.import_error_class(),
            ErrorKind::IndexError => self.class_store.index_error_class(),
            ErrorKind::NameError => self.class_store.name_error_class(),
            ErrorKind::RuntimeError => self.class_store.runtime_error_class(),
            ErrorKind::TypeError => self.class_store.type_error_class(),
            ErrorKind::ValueError => self.class_store.value_error_class(),
        };

        self.new_root_obj_err_with_class(class, Value::ObjString(msg))
    }

    fn new_error_from_value(&mut self, value: Value) -> Error {
        let (kind, exc_description, context) = if let Some(instance) = value.try_as_obj_instance() {
            let class = instance.borrow().class;
            let kind = if class == self.class_store.attribute_error_class() {
                ErrorKind::AttributeError
            } else if class == self.class_store.runtime_error_class() {
                ErrorKind::CompileError
            } else if class == self.class_store.import_error_class() {
                ErrorKind::ImportError
            } else if class == self.class_store.index_error_class() {
                ErrorKind::IndexError
            } else if class == self.class_store.name_error_class() {
                ErrorKind::NameError
            } else if class == self.class_store.runtime_error_class() {
                ErrorKind::RuntimeError
            } else if class == self.class_store.type_error_class() {
                ErrorKind::TypeError
            } else if class == self.class_store.value_error_class() {
                ErrorKind::ValueError
            } else {
                ErrorKind::RuntimeError
            };
            let context_string = self.new_gc_obj_string("context");
            let borrowed_instance = instance.borrow();
            let context = borrowed_instance
                .fields
                .get(&context_string)
                .map(|&v| v)
                .unwrap_or(value);
            (kind, class.name.as_str().to_owned(), context)
        } else {
            (ErrorKind::RuntimeError, "exception".to_owned(), value)
        };

        let msg = format!("Unhandled {}: {}", exc_description, context);
        let lines = msg.lines().collect::<Vec<_>>();

        Error::with_messages(kind, &lines)
    }

    fn try_handle_error(&mut self, error: Error) -> Result<(), Error> {
        let obj_err = self.new_root_obj_err_from_error(error);
        self.push(Value::ObjInstance(obj_err.as_gc()));
        self.unwind_stack()
    }

    fn build_hash_map(&mut self, num_elements: usize) -> Result<Root<RefCell<ObjHashMap>>, Error> {
        let map = self.new_root_obj_hash_map();
        let begin = self.stack_size() - num_elements * 2;
        for i in 0..num_elements {
            let key = self.active_fiber().stack[begin + 2 * i];
            if !key.has_hash() {
                return Err(error!(
                    ErrorKind::ValueError,
                    "Cannot use unhashable value '{}' as HashMap key.", key
                ));
            }
            let value = self.active_fiber().stack[begin + 2 * i + 1];
            map.borrow_mut().elements.insert(key, value);
        }
        self.discard(num_elements * 2);
        Ok(map)
    }

    fn init_heap_allocated_data(&mut self) {
        let mut root_base_metaclass = unsafe { core::new_base_metaclass() };
        let mut root_object_class = Root::new(ObjClass {
            name: Gc::dangling(),
            metaclass: root_base_metaclass.as_gc(),
            superclass: None,
            methods: object::new_obj_string_value_map(),
        });
        let mut root_string_metaclass = Root::new(ObjClass::new(
            Gc::dangling(),
            root_base_metaclass.as_gc(),
            Some(root_object_class.as_gc()),
            object::new_obj_string_value_map(),
        ));
        let mut string_class = Root::new(ObjClass::new(
            Gc::dangling(),
            root_string_metaclass.as_gc(),
            Some(root_object_class.as_gc()),
            object::new_obj_string_value_map(),
        ));

        self.string_class = Some(string_class.clone());
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
            root_object_class.as_mut().name = object_class_name;
            root_base_metaclass.as_mut().name = base_metaclass_name;
            root_base_metaclass.as_mut().superclass = Some(root_object_class.as_gc());
            root_string_metaclass.as_mut().name = string_metaclass_name;
            string_class.as_mut().name = string_class_name;
            core::bind_object_class(self, &mut root_object_class);
            core::bind_type_class(self, &mut root_base_metaclass);
            core::bind_gc_obj_string_class(self, &mut string_class, &mut root_string_metaclass);
        }

        let empty_chunk = Root::new(Chunk::new()).as_gc();
        let next_string = self.new_gc_obj_string("next");
        self.active_chunk = empty_chunk;
        self.next_string = next_string;
        let class_store =
            CoreClassStore::new(self, root_base_metaclass.clone(), root_object_class.clone());
        self.class_store = class_store;
        let class_store =
            CoreClassStore::new_with_built_ins(self, root_base_metaclass, root_object_class);
        self.core_chunks = self.chunks.clone();
        self.class_store = class_store;
    }

    fn init_built_in_globals(&mut self, module_path: &str) {
        self.define_native(module_path, "clock", core::clock);
        self.define_native(module_path, "type", core::type_);
        self.define_native(module_path, "print", self.printer);
        let base_metaclass = self.class_store.base_metaclass();
        self.set_global(module_path, "Type", Value::ObjClass(base_metaclass));
        let object_class = self.class_store.object_class();
        self.set_global(module_path, "Object", Value::ObjClass(object_class));
        let nil_class = self.class_store.nil_class();
        self.set_global(module_path, "Nil", Value::ObjClass(nil_class));
        let boolean_class = self.class_store.boolean_class();
        self.set_global(module_path, "Bool", Value::ObjClass(boolean_class));
        let number_class = self.class_store.num_class();
        self.set_global(module_path, "Num", Value::ObjClass(number_class));
        let obj_closure_class = self.class_store.closure_class();
        self.set_global(module_path, "Func", Value::ObjClass(obj_closure_class));
        let obj_native_class = self.class_store.native_class();
        self.set_global(module_path, "BuiltIn", Value::ObjClass(obj_native_class));
        let obj_closure_method_class = self.class_store.closure_method_class();
        self.set_global(
            module_path,
            "Method",
            Value::ObjClass(obj_closure_method_class),
        );
        let obj_native_method_class = self.class_store.native_method_class();
        self.set_global(
            module_path,
            "BuiltInMethod",
            Value::ObjClass(obj_native_method_class),
        );
        let obj_string_class = self.string_class.as_ref().expect("Expected Root.").as_gc();
        self.set_global(module_path, "String", Value::ObjClass(obj_string_class));
        let obj_iter_class = self.class_store.iter_class();
        self.set_global(module_path, "Iter", Value::ObjClass(obj_iter_class));
        let obj_map_iter_class = self.class_store.map_iter_class();
        self.set_global(module_path, "MapIter", Value::ObjClass(obj_map_iter_class));
        let obj_filter_iter_class = self.class_store.filter_iter_class();
        self.set_global(
            module_path,
            "FilterIter",
            Value::ObjClass(obj_filter_iter_class),
        );
        let obj_tuple_class = self.class_store.tuple_class();
        self.set_global(module_path, "Tuple", Value::ObjClass(obj_tuple_class));
        let obj_vec_class = self.class_store.vec_class();
        self.set_global(module_path, "Vec", Value::ObjClass(obj_vec_class));
        let obj_range_class = self.class_store.range_class();
        self.set_global(module_path, "Range", Value::ObjClass(obj_range_class));
        let obj_hash_map_class = self.class_store.hash_map_class();
        self.set_global(module_path, "HashMap", Value::ObjClass(obj_hash_map_class));
        let obj_fiber_class = self.class_store.fiber_class();
        self.set_global(module_path, "Fiber", Value::ObjClass(obj_fiber_class));
    }

    fn load_frame(&mut self) {
        let (prev_chunk, prev_module, new_ip) = {
            let active_fiber = self.active_fiber();
            let current_frame = active_fiber.current_frame().unwrap();
            (
                current_frame.closure.function.chunk,
                current_frame.closure.module,
                current_frame.ip,
            )
        };
        self.active_chunk = prev_chunk;
        self.active_module = prev_module;
        self.ip = new_ip;
    }

    #[cfg(any(debug_assertions, feature = "safe_active_fiber"))]
    fn active_fiber(&self) -> Ref<'_, ObjFiber> {
        self.fiber.as_ref().unwrap().borrow()
    }

    #[cfg(not(any(debug_assertions, feature = "safe_active_fiber")))]
    fn active_fiber(&self) -> &ObjFiber {
        unsafe { &*(self.unsafe_fiber as *const _) }
    }

    #[cfg(any(debug_assertions, feature = "safe_active_fiber"))]
    fn active_fiber_mut(&mut self) -> RefMut<'_, ObjFiber> {
        self.fiber.as_ref().unwrap().borrow_mut()
    }

    #[cfg(not(any(debug_assertions, feature = "safe_active_fiber")))]
    fn active_fiber_mut(&mut self) -> &mut ObjFiber {
        unsafe { &mut (*self.unsafe_fiber) }
    }

    fn stack_size(&self) -> usize {
        self.active_fiber().stack.len()
    }

    fn poke(&mut self, depth: usize, value: Value) {
        *self.active_fiber_mut().stack.peek_mut(depth) = value;
    }

    fn discard(&mut self, num: usize) {
        let stack_len = self.active_fiber_mut().stack.len();
        self.active_fiber_mut().stack.truncate(stack_len - num);
    }
}

mod string_store {
    use std::mem;

    use crate::memory::Root;
    use crate::object::ObjString;

    const INIT_CAPACITY: usize = 4;
    const MAX_LOAD: f64 = 0.75;

    // We want somewhere to intern our heap-allocated strings, and the built-in HashSet
    // unfortunately doesn't meet our requirements. We need fine-grained hash control, since we're
    // using a custom hash algorithm along with caching of hash on the stored ObjString, meaning the
    // &str objects we use for look-up and the ObjString objects we store have different
    // implementations of Hash.
    pub(super) struct ObjStringStore {
        entries: Vec<Option<Root<ObjString>>>,
        size: usize,
        mask: usize,
    }

    impl ObjStringStore {
        pub(super) fn new() -> Self {
            Default::default()
        }

        pub(super) fn get(&self, key: (u64, &str)) -> Option<&Root<ObjString>> {
            let entry = &self.entries[find_index(&self.entries, key, self.mask)];
            if entry.is_some() {
                entry.as_ref()
            } else {
                None
            }
        }

        pub(super) fn insert(&mut self, value: Root<ObjString>) -> Option<Root<ObjString>> {
            if self.size + 1 > (self.entries.len() as f64 * MAX_LOAD) as usize {
                self.adjust_capacity(self.entries.len() * 2);
            }

            let key = (value.hash, value.as_str());
            let index = find_index(&self.entries, key, self.mask);
            let entry = &mut self.entries[index];

            let is_new_key = entry.is_none();
            if is_new_key {
                self.size += 1;
            }
            entry.replace(value)
        }

        fn adjust_capacity(&mut self, new_capacity: usize) {
            let mut new_entries: Vec<Option<Root<ObjString>>> = vec![None; new_capacity];
            let mask = new_capacity - 1;

            for entry in self.entries.iter_mut() {
                if entry.is_none() {
                    continue;
                }

                let key = {
                    let entry = entry.as_ref().unwrap();
                    (entry.hash, entry.as_str())
                };
                let index = find_index(&new_entries, key, mask);
                let dest = &mut new_entries[index];
                *dest = mem::take(entry);
            }

            self.entries = new_entries;
            self.mask = mask;
        }
    }

    fn find_index(entries: &Vec<Option<Root<ObjString>>>, key: (u64, &str), mask: usize) -> usize {
        let (hash, string) = key;
        let mut index = (hash as usize) & mask;

        loop {
            match entries[index].as_ref() {
                Some(entry) => {
                    if entry.hash == hash && entry.as_str() == string {
                        return index;
                    }
                }
                None => {
                    return index;
                }
            }

            index = (index + 1) & mask;
        }
    }

    impl Default for ObjStringStore {
        fn default() -> Self {
            ObjStringStore {
                entries: vec![Default::default(); INIT_CAPACITY],
                size: 0,
                mask: INIT_CAPACITY - 1,
            }
        }
    }
}
