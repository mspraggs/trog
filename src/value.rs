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

use std::cmp;
use std::fmt;
use std::rc;

use crate::object;

#[derive(Clone)]
pub enum Value {
    Boolean(bool),
    Number(f64),
    ObjString(rc::Rc<object::ObjString>),
    None,
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::ObjString(rc::Rc::new(object::ObjString::new(value)))
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::from(String::from(value))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(underlying) => write!(f, "{}", underlying),
            Value::Boolean(underlying) => write!(f, "{}", underlying),
            Value::ObjString(underlying) => write!(f, "{}", underlying.data),
            Value::None => write!(f, "nil"),
        }
    }
}

impl cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Boolean(first), Value::Boolean(second)) => first == second,
            (Value::Number(first), Value::Number(second)) => first == second,
            (Value::ObjString(first), Value::ObjString(second)) => {
                first == second
            }
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}
