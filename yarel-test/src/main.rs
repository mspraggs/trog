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

use std::{io, process};

mod test;
mod utils;

use yarel::vm::Vm;

fn main() {
    let paths = match utils::get_paths("tests", Some(".yl")) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Error reading test paths");
            return;
        }
    };

    let mut num_passed = 0;
    let mut num_skipped = 0;
    let mut num_failed = 0;
    let mut stdout = io::stdout();

    let mut vm = Vm::with_built_ins();

    let failures: Vec<test::Failure> = paths
        .iter()
        .map(|p| {
            let ret = test::run_test(p, &mut vm);
            match ret {
                Ok(ref success) => {
                    if !success.skipped {
                        num_passed += 1;
                    } else {
                        num_skipped += 1;
                    }
                }
                Err(_) => {
                    num_failed += 1;
                }
            };
            utils::print_stats(&mut stdout, num_passed, num_skipped, num_failed);
            ret
        })
        .filter(|r| r.is_err())
        .map(|r| r.unwrap_err())
        .collect();
    println!();

    let tests_failed = !failures.is_empty();

    if tests_failed {
        println!();
        println!("Failing tests:");
    }

    for fail in failures {
        utils::write_failure(&mut stdout, &fail);
    }

    if tests_failed {
        process::exit(1);
    }
}
