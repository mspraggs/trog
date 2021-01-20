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
use std::collections::HashMap;
use std::fs;
use std::mem;

use yarel::compiler;
use yarel::error::{Error, ErrorKind};
use yarel::value::Value;
use yarel::vm::Vm;

type Matcher = fn(&str) -> Option<usize>;

const WILDCARDS: [(&str, Matcher); 1] = [("[MEMADDR]", match_memaddr)];

#[derive(Debug)]
pub struct Success {
    pub path: String,
    pub skipped: bool,
}

pub struct Failure {
    pub path: String,
    pub expected: Vec<String>,
    pub actual: Vec<String>,
}

thread_local!(static OUTPUT: RefCell<Vec<String>> = RefCell::new(Vec::new()));

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

fn parse_test(source: String) -> Option<Vec<String>> {
    if source.as_str().starts_with("// skip\n") {
        return None;
    }
    let mut lines = Vec::new();
    let mut cont = true;
    source.as_str().lines().for_each(|l| {
        if cont && l.starts_with("// ") {
            lines.push(l[3..].to_owned());
        } else {
            cont = false;
        }
    });
    lines.pop();
    Some(lines)
}

fn local_print(_heap: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    if args.len() != 2 {
        return Err(Error::with_message(
            ErrorKind::RuntimeError,
            "Expected one argument to 'print'.",
        ));
    }
    let lines = format!("{}", args[1]);
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

pub(crate) fn run_test(path: &str, vm: &mut Vm) -> Result<Success, Failure> {
    vm.reset();
    vm.define_native("print", local_print);

    let source = match fs::read_to_string(path) {
        Ok(contents) => contents,
        _ => panic!("Unable to open test file."),
    };

    let expected_output = match parse_test(source.clone()) {
        Some(output) => output,
        None => {
            return Ok(Success {
                path: path.to_owned(),
                skipped: true,
            });
        }
    };

    let result = compiler::compile(vm, source);
    let error_output = match result {
        Ok(f) => match vm.execute(f, &[]) {
            Ok(_) => Vec::new(),
            Err(e) => e.get_messages().clone(),
        },
        Err(e) => e.get_messages().clone(),
    };

    let mut output = OUTPUT.with(|output| mem::replace(&mut *output.borrow_mut(), vec![]));
    output.extend_from_slice(&error_output);

    if !match_output(&expected_output, &output) {
        return Err(Failure {
            path: path.to_owned(),
            expected: expected_output,
            actual: output,
        });
    }

    Ok(Success {
        path: path.to_owned(),
        skipped: false,
    })
}
