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
use std::sync::Once;

use crate::common;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, FnvHasher};
use crate::memory::{self, Gc, Root};
use crate::value::Value;
use crate::vm;

include!(concat!(env!("OUT_DIR"), "/core.yrl.rs"));

type ObjStringCache = Root<RefCell<HashMap<u64, Gc<ObjString>>>>;

// The use of ManuallyDrop here is sadly necessary, otherwise the Root may try
// to read memory that's been free'd when the Root is dropped.
thread_local! {
    static OBJ_STRING_CACHE: ManuallyDrop<ObjStringCache> =
        ManuallyDrop::new(memory::allocate_root(RefCell::new(HashMap::new())));
}

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

    pub fn as_str(&self) -> &str {
        self.string.as_str()
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

pub fn new_gc_obj_function(
    name: Gc<ObjString>,
    arity: u32,
    upvalue_count: usize,
    chunk_index: usize,
) -> Gc<ObjFunction> {
    memory::allocate(ObjFunction::new(name, arity, upvalue_count, chunk_index))
}

pub fn new_root_obj_function(
    name: Gc<ObjString>,
    arity: u32,
    upvalue_count: usize,
    chunk_index: usize,
) -> Root<ObjFunction> {
    new_gc_obj_function(name, arity, upvalue_count, chunk_index).as_root()
}

impl ObjFunction {
    fn new(
        name: memory::Gc<ObjString>,
        arity: u32,
        upvalue_count: usize,
        chunk_index: usize,
    ) -> Self {
        ObjFunction {
            arity,
            upvalue_count,
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

//Box<dyn FnMut(&mut [Value]) -> Result<Value, Error>>;
pub type NativeFn = fn(&mut [Value]) -> Result<Value, Error>;

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

    pub fn add_superclass(&mut self, superclass: memory::Gc<RefCell<ObjClass>>) {
        for (name, value) in superclass.borrow().methods.iter() {
            self.methods.insert(name.clone(), *value);
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

impl fmt::Display for ObjBoundMethod<ObjNative> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self.method)
    }
}

impl fmt::Display for ObjBoundMethod<RefCell<ObjClosure>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.method.borrow())
    }
}

pub struct ObjVec {
    pub class: Gc<RefCell<ObjClass>>,
    pub elements: Vec<Value>,
}

pub fn new_gc_obj_vec() -> Gc<RefCell<ObjVec>> {
    memory::allocate(RefCell::new(ObjVec::new()))
}

pub fn new_root_obj_vec() -> Root<RefCell<ObjVec>> {
    new_gc_obj_vec().as_root()
}

pub fn new_root_obj_vec_class() -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string("Vec");
    let class = new_root_obj_class(class_name);
    add_native_method_to_class(class.as_gc(), "__init__", vec_init);
    add_native_method_to_class(class.as_gc(), "push", vec_push);
    add_native_method_to_class(class.as_gc(), "pop", vec_pop);
    add_native_method_to_class(class.as_gc(), "__getitem__", vec_get);
    add_native_method_to_class(class.as_gc(), "__setitem__", vec_set);
    add_native_method_to_class(class.as_gc(), "len", vec_len);
    add_native_method_to_class(class.as_gc(), "__iter__", vec_iter);
    class
        .borrow_mut()
        .add_superclass(classes::get_gc_obj_iter_class());
    class
}

impl ObjVec {
    fn new() -> Self {
        ObjVec {
            class: classes::get_gc_obj_vec_class(),
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

fn vec_init(_args: &mut [Value]) -> Result<Value, Error> {
    let vec = new_root_obj_vec();
    Ok(Value::ObjVec(vec.as_gc()))
}

fn vec_push(args: &mut [Value]) -> Result<Value, Error> {
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

fn vec_pop(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}",
            args.len() - 1
        );
    }

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
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

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");

    let index = get_vec_index(vec, args[1])?;
    let borrowed_vec = vec.borrow();
    Ok(borrowed_vec.elements[index])
}

fn vec_set(args: &mut [Value]) -> Result<Value, Error> {
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

fn vec_len(args: &mut [Value]) -> Result<Value, Error> {
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

fn vec_iter(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}.",
            args.len() - 1
        );
    }

    let iter = new_root_obj_vec_iter(args[0].try_as_obj_vec().expect("Expected ObjVec instance."));
    Ok(Value::ObjVecIter(iter.as_gc()))
}

pub struct ObjVecIter {
    pub class: Gc<RefCell<ObjClass>>,
    pub iterable: Gc<RefCell<ObjVec>>,
    pub current: usize,
}

pub fn new_gc_obj_vec_iter(vec: Gc<RefCell<ObjVec>>) -> Gc<RefCell<ObjVecIter>> {
    memory::allocate(RefCell::new(ObjVecIter::new(vec)))
}

pub fn new_root_obj_vec_iter(vec: Gc<RefCell<ObjVec>>) -> Root<RefCell<ObjVecIter>> {
    new_gc_obj_vec_iter(vec).as_root()
}

impl ObjVecIter {
    fn new(iterable: Gc<RefCell<ObjVec>>) -> Self {
        ObjVecIter {
            class: classes::get_gc_obj_vec_iter_class(),
            iterable,
            current: 0,
        }
    }

    fn next(&mut self) -> Value {
        let borrowed_vec = self.iterable.borrow();
        if self.current >= borrowed_vec.elements.len() {
            return Value::Sentinel;
        }
        let ret = borrowed_vec.elements[self.current];
        self.current += 1;
        ret
    }
}

impl memory::GcManaged for ObjVecIter {
    fn mark(&self) {
        self.iterable.mark();
    }

    fn blacken(&self) {
        self.iterable.blacken();
    }
}

impl fmt::Display for ObjVecIter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjVecIter instance")
    }
}

fn vec_iter_next(args: &mut [Value]) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_vec_iter()
        .expect("Expected ObjVecIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

pub fn new_root_obj_vec_iter_class() -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string("VecIter");
    let class = new_root_obj_class(class_name);
    add_native_method_to_class(class.as_gc(), "__next__", vec_iter_next);
    class
}

fn get_vec_index(vec: Gc<RefCell<ObjVec>>, value: Value) -> Result<usize, Error> {
    let vec_len = vec.borrow_mut().elements.len() as isize;
    let index = if let Value::Number(n) = value {
        if n.trunc() != n {
            return error!(
                ErrorKind::ValueError,
                "Expected an integer but found '{}'.", n
            );
        }
        let n = if n < 0.0 {
            n as isize + vec_len
        } else {
            n as isize
        };
        n
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

pub struct ObjRange {
    pub class: Gc<RefCell<ObjClass>>,
    pub begin: isize,
    pub end: isize,
}

pub fn new_gc_obj_range(begin: isize, end: isize) -> Gc<ObjRange> {
    memory::allocate(ObjRange::new(begin, end))
}

pub fn new_root_obj_range(begin: isize, end: isize) -> Root<ObjRange> {
    new_gc_obj_range(begin, end).as_root()
}

pub fn new_root_obj_range_class() -> Root<RefCell<ObjClass>> {
    classes::get_gc_obj_iter_class();
    let class_name = new_gc_obj_string("Range");
    let class = new_root_obj_class(class_name);
    add_native_method_to_class(class.as_gc(), "__init__", range_init);
    add_native_method_to_class(class.as_gc(), "__iter__", range_iter);
    class
        .borrow_mut()
        .add_superclass(classes::get_gc_obj_iter_class());
    class
}

impl ObjRange {
    fn new(begin: isize, end: isize) -> Self {
        ObjRange {
            class: classes::get_gc_obj_range_class(),
            begin,
            end,
        }
    }
}

impl memory::GcManaged for ObjRange {
    fn mark(&self) {
        self.class.mark();
    }

    fn blacken(&self) {
        self.class.blacken();
    }
}

impl fmt::Display for ObjRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Range({}, {})", self.begin, self.end)
    }
}

fn range_init(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 3 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 2 parameters but got {}",
            args.len() - 1
        );
    }
    let mut bounds: [isize; 2] = [0; 2];
    for i in 0..2 {
        bounds[i] = validate_integer(args[i + 1])?;
    }
    let range = new_root_obj_range(bounds[0], bounds[1]);
    Ok(Value::ObjRange(range.as_gc()))
}

fn range_iter(args: &mut [Value]) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}.",
            args.len() - 1
        );
    }

    let iter = new_root_obj_range_iter(
        args[0]
            .try_as_obj_range()
            .expect("Expected ObjRange instance."),
    );
    Ok(Value::ObjRangeIter(iter.as_gc()))
}

pub(crate) fn validate_integer(value: Value) -> Result<isize, Error> {
    match value {
        Value::Number(n) => {
            if n.trunc() != n {
                return error!(ErrorKind::ValueError, "Expected integer value.");
            }
            Ok(n as isize)
        }
        _ => return error!(ErrorKind::TypeError, "Expected integer value."),
    }
}

pub struct ObjRangeIter {
    pub class: Gc<RefCell<ObjClass>>,
    pub iterable: Gc<ObjRange>,
    current: isize,
    step: isize,
}

pub fn new_gc_obj_range_iter(range: Gc<ObjRange>) -> Gc<RefCell<ObjRangeIter>> {
    memory::allocate(RefCell::new(ObjRangeIter::new(range)))
}

pub fn new_root_obj_range_iter(range: Gc<ObjRange>) -> Root<RefCell<ObjRangeIter>> {
    new_gc_obj_range_iter(range).as_root()
}

impl ObjRangeIter {
    fn new(iterable: Gc<ObjRange>) -> Self {
        let current = iterable.begin;
        ObjRangeIter {
            class: classes::get_gc_obj_range_iter_class(),
            iterable,
            current,
            step: if iterable.begin < iterable.end { 1 } else { -1 },
        }
    }

    fn next(&mut self) -> Value {
        if self.current == self.iterable.end {
            return Value::Sentinel;
        }
        let ret = Value::Number(self.current as f64);
        self.current += self.step;
        ret
    }
}

impl memory::GcManaged for ObjRangeIter {
    fn mark(&self) {
        self.iterable.mark();
    }

    fn blacken(&self) {
        self.iterable.blacken();
    }
}

impl fmt::Display for ObjRangeIter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjRangeIter instance")
    }
}

fn range_iter_next(args: &mut [Value]) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_range_iter()
        .expect("Expected ObjIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

pub fn new_root_obj_range_iter_class() -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string("IterVec");
    let class = new_root_obj_class(class_name);
    add_native_method_to_class(class.as_gc(), "__next__", range_iter_next);
    class
}

pub(crate) mod classes {
    use super::*;

    thread_local! {
        static ROOT_OBJ_ITER_CLASS: RefCell<Option<ManuallyDrop<Root<RefCell<ObjClass>>>>> =
            RefCell::new(None);

        static ROOT_OBJ_MAP_ITER_CLASS: RefCell<Option<ManuallyDrop<Root<RefCell<ObjClass>>>>> =
            RefCell::new(None);

        static ROOT_OBJ_VEC_CLASS: ManuallyDrop<Root<RefCell<ObjClass>>> =
            ManuallyDrop::new(new_root_obj_vec_class());

        static ROOT_OBJ_VEC_ITER_CLASS: ManuallyDrop<Root<RefCell<ObjClass>>> =
            ManuallyDrop::new(new_root_obj_vec_iter_class());

        static ROOT_OBJ_RANGE_CLASS: ManuallyDrop<Root<RefCell<ObjClass>>> =
            ManuallyDrop::new(new_root_obj_range_class());

        static ROOT_OBJ_RANGE_ITER_CLASS: ManuallyDrop<Root<RefCell<ObjClass>>> =
            ManuallyDrop::new(new_root_obj_range_iter_class());
    }

    static START: Once = Once::new();
    fn init_core_static_classes() {
        START.call_once(|| {
            let mut vm = vm::new_root_vm();
            let source = String::from(CORE_SOURCE);
            let result = vm::interpret(&mut vm, source);
            match result {
                Ok(_) => {}
                Err(error) => eprint!("{}", error),
            }
            ROOT_OBJ_ITER_CLASS.with(|v| {
                *v.borrow_mut() = Some(ManuallyDrop::new(
                    vm.get_global("Iter")
                        .unwrap()
                        .try_as_obj_class()
                        .expect("Expected ObjClass.")
                        .as_root(),
                ));
            });
            ROOT_OBJ_MAP_ITER_CLASS.with(|v| {
                *v.borrow_mut() = Some(ManuallyDrop::new(
                    vm.get_global("MapIter")
                        .unwrap()
                        .try_as_obj_class()
                        .expect("Expected ObjClass.")
                        .as_root(),
                ));
            });
        });
    }

    pub(crate) fn get_gc_obj_iter_class() -> Gc<RefCell<ObjClass>> {
        init_core_static_classes();
        ROOT_OBJ_ITER_CLASS.with(|c| c.borrow().as_ref().unwrap().as_gc())
    }

    pub(crate) fn get_gc_obj_map_iter_class() -> Gc<RefCell<ObjClass>> {
        init_core_static_classes();
        ROOT_OBJ_MAP_ITER_CLASS.with(|c| c.borrow().as_ref().unwrap().as_gc())
    }

    pub(crate) fn get_gc_obj_vec_class() -> Gc<RefCell<ObjClass>> {
        init_core_static_classes();
        ROOT_OBJ_VEC_CLASS.with(|c| c.as_gc())
    }

    pub(crate) fn get_gc_obj_vec_iter_class() -> Gc<RefCell<ObjClass>> {
        init_core_static_classes();
        ROOT_OBJ_VEC_ITER_CLASS.with(|c| c.as_gc())
    }

    pub(crate) fn get_gc_obj_range_class() -> Gc<RefCell<ObjClass>> {
        init_core_static_classes();
        ROOT_OBJ_RANGE_CLASS.with(|c| c.as_gc())
    }

    pub(crate) fn get_gc_obj_range_iter_class() -> Gc<RefCell<ObjClass>> {
        init_core_static_classes();
        ROOT_OBJ_RANGE_ITER_CLASS.with(|c| c.as_gc())
    }
}
