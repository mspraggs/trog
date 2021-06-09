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

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::mem;

use yarel::error::{Error, ErrorKind};
use yarel::value::Value;
use yarel::vm::{self, Vm};

type Matcher = fn(&str) -> Option<usize>;

const WILDCARDS: [(&str, Matcher); 1] = [("[MEMADDR]", match_memaddr)];

thread_local!(static OUTPUT: RefCell<Vec<String>> = RefCell::new(Vec::new()));

#[allow(dead_code)]
struct Outcome {
    pass: bool,
    expected: Vec<String>,
    actual: Vec<String>,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Expected:\n")?;
        for line in &self.expected {
            write!(f, "    {}\n", line)?;
        }
        write!(f, "Actual:\n")?;
        for line in &self.actual {
            write!(f, "    {}\n", line)?;
        }
        Ok(())
    }
}

fn match_memaddr(s: &str) -> Option<usize> {
    if !s.is_char_boundary(2) {
        return None;
    }
    for (i, c) in s[2..].chars().enumerate() {
        if !c.is_ascii_hexdigit() {
            return if i > 0 { Some(i + 2) } else { None };
        }
    }
    Some(s.len())
}

fn get_next_char_boundary(s: &str, i: usize) -> usize {
    for pos in (i + 1)..s.len() {
        if s.is_char_boundary(pos) {
            return pos;
        }
    }
    s.len()
}

fn match_line(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }

    let mut matchers = HashMap::new();
    for (pattern, matcher) in &WILDCARDS {
        for (pos, _) in expected.match_indices(pattern) {
            matchers.insert(pos, (pattern.len(), matcher));
        }
    }

    let mut i = 0;
    let mut j = 0;
    while i < expected.len() && j < actual.len() {
        if let Some((i_offset, matcher)) = matchers.get(&i) {
            if let Some(j_offset) = matcher(&actual[j..]) {
                i += i_offset;
                j += j_offset;
                continue;
            } else {
                return false;
            }
        }
        let next_i = get_next_char_boundary(expected, i);
        let next_j = get_next_char_boundary(actual, j);
        if expected[i..next_i] != actual[j..next_j] {
            return false;
        }
        i = next_i;
        j = next_j;
    }

    true
}

fn parse_test(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cont = true;
    source.lines().for_each(|l| {
        if cont && l.starts_with("// ") {
            lines.push(l[3..].to_owned());
        } else {
            cont = false;
        }
    });
    lines.pop();
    lines
}

fn local_print(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    if num_args != 1 {
        return Err(Error::with_message(
            ErrorKind::TypeError,
            "Expected one argument to 'print'.",
        ));
    }
    let lines = format!("{}", vm.native_arg(1));
    for line in lines.as_str().lines() {
        OUTPUT.with(|output| output.borrow_mut().push(line.to_string()));
    }
    Ok(Value::None)
}

fn match_output(expected: &[String], actual: &[String]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }

    if expected == actual {
        return true;
    }

    for (expected, actual) in expected.iter().zip(actual.iter()) {
        if !match_line(expected, actual) {
            return false;
        }
    }

    true
}

#[allow(dead_code)]
fn run_test(source: &str) -> Outcome {
    let mut vm = Vm::with_built_ins();
    vm.set_printer(local_print);
    vm.set_module_loader(module_loader);

    let result = vm::interpret(&mut vm, source.to_string(), None);
    let error_output = result
        .map_err(|e| e.messages().clone())
        .err()
        .unwrap_or_default();

    let mut output = OUTPUT.with(|output| mem::take(&mut *output.borrow_mut()));
    output.extend_from_slice(&error_output);
    let expected = parse_test(source);

    Outcome {
        pass: match_output(&expected, &output),
        expected,
        actual: output,
    }
}

#[allow(unused_macros)]
macro_rules! test_case {
    ($name:ident, $source:expr) => {
        #[allow(non_snake_case)]
        #[test]
        fn $name() {
            let outcome = run_test($source);
            assert!(outcome.pass, "\n{}", outcome);
        }
    };
}

#[allow(unused_macros)]
macro_rules! gen_module_loader {
    ($($key:expr => $result:expr),*) => {
        fn module_loader(path: &str) -> Result<String, Error> {
            match path {
                $($key => Ok($result.to_string()),)*
                _ => Err(Error::with_message(
                    ErrorKind::ImportError,
                    &format!("Unable to read file '{}.yl' (file not found).", path),
                )),
            }
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/module_loader.rs"));
include!(concat!(env!("OUT_DIR"), "/compiled_tests.rs"));
