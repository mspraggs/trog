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
use std::fs;
use std::mem;

use regex::{self, Regex};

use yarel::compiler;
use yarel::error::{Error, ErrorKind};
use yarel::value::Value;
use yarel::vm::Vm;

const WILDCARDS: [(&str, &str); 1] = [("[MEMADDR]", "0x[a-f0-9]+")];

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

fn parse_line(source: &str) -> String {
    let mut result = regex::escape(source);
    for (wildcard, regex) in &WILDCARDS {
        let escaped_wildcard = &regex::escape(wildcard.to_owned());
        result = result.replace(escaped_wildcard, regex);
    }

    format!("^{}$", result)
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
        if expected == actual {
            continue;
        }
        let expected = parse_line(expected);
        let re = Regex::new(&expected).unwrap();
        if !re.is_match(actual) {
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
