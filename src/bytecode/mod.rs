//! Bytecode representation and instruction set
//!
//! This module defines the bytecode format used by the Quicksilver VM.
//! The bytecode is a register-based format designed for efficient interpretation.

//! **Status:** ✅ Complete — Compiler, opcodes, optimizer — production-ready

mod compiler;
mod opcode;
mod optimizer;
pub mod cache;

pub use compiler::{compile, compile_with_source_file, Compiler};
pub use opcode::Opcode;
pub use optimizer::{Optimizer, OptimizerConfig};
pub use cache::{BytecodeCache, CacheConfig, CacheStats, compile_cached, clear_cache, cache_stats, global_cache};

use crate::runtime::Value;
use std::fmt;

/// A compiled bytecode chunk
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    /// Bytecode instructions
    pub code: Vec<u8>,
    /// Constant pool
    pub constants: Vec<Value>,
    /// Line number information for debugging
    pub lines: Vec<u32>,
    /// Column number information for debugging (source maps)
    pub columns: Vec<u32>,
    /// Local variable names for debugging
    pub locals: Vec<String>,
    /// Number of registers needed
    pub register_count: u8,
    /// Number of parameters (not including rest param)
    pub param_count: u8,
    /// Has rest parameter (...args)
    pub has_rest_param: bool,
    /// Is this a generator function?
    pub is_generator: bool,
    /// Is this an async function?
    pub is_async: bool,
    /// Is this code in strict mode?
    pub is_strict: bool,
    /// Source filename (for source maps)
    pub source_file: Option<String>,
}

impl Chunk {
    /// Create a new empty chunk
    pub fn new() -> Self {
        Self::default()
    }

    /// Write a byte to the chunk with line information only
    pub fn write(&mut self, byte: u8, line: u32) {
        self.code.push(byte);
        self.lines.push(line);
        self.columns.push(1); // Default column
    }

    /// Write a byte to the chunk with full source location
    pub fn write_with_location(&mut self, byte: u8, line: u32, column: u32) {
        self.code.push(byte);
        self.lines.push(line);
        self.columns.push(column);
    }

    /// Get the line number for a bytecode offset
    pub fn get_line(&self, offset: usize) -> u32 {
        self.lines.get(offset).copied().unwrap_or(1)
    }

    /// Get the column number for a bytecode offset
    pub fn get_column(&self, offset: usize) -> u32 {
        self.columns.get(offset).copied().unwrap_or(1)
    }

    /// Get both line and column for a bytecode offset
    pub fn get_location(&self, offset: usize) -> (u32, u32) {
        (self.get_line(offset), self.get_column(offset))
    }

    /// Write an opcode to the chunk with line information only
    pub fn write_opcode(&mut self, opcode: Opcode, line: u32) {
        self.write(opcode as u8, line);
    }

    /// Write an opcode to the chunk with full source location
    pub fn write_opcode_with_location(&mut self, opcode: Opcode, line: u32, column: u32) {
        self.write_with_location(opcode as u8, line, column);
    }

    /// Add a constant to the pool and return its index
    pub fn add_constant(&mut self, value: Value) -> u16 {
        // Check if constant already exists
        for (i, existing) in self.constants.iter().enumerate() {
            if existing.strict_equals(&value) {
                return i as u16;
            }
        }

        let index = self.constants.len();
        self.constants.push(value);
        index as u16
    }

    /// Get a constant from the pool
    pub fn get_constant(&self, index: u16) -> Option<&Value> {
        self.constants.get(index as usize)
    }

    /// Disassemble the chunk for debugging
    pub fn disassemble(&self, name: &str) -> String {
        let mut output = format!("== {} ==\n", name);
        let mut offset = 0;

        while offset < self.code.len() {
            let (instruction, new_offset) = self.disassemble_instruction(offset);
            output.push_str(&instruction);
            output.push('\n');
            offset = new_offset;
        }

        output
    }

    /// Disassemble a single instruction
    pub fn disassemble_instruction(&self, offset: usize) -> (String, usize) {
        let line = self.lines.get(offset).copied().unwrap_or(0);
        let opcode = Opcode::from_u8(self.code[offset]);

        let line_str = if offset > 0 && self.lines.get(offset - 1) == Some(&line) {
            "   |".to_string()
        } else {
            format!("{:4}", line)
        };

        match opcode {
            Some(op) => {
                let (operands, size) = self.format_operands(op, offset + 1);
                let instruction = format!(
                    "{:04} {} {:16} {}",
                    offset,
                    line_str,
                    format!("{:?}", op),
                    operands
                );
                (instruction, offset + 1 + size)
            }
            None => {
                let instruction =
                    format!("{:04} {} UNKNOWN({})", offset, line_str, self.code[offset]);
                (instruction, offset + 1)
            }
        }
    }

    fn format_operands(&self, opcode: Opcode, offset: usize) -> (String, usize) {
        match opcode {
            // No operands
            Opcode::Nop
            | Opcode::Pop
            | Opcode::Dup
            | Opcode::Swap
            | Opcode::Return
            | Opcode::ReturnUndefined
            | Opcode::Throw
            | Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Mod
            | Opcode::Pow
            | Opcode::Neg
            | Opcode::Not
            | Opcode::BitwiseNot
            | Opcode::BitwiseAnd
            | Opcode::BitwiseOr
            | Opcode::BitwiseXor
            | Opcode::Shl
            | Opcode::Shr
            | Opcode::UShr
            | Opcode::Eq
            | Opcode::Ne
            | Opcode::StrictEq
            | Opcode::StrictNe
            | Opcode::Lt
            | Opcode::Le
            | Opcode::Gt
            | Opcode::Ge
            | Opcode::In
            | Opcode::Instanceof
            | Opcode::Typeof
            | Opcode::Void
            | Opcode::Delete
            | Opcode::This
            | Opcode::Super
            | Opcode::NewTarget
            | Opcode::Undefined
            | Opcode::Null
            | Opcode::True
            | Opcode::False
            | Opcode::Increment
            | Opcode::Decrement
            | Opcode::GetIterator
            | Opcode::IteratorNext
            | Opcode::IteratorDone
            | Opcode::IteratorValue
            | Opcode::Yield
            | Opcode::Await
            | Opcode::ExportAll
            | Opcode::SetSuperClass
            | Opcode::DynamicImport
            | Opcode::MatchPattern
            | Opcode::MatchEnd => ("".to_string(), 0),

            // 1-byte operand (register or arg count)
            Opcode::LoadReg
            | Opcode::StoreReg
            | Opcode::Call
            | Opcode::TailCall
            | Opcode::New
            | Opcode::CreateArray
            | Opcode::CreateObject
            | Opcode::SuperCall
            | Opcode::MatchBind => {
                if offset < self.code.len() {
                    (format!("r{}", self.code[offset]), 1)
                } else {
                    ("???".to_string(), 0)
                }
            }

            // 2-byte operand (constant index)
            Opcode::Constant
            | Opcode::GetGlobal
            | Opcode::TryGetGlobal
            | Opcode::SetGlobal
            | Opcode::DefineGlobal
            | Opcode::GetProperty
            | Opcode::SetProperty
            | Opcode::DefineProperty
            | Opcode::DeleteProperty
            | Opcode::GetPrivateField
            | Opcode::SetPrivateField
            | Opcode::DefinePrivateField
            | Opcode::CreateFunction
            | Opcode::CreateClass
            | Opcode::CreateClosure
            | Opcode::LoadModule
            | Opcode::ExportValue
            | Opcode::SuperGet => {
                if offset + 1 < self.code.len() {
                    let index = u16::from_le_bytes([self.code[offset], self.code[offset + 1]]);
                    if let Some(constant) = self.constants.get(index as usize) {
                        (format!("{} ({})", index, constant), 2)
                    } else {
                        (format!("{}", index), 2)
                    }
                } else {
                    ("???".to_string(), 0)
                }
            }

            // 2-byte operand (jump offset)
            Opcode::Jump
            | Opcode::JumpIfFalse
            | Opcode::JumpIfTrue
            | Opcode::JumpIfNull
            | Opcode::JumpIfNotNull => {
                if offset + 1 < self.code.len() {
                    let jump = i16::from_le_bytes([self.code[offset], self.code[offset + 1]]);
                    let target = offset as i32 + 2 + jump as i32;
                    (format!("{} -> {}", jump, target), 2)
                } else {
                    ("???".to_string(), 0)
                }
            }

            // 1-byte operand (local index)
            Opcode::GetLocal | Opcode::SetLocal => {
                if offset < self.code.len() {
                    let index = self.code[offset];
                    if let Some(name) = self.locals.get(index as usize) {
                        (format!("{} ({})", index, name), 1)
                    } else {
                        (format!("{}", index), 1)
                    }
                } else {
                    ("???".to_string(), 0)
                }
            }

            // 2-byte operand (upvalue index)
            Opcode::GetUpvalue | Opcode::SetUpvalue | Opcode::CloseUpvalue => {
                if offset + 1 < self.code.len() {
                    let index = u16::from_le_bytes([self.code[offset], self.code[offset + 1]]);
                    (format!("{}", index), 2)
                } else {
                    ("???".to_string(), 0)
                }
            }

            Opcode::GetElement | Opcode::SetElement => ("".to_string(), 0),

            Opcode::EnterTry | Opcode::LeaveTry | Opcode::EnterWith | Opcode::LeaveWith => {
                if offset + 1 < self.code.len() {
                    let offset = u16::from_le_bytes([self.code[offset], self.code[offset + 1]]);
                    (format!("{}", offset), 2)
                } else {
                    ("???".to_string(), 0)
                }
            }

            Opcode::Spread | Opcode::RestParam => ("".to_string(), 0),

            // 3-byte operand (name_index u16 + arg_count u8)
            Opcode::CallMethod => {
                if offset + 2 < self.code.len() {
                    let name_idx = u16::from_le_bytes([self.code[offset], self.code[offset + 1]]);
                    let arg_count = self.code[offset + 2];
                    if let Some(constant) = self.constants.get(name_idx as usize) {
                        (format!("{} ({}) args={}", name_idx, constant, arg_count), 3)
                    } else {
                        (format!("{} args={}", name_idx, arg_count), 3)
                    }
                } else {
                    ("???".to_string(), 0)
                }
            }

            // 5-byte operand (effect_type_index u16 + operation_index u16 + arg_count u8)
            Opcode::Perform => {
                if offset + 4 < self.code.len() {
                    let effect_idx =
                        u16::from_le_bytes([self.code[offset], self.code[offset + 1]]);
                    let op_idx =
                        u16::from_le_bytes([self.code[offset + 2], self.code[offset + 3]]);
                    let arg_count = self.code[offset + 4];
                    let effect_name = self
                        .constants
                        .get(effect_idx as usize)
                        .map(|v| format!("{}", v))
                        .unwrap_or_else(|| "?".to_string());
                    let op_name = self
                        .constants
                        .get(op_idx as usize)
                        .map(|v| format!("{}", v))
                        .unwrap_or_else(|| "?".to_string());
                    (
                        format!("{}.{} args={}", effect_name, op_name, arg_count),
                        5,
                    )
                } else {
                    ("???".to_string(), 0)
                }
            }
        }
    }
}

impl fmt::Display for Chunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.disassemble("chunk"))
    }
}

/// A compiled function
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    /// Function name (if any)
    pub name: Option<String>,
    /// Bytecode chunk
    pub chunk: Chunk,
    /// Upvalue descriptors
    pub upvalues: Vec<UpvalueDescriptor>,
}

/// Describes how to capture an upvalue
#[derive(Debug, Clone, Copy)]
pub struct UpvalueDescriptor {
    /// Index in the enclosing scope
    pub index: u8,
    /// Is this a local in the enclosing scope, or an upvalue?
    pub is_local: bool,
}

impl CompiledFunction {
    /// Create a new compiled function
    pub fn new(name: Option<String>, chunk: Chunk) -> Self {
        Self {
            name,
            chunk,
            upvalues: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_creation() {
        let mut chunk = Chunk::new();
        chunk.write_opcode(Opcode::Constant, 1);
        chunk.write(0, 1);
        chunk.write(0, 1);
        chunk.add_constant(Value::Number(42.0));

        assert_eq!(chunk.code.len(), 3);
        assert_eq!(chunk.constants.len(), 1);
    }

    #[test]
    fn test_disassemble() {
        let mut chunk = Chunk::new();
        chunk.write_opcode(Opcode::Constant, 1);
        let idx = chunk.add_constant(Value::Number(42.0));
        chunk.write((idx & 0xFF) as u8, 1);
        chunk.write((idx >> 8) as u8, 1);
        chunk.write_opcode(Opcode::Return, 1);

        let output = chunk.disassemble("test");
        assert!(output.contains("Constant"));
        assert!(output.contains("42"));
        assert!(output.contains("Return"));
    }
}
