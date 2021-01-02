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

use crate::core;
use crate::memory::{Gc, GcBoxPtr, Root};
use crate::object::{self, ObjClass};
use crate::vm::{self, Vm};

include!(concat!(env!("OUT_DIR"), "/core.yl.rs"));

#[derive(Clone)]
pub struct CoreClassStore {
    root_obj_base_metaclass: Root<ObjClass>,
    root_obj_iter_class: Root<ObjClass>,
    root_obj_map_iter_class: Root<ObjClass>,
    root_obj_filter_iter_class: Root<ObjClass>,
    root_obj_vec_class: Root<ObjClass>,
    root_obj_vec_iter_class: Root<ObjClass>,
    root_obj_range_class: Root<ObjClass>,
    root_obj_range_iter_class: Root<ObjClass>,
    root_obj_string_iter_class: Root<ObjClass>,
}

impl CoreClassStore {
    pub(crate) unsafe fn new_empty() -> Self {
        CoreClassStore {
            root_obj_base_metaclass: Root::dangling(),
            root_obj_iter_class: Root::dangling(),
            root_obj_map_iter_class: Root::dangling(),
            root_obj_filter_iter_class: Root::dangling(),
            root_obj_vec_class: Root::dangling(),
            root_obj_vec_iter_class: Root::dangling(),
            root_obj_range_class: Root::dangling(),
            root_obj_range_iter_class: Root::dangling(),
            root_obj_string_iter_class: Root::dangling(),
        }
    }

    pub(crate) fn new(vm: &mut Vm, root_obj_base_metaclass: Root<ObjClass>) -> Self {
        let empty = vm.new_gc_obj_string("");
        let methods = object::new_obj_string_value_map();
        let root_obj_iter_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        let root_obj_map_iter_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        let root_obj_filter_iter_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        let root_obj_vec_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        let root_obj_vec_iter_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        let root_obj_range_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        let root_obj_range_iter_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        let root_obj_string_iter_class =
            object::new_root_obj_class(vm, empty, root_obj_base_metaclass.as_gc(), methods.clone());
        CoreClassStore {
            root_obj_base_metaclass,
            root_obj_iter_class,
            root_obj_map_iter_class,
            root_obj_filter_iter_class,
            root_obj_vec_class,
            root_obj_vec_iter_class,
            root_obj_range_class,
            root_obj_range_iter_class,
            root_obj_string_iter_class,
        }
    }

    pub(crate) fn new_with_built_ins(vm: &mut Vm, root_obj_base_metaclass: Root<ObjClass>) -> Self {
        let class_store = Self::new(vm, root_obj_base_metaclass.clone());
        vm.class_store = class_store;
        let source = String::from(CORE_SOURCE);
        let result = vm::interpret(vm, source);
        match result {
            Ok(_) => {}
            Err(error) => eprint!("{}", error),
        }
        let root_obj_iter_class = vm
            .get_global("Iter")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_map_iter_class = vm
            .get_global("MapIter")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_filter_iter_class = vm
            .get_global("FilterIter")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_vec_class = core::new_root_obj_vec_class(
            vm,
            root_obj_base_metaclass.as_gc(),
            root_obj_iter_class.as_gc(),
        );
        let root_obj_vec_iter_class =
            core::new_root_obj_vec_iter_class(vm, root_obj_base_metaclass.as_gc());
        let root_obj_range_class = core::new_root_obj_range_class(
            vm,
            root_obj_base_metaclass.as_gc(),
            root_obj_iter_class.as_gc(),
        );
        let root_obj_range_iter_class =
            core::new_root_obj_range_iter_class(vm, root_obj_base_metaclass.as_gc());
        let root_obj_string_iter_class =
            core::new_root_obj_string_iter_class(vm, root_obj_base_metaclass.as_gc());
        CoreClassStore {
            root_obj_base_metaclass,
            root_obj_iter_class,
            root_obj_map_iter_class,
            root_obj_filter_iter_class,
            root_obj_vec_class,
            root_obj_vec_iter_class,
            root_obj_range_class,
            root_obj_range_iter_class,
            root_obj_string_iter_class,
        }
    }

    pub(crate) fn get_obj_base_metaclass(&self) -> Gc<ObjClass> {
        self.root_obj_base_metaclass.as_gc()
    }

    pub(crate) fn get_obj_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_iter_class.as_gc()
    }

    pub(crate) fn get_obj_map_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_map_iter_class.as_gc()
    }

    pub(crate) fn get_obj_filter_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_filter_iter_class.as_gc()
    }

    pub(crate) fn get_obj_vec_class(&self) -> Gc<ObjClass> {
        self.root_obj_vec_class.as_gc()
    }

    pub(crate) fn get_obj_vec_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_vec_iter_class.as_gc()
    }

    pub(crate) fn get_obj_range_class(&self) -> Gc<ObjClass> {
        self.root_obj_range_class.as_gc()
    }

    pub(crate) fn get_obj_range_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_range_iter_class.as_gc()
    }

    pub(crate) fn get_obj_string_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_string_iter_class.as_gc()
    }
}

pub(crate) fn new_base_metaclass(vm: &mut Vm) -> GcBoxPtr<ObjClass> {
    // # Safety
    // The root metaclass is its own metaclass, so we need to add a pointer to the metaclass to the
    // class's data. To do this we allocate the object and mutate it whilst an immutable reference
    // is held by a local `Root` instance. This is safe because the `Root` instance doesn't access
    // any fields on the pointer it holds whilst the metaclass assignment is being performed.
    unsafe {
        let data = ObjClass {
            name: None,
            metaclass: Gc::dangling(),
            methods: object::new_obj_string_value_map(),
        };
        let mut ptr = vm.allocate_bare(data);
        let root = Root::from(ptr);
        ptr.as_mut().data.metaclass = root.as_gc();
        ptr
    }
}
