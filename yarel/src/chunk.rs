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

use crate::memory;
use crate::value;

#[repr(u8)]
pub enum OpCode {
    Constant,
    Nil,
    True,
    False,
    Pop,
    CopyTop,
    GetLocal,
    SetLocal,
    GetGlobal,
    DefineGlobal,
    SetGlobal,
    GetUpvalue,
    SetUpvalue,
    GetProperty,
    SetProperty,
    GetClass,
    GetSuper,
    Equal,
    Greater,
    Less,
    Add,
    Subtract,
    Multiply,
    Divide,
    Not,
    Negate,
    FormatString,
    BuildHashMap,
    BuildRange,
    BuildString,
    BuildTuple,
    BuildVec,
    IterNext,
    Jump,
    JumpIfFalse,
    JumpIfSentinel,
    Loop,
    Call,
    Invoke,
    Construct,
    SuperInvoke,
    Closure,
    CloseUpvalue,
    Return,
    DeclareClass,
    DefineClass,
    Inherit,
    Method,
    StaticMethod,
    StartImport,
    FinishImport,
}

impl OpCode {
    pub(crate) fn arg_sizes(&self) -> &[usize] {
        match self {
            OpCode::Constant => &[2],
            OpCode::Nil => &[],
            OpCode::True => &[],
            OpCode::False => &[],
            OpCode::Pop => &[],
            OpCode::CopyTop => &[],
            OpCode::GetLocal => &[1],
            OpCode::SetLocal => &[1],
            OpCode::GetGlobal => &[2],
            OpCode::DefineGlobal => &[2],
            OpCode::SetGlobal => &[2],
            OpCode::GetUpvalue => &[1],
            OpCode::SetUpvalue => &[1],
            OpCode::GetProperty => &[2],
            OpCode::SetProperty => &[2],
            OpCode::GetClass => &[],
            OpCode::GetSuper => &[2],
            OpCode::Equal => &[],
            OpCode::Greater => &[],
            OpCode::Less => &[],
            OpCode::Add => &[],
            OpCode::Subtract => &[],
            OpCode::Multiply => &[],
            OpCode::Divide => &[],
            OpCode::Not => &[],
            OpCode::Negate => &[],
            OpCode::FormatString => &[],
            OpCode::BuildHashMap => &[1],
            OpCode::BuildRange => &[],
            OpCode::BuildString => &[1],
            OpCode::BuildTuple => &[1],
            OpCode::BuildVec => &[1],
            OpCode::IterNext => &[],
            OpCode::Jump => &[2],
            OpCode::JumpIfFalse => &[2],
            OpCode::JumpIfSentinel => &[2],
            OpCode::Loop => &[2],
            OpCode::Call => &[1],
            OpCode::Invoke => &[2, 1],
            OpCode::Construct => &[1],
            OpCode::SuperInvoke => &[2, 1],
            OpCode::Closure => &[2],
            OpCode::CloseUpvalue => &[],
            OpCode::Return => &[],
            OpCode::DeclareClass => &[2],
            OpCode::DefineClass => &[],
            OpCode::Inherit => &[],
            OpCode::Method => &[2],
            OpCode::StaticMethod => &[2],
            OpCode::StartImport => &[2],
            OpCode::FinishImport => &[],
        }
    }
}

impl From<u8> for OpCode {
    fn from(value: u8) -> Self {
        match value {
            value if value == OpCode::Constant as u8 => OpCode::Constant,
            value if value == OpCode::Nil as u8 => OpCode::Nil,
            value if value == OpCode::True as u8 => OpCode::True,
            value if value == OpCode::False as u8 => OpCode::False,
            value if value == OpCode::Pop as u8 => OpCode::Pop,
            value if value == OpCode::CopyTop as u8 => OpCode::CopyTop,
            value if value == OpCode::GetLocal as u8 => OpCode::GetLocal,
            value if value == OpCode::SetLocal as u8 => OpCode::SetLocal,
            value if value == OpCode::GetGlobal as u8 => OpCode::GetGlobal,
            value if value == OpCode::DefineGlobal as u8 => OpCode::DefineGlobal,
            value if value == OpCode::SetGlobal as u8 => OpCode::SetGlobal,
            value if value == OpCode::GetUpvalue as u8 => OpCode::GetUpvalue,
            value if value == OpCode::SetUpvalue as u8 => OpCode::SetUpvalue,
            value if value == OpCode::GetProperty as u8 => OpCode::GetProperty,
            value if value == OpCode::SetProperty as u8 => OpCode::SetProperty,
            value if value == OpCode::GetClass as u8 => OpCode::GetClass,
            value if value == OpCode::GetSuper as u8 => OpCode::GetSuper,
            value if value == OpCode::Equal as u8 => OpCode::Equal,
            value if value == OpCode::Greater as u8 => OpCode::Greater,
            value if value == OpCode::Less as u8 => OpCode::Less,
            value if value == OpCode::Add as u8 => OpCode::Add,
            value if value == OpCode::Subtract as u8 => OpCode::Subtract,
            value if value == OpCode::Multiply as u8 => OpCode::Multiply,
            value if value == OpCode::Divide as u8 => OpCode::Divide,
            value if value == OpCode::Not as u8 => OpCode::Not,
            value if value == OpCode::Negate as u8 => OpCode::Negate,
            value if value == OpCode::FormatString as u8 => OpCode::FormatString,
            value if value == OpCode::BuildHashMap as u8 => OpCode::BuildHashMap,
            value if value == OpCode::BuildRange as u8 => OpCode::BuildRange,
            value if value == OpCode::BuildString as u8 => OpCode::BuildString,
            value if value == OpCode::BuildTuple as u8 => OpCode::BuildTuple,
            value if value == OpCode::BuildVec as u8 => OpCode::BuildVec,
            value if value == OpCode::IterNext as u8 => OpCode::IterNext,
            value if value == OpCode::Jump as u8 => OpCode::Jump,
            value if value == OpCode::JumpIfFalse as u8 => OpCode::JumpIfFalse,
            value if value == OpCode::JumpIfSentinel as u8 => OpCode::JumpIfSentinel,
            value if value == OpCode::Loop as u8 => OpCode::Loop,
            value if value == OpCode::Call as u8 => OpCode::Call,
            value if value == OpCode::Invoke as u8 => OpCode::Invoke,
            value if value == OpCode::Construct as u8 => OpCode::Construct,
            value if value == OpCode::SuperInvoke as u8 => OpCode::SuperInvoke,
            value if value == OpCode::Closure as u8 => OpCode::Closure,
            value if value == OpCode::CloseUpvalue as u8 => OpCode::CloseUpvalue,
            value if value == OpCode::Return as u8 => OpCode::Return,
            value if value == OpCode::DeclareClass as u8 => OpCode::DeclareClass,
            value if value == OpCode::DefineClass as u8 => OpCode::DefineClass,
            value if value == OpCode::Inherit as u8 => OpCode::Inherit,
            value if value == OpCode::Method as u8 => OpCode::Method,
            value if value == OpCode::StaticMethod as u8 => OpCode::StaticMethod,
            value if value == OpCode::StartImport as u8 => OpCode::StartImport,
            value if value == OpCode::FinishImport as u8 => OpCode::FinishImport,
            _ => panic!("Unknown opcode {}", value),
        }
    }
}

#[derive(Clone, Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub lines: Vec<i32>,
    pub constants: Vec<value::Value>,
}

impl Chunk {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn write(&mut self, byte: u8, line: i32) {
        self.code.push(byte);
        self.lines.push(line);
    }

    pub fn add_constant(&mut self, value: value::Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    pub(crate) fn code_offset(&self, ptr: *const u8) -> usize {
        ptr as usize - (&self.code[0] as *const u8) as usize
    }
}

impl memory::GcManaged for Chunk {
    fn mark(&self) {
        self.constants.mark();
    }

    fn blacken(&self) {
        self.constants.blacken();
    }
}
