//! WebAssembly Integration
//!
//! Bidirectional WASM support: run WASM modules from JavaScript and
//! compile JavaScript to WASM for maximum portability.
//!
//! # Example
//! ```text
//! // Load and instantiate a WASM module
//! const wasmModule = await WebAssembly.instantiate(wasmBytes, {
//!   env: {
//!     log: (ptr, len) => console.log(readString(ptr, len))
//!   }
//! });
//!
//! // Call exported function
//! const result = wasmModule.exports.fibonacci(10);
//! ```

use rustc_hash::FxHashMap as HashMap;

/// WebAssembly value types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmValueType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}

impl WasmValueType {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x7F => Some(Self::I32),
            0x7E => Some(Self::I64),
            0x7D => Some(Self::F32),
            0x7C => Some(Self::F64),
            0x7B => Some(Self::V128),
            0x70 => Some(Self::FuncRef),
            0x6F => Some(Self::ExternRef),
            _ => None,
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Self::I32 => 0x7F,
            Self::I64 => 0x7E,
            Self::F32 => 0x7D,
            Self::F64 => 0x7C,
            Self::V128 => 0x7B,
            Self::FuncRef => 0x70,
            Self::ExternRef => 0x6F,
        }
    }
}

/// WebAssembly runtime values
#[derive(Debug, Clone)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128([u8; 16]),
    FuncRef(Option<u32>),
    ExternRef(Option<u32>),
}

impl WasmValue {
    pub fn value_type(&self) -> WasmValueType {
        match self {
            Self::I32(_) => WasmValueType::I32,
            Self::I64(_) => WasmValueType::I64,
            Self::F32(_) => WasmValueType::F32,
            Self::F64(_) => WasmValueType::F64,
            Self::V128(_) => WasmValueType::V128,
            Self::FuncRef(_) => WasmValueType::FuncRef,
            Self::ExternRef(_) => WasmValueType::ExternRef,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        if let Self::I32(v) = self { Some(*v) } else { None }
    }

    pub fn as_i64(&self) -> Option<i64> {
        if let Self::I64(v) = self { Some(*v) } else { None }
    }

    pub fn as_f32(&self) -> Option<f32> {
        if let Self::F32(v) = self { Some(*v) } else { None }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Self::F64(v) = self { Some(*v) } else { None }
    }
}

/// Function type signature
#[derive(Debug, Clone)]
pub struct FuncType {
    pub params: Vec<WasmValueType>,
    pub results: Vec<WasmValueType>,
}

impl FuncType {
    pub fn new(params: Vec<WasmValueType>, results: Vec<WasmValueType>) -> Self {
        Self { params, results }
    }
}

/// A WebAssembly module
#[derive(Debug)]
pub struct Module {
    /// Type section - function signatures
    pub types: Vec<FuncType>,
    /// Import section
    pub imports: Vec<Import>,
    /// Function section - type indices
    pub functions: Vec<u32>,
    /// Table section
    pub tables: Vec<TableType>,
    /// Memory section
    pub memories: Vec<MemoryType>,
    /// Global section
    pub globals: Vec<Global>,
    /// Export section
    pub exports: Vec<Export>,
    /// Start function index
    pub start: Option<u32>,
    /// Element section
    pub elements: Vec<Element>,
    /// Code section - function bodies
    pub code: Vec<FunctionBody>,
    /// Data section
    pub data: Vec<Data>,
    /// Custom sections
    pub custom_sections: HashMap<String, Vec<u8>>,
}

/// Import entry
#[derive(Debug, Clone)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub desc: ImportDesc,
}

/// Import descriptor
#[derive(Debug, Clone)]
pub enum ImportDesc {
    Func(u32),      // type index
    Table(TableType),
    Memory(MemoryType),
    Global(GlobalType),
}

/// Export entry
#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub desc: ExportDesc,
}

/// Export descriptor
#[derive(Debug, Clone)]
pub enum ExportDesc {
    Func(u32),
    Table(u32),
    Memory(u32),
    Global(u32),
}

/// Table type
#[derive(Debug, Clone)]
pub struct TableType {
    pub element_type: WasmValueType,
    pub limits: Limits,
}

/// Memory type
#[derive(Debug, Clone)]
pub struct MemoryType {
    pub limits: Limits,
}

/// Limits (min, optional max)
#[derive(Debug, Clone)]
pub struct Limits {
    pub min: u32,
    pub max: Option<u32>,
}

/// Global type
#[derive(Debug, Clone)]
pub struct GlobalType {
    pub value_type: WasmValueType,
    pub mutable: bool,
}

/// Global variable
#[derive(Debug, Clone)]
pub struct Global {
    pub global_type: GlobalType,
    pub init: Vec<u8>, // init expression bytecode
}

/// Element segment
#[derive(Debug, Clone)]
pub struct Element {
    pub table_index: u32,
    pub offset: Vec<u8>, // offset expression
    pub init: Vec<u32>,  // function indices
}

/// Function body
#[derive(Debug, Clone)]
pub struct FunctionBody {
    pub locals: Vec<(u32, WasmValueType)>, // (count, type) pairs
    pub code: Vec<u8>,
}

/// Data segment
#[derive(Debug, Clone)]
pub struct Data {
    pub memory_index: u32,
    pub offset: Vec<u8>, // offset expression
    pub init: Vec<u8>,
}

impl Module {
    /// Create an empty module
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            imports: Vec::new(),
            functions: Vec::new(),
            tables: Vec::new(),
            memories: Vec::new(),
            globals: Vec::new(),
            exports: Vec::new(),
            start: None,
            elements: Vec::new(),
            code: Vec::new(),
            data: Vec::new(),
            custom_sections: HashMap::default(),
        }
    }

    /// Parse a WASM binary
    pub fn parse(bytes: &[u8]) -> Result<Self, WasmError> {
        // Check magic number and version
        if bytes.len() < 8 {
            return Err(WasmError::InvalidFormat("too short".to_string()));
        }

        let magic = &bytes[0..4];
        if magic != b"\0asm" {
            return Err(WasmError::InvalidFormat("invalid magic number".to_string()));
        }

        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != 1 {
            return Err(WasmError::UnsupportedVersion(version));
        }

        let mut module = Self::new();
        let mut offset = 8;

        // Parse sections
        while offset < bytes.len() {
            let section_id = bytes[offset];
            offset += 1;

            let (section_size, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;

            let section_end = offset + section_size as usize;
            let section_bytes = &bytes[offset..section_end];

            match section_id {
                0 => {
                    // Custom section
                    let (name, name_len) = read_string(section_bytes)?;
                    module.custom_sections.insert(
                        name,
                        section_bytes[name_len..].to_vec(),
                    );
                }
                1 => module.parse_type_section(section_bytes)?,
                2 => module.parse_import_section(section_bytes)?,
                3 => module.parse_function_section(section_bytes)?,
                4 => module.parse_table_section(section_bytes)?,
                5 => module.parse_memory_section(section_bytes)?,
                6 => module.parse_global_section(section_bytes)?,
                7 => module.parse_export_section(section_bytes)?,
                8 => module.parse_start_section(section_bytes)?,
                9 => module.parse_element_section(section_bytes)?,
                10 => module.parse_code_section(section_bytes)?,
                11 => module.parse_data_section(section_bytes)?,
                _ => {} // Skip unknown sections
            }

            offset = section_end;
        }

        Ok(module)
    }

    fn parse_type_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            if bytes[offset] != 0x60 {
                return Err(WasmError::InvalidFormat("expected functype".to_string()));
            }
            offset += 1;

            // Parse params
            let (param_count, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;
            let mut params = Vec::with_capacity(param_count as usize);
            for _ in 0..param_count {
                let vt = WasmValueType::from_byte(bytes[offset])
                    .ok_or_else(|| WasmError::InvalidFormat("invalid value type".to_string()))?;
                params.push(vt);
                offset += 1;
            }

            // Parse results
            let (result_count, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;
            let mut results = Vec::with_capacity(result_count as usize);
            for _ in 0..result_count {
                let vt = WasmValueType::from_byte(bytes[offset])
                    .ok_or_else(|| WasmError::InvalidFormat("invalid value type".to_string()))?;
                results.push(vt);
                offset += 1;
            }

            self.types.push(FuncType { params, results });
        }

        Ok(())
    }

    fn parse_import_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let (module_name, name_len) = read_string(&bytes[offset..])?;
            offset += name_len;

            let (import_name, name_len) = read_string(&bytes[offset..])?;
            offset += name_len;

            let desc = match bytes[offset] {
                0x00 => {
                    offset += 1;
                    let (idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
                    offset += leb_size;
                    ImportDesc::Func(idx)
                }
                0x01 => {
                    offset += 1;
                    let table_type = parse_table_type(&bytes[offset..], &mut offset)?;
                    ImportDesc::Table(table_type)
                }
                0x02 => {
                    offset += 1;
                    let mem_type = parse_memory_type(&bytes[offset..], &mut offset)?;
                    ImportDesc::Memory(mem_type)
                }
                0x03 => {
                    offset += 1;
                    let global_type = parse_global_type(&bytes[offset..], &mut offset)?;
                    ImportDesc::Global(global_type)
                }
                _ => return Err(WasmError::InvalidFormat("invalid import desc".to_string())),
            };

            self.imports.push(Import {
                module: module_name,
                name: import_name,
                desc,
            });
        }

        Ok(())
    }

    fn parse_function_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let (idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;
            self.functions.push(idx);
        }

        Ok(())
    }

    fn parse_table_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let table_type = parse_table_type(&bytes[offset..], &mut offset)?;
            self.tables.push(table_type);
        }

        Ok(())
    }

    fn parse_memory_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let mem_type = parse_memory_type(&bytes[offset..], &mut offset)?;
            self.memories.push(mem_type);
        }

        Ok(())
    }

    fn parse_global_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let global_type = parse_global_type(&bytes[offset..], &mut offset)?;

            // Parse init expression until 0x0B (end)
            let init_start = offset;
            while bytes[offset] != 0x0B {
                offset += 1;
            }
            offset += 1; // consume 0x0B

            self.globals.push(Global {
                global_type,
                init: bytes[init_start..offset].to_vec(),
            });
        }

        Ok(())
    }

    fn parse_export_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let (name, name_len) = read_string(&bytes[offset..])?;
            offset += name_len;

            let desc = match bytes[offset] {
                0x00 => {
                    offset += 1;
                    let (idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
                    offset += leb_size;
                    ExportDesc::Func(idx)
                }
                0x01 => {
                    offset += 1;
                    let (idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
                    offset += leb_size;
                    ExportDesc::Table(idx)
                }
                0x02 => {
                    offset += 1;
                    let (idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
                    offset += leb_size;
                    ExportDesc::Memory(idx)
                }
                0x03 => {
                    offset += 1;
                    let (idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
                    offset += leb_size;
                    ExportDesc::Global(idx)
                }
                _ => return Err(WasmError::InvalidFormat("invalid export desc".to_string())),
            };

            self.exports.push(Export { name, desc });
        }

        Ok(())
    }

    fn parse_start_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let (idx, _) = read_leb128_u32(bytes)?;
        self.start = Some(idx);
        Ok(())
    }

    fn parse_element_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let (table_idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;

            // Parse offset expression
            let offset_start = offset;
            while bytes[offset] != 0x0B {
                offset += 1;
            }
            offset += 1;
            let offset_expr = bytes[offset_start..offset].to_vec();

            // Parse function indices
            let (init_count, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;
            let mut init = Vec::with_capacity(init_count as usize);
            for _ in 0..init_count {
                let (idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
                offset += leb_size;
                init.push(idx);
            }

            self.elements.push(Element {
                table_index: table_idx,
                offset: offset_expr,
                init,
            });
        }

        Ok(())
    }

    fn parse_code_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let (body_size, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;
            let body_end = offset + body_size as usize;

            // Parse locals
            let (local_count, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;
            let mut locals = Vec::with_capacity(local_count as usize);
            for _ in 0..local_count {
                let (n, leb_size) = read_leb128_u32(&bytes[offset..])?;
                offset += leb_size;
                let vt = WasmValueType::from_byte(bytes[offset])
                    .ok_or_else(|| WasmError::InvalidFormat("invalid local type".to_string()))?;
                offset += 1;
                locals.push((n, vt));
            }

            let code = bytes[offset..body_end].to_vec();
            offset = body_end;

            self.code.push(FunctionBody { locals, code });
        }

        Ok(())
    }

    fn parse_data_section(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        let mut offset = 0;
        let (count, leb_size) = read_leb128_u32(&bytes[offset..])?;
        offset += leb_size;

        for _ in 0..count {
            let (mem_idx, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;

            // Parse offset expression
            let offset_start = offset;
            while bytes[offset] != 0x0B {
                offset += 1;
            }
            offset += 1;
            let offset_expr = bytes[offset_start..offset].to_vec();

            // Parse data bytes
            let (data_len, leb_size) = read_leb128_u32(&bytes[offset..])?;
            offset += leb_size;
            let init = bytes[offset..offset + data_len as usize].to_vec();
            offset += data_len as usize;

            self.data.push(Data {
                memory_index: mem_idx,
                offset: offset_expr,
                init,
            });
        }

        Ok(())
    }

    /// Get an exported function by name
    pub fn get_export(&self, name: &str) -> Option<&Export> {
        self.exports.iter().find(|e| e.name == name)
    }
}

impl Default for Module {
    fn default() -> Self {
        Self::new()
    }
}

/// WASM errors
#[derive(Debug, Clone)]
pub enum WasmError {
    InvalidFormat(String),
    UnsupportedVersion(u32),
    LinkError(String),
    RuntimeError(String),
    OutOfBounds,
    DivisionByZero,
    IntegerOverflow,
    InvalidConversion,
    Unreachable,
}

impl std::fmt::Display for WasmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat(msg) => write!(f, "invalid WASM format: {}", msg),
            Self::UnsupportedVersion(v) => write!(f, "unsupported WASM version: {}", v),
            Self::LinkError(msg) => write!(f, "link error: {}", msg),
            Self::RuntimeError(msg) => write!(f, "runtime error: {}", msg),
            Self::OutOfBounds => write!(f, "out of bounds memory access"),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::IntegerOverflow => write!(f, "integer overflow"),
            Self::InvalidConversion => write!(f, "invalid conversion"),
            Self::Unreachable => write!(f, "unreachable code executed"),
        }
    }
}

impl std::error::Error for WasmError {}

// Helper functions

fn read_leb128_u32(bytes: &[u8]) -> Result<(u32, usize), WasmError> {
    let mut result = 0u32;
    let mut shift = 0;
    let mut i = 0;

    loop {
        if i >= bytes.len() {
            return Err(WasmError::InvalidFormat("unexpected end of LEB128".to_string()));
        }
        let byte = bytes[i];
        result |= ((byte & 0x7F) as u32) << shift;
        i += 1;
        if byte & 0x80 == 0 {
            return Ok((result, i));
        }
        shift += 7;
        if shift >= 35 {
            return Err(WasmError::InvalidFormat("LEB128 too long".to_string()));
        }
    }
}

fn read_string(bytes: &[u8]) -> Result<(String, usize), WasmError> {
    let (len, leb_size) = read_leb128_u32(bytes)?;
    let str_bytes = &bytes[leb_size..leb_size + len as usize];
    let s = std::str::from_utf8(str_bytes)
        .map_err(|_| WasmError::InvalidFormat("invalid UTF-8 string".to_string()))?;
    Ok((s.to_string(), leb_size + len as usize))
}

fn parse_limits(bytes: &[u8], offset: &mut usize) -> Result<Limits, WasmError> {
    let flags = bytes[*offset];
    *offset += 1;

    let (min, leb_size) = read_leb128_u32(&bytes[*offset..])?;
    *offset += leb_size;

    let max = if flags & 0x01 != 0 {
        let (max, leb_size) = read_leb128_u32(&bytes[*offset..])?;
        *offset += leb_size;
        Some(max)
    } else {
        None
    };

    Ok(Limits { min, max })
}

fn parse_table_type(bytes: &[u8], offset: &mut usize) -> Result<TableType, WasmError> {
    let element_type = WasmValueType::from_byte(bytes[*offset])
        .ok_or_else(|| WasmError::InvalidFormat("invalid table element type".to_string()))?;
    *offset += 1;
    let limits = parse_limits(bytes, offset)?;
    Ok(TableType { element_type, limits })
}

fn parse_memory_type(bytes: &[u8], offset: &mut usize) -> Result<MemoryType, WasmError> {
    let limits = parse_limits(bytes, offset)?;
    Ok(MemoryType { limits })
}

fn parse_global_type(bytes: &[u8], offset: &mut usize) -> Result<GlobalType, WasmError> {
    let value_type = WasmValueType::from_byte(bytes[*offset])
        .ok_or_else(|| WasmError::InvalidFormat("invalid global type".to_string()))?;
    *offset += 1;
    let mutable = bytes[*offset] != 0;
    *offset += 1;
    Ok(GlobalType { value_type, mutable })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leb128_parsing() {
        assert_eq!(read_leb128_u32(&[0x00]).unwrap(), (0, 1));
        assert_eq!(read_leb128_u32(&[0x01]).unwrap(), (1, 1));
        assert_eq!(read_leb128_u32(&[0x7F]).unwrap(), (127, 1));
        assert_eq!(read_leb128_u32(&[0x80, 0x01]).unwrap(), (128, 2));
        assert_eq!(read_leb128_u32(&[0xE5, 0x8E, 0x26]).unwrap(), (624485, 3));
    }

    #[test]
    fn test_value_types() {
        assert_eq!(WasmValueType::from_byte(0x7F), Some(WasmValueType::I32));
        assert_eq!(WasmValueType::from_byte(0x7E), Some(WasmValueType::I64));
        assert_eq!(WasmValueType::I32.to_byte(), 0x7F);
    }

    #[test]
    fn test_minimal_wasm() {
        // Minimal valid WASM module (magic + version only)
        let bytes = b"\0asm\x01\x00\x00\x00";
        let module = Module::parse(bytes).unwrap();
        assert!(module.types.is_empty());
        assert!(module.exports.is_empty());
    }
}
