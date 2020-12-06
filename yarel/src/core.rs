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

use crate::class_store::CoreClassStore;
use crate::common;
use crate::error::{Error, ErrorKind};
use crate::memory::{Gc, Heap, Root};
use crate::object::{self, NativeFn, ObjClass, ObjStringStore, ObjVec};
use crate::utils;
use crate::value::Value;

fn add_native_method_to_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
    class: Gc<RefCell<ObjClass>>,
    name: &str,
    native: NativeFn,
) {
    let name = string_store.new_gc_obj_string(heap, name);
    let obj_native = object::new_root_obj_native(heap, native);
    class
        .borrow_mut()
        .methods
        .insert(name, Value::ObjNative(obj_native.as_gc()));
}

// String implementation

pub fn bind_gc_obj_string_class(heap: &mut Heap, string_store: &mut ObjStringStore) {
    let string_class = string_store.get_obj_string_class();
    add_native_method_to_class(heap, string_store, string_class, "__init__", string_init);
}

fn string_init(
    heap: &mut Heap,
    _class_store: &CoreClassStore,
    string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 2 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected one argument to 'String'."
        );
    }
    Ok(Value::ObjString(
        string_store.new_gc_obj_string(heap, format!("{}", args[1]).as_str()),
    ))
}

/// Vec implemenation

pub fn new_root_obj_vec_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
    iter_class: Gc<RefCell<ObjClass>>,
) -> Root<RefCell<ObjClass>> {
    let class_name = string_store.new_gc_obj_string(heap, "Vec");
    let class = object::new_root_obj_class(heap, class_name);
    add_native_method_to_class(heap, string_store, class.as_gc(), "__init__", vec_init);
    add_native_method_to_class(heap, string_store, class.as_gc(), "push", vec_push);
    add_native_method_to_class(heap, string_store, class.as_gc(), "pop", vec_pop);
    add_native_method_to_class(
        heap,
        string_store,
        class.as_gc(),
        "__getitem__",
        vec_get_item,
    );
    add_native_method_to_class(
        heap,
        string_store,
        class.as_gc(),
        "__setitem__",
        vec_set_item,
    );
    add_native_method_to_class(heap, string_store, class.as_gc(), "len", vec_len);
    add_native_method_to_class(heap, string_store, class.as_gc(), "__iter__", vec_iter);
    class.borrow_mut().add_superclass(iter_class);
    class
}

fn vec_init(
    heap: &mut Heap,
    class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    _args: &mut [Value],
) -> Result<Value, Error> {
    let vec = object::new_root_obj_vec(heap, class_store.get_obj_vec_class());
    Ok(Value::ObjVec(vec.as_gc()))
}

fn vec_push(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 2 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 1 parameter but got {}",
            args.len() - 1
        );
    }

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");

    if vec.borrow().elements.len() >= common::VEC_ELEMS_MAX {
        return error!(ErrorKind::RuntimeError, "Vec max capcity reached.");
    }

    vec.borrow_mut().elements.push(args[1]);

    Ok(args[0])
}

fn vec_pop(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}",
            args.len() - 1
        );
    }

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements.pop().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            "Cannot pop from empty Vec instance.",
        )
    })
}

fn vec_get_item(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 2 {
        let msg = format!("Expected 1 parameters but got {}", args.len() - 1);
        return Err(Error::with_message(ErrorKind::RuntimeError, msg.as_str()));
    }

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");

    let index = get_vec_index(vec, args[1])?;
    let borrowed_vec = vec.borrow();
    Ok(borrowed_vec.elements[index])
}

fn vec_set_item(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 3 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 2 parameters but got {}",
            args.len() - 1
        );
    }

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let index = get_vec_index(vec, args[1])?;
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements[index] = args[2];
    Ok(Value::None)
}

fn vec_len(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}",
            args.len() - 1
        );
    }

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let borrowed_vec = vec.borrow();
    Ok(Value::from(borrowed_vec.elements.len() as f64))
}

fn vec_iter(
    heap: &mut Heap,
    class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}.",
            args.len() - 1
        );
    }

    let iter = object::new_root_obj_vec_iter(
        heap,
        class_store.get_obj_vec_iter_class(),
        args[0].try_as_obj_vec().expect("Expected ObjVec instance."),
    );
    Ok(Value::ObjVecIter(iter.as_gc()))
}

fn get_vec_index(vec: Gc<RefCell<ObjVec>>, value: Value) -> Result<usize, Error> {
    let vec_len = vec.borrow_mut().elements.len() as isize;
    let index = if let Value::Number(n) = value {
        #[allow(clippy::float_cmp)]
        if n.trunc() != n {
            return error!(
                ErrorKind::ValueError,
                "Expected an integer but found '{}'.", n
            );
        }
        if n < 0.0 {
            n as isize + vec_len
        } else {
            n as isize
        }
    } else {
        return error!(
            ErrorKind::ValueError,
            "Expected an integer but found '{}'.", value
        );
    };

    if index < 0 || index >= vec_len {
        return error!(ErrorKind::IndexError, "Vec index parameter out of bounds");
    }

    Ok(index as usize)
}

/// VecIter implementation

pub fn new_root_obj_vec_iter_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
) -> Root<RefCell<ObjClass>> {
    let class_name = string_store.new_gc_obj_string(heap, "VecIter");
    let class = object::new_root_obj_class(heap, class_name);
    add_native_method_to_class(heap, string_store, class.as_gc(), "__next__", vec_iter_next);
    class
}

fn vec_iter_next(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_vec_iter()
        .expect("Expected ObjVecIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

/// Range implementation

pub fn new_root_obj_range_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
    iter_class: Gc<RefCell<ObjClass>>,
) -> Root<RefCell<ObjClass>> {
    let class_name = string_store.new_gc_obj_string(heap, "Range");
    let class = object::new_root_obj_class(heap, class_name);
    add_native_method_to_class(heap, string_store, class.as_gc(), "__init__", range_init);
    add_native_method_to_class(heap, string_store, class.as_gc(), "__iter__", range_iter);
    class.borrow_mut().add_superclass(iter_class);
    class
}

fn range_init(
    heap: &mut Heap,
    class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 3 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 2 parameters but got {}",
            args.len() - 1
        );
    }
    let mut bounds: [isize; 2] = [0; 2];
    for i in 0..2 {
        bounds[i] = utils::validate_integer(args[i + 1])?;
    }
    let range = object::new_root_obj_range(
        heap,
        class_store.get_obj_range_class(),
        bounds[0],
        bounds[1],
    );
    Ok(Value::ObjRange(range.as_gc()))
}

fn range_iter(
    heap: &mut Heap,
    class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}.",
            args.len() - 1
        );
    }

    let iter = object::new_root_obj_range_iter(
        heap,
        class_store.get_obj_range_iter_class(),
        args[0]
            .try_as_obj_range()
            .expect("Expected ObjRange instance."),
    );
    Ok(Value::ObjRangeIter(iter.as_gc()))
}

/// RangeIter implementation

fn range_iter_next(
    _heap: &mut Heap,
    _context: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_range_iter()
        .expect("Expected ObjIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

pub fn new_root_obj_range_iter_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
) -> Root<RefCell<ObjClass>> {
    let class_name = string_store.new_gc_obj_string(heap, "RangeIter");
    let class = object::new_root_obj_class(heap, class_name);
    add_native_method_to_class(
        heap,
        string_store,
        class.as_gc(),
        "__next__",
        range_iter_next,
    );
    class
}
