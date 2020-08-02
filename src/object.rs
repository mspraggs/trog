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
use std::fmt;
use std::rc;

use crate::chunk;
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

#[derive(Clone, Default)]
pub struct ObjFunction {
    pub arity: u32,
    pub upvalue_count: usize,
    pub chunk: chunk::Chunk,
    pub name: rc::Rc<cell::RefCell<ObjString>>,
}

impl ObjFunction {
    pub fn new(name: String) -> Self {
        ObjFunction {
            arity: 0,
            upvalue_count: 0,
            chunk: chunk::Chunk::new(),
            name: rc::Rc::new(cell::RefCell::new(ObjString::new(name))),
        }
    }
}

impl fmt::Display for ObjFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name.borrow().data.len() {
            0 => write!(f, "<script>"),
            _ => write!(f, "<fn {}>", self.name.borrow().data),
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

pub struct ObjClosure {
    pub function: rc::Rc<cell::RefCell<ObjFunction>>,
    pub upvalues: Vec<rc::Rc<cell::RefCell<ObjUpvalue>>>,
}

impl ObjClosure {
    pub fn new(function: rc::Rc<cell::RefCell<ObjFunction>>) -> Self {
        let upvalue_count = function.borrow().upvalue_count as usize;
        ObjClosure {
            function: function,
            upvalues: vec![rc::Rc::new(cell::RefCell::new(ObjUpvalue::new(0))); upvalue_count],
        }
    }
}
