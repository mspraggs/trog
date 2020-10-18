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
use std::fmt;

use crate::memory::{self, Gc};
use crate::value::Value;
use crate::vm::{Vm, VmError};

pub type ObjString = String;

pub fn new_gc_obj_string(vm: &mut Vm, data: &str) -> memory::Gc<ObjString> {
    memory::allocate(vm, ObjString::from(data))
}

impl memory::GcManaged for ObjString {
    fn mark(&self) {}

    fn blacken(&self) {}
}

pub enum ObjUpvalue {
    Closed(Value),
    Open(usize),
}

pub fn new_gc_obj_upvalue(vm: &mut Vm, index: usize) -> Gc<RefCell<ObjUpvalue>> {
    memory::allocate(vm, RefCell::new(ObjUpvalue::new(index)))
}

impl ObjUpvalue {
    fn new(index: usize) -> Self {
        ObjUpvalue::Open(index)
    }

    pub fn is_open(&self) -> bool {
        match self {
            Self::Open(_) => true,
            Self::Closed(_) => false,
        }
    }

    pub fn is_open_with_index(&self, index: usize) -> bool {
        self.is_open_with_pred(|i| i == index)
    }

    pub fn is_open_with_pred(&self, predicate: impl Fn(usize) -> bool) -> bool {
        match self {
            Self::Open(index) => predicate(*index),
            Self::Closed(_) => false,
        }
    }

    pub fn close(&mut self, value: Value) {
        *self = Self::Closed(value);
    }
}

impl memory::GcManaged for ObjUpvalue {
    fn mark(&self) {
        match self {
            ObjUpvalue::Closed(value) => value.mark(),
            ObjUpvalue::Open(_) => {}
        }
    }

    fn blacken(&self) {
        match self {
            ObjUpvalue::Closed(value) => value.blacken(),
            ObjUpvalue::Open(_) => {}
        }
    }
}

#[derive(Clone)]
#[repr(align(16))]  // Figure out why this is necessary (prevents corruption/double-free)
pub struct ObjFunction {
    pub arity: u32,
    pub upvalue_count: usize,
    pub chunk_index: usize,
    pub name: memory::Gc<ObjString>,
}

pub fn new_gc_obj_function(vm: &mut Vm, name: Gc<ObjString>) -> Gc<ObjFunction> {
    let chunk_index = vm.new_chunk();
    memory::allocate(vm, ObjFunction::new(name, chunk_index))
}

impl ObjFunction {
    fn new(name: memory::Gc<ObjString>, chunk_index: usize) -> Self {
        ObjFunction {
            arity: 0,
            upvalue_count: 0,
            chunk_index: chunk_index,
            name,
        }
    }
}

impl memory::GcManaged for ObjFunction {
    fn mark(&self) {
        self.name.mark();
    }

    fn blacken(&self) {
        self.name.blacken();
    }
}

impl fmt::Display for ObjFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name.len() {
            0 => write!(f, "<script>"),
            _ => write!(f, "<fn {}>", *self.name),
        }
    }
}

pub type NativeFn = fn(usize, &mut [Value]) -> Result<Value, VmError>;

#[derive(Clone)]
pub struct ObjNative {
    pub function: Option<NativeFn>,
}

pub fn new_gc_obj_native(vm: &mut Vm, function: NativeFn) -> Gc<ObjNative> {
    memory::allocate(vm, ObjNative::new(function))
}

impl ObjNative {
    fn new(function: NativeFn) -> Self {
        ObjNative {
            function: Some(function),
        }
    }
}

impl memory::GcManaged for ObjNative {
    fn mark(&self) {}

    fn blacken(&self) {}
}

pub struct ObjClosure {
    pub function: memory::Gc<ObjFunction>,
    pub upvalues: Vec<memory::Gc<RefCell<ObjUpvalue>>>,
}

pub fn new_gc_obj_closure(vm: &mut Vm, function: Gc<ObjFunction>) -> Gc<RefCell<ObjClosure>> {
    let closure = ObjClosure::new(vm, function);
    memory::allocate(vm, RefCell::new(closure))
}

impl ObjClosure {
    fn new(vm: &mut Vm, function: memory::Gc<ObjFunction>) -> Self {
        let upvalue_count = function.upvalue_count as usize;
        ObjClosure {
            function,
            upvalues: vec![memory::allocate(vm, RefCell::new(ObjUpvalue::new(0))); upvalue_count],
        }
    }
}

impl memory::GcManaged for ObjClosure {
    fn mark(&self) {
        self.function.mark();
        self.upvalues.mark();
    }

    fn blacken(&self) {
        self.function.blacken();
        self.upvalues.blacken();
    }
}

pub struct ObjClass {
    pub name: memory::Gc<ObjString>,
    pub methods: HashMap<String, Value>,
}

pub fn new_gc_obj_class(vm: &mut Vm, name: Gc<ObjString>) -> Gc<RefCell<ObjClass>> {
    memory::allocate(vm, RefCell::new(ObjClass::new(name)))
}

impl ObjClass {
    fn new(name: memory::Gc<ObjString>) -> Self {
        ObjClass {
            name,
            methods: HashMap::new(),
        }
    }
}

impl memory::GcManaged for ObjClass {
    fn mark(&self) {
        self.name.mark();
        self.methods.mark();
    }

    fn blacken(&self) {
        self.name.blacken();
        self.methods.blacken();
    }
}

pub struct ObjInstance {
    pub class: memory::Gc<RefCell<ObjClass>>,
    pub fields: HashMap<String, Value>,
}

pub fn new_gc_obj_instance(vm: &mut Vm, class: Gc<RefCell<ObjClass>>) -> Gc<RefCell<ObjInstance>> {
    memory::allocate(vm, RefCell::new(ObjInstance::new(class)))
}

impl ObjInstance {
    fn new(class: Gc<RefCell<ObjClass>>) -> Self {
        ObjInstance {
            class,
            fields: HashMap::new(),
        }
    }
}

impl memory::GcManaged for ObjInstance {
    fn mark(&self) {
        self.class.mark();
        self.fields.mark();
    }

    fn blacken(&self) {
        self.class.blacken();
        self.fields.blacken();
    }
}

pub struct ObjBoundMethod {
    pub receiver: Value,
    pub method: memory::Gc<RefCell<ObjClosure>>,
}

pub fn new_gc_obj_bound_method(
    vm: &mut Vm,
    receiver: Value,
    method: Gc<RefCell<ObjClosure>>,
) -> Gc<RefCell<ObjBoundMethod>> {
    memory::allocate(vm, RefCell::new(ObjBoundMethod::new(receiver, method)))
}

impl ObjBoundMethod {
    fn new(receiver: Value, method: memory::Gc<RefCell<ObjClosure>>) -> Self {
        ObjBoundMethod { receiver, method }
    }
}

impl memory::GcManaged for ObjBoundMethod {
    fn mark(&self) {
        self.receiver.mark();
        self.method.mark();
    }

    fn blacken(&self) {
        self.receiver.mark();
        self.method.blacken();
    }
}
