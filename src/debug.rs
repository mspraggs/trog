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
use crate::value;

pub fn disassemble_chunk(chunk: &chunk::Chunk, name: &str) {
    println!("=== {} ===", name);

    let mut offset = 0;
    while offset < chunk.code.len() {
        offset = disassemble_instruction(chunk, offset);
    }
}

pub fn disassemble_instruction(chunk: &chunk::Chunk, offset: usize) -> usize {
    print!("{:04} ", offset);

    if offset > 0 && chunk.lines[offset] == chunk.lines[offset - 1] {
        print!("   | ");
    } else {
        print!("{:4} ", chunk.lines[offset]);
    }

    let instruction = chunk::OpCode::from(chunk.code[offset]);
    match instruction {
        chunk::OpCode::Constant => constant_instruction("CONSTANT", chunk, offset),
        chunk::OpCode::Nil => simple_instruction("NIL", offset),
        chunk::OpCode::True => simple_instruction("TRUE", offset),
        chunk::OpCode::False => simple_instruction("FALSE", offset),
        chunk::OpCode::Pop => simple_instruction("POP", offset),
        chunk::OpCode::GetLocal => byte_instruction("GET_LOCAL", chunk, offset),
        chunk::OpCode::SetLocal => byte_instruction("SET_LOCAL", chunk, offset),
        chunk::OpCode::GetGlobal => constant_instruction("GET_GLOBAL", chunk, offset),
        chunk::OpCode::DefineGlobal => constant_instruction("DEFINE_GLOBAL", chunk, offset),
        chunk::OpCode::SetGlobal => constant_instruction("SET_GLOBAL", chunk, offset),
        chunk::OpCode::GetUpvalue => byte_instruction("GET_UPVALUE", chunk, offset),
        chunk::OpCode::SetUpvalue => byte_instruction("SET_UPVALUE", chunk, offset),
        chunk::OpCode::GetProperty => constant_instruction("GET_PROPERTY", chunk, offset),
        chunk::OpCode::SetProperty => constant_instruction("SET_PROPERTY", chunk, offset),
        chunk::OpCode::GetSuper => constant_instruction("GET_SUPER", chunk, offset),
        chunk::OpCode::Equal => simple_instruction("EQUAL", offset),
        chunk::OpCode::Greater => simple_instruction("GREATER", offset),
        chunk::OpCode::Less => simple_instruction("LESS", offset),
        chunk::OpCode::Add => simple_instruction("ADD", offset),
        chunk::OpCode::Subtract => simple_instruction("SUBTRACT", offset),
        chunk::OpCode::Multiply => simple_instruction("MULTIPLY", offset),
        chunk::OpCode::Divide => simple_instruction("DIVIDE", offset),
        chunk::OpCode::Not => simple_instruction("NOT", offset),
        chunk::OpCode::Negate => simple_instruction("NEGATE", offset),
        chunk::OpCode::Print => simple_instruction("PRINT", offset),
        chunk::OpCode::Jump => jump_instruction("JUMP", 1, chunk, offset),
        chunk::OpCode::JumpIfFalse => jump_instruction("JUMP_IF_FALSE", 1, chunk, offset),
        chunk::OpCode::Loop => jump_instruction("LOOP", -1, chunk, offset),
        chunk::OpCode::Call => byte_instruction("CALL", chunk, offset),
        chunk::OpCode::Invoke => invoke_instruction("INVOKE", chunk, offset),
        chunk::OpCode::SuperInvoke => invoke_instruction("SUPER_INVOKE", chunk, offset),
        chunk::OpCode::Closure => {
            let mut offset = offset + 1;
            let constant = chunk.code[offset] as usize;
            offset += 1;
            println!(
                "{:16} {:4} {}",
                "CLOSURE", constant, chunk.constants[constant]
            );

            let function = match chunk.constants[constant] {
                value::Value::ObjFunction(ref underlying) => underlying.clone(),
                _ => panic!("Expected function object."),
            };

            for _ in 0..function.upvalue_count {
                let is_local = if chunk.code[offset] != 0 {
                    "local"
                } else {
                    "upvalue"
                };
                offset += 1;
                let index = chunk.code[offset] as usize;
                offset += 1;

                println!(
                    "{:04}      |                     {} {}",
                    offset - 2,
                    is_local,
                    index
                );
            }

            offset
        }
        chunk::OpCode::CloseUpvalue => simple_instruction("CLOSE_UPVALUE", offset),
        chunk::OpCode::Return => simple_instruction("RETURN", offset),
        chunk::OpCode::Class => constant_instruction("CLASS", chunk, offset),
        chunk::OpCode::Inherit => simple_instruction("INHERIT", offset),
        chunk::OpCode::Method => constant_instruction("METHOD", chunk, offset),
    }
}

fn simple_instruction(name: &str, offset: usize) -> usize {
    println!("{}", name);
    offset + 1
}

fn byte_instruction(name: &str, chunk: &chunk::Chunk, offset: usize) -> usize {
    let slot = chunk.code[offset + 1];
    println!("{:16} {:4}", name, slot as usize);
    offset + 2
}

fn jump_instruction(name: &str, sign: i32, chunk: &chunk::Chunk, offset: usize) -> usize {
    let jump = ((chunk.code[offset + 1] as u16) << 8) | (chunk.code[offset + 2] as u16);
    let target = (offset + 3) as isize + sign as isize * jump as isize;
    println!("{:16} {:4} -> {}", name, offset, target);
    offset + 3
}

fn constant_instruction(name: &str, chunk: &chunk::Chunk, offset: usize) -> usize {
    let constant = chunk.code[offset + 1];
    println!(
        "{:16} {:4} '{}'",
        name, constant, chunk.constants[constant as usize]
    );
    offset + 2
}

fn invoke_instruction(name: &str, chunk: &chunk::Chunk, offset: usize) -> usize {
    let constant = chunk.code[offset + 1];
    let arg_count = chunk.code[offset + 2];
    println!(
        "{:16} ({} args) {:4} '{}'",
        name, arg_count, constant, chunk.constants[constant as usize]
    );
    offset + 3
}
