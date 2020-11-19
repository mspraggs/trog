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

use crate::memory::{self, Gc};
use crate::object::{
    ObjBoundMethod, ObjClass, ObjClosure, ObjFunction, ObjInstance, ObjNative, ObjString, ObjVec,
    ObjVecIter,
};

#[derive(Clone, Copy)]
pub enum Value {
    Boolean(bool),
    Number(f64),
    ObjString(Gc<ObjString>),
    ObjFunction(Gc<ObjFunction>),
    ObjNative(Gc<ObjNative>),
    ObjClosure(Gc<RefCell<ObjClosure>>),
    ObjClass(Gc<RefCell<ObjClass>>),
    ObjInstance(Gc<RefCell<ObjInstance>>),
    ObjBoundMethod(Gc<RefCell<ObjBoundMethod<RefCell<ObjClosure>>>>),
    ObjBoundNative(Gc<RefCell<ObjBoundMethod<ObjNative>>>),
    ObjVec(Gc<RefCell<ObjVec>>),
    ObjVecIter(Gc<RefCell<ObjVecIter>>),
    None,
    Sentinel,
}

impl Value {
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Boolean(underlying) => *underlying,
            Value::None => false,
            _ => true,
        }
    }

    pub fn try_as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn try_as_number(&self) -> Option<f64> {
        match self {
            Value::Number(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn try_as_obj_string(&self) -> Option<Gc<ObjString>> {
        match self {
            Value::ObjString(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn try_as_obj_function(&self) -> Option<Gc<ObjFunction>> {
        match self {
            Value::ObjFunction(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn try_as_obj_native(&self) -> Option<Gc<ObjNative>> {
        match self {
            Value::ObjNative(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_closure(&self) -> Option<Gc<RefCell<ObjClosure>>> {
        match self {
            Value::ObjClosure(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_class(&self) -> Option<Gc<RefCell<ObjClass>>> {
        match self {
            Value::ObjClass(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_instance(&self) -> Option<Gc<RefCell<ObjInstance>>> {
        match self {
            Value::ObjInstance(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_bound_method(
        &self,
    ) -> Option<Gc<RefCell<ObjBoundMethod<RefCell<ObjClosure>>>>> {
        match self {
            Value::ObjBoundMethod(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_bound_native(&self) -> Option<Gc<RefCell<ObjBoundMethod<ObjNative>>>> {
        match self {
            Value::ObjBoundNative(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_vec(&self) -> Option<Gc<RefCell<ObjVec>>> {
        match self {
            Value::ObjVec(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_vec_iter(&self) -> Option<Gc<RefCell<ObjVecIter>>> {
        match self {
            Value::ObjVecIter(inner) => Some(*inner),
            _ => None,
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::None
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
            Value::ObjBoundNative(inner) => inner.mark(),
            Value::ObjVec(inner) => inner.mark(),
            Value::ObjVecIter(inner) => inner.mark(),
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
            Value::ObjBoundNative(inner) => inner.blacken(),
            Value::ObjVec(inner) => inner.blacken(),
            Value::ObjVecIter(inner) => inner.blacken(),
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
            Value::ObjClosure(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjClass(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjInstance(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjBoundMethod(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjBoundNative(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjVec(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjVecIter(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::None => write!(f, "nil"),
            Value::Sentinel => write!(f, "<sentinel>"),
        }
    }
}

impl cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Boolean(first), Value::Boolean(second)) => first == second,
            (Value::Number(first), Value::Number(second)) => first == second,
            (Value::ObjString(first), Value::ObjString(second)) => *first == *second,
            (Value::ObjFunction(first), Value::ObjFunction(second)) => *first == *second,
            (Value::ObjNative(first), Value::ObjNative(second)) => *first == *second,
            (Value::ObjClosure(first), Value::ObjClosure(second)) => *first == *second,
            (Value::ObjClass(first), Value::ObjClass(second)) => *first == *second,
            (Value::ObjInstance(first), Value::ObjInstance(second)) => *first == *second,
            (Value::ObjBoundMethod(first), Value::ObjBoundMethod(second)) => *first == *second,
            (Value::ObjVec(first), Value::ObjVec(second)) => *first.borrow() == *second.borrow(),
            (Value::ObjVecIter(first), Value::ObjVecIter(second)) => *first == *second,
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}
