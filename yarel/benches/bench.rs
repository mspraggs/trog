/* Copyright 2021 Matt Spraggs
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

use criterion::{criterion_group, criterion_main, Criterion};

use yarel::vm;

const FIBONACCI_SOURCE: &str = "
fn fib(n) {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}
fib(30);
";

const FOR_LOOP_SOURCE: &str = "
for _ in 0..10000000 {}
";

const WHILE_LOOP_SOURCE: &str = "
var i = 1000000;
while i > 0 {
    i -= 1;
}
";

const STRING_COMPARE_SOURCE: &str = "
var a = \"one\";
var b = \"two\";
var i = 1000000;
while i > 0 {
    i -= 1;
    a == b;
}
";

fn criterion_benchmark(c: &mut Criterion) {
    let mut vm = vm::Vm::with_built_ins();

    c.bench_function("fib 30", |b| {
        b.iter(|| vm::interpret(&mut vm, FIBONACCI_SOURCE.to_string(), None))
    });

    c.bench_function("for loop 10m", |b| {
        b.iter(|| vm::interpret(&mut vm, FOR_LOOP_SOURCE.to_string(), None))
    });

    c.bench_function("string compare 1m", |b| {
        b.iter(|| vm::interpret(&mut vm, STRING_COMPARE_SOURCE.to_string(), None))
    });

    c.bench_function("while loop 1m", |b| {
        b.iter(|| vm::interpret(&mut vm, WHILE_LOOP_SOURCE.to_string(), None))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
