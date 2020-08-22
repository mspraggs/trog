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
use std::cmp;
use std::fmt;

use crate::memory;
use crate::object;

#[derive(Clone, Copy)]
pub enum Value {
    Boolean(bool),
    Number(f64),
    ObjString(memory::Gc<object::ObjString>),
    ObjUpvalue(memory::Gc<RefCell<object::ObjUpvalue>>),
    ObjFunction(memory::Gc<object::ObjFunction>),
    ObjNative(memory::Gc<object::ObjNative>),
    ObjClosure(memory::Gc<RefCell<object::ObjClosure>>),
    ObjClass(memory::Gc<object::ObjClass>),
    ObjInstance(memory::Gc<RefCell<object::ObjInstance>>),
    None,
}

impl Value {
    pub fn as_bool(&self) -> bool {
        match self {
            Value::None => true,
            Value::Boolean(underlying) => *underlying,
            _ => false,
        }
    }
}

impl memory::GcManaged for Value {
    fn mark(&self) {
        match self {
            Value::ObjString(inner) => inner.mark(),
            Value::ObjUpvalue(inner) => inner.mark(),
            Value::ObjFunction(inner) => inner.mark(),
            Value::ObjNative(inner) => inner.mark(),
            Value::ObjClosure(inner) => inner.mark(),
            Value::ObjClass(inner) => inner.mark(),
            Value::ObjInstance(inner) => inner.mark(),
            _ => {}
        }
    }

    fn blacken(&self) {
        match self {
            Value::ObjString(inner) => inner.blacken(),
            Value::ObjUpvalue(inner) => inner.blacken(),
            Value::ObjFunction(inner) => inner.blacken(),
            Value::ObjNative(inner) => inner.blacken(),
            Value::ObjClosure(inner) => inner.blacken(),
            Value::ObjClass(inner) => inner.blacken(),
            Value::ObjInstance(inner) => inner.blacken(),
            _ => {}
        }
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        let root = memory::allocate(object::ObjString::new(value));
        Value::ObjString(root.as_gc())
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::from(String::from(value))
    }
}

impl From<object::ObjFunction> for Value {
    fn from(value: object::ObjFunction) -> Self {
        let root = memory::allocate(value);
        Value::ObjFunction(root.as_gc())
    }
}

impl From<memory::Gc<object::ObjFunction>> for Value {
    fn from(value: memory::Gc<object::ObjFunction>) -> Self {
        Value::ObjFunction(value)
    }
}

impl From<object::NativeFn> for Value {
    fn from(value: object::NativeFn) -> Self {
        let root = memory::allocate(object::ObjNative::new(value));
        Value::ObjNative(root.as_gc())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(underlying) => write!(f, "{}", underlying),
            Value::Boolean(underlying) => write!(f, "{}", underlying),
            Value::ObjString(underlying) => write!(f, "{}", underlying.data),
            Value::ObjUpvalue(_) => write!(f, "upvalue"),
            Value::ObjFunction(underlying) => write!(f, "{}", **underlying),
            Value::ObjNative(_) => write!(f, "<native fn>"),
            Value::ObjClosure(underlying) => write!(f, "{}", *underlying.borrow().function),
            Value::ObjClass(underlying) => write!(f, "{}", underlying.name.data),
            Value::ObjInstance(underlying) => {
                write!(f, "{} instance", underlying.borrow().class.name.data)
            }
            Value::None => write!(f, "nil"),
        }
    }
}

impl cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Boolean(first), Value::Boolean(second)) => first == second,
            (Value::Number(first), Value::Number(second)) => first == second,
            (Value::ObjString(first), Value::ObjString(second)) => **first == **second,
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}
