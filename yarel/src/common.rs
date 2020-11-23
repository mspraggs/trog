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

pub const LOCALS_MAX: usize = u8::MAX as usize + 1;
pub const UPVALUES_MAX: usize = u8::MAX as usize + 1;
pub const JUMP_SIZE_MAX: usize = u16::MAX as usize + 1;
pub const HEAP_INIT_BYTES_MAX: usize = 65536;
pub const HEAP_GROWTH_FACTOR: usize = 2;
pub const VEC_ELEMS_MAX: usize = isize::MAX as usize + 1;
pub const INTERPOLATION_DEPTH_MAX: usize = 8;
