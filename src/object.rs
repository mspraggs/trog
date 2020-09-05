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

use std::cell;
use std::cmp;
use std::collections::HashMap;
use std::fmt;

use crate::chunk;
use crate::memory;
use crate::value;

#[derive(Clone, Default)]
pub struct ObjString {
    pub data: String,
}

impl ObjString {
    pub fn new(data: String) -> Self {
        ObjString { data: data }
    }
}

impl memory::GcManaged for ObjString {
    fn mark(&self) {}

    fn blacken(&self) {}
}

impl cmp::PartialEq for ObjString {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

pub enum ObjUpvalue {
    Closed(value::Value),
    Open(usize),
}

impl ObjUpvalue {
    pub fn new(index: usize) -> Self {
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

    pub fn close(&mut self, value: value::Value) {
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

#[derive(Clone, Default)]
pub struct ObjFunction {
    pub arity: u32,
    pub upvalue_count: usize,
    pub chunk: chunk::Chunk,
    pub name: memory::Gc<ObjString>,
}

impl ObjFunction {
    pub fn new(name: memory::Gc<ObjString>) -> Self {
        ObjFunction {
            arity: 0,
            upvalue_count: 0,
            chunk: chunk::Chunk::new(),
            name: name,
        }
    }
}

impl memory::GcManaged for ObjFunction {
    fn mark(&self) {
        self.chunk.mark();
        self.name.mark();
    }

    fn blacken(&self) {
        self.chunk.blacken();
        self.name.blacken();
    }
}

impl fmt::Display for ObjFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name.data.len() {
            0 => write!(f, "<script>"),
            _ => write!(f, "<fn {}>", self.name.data),
        }
    }
}

pub type NativeFn = fn(usize, &mut [value::Value]) -> value::Value;

#[derive(Clone, Default)]
pub struct ObjNative {
    pub function: Option<NativeFn>,
}

impl ObjNative {
    pub fn new(function: NativeFn) -> Self {
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
    pub upvalues: Vec<memory::Gc<cell::RefCell<ObjUpvalue>>>,
}

impl ObjClosure {
    pub fn new(function: memory::Gc<ObjFunction>) -> Self {
        let upvalue_count = function.upvalue_count as usize;
        ObjClosure {
            function: function,
            upvalues: vec![
                memory::allocate(cell::RefCell::new(ObjUpvalue::new(0))).as_gc();
                upvalue_count
            ],
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
    pub methods: HashMap<String, value::Value>,
}

impl ObjClass {
    pub fn new(name: memory::Gc<ObjString>) -> Self {
        ObjClass {
            name: name,
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
    pub class: memory::Gc<cell::RefCell<ObjClass>>,
    pub fields: HashMap<String, value::Value>,
}

impl ObjInstance {
    pub fn new(class: memory::Gc<cell::RefCell<ObjClass>>) -> Self {
        ObjInstance {
            class: class,
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
    pub receiver: value::Value,
    pub method: memory::Gc<cell::RefCell<ObjClosure>>,
}

impl ObjBoundMethod {
    pub fn new(receiver: value::Value, method: memory::Gc<cell::RefCell<ObjClosure>>) -> Self {
        ObjBoundMethod {
            receiver: receiver,
            method: method,
        }
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
