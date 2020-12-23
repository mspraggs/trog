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

use crate::common;
use crate::error::{Error, ErrorKind};
use crate::memory::{Gc, Heap, Root};
use crate::object::{
    self, NativeFn, ObjClass, ObjNative, ObjString, ObjStringStore, ObjStringValueMap,
};
use crate::utils;
use crate::value::Value;
use crate::vm::Vm;

fn check_num_args(args: &[Value], expected: usize) -> Result<(), Error> {
    if args.len() != expected + 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected {} parameter{} but found {}.",
            expected,
            if expected == 1 { "" } else { "s" },
            args.len() - 1
        );
    }
    Ok(())
}

fn build_methods(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
    definitions: &[(&str, NativeFn)],
    extra_methods: Option<ObjStringValueMap>,
) -> (ObjStringValueMap, Vec<Root<ObjNative>>) {
    let mut roots = Vec::new();
    let mut methods = extra_methods.unwrap_or(object::new_obj_string_value_map());

    for (name, native) in definitions {
        let name = string_store.new_gc_obj_string(heap, name);
        let obj_native = object::new_root_obj_native(heap, *native);
        roots.push(obj_native.clone());
        methods.insert(name, Value::ObjNative(obj_native.as_gc()));
    }

    (methods, roots)
}

/// String implementation

pub fn bind_gc_obj_string_class(heap: &mut Heap, string_store: &mut ObjStringStore) {
    let method_map = [
        ("__init__", string_init as NativeFn),
        ("__getitem__", string_get_item as NativeFn),
        ("__iter__", string_iter as NativeFn),
        ("len", string_len as NativeFn),
        ("count_chars", string_count_chars as NativeFn),
        ("char_byte_index", string_char_byte_index as NativeFn),
        ("find", string_find as NativeFn),
        ("replace", string_replace as NativeFn),
        ("split", string_split as NativeFn),
        ("starts_with", string_starts_with as NativeFn),
        ("ends_with", string_ends_with as NativeFn),
        ("as_num", string_as_num as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(heap, string_store, &method_map, None);

    string_store.set_obj_string_class_methods(methods);
}

fn string_init(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    Ok(Value::ObjString(
        vm.string_store
            .borrow_mut()
            .new_gc_obj_string(&mut vm.heap.borrow_mut(), format!("{}", args[1]).as_str()),
    ))
}

fn string_get_item(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let string_len = string.len() as isize;

    let (begin, end) = match args[1] {
        Value::Number(_) => {
            let begin = get_bounded_index(args[1], string_len, "String index out of bounds.")?;
            check_char_boundary(string, begin, "string index")?;
            let mut end = begin + 1;
            while end <= string.len() && !string.as_str().is_char_boundary(end) {
                end += 1;
            }
            (begin, end)
        }
        Value::ObjRange(r) => {
            let (begin, end) = r.get_bounded_range(string_len, "String")?;
            check_char_boundary(string, begin, "string slice start")?;
            check_char_boundary(string, end, "string slice end")?;
            (begin, end)
        }
        _ => return error!(ErrorKind::TypeError, "Expected an integer or range."),
    };

    let new_string = vm
        .string_store
        .borrow_mut()
        .new_gc_obj_string(&mut vm.heap.borrow_mut(), &string.as_str()[begin..end]);

    Ok(Value::ObjString(new_string))
}

fn string_iter(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let iter = object::new_root_obj_string_iter(
        &mut vm.heap.borrow_mut(),
        vm.class_store.get_obj_string_iter_class(),
        args[0]
            .try_as_obj_string()
            .expect("Expected ObjString instance."),
    );
    Ok(Value::ObjStringIter(iter.as_gc()))
}

fn string_len(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    Ok(Value::Number(string.len() as f64))
}

fn string_count_chars(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    Ok(Value::Number(string.chars().count() as f64))
}

fn string_char_byte_index(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let char_index = get_bounded_index(
        args[1],
        string.as_str().chars().count() as isize,
        "String index parameter out of bounds.",
    )?;

    let mut char_count = 0;
    for i in 0..string.len() + 1 {
        if string.as_str().is_char_boundary(i) {
            if char_count == char_index {
                return Ok(Value::Number(i as f64));
            }
            char_count += 1;
        }
    }
    error!(
        ErrorKind::IndexError,
        "Provided character index out of range."
    )
}

fn string_find(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let substring = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[1]),
        )
    })?;
    if substring.is_empty() {
        return error!(ErrorKind::ValueError, "Cannot find empty string.");
    }
    let string_len = string.len() as isize;
    let start = {
        let i = utils::validate_integer(args[2])?;
        if i < 0 {
            i + string_len
        } else {
            i
        }
    };
    if start < 0 || start >= string_len {
        return error!(ErrorKind::ValueError, "String index out of bounds.");
    }
    let start = start as usize;
    check_char_boundary(string, start, "string index")?;
    for i in start..string.as_str().len() {
        if !string.is_char_boundary(i) || !string.is_char_boundary(i + substring.len()) {
            continue;
        }
        let slice = &string[i..i + substring.len()];
        if i >= start && slice == substring.as_str() {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::None)
}

fn string_replace(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let old = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[1]),
        )
    })?;
    if old.is_empty() {
        return error!(ErrorKind::ValueError, "Cannot replace empty string.");
    }
    let new = args[2].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[2]),
        )
    })?;
    let new_string = vm.string_store.borrow_mut().new_gc_obj_string(
        &mut vm.heap.borrow_mut(),
        &string.replace(old.as_str(), new.as_str()),
    );
    Ok(Value::ObjString(new_string))
}

fn string_split(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let delim = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[1]),
        )
    })?;
    if delim.is_empty() {
        return error!(ErrorKind::ValueError, "Cannot split using an empty string.");
    }
    let splits = object::new_root_obj_vec(
        &mut vm.heap.borrow_mut(),
        vm.class_store.get_obj_vec_class(),
    );
    for substr in string.as_str().split(delim.as_str()) {
        let new_str = Value::ObjString(
            vm.string_store
                .borrow_mut()
                .new_gc_obj_string(&mut vm.heap.borrow_mut(), substr),
        );
        splits.borrow_mut().elements.push(new_str);
    }
    Ok(Value::ObjVec(splits.as_gc()))
}

fn string_starts_with(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let prefix = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            format!("Expected a string but found '{}'.", args[1]).as_str(),
        )
    })?;

    Ok(Value::Boolean(string.as_str().starts_with(prefix.as_str())))
}

fn string_ends_with(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let prefix = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            format!("Expected a string but found '{}'.", args[1]).as_str(),
        )
    })?;

    Ok(Value::Boolean(string.as_str().ends_with(prefix.as_str())))
}

fn string_as_num(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let num = string.parse::<f64>().or_else(|_| {
        error!(
            ErrorKind::ValueError,
            "Unable to parse number from '{}'.", args[0]
        )
    })?;

    Ok(Value::Number(num))
}

fn check_char_boundary(string: Gc<ObjString>, pos: usize, desc: &str) -> Result<(), Error> {
    if !string.as_str().is_char_boundary(pos) {
        return error!(
            ErrorKind::IndexError,
            "Provided {} is not on a character boundary.", desc
        );
    }
    Ok(())
}

/// StringIter implementation

fn string_iter_next(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_string_iter()
        .expect("Expected ObjIter instance.");
    let next = {
        let mut borrowed_iter = iter.borrow_mut();
        borrowed_iter.next()
    };
    if let Some(slice) = next {
        let string = vm
            .string_store
            .borrow_mut()
            .new_gc_obj_string(&mut vm.heap.borrow_mut(), &slice);
        return Ok(Value::ObjString(string));
    }
    Ok(Value::Sentinel)
}

pub fn new_root_obj_string_iter_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
) -> Root<ObjClass> {
    let class_name = string_store.new_gc_obj_string(heap, "StringIter");
    let (methods, _native_roots) = build_methods(
        heap,
        string_store,
        &[("__next__", string_iter_next as NativeFn)],
        None,
    );
    object::new_root_obj_class(heap, class_name, methods)
}

/// Vec implemenation

pub fn new_root_obj_vec_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
    iter_class: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = string_store.new_gc_obj_string(heap, "Vec");
    let method_map = [
        ("__init__", vec_init as NativeFn),
        ("push", vec_push as NativeFn),
        ("pop", vec_pop as NativeFn),
        ("__getitem__", vec_get_item as NativeFn),
        ("__setitem__", vec_set_item as NativeFn),
        ("len", vec_len as NativeFn),
        ("__iter__", vec_iter as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(
        heap,
        string_store,
        &method_map,
        Some(iter_class.methods.clone()),
    );
    // class.borrow_mut().add_superclass(iter_class);
    object::new_root_obj_class(heap, class_name, methods)
}

fn vec_init(vm: &Vm, _args: &[Value]) -> Result<Value, Error> {
    let vec = object::new_root_obj_vec(
        &mut vm.heap.borrow_mut(),
        vm.class_store.get_obj_vec_class(),
    );
    Ok(Value::ObjVec(vec.as_gc()))
}

fn vec_push(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");

    if vec.borrow().elements.len() >= common::VEC_ELEMS_MAX {
        return error!(ErrorKind::RuntimeError, "Vec max capcity reached.");
    }

    vec.borrow_mut().elements.push(args[1]);

    Ok(args[0])
}

fn vec_pop(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements.pop().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            "Cannot pop from empty Vec instance.",
        )
    })
}

fn vec_get_item(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");

    match args[1] {
        Value::Number(_) => {
            let borrowed_vec = vec.borrow();
            let index = get_bounded_index(
                args[1],
                borrowed_vec.elements.len() as isize,
                "Vec index parameter out of bounds",
            )?;
            Ok(borrowed_vec.elements[index])
        }
        Value::ObjRange(r) => {
            let vec_len = vec.borrow().elements.len() as isize;
            let (begin, end) = r.get_bounded_range(vec_len, "Vec")?;
            let new_vec = object::new_gc_obj_vec(
                &mut vm.heap.borrow_mut(),
                vm.class_store.get_obj_vec_class(),
            );
            new_vec
                .borrow_mut()
                .elements
                .extend_from_slice(&vec.borrow().elements[begin..end]);
            Ok(Value::ObjVec(new_vec))
        }
        _ => error!(ErrorKind::TypeError, "Expected an integer or range."),
    }
}

fn vec_set_item(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let index = get_bounded_index(
        args[1],
        vec.borrow().elements.len() as isize,
        "Vec index parameter out of bounds",
    )?;
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements[index] = args[2];
    Ok(Value::None)
}

fn vec_len(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let borrowed_vec = vec.borrow();
    Ok(Value::from(borrowed_vec.elements.len() as f64))
}

fn vec_iter(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let iter = object::new_root_obj_vec_iter(
        &mut vm.heap.borrow_mut(),
        vm.class_store.get_obj_vec_iter_class(),
        args[0].try_as_obj_vec().expect("Expected ObjVec instance."),
    );
    Ok(Value::ObjVecIter(iter.as_gc()))
}

fn get_bounded_index(value: Value, bound: isize, msg: &str) -> Result<usize, Error> {
    let mut index = utils::validate_integer(value)?;
    if index < 0 {
        index += bound;
    }
    if index < 0 || index >= bound {
        return error!(ErrorKind::IndexError, "{}", msg);
    }

    Ok(index as usize)
}

/// VecIter implementation

pub fn new_root_obj_vec_iter_class(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
) -> Root<ObjClass> {
    let class_name = string_store.new_gc_obj_string(heap, "VecIter");
    let (methods, _native_roots) = build_methods(
        heap,
        string_store,
        &[("__next__", vec_iter_next as NativeFn)],
        None,
    );
    object::new_root_obj_class(heap, class_name, methods)
}

fn vec_iter_next(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
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
    iter_class: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = string_store.new_gc_obj_string(heap, "Range");
    let method_map = [
        ("__init__", range_init as NativeFn),
        ("__iter__", range_iter as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(
        heap,
        string_store,
        &method_map,
        Some(iter_class.methods.clone()),
    );
    // class.borrow_mut().add_superclass(iter_class);
    object::new_root_obj_class(heap, class_name, methods)
}

fn range_init(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let mut bounds: [isize; 2] = [0; 2];
    for i in 0..2 {
        bounds[i] = utils::validate_integer(args[i + 1])?;
    }
    let range = object::new_root_obj_range(
        &mut vm.heap.borrow_mut(),
        vm.class_store.get_obj_range_class(),
        bounds[0],
        bounds[1],
    );
    Ok(Value::ObjRange(range.as_gc()))
}

fn range_iter(vm: &Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let iter = object::new_root_obj_range_iter(
        &mut vm.heap.borrow_mut(),
        vm.class_store.get_obj_range_iter_class(),
        args[0]
            .try_as_obj_range()
            .expect("Expected ObjRange instance."),
    );
    Ok(Value::ObjRangeIter(iter.as_gc()))
}

/// RangeIter implementation

fn range_iter_next(_vm: &Vm, args: &[Value]) -> Result<Value, Error> {
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
) -> Root<ObjClass> {
    let class_name = string_store.new_gc_obj_string(heap, "RangeIter");
    let (methods, _native_roots) = build_methods(
        heap,
        string_store,
        &[("__next__", range_iter_next as NativeFn)],
        None,
    );
    object::new_root_obj_class(heap, class_name, methods)
}
