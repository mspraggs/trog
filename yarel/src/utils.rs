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

use crate::error::{Error, ErrorKind};
use crate::value::Value;

pub(crate) fn validate_integer(value: Value) -> Result<isize, Error> {
    if let Value::Number(n) = value {
        #[allow(clippy::float_cmp)]
        if n.trunc() != n {
            return Err(error!(
                ErrorKind::ValueError,
                "Expected an integer value but found '{}'.", value
            ));
        }
        Ok(n as isize)
    } else {
        Err(error!(
            ErrorKind::TypeError,
            "Expected an integer value but found '{}'.", value
        ))
    }
}

pub(crate) fn hash_number(num: f64) -> u64 {
    let mut hash = u64::from_ne_bytes(num.to_ne_bytes()) as u128;
    hash = (!hash).wrapping_add(hash.wrapping_shl(18));
    hash = hash ^ hash.wrapping_shr(31);
    hash = hash.wrapping_mul(21);
    hash = hash ^ hash.wrapping_shr(11);
    hash = hash.wrapping_add(hash.wrapping_shl(6));
    hash = hash ^ hash.wrapping_shr(22);
    hash as u64
}
