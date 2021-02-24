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

use std::cell::{Cell, RefCell};
use std::cmp::{self, Eq};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::ptr;

use crate::chunk::Chunk;
use crate::common;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, PassThroughHasher};
use crate::memory::{self, Gc, GcManaged, Root};
use crate::stack::Stack;
use crate::value::Value;
use crate::vm::Vm;

pub struct ObjString {
    pub(crate) class: Gc<ObjClass>,
    string: String,
    pub(crate) hash: u64,
}

impl ObjString {
    pub(crate) fn new(class: Gc<ObjClass>, string: &str, hash: u64) -> Self {
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

pub type ObjStringValueMap = HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>;

pub fn new_obj_string_value_map() -> ObjStringValueMap {
    ObjStringValueMap::with_hasher(BuildPassThroughHasher::default())
}

pub struct ObjStringIter {
    pub(crate) class: Gc<ObjClass>,
    pub(crate) iterable: Gc<ObjString>,
    pos: usize,
}

pub fn new_gc_obj_string_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    string: Gc<ObjString>,
) -> Gc<RefCell<ObjStringIter>> {
    vm.allocate(RefCell::new(ObjStringIter::new(class, string)))
}

pub fn new_root_obj_string_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    string: Gc<ObjString>,
) -> Root<RefCell<ObjStringIter>> {
    new_gc_obj_string_iter(vm, class, string).as_root()
}

impl ObjStringIter {
    fn new(class: Gc<ObjClass>, iterable: Gc<ObjString>) -> Self {
        ObjStringIter {
            class,
            iterable,
            pos: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Option<(usize, usize)> {
        if self.pos == self.iterable.len() {
            return None;
        }
        let old_pos = self.pos;
        self.pos += 1;
        while self.pos < self.iterable.len() && !self.iterable.is_char_boundary(self.pos) {
            self.pos += 1;
        }
        Some((old_pos, self.pos))
    }
}

impl memory::GcManaged for ObjStringIter {
    fn mark(&self) {
        self.iterable.mark();
    }

    fn blacken(&self) {
        self.iterable.blacken();
    }
}

impl fmt::Display for ObjStringIter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjStringIter instance")
    }
}

pub enum ObjUpvalue {
    Closed(Value),
    Open(*mut Value),
}

pub fn new_gc_obj_upvalue(vm: &mut Vm, value: &mut Value) -> Gc<RefCell<ObjUpvalue>> {
    vm.allocate(RefCell::new(ObjUpvalue::new(value)))
}

pub fn new_root_obj_upvalue(vm: &mut Vm, index: &mut Value) -> Root<RefCell<ObjUpvalue>> {
    new_gc_obj_upvalue(vm, index).as_root()
}

impl ObjUpvalue {
    pub(crate) fn new(address: *mut Value) -> Self {
        ObjUpvalue::Open(address)
    }

    pub(crate) fn get(&self) -> Value {
        match self {
            Self::Open(a) => unsafe { **a },
            Self::Closed(v) => *v,
        }
    }

    pub(crate) fn set(&mut self, value: Value) {
        match self {
            Self::Open(a) => unsafe { **a = value },
            Self::Closed(ref mut v) => *v = value,
        }
    }

    pub fn is_open(&self) -> bool {
        match self {
            Self::Open(_) => true,
            Self::Closed(_) => false,
        }
    }

    pub fn is_open_with_value(&self, value: &Value) -> bool {
        match self {
            Self::Open(address) => *address as *const _ == value,
            Self::Closed(_) => false,
        }
    }

    pub fn is_open_with_pred(&self, predicate: impl Fn(*const Value) -> bool) -> bool {
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
    pub chunk: Gc<Chunk>,
    pub name: Gc<ObjString>,
    pub(crate) module_path: Gc<ObjString>,
}

pub fn new_gc_obj_function(
    vm: &mut Vm,
    name: Gc<ObjString>,
    arity: u32,
    upvalue_count: usize,
    chunk: Gc<Chunk>,
    module_path: Gc<ObjString>,
) -> Gc<ObjFunction> {
    vm.allocate(ObjFunction::new(
        name,
        arity,
        upvalue_count,
        chunk,
        module_path,
    ))
}

pub fn new_root_obj_function(
    vm: &mut Vm,
    name: Gc<ObjString>,
    arity: u32,
    upvalue_count: usize,
    chunk: Gc<Chunk>,
    module_path: Gc<ObjString>,
) -> Root<ObjFunction> {
    new_gc_obj_function(vm, name, arity, upvalue_count, chunk, module_path).as_root()
}

impl ObjFunction {
    fn new(
        name: memory::Gc<ObjString>,
        arity: u32,
        upvalue_count: usize,
        chunk: Gc<Chunk>,
        module_path: Gc<ObjString>,
    ) -> Self {
        ObjFunction {
            name,
            arity,
            upvalue_count,
            chunk,
            module_path,
        }
    }
}

impl memory::GcManaged for ObjFunction {
    fn mark(&self) {
        self.name.mark();
        self.chunk.mark();
    }

    fn blacken(&self) {
        self.name.blacken();
        self.chunk.blacken();
    }
}

impl fmt::Display for ObjFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name.len() {
            0 => write!(f, "script"),
            _ => write!(f, "fn {}", *self.name),
        }
    }
}

pub type NativeFn = fn(&mut Vm, usize) -> Result<Value, Error>;

pub struct ObjNative {
    pub(crate) name: Gc<ObjString>,
    pub function: NativeFn,
}

pub fn new_gc_obj_native(vm: &mut Vm, name: Gc<ObjString>, function: NativeFn) -> Gc<ObjNative> {
    vm.allocate(ObjNative::new(name, function))
}

pub fn new_root_obj_native(
    vm: &mut Vm,
    name: Gc<ObjString>,
    function: NativeFn,
) -> Root<ObjNative> {
    new_gc_obj_native(vm, name, function).as_root()
}

impl ObjNative {
    fn new(name: Gc<ObjString>, function: NativeFn) -> Self {
        ObjNative { name, function }
    }
}

impl memory::GcManaged for ObjNative {
    fn mark(&self) {}

    fn blacken(&self) {}
}

impl fmt::Display for ObjNative {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "built-in fn {}", *self.name)
    }
}

pub struct ObjClosure {
    pub function: memory::Gc<ObjFunction>,
    pub upvalues: Vec<memory::Gc<RefCell<ObjUpvalue>>>,
    pub(crate) module: Gc<RefCell<ObjModule>>,
}

pub fn new_gc_obj_closure(
    vm: &mut Vm,
    function: Gc<ObjFunction>,
    module: Gc<RefCell<ObjModule>>,
) -> Gc<RefCell<ObjClosure>> {
    let upvalue_roots: Vec<Root<RefCell<ObjUpvalue>>> = (0..function.upvalue_count)
        .map(|_| vm.allocate_root(RefCell::new(ObjUpvalue::new(ptr::null_mut()))))
        .collect();
    let upvalues = upvalue_roots.iter().map(|u| u.as_gc()).collect();

    vm.allocate(RefCell::new(ObjClosure::new(function, upvalues, module)))
}

pub fn new_root_obj_closure(
    vm: &mut Vm,
    function: Gc<ObjFunction>,
    module: Gc<RefCell<ObjModule>>,
) -> Root<RefCell<ObjClosure>> {
    new_gc_obj_closure(vm, function, module).as_root()
}

impl ObjClosure {
    fn new(
        function: memory::Gc<ObjFunction>,
        upvalues: Vec<memory::Gc<RefCell<ObjUpvalue>>>,
        module: Gc<RefCell<ObjModule>>,
    ) -> Self {
        ObjClosure {
            function,
            upvalues,
            module,
        }
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
    pub metaclass: Gc<ObjClass>,
    pub superclass: Option<Gc<ObjClass>>,
    pub methods: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

pub fn new_gc_obj_class(
    vm: &mut Vm,
    name: Gc<ObjString>,
    metaclass: Gc<ObjClass>,
    superclass: Option<Gc<ObjClass>>,
    methods: ObjStringValueMap,
) -> Gc<ObjClass> {
    let mut merged_methods = if let Some(parent) = superclass {
        parent.methods.clone()
    } else {
        new_obj_string_value_map()
    };
    for (&k, &v) in &methods {
        merged_methods.insert(k, v);
    }
    vm.allocate(ObjClass::new(name, metaclass, superclass, merged_methods))
}

pub fn new_root_obj_class(
    vm: &mut Vm,
    name: Gc<ObjString>,
    metaclass: Gc<ObjClass>,
    superclass: Option<Gc<ObjClass>>,
    methods: ObjStringValueMap,
) -> Root<ObjClass> {
    new_gc_obj_class(vm, name, metaclass, superclass, methods).as_root()
}

impl ObjClass {
    pub(crate) fn new(
        name: memory::Gc<ObjString>,
        metaclass: Gc<ObjClass>,
        superclass: Option<Gc<ObjClass>>,
        methods: ObjStringValueMap,
    ) -> Self {
        ObjClass {
            name,
            metaclass,
            superclass,
            methods,
        }
    }
}

impl memory::GcManaged for ObjClass {
    fn mark(&self) {
        self.metaclass.mark();
        self.methods.mark();
    }

    fn blacken(&self) {
        self.metaclass.blacken();
        self.methods.blacken();
    }
}

impl fmt::Display for ObjClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self.name)
    }
}

pub struct ObjInstance {
    pub class: memory::Gc<ObjClass>,
    pub fields: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

pub fn new_gc_obj_instance(vm: &mut Vm, class: Gc<ObjClass>) -> Gc<RefCell<ObjInstance>> {
    vm.allocate(RefCell::new(ObjInstance::new(class)))
}

pub fn new_root_obj_instance(vm: &mut Vm, class: Gc<ObjClass>) -> Root<RefCell<ObjInstance>> {
    new_gc_obj_instance(vm, class).as_root()
}

impl ObjInstance {
    fn new(class: Gc<ObjClass>) -> Self {
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
        write!(f, "{} instance", *self.class)
    }
}

pub struct ObjBoundMethod<T: memory::GcManaged> {
    pub receiver: Value,
    pub method: memory::Gc<T>,
}

pub fn new_gc_obj_bound_method<T: 'static + memory::GcManaged>(
    vm: &mut Vm,
    receiver: Value,
    method: Gc<T>,
) -> Gc<RefCell<ObjBoundMethod<T>>> {
    vm.allocate(RefCell::new(ObjBoundMethod::new(receiver, method)))
}

pub fn new_root_obj_bound_method<T: 'static + memory::GcManaged>(
    vm: &mut Vm,
    receiver: Value,
    method: Gc<T>,
) -> Root<RefCell<ObjBoundMethod<T>>> {
    new_gc_obj_bound_method(vm, receiver, method).as_root()
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
        write!(
            f,
            "built-in method {} on {}",
            *self.method.name, self.receiver
        )
    }
}

impl fmt::Display for ObjBoundMethod<RefCell<ObjClosure>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "method {} on {}",
            *self.method.borrow().function.name,
            self.receiver
        )
    }
}

pub struct ObjVec {
    pub class: Gc<ObjClass>,
    pub elements: Vec<Value>,
    disp_lock: Cell<bool>,
}

pub fn new_gc_obj_vec(vm: &mut Vm, class: Gc<ObjClass>) -> Gc<RefCell<ObjVec>> {
    vm.allocate(RefCell::new(ObjVec::new(class)))
}

pub fn new_root_obj_vec(vm: &mut Vm, class: Gc<ObjClass>) -> Root<RefCell<ObjVec>> {
    new_gc_obj_vec(vm, class).as_root()
}

impl ObjVec {
    fn new(class: Gc<ObjClass>) -> Self {
        ObjVec {
            class,
            elements: Vec::new(),
            disp_lock: Cell::new(false),
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
        if self.disp_lock.get() {
            return write!(f, "[...]");
        }
        let prev_disp_lock = self.disp_lock.replace(true);
        write!(f, "[")?;
        let num_elems = self.elements.len();
        for (i, e) in self.elements.iter().enumerate() {
            write!(f, "{}{}", e, if i == num_elems - 1 { "" } else { ", " })?;
        }
        self.disp_lock.set(prev_disp_lock);
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
    pub class: Gc<ObjClass>,
    pub iterable: Gc<RefCell<ObjVec>>,
    pub current: usize,
}

pub fn new_gc_obj_vec_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    vec: Gc<RefCell<ObjVec>>,
) -> Gc<RefCell<ObjVecIter>> {
    vm.allocate(RefCell::new(ObjVecIter::new(class, vec)))
}

pub fn new_root_obj_vec_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    vec: Gc<RefCell<ObjVec>>,
) -> Root<RefCell<ObjVecIter>> {
    new_gc_obj_vec_iter(vm, class, vec).as_root()
}

impl ObjVecIter {
    fn new(class: Gc<ObjClass>, iterable: Gc<RefCell<ObjVec>>) -> Self {
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
    pub class: Gc<ObjClass>,
    pub begin: isize,
    pub end: isize,
}

pub fn new_gc_obj_range(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    begin: isize,
    end: isize,
) -> Gc<ObjRange> {
    vm.allocate(ObjRange::new(class, begin, end))
}

pub fn new_root_obj_range(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    begin: isize,
    end: isize,
) -> Root<ObjRange> {
    new_gc_obj_range(vm, class, begin, end).as_root()
}

impl ObjRange {
    fn new(class: Gc<ObjClass>, begin: isize, end: isize) -> Self {
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
            return Err(error!(
                ErrorKind::IndexError,
                "{} slice start out of range.", type_name
            ));
        }
        let end = if self.end < 0 {
            self.end + limit
        } else {
            self.end
        };
        if end < 0 || end > limit {
            return Err(error!(
                ErrorKind::IndexError,
                "{} slice end out of range.", type_name
            ));
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
    pub class: Gc<ObjClass>,
    pub iterable: Gc<ObjRange>,
    current: isize,
    step: isize,
}

pub fn new_gc_obj_range_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    range: Gc<ObjRange>,
) -> Gc<RefCell<ObjRangeIter>> {
    vm.allocate(RefCell::new(ObjRangeIter::new(class, range)))
}

pub fn new_root_obj_range_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    range: Gc<ObjRange>,
) -> Root<RefCell<ObjRangeIter>> {
    new_gc_obj_range_iter(vm, class, range).as_root()
}

impl ObjRangeIter {
    fn new(class: Gc<ObjClass>, iterable: Gc<ObjRange>) -> Self {
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

pub struct ObjHashMap {
    pub class: Gc<ObjClass>,
    pub elements: HashMap<Value, Value, BuildPassThroughHasher>,
    disp_lock: Cell<bool>,
}

pub fn new_gc_obj_hash_map(vm: &mut Vm, class: Gc<ObjClass>) -> Gc<RefCell<ObjHashMap>> {
    vm.allocate(RefCell::new(ObjHashMap::new(class)))
}

pub fn new_root_obj_hash_map(vm: &mut Vm, class: Gc<ObjClass>) -> Root<RefCell<ObjHashMap>> {
    new_gc_obj_hash_map(vm, class).as_root()
}

impl ObjHashMap {
    fn new(class: Gc<ObjClass>) -> Self {
        ObjHashMap {
            class,
            elements: HashMap::with_hasher(BuildPassThroughHasher::default()),
            disp_lock: Cell::new(false),
        }
    }
}

impl memory::GcManaged for ObjHashMap {
    fn mark(&self) {
        self.class.mark();
        self.elements.mark();
    }

    fn blacken(&self) {
        self.class.blacken();
        self.elements.blacken();
    }
}

impl fmt::Display for ObjHashMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.disp_lock.get() {
            return write!(f, "{{...}}");
        }
        let prev_disp_lock = self.disp_lock.replace(true);
        write!(f, "{{")?;
        let num_elems = self.elements.len();
        for (i, (&k, &v)) in self.elements.iter().enumerate() {
            write!(
                f,
                "{}: {}{}",
                k,
                v,
                if i == num_elems - 1 { "" } else { ", " }
            )?;
        }
        self.disp_lock.set(prev_disp_lock);
        write!(f, "}}")
    }
}

impl cmp::PartialEq for ObjHashMap {
    fn eq(&self, other: &ObjHashMap) -> bool {
        if self as *const _ == other as *const _ {
            return true;
        }
        self.elements == other.elements
    }
}

pub struct ObjTuple {
    pub class: Gc<ObjClass>,
    pub elements: Vec<Value>,
    self_lock: Cell<bool>,
}

pub fn new_gc_obj_tuple(vm: &mut Vm, class: Gc<ObjClass>, elements: Vec<Value>) -> Gc<ObjTuple> {
    vm.allocate(ObjTuple::new(class, elements))
}

pub fn new_root_obj_tuple(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    elements: Vec<Value>,
) -> Root<ObjTuple> {
    new_gc_obj_tuple(vm, class, elements).as_root()
}

impl ObjTuple {
    fn new(class: Gc<ObjClass>, elements: Vec<Value>) -> Self {
        ObjTuple {
            class,
            elements,
            self_lock: Cell::new(false),
        }
    }

    pub(crate) fn has_hash(&self) -> bool {
        if self.self_lock.get() {
            return true;
        }
        let self_lock_prev = self.self_lock.replace(true);
        let ret = self
            .elements
            .iter()
            .map(|v| v.has_hash())
            .fold(true, |a, b| a && b);
        self.self_lock.set(self_lock_prev);
        ret
    }
}

impl memory::GcManaged for ObjTuple {
    fn mark(&self) {
        self.class.mark();
        self.elements.mark();
    }

    fn blacken(&self) {
        self.class.blacken();
        self.elements.blacken();
    }
}

impl fmt::Display for ObjTuple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.self_lock.get() {
            return write!(f, "(...)");
        }
        let prev_self_lock = self.self_lock.replace(true);
        write!(f, "(")?;
        let num_elems = self.elements.len();
        for (i, e) in self.elements.iter().enumerate() {
            let suffix = if num_elems == 1 {
                ","
            } else if i == num_elems - 1 {
                ""
            } else {
                ", "
            };
            write!(f, "{}{}", e, suffix)?;
        }
        self.self_lock.set(prev_self_lock);
        write!(f, ")")
    }
}

impl cmp::PartialEq for ObjTuple {
    fn eq(&self, other: &ObjTuple) -> bool {
        if self as *const _ == other as *const _ {
            return true;
        }
        self.elements == other.elements
    }
}

impl Hash for Gc<ObjTuple> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let hash = self
            .elements
            .iter()
            .map(|v| {
                let mut hasher = PassThroughHasher::default();
                v.hash(&mut hasher);
                hasher.finish()
            })
            .fold(0_64, |a, b| a ^ b);
        state.write_u64(hash);
    }
}

pub struct ObjTupleIter {
    pub class: Gc<ObjClass>,
    pub iterable: Gc<ObjTuple>,
    pub current: usize,
}

pub fn new_gc_obj_tuple_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    vec: Gc<ObjTuple>,
) -> Gc<RefCell<ObjTupleIter>> {
    vm.allocate(RefCell::new(ObjTupleIter::new(class, vec)))
}

pub fn new_root_obj_tuple_iter(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    vec: Gc<ObjTuple>,
) -> Root<RefCell<ObjTupleIter>> {
    new_gc_obj_tuple_iter(vm, class, vec).as_root()
}

impl ObjTupleIter {
    fn new(class: Gc<ObjClass>, iterable: Gc<ObjTuple>) -> Self {
        ObjTupleIter {
            class,
            iterable,
            current: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Value {
        if self.current >= self.iterable.elements.len() {
            return Value::Sentinel;
        }
        let ret = self.iterable.elements[self.current];
        self.current += 1;
        ret
    }
}

impl memory::GcManaged for ObjTupleIter {
    fn mark(&self) {
        self.iterable.mark();
    }

    fn blacken(&self) {
        self.iterable.blacken();
    }
}

impl fmt::Display for ObjTupleIter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjTupleIter instance")
    }
}

pub struct ObjModule {
    pub(crate) imported: bool,
    pub(crate) class: Gc<ObjClass>,
    pub(crate) path: Gc<ObjString>,
    pub attributes: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

pub(crate) fn new_gc_obj_module(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    path: Gc<ObjString>,
) -> Gc<RefCell<ObjModule>> {
    vm.allocate(RefCell::new(ObjModule::new(class, path)))
}

pub(crate) fn new_root_obj_module(
    vm: &mut Vm,
    class: Gc<ObjClass>,
    path: Gc<ObjString>,
) -> Root<RefCell<ObjModule>> {
    new_gc_obj_module(vm, class, path).as_root()
}

impl ObjModule {
    pub(crate) fn new(class: Gc<ObjClass>, path: Gc<ObjString>) -> Self {
        ObjModule {
            imported: false,
            class,
            path,
            attributes: new_obj_string_value_map(),
        }
    }
}

impl memory::GcManaged for ObjModule {
    fn mark(&self) {
        self.attributes.mark();
    }

    fn blacken(&self) {
        self.attributes.blacken();
    }
}

impl fmt::Display for ObjModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "module \"{}\"", *self.path)
    }
}

pub(crate) struct CallFrame {
    pub(crate) closure: Gc<RefCell<ObjClosure>>,
    pub(crate) ip: *const u8,
    pub(crate) slot_base: usize,
}

impl GcManaged for CallFrame {
    fn mark(&self) {
        self.closure.mark();
    }

    fn blacken(&self) {
        self.closure.blacken();
    }
}

pub struct ObjFiber {
    pub(crate) class: Gc<ObjClass>,
    pub(crate) caller: Option<Gc<RefCell<ObjFiber>>>,
    pub(crate) stack: Stack<Value>,
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) open_upvalues: Vec<Gc<RefCell<ObjUpvalue>>>,
}

impl ObjFiber {
    pub(crate) fn new(class: Gc<ObjClass>, closure: Gc<RefCell<ObjClosure>>) -> Self {
        let mut frames = Vec::with_capacity(common::FRAMES_MAX);
        let ip = closure.borrow().function.chunk.code.as_ptr();
        frames.push(CallFrame {
            closure,
            ip,
            slot_base: 0,
        });
        ObjFiber {
            class,
            caller: None,
            stack: Stack::new(),
            frames,
            open_upvalues: Vec::new(),
        }
    }

    pub(crate) fn push_call_frame(&mut self, closure: Gc<RefCell<ObjClosure>>) {
        let (ip, arity) = {
            let borrowed_closure = closure.borrow();
            (
                borrowed_closure.function.chunk.code.as_ptr(),
                borrowed_closure.function.arity,
            )
        };
        self.frames.push(CallFrame {
            closure,
            ip,
            slot_base: self.stack.len() - arity as usize,
        })
    }

    pub(crate) fn close_upvalues(&mut self, index: usize) {
        let value = self.stack[index];
        let value_ref = &self.stack[index];
        for upvalue in self.open_upvalues.iter() {
            if upvalue.borrow().is_open_with_value(value_ref) {
                upvalue.borrow_mut().close(value);
            }
        }

        self.open_upvalues.retain(|u| u.borrow().is_open());
    }
}

impl GcManaged for ObjFiber {
    fn mark(&self) {
        self.stack.mark();
        self.frames.mark();
        self.open_upvalues.mark();
        if let Some(&caller) = self.caller.as_ref() {
            caller.mark();
        }
    }

    fn blacken(&self) {
        self.stack.blacken();
        self.frames.blacken();
        self.open_upvalues.blacken();
        if let Some(&caller) = self.caller.as_ref() {
            caller.blacken();
        }
    }
}

impl fmt::Display for ObjFiber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fiber")
    }
}
