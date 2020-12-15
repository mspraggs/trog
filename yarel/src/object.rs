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
use std::ops::Deref;
use std::rc::Rc;

use crate::class_store::CoreClassStore;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, FnvHasher};
use crate::memory::{self, Gc, Heap, Root};
use crate::value::Value;

pub struct ObjString {
    pub(crate) class: Gc<RefCell<ObjClass>>,
    string: String,
    hash: u64,
}

impl ObjString {
    fn new(class: Gc<RefCell<ObjClass>>, string: &str, hash: u64) -> Self {
        ObjString {
            class,
            string: String::from(string),
            hash,
        }
    }

    pub fn as_str(&self) -> &str {
        self.string.as_str()
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

impl Deref for ObjString {
    type Target = str;

    fn deref(&self) -> &str {
        self.string.as_str()
    }
}

impl memory::GcManaged for ObjString {
    fn mark(&self) {}

    fn blacken(&self) {}
}

pub struct ObjStringStore {
    class: Root<RefCell<ObjClass>>,
    store: HashMap<u64, Root<ObjString>>,
}

pub fn new_obj_string_store(heap: &mut Heap) -> Rc<RefCell<ObjStringStore>> {
    let class = new_root_obj_class_anon(heap);
    let store = Rc::new(RefCell::new(ObjStringStore::new(class.clone())));
    let name = store.borrow_mut().new_gc_obj_string(heap, "String");
    class.borrow_mut().name = Some(name);
    store
}

impl ObjStringStore {
    pub fn new(class: Root<RefCell<ObjClass>>) -> Self {
        ObjStringStore {
            class,
            store: HashMap::new(),
        }
    }

    pub(crate) fn get_obj_string_class(&self) -> Gc<RefCell<ObjClass>> {
        self.class.as_gc()
    }

    pub fn new_gc_obj_string(&mut self, heap: &mut Heap, data: &str) -> Gc<ObjString> {
        let hash = {
            let mut hasher = FnvHasher::new();
            (*data).hash(&mut hasher);
            hasher.finish()
        };
        if let Some(gc_string) = self.store.get(&hash).map(|v| v.as_gc()) {
            return gc_string;
        }
        let ret = heap.allocate(ObjString::new(self.class.as_gc(), data, hash));
        self.store.insert(hash, ret.as_root());
        ret
    }
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

pub type NativeFn = fn(
    &mut Heap,
    &CoreClassStore,
    string_store: &mut ObjStringStore,
    &mut [Value],
) -> Result<Value, Error>;

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
    pub name: Option<memory::Gc<ObjString>>,
    pub methods: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

pub fn new_gc_obj_class(heap: &mut Heap, name: Gc<ObjString>) -> Gc<RefCell<ObjClass>> {
    heap.allocate(RefCell::new(ObjClass::with_name(name)))
}

pub fn new_root_obj_class(heap: &mut Heap, name: Gc<ObjString>) -> Root<RefCell<ObjClass>> {
    new_gc_obj_class(heap, name).as_root()
}

pub fn new_root_obj_class_anon(heap: &mut Heap) -> Root<RefCell<ObjClass>> {
    heap.allocate(RefCell::new(ObjClass::new())).as_root()
}

impl ObjClass {
    fn new() -> Self {
        ObjClass {
            name: None,
            methods: HashMap::with_hasher(BuildPassThroughHasher::default()),
        }
    }

    fn with_name(name: memory::Gc<ObjString>) -> Self {
        ObjClass {
            name: Some(name),
            methods: HashMap::with_hasher(BuildPassThroughHasher::default()),
        }
    }

    pub fn add_superclass(&mut self, superclass: memory::Gc<RefCell<ObjClass>>) {
        for (name, value) in superclass.borrow().methods.iter() {
            self.methods.insert(*name, *value);
        }
    }
}

impl memory::GcManaged for ObjClass {
    fn mark(&self) {
        self.methods.mark();
    }

    fn blacken(&self) {
        self.methods.blacken();
    }
}

impl fmt::Display for ObjClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self.name.unwrap())
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

    pub(crate) fn next(&mut self) -> Value {
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

impl ObjRange {
    fn new(class: Gc<RefCell<ObjClass>>, begin: isize, end: isize) -> Self {
        ObjRange { class, begin, end }
    }

    pub(crate) fn get_bounded_range(
        &self,
        limit: isize,
        type_name: &str,
    ) -> Result<(usize, usize), Error> {
        let begin = if self.begin < 0 {
            self.begin + limit
        } else {
            self.begin
        };
        if begin < 0 || begin >= limit {
            return error!(
                ErrorKind::IndexError,
                "{} slice start out of range.", type_name
            );
        }
        let end = if self.end < 0 {
            self.end + limit
        } else {
            self.end
        };
        if end < 0 || end > limit {
            return error!(
                ErrorKind::IndexError,
                "{} slice end out of range.", type_name
            );
        }
        Ok((
            begin as usize,
            if end >= begin { end } else { begin } as usize,
        ))
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

    pub(crate) fn next(&mut self) -> Value {
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
