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
    ObjFunction(memory::Gc<object::ObjFunction>),
    ObjNative(memory::Gc<object::ObjNative>),
    ObjClosure(memory::Gc<RefCell<object::ObjClosure>>),
    ObjClass(memory::Gc<RefCell<object::ObjClass>>),
    ObjInstance(memory::Gc<RefCell<object::ObjInstance>>),
    ObjBoundMethod(memory::Gc<RefCell<object::ObjBoundMethod>>),
    None,
}

impl Value {
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Boolean(underlying) => *underlying,
            Value::None => false,
            _ => true,
        }
    }
}

impl memory::GcManaged for Value {
    fn mark(&self) {
        match self {
            Value::ObjString(inner) => inner.mark(),
            Value::ObjFunction(inner) => inner.mark(),
            Value::ObjNative(inner) => inner.mark(),
            Value::ObjClosure(inner) => inner.mark(),
            Value::ObjClass(inner) => inner.mark(),
            Value::ObjInstance(inner) => inner.mark(),
            Value::ObjBoundMethod(inner) => inner.mark(),
            _ => {}
        }
    }

    fn blacken(&self) {
        match self {
            Value::ObjString(inner) => inner.blacken(),
            Value::ObjFunction(inner) => inner.blacken(),
            Value::ObjNative(inner) => inner.blacken(),
            Value::ObjClosure(inner) => inner.blacken(),
            Value::ObjClass(inner) => inner.blacken(),
            Value::ObjInstance(inner) => inner.blacken(),
            Value::ObjBoundMethod(inner) => inner.blacken(),
            _ => {}
        }
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(value)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(underlying) => {
                // Sigh... surely there's a more succinct way to do this?
                if *underlying == 0.0 && underlying.is_sign_negative() {
                    write!(f, "-0")
                } else {
                    write!(f, "{}", underlying)
                }
            }
            Value::Boolean(underlying) => write!(f, "{}", underlying),
            Value::ObjString(underlying) => write!(f, "{}", **underlying),
            Value::ObjFunction(underlying) => write!(f, "{}", **underlying),
            Value::ObjNative(_) => write!(f, "<native fn>"),
            Value::ObjClosure(underlying) => write!(f, "{}", *underlying.borrow().function),
            Value::ObjClass(underlying) => write!(f, "{}", *underlying.borrow().name),
            Value::ObjInstance(underlying) => {
                write!(f, "{} instance", *underlying.borrow().class.borrow().name)
            }
            Value::ObjBoundMethod(underlying) => {
                write!(f, "{}", *underlying.borrow().method.borrow().function)
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
            (Value::ObjFunction(first), Value::ObjFunction(second)) => *first == *second,
            (Value::ObjNative(first), Value::ObjNative(second)) => *first == *second,
            (Value::ObjClosure(first), Value::ObjClosure(second)) => *first == *second,
            (Value::ObjClass(first), Value::ObjClass(second)) => *first == *second,
            (Value::ObjInstance(first), Value::ObjInstance(second)) => *first == *second,
            (Value::ObjBoundMethod(first), Value::ObjBoundMethod(second)) => *first == *second,
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}
