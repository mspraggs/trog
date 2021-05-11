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

use crate::core;
use crate::memory::{Gc, GcBoxPtr, Heap, Root};
use crate::object::{self, ObjClass};
use crate::vm::{self, Vm};

include!(concat!(env!("OUT_DIR"), "/core.yl.rs"));

#[derive(Clone)]
pub struct CoreClassStore {
    root_base_metaclass: Option<Root<ObjClass>>,
    root_object_class: Option<Root<ObjClass>>,
    root_nil_class: Option<Root<ObjClass>>,
    root_boolean_class: Option<Root<ObjClass>>,
    root_number_class: Option<Root<ObjClass>>,
    root_obj_closure_class: Option<Root<ObjClass>>,
    root_obj_native_class: Option<Root<ObjClass>>,
    root_obj_closure_method_class: Option<Root<ObjClass>>,
    root_obj_native_method_class: Option<Root<ObjClass>>,
    root_obj_iter_class: Option<Root<ObjClass>>,
    root_obj_map_iter_class: Option<Root<ObjClass>>,
    root_obj_filter_iter_class: Option<Root<ObjClass>>,
    root_obj_tuple_class: Option<Root<ObjClass>>,
    root_obj_tuple_iter_class: Option<Root<ObjClass>>,
    root_obj_vec_class: Option<Root<ObjClass>>,
    root_obj_vec_iter_class: Option<Root<ObjClass>>,
    root_obj_range_class: Option<Root<ObjClass>>,
    root_obj_range_iter_class: Option<Root<ObjClass>>,
    root_obj_hash_map_class: Option<Root<ObjClass>>,
    root_obj_module_class: Option<Root<ObjClass>>,
    root_obj_string_iter_class: Option<Root<ObjClass>>,
    root_obj_fiber_class: Option<Root<ObjClass>>,
    root_obj_error_class: Option<Root<ObjClass>>,
    root_obj_stop_iter_class: Option<Root<ObjClass>>,
    root_obj_runtime_error_class: Option<Root<ObjClass>>,
    root_obj_attribute_error_class: Option<Root<ObjClass>>,
    root_obj_index_error_class: Option<Root<ObjClass>>,
    root_obj_import_error_class: Option<Root<ObjClass>>,
    root_obj_name_error_class: Option<Root<ObjClass>>,
    root_obj_type_error_class: Option<Root<ObjClass>>,
    root_obj_value_error_class: Option<Root<ObjClass>>,
}

impl CoreClassStore {
    pub(crate) unsafe fn new_empty() -> Self {
        CoreClassStore {
            root_base_metaclass: None,
            root_object_class: None,
            root_nil_class: None,
            root_boolean_class: None,
            root_number_class: None,
            root_obj_closure_class: None,
            root_obj_native_class: None,
            root_obj_closure_method_class: None,
            root_obj_native_method_class: None,
            root_obj_iter_class: None,
            root_obj_map_iter_class: None,
            root_obj_filter_iter_class: None,
            root_obj_tuple_class: None,
            root_obj_tuple_iter_class: None,
            root_obj_vec_class: None,
            root_obj_vec_iter_class: None,
            root_obj_range_class: None,
            root_obj_range_iter_class: None,
            root_obj_hash_map_class: None,
            root_obj_module_class: None,
            root_obj_string_iter_class: None,
            root_obj_fiber_class: None,
            root_obj_error_class: None,
            root_obj_stop_iter_class: None,
            root_obj_runtime_error_class: None,
            root_obj_attribute_error_class: None,
            root_obj_import_error_class: None,
            root_obj_index_error_class: None,
            root_obj_name_error_class: None,
            root_obj_type_error_class: None,
            root_obj_value_error_class: None,
        }
    }

    pub(crate) fn new(
        vm: &mut Vm,
        root_base_metaclass: Root<ObjClass>,
        root_object_class: Root<ObjClass>,
    ) -> Self {
        let empty = vm.new_gc_obj_string("");
        let methods = object::new_obj_string_value_map();
        let mut build_empty_class =
            || vm.new_root_obj_class(empty, root_base_metaclass.as_gc(), None, methods.clone());
        let root_obj_iter_class = build_empty_class();
        let root_nil_class = build_empty_class();
        let root_boolean_class = build_empty_class();
        let root_number_class = build_empty_class();
        let root_obj_closure_class = build_empty_class();
        let root_obj_native_class = build_empty_class();
        let root_obj_closure_method_class = build_empty_class();
        let root_obj_native_method_class = build_empty_class();
        let root_obj_map_iter_class = build_empty_class();
        let root_obj_filter_iter_class = build_empty_class();
        let root_obj_tuple_class = build_empty_class();
        let root_obj_tuple_iter_class = build_empty_class();
        let root_obj_vec_class = build_empty_class();
        let root_obj_vec_iter_class = build_empty_class();
        let root_obj_range_class = build_empty_class();
        let root_obj_range_iter_class = build_empty_class();
        let root_obj_hash_map_class = build_empty_class();
        let root_obj_module_class = build_empty_class();
        let root_obj_string_iter_class = build_empty_class();
        let root_obj_fiber_class = build_empty_class();
        let root_obj_error_class = build_empty_class();
        let root_obj_stop_iter_class = build_empty_class();
        let root_obj_runtime_error_class = build_empty_class();
        let root_obj_attribute_error_class = build_empty_class();
        let root_obj_index_error_class = build_empty_class();
        let root_obj_import_error_class = build_empty_class();
        let root_obj_name_error_class = build_empty_class();
        let root_obj_type_error_class = build_empty_class();
        let root_obj_value_error_class = build_empty_class();
        CoreClassStore {
            root_base_metaclass: Some(root_base_metaclass),
            root_object_class: Some(root_object_class),
            root_nil_class: Some(root_nil_class),
            root_boolean_class: Some(root_boolean_class),
            root_number_class: Some(root_number_class),
            root_obj_closure_class: Some(root_obj_closure_class),
            root_obj_native_class: Some(root_obj_native_class),
            root_obj_closure_method_class: Some(root_obj_closure_method_class),
            root_obj_native_method_class: Some(root_obj_native_method_class),
            root_obj_iter_class: Some(root_obj_iter_class),
            root_obj_map_iter_class: Some(root_obj_map_iter_class),
            root_obj_filter_iter_class: Some(root_obj_filter_iter_class),
            root_obj_tuple_class: Some(root_obj_tuple_class),
            root_obj_tuple_iter_class: Some(root_obj_tuple_iter_class),
            root_obj_vec_class: Some(root_obj_vec_class),
            root_obj_vec_iter_class: Some(root_obj_vec_iter_class),
            root_obj_range_class: Some(root_obj_range_class),
            root_obj_range_iter_class: Some(root_obj_range_iter_class),
            root_obj_hash_map_class: Some(root_obj_hash_map_class),
            root_obj_module_class: Some(root_obj_module_class),
            root_obj_string_iter_class: Some(root_obj_string_iter_class),
            root_obj_fiber_class: Some(root_obj_fiber_class),
            root_obj_error_class: Some(root_obj_error_class),
            root_obj_stop_iter_class: Some(root_obj_stop_iter_class),
            root_obj_runtime_error_class: Some(root_obj_runtime_error_class),
            root_obj_attribute_error_class: Some(root_obj_attribute_error_class),
            root_obj_import_error_class: Some(root_obj_import_error_class),
            root_obj_index_error_class: Some(root_obj_index_error_class),
            root_obj_name_error_class: Some(root_obj_name_error_class),
            root_obj_type_error_class: Some(root_obj_type_error_class),
            root_obj_value_error_class: Some(root_obj_value_error_class),
        }
    }

    pub(crate) fn new_with_built_ins(
        vm: &mut Vm,
        root_base_metaclass: Root<ObjClass>,
        root_object_class: Root<ObjClass>,
    ) -> Self {
        let class_store = Self::new(vm, root_base_metaclass.clone(), root_object_class.clone());
        vm.class_store = class_store;
        let source = String::from(CORE_SOURCE);
        let result = vm::interpret(vm, source, None);
        match result {
            Ok(_) => {}
            Err(error) => eprint!("{}", error),
        }

        let mut build_value_type_class = |name| {
            let name = vm.new_gc_obj_string(name);
            vm.new_root_obj_class(
                name,
                root_base_metaclass.as_gc(),
                Some(root_object_class.as_gc()),
                object::new_obj_string_value_map(),
            )
        };
        let root_nil_class = build_value_type_class("Nil");
        let root_boolean_class = build_value_type_class("Bool");
        let root_number_class = build_value_type_class("Num");
        let root_obj_closure_class = build_value_type_class("Func");
        let root_obj_native_class = build_value_type_class("BuiltIn");
        let root_obj_closure_method_class = build_value_type_class("Method");
        let root_obj_native_method_class = build_value_type_class("BuiltInMethod");
        let root_obj_error_class = vm
            .get_global("main", "Error")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_stop_iter_class = vm
            .get_global("main", "StopIter")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_runtime_error_class = vm
            .get_global("main", "RuntimeError")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_attribute_error_class = vm
            .get_global("main", "AttributeError")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_import_error_class = vm
            .get_global("main", "ImportError")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_index_error_class = vm
            .get_global("main", "IndexError")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_name_error_class = vm
            .get_global("main", "NameError")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_type_error_class = vm
            .get_global("main", "TypeError")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_value_error_class = vm
            .get_global("main", "ValueError")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_iter_class = vm
            .get_global("main", "Iter")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_map_iter_class = vm
            .get_global("main", "MapIter")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_filter_iter_class = vm
            .get_global("main", "FilterIter")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();
        let root_obj_tuple_class = core::new_root_obj_tuple_class(
            vm,
            root_base_metaclass.as_gc(),
            root_object_class.as_gc(),
        );
        let root_obj_tuple_iter_class = core::new_root_obj_tuple_iter_class(
            vm,
            root_base_metaclass.as_gc(),
            root_obj_iter_class.as_gc(),
        );
        let root_obj_vec_class = core::new_root_obj_vec_class(
            vm,
            root_base_metaclass.as_gc(),
            root_object_class.as_gc(),
        );
        let root_obj_vec_iter_class = core::new_root_obj_vec_iter_class(
            vm,
            root_base_metaclass.as_gc(),
            root_obj_iter_class.as_gc(),
        );
        let root_obj_range_class = core::new_root_obj_range_class(
            vm,
            root_base_metaclass.as_gc(),
            root_object_class.as_gc(),
        );
        let root_obj_range_iter_class = core::new_root_obj_range_iter_class(
            vm,
            root_base_metaclass.as_gc(),
            root_obj_iter_class.as_gc(),
        );
        let root_obj_hash_map_class = core::new_root_obj_hash_map_class(
            vm,
            root_base_metaclass.as_gc(),
            root_object_class.as_gc(),
        );
        let root_obj_module_class = core::new_root_obj_module_class(
            vm,
            root_base_metaclass.as_gc(),
            root_object_class.as_gc(),
        );
        let root_obj_string_iter_class = core::new_root_obj_string_iter_class(
            vm,
            root_base_metaclass.as_gc(),
            root_obj_iter_class.as_gc(),
        );
        let root_obj_fiber_metaclass = core::new_root_obj_fiber_metaclass(
            vm,
            root_base_metaclass.as_gc(),
            root_object_class.as_gc(),
        );
        let root_obj_fiber_class = core::new_root_obj_fiber_class(
            vm,
            root_obj_fiber_metaclass.as_gc(),
            root_object_class.as_gc(),
        );
        CoreClassStore {
            root_base_metaclass: Some(root_base_metaclass),
            root_object_class: Some(root_object_class),
            root_nil_class: Some(root_nil_class),
            root_boolean_class: Some(root_boolean_class),
            root_number_class: Some(root_number_class),
            root_obj_closure_class: Some(root_obj_closure_class),
            root_obj_native_class: Some(root_obj_native_class),
            root_obj_closure_method_class: Some(root_obj_closure_method_class),
            root_obj_native_method_class: Some(root_obj_native_method_class),
            root_obj_iter_class: Some(root_obj_iter_class),
            root_obj_map_iter_class: Some(root_obj_map_iter_class),
            root_obj_filter_iter_class: Some(root_obj_filter_iter_class),
            root_obj_tuple_class: Some(root_obj_tuple_class),
            root_obj_tuple_iter_class: Some(root_obj_tuple_iter_class),
            root_obj_vec_class: Some(root_obj_vec_class),
            root_obj_vec_iter_class: Some(root_obj_vec_iter_class),
            root_obj_range_class: Some(root_obj_range_class),
            root_obj_range_iter_class: Some(root_obj_range_iter_class),
            root_obj_hash_map_class: Some(root_obj_hash_map_class),
            root_obj_module_class: Some(root_obj_module_class),
            root_obj_string_iter_class: Some(root_obj_string_iter_class),
            root_obj_fiber_class: Some(root_obj_fiber_class),
            root_obj_error_class: Some(root_obj_error_class),
            root_obj_stop_iter_class: Some(root_obj_stop_iter_class),
            root_obj_runtime_error_class: Some(root_obj_runtime_error_class),
            root_obj_attribute_error_class: Some(root_obj_attribute_error_class),
            root_obj_import_error_class: Some(root_obj_import_error_class),
            root_obj_index_error_class: Some(root_obj_index_error_class),
            root_obj_name_error_class: Some(root_obj_name_error_class),
            root_obj_type_error_class: Some(root_obj_type_error_class),
            root_obj_value_error_class: Some(root_obj_value_error_class),
        }
    }

    pub(crate) fn get_base_metaclass(&self) -> Gc<ObjClass> {
        self.root_base_metaclass
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_object_class(&self) -> Gc<ObjClass> {
        self.root_object_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_nil_class(&self) -> Gc<ObjClass> {
        self.root_nil_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_boolean_class(&self) -> Gc<ObjClass> {
        self.root_boolean_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_number_class(&self) -> Gc<ObjClass> {
        self.root_number_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_closure_class(&self) -> Gc<ObjClass> {
        self.root_obj_closure_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_native_class(&self) -> Gc<ObjClass> {
        self.root_obj_native_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_closure_method_class(&self) -> Gc<ObjClass> {
        self.root_obj_closure_method_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_native_method_class(&self) -> Gc<ObjClass> {
        self.root_obj_native_method_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_map_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_map_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_filter_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_filter_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_tuple_class(&self) -> Gc<ObjClass> {
        self.root_obj_tuple_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_tuple_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_tuple_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_vec_class(&self) -> Gc<ObjClass> {
        self.root_obj_vec_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_vec_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_vec_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_range_class(&self) -> Gc<ObjClass> {
        self.root_obj_range_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_range_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_range_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_hash_map_class(&self) -> Gc<ObjClass> {
        self.root_obj_hash_map_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_module_class(&self) -> Gc<ObjClass> {
        self.root_obj_module_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_string_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_string_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_fiber_class(&self) -> Gc<ObjClass> {
        self.root_obj_fiber_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_stop_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_stop_iter_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_runtime_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_runtime_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_attribute_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_attribute_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_import_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_import_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_index_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_index_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_name_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_name_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_type_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_type_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn get_obj_value_error_class(&self) -> Gc<ObjClass> {
        self.root_obj_value_error_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }
}

pub(crate) unsafe fn new_base_metaclass(heap: &mut Heap) -> GcBoxPtr<ObjClass> {
    // # Safety
    // The root metaclass is its own metaclass, so we need to add a pointer to the metaclass to the
    // class's data. To do this we allocate the object and mutate it whilst an immutable reference
    // is held by a local `Root` instance. This is safe because the `Root` instance doesn't access
    // any fields on the pointer it holds whilst the metaclass assignment is being performed.
    let data = ObjClass {
        name: Gc::dangling(),
        metaclass: Gc::dangling(),
        superclass: None,
        methods: object::new_obj_string_value_map(),
    };
    let mut ptr = heap.allocate_bare(data);
    let root = Root::from(ptr);
    ptr.as_mut().data.metaclass = root.as_gc();
    ptr
}
