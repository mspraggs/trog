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

#[derive(Clone, Default)]
pub struct CoreClassStore {
    root_base_metaclass: Option<Root<ObjClass>>,
    root_object_class: Option<Root<ObjClass>>,
    {% for spec in class_specs %}
    root_{{ spec.name }}: Option<Root<ObjClass>>,{% endfor %}
}

impl CoreClassStore {
    pub(crate) fn new_empty() -> Self {
        Default::default()
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
        {% for spec in class_specs %}
        let root_{{ spec.name }} = build_empty_class();{% endfor %}

        CoreClassStore {
            root_base_metaclass: Some(root_base_metaclass),
            root_object_class: Some(root_object_class),
            {% for spec in class_specs %}
            root_{{ spec.name }}: Some(root_{{ spec.name }}),{% endfor %}
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
        {% for spec in class_specs %}
        {% if spec.kind == "NativeValue" %}
        let root_{{ spec.name }} = build_value_type_class("{{ spec.repr }}");
        {% elif spec.kind == "NativeObject" %}
        let root_{{ spec.name }} = core::new_root_obj_{{ spec.name }}(
            vm,
            root_{{ spec.metaclass }}.as_gc(),
            root_{{ spec.superclass }}_class.as_gc(),
        );{% else %}
        let root_{{ spec.name }} = vm
            .global("main", "{{ spec.repr }}")
            .unwrap()
            .try_as_obj_class()
            .expect("Expected ObjClass.")
            .as_root();{% endif %}{% endfor %}

        CoreClassStore {
            root_base_metaclass: Some(root_base_metaclass),
            root_object_class: Some(root_object_class),
            {% for spec in class_specs %}
            root_{{ spec.name }}: Some(root_{{ spec.name }}),{% endfor %}
        }
    }

    pub(crate) fn object_class(&self) -> Gc<ObjClass> {
        self.root_object_class
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    pub(crate) fn base_metaclass(&self) -> Gc<ObjClass> {
        self.root_base_metaclass
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }

    {% for spec in class_specs %}
    #[allow(dead_code)]
    pub(crate) fn {{ spec.name }}(&self) -> Gc<ObjClass> {
        self.root_{{ spec.name }}
            .as_ref()
            .expect("Expected Root.")
            .as_gc()
    }
    {% endfor %}
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

