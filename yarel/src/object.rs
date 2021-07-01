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

use crate::chunk::Chunk;
use crate::common;
use crate::error::{Error, ErrorKind};
use crate::hash::{BuildPassThroughHasher, PassThroughHasher};
use crate::memory::{Gc, GcManaged};
use crate::stack::Stack;
use crate::value::Value;
use crate::vm::Vm;

const STACK_MAX: usize = common::LOCALS_MAX * common::FRAMES_MAX;

#[derive(Clone, Debug)]
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

    pub fn validate_char_boundary(&self, pos: usize, desc: &str) -> Result<(), Error> {
        if !self.as_str().is_char_boundary(pos) {
            return Err(error!(
                ErrorKind::IndexError,
                "Provided {} is not on a character boundary.", desc
            ));
        }
        Ok(())
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

impl PartialEq for ObjString {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.string.eq(&other.string)
    }
}

impl Eq for ObjString {}

impl PartialOrd for ObjString {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        self.string.partial_cmp(&other.string)
    }
}

impl Ord for ObjString {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.string.cmp(&other.string)
    }
}

impl Deref for ObjString {
    type Target = str;

    fn deref(&self) -> &str {
        self.string.as_str()
    }
}

impl GcManaged for ObjString {
    fn mark(&self) {}

    fn blacken(&self) {}
}

impl Eq for Gc<ObjString> {}

pub type ObjStringValueMap = HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>;

pub fn new_obj_string_value_map() -> ObjStringValueMap {
    ObjStringValueMap::with_hasher(BuildPassThroughHasher::default())
}

#[derive(Copy, Clone, Debug)]
pub struct ObjStringIter {
    pub(crate) class: Gc<ObjClass>,
    pub(crate) iterable: Gc<ObjString>,
    pos: usize,
}

impl ObjStringIter {
    pub(crate) fn new(class: Gc<ObjClass>, iterable: Gc<ObjString>) -> Self {
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

impl GcManaged for ObjStringIter {
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

#[derive(Copy, Clone, Debug)]
enum ObjUpvalueState {
    Closed(Value),
    Open(*mut Value),
}

impl fmt::Display for ObjUpvalueState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjUpvalueState::Closed(v) => write!(f, "{}", v),
            ObjUpvalueState::Open(v) => write!(f, "{}", unsafe { **v }),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ObjUpvalue {
    data: ObjUpvalueState,
    pub(crate) next: Option<Gc<RefCell<ObjUpvalue>>>,
}

impl ObjUpvalue {
    pub(crate) fn new(address: *mut Value) -> Self {
        ObjUpvalue {
            data: ObjUpvalueState::Open(address),
            next: None,
        }
    }

    pub(crate) fn get(&self) -> Value {
        match self.data {
            ObjUpvalueState::Open(a) => unsafe { *a },
            ObjUpvalueState::Closed(v) => v,
        }
    }

    pub(crate) fn set(&mut self, value: Value) {
        match self.data {
            ObjUpvalueState::Open(a) => unsafe { *a = value },
            ObjUpvalueState::Closed(ref mut v) => *v = value,
        }
    }

    pub fn is_open(&self) -> bool {
        match self.data {
            ObjUpvalueState::Open(_) => true,
            ObjUpvalueState::Closed(_) => false,
        }
    }

    pub fn is_open_with_pred(&self, predicate: impl Fn(*const Value) -> bool) -> bool {
        match self.data {
            ObjUpvalueState::Open(address) => predicate(address),
            ObjUpvalueState::Closed(_) => false,
        }
    }

    pub fn close(&mut self) {
        let value = self.get();
        self.data = ObjUpvalueState::Closed(value);
    }
}

impl GcManaged for ObjUpvalue {
    fn mark(&self) {
        match self.data {
            ObjUpvalueState::Closed(value) => value.mark(),
            ObjUpvalueState::Open(_) => {}
        }
        if let Some(u) = self.next.as_ref() {
            u.mark();
        }
    }

    fn blacken(&self) {
        match self.data {
            ObjUpvalueState::Closed(value) => value.blacken(),
            ObjUpvalueState::Open(_) => {}
        }
        if let Some(u) = self.next.as_ref() {
            u.blacken();
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ObjFunction {
    pub arity: usize,
    pub upvalue_count: usize,
    pub chunk: Gc<Chunk>,
    pub name: Gc<ObjString>,
    pub(crate) module_path: Gc<ObjString>,
}

impl ObjFunction {
    pub(crate) fn new(
        name: Gc<ObjString>,
        arity: usize,
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

impl GcManaged for ObjFunction {
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

#[derive(Copy, Clone)]
pub struct ObjNative {
    pub(crate) name: Gc<ObjString>,
    pub function: NativeFn,
    pub(crate) manages_stack: bool,
}

impl fmt::Debug for ObjNative {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let function = self.function as *const ();
        f.debug_struct("ObjNative")
            .field("name", &self.name)
            .field("function", &function)
            .field("manages_stack", &self.manages_stack)
            .finish()
    }
}

impl ObjNative {
    pub(crate) fn new(name: Gc<ObjString>, function: NativeFn, manages_stack: bool) -> Self {
        ObjNative {
            name,
            function,
            manages_stack,
        }
    }
}

impl GcManaged for ObjNative {
    fn mark(&self) {}

    fn blacken(&self) {}
}

impl fmt::Display for ObjNative {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "built-in fn {}", *self.name)
    }
}

#[derive(Clone, Debug)]
pub struct ObjClosure {
    pub function: Gc<ObjFunction>,
    pub upvalues: RefCell<Vec<Gc<RefCell<ObjUpvalue>>>>,
    pub(crate) module: Gc<RefCell<ObjModule>>,
}

impl ObjClosure {
    pub(crate) fn new(
        function: Gc<ObjFunction>,
        upvalues: Vec<Gc<RefCell<ObjUpvalue>>>,
        module: Gc<RefCell<ObjModule>>,
    ) -> Self {
        ObjClosure {
            function,
            upvalues: RefCell::new(upvalues),
            module,
        }
    }
}

impl GcManaged for ObjClosure {
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

#[derive(Clone, Debug)]
pub struct ObjClass {
    pub name: Gc<ObjString>,
    pub metaclass: Gc<ObjClass>,
    pub superclass: Option<Gc<ObjClass>>,
    pub methods: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

impl ObjClass {
    pub(crate) fn new(
        name: Gc<ObjString>,
        metaclass: Gc<ObjClass>,
        superclass: Option<Gc<ObjClass>>,
        methods: ObjStringValueMap,
    ) -> Self {
        let mut merged_methods = if let Some(parent) = superclass {
            parent.methods.clone()
        } else {
            new_obj_string_value_map()
        };
        for (&k, &v) in &methods {
            merged_methods.insert(k, v);
        }
        ObjClass {
            name,
            metaclass,
            superclass,
            methods: merged_methods,
        }
    }
}

impl GcManaged for ObjClass {
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

#[derive(Clone, Debug)]
pub struct ObjInstance {
    pub class: Gc<ObjClass>,
    pub fields: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
}

impl ObjInstance {
    pub(crate) fn new(class: Gc<ObjClass>) -> Self {
        ObjInstance {
            class,
            fields: HashMap::with_hasher(BuildPassThroughHasher::default()),
        }
    }
}

impl GcManaged for ObjInstance {
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

#[derive(Copy, Clone)]
pub struct ObjBoundMethod<T: GcManaged> {
    pub receiver: Value,
    pub method: Gc<T>,
}

impl<T: GcManaged> ObjBoundMethod<T> {
    pub(crate) fn new(receiver: Value, method: Gc<T>) -> Self {
        ObjBoundMethod { receiver, method }
    }
}

impl<T: 'static + GcManaged> GcManaged for ObjBoundMethod<T> {
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

impl fmt::Debug for ObjBoundMethod<ObjNative> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ObjBoundMethod<ObjNative>")
            .field("receiver", &self.receiver)
            .field("method", &self.method)
            .finish()
    }
}

impl fmt::Display for ObjBoundMethod<ObjClosure> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "method {} on {}",
            *self.method.function.name, self.receiver
        )
    }
}

impl fmt::Debug for ObjBoundMethod<ObjClosure> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ObjBoundMethod<ObjClosure>")
            .field("receiver", &self.receiver)
            .field("method", &self.method)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ObjVec {
    pub class: Gc<ObjClass>,
    pub elements: Vec<Value>,
    disp_lock: Cell<bool>,
}

impl ObjVec {
    pub(crate) fn new(class: Gc<ObjClass>) -> Self {
        ObjVec {
            class,
            elements: Vec::new(),
            disp_lock: Cell::new(false),
        }
    }
}

impl GcManaged for ObjVec {
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

#[derive(Clone, Debug)]
pub struct ObjVecIter {
    pub class: Gc<ObjClass>,
    pub iterable: Gc<RefCell<ObjVec>>,
    pub current: usize,
}

impl ObjVecIter {
    pub(crate) fn new(class: Gc<ObjClass>, iterable: Gc<RefCell<ObjVec>>) -> Self {
        ObjVecIter {
            class,
            iterable,
            current: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Option<Value> {
        let borrowed_vec = self.iterable.borrow();
        if self.current >= borrowed_vec.elements.len() {
            return None;
        }
        let ret = borrowed_vec.elements[self.current];
        self.current += 1;
        Some(ret)
    }
}

impl GcManaged for ObjVecIter {
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

#[derive(Copy, Clone, Debug)]
pub struct ObjRange {
    pub class: Gc<ObjClass>,
    pub begin: isize,
    pub end: isize,
}

impl ObjRange {
    pub(crate) fn new(class: Gc<ObjClass>, begin: isize, end: isize) -> Self {
        ObjRange { class, begin, end }
    }

    pub(crate) fn make_bounded_range(
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

impl GcManaged for ObjRange {
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

#[derive(Copy, Clone, Debug)]
pub struct ObjRangeIter {
    pub class: Gc<ObjClass>,
    pub iterable: Gc<ObjRange>,
    current: isize,
    step: isize,
}

impl ObjRangeIter {
    pub(crate) fn new(class: Gc<ObjClass>, iterable: Gc<ObjRange>) -> Self {
        let current = iterable.begin;
        ObjRangeIter {
            class,
            iterable,
            current,
            step: if iterable.begin < iterable.end { 1 } else { -1 },
        }
    }

    pub(crate) fn next(&mut self) -> Option<Value> {
        if self.current == self.iterable.end {
            return None;
        }
        let ret = Value::Number(self.current as f64);
        self.current += self.step;
        Some(ret)
    }
}

impl GcManaged for ObjRangeIter {
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

#[derive(Clone, Debug)]
pub struct ObjHashMap {
    pub class: Gc<ObjClass>,
    pub elements: HashMap<Value, Value, BuildPassThroughHasher>,
    disp_lock: Cell<bool>,
}

impl ObjHashMap {
    pub(crate) fn new(class: Gc<ObjClass>) -> Self {
        ObjHashMap {
            class,
            elements: HashMap::with_hasher(BuildPassThroughHasher::default()),
            disp_lock: Cell::new(false),
        }
    }
}

impl GcManaged for ObjHashMap {
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

#[derive(Clone, Debug)]
pub struct ObjTuple {
    pub class: Gc<ObjClass>,
    pub elements: Vec<Value>,
    self_lock: Cell<bool>,
}

impl ObjTuple {
    pub(crate) fn new(class: Gc<ObjClass>, elements: Vec<Value>) -> Self {
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

impl GcManaged for ObjTuple {
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

#[derive(Copy, Clone, Debug)]
pub struct ObjTupleIter {
    pub class: Gc<ObjClass>,
    pub iterable: Gc<ObjTuple>,
    pub current: usize,
}

impl ObjTupleIter {
    pub(crate) fn new(class: Gc<ObjClass>, iterable: Gc<ObjTuple>) -> Self {
        ObjTupleIter {
            class,
            iterable,
            current: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Option<Value> {
        if self.current >= self.iterable.elements.len() {
            return None;
        }
        let ret = self.iterable.elements[self.current];
        self.current += 1;
        Some(ret)
    }
}

impl GcManaged for ObjTupleIter {
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

#[derive(Clone, Debug)]
pub struct ObjModule {
    pub(crate) imported: bool,
    pub(crate) class: Gc<ObjClass>,
    pub(crate) path: Gc<ObjString>,
    pub attributes: HashMap<Gc<ObjString>, Value, BuildPassThroughHasher>,
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

impl GcManaged for ObjModule {
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct CallFrame {
    pub(crate) closure: Gc<ObjClosure>,
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExcHandler {
    pub(crate) catch_ip: *const u8,
    pub(crate) finally_ip: *const u8,
    pub(crate) init_stack_size: usize,
    pub(crate) frame_count: usize,
}

impl ExcHandler {
    pub(crate) fn has_catch_block(&self) -> bool {
        self.finally_ip == self.catch_ip
    }
}

#[derive(Clone, Debug)]
pub struct ObjFiber {
    pub(crate) class: Gc<ObjClass>,
    pub(crate) caller: Option<Gc<RefCell<ObjFiber>>>,
    pub(crate) stack: Stack<Value, STACK_MAX>,
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) native_arity: Option<usize>,
    pub(crate) open_upvalues: Option<Gc<RefCell<ObjUpvalue>>>,
    pub(crate) call_arity: usize,
    pub(crate) return_value: Value,
    pub(crate) exc_handlers: Vec<ExcHandler>,
    pub(crate) return_ip: Option<*const u8>,
    pub(crate) error_ip: Option<*const u8>,
}

impl ObjFiber {
    pub(crate) fn new(class: Gc<ObjClass>, closure: Gc<ObjClosure>) -> Self {
        let mut frames = Vec::with_capacity(common::FRAMES_MAX);
        let (ip, arity) = { (closure.function.chunk.code.as_ptr(), closure.function.arity) };
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
            native_arity: None,
            open_upvalues: None,
            call_arity: arity as usize,
            return_value: Value::None,
            exc_handlers: Vec::new(),
            return_ip: None,
            error_ip: None,
        }
    }

    pub(crate) fn push_call_frame(&mut self, closure: Gc<ObjClosure>) {
        let (ip, arity) = (closure.function.chunk.code.as_ptr(), closure.function.arity);
        self.frames.push(CallFrame {
            closure,
            ip,
            slot_base: self.stack.len() - arity,
        })
    }

    pub(crate) fn set_native_arity(&mut self, arity: usize) {
        self.native_arity = Some(arity);
    }

    pub(crate) fn take_native_arity(&mut self) -> Option<usize> {
        self.native_arity.take()
    }

    pub(crate) fn close_upvalues(&mut self, index: usize) {
        let index_addr = &self.stack[index] as *const _;
        let predicate = |v| v >= index_addr;

        while self.open_upvalues.is_some()
            && self
                .open_upvalues
                .unwrap()
                .borrow()
                .is_open_with_pred(predicate)
        {
            let upvalue = self.open_upvalues.unwrap();
            self.open_upvalues = {
                let mut borrowed_upvalue = upvalue.borrow_mut();
                borrowed_upvalue.close();
                borrowed_upvalue.next
            };
        }
    }

    pub(crate) fn close_upvalues_for_frame(&mut self) {
        let slot_base = self.current_frame().unwrap().slot_base;
        self.close_upvalues(slot_base);
    }

    pub(crate) fn current_frame(&self) -> Option<&CallFrame> {
        self.frames.last()
    }

    pub(crate) fn current_frame_mut(&mut self) -> Option<&mut CallFrame> {
        self.frames.last_mut()
    }

    pub(crate) fn is_new(&self) -> bool {
        self.frames.len() == 1
            && self.frames[0].ip == self.frames[0].closure.function.chunk.code.as_ptr()
    }

    pub(crate) fn has_finished(&self) -> bool {
        self.frames.is_empty()
    }

    pub(crate) fn push_exc_handler(&mut self, catch_ip: *const u8, finally_ip: *const u8) {
        self.exc_handlers.push(ExcHandler {
            catch_ip,
            finally_ip,
            init_stack_size: self.stack.len(),
            frame_count: self.frames.len(),
        })
    }

    pub(crate) fn pop_exc_handler(&mut self) -> Option<ExcHandler> {
        self.exc_handlers.pop()
    }

    pub(crate) fn take_return_data(&mut self) -> Option<(Value, *const u8)> {
        if let Some(ip) = self.return_ip.take() {
            let value = self.return_value;
            self.return_value = Value::None;
            Some((value, ip))
        } else {
            None
        }
    }

    pub(crate) fn store_error_ip_or(&mut self, alternative: *const u8) {
        self.current_frame_mut().expect("Expected CallFrame.").ip =
            self.error_ip.unwrap_or(alternative);
    }

    pub(crate) unsafe fn unchecked_native_frame_slot(&self, index: usize) -> Value {
        let slot_base = self.stack.len() - self.native_arity.unwrap() - 1;
        let pos = slot_base + index;
        self.stack[pos]
    }

    pub(crate) fn native_frame_slot(&self, index: usize) -> Value {
        let slot_base = self.stack.len() - self.native_arity.unwrap() - 1;
        let pos = slot_base + index;
        if pos >= self.stack.len() {
            panic!("Stack index out of range.");
        }
        self.stack[pos]
    }
}

impl GcManaged for ObjFiber {
    fn mark(&self) {
        self.stack.mark();
        self.frames.mark();
        if let Some(upvalues) = self.open_upvalues.as_ref() {
            upvalues.mark();
        }
        if let Some(&caller) = self.caller.as_ref() {
            caller.mark();
        }
        self.return_value.mark();
    }

    fn blacken(&self) {
        self.stack.blacken();
        self.frames.blacken();
        if let Some(upvalues) = self.open_upvalues.as_ref() {
            upvalues.blacken();
        }
        if let Some(&caller) = self.caller.as_ref() {
            caller.blacken();
        }
        self.return_value.blacken();
    }
}

impl fmt::Display for ObjFiber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fiber")
    }
}
