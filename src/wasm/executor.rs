//! WebAssembly instruction execution engine
//!
//! Executes WASM bytecode from parsed modules, providing memory management,
//! table operations, and JS-WASM interop.

use crate::error::{Error, Result};
use crate::runtime::Value;
use std::collections::HashMap;

/// WASM value types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl WasmValue {
    pub fn as_i32(&self) -> Result<i32> {
        match self {
            WasmValue::I32(v) => Ok(*v),
            _ => Err(Error::type_error("Expected i32")),
        }
    }

    pub fn as_i64(&self) -> Result<i64> {
        match self {
            WasmValue::I64(v) => Ok(*v),
            _ => Err(Error::type_error("Expected i64")),
        }
    }

    pub fn as_f32(&self) -> Result<f32> {
        match self {
            WasmValue::F32(v) => Ok(*v),
            _ => Err(Error::type_error("Expected f32")),
        }
    }

    pub fn as_f64(&self) -> Result<f64> {
        match self {
            WasmValue::F64(v) => Ok(*v),
            _ => Err(Error::type_error("Expected f64")),
        }
    }

    /// Convert to JavaScript Value
    pub fn to_js_value(&self) -> Value {
        match self {
            WasmValue::I32(v) => Value::Number(*v as f64),
            WasmValue::I64(v) => Value::Number(*v as f64),
            WasmValue::F32(v) => Value::Number(*v as f64),
            WasmValue::F64(v) => Value::Number(*v),
        }
    }

    /// Create from JavaScript Value
    pub fn from_js_value(value: &Value, ty: WasmType) -> Result<Self> {
        let num = value.to_number();
        match ty {
            WasmType::I32 => Ok(WasmValue::I32(num as i32)),
            WasmType::I64 => Ok(WasmValue::I64(num as i64)),
            WasmType::F32 => Ok(WasmValue::F32(num as f32)),
            WasmType::F64 => Ok(WasmValue::F64(num)),
        }
    }
}

/// WASM type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
}

/// Linear memory for WASM execution
pub struct WasmMemory {
    /// Memory data in 64KB pages
    data: Vec<u8>,
    /// Current size in pages
    current_pages: u32,
    /// Maximum size in pages (None = unlimited)
    max_pages: Option<u32>,
}

const PAGE_SIZE: usize = 65536; // 64KB

impl WasmMemory {
    pub fn new(initial_pages: u32, max_pages: Option<u32>) -> Self {
        let size = initial_pages as usize * PAGE_SIZE;
        Self {
            data: vec![0u8; size],
            current_pages: initial_pages,
            max_pages,
        }
    }

    /// Grow memory by the given number of pages. Returns previous size or -1 on failure.
    pub fn grow(&mut self, delta: u32) -> i32 {
        let new_pages = self.current_pages + delta;
        if let Some(max) = self.max_pages {
            if new_pages > max {
                return -1;
            }
        }
        let old_pages = self.current_pages;
        self.data.resize(new_pages as usize * PAGE_SIZE, 0);
        self.current_pages = new_pages;
        old_pages as i32
    }

    /// Get current size in pages
    pub fn size(&self) -> u32 {
        self.current_pages
    }

    /// Load a byte from memory
    pub fn load_u8(&self, addr: u32) -> Result<u8> {
        let addr = addr as usize;
        if addr >= self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        Ok(self.data[addr])
    }

    /// Load a 32-bit integer from memory (little-endian)
    pub fn load_i32(&self, addr: u32) -> Result<i32> {
        let addr = addr as usize;
        if addr + 4 > self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        Ok(i32::from_le_bytes([
            self.data[addr],
            self.data[addr + 1],
            self.data[addr + 2],
            self.data[addr + 3],
        ]))
    }

    /// Load a 64-bit integer from memory (little-endian)
    pub fn load_i64(&self, addr: u32) -> Result<i64> {
        let addr = addr as usize;
        if addr + 8 > self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data[addr..addr + 8]);
        Ok(i64::from_le_bytes(bytes))
    }

    /// Load a 32-bit float from memory
    pub fn load_f32(&self, addr: u32) -> Result<f32> {
        let bits = self.load_i32(addr)?;
        Ok(f32::from_bits(bits as u32))
    }

    /// Load a 64-bit float from memory
    pub fn load_f64(&self, addr: u32) -> Result<f64> {
        let bits = self.load_i64(addr)?;
        Ok(f64::from_bits(bits as u64))
    }

    /// Store a byte to memory
    pub fn store_u8(&mut self, addr: u32, val: u8) -> Result<()> {
        let addr = addr as usize;
        if addr >= self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        self.data[addr] = val;
        Ok(())
    }

    /// Store a 32-bit integer to memory (little-endian)
    pub fn store_i32(&mut self, addr: u32, val: i32) -> Result<()> {
        let addr = addr as usize;
        if addr + 4 > self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        let bytes = val.to_le_bytes();
        self.data[addr..addr + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Store a 64-bit integer to memory (little-endian)
    pub fn store_i64(&mut self, addr: u32, val: i64) -> Result<()> {
        let addr = addr as usize;
        if addr + 8 > self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        let bytes = val.to_le_bytes();
        self.data[addr..addr + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// Store a 32-bit float to memory
    pub fn store_f32(&mut self, addr: u32, val: f32) -> Result<()> {
        self.store_i32(addr, val.to_bits() as i32)
    }

    /// Store a 64-bit float to memory
    pub fn store_f64(&mut self, addr: u32, val: f64) -> Result<()> {
        self.store_i64(addr, val.to_bits() as i64)
    }

    /// Get a slice of memory as bytes
    pub fn slice(&self, offset: u32, length: u32) -> Result<&[u8]> {
        let start = offset as usize;
        let end = start + length as usize;
        if end > self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        Ok(&self.data[start..end])
    }

    /// Write bytes to memory
    pub fn write(&mut self, offset: u32, data: &[u8]) -> Result<()> {
        let start = offset as usize;
        let end = start + data.len();
        if end > self.data.len() {
            return Err(Error::type_error("Memory access out of bounds"));
        }
        self.data[start..end].copy_from_slice(data);
        Ok(())
    }
}

/// WASM table for indirect function calls
pub struct WasmTable {
    elements: Vec<Option<u32>>,
    max_size: Option<u32>,
}

impl WasmTable {
    pub fn new(initial_size: u32, max_size: Option<u32>) -> Self {
        Self {
            elements: vec![None; initial_size as usize],
            max_size,
        }
    }

    pub fn get(&self, index: u32) -> Result<Option<u32>> {
        let idx = index as usize;
        if idx >= self.elements.len() {
            return Err(Error::type_error("Table access out of bounds"));
        }
        Ok(self.elements[idx])
    }

    pub fn set(&mut self, index: u32, func_idx: u32) -> Result<()> {
        let idx = index as usize;
        if idx >= self.elements.len() {
            return Err(Error::type_error("Table access out of bounds"));
        }
        self.elements[idx] = Some(func_idx);
        Ok(())
    }

    pub fn grow(&mut self, delta: u32) -> i32 {
        let new_size = self.elements.len() + delta as usize;
        if let Some(max) = self.max_size {
            if new_size > max as usize {
                return -1;
            }
        }
        let old_size = self.elements.len();
        self.elements.resize(new_size, None);
        old_size as i32
    }

    pub fn size(&self) -> u32 {
        self.elements.len() as u32
    }
}

/// A WASM function (either internal or imported)
#[derive(Clone)]
pub enum WasmFunction {
    /// Internal function with bytecode
    Internal {
        type_idx: u32,
        locals: Vec<WasmType>,
        body: Vec<WasmInstruction>,
    },
    /// Imported host function
    Host {
        type_idx: u32,
        handler: std::rc::Rc<dyn Fn(&[WasmValue]) -> Result<Vec<WasmValue>>>,
    },
}

/// WASM instruction set (MVP subset)
#[derive(Debug, Clone)]
pub enum WasmInstruction {
    // Control flow
    Unreachable,
    Nop,
    Block(i64),
    Loop(i64),
    If(i64),
    Else,
    End,
    Br(u32),
    BrIf(u32),
    Return,
    Call(u32),
    CallIndirect(u32, u32),

    // Parametric
    Drop,
    Select,

    // Variable access
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),

    // Memory
    I32Load(u32, u32),
    I64Load(u32, u32),
    F32Load(u32, u32),
    F64Load(u32, u32),
    I32Store(u32, u32),
    I64Store(u32, u32),
    F32Store(u32, u32),
    F64Store(u32, u32),
    I32Load8S(u32, u32),
    I32Load8U(u32, u32),
    I32Load16S(u32, u32),
    I32Load16U(u32, u32),
    MemorySize,
    MemoryGrow,

    // Constants
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),

    // i32 operations
    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,

    // i64 operations
    I64Eqz,
    I64Eq,
    I64Ne,
    I64LtS,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,

    // f32 operations
    F32Abs,
    F32Neg,
    F32Ceil,
    F32Floor,
    F32Sqrt,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Min,
    F32Max,

    // f64 operations
    F64Abs,
    F64Neg,
    F64Ceil,
    F64Floor,
    F64Sqrt,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Min,
    F64Max,

    // Conversions
    I32WrapI64,
    I32TruncF32S,
    I32TruncF64S,
    I64ExtendI32S,
    I64ExtendI32U,
    I64TruncF64S,
    F32ConvertI32S,
    F32ConvertI64S,
    F32DemoteF64,
    F64ConvertI32S,
    F64ConvertI64S,
    F64PromoteF32,
    I32ReinterpretF32,
    F32ReinterpretI32,
}

/// Stack frame for WASM execution
struct WasmFrame {
    /// Function index
    _func_idx: u32,
    /// Instruction pointer
    ip: usize,
    /// Local variables
    locals: Vec<WasmValue>,
    /// Label stack for block/loop control flow
    _label_stack: Vec<LabelInfo>,
}

#[derive(Debug, Clone)]
struct LabelInfo {
    /// Target IP for branching
    _target_ip: usize,
    /// Whether this is a loop (branch goes back to start)
    _is_loop: bool,
    /// Stack depth at block entry
    _stack_depth: usize,
}

/// The WASM execution engine
pub struct WasmExecutor {
    /// Functions (internal + imported)
    functions: Vec<WasmFunction>,
    /// Linear memory
    pub memory: WasmMemory,
    /// Function table
    pub table: WasmTable,
    /// Global variables
    globals: Vec<WasmValue>,
    /// Value stack
    stack: Vec<WasmValue>,
    /// Call frames
    frames: Vec<WasmFrame>,
    /// Exports map (name -> function index)
    exports: HashMap<String, u32>,
    /// Maximum stack depth
    max_stack: usize,
    /// Maximum call depth
    max_call_depth: usize,
}

impl WasmExecutor {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            memory: WasmMemory::new(1, Some(256)),
            table: WasmTable::new(0, None),
            globals: Vec::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            exports: HashMap::new(),
            max_stack: 65536,
            max_call_depth: 1000,
        }
    }

    /// Add a function to the executor
    pub fn add_function(&mut self, func: WasmFunction) -> u32 {
        let idx = self.functions.len() as u32;
        self.functions.push(func);
        idx
    }

    /// Export a function by name
    pub fn export_function(&mut self, name: &str, func_idx: u32) {
        self.exports.insert(name.to_string(), func_idx);
    }

    /// Add a global variable
    pub fn add_global(&mut self, value: WasmValue) -> u32 {
        let idx = self.globals.len() as u32;
        self.globals.push(value);
        idx
    }

    /// Call an exported function
    pub fn call_export(&mut self, name: &str, args: &[WasmValue]) -> Result<Vec<WasmValue>> {
        let func_idx = self
            .exports
            .get(name)
            .copied()
            .ok_or_else(|| Error::type_error(format!("Export '{}' not found", name)))?;
        self.call_function(func_idx, args)
    }

    /// Call a function by index
    pub fn call_function(&mut self, func_idx: u32, args: &[WasmValue]) -> Result<Vec<WasmValue>> {
        let func = self
            .functions
            .get(func_idx as usize)
            .ok_or_else(|| Error::type_error("Invalid function index"))?
            .clone();

        match func {
            WasmFunction::Host { handler, .. } => handler(args),
            WasmFunction::Internal { locals, body, .. } => {
                // Set up locals: args + declared locals
                let mut all_locals = args.to_vec();
                for ty in &locals {
                    all_locals.push(match ty {
                        WasmType::I32 => WasmValue::I32(0),
                        WasmType::I64 => WasmValue::I64(0),
                        WasmType::F32 => WasmValue::F32(0.0),
                        WasmType::F64 => WasmValue::F64(0.0),
                    });
                }

                let frame = WasmFrame {
                    _func_idx: func_idx,
                    ip: 0,
                    locals: all_locals,
                    _label_stack: Vec::new(),
                };

                if self.frames.len() >= self.max_call_depth {
                    return Err(Error::type_error("WASM call stack overflow"));
                }

                self.frames.push(frame);
                let result = self.execute(&body)?;
                self.frames.pop();
                Ok(result)
            }
        }
    }

    /// Execute WASM instructions
    fn execute(&mut self, instructions: &[WasmInstruction]) -> Result<Vec<WasmValue>> {
        let frame_idx = self.frames.len() - 1;

        while self.frames[frame_idx].ip < instructions.len() {
            let ip = self.frames[frame_idx].ip;
            let instr = &instructions[ip];
            self.frames[frame_idx].ip += 1;

            match instr {
                WasmInstruction::Nop => {}
                WasmInstruction::Unreachable => {
                    return Err(Error::type_error("WASM unreachable executed"));
                }

                // Constants
                WasmInstruction::I32Const(v) => self.push(WasmValue::I32(*v))?,
                WasmInstruction::I64Const(v) => self.push(WasmValue::I64(*v))?,
                WasmInstruction::F32Const(v) => self.push(WasmValue::F32(*v))?,
                WasmInstruction::F64Const(v) => self.push(WasmValue::F64(*v))?,

                // Variable access
                WasmInstruction::LocalGet(idx) => {
                    let val = self.frames[frame_idx].locals[*idx as usize];
                    self.push(val)?;
                }
                WasmInstruction::LocalSet(idx) => {
                    let val = self.pop()?;
                    self.frames[frame_idx].locals[*idx as usize] = val;
                }
                WasmInstruction::LocalTee(idx) => {
                    let val = *self.stack.last().ok_or_else(|| Error::type_error("Stack underflow"))?;
                    self.frames[frame_idx].locals[*idx as usize] = val;
                }
                WasmInstruction::GlobalGet(idx) => {
                    let val = self.globals[*idx as usize];
                    self.push(val)?;
                }
                WasmInstruction::GlobalSet(idx) => {
                    let val = self.pop()?;
                    self.globals[*idx as usize] = val;
                }

                // i32 arithmetic
                WasmInstruction::I32Add => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a.wrapping_add(b)))?;
                }
                WasmInstruction::I32Sub => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a.wrapping_sub(b)))?;
                }
                WasmInstruction::I32Mul => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a.wrapping_mul(b)))?;
                }
                WasmInstruction::I32DivS => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    if b == 0 {
                        return Err(Error::type_error("Integer division by zero"));
                    }
                    self.push(WasmValue::I32(a.wrapping_div(b)))?;
                }
                WasmInstruction::I32RemS => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    if b == 0 {
                        return Err(Error::type_error("Integer remainder by zero"));
                    }
                    self.push(WasmValue::I32(a.wrapping_rem(b)))?;
                }
                WasmInstruction::I32And => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a & b))?;
                }
                WasmInstruction::I32Or => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a | b))?;
                }
                WasmInstruction::I32Xor => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a ^ b))?;
                }
                WasmInstruction::I32Shl => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a.wrapping_shl(b as u32)))?;
                }
                WasmInstruction::I32ShrS => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(a.wrapping_shr(b as u32)))?;
                }

                // i32 comparisons
                WasmInstruction::I32Eqz => {
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(if a == 0 { 1 } else { 0 }))?;
                }
                WasmInstruction::I32Eq => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(if a == b { 1 } else { 0 }))?;
                }
                WasmInstruction::I32Ne => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(if a != b { 1 } else { 0 }))?;
                }
                WasmInstruction::I32LtS => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(if a < b { 1 } else { 0 }))?;
                }
                WasmInstruction::I32GtS => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(if a > b { 1 } else { 0 }))?;
                }
                WasmInstruction::I32LeS => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(if a <= b { 1 } else { 0 }))?;
                }
                WasmInstruction::I32GeS => {
                    let b = self.pop()?.as_i32()?;
                    let a = self.pop()?.as_i32()?;
                    self.push(WasmValue::I32(if a >= b { 1 } else { 0 }))?;
                }

                // f64 operations
                WasmInstruction::F64Add => {
                    let b = self.pop()?.as_f64()?;
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a + b))?;
                }
                WasmInstruction::F64Sub => {
                    let b = self.pop()?.as_f64()?;
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a - b))?;
                }
                WasmInstruction::F64Mul => {
                    let b = self.pop()?.as_f64()?;
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a * b))?;
                }
                WasmInstruction::F64Div => {
                    let b = self.pop()?.as_f64()?;
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a / b))?;
                }
                WasmInstruction::F64Neg => {
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(-a))?;
                }
                WasmInstruction::F64Abs => {
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a.abs()))?;
                }
                WasmInstruction::F64Sqrt => {
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a.sqrt()))?;
                }
                WasmInstruction::F64Floor => {
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a.floor()))?;
                }
                WasmInstruction::F64Ceil => {
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a.ceil()))?;
                }
                WasmInstruction::F64Min => {
                    let b = self.pop()?.as_f64()?;
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a.min(b)))?;
                }
                WasmInstruction::F64Max => {
                    let b = self.pop()?.as_f64()?;
                    let a = self.pop()?.as_f64()?;
                    self.push(WasmValue::F64(a.max(b)))?;
                }

                // Memory operations
                WasmInstruction::I32Load(_align, offset) => {
                    let addr = self.pop()?.as_i32()? as u32 + offset;
                    let val = self.memory.load_i32(addr)?;
                    self.push(WasmValue::I32(val))?;
                }
                WasmInstruction::I64Load(_align, offset) => {
                    let addr = self.pop()?.as_i32()? as u32 + offset;
                    let val = self.memory.load_i64(addr)?;
                    self.push(WasmValue::I64(val))?;
                }
                WasmInstruction::F64Load(_align, offset) => {
                    let addr = self.pop()?.as_i32()? as u32 + offset;
                    let val = self.memory.load_f64(addr)?;
                    self.push(WasmValue::F64(val))?;
                }
                WasmInstruction::I32Store(_align, offset) => {
                    let val = self.pop()?.as_i32()?;
                    let addr = self.pop()?.as_i32()? as u32 + offset;
                    self.memory.store_i32(addr, val)?;
                }
                WasmInstruction::I64Store(_align, offset) => {
                    let val = self.pop()?.as_i64()?;
                    let addr = self.pop()?.as_i32()? as u32 + offset;
                    self.memory.store_i64(addr, val)?;
                }
                WasmInstruction::MemorySize => {
                    self.push(WasmValue::I32(self.memory.size() as i32))?;
                }
                WasmInstruction::MemoryGrow => {
                    let delta = self.pop()?.as_i32()? as u32;
                    let result = self.memory.grow(delta);
                    self.push(WasmValue::I32(result))?;
                }

                // Conversions
                WasmInstruction::I32WrapI64 => {
                    let v = self.pop()?.as_i64()?;
                    self.push(WasmValue::I32(v as i32))?;
                }
                WasmInstruction::I64ExtendI32S => {
                    let v = self.pop()?.as_i32()?;
                    self.push(WasmValue::I64(v as i64))?;
                }
                WasmInstruction::F64ConvertI32S => {
                    let v = self.pop()?.as_i32()?;
                    self.push(WasmValue::F64(v as f64))?;
                }
                WasmInstruction::I32TruncF64S => {
                    let v = self.pop()?.as_f64()?;
                    self.push(WasmValue::I32(v as i32))?;
                }

                // Parametric
                WasmInstruction::Drop => {
                    self.pop()?;
                }
                WasmInstruction::Select => {
                    let cond = self.pop()?.as_i32()?;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(if cond != 0 { a } else { b })?;
                }

                // Function calls
                WasmInstruction::Call(func_idx) => {
                    let result = self.call_function(*func_idx, &[])?;
                    for val in result {
                        self.push(val)?;
                    }
                }

                WasmInstruction::Br(offset) => {
                    // Jump forward by offset instructions (simple branch)
                    let new_ip = self.frames[frame_idx].ip + *offset as usize;
                    if new_ip <= instructions.len() {
                        self.frames[frame_idx].ip = new_ip;
                    } else {
                        break;
                    }
                }
                WasmInstruction::BrIf(offset) => {
                    let cond = self.pop()?.as_i32()?;
                    if cond != 0 {
                        let new_ip = self.frames[frame_idx].ip + *offset as usize;
                        if new_ip <= instructions.len() {
                            self.frames[frame_idx].ip = new_ip;
                        } else {
                            break;
                        }
                    }
                }

                WasmInstruction::Return => {
                    break;
                }

                // For now, skip unimplemented instructions
                _ => {}
            }
        }

        // Return top of stack as result
        if self.stack.is_empty() {
            Ok(vec![])
        } else {
            Ok(vec![self.pop()?])
        }
    }

    fn push(&mut self, value: WasmValue) -> Result<()> {
        if self.stack.len() >= self.max_stack {
            return Err(Error::type_error("WASM stack overflow"));
        }
        self.stack.push(value);
        Ok(())
    }

    fn pop(&mut self) -> Result<WasmValue> {
        self.stack
            .pop()
            .ok_or_else(|| Error::type_error("WASM stack underflow"))
    }

    /// Get the exports as a JavaScript object
    pub fn exports_as_js_object(&self) -> Value {
        let mut props = rustc_hash::FxHashMap::default();
        for name in self.exports.keys() {
            props.insert(name.clone(), Value::String(format!("[WASM Function: {}]", name)));
        }
        Value::new_object_with_properties(props)
    }
}

impl Default for WasmExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// WASI preview1 error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasiErrno {
    Success = 0,
    Badf = 8,
    Inval = 28,
    Nosys = 52,
}

/// WASI preview1 syscall environment
pub struct WasiEnv {
    args: Vec<String>,
    env_vars: Vec<(String, String)>,
    stdout_buf: Vec<u8>,
    stderr_buf: Vec<u8>,
    exit_code: Option<i32>,
}

impl WasiEnv {
    pub fn new() -> Self {
        Self {
            args: Vec::new(),
            env_vars: Vec::new(),
            stdout_buf: Vec::new(),
            stderr_buf: Vec::new(),
            exit_code: None,
        }
    }

    pub fn set_args(&mut self, args: Vec<String>) {
        self.args = args;
    }

    pub fn set_env(&mut self, vars: Vec<(String, String)>) {
        self.env_vars = vars;
    }

    pub fn stdout_output(&self) -> &[u8] {
        &self.stdout_buf
    }

    pub fn stderr_output(&self) -> &[u8] {
        &self.stderr_buf
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// fd_write: write to a file descriptor (stdout=1, stderr=2)
    pub fn fd_write(&mut self, memory: &WasmMemory, fd: i32, iovs_ptr: u32, iovs_len: u32) -> Result<(WasiErrno, u32)> {
        if fd != 1 && fd != 2 {
            return Ok((WasiErrno::Badf, 0));
        }
        let mut total_written = 0u32;
        for i in 0..iovs_len {
            let iov_offset = iovs_ptr + i * 8;
            let buf_ptr = memory.load_i32(iov_offset)? as u32;
            let buf_len = memory.load_i32(iov_offset + 4)? as u32;
            let data = memory.slice(buf_ptr, buf_len)?;
            let buf = if fd == 1 { &mut self.stdout_buf } else { &mut self.stderr_buf };
            buf.extend_from_slice(data);
            total_written += buf_len;
        }
        Ok((WasiErrno::Success, total_written))
    }

    /// args_sizes_get: return (count, total_buf_size)
    pub fn args_sizes_get(&self) -> (u32, u32) {
        let count = self.args.len() as u32;
        let size: u32 = self.args.iter().map(|a| a.len() as u32 + 1).sum();
        (count, size)
    }

    /// environ_sizes_get: return (count, total_buf_size)
    pub fn environ_sizes_get(&self) -> (u32, u32) {
        let count = self.env_vars.len() as u32;
        let size: u32 = self.env_vars.iter().map(|(k, v)| k.len() as u32 + v.len() as u32 + 2).sum();
        (count, size)
    }

    /// clock_time_get: return current time in nanoseconds
    pub fn clock_time_get(&self, clock_id: u32) -> Result<u64> {
        match clock_id {
            0 => {
                // CLOCK_REALTIME
                Ok(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64)
            }
            1 => {
                // CLOCK_MONOTONIC
                Ok(std::time::Instant::now().elapsed().as_nanos() as u64)
            }
            _ => Err(Error::type_error(format!("Unknown clock_id: {}", clock_id))),
        }
    }

    /// proc_exit: set exit code
    pub fn proc_exit(&mut self, code: i32) {
        self.exit_code = Some(code);
    }
}

impl Default for WasiEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// JS-WASM bridge for linking JS functions as WASM imports
pub struct JsWasmBridge {
    /// Imported functions from JS (module::name -> handler)
    import_handlers: HashMap<(String, String), std::rc::Rc<dyn Fn(&[WasmValue]) -> Result<Vec<WasmValue>>>>,
}

impl JsWasmBridge {
    pub fn new() -> Self {
        Self {
            import_handlers: HashMap::default(),
        }
    }

    /// Register a JS function as a WASM import
    pub fn register_import<F>(&mut self, module: &str, name: &str, handler: F)
    where
        F: Fn(&[WasmValue]) -> Result<Vec<WasmValue>> + 'static,
    {
        self.import_handlers.insert(
            (module.to_string(), name.to_string()),
            std::rc::Rc::new(handler),
        );
    }

    /// Resolve an import to a WasmFunction
    pub fn resolve_import(&self, module: &str, name: &str) -> Option<WasmFunction> {
        self.import_handlers
            .get(&(module.to_string(), name.to_string()))
            .map(|handler| WasmFunction::Host {
                type_idx: 0,
                handler: handler.clone(),
            })
    }

    /// Link imports into an executor, returning the function indices
    pub fn link(&self, executor: &mut WasmExecutor, imports: &[(String, String)]) -> Result<Vec<u32>> {
        let mut indices = Vec::new();
        for (module, name) in imports {
            let func = self
                .resolve_import(module, name)
                .ok_or_else(|| Error::type_error(format!("Unresolved import: {}::{}", module, name)))?;
            let idx = executor.add_function(func);
            indices.push(idx);
        }
        Ok(indices)
    }

    /// Register standard WASI preview1 fd_write as an import
    pub fn register_wasi_fd_write(&mut self, wasi_env: std::rc::Rc<std::cell::RefCell<WasiEnv>>, memory: std::rc::Rc<std::cell::RefCell<WasmMemory>>) {
        let env = wasi_env;
        let mem = memory;
        self.register_import("wasi_snapshot_preview1", "fd_write", move |args| {
            let fd = args.first().map(|v| v.as_i32()).transpose()?.unwrap_or(1);
            let iovs_ptr = args.get(1).map(|v| v.as_i32()).transpose()?.unwrap_or(0) as u32;
            let iovs_len = args.get(2).map(|v| v.as_i32()).transpose()?.unwrap_or(0) as u32;
            let mem_borrow = mem.borrow();
            let (errno, written) = env.borrow_mut().fd_write(&mem_borrow, fd, iovs_ptr, iovs_len)?;
            Ok(vec![WasmValue::I32(errno as i32), WasmValue::I32(written as i32)])
        });
    }
}

impl Default for JsWasmBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_operations() {
        let mut mem = WasmMemory::new(1, Some(10));
        mem.store_i32(0, 42).unwrap();
        assert_eq!(mem.load_i32(0).unwrap(), 42);

        mem.store_i64(8, 123456789).unwrap();
        assert_eq!(mem.load_i64(8).unwrap(), 123456789);

        mem.store_f64(16, 3.14).unwrap();
        assert!((mem.load_f64(16).unwrap() - 3.14).abs() < f64::EPSILON);
    }

    #[test]
    fn test_memory_bounds_check() {
        let mem = WasmMemory::new(1, Some(1));
        assert!(mem.load_i32(65536).is_err());
    }

    #[test]
    fn test_memory_grow() {
        let mut mem = WasmMemory::new(1, Some(3));
        assert_eq!(mem.size(), 1);
        assert_eq!(mem.grow(1), 1);
        assert_eq!(mem.size(), 2);
        assert_eq!(mem.grow(2), -1); // exceeds max
    }

    #[test]
    fn test_table_operations() {
        let mut table = WasmTable::new(10, None);
        table.set(0, 42).unwrap();
        assert_eq!(table.get(0).unwrap(), Some(42));
        assert_eq!(table.get(1).unwrap(), None);
    }

    #[test]
    fn test_simple_add() {
        let mut executor = WasmExecutor::new();
        let func = WasmFunction::Internal {
            type_idx: 0,
            locals: vec![],
            body: vec![
                WasmInstruction::LocalGet(0),
                WasmInstruction::LocalGet(1),
                WasmInstruction::I32Add,
                WasmInstruction::Return,
            ],
        };
        let idx = executor.add_function(func);
        executor.export_function("add", idx);

        let result = executor
            .call_export("add", &[WasmValue::I32(3), WasmValue::I32(4)])
            .unwrap();
        assert_eq!(result, vec![WasmValue::I32(7)]);
    }

    #[test]
    fn test_factorial_unrolled() {
        // Test factorial(5) = 120 using a host function that computes it
        // (since our flat executor doesn't have loop constructs)
        let mut executor = WasmExecutor::new();
        let host_fn = WasmFunction::Host {
            type_idx: 0,
            handler: std::rc::Rc::new(|args| {
                let n = args[0].as_i32()?;
                let mut result = 1i32;
                for i in 1..=n {
                    result = result.wrapping_mul(i);
                }
                Ok(vec![WasmValue::I32(result)])
            }),
        };
        let idx = executor.add_function(host_fn);
        executor.export_function("factorial", idx);

        let result = executor
            .call_export("factorial", &[WasmValue::I32(5)])
            .unwrap();
        assert_eq!(result, vec![WasmValue::I32(120)]);
    }

    #[test]
    fn test_br_if_instruction() {
        let mut executor = WasmExecutor::new();
        // Test: if arg0 > 10, return 1, else return 0
        let func = WasmFunction::Internal {
            type_idx: 0,
            locals: vec![],
            body: vec![
                // [0] push arg0
                WasmInstruction::LocalGet(0),
                // [1] push 10
                WasmInstruction::I32Const(10),
                // [2] compare: arg0 > 10?
                WasmInstruction::I32GtS,
                // [3] if true, jump forward 2 instructions (skip the "return 0" path)
                WasmInstruction::BrIf(2),
                // [4] false path: push 0 and return
                WasmInstruction::I32Const(0),
                // [5]
                WasmInstruction::Return,
                // [6] true path: push 1 and return
                WasmInstruction::I32Const(1),
                // [7]
                WasmInstruction::Return,
            ],
        };
        let idx = executor.add_function(func);
        executor.export_function("check", idx);

        let result = executor.call_export("check", &[WasmValue::I32(15)]).unwrap();
        assert_eq!(result, vec![WasmValue::I32(1)]);

        let result = executor.call_export("check", &[WasmValue::I32(5)]).unwrap();
        assert_eq!(result, vec![WasmValue::I32(0)]);
    }

    #[test]
    fn test_host_function() {
        let mut executor = WasmExecutor::new();
        let host_fn = WasmFunction::Host {
            type_idx: 0,
            handler: std::rc::Rc::new(|args| {
                let a = args[0].as_i32()?;
                let b = args[1].as_i32()?;
                Ok(vec![WasmValue::I32(a * b)])
            }),
        };
        let idx = executor.add_function(host_fn);
        executor.export_function("multiply", idx);

        let result = executor
            .call_export("multiply", &[WasmValue::I32(6), WasmValue::I32(7)])
            .unwrap();
        assert_eq!(result, vec![WasmValue::I32(42)]);
    }

    #[test]
    fn test_memory_load_store() {
        let mut executor = WasmExecutor::new();
        let func = WasmFunction::Internal {
            type_idx: 0,
            locals: vec![],
            body: vec![
                // Store 42 at address 0
                WasmInstruction::I32Const(0),
                WasmInstruction::I32Const(42),
                WasmInstruction::I32Store(2, 0),
                // Load from address 0
                WasmInstruction::I32Const(0),
                WasmInstruction::I32Load(2, 0),
                WasmInstruction::Return,
            ],
        };
        let idx = executor.add_function(func);
        executor.export_function("store_load", idx);

        let result = executor.call_export("store_load", &[]).unwrap();
        assert_eq!(result, vec![WasmValue::I32(42)]);
    }

    #[test]
    fn test_f64_operations() {
        let mut executor = WasmExecutor::new();
        let func = WasmFunction::Internal {
            type_idx: 0,
            locals: vec![],
            body: vec![
                WasmInstruction::F64Const(3.0),
                WasmInstruction::F64Const(4.0),
                WasmInstruction::F64Add,
                WasmInstruction::Return,
            ],
        };
        let idx = executor.add_function(func);
        executor.export_function("add_f64", idx);

        let result = executor.call_export("add_f64", &[]).unwrap();
        assert_eq!(result, vec![WasmValue::F64(7.0)]);
    }

    #[test]
    fn test_wasm_js_interop() {
        let wasm_val = WasmValue::I32(42);
        let js_val = wasm_val.to_js_value();
        assert_eq!(js_val, Value::Number(42.0));

        let back = WasmValue::from_js_value(&js_val, WasmType::I32).unwrap();
        assert_eq!(back, WasmValue::I32(42));
    }

    #[test]
    fn test_wasi_env_args() {
        let mut env = WasiEnv::new();
        env.set_args(vec!["prog".to_string(), "arg1".to_string(), "arg2".to_string()]);
        let (count, size) = env.args_sizes_get();
        assert_eq!(count, 3);
        assert_eq!(size, 5 + 5 + 5); // "prog\0" + "arg1\0" + "arg2\0"
    }

    #[test]
    fn test_wasi_env_vars() {
        let mut env = WasiEnv::new();
        env.set_env(vec![("HOME".to_string(), "/home/user".to_string())]);
        let (count, size) = env.environ_sizes_get();
        assert_eq!(count, 1);
        assert_eq!(size, 4 + 10 + 2); // "HOME" + "/home/user" + "=\0"
    }

    #[test]
    fn test_wasi_fd_write() {
        let mut env = WasiEnv::new();
        let mut memory = WasmMemory::new(1, Some(2));
        // Set up an iov at address 0: buf_ptr=16, buf_len=5
        memory.store_i32(0, 16).unwrap();
        memory.store_i32(4, 5).unwrap();
        // Write "hello" at address 16
        memory.write(16, b"hello").unwrap();
        let (errno, written) = env.fd_write(&memory, 1, 0, 1).unwrap();
        assert_eq!(errno, WasiErrno::Success);
        assert_eq!(written, 5);
        assert_eq!(env.stdout_output(), b"hello");
    }

    #[test]
    fn test_wasi_fd_write_bad_fd() {
        let mut env = WasiEnv::new();
        let memory = WasmMemory::new(1, Some(1));
        let (errno, _) = env.fd_write(&memory, 99, 0, 0).unwrap();
        assert_eq!(errno, WasiErrno::Badf);
    }

    #[test]
    fn test_wasi_proc_exit() {
        let mut env = WasiEnv::new();
        assert_eq!(env.exit_code(), None);
        env.proc_exit(42);
        assert_eq!(env.exit_code(), Some(42));
    }

    #[test]
    fn test_wasi_clock_time_get() {
        let env = WasiEnv::new();
        let time = env.clock_time_get(0).unwrap();
        assert!(time > 0);
    }

    #[test]
    fn test_js_wasm_bridge_register_and_resolve() {
        let mut bridge = JsWasmBridge::new();
        bridge.register_import("env", "add", |args| {
            let a = args[0].as_i32()?;
            let b = args[1].as_i32()?;
            Ok(vec![WasmValue::I32(a + b)])
        });
        let func = bridge.resolve_import("env", "add");
        assert!(func.is_some());
        assert!(bridge.resolve_import("env", "missing").is_none());
    }

    #[test]
    fn test_js_wasm_bridge_link() {
        let mut bridge = JsWasmBridge::new();
        bridge.register_import("env", "double", |args| {
            let a = args[0].as_i32()?;
            Ok(vec![WasmValue::I32(a * 2)])
        });
        let mut executor = WasmExecutor::new();
        let imports = vec![("env".to_string(), "double".to_string())];
        let indices = bridge.link(&mut executor, &imports).unwrap();
        assert_eq!(indices.len(), 1);

        // Call the linked function
        let result = executor.call_function(indices[0], &[WasmValue::I32(21)]).unwrap();
        assert_eq!(result, vec![WasmValue::I32(42)]);
    }

    #[test]
    fn test_js_wasm_bridge_link_missing() {
        let bridge = JsWasmBridge::new();
        let mut executor = WasmExecutor::new();
        let imports = vec![("env".to_string(), "missing".to_string())];
        assert!(bridge.link(&mut executor, &imports).is_err());
    }
}
