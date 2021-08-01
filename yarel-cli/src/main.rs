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

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

use yarel::error::{Error, ErrorKind};
use yarel::value::Value;
use yarel::vm::{self, Vm};

fn repl(vm: &mut Vm) {
    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        let mut buffer = String::new();

        match io::stdin().read_line(&mut buffer) {
            Ok(bytes) => {
                if bytes == 0 {
                    println!();
                    process::exit(0);
                }
                match vm::interpret(vm, buffer, None) {
                    Ok(_) => {}
                    Err(error) => eprint!("{}", error),
                }
            }
            _ => {
                eprintln!("Failed to read from stdin.");
                process::exit(74);
            }
        }
    }
}

fn run_file(vm: &mut Vm, path: &str) {
    let source = fs::read_to_string(path);
    let result = match source {
        Ok(contents) => vm::interpret(vm, contents, None),
        _ => panic!("Unable to read from file."),
    };

    if let Err(error) = result {
        let exit_code = if error.kind() == ErrorKind::CompileError {
            65
        } else {
            70
        };
        eprint!("{}", error);
        process::exit(exit_code);
    }
}

fn read_file(vm: &mut Vm, num_args: usize) -> Result<Value, Error> {
    if num_args != 1 {
        return Err(yarel::error!(
            ErrorKind::TypeError,
            "Expected 1 parameter but found {}.",
            num_args
        ));
    }

    let path = vm.native_arg(1).try_as_obj_string().ok_or_else(|| {
        yarel::error!(
            ErrorKind::TypeError,
            "Expected a string but found '{}'.",
            vm.native_arg(1)
        )
    })?;

    let file_contents = fs::read_to_string(path.as_str())
        .map_err(|e| yarel::error!(ErrorKind::RuntimeError, "Unable to read file: {}", e))?;

    let file_contents = vm.new_gc_obj_string(&file_contents);
    Ok(Value::ObjString(file_contents))
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut vm = Vm::with_built_ins();
    vm.define_native("main", "read_file_to_string", read_file);

    if args.len() == 1 {
        repl(&mut vm);
    } else if args.len() == 2 {
        run_file(&mut vm, &args[1]);
    } else {
        eprintln!("Usage: ./yarel-cli [path]");
        process::exit(64);
    }
}
