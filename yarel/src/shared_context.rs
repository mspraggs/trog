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

use std::cell::RefCell;
use std::rc::Rc;

use crate::chunk::{self, ChunkStore};
use crate::class_store::{self, CoreClassStore};
use crate::memory::{self, Gc, GcBoxPtr, Heap, Root};
use crate::object::{self, ObjClass, ObjStringStore};

pub type SharedContext = (
    Rc<RefCell<Heap>>,
    Rc<RefCell<ObjStringStore>>,
    Rc<RefCell<ChunkStore>>,
    Box<CoreClassStore>,
);

pub fn new_shared_context() -> SharedContext {
    let heap = memory::new_heap();
    let mut obj_base_metaclass_ptr = new_base_metaclass(&mut heap.borrow_mut());
    let root_obj_base_metaclass = Root::from(obj_base_metaclass_ptr);
    let string_store =
        object::new_obj_string_store(&mut heap.borrow_mut(), root_obj_base_metaclass.as_gc());
    let base_metaclass_name = string_store
        .borrow_mut()
        .new_gc_obj_string(&mut heap.borrow_mut(), "Class");
    // # Safety
    // We're modifying data for which there are immutable references held by other data structures.
    // Because the code is single-threaded and the immutable references aren't being used to access
    // the data at this point in time (class names are only used by the Display trait), mutating the
    // data here should be safe.
    unsafe {
        obj_base_metaclass_ptr.as_mut().data_mut().name = Some(base_metaclass_name);
    }
    let chunk_store = chunk::new_chunk_store();
    let class_store = class_store::new_class_store(
        heap.clone(),
        string_store.clone(),
        chunk_store.clone(),
        root_obj_base_metaclass,
    );

    (heap, string_store, chunk_store, class_store)
}

fn new_base_metaclass(heap: &mut Heap) -> GcBoxPtr<ObjClass> {
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
        let mut ptr = heap.allocate_bare(data);
        let root = Root::from(ptr);
        ptr.as_mut().data_mut().metaclass = root.as_gc();
        ptr
    }
}
