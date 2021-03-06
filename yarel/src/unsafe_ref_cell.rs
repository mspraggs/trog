/* Copyright 2021 Matt Spraggs
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

use std::cell::{Cell, UnsafeCell};
use std::ops::{Deref, DerefMut};

pub struct UnsafeRefCell<T: ?Sized> {
    borrow_flag: Cell<isize>,
    value: UnsafeCell<T>,
}

impl<T> UnsafeRefCell<T> {
    pub(crate) fn new(data: T) -> Self {
        UnsafeRefCell {
            borrow_flag: Cell::new(0),
            value: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> UnsafeRefCell<T> {
    pub(crate) fn borrow(&self) -> Ref<'_, T> {
        let b = self.borrow_flag.get().wrapping_add(1);
        if b > 0 {
            self.borrow_flag.set(b);
            Ref {
                value: unsafe { self.get() },
                borrow_flag: BorrowFlagRef {
                    flag_ref: &self.borrow_flag,
                },
            }
        } else {
            panic!("already mutably borrowed")
        }
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, T> {
        let b = self.borrow_flag.get();
        if b == 0 {
            self.borrow_flag.set(-1);
            RefMut {
                value: unsafe { self.get_mut() },
                borrow_flag: BorrowFlagRefMut {
                    flag_ref: &self.borrow_flag,
                },
            }
        } else {
            panic!("already borrowed");
        }
    }

    pub(crate) unsafe fn get(&self) -> &T {
        &*self.value.get()
    }

    pub(crate) unsafe fn get_mut(&self) -> &mut T {
        &mut *self.value.get()
    }
}

struct BorrowFlagRef<'a> {
    flag_ref: &'a Cell<isize>,
}

impl Clone for BorrowFlagRef<'_> {
    fn clone(&self) -> Self {
        let b = self.flag_ref.get();
        assert!(b != isize::MAX);
        self.flag_ref.set(b + 1);
        BorrowFlagRef {
            flag_ref: self.flag_ref,
        }
    }
}

impl Drop for BorrowFlagRef<'_> {
    fn drop(&mut self) {
        let b = self.flag_ref.get();
        self.flag_ref.set(b - 1);
    }
}

struct BorrowFlagRefMut<'a> {
    flag_ref: &'a Cell<isize>,
}

impl Drop for BorrowFlagRefMut<'_> {
    fn drop(&mut self) {
        self.flag_ref.set(0);
    }
}

#[derive(Clone)]
pub(crate) struct Ref<'a, T: ?Sized + 'a> {
    value: &'a T,
    borrow_flag: BorrowFlagRef<'a>,
}

impl<T: ?Sized> Deref for Ref<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}

pub(crate) struct RefMut<'a, T: ?Sized + 'a> {
    value: &'a mut T,
    #[allow(dead_code)]
    borrow_flag: BorrowFlagRefMut<'a>,
}

impl<T: ?Sized> Deref for RefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}

impl<T: ?Sized> DerefMut for RefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}
