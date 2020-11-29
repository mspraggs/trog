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

use crate::class_store::CoreClassStore;
use crate::common;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, FnvHasher};
use crate::memory::{self, Gc, Heap, Root};
use crate::value::Value;

type ObjStringCache = RefCell<HashMap<u64, ManuallyDrop<Root<ObjString>>>>;

pub struct ObjString {
    string: String,
    pub(crate) hash: u64,
}

pub fn new_gc_obj_string(heap: &mut Heap, data: &str) -> Gc<ObjString> {
    // The use of ManuallyDrop here is sadly necessary, otherwise the Root may try
    // to read memory that's been free'd when the Root is dropped.
    thread_local! {
        static OBJ_STRING_CACHE: ObjStringCache = RefCell::new(HashMap::new());
    }

    let hash = {
        let mut hasher = FnvHasher::new();
        (*data).hash(&mut hasher);
        hasher.finish()
    };
    if let Some(gc_string) =
        OBJ_STRING_CACHE.with(|cache| cache.borrow().get(&hash).map(|v| v.as_gc()))
    {
        return gc_string;
    }
    let ret = heap.allocate(ObjString::new(data, hash));
    OBJ_STRING_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .insert(hash, ManuallyDrop::new(ret.as_root()))
    });
    ret
}

pub fn new_root_obj_string(heap: &mut Heap, data: &str) -> Root<ObjString> {
    new_gc_obj_string(heap, data).as_root()
}

impl ObjString {
    fn new(string: &str, hash: u64) -> Self {
        ObjString {
            string: String::from(string),
            hash,
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

pub fn new_gc_obj_upvalue(heap: &mut Heap, index: usize) -> Gc<RefCell<ObjUpvalue>> {
    heap.allocate(RefCell::new(ObjUpvalue::new(index)))
}

pub fn new_root_obj_upvalue(heap: &mut Heap, index: usize) -> Root<RefCell<ObjUpvalue>> {
    new_gc_obj_upvalue(heap, index).as_root()
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
    heap: &mut Heap,
    name: Gc<ObjString>,
    arity: u32,
    upvalue_count: usize,
    chunk_index: usize,
) -> Gc<ObjFunction> {
    heap.allocate(ObjFunction::new(name, arity, upvalue_count, chunk_index))
}

pub fn new_root_obj_function(
    heap: &mut Heap,
    name: Gc<ObjString>,
    arity: u32,
    upvalue_count: usize,
    chunk_index: usize,
) -> Root<ObjFunction> {
    new_gc_obj_function(heap, name, arity, upvalue_count, chunk_index).as_root()
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
            chunk_index,
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

pub type NativeFn = fn(&mut Heap, &CoreClassStore, &mut [Value]) -> Result<Value, Error>;

pub struct ObjNative {
    pub function: NativeFn,
}

pub fn new_gc_obj_native(heap: &mut Heap, function: NativeFn) -> Gc<ObjNative> {
    heap.allocate(ObjNative::new(function))
}

pub fn new_root_obj_native(heap: &mut Heap, function: NativeFn) -> Root<ObjNative> {
    new_gc_obj_native(heap, function).as_root()
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

pub fn new_gc_obj_closure(heap: &mut Heap, function: Gc<ObjFunction>) -> Gc<RefCell<ObjClosure>> {
    let upvalue_roots: Vec<Root<RefCell<ObjUpvalue>>> = (0..function.upvalue_count)
        .map(|_| heap.allocate_root(RefCell::new(ObjUpvalue::new(0))))
        .collect();
    let upvalues = upvalue_roots.iter().map(|u| u.as_gc()).collect();

    heap.allocate(RefCell::new(ObjClosure::new(function, upvalues)))
}

pub fn new_root_obj_closure(
    heap: &mut Heap,
    function: Gc<ObjFunction>,
) -> Root<RefCell<ObjClosure>> {
    new_gc_obj_closure(heap, function).as_root()
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

pub fn new_gc_obj_class(heap: &mut Heap, name: Gc<ObjString>) -> Gc<RefCell<ObjClass>> {
    heap.allocate(RefCell::new(ObjClass::new(name)))
}

pub fn new_root_obj_class(heap: &mut Heap, name: Gc<ObjString>) -> Root<RefCell<ObjClass>> {
    new_gc_obj_class(heap, name).as_root()
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
            self.methods.insert(*name, *value);
        }
    }
}

fn add_native_method_to_class(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
    name: &str,
    native: NativeFn,
) {
    let name = new_gc_obj_string(heap, name);
    let obj_native = new_root_obj_native(heap, native);
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

pub fn new_gc_obj_instance(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
) -> Gc<RefCell<ObjInstance>> {
    heap.allocate(RefCell::new(ObjInstance::new(class)))
}

pub fn new_root_obj_instance(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
) -> Root<RefCell<ObjInstance>> {
    new_gc_obj_instance(heap, class).as_root()
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
    heap: &mut Heap,
    receiver: Value,
    method: Gc<T>,
) -> Gc<RefCell<ObjBoundMethod<T>>> {
    heap.allocate(RefCell::new(ObjBoundMethod::new(receiver, method)))
}

pub fn new_root_obj_bound_method<T: 'static + memory::GcManaged>(
    heap: &mut Heap,
    receiver: Value,
    method: Gc<T>,
) -> Root<RefCell<ObjBoundMethod<T>>> {
    new_gc_obj_bound_method(heap, receiver, method).as_root()
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

pub fn new_gc_obj_vec(heap: &mut Heap, class: Gc<RefCell<ObjClass>>) -> Gc<RefCell<ObjVec>> {
    heap.allocate(RefCell::new(ObjVec::new(class)))
}

pub fn new_root_obj_vec(heap: &mut Heap, class: Gc<RefCell<ObjClass>>) -> Root<RefCell<ObjVec>> {
    new_gc_obj_vec(heap, class).as_root()
}

pub fn new_root_obj_vec_class(
    heap: &mut Heap,
    iter_class: Gc<RefCell<ObjClass>>,
) -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string(heap, "Vec");
    let class = new_root_obj_class(heap, class_name);
    add_native_method_to_class(heap, class.as_gc(), "__init__", vec_init);
    add_native_method_to_class(heap, class.as_gc(), "push", vec_push);
    add_native_method_to_class(heap, class.as_gc(), "pop", vec_pop);
    add_native_method_to_class(heap, class.as_gc(), "__getitem__", vec_get);
    add_native_method_to_class(heap, class.as_gc(), "__setitem__", vec_set);
    add_native_method_to_class(heap, class.as_gc(), "len", vec_len);
    add_native_method_to_class(heap, class.as_gc(), "__iter__", vec_iter);
    class.borrow_mut().add_superclass(iter_class);
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

fn vec_init(
    heap: &mut Heap,
    class_store: &CoreClassStore,
    _args: &mut [Value],
) -> Result<Value, Error> {
    let vec = new_root_obj_vec(heap, class_store.get_obj_vec_class());
    Ok(Value::ObjVec(vec.as_gc()))
}

fn vec_push(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
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

fn vec_get(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
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

fn vec_set(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
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
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}.",
            args.len() - 1
        );
    }

    let iter = new_root_obj_vec_iter(
        heap,
        class_store.get_obj_vec_iter_class(),
        args[0].try_as_obj_vec().expect("Expected ObjVec instance."),
    );
    Ok(Value::ObjVecIter(iter.as_gc()))
}

pub struct ObjVecIter {
    pub class: Gc<RefCell<ObjClass>>,
    pub iterable: Gc<RefCell<ObjVec>>,
    pub current: usize,
}

pub fn new_gc_obj_vec_iter(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
    vec: Gc<RefCell<ObjVec>>,
) -> Gc<RefCell<ObjVecIter>> {
    heap.allocate(RefCell::new(ObjVecIter::new(class, vec)))
}

pub fn new_root_obj_vec_iter(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
    vec: Gc<RefCell<ObjVec>>,
) -> Root<RefCell<ObjVecIter>> {
    new_gc_obj_vec_iter(heap, class, vec).as_root()
}

impl ObjVecIter {
    fn new(class: Gc<RefCell<ObjClass>>, iterable: Gc<RefCell<ObjVec>>) -> Self {
        ObjVecIter {
            class,
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

fn vec_iter_next(
    _heap: &mut Heap,
    _context: &CoreClassStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_vec_iter()
        .expect("Expected ObjVecIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

pub fn new_root_obj_vec_iter_class(heap: &mut Heap) -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string(heap, "VecIter");
    let class = new_root_obj_class(heap, class_name);
    add_native_method_to_class(heap, class.as_gc(), "__next__", vec_iter_next);
    class
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

pub struct ObjRange {
    pub class: Gc<RefCell<ObjClass>>,
    pub begin: isize,
    pub end: isize,
}

pub fn new_gc_obj_range(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
    begin: isize,
    end: isize,
) -> Gc<ObjRange> {
    heap.allocate(ObjRange::new(class, begin, end))
}

pub fn new_root_obj_range(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
    begin: isize,
    end: isize,
) -> Root<ObjRange> {
    new_gc_obj_range(heap, class, begin, end).as_root()
}

pub fn new_root_obj_range_class(
    heap: &mut Heap,
    iter_class: Gc<RefCell<ObjClass>>,
) -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string(heap, "Range");
    let class = new_root_obj_class(heap, class_name);
    add_native_method_to_class(heap, class.as_gc(), "__init__", range_init);
    add_native_method_to_class(heap, class.as_gc(), "__iter__", range_iter);
    class.borrow_mut().add_superclass(iter_class);
    class
}

impl ObjRange {
    fn new(class: Gc<RefCell<ObjClass>>, begin: isize, end: isize) -> Self {
        ObjRange { class, begin, end }
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

fn range_init(
    heap: &mut Heap,
    class_store: &CoreClassStore,
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
        bounds[i] = validate_integer(args[i + 1])?;
    }
    let range = new_root_obj_range(
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
    args: &mut [Value],
) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected 0 parameters but got {}.",
            args.len() - 1
        );
    }

    let iter = new_root_obj_range_iter(
        heap,
        class_store.get_obj_range_iter_class(),
        args[0]
            .try_as_obj_range()
            .expect("Expected ObjRange instance."),
    );
    Ok(Value::ObjRangeIter(iter.as_gc()))
}

pub(crate) fn validate_integer(value: Value) -> Result<isize, Error> {
    match value {
        Value::Number(n) => {
            #[allow(clippy::float_cmp)]
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

pub fn new_gc_obj_range_iter(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
    range: Gc<ObjRange>,
) -> Gc<RefCell<ObjRangeIter>> {
    heap.allocate(RefCell::new(ObjRangeIter::new(class, range)))
}

pub fn new_root_obj_range_iter(
    heap: &mut Heap,
    class: Gc<RefCell<ObjClass>>,
    range: Gc<ObjRange>,
) -> Root<RefCell<ObjRangeIter>> {
    new_gc_obj_range_iter(heap, class, range).as_root()
}

impl ObjRangeIter {
    fn new(class: Gc<RefCell<ObjClass>>, iterable: Gc<ObjRange>) -> Self {
        let current = iterable.begin;
        ObjRangeIter {
            class,
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

fn range_iter_next(
    _heap: &mut Heap,
    _context: &CoreClassStore,
    args: &mut [Value],
) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_range_iter()
        .expect("Expected ObjIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

pub fn new_root_obj_range_iter_class(heap: &mut Heap) -> Root<RefCell<ObjClass>> {
    let class_name = new_gc_obj_string(heap, "IterVec");
    let class = new_root_obj_class(heap, class_name);
    add_native_method_to_class(heap, class.as_gc(), "__next__", range_iter_next);
    class
}

pub(crate) mod classes {}
