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

#[derive(Clone, Default)]
pub struct ObjFunction {
    pub arity: u32,
    pub chunk: chunk::Chunk,
    pub name: rc::Rc<cell::RefCell<ObjString>>,
}

impl ObjFunction {
    pub fn new(name: String) -> Self {
        ObjFunction {
            arity: 0,
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
