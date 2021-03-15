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

use std::cell::RefCell;
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
use crate::class_store::{self, CoreClassStore};
use crate::common;
use crate::compiler;
use crate::core;
use crate::debug;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, FnvHasher};
use crate::memory::{self, Gc, GcBoxPtr, GcManaged, Heap, Root};
use crate::object::{
    self, NativeFn, ObjBoundMethod, ObjClass, ObjClosure, ObjFiber, ObjFunction, ObjHashMap,
    ObjInstance, ObjModule, ObjNative, ObjRange, ObjRangeIter, ObjString, ObjStringIter,
    ObjStringValueMap, ObjTuple, ObjTupleIter, ObjUpvalue, ObjVec, ObjVecIter,
};
#[allow(unused_imports)]
use crate::unsafe_ref_cell::{Ref, RefMut, UnsafeRefCell};
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
                crate::error::ErrorKind::RuntimeError,
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
    fiber: Option<Gc<UnsafeRefCell<ObjFiber>>>,
    init_string: Gc<ObjString>,
    next_string: Gc<ObjString>,
    pub(crate) class_store: CoreClassStore,
    chunks: Vec<Root<Chunk>>,
    modules: HashMap<Gc<ObjString>, Root<RefCell<ObjModule>>, BuildPassThroughHasher>,
    core_chunks: Vec<Root<Chunk>>,
    string_class: Root<ObjClass>,
    string_store: string_store::ObjStringStore,
    range_cache: Vec<(Root<ObjRange>, time::Instant)>,
    working_class_def: Option<ClassDef>,
    module_loader: LoadModuleFn,
    printer: NativeFn,
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
            active_module: Gc::null(),
            active_chunk: Gc::null(),
            fiber: None,
            init_string: Gc::null(),
            next_string: Gc::null(),
            class_store: unsafe { CoreClassStore::new_empty() },
            chunks: Vec::new(),
            modules: HashMap::with_hasher(BuildPassThroughHasher::default()),
            core_chunks: Vec::new(),
            string_class: Root::null(),
            string_store: string_store::ObjStringStore::new(), // HashMap::with_hasher(BuildPassThroughHasher::default()),
            heap,
            range_cache: Vec::with_capacity(RANGE_CACHE_SIZE),
            module_loader: default_read_module_source,
            printer: core::print,
            working_class_def: None,
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
        let module = self.get_module(&function.module_path);
        let closure = self.new_root_obj_closure(function.as_gc(), module);
        let fiber = self.new_root_obj_fiber(closure.as_gc());
        if closure.borrow().function.arity != args.len() as u32 + 1 {
            return Err(error!(
                ErrorKind::TypeError,
                "Expected {} arguments but found {}.",
                closure.borrow().function.arity - 1,
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

    pub fn get_global(&mut self, module_name: &str, var_name: &str) -> Option<Value> {
        let var_name = self.new_gc_obj_string(var_name);
        self.get_module(module_name)
            .borrow()
            .attributes
            .get(&var_name)
            .copied()
    }

    pub fn set_global(&mut self, module_name: &str, var_name: &str, value: Value) {
        let var_name = self.new_gc_obj_string(var_name);
        self.get_module(module_name)
            .borrow_mut()
            .attributes
            .insert(var_name, value);
    }

    pub fn define_native(&mut self, module_name: &str, var_name: &str, function: NativeFn) {
        let var_name = self.new_gc_obj_string(var_name);
        let native = self.new_root_obj_native(var_name, function);
        self.get_module(module_name)
            .borrow_mut()
            .attributes
            .insert(var_name, Value::ObjNative(native.as_gc()));
    }

    pub fn get_class(&self, value: Value) -> Gc<ObjClass> {
        value.get_class(&self.class_store)
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
        let string = self.allocate_root(ObjString::new(self.string_class.as_gc(), data, hash));
        let ret = string.as_gc();
        self.string_store.insert(string);
        ret
    }

    pub fn new_root_obj_upvalue(&mut self, value: &mut Value) -> Root<RefCell<ObjUpvalue>> {
        self.allocate_root(RefCell::new(ObjUpvalue::new(value)))
    }

    pub fn new_root_obj_function(
        &mut self,
        name: Gc<ObjString>,
        arity: u32,
        upvalue_count: usize,
        chunk: Gc<Chunk>,
        module_path: Gc<ObjString>,
    ) -> Root<ObjFunction> {
        self.allocate_root(ObjFunction::new(
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
        self.allocate_root(ObjNative::new(name, function))
    }

    pub fn new_root_obj_closure(
        &mut self,
        function: Gc<ObjFunction>,
        module: Gc<RefCell<ObjModule>>,
    ) -> Root<RefCell<ObjClosure>> {
        let upvalue_roots: Vec<Root<RefCell<ObjUpvalue>>> = (0..function.upvalue_count)
            .map(|_| self.allocate_root(RefCell::new(ObjUpvalue::new(ptr::null_mut()))))
            .collect();
        let upvalues = upvalue_roots.iter().map(|u| u.as_gc()).collect();
        self.allocate_root(RefCell::new(ObjClosure::new(function, upvalues, module)))
    }

    pub fn new_root_obj_class(
        &mut self,
        name: Gc<ObjString>,
        metaclass: Gc<ObjClass>,
        superclass: Option<Gc<ObjClass>>,
        methods: ObjStringValueMap,
    ) -> Root<ObjClass> {
        let mut merged_methods = if let Some(parent) = superclass {
            parent.methods.clone()
        } else {
            object::new_obj_string_value_map()
        };
        for (&k, &v) in &methods {
            merged_methods.insert(k, v);
        }
        self.allocate_root(ObjClass::new(name, metaclass, superclass, merged_methods))
    }

    pub fn new_root_obj_instance(&mut self, class: Gc<ObjClass>) -> Root<RefCell<ObjInstance>> {
        self.allocate_root(RefCell::new(ObjInstance::new(class)))
    }

    pub fn new_root_obj_bound_method<T: 'static + memory::GcManaged>(
        &mut self,
        receiver: Value,
        method: Gc<T>,
    ) -> Root<RefCell<ObjBoundMethod<T>>> {
        self.allocate_root(RefCell::new(ObjBoundMethod::new(receiver, method)))
    }

    pub fn new_root_obj_string_iter(
        &mut self,
        string: Gc<ObjString>,
    ) -> Root<RefCell<ObjStringIter>> {
        let class = self.class_store.get_obj_string_iter_class();
        self.allocate_root(RefCell::new(ObjStringIter::new(class, string)))
    }

    pub fn new_root_obj_hash_map(&mut self) -> Root<RefCell<ObjHashMap>> {
        let class = self.class_store.get_obj_hash_map_class();
        self.allocate_root(RefCell::new(ObjHashMap::new(class)))
    }

    pub fn new_root_obj_range(&mut self, begin: isize, end: isize) -> Root<ObjRange> {
        self.build_range(begin, end).as_root()
    }

    pub fn new_root_obj_range_iter(&mut self, range: Gc<ObjRange>) -> Root<RefCell<ObjRangeIter>> {
        let class = self.class_store.get_obj_range_iter_class();
        self.allocate_root(RefCell::new(ObjRangeIter::new(class, range)))
    }

    pub fn new_root_obj_tuple(&mut self, elements: Vec<Value>) -> Root<ObjTuple> {
        let class = self.class_store.get_obj_tuple_class();
        self.allocate_root(ObjTuple::new(class, elements))
    }

    pub fn new_root_obj_tuple_iter(&mut self, tuple: Gc<ObjTuple>) -> Root<RefCell<ObjTupleIter>> {
        let class = self.class_store.get_obj_tuple_iter_class();
        self.allocate_root(RefCell::new(ObjTupleIter::new(class, tuple)))
    }

    pub fn new_root_obj_vec(&mut self) -> Root<RefCell<ObjVec>> {
        let class = self.class_store.get_obj_vec_class();
        self.allocate_root(RefCell::new(ObjVec::new(class)))
    }

    pub fn new_root_obj_vec_iter(&mut self, vec: Gc<RefCell<ObjVec>>) -> Root<RefCell<ObjVecIter>> {
        let class = self.class_store.get_obj_vec_iter_class();
        self.allocate_root(RefCell::new(ObjVecIter::new(class, vec)))
    }

    pub fn new_root_obj_module(
        &mut self,
        class: Gc<ObjClass>,
        path: Gc<ObjString>,
    ) -> Root<RefCell<ObjModule>> {
        self.allocate_root(RefCell::new(ObjModule::new(class, path)))
    }

    pub(crate) fn new_root_obj_fiber(
        &mut self,
        closure: Gc<RefCell<ObjClosure>>,
    ) -> Root<UnsafeRefCell<ObjFiber>> {
        let class = self.class_store.get_obj_fiber_class();
        self.allocate_root(UnsafeRefCell::new(ObjFiber::new(class, closure)))
    }

    pub fn reset(&mut self) {
        self.reset_stack();
        self.chunks = self.core_chunks.clone();
        self.modules.retain(|&k, _| k.as_str() == "main");
        self.active_module = self.get_module("main");
        self.active_module.borrow_mut().attributes = object::new_obj_string_value_map();
        self.init_built_in_globals("main");
    }

    pub(crate) fn get_module(&mut self, path: &str) -> Gc<RefCell<ObjModule>> {
        let path = self.new_gc_obj_string(path);
        if let Some(module) = self.modules.get(&path) {
            return module.as_gc();
        }
        let module = self.allocate_root(RefCell::new(ObjModule::new(
            self.class_store.get_obj_module_class(),
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
        let root = self.allocate_root(chunk);
        let ret = root.as_gc();
        self.chunks.push(root);
        ret
    }

    pub(crate) fn allocate_bare<T: 'static + GcManaged>(&mut self, data: T) -> GcBoxPtr<T> {
        let mut roots: Vec<&dyn GcManaged> = Vec::new();
        if let Some(def) = self.working_class_def.as_ref() {
            roots.push(def);
        }
        if let Some(fiber) = self.fiber.as_ref() {
            roots.push(fiber);
        }
        self.heap.allocate_bare(&roots, data)
    }

    pub(crate) fn allocate<T: 'static + GcManaged>(&mut self, data: T) -> Gc<T> {
        self.allocate_root(data).as_gc()
    }

    pub(crate) fn allocate_root<T: 'static + GcManaged>(&mut self, data: T) -> Root<T> {
        let mut roots: Vec<&dyn GcManaged> = Vec::new();
        if let Some(def) = self.working_class_def.as_ref() {
            roots.push(def);
        }
        if let Some(fiber) = self.fiber.as_ref() {
            roots.push(fiber);
        }
        self.heap.allocate_root(&roots, data)
    }

    pub(crate) fn load_fiber(
        &mut self,
        fiber: Gc<UnsafeRefCell<ObjFiber>>,
        arg: Option<Value>,
    ) -> Result<(), Error> {
        if unsafe { &*fiber.get() }.has_finished() {
            return Err(error!(
                ErrorKind::RuntimeError,
                "Cannot call a finished fiber."
            ));
        }
        if self.fiber.is_some() {
            self.active_fiber_mut().current_frame_mut().unwrap().ip = self.ip;
        }

        let caller = self.fiber.replace(fiber);
        self.active_fiber_mut().caller = caller;

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
            self.fiber.replace(caller);
        } else {
            self.fiber = None;
            return Ok(());
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
                println!("          {}", self.active_fiber().stack);
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
                    let top = self.peek(0);
                    self.push(top);
                }

                byte if byte == OpCode::GetLocal as u8 => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.active_fiber().current_frame().unwrap().slot_base;
                    let value = self.active_fiber().stack[slot_base + slot];
                    self.push(value);
                }

                byte if byte == OpCode::SetLocal as u8 => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.active_fiber().current_frame().unwrap().slot_base;
                    self.active_fiber_mut().stack[slot_base + slot] = self.peek(0);
                }

                byte if byte == OpCode::GetGlobal as u8 => {
                    let name = read_string!();
                    let value = match self.active_module.borrow().attributes.get(&name) {
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
                    let value = self.peek(0);
                    self.active_module
                        .borrow_mut()
                        .attributes
                        .insert(name, value);
                    self.pop();
                }

                byte if byte == OpCode::SetGlobal as u8 => {
                    let name = read_string!();
                    let value = self.peek(0);
                    let globals = &mut self.active_module.borrow_mut().attributes;
                    let prev = globals.insert(name, value);
                    if prev.is_none() {
                        globals.remove(&name);
                        return Err(error!(
                            ErrorKind::RuntimeError,
                            "Undefined variable '{}'.", *name
                        ));
                    }
                }

                byte if byte == OpCode::GetUpvalue as u8 => {
                    let upvalue_index = read_byte!() as usize;
                    let upvalue = self
                        .active_fiber()
                        .current_frame()
                        .unwrap()
                        .closure
                        .borrow()
                        .upvalues[upvalue_index]
                        .borrow()
                        .get();
                    self.push(upvalue);
                }

                byte if byte == OpCode::SetUpvalue as u8 => {
                    let upvalue_index = read_byte!() as usize;
                    let stack_value = self.peek(0);
                    let closure = self.active_fiber().current_frame().unwrap().closure;
                    closure.borrow_mut().upvalues[upvalue_index]
                        .borrow_mut()
                        .set(stack_value);
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
                    if let Some(module) = self.peek(0).try_as_obj_module() {
                        if let Some(&property) = module.borrow().attributes.get(&name) {
                            self.pop();
                            self.push(property);
                            continue;
                        }
                    }

                    let class = self.peek(0).get_class(&self.class_store);
                    self.bind_method(class, name)?;
                }

                byte if byte == OpCode::SetProperty as u8 => {
                    if let Some(module) = self.peek(1).try_as_obj_module() {
                        let name = read_string!();
                        let value = self.peek(0);
                        module.borrow_mut().attributes.insert(name, value);
                        self.pop();
                        self.pop();
                        self.push(value);
                        continue;
                    }
                    let instance = if let Some(ptr) = self.peek(1).try_as_obj_instance() {
                        ptr
                    } else {
                        return Err(error!(
                            ErrorKind::RuntimeError,
                            "Only instances have fields."
                        ));
                    };
                    let name = read_string!();
                    let value = self.peek(0);
                    instance.borrow_mut().fields.insert(name, value);

                    self.pop();
                    self.pop();
                    self.push(value);
                }

                byte if byte == OpCode::GetClass as u8 => {
                    let value = self.peek(0);
                    match value {
                        Value::ObjClass(_) => continue,
                        Value::ObjInstance(instance) => {
                            self.poke(0, Value::ObjClass(instance.borrow().class));
                        }
                        _ => {
                            let class = value.get_class(&self.class_store);
                            self.poke(0, Value::ObjClass(class));
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
                            self.push(value)
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
                    let value = self.peek(0);
                    if value.try_as_obj_string().is_some() {
                        continue;
                    }
                    let obj =
                        Value::ObjString(self.new_gc_obj_string(format!("{}", value).as_str()));
                    self.poke(0, obj);
                }

                byte if byte == OpCode::BuildHashMap as u8 => {
                    let num_elements = read_byte!() as usize;
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
                    self.push(Value::ObjHashMap(map.as_gc()));
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
                    self.discard(num_operands);
                    let value = Value::ObjString(self.new_gc_obj_string(new_string.as_str()));
                    self.push(value);
                }

                byte if byte == OpCode::BuildTuple as u8 => {
                    let num_operands = read_byte!() as usize;
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

                byte if byte == OpCode::BuildVec as u8 => {
                    let num_operands = read_byte!() as usize;
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

                byte if byte == OpCode::IterNext as u8 => {
                    let iter = self.peek(0);
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
                    self.call_value(self.peek(arg_count), arg_count)?;
                }

                byte if byte == OpCode::Construct as u8 => {
                    let arg_count = read_byte!() as usize;
                    let value = self.peek(arg_count);
                    if let Some(class) = value.try_as_obj_class() {
                        let instance = self.new_root_obj_instance(class);
                        self.poke(arg_count, Value::ObjInstance(instance.as_gc()));
                    }
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

                    let closure = self.new_root_obj_closure(function, self.active_module);
                    self.push(Value::ObjClosure(closure.as_gc()));

                    for i in 0..upvalue_count {
                        let is_local = read_byte!() != 0;
                        let index = read_byte!() as usize;
                        let slot_base = self.active_fiber().current_frame().unwrap().slot_base;
                        closure.borrow_mut().upvalues[i] = if is_local {
                            self.capture_upvalue(slot_base + index)
                        } else {
                            self.active_fiber()
                                .current_frame()
                                .unwrap()
                                .closure
                                .borrow()
                                .upvalues[index]
                        };
                    }
                }

                byte if byte == OpCode::CloseUpvalue as u8 => {
                    let stack_size = self.stack_size();
                    self.active_fiber_mut().close_upvalues(stack_size - 1);
                    self.pop();
                }

                byte if byte == OpCode::Return as u8 => {
                    let result = self.pop();
                    let stack_top = self.stack_size();
                    let slot_base = self.active_fiber().current_frame().unwrap().slot_base;
                    for i in slot_base..stack_top {
                        self.active_fiber_mut().close_upvalues(i);
                    }

                    let prev_stack_size = self.active_fiber().current_frame().unwrap().slot_base;
                    self.active_fiber_mut().frames.pop();
                    if self.active_fiber().has_finished() {
                        if self.active_fiber().caller.is_some() {
                            self.unload_fiber(None)?;
                            self.poke(0, result);
                            continue;
                        }
                        return Ok(self.pop());
                    }
                    self.load_frame();
                    self.active_fiber_mut().stack.truncate(prev_stack_size);
                    self.push(result);
                }

                byte if byte == OpCode::DeclareClass as u8 => {
                    let name = read_string!();
                    let metaclass_name = self.new_gc_obj_string(format!("{}Class", *name).as_str());
                    let superclass = self.class_store.get_object_class();
                    self.working_class_def = Some(ClassDef::new(name, metaclass_name, superclass));
                    let class = self.new_root_obj_class(
                        name,
                        self.class_store.get_base_metaclass(),
                        None,
                        object::new_obj_string_value_map(),
                    );
                    self.push(Value::ObjClass(class.as_gc()));
                }

                byte if byte == OpCode::DefineClass as u8 => {
                    let class_def = self.working_class_def.as_ref().expect("Expected ClassDef.");

                    let base_metaclass = self.class_store.get_base_metaclass();
                    let name = class_def.name;
                    let metaclass_name = class_def.metaclass_name;
                    let methods = class_def.methods.clone();
                    let static_methods = class_def.static_methods.clone();
                    let superclass = class_def.superclass;
                    let metaclass = self.new_root_obj_class(
                        metaclass_name,
                        base_metaclass,
                        Some(self.class_store.get_object_class()),
                        static_methods,
                    );

                    let defined_class =
                        self.new_root_obj_class(name, metaclass.as_gc(), Some(superclass), methods);
                    self.working_class_def = None;
                    self.poke(0, Value::ObjClass(defined_class.as_gc()));
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

                byte if byte == OpCode::StartImport as u8 => {
                    let path = read_string!();

                    if let Some(module) = self.modules.get(&path).map(|m| m.as_gc()) {
                        if module.borrow().imported {
                            self.push(Value::ObjModule(module));
                            self.push(Value::None);
                            continue;
                        } else {
                            return Err(error!(
                                ErrorKind::RuntimeError,
                                "Circular dependency encountered when importing module '{}'.",
                                path.as_str()
                            ));
                        }
                    }

                    let source = (self.module_loader)(&path)?;

                    let function = match compiler::compile(self, source, Some(&path)) {
                        Ok(f) => f,
                        Err(e) => {
                            let mut error =
                                error!(ErrorKind::RuntimeError, "Error compiling module:");
                            for msg in e.get_messages() {
                                error.add_message(&format!("    {}", msg));
                            }
                            return Err(error);
                        }
                    };

                    let module = self.get_module(&path);
                    self.push(Value::ObjModule(module));

                    let closure = self.new_root_obj_closure(function.as_gc(), module);
                    self.push(Value::ObjClosure(closure.as_gc()));

                    self.call_value(self.peek(0), 0)?;
                    let active_module_path = self.active_module.borrow().path;
                    self.init_built_in_globals(&active_module_path);
                }

                byte if byte == OpCode::FinishImport as u8 => {
                    self.pop();
                    let module = self
                        .peek(0)
                        .try_as_obj_module()
                        .expect("Expected ObjModule.");
                    module.borrow_mut().imported = true;
                }

                _ => {
                    if cfg!(any(debug_assertions, feature = "more_vm_safety")) {
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

            _ => Err(error!(
                ErrorKind::TypeError,
                "Can only call functions and methods."
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
        let receiver = self.peek(arg_count);
        match receiver {
            Value::ObjInstance(instance) => {
                if let Some(value) = instance.borrow().fields.get(&name) {
                    self.poke(arg_count, *value);
                    return self.call_value(*value, arg_count);
                }

                self.invoke_from_class(instance.borrow().class, name, arg_count)
            }
            Value::ObjTuple(tuple) => {
                let class = tuple.class;
                self.invoke_from_class(class, name, arg_count)
            }
            Value::ObjTupleIter(iter) => {
                let class = iter.borrow().class;
                self.invoke_from_class(class, name, arg_count)
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
            Value::ObjHashMap(map) => {
                let class = map.borrow().class;
                self.invoke_from_class(class, name, arg_count)
            }
            Value::ObjString(string) => self.invoke_from_class(string.class, name, arg_count),
            Value::ObjStringIter(iter) => {
                let class = iter.borrow().class;
                self.invoke_from_class(class, name, arg_count)
            }
            Value::ObjClass(class) => self.invoke_from_class(class.metaclass, name, arg_count),
            Value::Boolean(_) => {
                self.invoke_from_class(self.class_store.get_boolean_class(), name, arg_count)
            }
            Value::Number(_) => {
                self.invoke_from_class(self.class_store.get_number_class(), name, arg_count)
            }
            Value::ObjFunction(_) => {
                self.invoke_from_class(self.class_store.get_obj_closure_class(), name, arg_count)
            }
            Value::ObjNative(_) => {
                self.invoke_from_class(self.class_store.get_obj_native_class(), name, arg_count)
            }
            Value::ObjClosure(_) => {
                self.invoke_from_class(self.class_store.get_obj_closure_class(), name, arg_count)
            }
            Value::ObjBoundMethod(_) => self.invoke_from_class(
                self.class_store.get_obj_closure_method_class(),
                name,
                arg_count,
            ),
            Value::ObjBoundNative(_) => self.invoke_from_class(
                self.class_store.get_obj_native_method_class(),
                name,
                arg_count,
            ),
            Value::ObjModule(module) => {
                let global = module.borrow().attributes.get(&name).copied();
                if let Some(value) = global {
                    self.poke(arg_count, value);
                    return self.call_value(value, arg_count);
                }
                self.invoke_from_class(module.borrow().class, name, arg_count)
            }
            Value::ObjFiber(fiber) => {
                let class = unsafe { &*fiber.get() }.class;
                self.invoke_from_class(class, name, arg_count)
            }
            Value::None => {
                self.invoke_from_class(self.class_store.get_nil_class(), name, arg_count)
            }
            Value::Sentinel => {
                self.invoke_from_class(self.class_store.get_sentinel_class(), name, arg_count)
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

        if self.active_fiber().frames.len() == common::FRAMES_MAX {
            return Err(error!(ErrorKind::IndexError, "Stack overflow."));
        }

        self.active_fiber_mut().current_frame_mut().unwrap().ip = self.ip;
        self.active_fiber_mut().push_call_frame(closure);
        self.load_frame();
        Ok(())
    }

    fn call_native(&mut self, native: Gc<ObjNative>, arg_count: usize) -> Result<(), Error> {
        let function = native.function;
        let result = function(self, arg_count)?;
        self.discard(arg_count);
        self.poke(0, result);
        Ok(())
    }

    fn reset_stack(&mut self) {
        if let Some(fiber) = self.fiber {
            let mut borrowed_fiber = fiber.borrow_mut();
            borrowed_fiber.stack.clear();
            borrowed_fiber.frames.clear();
        }
    }

    fn runtime_error(&mut self, error: &mut Error) -> Error {
        self.active_fiber_mut().current_frame_mut().unwrap().ip = self.ip;
        for frame in self.active_fiber().frames.iter().rev() {
            let (function, module) = {
                let borrowed_closure = frame.closure.borrow();
                (borrowed_closure.function, borrowed_closure.module)
            };

            let mut new_msg = String::new();
            let chunk = frame.closure.borrow().function.chunk;
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
        class_def.methods.insert(name, method);
        if is_static {
            class_def.static_methods.insert(name, method);
        } else {
            class_def.static_methods.remove(&name);
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
        let result = {
            let value = &self.active_fiber().stack[location];
            self.active_fiber()
                .open_upvalues
                .iter()
                .find(|&u| u.borrow().is_open_with_value(value))
                .copied()
        };

        let upvalue = if let Some(upvalue) = result {
            upvalue
        } else {
            let upvalue = ObjUpvalue::new(&mut self.active_fiber_mut().stack[location]);
            self.allocate(RefCell::new(upvalue))
        };

        self.active_fiber_mut().open_upvalues.push(upvalue);
        upvalue
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

        let class = self.class_store.get_obj_range_class();
        let range = self.allocate_root(ObjRange::new(class, begin, end));
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
            name: Gc::null(),
            metaclass: root_base_metaclass.as_gc(),
            superclass: None,
            methods: object::new_obj_string_value_map(),
        });
        let root_object_class = Root::from(object_class_ptr);
        let mut string_metaclass_ptr = self.allocate_bare(ObjClass::new(
            Gc::null(),
            root_base_metaclass.as_gc(),
            Some(root_object_class.as_gc()),
            object::new_obj_string_value_map(),
        ));
        let root_string_metaclass = Root::from(string_metaclass_ptr);
        let mut string_class_ptr = self.allocate_bare(ObjClass::new(
            Gc::null(),
            root_string_metaclass.as_gc(),
            Some(root_object_class.as_gc()),
            object::new_obj_string_value_map(),
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
            object_class_ptr.as_mut().data.name = object_class_name;
            base_metaclass_ptr.as_mut().data.name = base_metaclass_name;
            base_metaclass_ptr.as_mut().data.superclass = Some(root_object_class.as_gc());
            string_metaclass_ptr.as_mut().data.name = string_metaclass_name;
            string_class_ptr.as_mut().data.name = string_class_name;
            core::bind_object_class(self, &mut object_class_ptr);
            core::bind_type_class(self, &mut base_metaclass_ptr);
            core::bind_gc_obj_string_class(self, &mut string_class_ptr, &mut string_metaclass_ptr);
        }

        let empty_chunk = self.allocate(Chunk::new());
        let init_string = self.new_gc_obj_string("new");
        let next_string = self.new_gc_obj_string("next");
        self.active_chunk = empty_chunk;
        self.init_string = init_string;
        self.next_string = next_string;
        let class_store =
            CoreClassStore::new_with_built_ins(self, root_base_metaclass, root_object_class);
        self.core_chunks = self.chunks.clone();
        self.class_store = class_store;
    }

    fn init_built_in_globals(&mut self, module_path: &str) {
        self.define_native(module_path, "clock", core::clock);
        self.define_native(module_path, "type", core::type_);
        self.define_native(module_path, "print", self.printer);
        self.define_native(module_path, "sentinel", core::sentinel);
        let base_metaclass = self.class_store.get_base_metaclass();
        self.set_global(module_path, "Type", Value::ObjClass(base_metaclass));
        let object_class = self.class_store.get_object_class();
        self.set_global(module_path, "Object", Value::ObjClass(object_class));
        let nil_class = self.class_store.get_nil_class();
        self.set_global(module_path, "Nil", Value::ObjClass(nil_class));
        let boolean_class = self.class_store.get_boolean_class();
        self.set_global(module_path, "Bool", Value::ObjClass(boolean_class));
        let number_class = self.class_store.get_number_class();
        self.set_global(module_path, "Num", Value::ObjClass(number_class));
        let sentinel_class = self.class_store.get_sentinel_class();
        self.set_global(module_path, "Sentinel", Value::ObjClass(sentinel_class));
        let obj_closure_class = self.class_store.get_obj_closure_class();
        self.set_global(module_path, "Func", Value::ObjClass(obj_closure_class));
        let obj_native_class = self.class_store.get_obj_native_class();
        self.set_global(module_path, "BuiltIn", Value::ObjClass(obj_native_class));
        let obj_closure_method_class = self.class_store.get_obj_closure_method_class();
        self.set_global(
            module_path,
            "Method",
            Value::ObjClass(obj_closure_method_class),
        );
        let obj_native_method_class = self.class_store.get_obj_native_method_class();
        self.set_global(
            module_path,
            "BuiltInMethod",
            Value::ObjClass(obj_native_method_class),
        );
        let obj_string_class = self.string_class.as_gc();
        self.set_global(module_path, "String", Value::ObjClass(obj_string_class));
        let obj_iter_class = self.class_store.get_obj_iter_class();
        self.set_global(module_path, "Iter", Value::ObjClass(obj_iter_class));
        let obj_map_iter_class = self.class_store.get_obj_map_iter_class();
        self.set_global(module_path, "MapIter", Value::ObjClass(obj_map_iter_class));
        let obj_filter_iter_class = self.class_store.get_obj_filter_iter_class();
        self.set_global(
            module_path,
            "FilterIter",
            Value::ObjClass(obj_filter_iter_class),
        );
        let obj_tuple_class = self.class_store.get_obj_tuple_class();
        self.set_global(module_path, "Tuple", Value::ObjClass(obj_tuple_class));
        let obj_vec_class = self.class_store.get_obj_vec_class();
        self.set_global(module_path, "Vec", Value::ObjClass(obj_vec_class));
        let obj_range_class = self.class_store.get_obj_range_class();
        self.set_global(module_path, "Range", Value::ObjClass(obj_range_class));
        let obj_hash_map_class = self.class_store.get_obj_hash_map_class();
        self.set_global(module_path, "HashMap", Value::ObjClass(obj_hash_map_class));
        let obj_fiber_class = self.class_store.get_obj_fiber_class();
        self.set_global(module_path, "Fiber", Value::ObjClass(obj_fiber_class));
    }

    fn load_frame(&mut self) {
        let prev_chunk = self
            .active_fiber()
            .current_frame()
            .unwrap()
            .closure
            .borrow()
            .function
            .chunk;
        let prev_module = self
            .active_fiber()
            .current_frame()
            .unwrap()
            .closure
            .borrow()
            .module;
        self.active_chunk = prev_chunk;
        self.active_module = prev_module;
        let new_ip = self.active_fiber().current_frame().unwrap().ip;
        self.ip = new_ip;
    }

    #[cfg(any(debug_assertions, feature = "more_vm_safety"))]
    fn active_fiber(&self) -> Ref<'_, ObjFiber> {
        self.fiber.as_ref().unwrap().borrow()
    }

    #[cfg(not(any(debug_assertions, feature = "more_vm_safety")))]
    fn active_fiber(&self) -> &ObjFiber {
        unsafe { self.fiber.as_ref().unwrap().get() }
    }

    #[cfg(any(debug_assertions, feature = "more_vm_safety"))]
    fn active_fiber_mut(&mut self) -> RefMut<'_, ObjFiber> {
        self.fiber.as_ref().unwrap().borrow_mut()
    }

    #[cfg(not(any(debug_assertions, feature = "more_vm_safety")))]
    fn active_fiber_mut(&mut self) -> &mut ObjFiber {
        unsafe { self.fiber.as_ref().unwrap().get_mut() }
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
