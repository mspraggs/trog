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

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ErrorKind {
    AttributeError,
    CompileError,
    IndexError,
    NameError,
    RuntimeError,
    TypeError,
    ValueError,
}

#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
    messages: Vec<String>,
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Error {
            kind,
            messages: Vec::new(),
        }
    }

    pub fn with_message(kind: ErrorKind, message: &str) -> Self {
        Error {
            kind,
            messages: vec![String::from(message)],
        }
    }

    pub fn with_messages(kind: ErrorKind, messages: &[&str]) -> Self {
        let messages = messages.iter().map(|s| String::from(*s)).collect();
        Error { kind, messages }
    }

    pub fn add_message(&mut self, message: &str) {
        self.messages.push(String::from(message));
    }

    pub fn get_kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn get_messages(&self) -> &Vec<String> {
        &self.messages
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        for msg in &self.messages {
            match writeln!(f, "{}", msg) {
                Ok(()) => {}
                Err(error) => {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}
