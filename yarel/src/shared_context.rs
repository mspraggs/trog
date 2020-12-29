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
use crate::memory::{self, Heap};
use crate::object::{self, ObjStringStore};

pub type SharedContext = (
    Rc<RefCell<Heap>>,
    Rc<RefCell<ObjStringStore>>,
    Rc<RefCell<ChunkStore>>,
    Box<CoreClassStore>,
);

pub fn new_shared_context() -> SharedContext {
    let heap = memory::new_heap();
    let string_store = object::new_obj_string_store(&mut heap.borrow_mut());
    let chunk_store = chunk::new_chunk_store();
    let class_store =
        class_store::new_class_store(heap.clone(), string_store.clone(), chunk_store.clone());

    (heap, string_store, chunk_store, class_store)
}
