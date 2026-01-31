//! Bytecode optimization passes for Quicksilver
//!
//! This module implements various optimization passes that can be applied to
//! compiled bytecode to improve runtime performance.

use super::{Chunk, Opcode};
use crate::runtime::Value;

/// Configuration for the bytecode optimizer
#[derive(Debug, Clone)]
pub struct OptimizerConfig {
    /// Enable constant folding
    pub constant_folding: bool,
    /// Enable dead code elimination
    pub dead_code_elimination: bool,
    /// Enable peephole optimizations
    pub peephole: bool,
    /// Enable jump threading
    pub jump_threading: bool,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            constant_folding: true,
            dead_code_elimination: true,
            peephole: true,
            jump_threading: true,
        }
    }
}

/// Bytecode optimizer
pub struct Optimizer {
    config: OptimizerConfig,
}

impl Optimizer {
    /// Create a new optimizer with default configuration
    pub fn new() -> Self {
        Self {
            config: OptimizerConfig::default(),
        }
    }

    /// Create an optimizer with custom configuration
    pub fn with_config(config: OptimizerConfig) -> Self {
        Self { config }
    }

    /// Optimize a bytecode chunk
    pub fn optimize(&self, chunk: &mut Chunk) {
        // Apply optimization passes in order
        let mut changed = true;
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 10;

        // Keep iterating until no more changes or max iterations reached
        while changed && iterations < MAX_ITERATIONS {
            changed = false;
            iterations += 1;

            if self.config.constant_folding {
                changed |= self.constant_folding(chunk);
            }

            if self.config.peephole {
                changed |= self.peephole_optimize(chunk);
            }

            if self.config.dead_code_elimination {
                changed |= self.dead_code_elimination(chunk);
            }

            if self.config.jump_threading {
                changed |= self.jump_threading(chunk);
            }
        }
    }

    /// Constant folding: evaluate constant expressions at compile time
    fn constant_folding(&self, chunk: &mut Chunk) -> bool {
        let mut changed = false;
        let mut i = 0;

        while i + 6 < chunk.code.len() {
            // Look for pattern: Constant A, Constant B, BinaryOp
            if let (Some(Opcode::Constant), Some(Opcode::Constant)) = (
                Opcode::from_u8(chunk.code[i]),
                Opcode::from_u8(chunk.code[i + 3]),
            ) {
                let idx1 = u16::from_le_bytes([chunk.code[i + 1], chunk.code[i + 2]]);
                let idx2 = u16::from_le_bytes([chunk.code[i + 4], chunk.code[i + 5]]);

                if i + 6 < chunk.code.len() {
                    if let Some(op) = Opcode::from_u8(chunk.code[i + 6]) {
                        if let (Some(v1), Some(v2)) = (
                            chunk.constants.get(idx1 as usize),
                            chunk.constants.get(idx2 as usize),
                        ) {
                            if let (Value::Number(n1), Value::Number(n2)) = (v1, v2) {
                                let result = match op {
                                    Opcode::Add => Some(Value::Number(n1 + n2)),
                                    Opcode::Sub => Some(Value::Number(n1 - n2)),
                                    Opcode::Mul => Some(Value::Number(n1 * n2)),
                                    Opcode::Div if *n2 != 0.0 => Some(Value::Number(n1 / n2)),
                                    Opcode::Mod if *n2 != 0.0 => Some(Value::Number(n1 % n2)),
                                    Opcode::Pow => Some(Value::Number(n1.powf(*n2))),
                                    Opcode::Lt => Some(Value::Boolean(n1 < n2)),
                                    Opcode::Le => Some(Value::Boolean(n1 <= n2)),
                                    Opcode::Gt => Some(Value::Boolean(n1 > n2)),
                                    Opcode::Ge => Some(Value::Boolean(n1 >= n2)),
                                    Opcode::Eq | Opcode::StrictEq => {
                                        Some(Value::Boolean((n1 - n2).abs() < f64::EPSILON))
                                    }
                                    Opcode::Ne | Opcode::StrictNe => {
                                        Some(Value::Boolean((n1 - n2).abs() >= f64::EPSILON))
                                    }
                                    _ => None,
                                };

                                if let Some(folded) = result {
                                    // Add the folded constant
                                    let new_idx = chunk.add_constant(folded);

                                    // Get the line number
                                    let line = chunk.lines[i];

                                    // Replace with: Constant new_idx, Nop*4
                                    chunk.code[i] = Opcode::Constant as u8;
                                    chunk.code[i + 1] = (new_idx & 0xFF) as u8;
                                    chunk.code[i + 2] = (new_idx >> 8) as u8;
                                    // Fill remaining with Nop
                                    for j in (i + 3)..=(i + 6) {
                                        chunk.code[j] = Opcode::Nop as u8;
                                        chunk.lines[j] = line;
                                    }

                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            i += 1;
        }

        changed
    }

    /// Peephole optimizations: simplify common patterns
    fn peephole_optimize(&self, chunk: &mut Chunk) -> bool {
        let mut changed = false;
        let mut i = 0;

        while i < chunk.code.len() {
            // Pattern: Push then immediate Pop → remove both (now Constant, Pop)
            if i + 3 < chunk.code.len() {
                if let (Some(Opcode::Constant), Some(Opcode::Pop)) = (
                    Opcode::from_u8(chunk.code[i]),
                    Opcode::from_u8(chunk.code[i + 3]),
                ) {
                    // Only remove if not the last value
                    let line = chunk.lines[i];
                    chunk.code[i] = Opcode::Nop as u8;
                    chunk.code[i + 1] = Opcode::Nop as u8;
                    chunk.code[i + 2] = Opcode::Nop as u8;
                    chunk.code[i + 3] = Opcode::Nop as u8;
                    chunk.lines[i] = line;
                    chunk.lines[i + 1] = line;
                    chunk.lines[i + 2] = line;
                    chunk.lines[i + 3] = line;
                    changed = true;
                    i += 4;
                    continue;
                }
            }

            // Pattern: Not Not → remove both
            if i + 1 < chunk.code.len() {
                if let (Some(Opcode::Not), Some(Opcode::Not)) = (
                    Opcode::from_u8(chunk.code[i]),
                    Opcode::from_u8(chunk.code[i + 1]),
                ) {
                    let line = chunk.lines[i];
                    chunk.code[i] = Opcode::Nop as u8;
                    chunk.code[i + 1] = Opcode::Nop as u8;
                    chunk.lines[i] = line;
                    chunk.lines[i + 1] = line;
                    changed = true;
                    i += 2;
                    continue;
                }
            }

            // Pattern: Neg Neg → remove both
            if i + 1 < chunk.code.len() {
                if let (Some(Opcode::Neg), Some(Opcode::Neg)) = (
                    Opcode::from_u8(chunk.code[i]),
                    Opcode::from_u8(chunk.code[i + 1]),
                ) {
                    let line = chunk.lines[i];
                    chunk.code[i] = Opcode::Nop as u8;
                    chunk.code[i + 1] = Opcode::Nop as u8;
                    chunk.lines[i] = line;
                    chunk.lines[i + 1] = line;
                    changed = true;
                    i += 2;
                    continue;
                }
            }

            // Pattern: Dup followed by Pop → remove Dup
            if i + 1 < chunk.code.len() {
                if let (Some(Opcode::Dup), Some(Opcode::Pop)) = (
                    Opcode::from_u8(chunk.code[i]),
                    Opcode::from_u8(chunk.code[i + 1]),
                ) {
                    let line = chunk.lines[i];
                    chunk.code[i] = Opcode::Nop as u8;
                    chunk.code[i + 1] = Opcode::Nop as u8;
                    chunk.lines[i] = line;
                    chunk.lines[i + 1] = line;
                    changed = true;
                    i += 2;
                    continue;
                }
            }

            // Pattern: Jump to next instruction → remove jump
            if i + 2 < chunk.code.len() {
                if let Some(Opcode::Jump) = Opcode::from_u8(chunk.code[i]) {
                    let offset =
                        i16::from_le_bytes([chunk.code[i + 1], chunk.code[i + 2]]) as isize;
                    if offset == 0 {
                        // Jump to next instruction
                        let line = chunk.lines[i];
                        chunk.code[i] = Opcode::Nop as u8;
                        chunk.code[i + 1] = Opcode::Nop as u8;
                        chunk.code[i + 2] = Opcode::Nop as u8;
                        chunk.lines[i] = line;
                        chunk.lines[i + 1] = line;
                        chunk.lines[i + 2] = line;
                        changed = true;
                        i += 3;
                        continue;
                    }
                }
            }

            i += 1;
        }

        // Return whether any changes were made
        // Note: compact_nops is a placeholder for future optimization
        changed || self.compact_nops(chunk)
    }

    /// Dead code elimination: remove unreachable code
    fn dead_code_elimination(&self, chunk: &mut Chunk) -> bool {
        let mut changed = false;
        let mut i = 0;

        while i < chunk.code.len() {
            // Find unconditional jumps or returns
            if let Some(op) = Opcode::from_u8(chunk.code[i]) {
                let (is_terminal, skip) = match op {
                    Opcode::Return | Opcode::ReturnUndefined | Opcode::Throw => (true, 1),
                    Opcode::Jump => (true, 3), // Jump + 2-byte offset
                    _ => (false, 0),
                };

                if is_terminal {
                    // Check if there's code after this that isn't a jump target
                    let next_pos = i + skip;
                    if next_pos < chunk.code.len() {
                        // For now, simple check: if not at function boundary and next isn't a Nop
                        if let Some(next_op) = Opcode::from_u8(chunk.code[next_pos]) {
                            if next_op != Opcode::Nop {
                                // Check if this is a jump target (simplified - check for nearby jumps)
                                if !self.is_jump_target(chunk, next_pos) {
                                    // Mark as Nop
                                    let line = chunk.lines[next_pos];
                                    let nop_count = self.get_instruction_size(next_op);
                                    for j in 0..nop_count {
                                        if next_pos + j < chunk.code.len() {
                                            chunk.code[next_pos + j] = Opcode::Nop as u8;
                                            chunk.lines[next_pos + j] = line;
                                            changed = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            i += 1;
        }

        changed
    }

    /// Jump threading: optimize chains of jumps
    fn jump_threading(&self, chunk: &mut Chunk) -> bool {
        let mut changed = false;
        let mut i = 0;

        while i + 2 < chunk.code.len() {
            if let Some(Opcode::Jump) = Opcode::from_u8(chunk.code[i]) {
                let offset = i16::from_le_bytes([chunk.code[i + 1], chunk.code[i + 2]]);
                let target = (i as isize + 3 + offset as isize) as usize;

                // Check if target is another jump
                if target + 2 < chunk.code.len() {
                    if let Some(Opcode::Jump) = Opcode::from_u8(chunk.code[target]) {
                        let next_offset =
                            i16::from_le_bytes([chunk.code[target + 1], chunk.code[target + 2]]);
                        let final_target = target as isize + 3 + next_offset as isize;
                        let new_offset = final_target - (i as isize + 3);

                        if new_offset >= i16::MIN as isize && new_offset <= i16::MAX as isize {
                            let new_offset = new_offset as i16;
                            chunk.code[i + 1] = (new_offset & 0xFF) as u8;
                            chunk.code[i + 2] = (new_offset >> 8) as u8;
                            changed = true;
                        }
                    }
                }
            }
            i += 1;
        }

        changed
    }

    /// Check if a position is a jump target
    fn is_jump_target(&self, chunk: &Chunk, pos: usize) -> bool {
        let mut i = 0;
        while i < chunk.code.len() {
            if let Some(op) = Opcode::from_u8(chunk.code[i]) {
                match op {
                    Opcode::Jump
                    | Opcode::JumpIfFalse
                    | Opcode::JumpIfTrue
                    | Opcode::JumpIfNull
                    | Opcode::JumpIfNotNull => {
                        if i + 2 < chunk.code.len() {
                            let offset =
                                i16::from_le_bytes([chunk.code[i + 1], chunk.code[i + 2]]);
                            let target = (i as isize + 3 + offset as isize) as usize;
                            if target == pos {
                                return true;
                            }
                        }
                        i += 3;
                    }
                    Opcode::EnterTry => {
                        if i + 2 < chunk.code.len() {
                            let offset =
                                i16::from_le_bytes([chunk.code[i + 1], chunk.code[i + 2]]);
                            let target = (i as isize + 3 + offset as isize) as usize;
                            if target == pos {
                                return true;
                            }
                        }
                        i += 3;
                    }
                    _ => {
                        i += self.get_instruction_size(op);
                    }
                }
            } else {
                i += 1;
            }
        }
        false
    }

    /// Get the size of an instruction in bytes
    fn get_instruction_size(&self, op: Opcode) -> usize {
        match op {
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
            | Opcode::GetElement
            | Opcode::SetElement
            | Opcode::Spread
            | Opcode::RestParam
            | Opcode::LeaveTry
            | Opcode::LeaveWith
            | Opcode::SetSuperClass
            | Opcode::DynamicImport => 1,

            // 1-byte operand
            Opcode::LoadReg
            | Opcode::StoreReg
            | Opcode::Call
            | Opcode::TailCall
            | Opcode::New
            | Opcode::CreateArray
            | Opcode::CreateObject
            | Opcode::GetLocal
            | Opcode::SetLocal
            | Opcode::SuperCall => 2,

            // 2-byte operand
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
            | Opcode::Jump
            | Opcode::JumpIfFalse
            | Opcode::JumpIfTrue
            | Opcode::JumpIfNull
            | Opcode::JumpIfNotNull
            | Opcode::EnterTry
            | Opcode::EnterWith
            | Opcode::GetUpvalue
            | Opcode::SetUpvalue
            | Opcode::CloseUpvalue
            | Opcode::SuperGet => 3,

            // 3-byte operand
            Opcode::CallMethod => 4,

            // 5-byte operand (effect_type_index u16 + operation_index u16 + arg_count u8)
            Opcode::Perform => 6,
        }
    }

    /// Compact consecutive Nops
    fn compact_nops(&self, _chunk: &mut Chunk) -> bool {
        // For now, we don't actually remove NOPs as that would require
        // rewriting all jump offsets. Instead, we just leave them as is.
        // A more advanced optimizer would compact the bytecode.
        false
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to optimize a chunk with default settings
#[allow(dead_code)]
pub fn optimize(chunk: &mut Chunk) {
    let optimizer = Optimizer::new();
    optimizer.optimize(chunk);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_folding() {
        let mut chunk = Chunk::new();

        // Emit: 1 + 2
        let idx1 = chunk.add_constant(Value::Number(1.0));
        let idx2 = chunk.add_constant(Value::Number(2.0));

        chunk.write_opcode(Opcode::Constant, 1);
        chunk.write((idx1 & 0xFF) as u8, 1);
        chunk.write((idx1 >> 8) as u8, 1);

        chunk.write_opcode(Opcode::Constant, 1);
        chunk.write((idx2 & 0xFF) as u8, 1);
        chunk.write((idx2 >> 8) as u8, 1);

        chunk.write_opcode(Opcode::Add, 1);
        chunk.write_opcode(Opcode::Return, 1);

        let optimizer = Optimizer::new();
        optimizer.optimize(&mut chunk);

        // After optimization, there should be a constant 3.0
        let has_three = chunk.constants.iter().any(|v| {
            if let Value::Number(n) = v {
                (*n - 3.0).abs() < f64::EPSILON
            } else {
                false
            }
        });

        assert!(has_three, "Constant folding should produce 3.0");
    }

    #[test]
    fn test_double_negation_removal() {
        let mut chunk = Chunk::new();

        chunk.write_opcode(Opcode::True, 1);
        chunk.write_opcode(Opcode::Not, 1);
        chunk.write_opcode(Opcode::Not, 1);
        chunk.write_opcode(Opcode::Return, 1);

        let optimizer = Optimizer::new();
        optimizer.optimize(&mut chunk);

        // After optimization, the two Nots should be NOPs
        let not_count = chunk
            .code
            .iter()
            .filter(|&&b| Opcode::from_u8(b) == Some(Opcode::Not))
            .count();

        assert_eq!(not_count, 0, "Double negation should be removed");
    }

    #[test]
    fn test_optimizer_config() {
        let config = OptimizerConfig {
            constant_folding: false,
            dead_code_elimination: true,
            peephole: true,
            jump_threading: true,
        };

        let optimizer = Optimizer::with_config(config);
        assert!(!optimizer.config.constant_folding);
        assert!(optimizer.config.dead_code_elimination);
    }
}
