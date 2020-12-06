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
use std::fmt;
use std::fs;
use std::mem;
use std::rc::Rc;

use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};

use yarel::chunk::ChunkStore;
use yarel::class_store::CoreClassStore;
use yarel::compiler;
use yarel::error::{Error, ErrorKind};
use yarel::memory::Heap;
use yarel::object::ObjStringStore;
use yarel::value::Value;
use yarel::vm;

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

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        queue!(
            f,
            SetForegroundColor(Color::DarkBlue),
            Print(format!("Test {}\n", self.path)),
            SetForegroundColor(Color::DarkGreen),
            Print("  Expected:\n".to_string()),
            ResetColor,
        )
        .unwrap();
        for line in &self.expected {
            writeln!(f, "    {}", line)?;
        }
        queue!(
            f,
            SetForegroundColor(Color::Red),
            Print("  Actual:\n"),
            ResetColor,
        )
        .unwrap();
        for line in &self.actual {
            writeln!(f, "    {}", line)?;
        }
        Ok(())
    }
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

fn local_print(
    _heap: &mut Heap,
    _class_store: &CoreClassStore,
    _string_store: &mut ObjStringStore,
    args: &mut [Value],
) -> Result<Value, Error> {
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

pub fn run_test(
    heap: Rc<RefCell<Heap>>,
    string_store: Rc<RefCell<ObjStringStore>>,
    chunk_store: Rc<RefCell<ChunkStore>>,
    class_store: &CoreClassStore,
    path: &str,
) -> Result<Success, Failure> {
    let mut vm = vm::new_root_vm_with_built_ins(
        heap.clone(),
        string_store.clone(),
        chunk_store.clone(),
        Box::new(class_store.clone()),
    );
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

    let result = compiler::compile(
        &mut heap.borrow_mut(),
        &mut chunk_store.borrow_mut(),
        &mut string_store.borrow_mut(),
        source,
    );
    let error_output = match result {
        Ok(f) => match vm.execute(f, &[]) {
            Ok(_) => Vec::new(),
            Err(e) => e.get_messages().clone(),
        },
        Err(e) => e.get_messages().clone(),
    };

    let mut output = OUTPUT.with(|output| mem::replace(&mut *output.borrow_mut(), vec![]));
    output.extend_from_slice(&error_output);

    if output != expected_output {
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
