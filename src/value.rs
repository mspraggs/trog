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

#[derive(Copy, Clone)]
pub enum Value {
    Boolean(bool),
    Number(f64),
    None,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(underlying) => write!(f, "{}", underlying),
            Value::Boolean(underlying) => write!(f, "{}", underlying),
            Value::None => write!(f, "nil"),
        }
    }
}

impl cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Boolean(first), Value::Boolean(second)) => first == second,
            (Value::Number(first), Value::Number(second)) => first == second,
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}
