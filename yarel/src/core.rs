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

use std::char;
use std::time;

use crate::common;
use crate::error::{Error, ErrorKind};
use crate::memory::{Gc, GcBoxPtr, Root};
use crate::object::{self, NativeFn, ObjClass, ObjNative, ObjString, ObjStringValueMap};
use crate::utils;
use crate::value::Value;
use crate::vm::Vm;

fn check_num_args(num_args: usize, expected: usize) -> Result<(), Error> {
    if num_args != expected {
        return Err(error!(
            ErrorKind::RuntimeError,
            "Expected {} parameter{} but found {}.",
            expected,
            if expected == 1 { "" } else { "s" },
            num_args
        ));
    }
    Ok(())
}

fn build_methods(
    vm: &mut Vm,
    definitions: &[(&str, NativeFn)],
    extra_methods: Option<ObjStringValueMap>,
) -> (ObjStringValueMap, Vec<Root<ObjNative>>) {
    let mut roots = Vec::new();
    let mut methods = extra_methods.unwrap_or(object::new_obj_string_value_map());

    for (name, native) in definitions {
        let name = vm.new_gc_obj_string(name);
        let obj_native = object::new_root_obj_native(vm, name, *native);
        roots.push(obj_native.clone());
        methods.insert(name, Value::ObjNative(obj_native.as_gc()));
    }

    (methods, roots)
}

/// Global functions

pub(crate) fn clock(_vm: &mut Vm, _num_args: usize) -> Result<Value, Error> {
    let duration = match time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH) {
        Ok(value) => value,
        Err(_) => {
            return Err(error!(
                ErrorKind::RuntimeError,
                "Error calling native function."
            ));
        }
    };
    let seconds = duration.as_secs_f64();
    let nanos = duration.subsec_nanos() as f64 / 1e9;
    Ok(Value::Number(seconds + nanos))
}

pub(crate) fn print(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;
    println!("{}", vm.peek(0));
    Ok(Value::None)
}

pub(crate) fn type_(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    Ok(Value::ObjClass(vm.get_class(vm.peek(0))))
}

pub(crate) fn no_init(vm: &mut Vm, _num_args: usize) -> Result<Value, Error> {
    let class = type_(vm, 1)?;
    Err(error!(
        ErrorKind::RuntimeError,
        "Construction of type {} is unsupported.", class
    ))
}

pub(crate) fn build_unsupported_methods(
    vm: &mut Vm,
) -> (object::ObjStringValueMap, Vec<Root<ObjNative>>) {
    let method_map = &[("__init__", no_init as NativeFn)];
    build_methods(vm, method_map, None)
}

pub(crate) fn sentinel(_vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;
    Ok(Value::Sentinel)
}

/// Type implementation

pub(crate) unsafe fn bind_type_class(_vm: &mut Vm, class: &mut GcBoxPtr<ObjClass>) {
    let methods = class
        .as_ref()
        .data
        .superclass
        .expect("Expected ObjClass.")
        .methods
        .clone();
    class.as_mut().data.methods = methods;
}

/// Object implementation

pub(crate) fn object_is_a(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let receiver_class = vm.get_class(vm.peek(1));
    let query_class = vm.peek(0).try_as_obj_class().ok_or_else(|| {
        error!(
            ErrorKind::ValueError,
            "Expected a class name but found '{}'.",
            vm.peek(0)
        )
    })?;

    if receiver_class == query_class {
        return Ok(Value::Boolean(true));
    }
    let mut superclass = receiver_class.superclass;
    while let Some(parent) = superclass {
        if parent == query_class {
            return Ok(Value::Boolean(true));
        }
        superclass = parent.superclass;
    }
    Ok(Value::Boolean(false))
}

pub(crate) unsafe fn bind_object_class(vm: &mut Vm, class: &mut GcBoxPtr<ObjClass>) {
    let method_map = [("is_a", object_is_a as NativeFn)];
    let (methods, _native_roots) = build_methods(vm, &method_map, None);
    class.as_mut().data.methods = methods;
}

/// String implementation

pub(crate) unsafe fn bind_gc_obj_string_class(
    vm: &mut Vm,
    class: &mut GcBoxPtr<ObjClass>,
    metaclass: &mut GcBoxPtr<ObjClass>,
) {
    let static_method_map = [
        ("from_ascii", string_from_ascii as NativeFn),
        ("from_utf8", string_from_utf8 as NativeFn),
        ("from_code_points", string_from_code_points as NativeFn),
    ];
    let (static_methods, _native_roots) = build_methods(vm, &static_method_map, None);

    metaclass.as_mut().data.methods = static_methods;

    let inherited_methods = class
        .as_ref()
        .data
        .superclass
        .expect("Expected ObjClass.")
        .methods
        .clone();
    let method_map = [
        ("__init__", string_init as NativeFn),
        ("__getitem__", string_get_item as NativeFn),
        ("iter", string_iter as NativeFn),
        ("len", string_len as NativeFn),
        ("count_chars", string_count_chars as NativeFn),
        ("char_byte_index", string_char_byte_index as NativeFn),
        ("find", string_find as NativeFn),
        ("replace", string_replace as NativeFn),
        ("split", string_split as NativeFn),
        ("starts_with", string_starts_with as NativeFn),
        ("ends_with", string_ends_with as NativeFn),
        ("as_num", string_as_num as NativeFn),
        ("to_bytes", string_to_bytes as NativeFn),
        ("to_code_points", string_to_code_points as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, Some(inherited_methods));

    class.as_mut().data.methods = methods;
}

fn string_from_ascii(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let vec_arg = vm.peek(0).try_as_obj_vec().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            &format!("Expected a Vec instance but found '{}'.", vm.peek(0)),
        )
    })?;

    let mut bytes = Vec::with_capacity(vec_arg.borrow().elements.len() * 2);

    for value in vec_arg.borrow().elements.iter() {
        let num = value.try_as_number().ok_or_else(|| {
            Error::with_message(
                ErrorKind::TypeError,
                &format!("Expected a number but found '{}'.", value),
            )
        })?;
        if num < 0.0 || num > 255.0 || num.trunc() != num {
            return Err(error!(
                ErrorKind::ValueError,
                "Expected a positive integer less than 256 but found '{}'.", num
            ));
        }
        if num > 127.0 {
            bytes.push(195_u8);
            bytes.push((num as u8) & 0b1011_1111);
        } else {
            bytes.push(num as u8);
        }
    }

    let string = vm.new_gc_obj_string(&String::from_utf8(bytes).map_err(|_| {
        Error::with_message(
            ErrorKind::ValueError,
            &format!("Unable to create a string from byte sequence."),
        )
    })?);

    Ok(Value::ObjString(string))
}

fn string_from_utf8(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let vec_arg = vm.peek(0).try_as_obj_vec().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            &format!("Expected a Vec instance but found '{}'.", vm.peek(0)),
        )
    })?;

    let bytes: Result<Vec<u8>, Error> = vec_arg
        .borrow()
        .elements
        .iter()
        .map(|v| {
            let num = v.try_as_number().ok_or_else(|| {
                Error::with_message(
                    ErrorKind::TypeError,
                    &format!("Expected a number but found '{}'.", v),
                )
            })?;
            if num < 0.0 || num > 255.0 || num.trunc() != num {
                Err(error!(
                    ErrorKind::ValueError,
                    "Expected a positive integer less than 256 but found '{}'.", num
                ))
            } else {
                Ok(num as u8)
            }
        })
        .collect();

    let string = vm.new_gc_obj_string(&String::from_utf8(bytes?).map_err(|e| {
        let index = e.utf8_error().valid_up_to();
        let byte = e.into_bytes()[index];
        Error::with_message(
            ErrorKind::ValueError,
            &format!(
                "Invalid Unicode encountered at byte {} with index {}.",
                byte, index,
            ),
        )
    })?);

    Ok(Value::ObjString(string))
}

fn string_from_code_points(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let vec_arg = vm.peek(0).try_as_obj_vec().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            &format!("Expected a Vec instance but found '{}'.", vm.peek(0)),
        )
    })?;

    let string: Result<String, Error> = vec_arg
        .borrow()
        .elements
        .iter()
        .map(|v| {
            let num = v.try_as_number().ok_or_else(|| {
                Error::with_message(
                    ErrorKind::TypeError,
                    &format!("Expected a number but found '{}'.", v),
                )
            })?;
            if num < 0.0 || num > u32::MAX as f64 || num.trunc() != num {
                Err(error!(
                    ErrorKind::ValueError,
                    "Expected a positive integer less than {} but found '{}'.",
                    u32::MAX,
                    num
                ))
            } else {
                char::from_u32(num as u32).ok_or_else(|| {
                    error!(
                        ErrorKind::ValueError,
                        "Expected a valid Unicode code point but found '{}'.", num as u32
                    )
                })
            }
        })
        .collect();

    let string = vm.new_gc_obj_string(&string?);

    Ok(Value::ObjString(string))
}

fn string_init(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    Ok(Value::ObjString(
        vm.new_gc_obj_string(format!("{}", vm.peek(0)).as_str()),
    ))
}

fn string_get_item(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let string = vm.peek(1).try_as_obj_string().expect("Expected ObjString.");
    let string_len = string.len() as isize;

    let (begin, end) = match vm.peek(0) {
        Value::Number(_) => {
            let begin = get_bounded_index(vm.peek(0), string_len, "String index out of bounds.")?;
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
        _ => {
            return Err(error!(
                ErrorKind::TypeError,
                "Expected an integer or range."
            ))
        }
    };

    let new_string = vm.new_gc_obj_string(&string.as_str()[begin..end]);

    Ok(Value::ObjString(new_string))
}

fn string_iter(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let iter = vm.new_root_obj_string_iter(
        vm.peek(0)
            .try_as_obj_string()
            .expect("Expected ObjString instance."),
    );
    Ok(Value::ObjStringIter(iter.as_gc()))
}

fn string_len(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let string = vm.peek(0).try_as_obj_string().expect("Expected ObjString.");
    Ok(Value::Number(string.len() as f64))
}

fn string_count_chars(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let string = vm.peek(0).try_as_obj_string().expect("Expected ObjString.");
    Ok(Value::Number(string.chars().count() as f64))
}

fn string_char_byte_index(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let string = vm.peek(1).try_as_obj_string().expect("Expected ObjString.");
    let char_index = get_bounded_index(
        vm.peek(0),
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
    Err(error!(
        ErrorKind::IndexError,
        "Provided character index out of range."
    ))
}

fn string_find(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 2)?;

    let string = vm.peek(2).try_as_obj_string().expect("Expected ObjString.");
    let substring = vm.peek(1).try_as_obj_string().ok_or_else(|| {
        error!(
            ErrorKind::RuntimeError,
            "Expected a string but found '{}'.",
            vm.peek(1)
        )
    })?;
    if substring.is_empty() {
        return Err(error!(ErrorKind::ValueError, "Cannot find empty string."));
    }
    let string_len = string.len() as isize;
    let start = {
        let i = utils::validate_integer(vm.peek(0))?;
        if i < 0 {
            i + string_len
        } else {
            i
        }
    };
    if start < 0 || start >= string_len {
        return Err(error!(ErrorKind::ValueError, "String index out of bounds."));
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

fn string_replace(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 2)?;

    let string = vm.peek(2).try_as_obj_string().expect("Expected ObjString.");
    let old = vm.peek(1).try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", vm.peek(1)),
        )
    })?;
    if old.is_empty() {
        return Err(error!(
            ErrorKind::ValueError,
            "Cannot replace empty string."
        ));
    }
    let new = vm.peek(0).try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", vm.peek(0)),
        )
    })?;
    let new_string = vm.new_gc_obj_string(&string.replace(old.as_str(), new.as_str()));
    Ok(Value::ObjString(new_string))
}

fn string_split(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let string = vm.peek(1).try_as_obj_string().expect("Expected ObjString.");
    let delim = vm.peek(0).try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", vm.peek(0)),
        )
    })?;
    if delim.is_empty() {
        return Err(error!(
            ErrorKind::ValueError,
            "Cannot split using an empty string."
        ));
    }
    let splits = vm.new_root_obj_vec();
    for substr in string.as_str().split(delim.as_str()) {
        let new_str = Value::ObjString(vm.new_gc_obj_string(substr));
        splits.borrow_mut().elements.push(new_str);
    }
    Ok(Value::ObjVec(splits.as_gc()))
}

fn string_starts_with(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let string = vm.peek(1).try_as_obj_string().expect("Expected ObjString.");
    let prefix = vm.peek(0).try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            format!("Expected a string but found '{}'.", vm.peek(0)).as_str(),
        )
    })?;

    Ok(Value::Boolean(string.as_str().starts_with(prefix.as_str())))
}

fn string_ends_with(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let string = vm.peek(1).try_as_obj_string().expect("Expected ObjString.");
    let prefix = vm.peek(0).try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            format!("Expected a string but found '{}'.", vm.peek(0)).as_str(),
        )
    })?;

    Ok(Value::Boolean(string.as_str().ends_with(prefix.as_str())))
}

fn string_as_num(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let string = vm.peek(0).try_as_obj_string().expect("Expected ObjString.");
    let num = string.parse::<f64>().or_else(|_| {
        Err(error!(
            ErrorKind::ValueError,
            "Unable to parse number from '{}'.",
            vm.peek(0)
        ))
    })?;

    Ok(Value::Number(num))
}

fn string_to_bytes(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let string = vm.peek(0).try_as_obj_string().expect("Expected ObjString.");

    let vec = vm.new_root_obj_vec();
    vec.borrow_mut().elements = string
        .as_bytes()
        .iter()
        .map(|&b| Value::Number(b as f64))
        .collect();

    Ok(Value::ObjVec(vec.as_gc()))
}

fn string_to_code_points(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let string = vm.peek(0).try_as_obj_string().expect("Expected ObjString.");

    let vec = vm.new_root_obj_vec();
    vec.borrow_mut().elements = string
        .chars()
        .map(|c| Value::Number((c as u32) as f64))
        .collect();

    Ok(Value::ObjVec(vec.as_gc()))
}

fn check_char_boundary(string: Gc<ObjString>, pos: usize, desc: &str) -> Result<(), Error> {
    if !string.as_str().is_char_boundary(pos) {
        return Err(error!(
            ErrorKind::IndexError,
            "Provided {} is not on a character boundary.", desc
        ));
    }
    Ok(())
}

/// StringIter implementation

fn string_iter_next(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;
    let iter = vm
        .peek(0)
        .try_as_obj_string_iter()
        .expect("Expected ObjIter instance.");
    let iterable = iter.borrow().iterable;
    let next = {
        let mut borrowed_iter = iter.borrow_mut();
        borrowed_iter.next()
    };
    if let Some((begin, end)) = next {
        let slice = &iterable[begin..end];
        let string = vm.new_gc_obj_string(slice);
        return Ok(Value::ObjString(string));
    }
    Ok(Value::Sentinel)
}

pub fn new_root_obj_string_iter_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("StringIter");
    let (methods, _native_roots) =
        build_methods(vm, &[("next", string_iter_next as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

/// Tuple implementation

pub fn new_root_obj_tuple_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("Tuple");
    let method_map = [
        ("__init__", tuple_init as NativeFn),
        ("__getitem__", tuple_get_item as NativeFn),
        ("len", tuple_len as NativeFn),
        ("iter", tuple_iter as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

fn tuple_init(vm: &mut Vm, _num_args: usize) -> Result<Value, Error> {
    let vec = vm.new_root_obj_tuple(Vec::new());
    Ok(Value::ObjTuple(vec.as_gc()))
}

fn tuple_get_item(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let tuple = vm.peek(1).try_as_obj_tuple().expect("Expected ObjTuple");

    match vm.peek(0) {
        Value::Number(_) => {
            let index = get_bounded_index(
                vm.peek(0),
                tuple.elements.len() as isize,
                "Tuple index parameter out of bounds",
            )?;
            Ok(tuple.elements[index])
        }
        Value::ObjRange(r) => {
            let tuple_len = tuple.elements.len() as isize;
            let (begin, end) = r.get_bounded_range(tuple_len, "Tuple")?;
            let new_elements = Vec::from(&tuple.elements[begin..end]);
            let new_tuple = object::new_gc_obj_tuple(vm, tuple.class, new_elements);
            Ok(Value::ObjTuple(new_tuple))
        }
        _ => Err(error!(
            ErrorKind::TypeError,
            "Expected an integer or range."
        )),
    }
}

fn tuple_len(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let tuple = vm.peek(0).try_as_obj_tuple().expect("Expected ObjTuple");
    Ok(Value::from(tuple.elements.len() as f64))
}

fn tuple_iter(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let iter = vm.new_root_obj_tuple_iter(
        vm.peek(0)
            .try_as_obj_tuple()
            .expect("Expected ObjTuple instance."),
    );
    Ok(Value::ObjTupleIter(iter.as_gc()))
}

/// TupleIter implementation

pub fn new_root_obj_tuple_iter_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("TupleIter");
    let (methods, _native_roots) =
        build_methods(vm, &[("next", tuple_iter_next as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

fn tuple_iter_next(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;
    let iter = vm
        .peek(0)
        .try_as_obj_tuple_iter()
        .expect("Expected ObjTupleIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

/// Vec implemenation

pub fn new_root_obj_vec_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("Vec");
    let method_map = [
        ("__init__", vec_init as NativeFn),
        ("push", vec_push as NativeFn),
        ("pop", vec_pop as NativeFn),
        ("__getitem__", vec_get_item as NativeFn),
        ("__setitem__", vec_set_item as NativeFn),
        ("len", vec_len as NativeFn),
        ("iter", vec_iter as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

fn vec_init(vm: &mut Vm, _num_args: usize) -> Result<Value, Error> {
    let vec = vm.new_root_obj_vec();
    Ok(Value::ObjVec(vec.as_gc()))
}

fn vec_push(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let vec = vm.peek(1).try_as_obj_vec().expect("Expected ObjVec");

    if vec.borrow().elements.len() >= common::VEC_ELEMS_MAX {
        return Err(error!(ErrorKind::RuntimeError, "Vec max capcity reached."));
    }

    vec.borrow_mut().elements.push(vm.peek(0));

    Ok(vm.peek(1))
}

fn vec_pop(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let vec = vm.peek(0).try_as_obj_vec().expect("Expected ObjVec");
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements.pop().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            "Cannot pop from empty Vec instance.",
        )
    })
}

fn vec_get_item(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let vec = vm.peek(1).try_as_obj_vec().expect("Expected ObjVec");

    match vm.peek(0) {
        Value::Number(_) => {
            let borrowed_vec = vec.borrow();
            let index = get_bounded_index(
                vm.peek(0),
                borrowed_vec.elements.len() as isize,
                "Vec index parameter out of bounds",
            )?;
            Ok(borrowed_vec.elements[index])
        }
        Value::ObjRange(r) => {
            let vec_len = vec.borrow().elements.len() as isize;
            let (begin, end) = r.get_bounded_range(vec_len, "Vec")?;
            let new_vec = object::new_gc_obj_vec(vm, vec.borrow().class);
            new_vec
                .borrow_mut()
                .elements
                .extend_from_slice(&vec.borrow().elements[begin..end]);
            Ok(Value::ObjVec(new_vec))
        }
        _ => Err(error!(
            ErrorKind::TypeError,
            "Expected an integer or range."
        )),
    }
}

fn vec_set_item(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 2)?;

    let vec = vm.peek(2).try_as_obj_vec().expect("Expected ObjVec");
    let index = get_bounded_index(
        vm.peek(1),
        vec.borrow().elements.len() as isize,
        "Vec index parameter out of bounds",
    )?;
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements[index] = vm.peek(0);
    Ok(Value::None)
}

fn vec_len(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let vec = vm.peek(0).try_as_obj_vec().expect("Expected ObjVec");
    let borrowed_vec = vec.borrow();
    Ok(Value::from(borrowed_vec.elements.len() as f64))
}

fn vec_iter(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let iter = vm.new_root_obj_vec_iter(
        vm.peek(0)
            .try_as_obj_vec()
            .expect("Expected ObjVec instance."),
    );
    Ok(Value::ObjVecIter(iter.as_gc()))
}

fn get_bounded_index(value: Value, bound: isize, msg: &str) -> Result<usize, Error> {
    let mut index = utils::validate_integer(value)?;
    if index < 0 {
        index += bound;
    }
    if index < 0 || index >= bound {
        return Err(error!(ErrorKind::IndexError, "{}", msg));
    }

    Ok(index as usize)
}

/// VecIter implementation

pub fn new_root_obj_vec_iter_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("VecIter");
    let (methods, _native_roots) = build_methods(vm, &[("next", vec_iter_next as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

fn vec_iter_next(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;
    let iter = vm
        .peek(0)
        .try_as_obj_vec_iter()
        .expect("Expected ObjVecIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

/// Range implementation

pub fn new_root_obj_range_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("Range");
    let method_map = [
        ("__init__", range_init as NativeFn),
        ("iter", range_iter as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

fn range_init(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 2)?;

    let begin = utils::validate_integer(vm.peek(1))?;
    let end = utils::validate_integer(vm.peek(0))?;
    let range = vm.new_root_obj_range(begin, end);
    Ok(Value::ObjRange(range.as_gc()))
}

fn range_iter(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let iter = vm.new_root_obj_range_iter(
        vm.peek(0)
            .try_as_obj_range()
            .expect("Expected ObjRange instance."),
    );
    Ok(Value::ObjRangeIter(iter.as_gc()))
}

/// RangeIter implementation

fn range_iter_next(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;
    let iter = vm
        .peek(0)
        .try_as_obj_range_iter()
        .expect("Expected ObjIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

pub fn new_root_obj_range_iter_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("RangeIter");
    let (methods, _native_roots) =
        build_methods(vm, &[("next", range_iter_next as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

/// HashMap implementation

pub fn new_root_obj_hash_map_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("HashMap");
    let method_map = [
        ("__init__", hash_map_init as NativeFn),
        ("has_key", hash_map_has_key as NativeFn),
        ("get", hash_map_get as NativeFn),
        ("insert", hash_map_insert as NativeFn),
        ("remove", hash_map_remove as NativeFn),
        ("clear", hash_map_clear as NativeFn),
        ("len", hash_map_len as NativeFn),
        ("keys", hash_map_keys as NativeFn),
        ("values", hash_map_values as NativeFn),
        ("items", hash_map_items as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

fn hash_map_init(vm: &mut Vm, _num_args: usize) -> Result<Value, Error> {
    let hash_map = vm.new_root_obj_hash_map();
    Ok(Value::ObjHashMap(hash_map.as_gc()))
}

fn hash_map_has_key(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let hash_map = vm
        .peek(1)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap.");

    let key = validate_hash_map_key(vm.peek(0))?;
    let borrowed_hash_map = hash_map.borrow();
    Ok(Value::Boolean(
        borrowed_hash_map.elements.contains_key(&key),
    ))
}

fn hash_map_get(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let hash_map = vm
        .peek(1)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");

    let key = validate_hash_map_key(vm.peek(0))?;

    let borrowed_hash_map = hash_map.borrow();
    Ok(*borrowed_hash_map.elements.get(&key).unwrap_or(&Value::None))
}

fn hash_map_insert(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 2)?;

    let hash_map = vm
        .peek(2)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");

    let key = validate_hash_map_key(vm.peek(1))?;
    let value = vm.peek(0);

    let mut borrowed_hash_map = hash_map.borrow_mut();
    Ok(borrowed_hash_map
        .elements
        .insert(key, value)
        .unwrap_or(Value::None))
}

fn hash_map_remove(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;

    let hash_map = vm
        .peek(1)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");

    let key = validate_hash_map_key(vm.peek(0))?;

    let mut borrowed_hash_map = hash_map.borrow_mut();
    Ok(borrowed_hash_map
        .elements
        .remove(&key)
        .unwrap_or(Value::None))
}

fn hash_map_clear(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let hash_map = vm
        .peek(0)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");
    let mut borrowed_hash_map = hash_map.borrow_mut();
    borrowed_hash_map.elements.clear();
    Ok(Value::None)
}

fn hash_map_len(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let hash_map = vm
        .peek(0)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");
    let borrowed_hash_map = hash_map.borrow();
    Ok(Value::Number(borrowed_hash_map.elements.len() as f64))
}

fn hash_map_keys(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let hash_map = vm
        .peek(0)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");
    let borrowed_hash_map = hash_map.borrow();
    let keys: Vec<_> = borrowed_hash_map.elements.keys().map(|&v| v).collect();
    let obj_keys = vm.new_root_obj_vec();
    obj_keys.borrow_mut().elements = keys;
    Ok(Value::ObjVec(obj_keys.as_gc()))
}

fn hash_map_values(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let hash_map = vm
        .peek(0)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");
    let borrowed_hash_map = hash_map.borrow();
    let values: Vec<_> = borrowed_hash_map.elements.values().map(|&v| v).collect();
    let obj_values = vm.new_root_obj_vec();
    obj_values.borrow_mut().elements = values;
    Ok(Value::ObjVec(obj_values.as_gc()))
}

fn hash_map_items(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 0)?;

    let hash_map = vm
        .peek(0)
        .try_as_obj_hash_map()
        .expect("Expected ObjHashMap");
    let borrowed_hash_map = hash_map.borrow();
    let root_obj_pairs: Vec<_> = borrowed_hash_map
        .elements
        .iter()
        .map(|(&k, &v)| vm.new_root_obj_tuple(vec![k, v]))
        .collect();
    let vec_elements = root_obj_pairs
        .iter()
        .map(|o| Value::ObjTuple(o.as_gc()))
        .collect();
    let obj_items = vm.new_root_obj_vec();
    obj_items.borrow_mut().elements = vec_elements;
    Ok(Value::ObjVec(obj_items.as_gc()))
}

fn validate_hash_map_key(key: Value) -> Result<Value, Error> {
    if !key.has_hash() {
        return Err(error!(
            ErrorKind::ValueError,
            "Cannot use unhashable value '{}' as HashMap key.", key
        ));
    }
    Ok(key)
}

/// Module implementation

pub fn new_root_obj_module_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("Module");
    let (methods, _native_roots) = build_methods(vm, &[("__init__", no_init as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

/// Fiber implementation

pub fn new_root_obj_fiber_metaclass(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("FiberClass");
    let (methods, _native_roots) = build_methods(vm, &[("yield", fiber_yield as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

pub fn new_root_obj_fiber_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    superclass: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("Fiber");
    let (methods, _native_roots) = build_methods(
        vm,
        &[
            ("__init__", fiber_init as NativeFn),
            ("call", fiber_call as NativeFn),
        ],
        None,
    );
    object::new_root_obj_class(vm, class_name, metaclass, Some(superclass), methods)
}

fn fiber_init(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    check_num_args(num_args, 1)?;
    let closure = vm.peek(0).try_as_obj_closure().ok_or_else(|| {
        error!(
            ErrorKind::RuntimeError,
            "Expected a function but found '{}'.",
            vm.peek(0)
        )
    })?;
    if closure.borrow().function.arity > 2 {
        return Err(error!(
            ErrorKind::RuntimeError,
            "Fiber expects a closure that accepts at most 1 parameter."
        ));
    }
    let fiber = vm.new_root_obj_fiber(closure);
    Ok(Value::ObjFiber(fiber.as_gc()))
}

fn fiber_call(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    // `Fiber.call` and `Fiber.yield` are the only two functions that take a
    // variable number of arguments. To make handling call frames a little
    // easier, we inject a dummy nil argument where none is provided. (The same
    // approach is taken in `Fiber.yield`.) Also, because `Vm::call_native`
    // truncates the stack based on the number of arguments after each call, we
    // add a nil parameter after the closure has been pushed to account for the
    // fact that the stacks before and after the call will be different.
    let fiber = vm
        .peek(num_args)
        .try_as_obj_fiber()
        .expect("Expected ObjFiber.");
    let (is_new, arity) = {
        let borrowed_fiber = unsafe { fiber.get() };
        (borrowed_fiber.is_new(), borrowed_fiber.call_arity)
    };
    if is_new {
        check_num_args(num_args, arity - 1)?;
    } else {
        if num_args > 1 {
            return Err(error!(
                ErrorKind::RuntimeError,
                "Expected at most 1 parameter but found {}.", num_args
            ));
        }
    }
    let arg = if num_args == 1 {
        let value = vm.peek(0);
        Some(value)
    } else {
        vm.push(Value::None);
        None
    };
    vm.load_fiber(fiber, arg)?;

    Ok(vm.peek(0))
}

fn fiber_yield(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    if num_args > 1 {
        return Err(error!(
            ErrorKind::RuntimeError,
            "Expected at most 1 parameter but found {}.", num_args
        ));
    }
    let arg = if num_args == 1 {
        Some(vm.peek(0))
    } else {
        None
    };
    if num_args == 1 {
        vm.pop();
    }
    vm.unload_fiber(arg)?;
    Ok(vm.peek(0))
}
