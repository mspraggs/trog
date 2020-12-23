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

use std::convert::TryInto;
use std::hash::{BuildHasher, Hasher};

pub struct FnvHasher {
    hash: u64,
}

impl FnvHasher {
    pub fn new() -> Self {
        Default::default()
    }
}

impl Default for FnvHasher {
    fn default() -> Self {
        FnvHasher { hash: 2166136261 }
    }
}

impl Hasher for FnvHasher {
    fn write(&mut self, msg: &[u8]) {
        for c in msg {
            self.hash ^= *c as u64;
            self.hash = (self.hash as u128 * 16777619) as u64;
        }
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

pub struct PassThroughHasher {
    hash: u64,
}

impl Default for PassThroughHasher {
    fn default() -> Self {
        PassThroughHasher { hash: 0 }
    }
}

impl Hasher for PassThroughHasher {
    fn write(&mut self, msg: &[u8]) {
        // This is a little contrived, but the hasher should only ever have write_u64 called on it.
        self.hash = u64::from_ne_bytes(msg.try_into().expect("Expected eight bytes."));
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

#[derive(Clone)]
pub struct BuildPassThroughHasher;

impl Default for BuildPassThroughHasher {
    fn default() -> Self {
        BuildPassThroughHasher {}
    }
}

impl BuildHasher for BuildPassThroughHasher {
    type Hasher = PassThroughHasher;

    fn build_hasher(&self) -> Self::Hasher {
        PassThroughHasher::default()
    }
}
