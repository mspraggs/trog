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

mod chunk;
mod debug;
mod value;
mod vm;

fn main() {
    let mut vm = vm::VM::new();
    let mut chunk = chunk::Chunk::new();

    let constant0 = chunk.add_constant(1.2);
    chunk.write(chunk::OpCode::Constant as u8, 123);
    chunk.write(constant0 as u8, 123);

    let constant1 = chunk.add_constant(3.4);
    chunk.write(chunk::OpCode::Constant as u8, 123);
    chunk.write(constant1 as u8, 123);

    chunk.write(chunk::OpCode::Add as u8, 123);

    let constant2 = chunk.add_constant(5.6);
    chunk.write(chunk::OpCode::Constant as u8, 123);
    chunk.write(constant2 as u8, 123);

    chunk.write(chunk::OpCode::Divide as u8, 123);
    chunk.write(chunk::OpCode::Negate as u8, 123);
    chunk.write(chunk::OpCode::Return as u8, 123);
    debug::disassemble_chunk(&chunk, "test chunk");
    vm.interpret(chunk);
}
