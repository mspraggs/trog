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

// The code below is in part inspired by the mark-and-sweep GC implemented here:
// https://github.com/Darksecond/lox

use std::any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use crate::common;

pub fn allocate<T: 'static + GcManaged>(data: T) -> Gc<T> {
    let ptr = HEAP.with(|heap| {
        if cfg!(any(debug_assertions, feature = "debug_stress_gc")) {
            heap.borrow_mut().collect();
        } else {
            heap.borrow_mut().collect_if_required();
        }
        heap.borrow_mut().allocate(data)
    });
    Gc { ptr }
}

pub fn allocate_root<T: 'static + GcManaged>(data: T) -> Root<T> {
    allocate(data).as_root()
}

thread_local!(static HEAP: RefCell<Heap> = RefCell::new(Heap::new()));

#[derive(Copy, Clone, PartialEq)]
enum Colour {
    Black,
    Grey,
    White,
}

pub trait GcManaged {
    fn mark(&self);

    fn blacken(&self);
}

type GcBoxPtr<T> = NonNull<GcBox<T>>;

struct GcBox<T: GcManaged + ?Sized> {
    colour: Cell<Colour>,
    num_roots: Cell<usize>,
    data: T,
}

impl<T: 'static + GcManaged + ?Sized> GcBox<T> {
    fn unmark(&self) {
        self.colour.set(Colour::White);
    }

    fn mark(&self) {
        if self.colour.replace(Colour::Grey) == Colour::Grey {
            return;
        }
        if cfg!(feature = "debug_trace_gc") {
            println!("{:?} mark", self as *const _);
        }
        self.data.mark();
    }

    fn blacken(&self) {
        if self.colour.replace(Colour::Black) == Colour::Black {
            return;
        }
        if cfg!(feature = "debug_trace_gc") {
            println!("{:?} blacken", self as *const _);
        }
        self.data.blacken();
    }
}

pub struct Root<T: GcManaged + ?Sized> {
    ptr: GcBoxPtr<T>,
}

impl<T: GcManaged> Root<T> {
    pub fn as_gc(&self) -> Gc<T> {
        Gc { ptr: self.ptr }
    }
}

impl<T: GcManaged + ?Sized> Root<T> {
    fn inc_num_roots(&mut self) {
        *self.gc_box_mut().num_roots.get_mut() += 1;
    }

    fn dec_num_roots(&mut self) {
        *self.gc_box_mut().num_roots.get_mut() -= 1;
    }
}

impl<T: GcManaged + ?Sized> Root<T> {
    fn gc_box(&self) -> &GcBox<T> {
        unsafe { self.ptr.as_ref() }
    }

    fn gc_box_mut(&mut self) -> &mut GcBox<T> {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: 'static + GcManaged + ?Sized> GcManaged for Root<T> {
    fn mark(&self) {
        self.gc_box().mark();
    }

    fn blacken(&self) {
        self.gc_box().blacken();
    }
}

impl<T: GcManaged + ?Sized> Clone for Root<T> {
    fn clone(&self) -> Root<T> {
        let mut ret = Root { ptr: self.ptr };
        ret.inc_num_roots();
        ret
    }
}

impl<T: 'static + GcManaged> Deref for Root<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.gc_box().data
    }
}

impl<T: 'static + GcManaged> DerefMut for Root<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.gc_box_mut().data
    }
}

impl<T: GcManaged + ?Sized> Drop for Root<T> {
    fn drop(&mut self) {
        self.dec_num_roots();
    }
}

impl<T: GcManaged> From<Gc<T>> for Root<T> {
    fn from(gc: Gc<T>) -> Self {
        let mut ret = Root { ptr: gc.ptr };
        ret.inc_num_roots();
        ret
    }
}

pub struct Gc<T: GcManaged + ?Sized> {
    ptr: GcBoxPtr<T>,
}

impl<T: GcManaged> Gc<T> {
    pub fn as_root(&self) -> Root<T> {
        Root::from(*self)
    }
}

impl<T: 'static + GcManaged + ?Sized> Gc<T> {
    fn gc_box(&self) -> &GcBox<T> {
        unsafe { self.ptr.as_ref() }
    }

    fn gc_box_mut(&mut self) -> &mut GcBox<T> {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: 'static + GcManaged + ?Sized> GcManaged for Gc<T> {
    fn mark(&self) {
        self.gc_box().mark();
    }

    fn blacken(&self) {
        self.gc_box().blacken();
    }
}

impl<T: GcManaged> Copy for Gc<T> {}

impl<T: GcManaged> Clone for Gc<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static + GcManaged> Deref for Gc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.gc_box().data
    }
}

impl<T: 'static + GcManaged> DerefMut for Gc<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.gc_box_mut().data
    }
}

impl<T: GcManaged> PartialEq for Gc<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr.as_ptr() == other.ptr.as_ptr()
    }
}

#[derive(Default)]
pub struct Heap {
    collection_threshold: Cell<usize>,
    bytes_allocated: Cell<usize>,
    objects: Vec<Box<GcBox<dyn GcManaged>>>,
}

impl Heap {
    fn new() -> Self {
        Heap {
            collection_threshold: Cell::new(common::HEAP_INIT_BYTES_MAX),
            bytes_allocated: Cell::new(0),
            objects: Vec::new(),
        }
    }

    fn allocate<T: 'static + GcManaged>(&mut self, data: T) -> GcBoxPtr<T> {
        let mut obj = Box::new(GcBox {
            colour: Cell::new(Colour::White),
            num_roots: Cell::new(0),
            data,
        });

        let gc_box_ptr = unsafe { GcBoxPtr::new_unchecked(obj.as_mut()) };

        self.objects.push(obj);
        let size = mem::size_of::<T>();

        self.bytes_allocated
            .replace(self.bytes_allocated.get() + size);

        if cfg!(feature = "debug_trace_gc") {
            let new_ptr = self.objects.last().unwrap();
            println!(
                "{:?} allocate {} for {:?}",
                new_ptr.as_ref() as *const _,
                size,
                any::type_name::<T>(),
            )
        }

        gc_box_ptr
    }

    fn collect(&mut self) {
        if cfg!(feature = "debug_trace_gc") {
            println!("-- gc begin")
        }

        self.mark_roots();
        self.trace_references();
        let bytes_freed = self.sweep();

        let prev_bytes_allocated = self.bytes_allocated.get();
        self.bytes_allocated
            .replace(self.bytes_allocated.get() - bytes_freed);
        self.collection_threshold
            .replace(self.bytes_allocated.get() * common::HEAP_GROWTH_FACTOR);

        if cfg!(feature = "debug_trace_gc") {
            println!("-- gc end (freed {} bytes)", bytes_freed);
            println!(
                "   collected {} bytes (from {} to {}) next at {}",
                bytes_freed,
                prev_bytes_allocated,
                self.bytes_allocated.get(),
                self.collection_threshold.get(),
            )
        }
    }

    fn collect_if_required(&mut self) {
        if self.bytes_allocated.get() >= self.collection_threshold.get() {
            self.collect();
        }
    }

    fn mark_roots(&mut self) {
        self.objects.iter_mut().for_each(|obj| obj.unmark());
        self.objects.iter_mut().for_each(|obj| {
            if obj.num_roots.get() > 0 {
                obj.mark();
            }
        });
    }

    fn trace_references(&mut self) {
        let mut num_greys = self
            .objects
            .iter()
            .filter(|obj| obj.colour.get() == Colour::Grey)
            .count();
        while num_greys > 0 {
            num_greys = self
                .objects
                .iter_mut()
                .filter(|obj| obj.colour.get() == Colour::Grey)
                .map(|obj| obj.blacken())
                .count();
        }
    }

    fn sweep(&mut self) -> usize {
        let bytes_marked: usize = self
            .objects
            .iter()
            .filter(|obj| obj.colour.get() == Colour::White)
            .map(|obj| {
                if cfg!(feature = "debug_trace_gc") {
                    println!("{:?} free", obj.as_ref() as *const _);
                }
                mem::size_of_val(&obj.data)
            })
            .sum();

        self.objects.retain(|obj| obj.colour.get() == Colour::Black);

        bytes_marked
    }
}

impl<T: GcManaged> GcManaged for RefCell<T> {
    fn mark(&self) {
        self.borrow().mark();
    }

    fn blacken(&self) {
        self.borrow().blacken();
    }
}

impl<T: GcManaged> GcManaged for Vec<T> {
    fn mark(&self) {
        for e in self {
            e.mark();
        }
    }

    fn blacken(&self) {
        for e in self {
            e.blacken();
        }
    }
}

impl<K, V: GcManaged, S> GcManaged for HashMap<K, V, S> {
    fn mark(&self) {
        for v in self.values() {
            v.mark();
        }
    }

    fn blacken(&self) {
        for v in self.values() {
            v.blacken();
        }
    }
}
