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

use std::collections;
use std::rc;

use crate::chunk;
use crate::compiler;
use crate::debug;
use crate::object;
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
    globals: collections::HashMap<String, value::Value>,
}

impl Default for VM {
    fn default() -> Self {
        VM {
            ip: 0,
            chunk: Default::default(),
            stack: Vec::with_capacity(STACK_MAX),
            globals: Default::default(),
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

                chunk::OpCode::Pop => {
                    self.stack.pop();
                }

                chunk::OpCode::GetLocal => {
                    let slot = self.read_byte();
                    let value = self.stack[slot as usize].clone();
                    self.stack.push(value);
                }

                chunk::OpCode::SetLocal => {
                    let slot = self.read_byte();
                    self.stack[slot as usize] =
                        (*self.stack.last().unwrap()).clone();
                }

                chunk::OpCode::GetGlobal => {
                    let name = self.read_string();
                    match self.globals.get(&name.data) {
                        Some(value) => {
                            self.stack.push(value.clone());
                        }
                        None => {
                            let msg =
                                format!("Undefined variable '{}'.", name.data);
                            self.runtime_error(msg.as_str());
                            return InterpretResult::RuntimeError;
                        }
                    }
                }

                chunk::OpCode::DefineGlobal => {
                    let name = self.read_string();
                    self.globals.insert(
                        name.data.clone(),
                        self.stack.last().unwrap().clone(),
                    );
                    self.stack.pop();
                }

                chunk::OpCode::SetGlobal => {
                    let name = self.read_string();
                    let prev = self.globals.insert(
                        name.data.clone(),
                        self.stack.last().unwrap().clone(),
                    );
                    match prev {
                        Some(_) => {}
                        None => {
                            self.globals.remove(&name.data);
                            let msg =
                                format!("Undefined variable '{}'.", name.data);
                            self.runtime_error(msg.as_str());
                            return InterpretResult::RuntimeError;
                        }
                    }
                }

                chunk::OpCode::Equal => {
                    let b = self.stack.pop();
                    let a = self.stack.pop();
                    self.stack
                        .push(value::Value::Boolean(a.unwrap() == b.unwrap()));
                }

                chunk::OpCode::Greater => binary_op!(value::Value::Boolean, >),

                chunk::OpCode::Less => binary_op!(value::Value::Boolean, <),

                chunk::OpCode::Add => {
                    let b = self.stack.pop();
                    let a = self.stack.pop();
                    match (a.unwrap(), b.unwrap()) {
                        (
                            value::Value::ObjString(a),
                            value::Value::ObjString(b),
                        ) => self.stack.push(value::Value::from(format!(
                            "{}{}",
                            a.data, b.data
                        ))),

                        (value::Value::Number(a), value::Value::Number(b)) => {
                            self.stack.push(value::Value::Number(a + b));
                        }

                        _ => {
                            self.runtime_error(
                                "Operands must be two numbers or two strings.",
                            );
                            return InterpretResult::RuntimeError;
                        }
                    }
                }

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

                chunk::OpCode::Print => {
                    println!("{}", self.stack.pop().unwrap());
                }

                chunk::OpCode::Jump => {
                    let offset = self.read_short();
                    self.ip += offset as usize;
                }

                chunk::OpCode::JumpIfFalse => {
                    let offset = self.read_short();
                    if self.is_falsey((*self.stack.last().unwrap()).clone()) {
                        self.ip += offset as usize;
                    }
                }

                chunk::OpCode::Loop => {
                    let offset = self.read_short();
                    self.ip -= offset as usize;
                }

                chunk::OpCode::Return => {
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

    fn read_short(&mut self) -> u16 {
        let ret = ((self.chunk.code[self.ip] as u16) << 8)
            | self.chunk.code[self.ip + 1] as u16;
        self.ip += 2;
        ret
    }

    fn read_opcode(&mut self) -> chunk::OpCode {
        chunk::OpCode::from(self.read_byte())
    }

    fn read_constant(&mut self) -> value::Value {
        let idx = self.read_byte();
        self.chunk.constants[idx as usize].clone()
    }

    fn read_string(&mut self) -> rc::Rc<object::ObjString> {
        match self.read_constant() {
            value::Value::ObjString(s) => s,
            _ => panic!("Expected variable name."),
        }
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
