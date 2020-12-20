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

use std::fmt::{self, Display};
use std::ops::{Index, IndexMut};
use std::slice::{self};

use crate::common;
use crate::memory::GcManaged;

const STACK_MAX: usize = common::LOCALS_MAX * common::FRAMES_MAX;

// TODO: Use const-generics here when available
pub(crate) struct Stack<T: Clone + Copy + Default> {
    stack: [T; STACK_MAX],
    size: usize,
}

impl<T: Clone + Copy + Default> Stack<T> {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn peek(&self, depth: usize) -> &T {
        if cfg!(debug_assertions) && depth >= self.size {
            panic!("Stack index out of range.");
        }
        &self.stack[self.size - depth - 1]
    }

    pub(crate) fn peek_mut(&mut self, depth: usize) -> &mut T {
        &mut self.stack[self.size - depth - 1]
    }

    pub(crate) fn push(&mut self, data: T) {
        if cfg!(debug_assertions) && self.size == STACK_MAX {
            panic!("Stack overflow.");
        }
        self.stack[self.size] = data;
        self.size += 1;
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        if cfg!(debug_assertions) && self.size == 0 {
            None
        } else {
            self.size -= 1;
            Some(self.stack[self.size])
        }
    }

    pub(crate) fn truncate(&mut self, size: usize) {
        self.size = size;
    }

    pub(crate) fn extend_from_slice(&mut self, data: &[T]) {
        for elem in data {
            self.push(*elem);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.size
    }

    pub(crate) fn clear(&mut self) {
        self.size = 0;
    }
}

impl<T: Clone + Copy + Default + GcManaged> GcManaged for Stack<T> {
    fn mark(&self) {
        for elem in &self.stack[0..self.size] {
            elem.mark();
        }
    }

    fn blacken(&self) {
        for elem in &self.stack[0..self.size] {
            elem.blacken();
        }
    }
}

impl<T: Clone + Copy + Default> Default for Stack<T> {
    fn default() -> Self {
        Stack {
            stack: [Default::default(); STACK_MAX],
            size: 0,
        }
    }
}

impl<T: Clone + Copy + Default + Display> Display for Stack<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for elem in &self.stack[0..self.size] {
            write!(f, "[ {} ]", elem)?;
        }
        Ok(())
    }
}

impl<T: Clone + Copy + Default + GcManaged, Idx> Index<Idx> for Stack<T>
where
    Idx: slice::SliceIndex<[T]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.stack[index]
    }
}

impl<T: Clone + Copy + Default + GcManaged, Idx> IndexMut<Idx> for Stack<T>
where
    Idx: slice::SliceIndex<[T]>,
{
    fn index_mut(&mut self, index: Idx) -> &mut Self::Output {
        &mut self.stack[index]
    }
}
