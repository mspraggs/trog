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

use std::fmt::{self, Display};
use std::ops::{Index, IndexMut};
use std::ptr;
use std::slice;

use crate::memory::GcManaged;

#[derive(Debug)]
pub(crate) struct Stack<T: Clone + Copy + Default, const N: usize> {
    stack: Box<[T; N]>,
    top: *mut T,
}

impl<T: Clone + Copy + Default, const N: usize> Stack<T, N> {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn peek(&self, depth: usize) -> &T {
        if cfg!(any(debug_assertions, feature = "safe_stack")) && depth >= self.len() {
            panic!("Stack index out of range.");
        }
        unsafe { &*self.top.offset(-(depth as isize) - 1) }
    }

    pub(crate) fn peek_mut(&mut self, depth: usize) -> &mut T {
        if cfg!(any(debug_assertions, feature = "safe_stack")) && depth >= self.len() {
            panic!("Stack index out of range.");
        }
        unsafe { &mut *self.top.offset(-(depth as isize) - 1) }
    }

    pub(crate) fn push(&mut self, data: T) {
        if cfg!(any(debug_assertions, feature = "safe_stack")) && self.len() == N {
            panic!("Stack overflow.");
        }
        unsafe {
            *self.top = data;
            self.top = self.top.offset(1);
        }
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        if cfg!(any(debug_assertions, feature = "safe_stack")) && self.len() == 0 {
            return None;
        }
        unsafe {
            self.top = self.top.offset(-1);
            Some(*self.top)
        }
    }

    pub(crate) fn truncate(&mut self, size: usize) {
        let size = if cfg!(any(debug_assertions, feature = "safe_stack")) && size > self.len() {
            self.len()
        } else {
            size
        };
        self.top = unsafe { self.stack.as_ptr().offset(size as isize) as *mut _ };
    }

    pub(crate) fn len(&self) -> usize {
        unsafe { self.top.offset_from(self.stack.as_ptr() as *mut _) as usize }
    }

    pub(crate) fn as_ptr(&self) -> *const T {
        self.stack.as_ptr()
    }

    pub(crate) fn clear(&mut self) {
        self.top = self.stack.as_ptr() as *mut _;
    }
}

impl<T, const N: usize> GcManaged for Stack<T, N>
where
    T: Clone + Copy + Default + GcManaged,
{
    fn mark(&self) {
        for elem in &self.stack[0..self.len()] {
            elem.mark();
        }
    }

    fn blacken(&self) {
        for elem in &self.stack[0..self.len()] {
            elem.blacken();
        }
    }
}

impl<T, const N: usize> Default for Stack<T, N>
where
    T: Clone + Copy + Default,
{
    fn default() -> Self {
        let mut stack = Stack {
            stack: Box::new([Default::default(); N]),
            top: ptr::null_mut(),
        };
        stack.clear();
        stack
    }
}

impl<T, const N: usize> Display for Stack<T, N>
where
    T: Clone + Copy + Default + Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for elem in &self.stack[0..self.len()] {
            write!(f, "[ {} ]", elem)?;
        }
        Ok(())
    }
}

impl<T, Idx, const N: usize> Index<Idx> for Stack<T, N>
where
    T: Clone + Copy + Default + GcManaged,
    Idx: slice::SliceIndex<[T]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.stack[index]
    }
}

impl<T: Clone + Copy + Default + GcManaged, Idx, const N: usize> IndexMut<Idx> for Stack<T, N>
where
    Idx: slice::SliceIndex<[T]>,
{
    fn index_mut(&mut self, index: Idx) -> &mut Self::Output {
        &mut self.stack[index]
    }
}
