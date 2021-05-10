/* Copyright 2020-2021 Matt Spraggs
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
use std::hash::{Hash, Hasher};

use crate::hash::PassThroughHasher;
use crate::memory::{self, Gc};
use crate::object::{
    ObjBoundMethod, ObjClass, ObjClosure, ObjFiber, ObjFunction, ObjHashMap, ObjInstance,
    ObjModule, ObjNative, ObjRange, ObjRangeIter, ObjString, ObjStringIter, ObjTuple, ObjTupleIter,
    ObjVec, ObjVecIter,
};
use crate::unsafe_ref_cell::UnsafeRefCell;
use crate::utils;

#[derive(Clone, Copy)]
pub enum Value {
    Boolean(bool),
    Number(f64),
    ObjString(Gc<ObjString>),
    ObjStringIter(Gc<RefCell<ObjStringIter>>),
    ObjFunction(Gc<ObjFunction>),
    ObjNative(Gc<ObjNative>),
    ObjClosure(Gc<ObjClosure>),
    ObjClass(Gc<ObjClass>),
    ObjInstance(Gc<RefCell<ObjInstance>>),
    ObjBoundMethod(Gc<RefCell<ObjBoundMethod<ObjClosure>>>),
    ObjBoundNative(Gc<RefCell<ObjBoundMethod<ObjNative>>>),
    ObjTuple(Gc<ObjTuple>),
    ObjTupleIter(Gc<RefCell<ObjTupleIter>>),
    ObjVec(Gc<RefCell<ObjVec>>),
    ObjVecIter(Gc<RefCell<ObjVecIter>>),
    ObjRange(Gc<ObjRange>),
    ObjRangeIter(Gc<RefCell<ObjRangeIter>>),
    ObjHashMap(Gc<RefCell<ObjHashMap>>),
    ObjModule(Gc<RefCell<ObjModule>>),
    ObjFiber(Gc<UnsafeRefCell<ObjFiber>>),
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

    pub(crate) fn has_hash(&self) -> bool {
        match self {
            Value::Boolean(_) => true,
            Value::Number(_) => true,
            Value::ObjString(_) => true,
            Value::ObjClass(_) => true,
            Value::ObjTuple(t) => t.has_hash(),
            Value::ObjRange(_) => true,
            Value::None => true,
            _ => false,
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

    pub fn try_as_obj_string_iter(&self) -> Option<Gc<RefCell<ObjStringIter>>> {
        match self {
            Value::ObjStringIter(inner) => Some(*inner),
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
    pub fn try_as_obj_closure(&self) -> Option<Gc<ObjClosure>> {
        match self {
            Value::ObjClosure(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_class(&self) -> Option<Gc<ObjClass>> {
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
    pub fn try_as_obj_bound_method(&self) -> Option<Gc<RefCell<ObjBoundMethod<ObjClosure>>>> {
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
    pub fn try_as_obj_tuple(&self) -> Option<Gc<ObjTuple>> {
        match self {
            Value::ObjTuple(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_tuple_iter(&self) -> Option<Gc<RefCell<ObjTupleIter>>> {
        match self {
            Value::ObjTupleIter(inner) => Some(*inner),
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
    pub fn try_as_obj_range(&self) -> Option<Gc<ObjRange>> {
        match self {
            Value::ObjRange(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_range_iter(&self) -> Option<Gc<RefCell<ObjRangeIter>>> {
        match self {
            Value::ObjRangeIter(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_hash_map(&self) -> Option<Gc<RefCell<ObjHashMap>>> {
        match self {
            Value::ObjHashMap(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_module(&self) -> Option<Gc<RefCell<ObjModule>>> {
        match self {
            Value::ObjModule(inner) => Some(*inner),
            _ => None,
        }
    }
    pub fn try_as_obj_fiber(&self) -> Option<Gc<UnsafeRefCell<ObjFiber>>> {
        match self {
            Value::ObjFiber(inner) => Some(*inner),
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
            Value::ObjStringIter(inner) => inner.mark(),
            Value::ObjFunction(inner) => inner.mark(),
            Value::ObjNative(inner) => inner.mark(),
            Value::ObjClosure(inner) => inner.mark(),
            Value::ObjClass(inner) => inner.mark(),
            Value::ObjInstance(inner) => inner.mark(),
            Value::ObjBoundMethod(inner) => inner.mark(),
            Value::ObjBoundNative(inner) => inner.mark(),
            Value::ObjTuple(inner) => inner.mark(),
            Value::ObjTupleIter(inner) => inner.mark(),
            Value::ObjVec(inner) => inner.mark(),
            Value::ObjVecIter(inner) => inner.mark(),
            Value::ObjRange(inner) => inner.mark(),
            Value::ObjRangeIter(inner) => inner.mark(),
            Value::ObjHashMap(inner) => inner.mark(),
            Value::ObjModule(inner) => inner.mark(),
            Value::ObjFiber(inner) => inner.mark(),
            _ => {}
        }
    }

    fn blacken(&self) {
        match self {
            Value::ObjString(inner) => inner.blacken(),
            Value::ObjStringIter(inner) => inner.blacken(),
            Value::ObjFunction(inner) => inner.blacken(),
            Value::ObjNative(inner) => inner.blacken(),
            Value::ObjClosure(inner) => inner.blacken(),
            Value::ObjClass(inner) => inner.blacken(),
            Value::ObjInstance(inner) => inner.blacken(),
            Value::ObjBoundMethod(inner) => inner.blacken(),
            Value::ObjBoundNative(inner) => inner.blacken(),
            Value::ObjTuple(inner) => inner.blacken(),
            Value::ObjTupleIter(inner) => inner.blacken(),
            Value::ObjVec(inner) => inner.blacken(),
            Value::ObjVecIter(inner) => inner.blacken(),
            Value::ObjRange(inner) => inner.blacken(),
            Value::ObjRangeIter(inner) => inner.blacken(),
            Value::ObjHashMap(inner) => inner.blacken(),
            Value::ObjModule(inner) => inner.blacken(),
            Value::ObjFiber(inner) => inner.blacken(),
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
            Value::ObjStringIter(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjFunction(underlying) => {
                write!(f, "<{} @ {:p}>", **underlying, underlying.as_ptr())
            }
            Value::ObjNative(native) => write!(f, "<{}>", **native),
            Value::ObjClosure(underlying) => {
                write!(f, "<{} @ {:p}>", **underlying, underlying.as_ptr())
            }
            Value::ObjClass(underlying) => write!(f, "<class {}>", **underlying),
            Value::ObjInstance(underlying) => {
                write!(f, "<{} @ {:p}>", *underlying.borrow(), underlying.as_ptr())
            }
            Value::ObjBoundMethod(underlying) => {
                write!(f, "<{} @ {:p}>", *underlying.borrow(), underlying.as_ptr())
            }
            Value::ObjBoundNative(underlying) => {
                write!(f, "<{} @ {:p}>", *underlying.borrow(), underlying.as_ptr())
            }
            Value::ObjTuple(underlying) => write!(f, "{}", **underlying),
            Value::ObjTupleIter(underlying) => {
                write!(f, "<{} @ {:p}>", *underlying.borrow(), underlying.as_ptr())
            }
            Value::ObjVec(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjVecIter(underlying) => {
                write!(f, "<{} @ {:p}>", *underlying.borrow(), underlying.as_ptr())
            }
            Value::ObjRange(underlying) => write!(f, "{}", **underlying),
            Value::ObjRangeIter(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjHashMap(underlying) => write!(f, "{}", *underlying.borrow()),
            Value::ObjModule(underlying) => write!(f, "<{}>", *underlying.borrow()),
            Value::ObjFiber(underlying) => {
                write!(
                    f,
                    "<{} @ {:p}>",
                    unsafe { &*underlying.get() },
                    underlying.as_ptr()
                )
            }
            Value::None => write!(f, "nil"),
        }
    }
}

impl cmp::Eq for Value {}

impl cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Boolean(first), Value::Boolean(second)) => first == second,
            (Value::Number(first), Value::Number(second)) => first == second,
            (Value::ObjString(first), Value::ObjString(second)) => *first == *second,
            (Value::ObjStringIter(first), Value::ObjStringIter(second)) => *first == *second,
            (Value::ObjFunction(first), Value::ObjFunction(second)) => *first == *second,
            (Value::ObjNative(first), Value::ObjNative(second)) => *first == *second,
            (Value::ObjClosure(first), Value::ObjClosure(second)) => *first == *second,
            (Value::ObjClass(first), Value::ObjClass(second)) => *first == *second,
            (Value::ObjInstance(first), Value::ObjInstance(second)) => *first == *second,
            (Value::ObjBoundMethod(first), Value::ObjBoundMethod(second)) => *first == *second,
            (Value::ObjTuple(first), Value::ObjTuple(second)) => **first == **second,
            (Value::ObjTupleIter(first), Value::ObjTupleIter(second)) => *first == *second,
            (Value::ObjVec(first), Value::ObjVec(second)) => *first.borrow() == *second.borrow(),
            (Value::ObjVecIter(first), Value::ObjVecIter(second)) => *first == *second,
            (Value::ObjRange(first), Value::ObjRange(second)) => *first == *second,
            (Value::ObjRangeIter(first), Value::ObjRangeIter(second)) => *first == *second,
            (Value::ObjHashMap(first), Value::ObjHashMap(second)) => {
                *first.borrow() == *second.borrow()
            }
            (Value::ObjModule(first), Value::ObjModule(second)) => *first == *second,
            (Value::ObjFiber(first), Value::ObjFiber(second)) => *first == *second,
            (Value::None, Value::None) => true,
            _ => false,
        }
    }
}

impl Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let hash = match self {
            Value::Boolean(b) => {
                if *b {
                    1_u64
                } else {
                    0_u64
                }
            }
            Value::Number(n) => utils::hash_number(*n),
            Value::ObjString(s) => s.hash,
            Value::ObjClass(c) => c.name.hash,
            Value::ObjTuple(t) => {
                let mut hasher = PassThroughHasher::default();
                t.hash(&mut hasher);
                hasher.finish()
            }
            Value::ObjRange(r) => {
                utils::hash_number(r.begin as f64) ^ utils::hash_number(r.end as f64)
            }
            Value::None => 2_u64,
            _ => {
                panic!("Unhashable value type: {}", self);
            }
        };
        state.write_u64(hash);
    }
}
