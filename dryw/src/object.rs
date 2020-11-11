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
use std::cmp::{self, Eq};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;

use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, FnvHasher};
use crate::memory::{self, Gc, Root};
use crate::value::Value;

type ObjStringCache = Root<RefCell<HashMap<u64, Gc<ObjString>>>>;

// The use of ManuallyDrop here is sadly necessary, otherwise the Root may try
// to read memory that's been free'd when the Root is dropped.
thread_local!(
    static OBJ_STRING_CACHE: ManuallyDrop<ObjStringCache> =
        ManuallyDrop::new(memory::allocate_root(RefCell::new(HashMap::new())))
);

pub struct ObjString {
    string: String,
    pub(crate) hash: u64,
}

pub fn new_gc_obj_string(data: &str) -> Gc<ObjString> {
    let hash = {
        let mut hasher = FnvHasher::new();
        (*data).hash(&mut hasher);
        hasher.finish()
    };
    if let Some(gc_string) = OBJ_STRING_CACHE.with(|cache| cache.borrow().get(&hash).map(|v| *v)) {
        return gc_string;
    }
    let ret = memory::allocate(ObjString::new(data, hash));
    OBJ_STRING_CACHE.with(|cache| cache.borrow_mut().insert(hash, ret));
    ret
}

pub fn new_root_obj_string(data: &str) -> Root<ObjString> {
    new_gc_obj_string(data).as_root()
}

impl ObjString {
    fn new(string: &str, hash: u64) -> Self {
        ObjString {
            string: String::from(string),
            hash: hash,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.string.is_empty()
    }

    pub fn len(&self) -> usize {
        self.string.len()
    }
}

impl From<&str> for ObjString {
    #[inline]
    fn from(string: &str) -> ObjString {
        let mut hasher = FnvHasher::new();
        (*string).hash(&mut hasher);
        ObjString {
            string: String::from(string),
            hash: hasher.finish(),
        }
    }
}

impl fmt::Display for ObjString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.string)
    }
}

impl Hash for Gc<ObjString> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

impl Eq for Gc<ObjString> {}

impl memory::GcManaged for ObjString {
    fn mark(&self) {}

    fn blacken(&self) {}
}

pub enum ObjUpvalue {
    Closed(Value),
    Open(usize),
}

pub fn new_gc_obj_upvalue(index: usize) -> Gc<RefCell<ObjUpvalue>> {
    memory::allocate(RefCell::new(ObjUpvalue::new(index)))
}

pub fn new_root_obj_upvalue(index: usize) -> Root<RefCell<ObjUpvalue>> {
    new_gc_obj_upvalue(index).as_root()
}

impl ObjUpvalue {
    fn new(index: usize) -> Self {
        ObjUpvalue::Open(index)
    }

    pub fn is_open(&self) -> bool {
        match self {
            Self::Open(_) => true,
            Self::Closed(_) => false,
        }
    }

    pub fn is_open_with_index(&self, index: usize) -> bool {
        self.is_open_with_pred(|i| i == index)
    }

    pub fn is_open_with_pred(&self, predicate: impl Fn(usize) -> bool) -> bool {
        match self {
            Self::Open(index) => predicate(*index),
            Self::Closed(_) => false,
        }
    }

    pub fn close(&mut self, value: Value) {
        *self = Self::Closed(value);
    }
}

impl memory::GcManaged for ObjUpvalue {
    fn mark(&self) {
        match self {
            ObjUpvalue::Closed(value) => value.mark(),
            ObjUpvalue::Open(_) => {}
        }
    }

    fn blacken(&self) {
        match self {
            ObjUpvalue::Closed(value) => value.blacken(),
            ObjUpvalue::Open(_) => {}
        }
    }
}

#[derive(Clone)]
pub struct ObjFunction {
    pub arity: u32,
    pub upvalue_count: usize,
    pub chunk_index: usize,
    pub name: memory::Gc<ObjString>,
}

pub fn new_gc_obj_function(name: Gc<ObjString>, chunk_index: usize) -> Gc<ObjFunction> {
    memory::allocate(ObjFunction::new(name, chunk_index))
}

pub fn new_root_obj_function(name: Gc<ObjString>, chunk_index: usize) -> Root<ObjFunction> {
    new_gc_obj_function(name, chunk_index).as_root()
}

impl ObjFunction {
    fn new(name: memory::Gc<ObjString>, chunk_index: usize) -> Self {
        ObjFunction {
            arity: 1,
            upvalue_count: 0,
            chunk_index: chunk_index,
            name,
        }
    }
}

impl memory::GcManaged for ObjFunction {
    fn mark(&self) {
        self.name.mark();
    }

    fn blacken(&self) {
        self.name.blacken();
    }
}

impl fmt::Display for ObjFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name.len() {
            0 => write!(f, "<script>"),
            _ => write!(f, "<fn {}>", *self.name),
        }
    }
}

pub type NativeFn = Box<dyn FnMut(&mut [Value]) -> Result<Value, Error>>;

pub struct ObjNative {
    pub function: NativeFn,
}

pub fn new_gc_obj_native(function: NativeFn) -> Gc<ObjNative> {
    memory::allocate(ObjNative::new(function))
}

pub fn new_root_obj_native(function: NativeFn) -> Root<ObjNative> {
    new_gc_obj_native(function).as_root()
}

impl ObjNative {
    fn new(function: NativeFn) -> Self {
        ObjNative { function }
    }
}

impl memory::GcManaged for ObjNative {
    fn mark(&self) {}

    fn blacken(&self) {}
}

impl fmt::Display for ObjNative {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native fn>")
    }
}

pub struct ObjClosure {
    pub function: memory::Gc<ObjFunction>,
    pub upvalues: Vec<memory::Gc<RefCell<ObjUpvalue>>>,
}

pub fn new_gc_obj_closure(function: Gc<ObjFunction>) -> Gc<RefCell<ObjClosure>> {
    let upvalue_roots: Vec<Root<RefCell<ObjUpvalue>>> = (0..function.upvalue_count)
        .map(|_| memory::allocate_root(RefCell::new(ObjUpvalue::new(0))))
        .collect();
    let upvalues = upvalue_roots.iter().map(|u| u.as_gc()).collect();

    memory::allocate(RefCell::new(ObjClosure::new(function, upvalues)))
}

pub fn new_root_obj_closure(function: Gc<ObjFunction>) -> Root<RefCell<ObjClosure>> {
    new_gc_obj_closure(function).as_root()
}

impl ObjClosure {
    fn new(
        function: memory::Gc<ObjFunction>,
        upvalues: Vec<memory::Gc<RefCell<ObjUpvalue>>>,
    ) -> Self {
        ObjClosure { function, upvalues }
    }
}

impl memory::GcManaged for ObjClosure {
    fn mark(&self) {
        self.function.mark();
        self.upvalues.mark();
    }

    fn blacken(&self) {
        self.function.blacken();
        self.upvalues.blacken();
    }
}

impl fmt::Display for ObjClosure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self.function)
    }
}

pub struct ObjClass {
    pub name: memory::Gc<ObjString>,
    pub methods: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

pub fn new_gc_obj_class(name: Gc<ObjString>) -> Gc<RefCell<ObjClass>> {
    memory::allocate(RefCell::new(ObjClass::new(name)))
}

pub fn new_root_obj_class(name: Gc<ObjString>) -> Root<RefCell<ObjClass>> {
    new_gc_obj_class(name).as_root()
}

impl ObjClass {
    fn new(name: memory::Gc<ObjString>) -> Self {
        ObjClass {
            name,
            methods: HashMap::with_hasher(BuildPassThroughHasher::default()),
        }
    }
}

fn add_native_method_to_class(class: Gc<RefCell<ObjClass>>, name: &str, native: NativeFn) {
    let name = new_gc_obj_string(name);
    let obj_native = new_root_obj_native(native);
    class
        .borrow_mut()
        .methods
        .insert(name, Value::ObjNative(obj_native.as_gc()));
}

impl memory::GcManaged for ObjClass {
    fn mark(&self) {
        self.name.mark();
        self.methods.mark();
    }

    fn blacken(&self) {
        self.name.blacken();
        self.methods.blacken();
    }
}

impl fmt::Display for ObjClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self.name)
    }
}

pub struct ObjInstance {
    pub class: memory::Gc<RefCell<ObjClass>>,
    pub fields: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

pub fn new_gc_obj_instance(class: Gc<RefCell<ObjClass>>) -> Gc<RefCell<ObjInstance>> {
    memory::allocate(RefCell::new(ObjInstance::new(class)))
}

pub fn new_root_obj_instance(class: Gc<RefCell<ObjClass>>) -> Root<RefCell<ObjInstance>> {
    new_gc_obj_instance(class).as_root()
}

impl ObjInstance {
    fn new(class: Gc<RefCell<ObjClass>>) -> Self {
        ObjInstance {
            class,
            fields: HashMap::with_hasher(BuildPassThroughHasher::default()),
        }
    }
}

impl memory::GcManaged for ObjInstance {
    fn mark(&self) {
        self.class.mark();
        self.fields.mark();
    }

    fn blacken(&self) {
        self.class.blacken();
        self.fields.blacken();
    }
}

impl fmt::Display for ObjInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} instance", *self.class.borrow())
    }
}

pub struct ObjBoundMethod<T: memory::GcManaged> {
    pub receiver: Value,
    pub method: memory::Gc<T>,
}

pub fn new_gc_obj_bound_method<T: 'static + memory::GcManaged>(
    receiver: Value,
    method: Gc<T>,
) -> Gc<RefCell<ObjBoundMethod<T>>> {
    memory::allocate(RefCell::new(ObjBoundMethod::new(receiver, method)))
}

pub fn new_root_obj_bound_method<T: 'static + memory::GcManaged>(
    receiver: Value,
    method: Gc<T>,
) -> Root<RefCell<ObjBoundMethod<T>>> {
    new_gc_obj_bound_method(receiver, method).as_root()
}

impl<T: memory::GcManaged> ObjBoundMethod<T> {
    fn new(receiver: Value, method: memory::Gc<T>) -> Self {
        ObjBoundMethod { receiver, method }
    }
}

impl<T: 'static + memory::GcManaged> memory::GcManaged for ObjBoundMethod<T> {
    fn mark(&self) {
        self.receiver.mark();
        self.method.mark();
    }

    fn blacken(&self) {
        self.receiver.mark();
        self.method.blacken();
    }
}

impl fmt::Display for ObjBoundMethod<RefCell<ObjClosure>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self.method.borrow())
    }
}

impl fmt::Display for ObjBoundMethod<ObjNative> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self.method)
    }
}

pub struct ObjVec {
    pub class: Gc<RefCell<ObjClass>>,
    pub elements: Vec<Value>,
}

pub fn new_gc_obj_vec(class: Gc<RefCell<ObjClass>>) -> Gc<RefCell<ObjVec>> {
    memory::allocate(RefCell::new(ObjVec::new(class)))
}

pub fn new_root_obj_vec(class: Gc<RefCell<ObjClass>>) -> Root<RefCell<ObjVec>> {
    new_gc_obj_vec(class).as_root()
}

pub fn new_root_obj_vec_class() -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string("Vec");
    let class = new_root_obj_class(class_name);
    add_native_method_to_class(class.as_gc(), "init", Box::new(vec_init));
    add_native_method_to_class(class.as_gc(), "push", Box::new(vec_push));
    add_native_method_to_class(class.as_gc(), "pop", Box::new(vec_pop));
    add_native_method_to_class(class.as_gc(), "get", Box::new(vec_get));
    add_native_method_to_class(class.as_gc(), "set", Box::new(vec_set));
    add_native_method_to_class(class.as_gc(), "len", Box::new(vec_len));
    class
}

impl ObjVec {
    fn new(class: Gc<RefCell<ObjClass>>) -> Self {
        ObjVec {
            class,
            elements: Vec::new(),
        }
    }
}

impl memory::GcManaged for ObjVec {
    fn mark(&self) {
        self.class.mark();
        self.elements.mark();
    }

    fn blacken(&self) {
        self.class.blacken();
        self.elements.blacken();
    }
}

impl fmt::Display for ObjVec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        let num_elems = self.elements.len();
        for (i, e) in self.elements.iter().enumerate() {
            let is_self = match e {
                Value::ObjVec(v) => &(*v.borrow()) as *const _ == self as *const _,
                _ => false,
            };
            if is_self {
                write!(f, "[...]")?;
            } else {
                write!(f, "{}", e)?;
            }
            write!(f, "{}", if i == num_elems - 1 { "" } else { ", " })?;
        }
        write!(f, "]")
    }
}

impl cmp::PartialEq for ObjVec {
    fn eq(&self, other: &ObjVec) -> bool {
        if self as *const _ == other as *const _ {
            return true;
        }
        self.elements == other.elements
    }
}

fn vec_init(args: &mut [Value]) -> Result<Value, Error> {
    let class = if let Value::ObjInstance(c) = args[0] {
        c.borrow().class
    } else {
        unreachable!()
    };
    let vec = new_root_obj_vec(class);
    vec.borrow_mut().elements = args.iter().skip(1).map(|v| *v).collect();
    Ok(Value::ObjVec(vec.as_gc()))
}

fn vec_push(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 2 {
        let msg = format!("Expected 1 parameter but got {}", args.len() - 1);
        return Err(Error::with_message(ErrorKind::RuntimeError, msg.as_str()));
    }

    let vec = if let Value::ObjVec(v) = args[0] {
        v
    } else {
        unreachable!()
    };
    vec.borrow_mut().elements.push(args[1]);

    Ok(args[0])
}

fn vec_pop(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 1 {
        let msg = format!("Expected 0 parameters but got {}", args.len() - 1);
        return Err(Error::with_message(ErrorKind::RuntimeError, msg.as_str()));
    }

    let vec = if let Value::ObjVec(v) = args[0] {
        v
    } else {
        unreachable!()
    };
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements.pop().ok_or(Error::with_message(
        ErrorKind::RuntimeError,
        "Cannot pop from empty Vec instance.",
    ))
}

fn vec_get(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 2 {
        let msg = format!("Expected 1 parameters but got {}", args.len() - 1);
        return Err(Error::with_message(ErrorKind::RuntimeError, msg.as_str()));
    }

    let vec = if let Value::ObjVec(v) = args[0] {
        v
    } else {
        unreachable!()
    };

    let index = get_vec_index(vec, args[1])?;
    let borrowed_vec = vec.borrow();
    Ok(borrowed_vec.elements[index])
}

fn vec_set(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 3 {
        let msg = format!("Expected 2 parameters but got {}", args.len() - 1);
        return Err(Error::with_message(ErrorKind::RuntimeError, msg.as_str()));
    }

    let vec = if let Value::ObjVec(v) = args[0] {
        v
    } else {
        unreachable!()
    };

    let index = get_vec_index(vec, args[1])?;
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements[index] = args[2];
    Ok(Value::None)
}

fn vec_len(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 1 {
        let msg = format!("Expected 0 parameters but got {}", args.len() - 1);
        return Err(Error::with_message(ErrorKind::RuntimeError, msg.as_str()));
    }

    let vec = if let Value::ObjVec(v) = args[0] {
        v
    } else {
        unreachable!()
    };

    let borrowed_vec = vec.borrow();
    Ok(Value::from(borrowed_vec.elements.len() as f64))
}

fn get_vec_index(vec: Gc<RefCell<ObjVec>>, value: Value) -> Result<usize, Error> {
    let vec_len = vec.borrow_mut().elements.len();
    let index = if let Value::Number(n) = value {
        if n >= 0.0 {
            if n.floor() != n {
                let msg = format!("Expected an integer but found '{}'.", n);
                return Err(Error::with_message(ErrorKind::ValueError, msg.as_str()));
            }
            n as usize
        } else {
            if n.ceil() != n {
                let msg = format!("Expected an integer but found '{}'.", n);
                return Err(Error::with_message(ErrorKind::ValueError, msg.as_str()));
            }
            ((n as isize + vec_len as isize) % vec_len as isize) as usize
        }
    } else {
        let msg = format!("Expected an integer but found '{}'.", value);
        return Err(Error::with_message(ErrorKind::ValueError, msg.as_str()));
    };

    if index >= vec_len {
        return Err(Error::with_message(
            ErrorKind::ValueError,
            "Vec index parameter out of bounds",
        ));
    }

    Ok(index)
}
