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

use crate::chunk::{Chunk, OpCode};
use crate::value::Value;

pub fn disassemble_chunk(chunk: &Chunk, name: &str) {
    println!("=== {} ===", name);

    let mut offset = 0;
    while offset < chunk.code.len() {
        offset = disassemble_instruction(chunk, offset);
    }
}

pub fn disassemble_instruction(chunk: &Chunk, offset: usize) -> usize {
    print!("{:04} ", offset);

    if offset > 0 && chunk.lines[offset] == chunk.lines[offset - 1] {
        print!("   | ");
    } else {
        print!("{:4} ", chunk.lines[offset]);
    }

    let instruction = OpCode::from(chunk.code[offset]);
    match instruction {
        OpCode::Constant => constant_instruction("CONSTANT", chunk, offset),
        OpCode::Nil => simple_instruction("NIL", offset),
        OpCode::True => simple_instruction("TRUE", offset),
        OpCode::False => simple_instruction("FALSE", offset),
        OpCode::Pop => simple_instruction("POP", offset),
        OpCode::CopyTop => simple_instruction("COPY_TOP", offset),
        OpCode::GetLocal => byte_instruction("GET_LOCAL", chunk, offset),
        OpCode::SetLocal => byte_instruction("SET_LOCAL", chunk, offset),
        OpCode::GetGlobal => constant_instruction("GET_GLOBAL", chunk, offset),
        OpCode::DefineGlobal => constant_instruction("DEFINE_GLOBAL", chunk, offset),
        OpCode::SetGlobal => constant_instruction("SET_GLOBAL", chunk, offset),
        OpCode::GetUpvalue => byte_instruction("GET_UPVALUE", chunk, offset),
        OpCode::SetUpvalue => byte_instruction("SET_UPVALUE", chunk, offset),
        OpCode::GetProperty => constant_instruction("GET_PROPERTY", chunk, offset),
        OpCode::SetProperty => constant_instruction("SET_PROPERTY", chunk, offset),
        OpCode::GetClass => simple_instruction("GET_CLASS", offset),
        OpCode::GetSuper => constant_instruction("GET_SUPER", chunk, offset),
        OpCode::Equal => simple_instruction("EQUAL", offset),
        OpCode::Greater => simple_instruction("GREATER", offset),
        OpCode::Less => simple_instruction("LESS", offset),
        OpCode::Add => simple_instruction("ADD", offset),
        OpCode::Subtract => simple_instruction("SUBTRACT", offset),
        OpCode::Multiply => simple_instruction("MULTIPLY", offset),
        OpCode::Divide => simple_instruction("DIVIDE", offset),
        OpCode::Not => simple_instruction("NOT", offset),
        OpCode::Negate => simple_instruction("NEGATE", offset),
        OpCode::BuildHashMap => byte_instruction("BUILD_HASH_MAP", chunk, offset),
        OpCode::BuildRange => simple_instruction("BUILD_RANGE", offset),
        OpCode::BuildString => byte_instruction("BUILD_STRING", chunk, offset),
        OpCode::FormatString => simple_instruction("FORMAT_STRING", offset),
        OpCode::BuildVec => byte_instruction("BUILD_VEC", chunk, offset),
        OpCode::IterNext => simple_instruction("ITER_NEXT", offset),
        OpCode::Jump => jump_instruction("JUMP", 1, chunk, offset),
        OpCode::JumpIfFalse => jump_instruction("JUMP_IF_FALSE", 1, chunk, offset),
        OpCode::JumpIfSentinel => jump_instruction("JUMP_IF_SENTINEL", 1, chunk, offset),
        OpCode::Loop => jump_instruction("LOOP", -1, chunk, offset),
        OpCode::Call => byte_instruction("CALL", chunk, offset),
        OpCode::Invoke => invoke_instruction("INVOKE", chunk, offset),
        OpCode::SuperInvoke => invoke_instruction("SUPER_INVOKE", chunk, offset),
        OpCode::Closure => {
            let mut offset = offset + 1;
            let constant =
                u16::from_ne_bytes([chunk.code[offset], chunk.code[offset + 1]]) as usize;
            offset += 2;
            println!(
                "{:16} {:4} {}",
                "CLOSURE", constant, chunk.constants[constant]
            );

            let function = match chunk.constants[constant] {
                Value::ObjFunction(ref underlying) => underlying,
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
        OpCode::CloseUpvalue => simple_instruction("CLOSE_UPVALUE", offset),
        OpCode::Return => simple_instruction("RETURN", offset),
        OpCode::DeclareClass => constant_instruction("DECLARE_CLASS", chunk, offset),
        OpCode::DefineClass => simple_instruction("DEFINE_CLASS", offset),
        OpCode::Inherit => simple_instruction("INHERIT", offset),
        OpCode::Method => constant_instruction("METHOD", chunk, offset),
        OpCode::StaticMethod => constant_instruction("STATIC_METHOD", chunk, offset),
    }
}

fn simple_instruction(name: &str, offset: usize) -> usize {
    println!("{}", name);
    offset + 1
}

fn byte_instruction(name: &str, chunk: &Chunk, offset: usize) -> usize {
    let slot = chunk.code[offset + 1];
    println!("{:16} {:4}", name, slot as usize);
    offset + 2
}

fn jump_instruction(name: &str, sign: i32, chunk: &Chunk, offset: usize) -> usize {
    let jump = u16::from_ne_bytes([chunk.code[offset + 1], chunk.code[offset + 2]]);
    let target = (offset + 3) as isize + sign as isize * jump as isize;
    println!("{:16} {:4} -> {}", name, offset, target);
    offset + 3
}

fn constant_instruction(name: &str, chunk: &Chunk, offset: usize) -> usize {
    let constant = u16::from_ne_bytes([chunk.code[offset + 1], chunk.code[offset + 2]]);
    println!(
        "{:16} {:4} '{}'",
        name, constant, chunk.constants[constant as usize]
    );
    offset + 3
}

fn invoke_instruction(name: &str, chunk: &Chunk, offset: usize) -> usize {
    let constant = u16::from_ne_bytes([chunk.code[offset + 1], chunk.code[offset + 2]]);
    let arg_count = chunk.code[offset + 3];
    println!(
        "{:16} ({} args) {:4} '{}'",
        name, arg_count, constant, chunk.constants[constant as usize]
    );
    offset + 4
}
