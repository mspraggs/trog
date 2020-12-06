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

use crate::error::{Error, ErrorKind};
use crate::value::Value;

pub(crate) fn validate_integer(value: Value) -> Result<isize, Error> {
    match value {
        Value::Number(n) => {
            #[allow(clippy::float_cmp)]
            if n.trunc() != n {
                return error!(ErrorKind::ValueError, "Expected integer value.");
            }
            Ok(n as isize)
        }
        _ => return error!(ErrorKind::TypeError, "Expected integer value."),
    }
}