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

use crate::object;

#[derive(Clone)]
pub enum Value {
    Boolean(bool),
    Number(f64),
    ObjString(rc::Rc<cell::RefCell<object::ObjString>>),
    ObjUpvalue(rc::Rc<cell::RefCell<object::ObjUpvalue>>),
    ObjFunction(rc::Rc<cell::RefCell<object::ObjFunction>>),
    ObjNative(rc::Rc<cell::RefCell<object::ObjNative>>),
    ObjClosure(rc::Rc<cell::RefCell<object::ObjClosure>>),
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

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::ObjString(rc::Rc::new(cell::RefCell::new(object::ObjString::new(
            value,
        ))))
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::from(String::from(value))
    }
}

impl From<object::ObjFunction> for Value {
    fn from(value: object::ObjFunction) -> Self {
        Value::ObjFunction(rc::Rc::new(cell::RefCell::new(value)))
    }
}

impl From<object::NativeFn> for Value {
    fn from(value: object::NativeFn) -> Self {
        Value::ObjNative(rc::Rc::new(cell::RefCell::new(object::ObjNative::new(
            value,
        ))))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(underlying) => write!(f, "{}", underlying),
            Value::Boolean(underlying) => write!(f, "{}", underlying),
            Value::ObjString(underlying) => write!(f, "{}", underlying.borrow().data),
            Value::ObjUpvalue(_) => write!(f, "upvalue"),
            Value::ObjFunction(underlying) => write!(f, "{}", underlying.borrow()),
            Value::ObjNative(_) => write!(f, "<native fn>"),
            Value::ObjClosure(underlying) => write!(f, "{}", underlying.borrow().function.borrow()),
            Value::None => write!(f, "nil"),
        }
    }
}

impl cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Boolean(first), Value::Boolean(second)) => first == second,
            (Value::Number(first), Value::Number(second)) => first == second,
            (Value::ObjString(first), Value::ObjString(second)) => first == second,
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}
