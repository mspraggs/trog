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

use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::rc::Rc;

use crate::common;

pub fn allocate<T: 'static + GcManaged>(data: T) -> Root<T> {
    let ptr = HEAP.with(|heap| {
        if cfg!(debug_assertions) {
            heap.borrow_mut().collect();
        } else {
            heap.borrow_mut().collect_if_required();
        }
        heap.borrow_mut().allocate(data)
    });
    let root = Root { ptr: ptr };
    root.gc_box().root();
    root
}

pub fn allocate_unique<T: 'static + GcManaged>(data: T) -> UniqueRoot<T> {
    let ptr = HEAP.with(|heap| {
        heap.borrow_mut().collect_if_required();
        heap.borrow_mut().allocate(data)
    });
    let root = UniqueRoot { ptr: ptr };
    root.gc_box().root();
    root
}

pub fn get_root<T: 'static + GcManaged>(ptr: Gc<T>) -> Root<T> {
    HEAP.with(|heap| heap.borrow_mut().get_root(ptr))
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
    num_roots: Cell<usize>,
    colour: Cell<Colour>,
    num_greys: Rc<RefCell<usize>>,
    data: T,
}

impl<T: 'static + GcManaged + ?Sized> GcBox<T> {
    fn unmark(&self) {
        self.colour.set(Colour::White);
    }

    fn root(&self) {
        self.num_roots.replace(self.num_roots.get() + 1);
    }

    fn unroot(&self) {
        self.num_roots.replace(self.num_roots.get() - 1);
    }
}

impl<T: 'static + GcManaged + ?Sized> GcManaged for GcBox<T> {
    fn mark(&self) {
        if self.colour.replace(Colour::Grey) == Colour::Grey {
            return;
        }
        *self.num_greys.borrow_mut() += 1;
        if cfg!(debug_assertions) {
            println!("{:?} mark", self as *const _);
        }
        self.data.mark();
    }

    fn blacken(&self) {
        if self.colour.replace(Colour::Black) == Colour::Black {
            return;
        }
        *self.num_greys.borrow_mut() -= 1;
        if cfg!(debug_assertions) {
            println!("{:?} blacken", self as *const _);
        }
        self.data.blacken();
    }
}

pub struct Gc<T: GcManaged> {
    ptr: GcBoxPtr<T>,
}

impl<T: 'static + GcManaged> Gc<T> {
    fn gc_box(&self) -> &GcBox<T> {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: 'static + GcManaged> GcManaged for Gc<T> {
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

impl<T: 'static + Default + GcManaged> Default for Gc<T> {
    fn default() -> Self {
        let root: Root<T> = Default::default();
        root.as_gc()
    }
}

impl<T: 'static + GcManaged> Deref for Gc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.gc_box().data
    }
}

pub struct Root<T: 'static + GcManaged> {
    ptr: GcBoxPtr<T>,
}

impl<T: GcManaged> Root<T> {
    pub fn as_gc(&self) -> Gc<T> {
        Gc { ptr: self.ptr }
    }

    fn gc_box(&self) -> &GcBox<T> {
        unsafe { self.ptr.as_ref() }
    }

    fn gc_box_mut(&mut self) -> &mut GcBox<T> {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: 'static + GcManaged> GcManaged for Root<T> {
    fn mark(&self) {
        self.gc_box().mark();
    }

    fn blacken(&self) {
        self.gc_box().blacken();
    }
}

impl<T: GcManaged> Clone for Root<T> {
    fn clone(&self) -> Self {
        self.gc_box().root();
        Root { ptr: self.ptr }
    }
}

impl<T: Default + GcManaged> Default for Root<T> {
    fn default() -> Self {
        allocate(Default::default())
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

impl<T: 'static + GcManaged> Drop for Root<T> {
    fn drop(&mut self) {
        self.gc_box().unroot();
    }
}

pub struct UniqueRoot<T: 'static + GcManaged> {
    ptr: GcBoxPtr<T>,
}

impl<T: GcManaged> UniqueRoot<T> {
    fn gc_box(&self) -> &GcBox<T> {
        unsafe { self.ptr.as_ref() }
    }

    fn gc_box_mut(&mut self) -> &mut GcBox<T> {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: 'static + GcManaged> GcManaged for UniqueRoot<T> {
    fn mark(&self) {
        self.gc_box().mark();
    }

    fn blacken(&self) {
        self.gc_box().blacken();
    }
}

impl<T: Default + GcManaged> Default for UniqueRoot<T> {
    fn default() -> Self {
        allocate_unique(Default::default())
    }
}

impl<T: 'static + GcManaged> Deref for UniqueRoot<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.gc_box().data
    }
}

impl<T: 'static + GcManaged> DerefMut for UniqueRoot<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.gc_box_mut().data
    }
}

impl<T: 'static + GcManaged> Drop for UniqueRoot<T> {
    fn drop(&mut self) {
        self.gc_box().unroot();
    }
}

#[derive(Default)]
pub struct Heap {
    collection_threshold: Cell<usize>,
    bytes_allocated: Cell<usize>,
    num_greys: Rc<RefCell<usize>>,
    objects: Vec<Box<GcBox<dyn GcManaged>>>,
}

impl Heap {
    fn new() -> Self {
        Heap {
            collection_threshold: Cell::new(common::HEAP_INIT_BYTES_MAX),
            bytes_allocated: Cell::new(0),
            num_greys: Rc::new(RefCell::new(0)),
            objects: Vec::new(),
        }
    }

    fn allocate<T: 'static + GcManaged>(&mut self, data: T) -> GcBoxPtr<T> {
        let mut obj = Box::new(GcBox {
            num_roots: Cell::new(0),
            colour: Cell::new(Colour::White),
            num_greys: self.num_greys.clone(),
            data: data,
        });

        let gc_box_ptr = unsafe { GcBoxPtr::new_unchecked(obj.as_mut()) };

        self.objects.push(obj);
        let size = mem::size_of::<T>();

        self.bytes_allocated
            .replace(self.bytes_allocated.get() + size);

        if cfg!(debug_assertions) {
            let new_ptr = self.objects.last().unwrap();
            println!(
                "{:?} allocate {} for {:?}",
                new_ptr.as_ref() as *const _,
                size,
                TypeId::of::<T>(),
            )
        }

        gc_box_ptr
    }

    fn get_root<T: 'static + GcManaged>(&mut self, obj: Gc<T>) -> Root<T> {
        obj.gc_box().root();
        Root { ptr: obj.ptr }
    }

    fn collect(&mut self) {
        if cfg!(debug_assertions) {
            println!("-- gc begin")
        }

        self.mark();
        self.trace_references();
        let bytes_freed = self.sweep();

        let prev_bytes_allocated = self.bytes_allocated.get();
        self.bytes_allocated
            .replace(self.bytes_allocated.get() - bytes_freed);
        self.collection_threshold
            .replace(self.bytes_allocated.get() * common::HEAP_GROWTH_FACTOR);

        if cfg!(debug_assertions) {
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

    fn mark(&mut self) {
        self.objects.iter_mut().for_each(|obj| obj.unmark());
        self.objects
            .iter_mut()
            .filter(|obj| obj.num_roots.get() > 0)
            .for_each(|obj| obj.mark());
    }

    fn trace_references(&mut self) {
        while *self.num_greys.borrow() > 0 {
            self.objects
                .iter_mut()
                .filter(|obj| obj.colour.get() == Colour::Grey)
                .for_each(|obj| obj.blacken());
        }
    }

    fn sweep(&mut self) -> usize {
        let bytes_marked: usize = self
            .objects
            .iter()
            .filter(|obj| obj.colour.get() == Colour::White)
            .map(|obj| {
                if cfg!(debug_assertions) {
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

impl<K, V: GcManaged> GcManaged for HashMap<K, V> {
    fn mark(&self) {
        for (_, v) in self {
            v.mark();
        }
    }

    fn blacken(&self) {
        for (_, v) in self {
            v.blacken();
        }
    }
}
