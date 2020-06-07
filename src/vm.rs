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

use crate::chunk;
use crate::compiler;
use crate::debug;
use crate::value;

const STACK_MAX: usize = 256;

pub enum InterpretResult {
    Ok,
    CompileError,
    RuntimeError,
}

pub fn interpret(vm: &mut VM, source: String) -> InterpretResult {
    compiler::compile(source);
    InterpretResult::Ok
}

pub struct VM {
    ip: usize,
    chunk: chunk::Chunk,
    stack: Vec<value::Value>,
}

impl Default for VM {
    fn default() -> Self {
        VM {
            ip: 0,
            chunk: Default::default(),
            stack: Vec::with_capacity(STACK_MAX),
        }
    }
}

impl VM {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn interpret(&mut self, chunk: chunk::Chunk) -> InterpretResult {
        self.ip = 0;
        self.chunk = chunk;
        self.run()
    }

    fn run(&mut self) -> InterpretResult {
        macro_rules! binary_op {
            ($op:tt) => {
                {
                    let second = self.stack.pop().unwrap();
                    let first = self.stack.pop().unwrap();
                    self.stack.push(first $op second);
                }
            };
        }

        loop {
            if cfg!(debug_assertions) {
                print!("          ");
                for v in self.stack.iter() {
                    print!("[ {} ]", v);
                }
                println!("");
                debug::disassemble_instruction(&self.chunk, self.ip);
            }
            let instruction = self.read_opcode();

            match instruction {
                chunk::OpCode::Constant => {
                    let constant = self.read_constant();
                    self.stack.push(constant);
                    println!("{}", constant);
                }
                chunk::OpCode::Add => binary_op!(+),
                chunk::OpCode::Subtract => binary_op!(-),
                chunk::OpCode::Multiply => binary_op!(*),
                chunk::OpCode::Divide => binary_op!(/),
                chunk::OpCode::Negate => {
                    let value = self.stack.pop().unwrap();
                    self.stack.push(-value);
                }
                chunk::OpCode::Return => {
                    println!("{}", self.stack.pop().unwrap());
                    return InterpretResult::Ok;
                }
            }
        }
    }

    fn read_byte(&mut self) -> u8 {
        let ret = self.chunk.code[self.ip];
        self.ip += 1;
        return ret;
    }

    fn read_opcode(&mut self) -> chunk::OpCode {
        chunk::OpCode::from(self.read_byte())
    }

    fn read_constant(&mut self) -> value::Value {
        let idx = self.read_byte();
        self.chunk.constants[idx as usize]
    }
}
