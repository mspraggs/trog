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
    let compile_result = compiler::compile(source);
    match compile_result {
        Some(chunk) => vm.interpret(chunk),
        None => InterpretResult::CompileError,
    }
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
            ($value_type:expr, $op:tt) => {
                {
                    let second_value = self.stack.pop().unwrap();
                    let first_value = self.stack.pop().unwrap();
                    let (first, second) = match (first_value, second_value) {
                        (
                            value::Value::Number(first),
                            value::Value::Number(second)
                        ) => (first, second),
                        _ => {
                            self.runtime_error("Operands must be numbers.");
                            return InterpretResult::RuntimeError;
                        }
                    };
                    self.stack.push($value_type(first $op second));
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
                    println!("{}", constant);
                    self.stack.push(constant);
                }
                chunk::OpCode::Nil => {
                    self.stack.push(value::Value::None);
                }
                chunk::OpCode::True => {
                    self.stack.push(value::Value::Boolean(true));
                }
                chunk::OpCode::False => {
                    self.stack.push(value::Value::Boolean(false));
                }
                chunk::OpCode::Equal => {
                    let b = self.stack.pop();
                    let a = self.stack.pop();
                    self.stack.push(value::Value::Boolean(a == b));
                }
                chunk::OpCode::Greater => binary_op!(value::Value::Boolean, >),
                chunk::OpCode::Less => binary_op!(value::Value::Boolean, <),
                chunk::OpCode::Add => binary_op!(value::Value::Number, +),
                chunk::OpCode::Subtract => binary_op!(value::Value::Number, -),
                chunk::OpCode::Multiply => binary_op!(value::Value::Number, *),
                chunk::OpCode::Divide => binary_op!(value::Value::Number, /),
                chunk::OpCode::Not => {
                    let value = self.stack.pop().unwrap();
                    self.stack
                        .push(value::Value::Boolean(self.is_falsey(value)));
                }
                chunk::OpCode::Negate => {
                    let value = self.stack.pop().unwrap();
                    match value {
                        value::Value::Number(underlying) => {
                            self.stack.push(value::Value::Number(-underlying));
                        }
                        _ => {
                            self.runtime_error("Operand must be a number.");
                            return InterpretResult::RuntimeError;
                        }
                    }
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

    fn is_falsey(&self, value: value::Value) -> bool {
        match value {
            value::Value::None => true,
            value::Value::Boolean(underlying) => !underlying,
            _ => false,
        }
    }

    fn runtime_error(&self, message: &str) {
        eprintln!("{}", message);
        eprintln!("[line {}] in script", self.chunk.lines[self.ip - 1]);
    }
}
