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

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

mod chunk;
mod common;
mod compiler;
mod debug;
mod hash;
mod memory;
mod object;
mod scanner;
mod value;
mod vm;

fn repl(vm: &mut vm::Vm) {
    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        let mut buffer = String::new();

        match io::stdin().read_line(&mut buffer) {
            Ok(_) => {
                vm::interpret(vm, buffer).unwrap_or_default();
            }
            _ => {
                eprintln!("Failed to read from stdin.");
                process::exit(74);
            }
        }
    }
}

fn run_file(vm: &mut vm::Vm, path: &str) {
    let source = fs::read_to_string(path);
    let result = match source {
        Ok(contents) => vm::interpret(vm, contents),
        _ => panic!("Unable to read from file."),
    };

    match result {
        Err(vm::VmError::CompileError(msgs)) => {
            eprint!("{}", vm::VmError::CompileError(msgs));
            process::exit(65);
        }
        Err(error) => {
            eprint!("{}", error);
            process::exit(70);
        }
        Ok(_) => {}
    };
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut vm = memory::allocate_root(vm::Vm::new());

    if args.len() == 1 {
        repl(&mut vm);
    } else if args.len() == 2 {
        run_file(&mut vm, &args[1]);
    } else {
        eprintln!("Usage: ./dryw [path]");
        process::exit(64);
    }
}
