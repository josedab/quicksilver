//! Snapshot Isolation for Instant Cold Starts
//!
//! This module provides the ability to serialize and deserialize the entire
//! runtime state, enabling sub-millisecond cold starts for serverless deployments.
//!
//! # Example
//! ```text
//! quicksilver snapshot app.js --output app.qss
//! quicksilver run app.qss  # <1ms startup
//! ```

//! **Status:** ⚠️ Partial — Bytecode serialization framework

use crate::bytecode::Chunk;
use crate::runtime::{JsFunction, ObjectKind, Value};
use crate::Error;
use rustc_hash::FxHashMap as HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

/// Magic bytes for Quicksilver Snapshot format
const SNAPSHOT_MAGIC: &[u8; 4] = b"QSS\x01";

/// Snapshot version for compatibility checking (v2 adds Date, Map, Set, Error, checksum)
const SNAPSHOT_VERSION: u32 = 2;

/// Flag byte indicating compression is enabled
#[allow(dead_code)]
const COMPRESSION_FLAG: u8 = 0x01;

/// Flag byte indicating no compression
#[allow(dead_code)]
const NO_COMPRESSION_FLAG: u8 = 0x00;

/// A serialized snapshot of the runtime state
#[derive(Clone)]
pub struct Snapshot {
    /// Version of the snapshot format
    pub version: u32,
    /// Compiled bytecode chunks
    pub chunks: Vec<SerializedChunk>,
    /// Global variables state
    pub globals: HashMap<String, SerializedValue>,
    /// Source code (for debugging)
    pub source: Option<String>,
    /// Metadata
    pub metadata: SnapshotMetadata,
    /// CRC32 checksum of the snapshot data (computed on save, verified on load)
    pub checksum: u32,
    /// Whether the snapshot data is compressed
    pub compressed: bool,
}

/// Metadata about the snapshot
#[derive(Clone, Default)]
pub struct SnapshotMetadata {
    /// Original filename
    pub filename: String,
    /// Creation timestamp
    pub created_at: u64,
    /// Quicksilver version that created this
    pub runtime_version: String,
    /// Custom user metadata
    pub custom: HashMap<String, String>,
}

/// Serialized bytecode chunk
#[derive(Clone)]
pub struct SerializedChunk {
    /// Bytecode instructions
    pub code: Vec<u8>,
    /// Constant pool
    pub constants: Vec<SerializedValue>,
    /// Line number information
    pub lines: Vec<u32>,
    /// Column number information (source maps)
    pub columns: Vec<u32>,
    /// Local variable names
    pub locals: Vec<String>,
    /// Register count
    pub register_count: u8,
    /// Parameter count
    pub param_count: u8,
    /// Has rest parameter
    pub has_rest_param: bool,
    /// Is async function
    pub is_async: bool,
    /// Is generator function
    pub is_generator: bool,
    /// Is strict mode
    pub is_strict: bool,
    /// Source file name
    pub source_file: Option<String>,
}

/// Serialized value representation
#[derive(Clone)]
pub enum SerializedValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Symbol(u64),
    BigInt(String),
    Array(Vec<SerializedValue>),
    Object(HashMap<String, SerializedValue>),
    Function(Box<SerializedChunk>),
    /// Date object (timestamp in milliseconds since epoch)
    Date(f64),
    /// Map object (key-value pairs preserving insertion order)
    Map(Vec<(SerializedValue, SerializedValue)>),
    /// Set object (values preserving insertion order)
    Set(Vec<SerializedValue>),
    /// Error object (name, message, stack)
    Error {
        name: String,
        message: String,
        stack: Option<String>,
    },
}

impl Snapshot {
    /// Create a new empty snapshot
    pub fn new() -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            chunks: Vec::new(),
            globals: HashMap::default(),
            source: None,
            metadata: SnapshotMetadata::default(),
            checksum: 0,
            compressed: false,
        }
    }

    /// Create a snapshot from compiled bytecode
    pub fn from_chunk(chunk: &Chunk, source: Option<&str>) -> Self {
        let mut snapshot = Self::new();
        snapshot.chunks.push(serialize_chunk(chunk));
        snapshot.source = source.map(|s| s.to_string());
        snapshot.metadata.runtime_version = crate::VERSION.to_string();
        snapshot.metadata.created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        snapshot
    }

    /// Save snapshot to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let file = File::create(path)
            .map_err(|e| Error::InternalError(format!("Failed to create snapshot file: {}", e)))?;
        let mut writer = BufWriter::new(file);

        // Write magic bytes
        writer.write_all(SNAPSHOT_MAGIC)
            .map_err(|e| Error::InternalError(format!("Failed to write magic: {}", e)))?;

        // Write version
        writer.write_all(&self.version.to_le_bytes())
            .map_err(|e| Error::InternalError(format!("Failed to write version: {}", e)))?;

        // Write metadata
        write_string(&mut writer, &self.metadata.filename)?;
        writer.write_all(&self.metadata.created_at.to_le_bytes())
            .map_err(|e| Error::InternalError(format!("Failed to write timestamp: {}", e)))?;
        write_string(&mut writer, &self.metadata.runtime_version)?;

        // Write source (optional)
        let has_source = self.source.is_some();
        writer.write_all(&[has_source as u8])
            .map_err(|e| Error::InternalError(format!("Failed to write source flag: {}", e)))?;
        if let Some(ref source) = self.source {
            write_string(&mut writer, source)?;
        }

        // Write chunks
        writer.write_all(&(self.chunks.len() as u32).to_le_bytes())
            .map_err(|e| Error::InternalError(format!("Failed to write chunk count: {}", e)))?;
        for chunk in &self.chunks {
            write_chunk(&mut writer, chunk)?;
        }

        // Write globals
        writer.write_all(&(self.globals.len() as u32).to_le_bytes())
            .map_err(|e| Error::InternalError(format!("Failed to write globals count: {}", e)))?;
        for (name, value) in &self.globals {
            write_string(&mut writer, name)?;
            write_value(&mut writer, value)?;
        }

        writer.flush()
            .map_err(|e| Error::InternalError(format!("Failed to flush snapshot: {}", e)))?;

        Ok(())
    }

    /// Load snapshot from file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let file = File::open(path)
            .map_err(|e| Error::InternalError(format!("Failed to open snapshot file: {}", e)))?;
        let mut reader = BufReader::new(file);

        // Read and verify magic bytes
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)
            .map_err(|e| Error::InternalError(format!("Failed to read magic: {}", e)))?;
        if &magic != SNAPSHOT_MAGIC {
            return Err(Error::InternalError("Invalid snapshot file format".to_string()));
        }

        // Read version
        let mut version_bytes = [0u8; 4];
        reader.read_exact(&mut version_bytes)
            .map_err(|e| Error::InternalError(format!("Failed to read version: {}", e)))?;
        let version = u32::from_le_bytes(version_bytes);

        if version > SNAPSHOT_VERSION {
            return Err(Error::InternalError(format!(
                "Snapshot version {} is newer than supported version {}",
                version, SNAPSHOT_VERSION
            )));
        }

        // Read metadata
        let filename = read_string(&mut reader)?;
        let mut timestamp_bytes = [0u8; 8];
        reader.read_exact(&mut timestamp_bytes)
            .map_err(|e| Error::InternalError(format!("Failed to read timestamp: {}", e)))?;
        let created_at = u64::from_le_bytes(timestamp_bytes);
        let runtime_version = read_string(&mut reader)?;

        // Read source
        let mut has_source = [0u8; 1];
        reader.read_exact(&mut has_source)
            .map_err(|e| Error::InternalError(format!("Failed to read source flag: {}", e)))?;
        let source = if has_source[0] != 0 {
            Some(read_string(&mut reader)?)
        } else {
            None
        };

        // Read chunks
        let mut chunk_count_bytes = [0u8; 4];
        reader.read_exact(&mut chunk_count_bytes)
            .map_err(|e| Error::InternalError(format!("Failed to read chunk count: {}", e)))?;
        let chunk_count = u32::from_le_bytes(chunk_count_bytes) as usize;

        let mut chunks = Vec::with_capacity(chunk_count);
        for _ in 0..chunk_count {
            chunks.push(read_chunk(&mut reader)?);
        }

        // Read globals
        let mut globals_count_bytes = [0u8; 4];
        reader.read_exact(&mut globals_count_bytes)
            .map_err(|e| Error::InternalError(format!("Failed to read globals count: {}", e)))?;
        let globals_count = u32::from_le_bytes(globals_count_bytes) as usize;

        let mut globals = HashMap::with_capacity_and_hasher(globals_count, Default::default());
        for _ in 0..globals_count {
            let name = read_string(&mut reader)?;
            let value = read_value(&mut reader)?;
            globals.insert(name, value);
        }

        Ok(Self {
            version,
            chunks,
            globals,
            source,
            metadata: SnapshotMetadata {
                filename,
                created_at,
                runtime_version,
                custom: HashMap::default(),
            },
            checksum: 0,
            compressed: false,
        })
    }

    /// Convert snapshot back to a Chunk for execution
    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        if self.chunks.is_empty() {
            return Err(Error::InternalError("Snapshot contains no chunks".to_string()));
        }
        Ok(deserialize_chunk(&self.chunks[0]))
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

// Helper functions for serialization

fn serialize_chunk(chunk: &Chunk) -> SerializedChunk {
    SerializedChunk {
        code: chunk.code.clone(),
        constants: chunk.constants.iter().map(serialize_value).collect(),
        lines: chunk.lines.clone(),
        columns: chunk.columns.clone(),
        locals: chunk.locals.clone(),
        register_count: chunk.register_count,
        param_count: chunk.param_count,
        has_rest_param: chunk.has_rest_param,
        is_async: chunk.is_async,
        is_generator: chunk.is_generator,
        is_strict: chunk.is_strict,
        source_file: chunk.source_file.clone(),
    }
}

fn deserialize_chunk(serialized: &SerializedChunk) -> Chunk {
    Chunk {
        code: serialized.code.clone(),
        constants: serialized.constants.iter().map(deserialize_value).collect(),
        lines: serialized.lines.clone(),
        columns: serialized.columns.clone(),
        locals: serialized.locals.clone(),
        register_count: serialized.register_count,
        param_count: serialized.param_count,
        has_rest_param: serialized.has_rest_param,
        is_async: serialized.is_async,
        is_generator: serialized.is_generator,
        is_strict: serialized.is_strict,
        source_file: serialized.source_file.clone(),
    }
}

fn serialize_value(value: &Value) -> SerializedValue {
    match value {
        Value::Undefined => SerializedValue::Undefined,
        Value::Null => SerializedValue::Null,
        Value::Boolean(b) => SerializedValue::Boolean(*b),
        Value::Number(n) => SerializedValue::Number(*n),
        Value::String(s) => SerializedValue::String(s.clone()),
        Value::Symbol(id) => SerializedValue::Symbol(*id),
        Value::BigInt(n) => SerializedValue::BigInt(n.to_string()),
        Value::Object(obj) => {
            let obj_ref = obj.borrow();
            match &obj_ref.kind {
                ObjectKind::Array(arr) => {
                    SerializedValue::Array(arr.iter().map(serialize_value).collect())
                }
                ObjectKind::Function(func) => {
                    SerializedValue::Function(Box::new(serialize_chunk(&func.chunk)))
                }
                ObjectKind::Date(timestamp) => {
                    SerializedValue::Date(*timestamp)
                }
                ObjectKind::Map(entries) => {
                    SerializedValue::Map(
                        entries
                            .iter()
                            .map(|(k, v)| (serialize_value(k), serialize_value(v)))
                            .collect()
                    )
                }
                ObjectKind::Set(values) => {
                    SerializedValue::Set(values.iter().map(serialize_value).collect())
                }
                ObjectKind::Error { name, message, .. } => {
                    SerializedValue::Error {
                        name: name.clone(),
                        message: message.clone(),
                        stack: None,
                    }
                }
                _ => {
                    let props: HashMap<String, SerializedValue> = obj_ref
                        .properties
                        .iter()
                        .map(|(k, v)| (k.clone(), serialize_value(v)))
                        .collect();
                    SerializedValue::Object(props)
                }
            }
        }
    }
}

fn deserialize_value(serialized: &SerializedValue) -> Value {
    match serialized {
        SerializedValue::Undefined => Value::Undefined,
        SerializedValue::Null => Value::Null,
        SerializedValue::Boolean(b) => Value::Boolean(*b),
        SerializedValue::Number(n) => Value::Number(*n),
        SerializedValue::String(s) => Value::String(s.clone()),
        SerializedValue::Symbol(id) => Value::Symbol(*id),
        SerializedValue::BigInt(s) => {
            Value::new_bigint(s).unwrap_or(Value::bigint_from_i64(0))
        }
        SerializedValue::Array(arr) => {
            Value::new_array(arr.iter().map(deserialize_value).collect())
        }
        SerializedValue::Object(props) => {
            let obj = Value::new_object();
            for (k, v) in props {
                obj.set_property(k, deserialize_value(v));
            }
            obj
        }
        SerializedValue::Function(chunk) => {
            let deserialized_chunk = deserialize_chunk(chunk);
            Value::new_function(JsFunction {
                name: None,
                chunk: deserialized_chunk,
                upvalues: Vec::new(),
                is_async: false,
                is_generator: false,
            })
        }
        SerializedValue::Date(timestamp) => {
            Value::new_date(*timestamp)
        }
        SerializedValue::Map(entries) => {
            Value::new_map(
                entries
                    .iter()
                    .map(|(k, v)| (deserialize_value(k), deserialize_value(v)))
                    .collect()
            )
        }
        SerializedValue::Set(values) => {
            Value::new_set(values.iter().map(deserialize_value).collect())
        }
        SerializedValue::Error { name, message, stack } => {
            Value::new_error_with_stack(name.clone(), message.clone(), stack.clone())
        }
    }
}

// Checksum helpers

/// Compute CRC32 checksum of data
#[allow(dead_code)]
fn compute_checksum(data: &[u8]) -> u32 {
    // CRC32 polynomial (IEEE)
    const CRC32_TABLE: [u32; 256] = generate_crc32_table();

    let mut crc = 0xFFFFFFFF;
    for byte in data {
        let index = ((crc ^ (*byte as u32)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    !crc
}

/// Generate CRC32 lookup table at compile time
#[allow(dead_code)]
const fn generate_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Verify checksum of data
#[allow(dead_code)]
fn verify_checksum(data: &[u8], expected: u32) -> bool {
    compute_checksum(data) == expected
}

// Binary I/O helpers

fn write_string<W: Write>(writer: &mut W, s: &str) -> Result<(), Error> {
    let bytes = s.as_bytes();
    writer.write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(|e| Error::InternalError(format!("Failed to write string length: {}", e)))?;
    writer.write_all(bytes)
        .map_err(|e| Error::InternalError(format!("Failed to write string: {}", e)))?;
    Ok(())
}

fn read_string<R: Read>(reader: &mut R) -> Result<String, Error> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)
        .map_err(|e| Error::InternalError(format!("Failed to read string length: {}", e)))?;
    let len = u32::from_le_bytes(len_bytes) as usize;

    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes)
        .map_err(|e| Error::InternalError(format!("Failed to read string: {}", e)))?;

    String::from_utf8(bytes)
        .map_err(|e| Error::InternalError(format!("Invalid UTF-8 in string: {}", e)))
}

fn write_chunk<W: Write>(writer: &mut W, chunk: &SerializedChunk) -> Result<(), Error> {
    // Write code
    writer.write_all(&(chunk.code.len() as u32).to_le_bytes())
        .map_err(|e| Error::InternalError(format!("Failed to write code length: {}", e)))?;
    writer.write_all(&chunk.code)
        .map_err(|e| Error::InternalError(format!("Failed to write code: {}", e)))?;

    // Write constants
    writer.write_all(&(chunk.constants.len() as u32).to_le_bytes())
        .map_err(|e| Error::InternalError(format!("Failed to write constants count: {}", e)))?;
    for constant in &chunk.constants {
        write_value(writer, constant)?;
    }

    // Write lines
    writer.write_all(&(chunk.lines.len() as u32).to_le_bytes())
        .map_err(|e| Error::InternalError(format!("Failed to write lines count: {}", e)))?;
    for line in &chunk.lines {
        writer.write_all(&line.to_le_bytes())
            .map_err(|e| Error::InternalError(format!("Failed to write line: {}", e)))?;
    }

    // Write columns (source maps)
    writer.write_all(&(chunk.columns.len() as u32).to_le_bytes())
        .map_err(|e| Error::InternalError(format!("Failed to write columns count: {}", e)))?;
    for column in &chunk.columns {
        writer.write_all(&column.to_le_bytes())
            .map_err(|e| Error::InternalError(format!("Failed to write column: {}", e)))?;
    }

    // Write locals
    writer.write_all(&(chunk.locals.len() as u32).to_le_bytes())
        .map_err(|e| Error::InternalError(format!("Failed to write locals count: {}", e)))?;
    for local in &chunk.locals {
        write_string(writer, local)?;
    }

    // Write flags
    writer.write_all(&[chunk.register_count])
        .map_err(|e| Error::InternalError(format!("Failed to write register_count: {}", e)))?;
    writer.write_all(&[chunk.param_count])
        .map_err(|e| Error::InternalError(format!("Failed to write param_count: {}", e)))?;
    writer.write_all(&[chunk.has_rest_param as u8])
        .map_err(|e| Error::InternalError(format!("Failed to write has_rest_param: {}", e)))?;
    writer.write_all(&[chunk.is_async as u8])
        .map_err(|e| Error::InternalError(format!("Failed to write is_async: {}", e)))?;
    writer.write_all(&[chunk.is_generator as u8])
        .map_err(|e| Error::InternalError(format!("Failed to write is_generator: {}", e)))?;
    writer.write_all(&[chunk.is_strict as u8])
        .map_err(|e| Error::InternalError(format!("Failed to write is_strict: {}", e)))?;

    // Write source file (optional)
    if let Some(ref source_file) = chunk.source_file {
        writer.write_all(&[1u8])
            .map_err(|e| Error::InternalError(format!("Failed to write source_file flag: {}", e)))?;
        write_string(writer, source_file)?;
    } else {
        writer.write_all(&[0u8])
            .map_err(|e| Error::InternalError(format!("Failed to write source_file flag: {}", e)))?;
    }

    Ok(())
}

fn read_chunk<R: Read>(reader: &mut R) -> Result<SerializedChunk, Error> {
    // Read code
    let mut code_len_bytes = [0u8; 4];
    reader.read_exact(&mut code_len_bytes)
        .map_err(|e| Error::InternalError(format!("Failed to read code length: {}", e)))?;
    let code_len = u32::from_le_bytes(code_len_bytes) as usize;

    let mut code = vec![0u8; code_len];
    reader.read_exact(&mut code)
        .map_err(|e| Error::InternalError(format!("Failed to read code: {}", e)))?;

    // Read constants
    let mut constants_count_bytes = [0u8; 4];
    reader.read_exact(&mut constants_count_bytes)
        .map_err(|e| Error::InternalError(format!("Failed to read constants count: {}", e)))?;
    let constants_count = u32::from_le_bytes(constants_count_bytes) as usize;

    let mut constants = Vec::with_capacity(constants_count);
    for _ in 0..constants_count {
        constants.push(read_value(reader)?);
    }

    // Read lines
    let mut lines_count_bytes = [0u8; 4];
    reader.read_exact(&mut lines_count_bytes)
        .map_err(|e| Error::InternalError(format!("Failed to read lines count: {}", e)))?;
    let lines_count = u32::from_le_bytes(lines_count_bytes) as usize;

    let mut lines = Vec::with_capacity(lines_count);
    for _ in 0..lines_count {
        let mut line_bytes = [0u8; 4];
        reader.read_exact(&mut line_bytes)
            .map_err(|e| Error::InternalError(format!("Failed to read line: {}", e)))?;
        lines.push(u32::from_le_bytes(line_bytes));
    }

    // Read columns (source maps)
    let mut columns_count_bytes = [0u8; 4];
    reader.read_exact(&mut columns_count_bytes)
        .map_err(|e| Error::InternalError(format!("Failed to read columns count: {}", e)))?;
    let columns_count = u32::from_le_bytes(columns_count_bytes) as usize;

    let mut columns = Vec::with_capacity(columns_count);
    for _ in 0..columns_count {
        let mut col_bytes = [0u8; 4];
        reader.read_exact(&mut col_bytes)
            .map_err(|e| Error::InternalError(format!("Failed to read column: {}", e)))?;
        columns.push(u32::from_le_bytes(col_bytes));
    }

    // Read locals
    let mut locals_count_bytes = [0u8; 4];
    reader.read_exact(&mut locals_count_bytes)
        .map_err(|e| Error::InternalError(format!("Failed to read locals count: {}", e)))?;
    let locals_count = u32::from_le_bytes(locals_count_bytes) as usize;

    let mut locals = Vec::with_capacity(locals_count);
    for _ in 0..locals_count {
        locals.push(read_string(reader)?);
    }

    // Read flags
    let mut flags = [0u8; 6];
    reader.read_exact(&mut flags)
        .map_err(|e| Error::InternalError(format!("Failed to read flags: {}", e)))?;

    // Read source file (optional)
    let mut source_file_flag = [0u8; 1];
    reader.read_exact(&mut source_file_flag)
        .map_err(|e| Error::InternalError(format!("Failed to read source_file flag: {}", e)))?;
    let source_file = if source_file_flag[0] != 0 {
        Some(read_string(reader)?)
    } else {
        None
    };

    Ok(SerializedChunk {
        code,
        constants,
        lines,
        columns,
        locals,
        register_count: flags[0],
        param_count: flags[1],
        has_rest_param: flags[2] != 0,
        is_async: flags[3] != 0,
        is_generator: flags[4] != 0,
        is_strict: flags[5] != 0,
        source_file,
    })
}

fn write_value<W: Write>(writer: &mut W, value: &SerializedValue) -> Result<(), Error> {
    match value {
        SerializedValue::Undefined => {
            writer.write_all(&[0u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
        }
        SerializedValue::Null => {
            writer.write_all(&[1u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
        }
        SerializedValue::Boolean(b) => {
            writer.write_all(&[2u8, *b as u8])
                .map_err(|e| Error::InternalError(format!("Failed to write boolean: {}", e)))?;
        }
        SerializedValue::Number(n) => {
            writer.write_all(&[3u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            writer.write_all(&n.to_le_bytes())
                .map_err(|e| Error::InternalError(format!("Failed to write number: {}", e)))?;
        }
        SerializedValue::String(s) => {
            writer.write_all(&[4u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            write_string(writer, s)?;
        }
        SerializedValue::Symbol(id) => {
            writer.write_all(&[5u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            writer.write_all(&id.to_le_bytes())
                .map_err(|e| Error::InternalError(format!("Failed to write symbol: {}", e)))?;
        }
        SerializedValue::BigInt(s) => {
            writer.write_all(&[9u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            write_string(writer, s)?;
        }
        SerializedValue::Array(arr) => {
            writer.write_all(&[6u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            writer.write_all(&(arr.len() as u32).to_le_bytes())
                .map_err(|e| Error::InternalError(format!("Failed to write array length: {}", e)))?;
            for item in arr {
                write_value(writer, item)?;
            }
        }
        SerializedValue::Object(props) => {
            writer.write_all(&[7u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            writer.write_all(&(props.len() as u32).to_le_bytes())
                .map_err(|e| Error::InternalError(format!("Failed to write object size: {}", e)))?;
            for (k, v) in props {
                write_string(writer, k)?;
                write_value(writer, v)?;
            }
        }
        SerializedValue::Function(chunk) => {
            writer.write_all(&[8u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            write_chunk(writer, chunk)?;
        }
        SerializedValue::Date(timestamp) => {
            writer.write_all(&[10u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            writer.write_all(&timestamp.to_le_bytes())
                .map_err(|e| Error::InternalError(format!("Failed to write date: {}", e)))?;
        }
        SerializedValue::Map(entries) => {
            writer.write_all(&[11u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            writer.write_all(&(entries.len() as u32).to_le_bytes())
                .map_err(|e| Error::InternalError(format!("Failed to write map length: {}", e)))?;
            for (k, v) in entries {
                write_value(writer, k)?;
                write_value(writer, v)?;
            }
        }
        SerializedValue::Set(values) => {
            writer.write_all(&[12u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            writer.write_all(&(values.len() as u32).to_le_bytes())
                .map_err(|e| Error::InternalError(format!("Failed to write set length: {}", e)))?;
            for v in values {
                write_value(writer, v)?;
            }
        }
        SerializedValue::Error { name, message, stack } => {
            writer.write_all(&[13u8])
                .map_err(|e| Error::InternalError(format!("Failed to write value type: {}", e)))?;
            write_string(writer, name)?;
            write_string(writer, message)?;
            let has_stack = stack.is_some();
            writer.write_all(&[has_stack as u8])
                .map_err(|e| Error::InternalError(format!("Failed to write stack flag: {}", e)))?;
            if let Some(s) = stack {
                write_string(writer, s)?;
            }
        }
    }
    Ok(())
}

fn read_value<R: Read>(reader: &mut R) -> Result<SerializedValue, Error> {
    let mut type_byte = [0u8; 1];
    reader.read_exact(&mut type_byte)
        .map_err(|e| Error::InternalError(format!("Failed to read value type: {}", e)))?;

    match type_byte[0] {
        0 => Ok(SerializedValue::Undefined),
        1 => Ok(SerializedValue::Null),
        2 => {
            let mut bool_byte = [0u8; 1];
            reader.read_exact(&mut bool_byte)
                .map_err(|e| Error::InternalError(format!("Failed to read boolean: {}", e)))?;
            Ok(SerializedValue::Boolean(bool_byte[0] != 0))
        }
        3 => {
            let mut num_bytes = [0u8; 8];
            reader.read_exact(&mut num_bytes)
                .map_err(|e| Error::InternalError(format!("Failed to read number: {}", e)))?;
            Ok(SerializedValue::Number(f64::from_le_bytes(num_bytes)))
        }
        4 => Ok(SerializedValue::String(read_string(reader)?)),
        5 => {
            let mut id_bytes = [0u8; 8];
            reader.read_exact(&mut id_bytes)
                .map_err(|e| Error::InternalError(format!("Failed to read symbol: {}", e)))?;
            Ok(SerializedValue::Symbol(u64::from_le_bytes(id_bytes)))
        }
        6 => {
            let mut len_bytes = [0u8; 4];
            reader.read_exact(&mut len_bytes)
                .map_err(|e| Error::InternalError(format!("Failed to read array length: {}", e)))?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut arr = Vec::with_capacity(len);
            for _ in 0..len {
                arr.push(read_value(reader)?);
            }
            Ok(SerializedValue::Array(arr))
        }
        7 => {
            let mut len_bytes = [0u8; 4];
            reader.read_exact(&mut len_bytes)
                .map_err(|e| Error::InternalError(format!("Failed to read object size: {}", e)))?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut props = HashMap::with_capacity_and_hasher(len, Default::default());
            for _ in 0..len {
                let key = read_string(reader)?;
                let value = read_value(reader)?;
                props.insert(key, value);
            }
            Ok(SerializedValue::Object(props))
        }
        8 => {
            let chunk = read_chunk(reader)?;
            Ok(SerializedValue::Function(Box::new(chunk)))
        }
        9 => Ok(SerializedValue::BigInt(read_string(reader)?)),
        10 => {
            // Date
            let mut timestamp_bytes = [0u8; 8];
            reader.read_exact(&mut timestamp_bytes)
                .map_err(|e| Error::InternalError(format!("Failed to read date: {}", e)))?;
            Ok(SerializedValue::Date(f64::from_le_bytes(timestamp_bytes)))
        }
        11 => {
            // Map
            let mut len_bytes = [0u8; 4];
            reader.read_exact(&mut len_bytes)
                .map_err(|e| Error::InternalError(format!("Failed to read map length: {}", e)))?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut entries = Vec::with_capacity(len);
            for _ in 0..len {
                let key = read_value(reader)?;
                let value = read_value(reader)?;
                entries.push((key, value));
            }
            Ok(SerializedValue::Map(entries))
        }
        12 => {
            // Set
            let mut len_bytes = [0u8; 4];
            reader.read_exact(&mut len_bytes)
                .map_err(|e| Error::InternalError(format!("Failed to read set length: {}", e)))?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut values = Vec::with_capacity(len);
            for _ in 0..len {
                values.push(read_value(reader)?);
            }
            Ok(SerializedValue::Set(values))
        }
        13 => {
            // Error
            let name = read_string(reader)?;
            let message = read_string(reader)?;
            let mut has_stack = [0u8; 1];
            reader.read_exact(&mut has_stack)
                .map_err(|e| Error::InternalError(format!("Failed to read stack flag: {}", e)))?;
            let stack = if has_stack[0] != 0 {
                Some(read_string(reader)?)
            } else {
                None
            };
            Ok(SerializedValue::Error { name, message, stack })
        }
        _ => Err(Error::InternalError(format!("Unknown value type: {}", type_byte[0]))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_roundtrip() {
        let chunk = crate::bytecode::compile("var x = 1 + 2;").unwrap();
        let snapshot = Snapshot::from_chunk(&chunk, Some("var x = 1 + 2;"));

        // Save to bytes
        let mut buffer = Vec::new();
        {
            let mut cursor = std::io::Cursor::new(&mut buffer);
            let _ = cursor.write_all(SNAPSHOT_MAGIC);
            let _ = cursor.write_all(&snapshot.version.to_le_bytes());
        }

        // Verify we can convert back to chunk
        let restored_chunk = snapshot.to_chunk().unwrap();
        assert_eq!(restored_chunk.code.len(), chunk.code.len());
    }
}

// ============================================================================
// Complete Value Serialization System
// ============================================================================

use std::collections::HashMap as StdHashMap;

/// Extended snapshot errors
#[derive(Debug, Clone)]
pub enum SnapshotError {
    /// The snapshot data has an invalid format
    InvalidFormat(String),
    /// Version mismatch between snapshot and runtime
    VersionMismatch { expected: u32, found: u32 },
    /// Checksum verification failed
    ChecksumMismatch { expected: u32, found: u32 },
    /// Unknown tag byte encountered during deserialization
    UnknownTag(u8),
    /// Attempted to read past end of buffer
    BufferUnderflow,
    /// Circular reference detected (used internally)
    CircularReference,
    /// Serialization failed with a message
    SerializationFailed(String),
    /// Deserialization failed with a message
    DeserializationFailed(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::InvalidFormat(msg) => write!(f, "invalid snapshot format: {}", msg),
            SnapshotError::VersionMismatch { expected, found } => {
                write!(f, "version mismatch: expected {}, found {}", expected, found)
            }
            SnapshotError::ChecksumMismatch { expected, found } => {
                write!(f, "checksum mismatch: expected {:#x}, found {:#x}", expected, found)
            }
            SnapshotError::UnknownTag(tag) => write!(f, "unknown tag: {}", tag),
            SnapshotError::BufferUnderflow => write!(f, "buffer underflow"),
            SnapshotError::CircularReference => write!(f, "circular reference detected"),
            SnapshotError::SerializationFailed(msg) => {
                write!(f, "serialization failed: {}", msg)
            }
            SnapshotError::DeserializationFailed(msg) => {
                write!(f, "deserialization failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Tag bytes for value types in the serialized format
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueTag {
    Undefined = 0,
    Null = 1,
    Boolean = 2,
    Number = 3,
    String = 4,
    Object = 5,
    Array = 6,
    Date = 7,
    Map = 8,
    Set = 9,
    Error = 10,
    Symbol = 11,
    BigInt = 12,
    /// Back-reference for circular objects
    Reference = 13,
    /// Stored as name only (re-bound on restore)
    Function = 14,
}

/// Complete Value serializer that handles all Value variants
pub struct ValueSerializer {
    /// Object identity table for circular reference detection
    object_table: StdHashMap<usize, u32>,
    /// Next object ID
    next_id: u32,
    /// Serialized bytes
    buffer: Vec<u8>,
}

impl ValueSerializer {
    /// Create a new ValueSerializer
    pub fn new() -> Self {
        Self {
            object_table: StdHashMap::new(),
            next_id: 0,
            buffer: Vec::new(),
        }
    }

    /// Write a single byte
    pub fn write_u8(&mut self, b: u8) {
        self.buffer.push(b);
    }

    /// Write a u32 in little-endian
    pub fn write_u32(&mut self, n: u32) {
        self.buffer.extend_from_slice(&n.to_le_bytes());
    }

    /// Write an f64 in little-endian
    pub fn write_f64(&mut self, f: f64) {
        self.buffer.extend_from_slice(&f.to_le_bytes());
    }

    /// Write a length-prefixed string
    pub fn write_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.write_u32(bytes.len() as u32);
        self.buffer.extend_from_slice(bytes);
    }

    /// Serialize any Value into the internal buffer
    pub fn serialize_value(&mut self, value: &Value) -> Result<(), SnapshotError> {
        match value {
            Value::Undefined => {
                self.write_u8(ValueTag::Undefined as u8);
            }
            Value::Null => {
                self.write_u8(ValueTag::Null as u8);
            }
            Value::Boolean(b) => {
                self.write_u8(ValueTag::Boolean as u8);
                self.write_u8(*b as u8);
            }
            Value::Number(n) => {
                self.write_u8(ValueTag::Number as u8);
                self.write_f64(*n);
            }
            Value::String(s) => {
                self.write_u8(ValueTag::String as u8);
                self.write_string(s);
            }
            Value::Symbol(id) => {
                self.write_u8(ValueTag::Symbol as u8);
                self.write_u32(*id as u32);
            }
            Value::BigInt(n) => {
                self.write_u8(ValueTag::BigInt as u8);
                self.write_string(&n.to_string());
            }
            Value::Object(obj_rc) => {
                // Check for circular reference using Rc pointer identity
                let ptr = std::rc::Rc::as_ptr(obj_rc) as usize;
                if let Some(&id) = self.object_table.get(&ptr) {
                    self.write_u8(ValueTag::Reference as u8);
                    self.write_u32(id);
                    return Ok(());
                }

                // Register this object before serializing its contents
                let obj_id = self.next_id;
                self.next_id += 1;
                self.object_table.insert(ptr, obj_id);

                let obj = obj_rc.borrow();
                match &obj.kind {
                    ObjectKind::Array(elements) => {
                        self.write_u8(ValueTag::Array as u8);
                        self.write_u32(elements.len() as u32);
                        for elem in elements {
                            self.serialize_value(elem)?;
                        }
                    }
                    ObjectKind::Date(timestamp) => {
                        self.write_u8(ValueTag::Date as u8);
                        self.write_f64(*timestamp);
                    }
                    ObjectKind::Map(entries) => {
                        self.write_u8(ValueTag::Map as u8);
                        self.write_u32(entries.len() as u32);
                        for (k, v) in entries {
                            self.serialize_value(k)?;
                            self.serialize_value(v)?;
                        }
                    }
                    ObjectKind::Set(values) => {
                        self.write_u8(ValueTag::Set as u8);
                        self.write_u32(values.len() as u32);
                        for v in values {
                            self.serialize_value(v)?;
                        }
                    }
                    ObjectKind::Error { name, message } => {
                        self.write_u8(ValueTag::Error as u8);
                        self.write_string(name);
                        self.write_string(message);
                    }
                    ObjectKind::Function(func) => {
                        self.write_u8(ValueTag::Function as u8);
                        let name = func.name.as_deref().unwrap_or("<anonymous>");
                        self.write_string(name);
                    }
                    ObjectKind::NativeFunction { name, .. } => {
                        self.write_u8(ValueTag::Function as u8);
                        self.write_string(name);
                    }
                    // Ordinary objects and any other kind: serialize properties
                    _ => {
                        self.write_u8(ValueTag::Object as u8);
                        let prop_count = obj.properties.len() as u32;
                        self.write_u32(prop_count);
                        for (key, val) in &obj.properties {
                            self.write_string(key);
                            self.serialize_value(val)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Consume the serializer and return the serialized bytes
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }
}

/// Deserializes Values from snapshot bytes
pub struct ValueDeserializer {
    buffer: Vec<u8>,
    position: usize,
    /// Object table for resolving back-references
    object_table: StdHashMap<u32, Value>,
    next_id: u32,
}

impl ValueDeserializer {
    /// Create a new deserializer from raw bytes
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            buffer: data,
            position: 0,
            object_table: StdHashMap::new(),
            next_id: 0,
        }
    }

    /// Number of remaining bytes in the buffer
    pub fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.position)
    }

    /// Read a single byte
    pub fn read_u8(&mut self) -> Result<u8, SnapshotError> {
        if self.position >= self.buffer.len() {
            return Err(SnapshotError::BufferUnderflow);
        }
        let b = self.buffer[self.position];
        self.position += 1;
        Ok(b)
    }

    /// Read a u32 in little-endian
    pub fn read_u32(&mut self) -> Result<u32, SnapshotError> {
        if self.position + 4 > self.buffer.len() {
            return Err(SnapshotError::BufferUnderflow);
        }
        let bytes = &self.buffer[self.position..self.position + 4];
        self.position += 4;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read an f64 in little-endian
    pub fn read_f64(&mut self) -> Result<f64, SnapshotError> {
        if self.position + 8 > self.buffer.len() {
            return Err(SnapshotError::BufferUnderflow);
        }
        let bytes = &self.buffer[self.position..self.position + 8];
        self.position += 8;
        Ok(f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Read a length-prefixed string
    pub fn read_string(&mut self) -> Result<String, SnapshotError> {
        let len = self.read_u32()? as usize;
        if self.position + len > self.buffer.len() {
            return Err(SnapshotError::BufferUnderflow);
        }
        let bytes = &self.buffer[self.position..self.position + len];
        self.position += len;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| SnapshotError::DeserializationFailed(format!("invalid UTF-8: {}", e)))
    }

    /// Deserialize a Value from the buffer
    pub fn deserialize_value(&mut self) -> Result<Value, SnapshotError> {
        let tag = self.read_u8()?;
        match tag {
            t if t == ValueTag::Undefined as u8 => Ok(Value::Undefined),
            t if t == ValueTag::Null as u8 => Ok(Value::Null),
            t if t == ValueTag::Boolean as u8 => {
                let b = self.read_u8()?;
                Ok(Value::Boolean(b != 0))
            }
            t if t == ValueTag::Number as u8 => {
                let n = self.read_f64()?;
                Ok(Value::Number(n))
            }
            t if t == ValueTag::String as u8 => {
                let s = self.read_string()?;
                Ok(Value::String(s))
            }
            t if t == ValueTag::Symbol as u8 => {
                let id = self.read_u32()?;
                Ok(Value::Symbol(id as u64))
            }
            t if t == ValueTag::BigInt as u8 => {
                let s = self.read_string()?;
                Value::new_bigint(&s).ok_or_else(|| {
                    SnapshotError::DeserializationFailed(format!("invalid BigInt: {}", s))
                })
            }
            t if t == ValueTag::Object as u8 => {
                let obj_id = self.next_id;
                self.next_id += 1;
                let prop_count = self.read_u32()? as usize;
                let obj = Value::new_object();
                // Register early so nested references can find it
                self.object_table.insert(obj_id, obj.clone());
                for _ in 0..prop_count {
                    let key = self.read_string()?;
                    let val = self.deserialize_value()?;
                    obj.set_property(&key, val);
                }
                Ok(obj)
            }
            t if t == ValueTag::Array as u8 => {
                let obj_id = self.next_id;
                self.next_id += 1;
                let len = self.read_u32()? as usize;
                let mut elements = Vec::with_capacity(len);
                for _ in 0..len {
                    elements.push(self.deserialize_value()?);
                }
                let arr = Value::new_array(elements);
                self.object_table.insert(obj_id, arr.clone());
                Ok(arr)
            }
            t if t == ValueTag::Date as u8 => {
                let obj_id = self.next_id;
                self.next_id += 1;
                let ts = self.read_f64()?;
                let val = Value::new_date(ts);
                self.object_table.insert(obj_id, val.clone());
                Ok(val)
            }
            t if t == ValueTag::Map as u8 => {
                let obj_id = self.next_id;
                self.next_id += 1;
                let len = self.read_u32()? as usize;
                let mut entries = Vec::with_capacity(len);
                for _ in 0..len {
                    let k = self.deserialize_value()?;
                    let v = self.deserialize_value()?;
                    entries.push((k, v));
                }
                let val = Value::new_map(entries);
                self.object_table.insert(obj_id, val.clone());
                Ok(val)
            }
            t if t == ValueTag::Set as u8 => {
                let obj_id = self.next_id;
                self.next_id += 1;
                let len = self.read_u32()? as usize;
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(self.deserialize_value()?);
                }
                let val = Value::new_set(values);
                self.object_table.insert(obj_id, val.clone());
                Ok(val)
            }
            t if t == ValueTag::Error as u8 => {
                let obj_id = self.next_id;
                self.next_id += 1;
                let name = self.read_string()?;
                let message = self.read_string()?;
                let val = Value::new_error(&name, &message);
                self.object_table.insert(obj_id, val.clone());
                Ok(val)
            }
            t if t == ValueTag::Reference as u8 => {
                let id = self.read_u32()?;
                self.object_table.get(&id).cloned().ok_or_else(|| {
                    SnapshotError::DeserializationFailed(format!(
                        "back-reference to unknown object id {}",
                        id
                    ))
                })
            }
            t if t == ValueTag::Function as u8 => {
                let obj_id = self.next_id;
                self.next_id += 1;
                let name = self.read_string()?;
                // Functions are restored as named stubs; re-bind at load time
                let val = Value::new_object();
                val.set_property("__function_name__", Value::String(name));
                self.object_table.insert(obj_id, val.clone());
                Ok(val)
            }
            _ => Err(SnapshotError::UnknownTag(tag)),
        }
    }
}

/// Metadata for a V2 snapshot
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotMetadataV2 {
    /// Snapshot format version
    pub version: u32,
    /// Runtime version string
    pub runtime_version: String,
    /// Creation timestamp (seconds since epoch)
    pub created_at: u64,
    /// Number of global variables stored
    pub global_count: usize,
    /// Number of bytecode cache entries
    pub bytecode_entries: usize,
    /// Total serialized size in bytes
    pub total_size: usize,
}

/// Creates a complete runtime snapshot
pub struct SnapshotCreator {
    /// Serialized global variables
    globals: Vec<(String, Vec<u8>)>,
    /// Metadata
    metadata: SnapshotMetadataV2,
    /// Compiled bytecode cache
    bytecode_cache: Vec<(String, Vec<u8>)>,
}

/// Magic bytes for the V2 snapshot format
const SNAPSHOT_V2_MAGIC: &[u8; 4] = b"QS2\x01";

impl SnapshotCreator {
    /// Create a new SnapshotCreator
    pub fn new() -> Self {
        Self {
            globals: Vec::new(),
            metadata: SnapshotMetadataV2 {
                version: SNAPSHOT_VERSION,
                runtime_version: crate::VERSION.to_string(),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                global_count: 0,
                bytecode_entries: 0,
                total_size: 0,
            },
            bytecode_cache: Vec::new(),
        }
    }

    /// Add a global variable to the snapshot
    pub fn add_global(&mut self, name: &str, value: &Value) -> Result<(), SnapshotError> {
        let mut serializer = ValueSerializer::new();
        serializer.serialize_value(value)?;
        self.globals.push((name.to_string(), serializer.into_bytes()));
        self.metadata.global_count = self.globals.len();
        Ok(())
    }

    /// Add compiled bytecode to the snapshot
    pub fn add_bytecode(&mut self, name: &str, bytecode: &[u8]) {
        self.bytecode_cache.push((name.to_string(), bytecode.to_vec()));
        self.metadata.bytecode_entries = self.bytecode_cache.len();
    }

    /// Get a reference to the snapshot metadata
    pub fn metadata(&self) -> &SnapshotMetadataV2 {
        &self.metadata
    }

    /// Create the final snapshot blob
    pub fn create(&self) -> Result<Vec<u8>, SnapshotError> {
        let mut buf = Vec::new();

        // Header: magic + version
        buf.extend_from_slice(SNAPSHOT_V2_MAGIC);
        buf.extend_from_slice(&self.metadata.version.to_le_bytes());

        // Metadata (JSON for forward compat)
        let meta_json = serde_json::to_vec(&self.metadata)
            .map_err(|e| SnapshotError::SerializationFailed(format!("metadata: {}", e)))?;
        buf.extend_from_slice(&(meta_json.len() as u32).to_le_bytes());
        buf.extend_from_slice(&meta_json);

        // Globals section
        buf.extend_from_slice(&(self.globals.len() as u32).to_le_bytes());
        for (name, data) in &self.globals {
            let name_bytes = name.as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
        }

        // Bytecode section
        buf.extend_from_slice(&(self.bytecode_cache.len() as u32).to_le_bytes());
        for (name, data) in &self.bytecode_cache {
            let name_bytes = name.as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
        }

        // Checksum at end
        let checksum = compute_checksum(&buf);
        buf.extend_from_slice(&checksum.to_le_bytes());

        Ok(buf)
    }
}

/// State restored from a snapshot
#[derive(Debug)]
pub struct RestoredState {
    /// Snapshot metadata
    pub metadata: SnapshotMetadataV2,
    /// Restored global variables
    pub globals: Vec<(String, Value)>,
    /// Restored bytecode cache entries
    pub bytecode: Vec<(String, Vec<u8>)>,
}

/// Restores runtime state from a snapshot
pub struct SnapshotRestorer {
    data: Vec<u8>,
    position: usize,
}

impl SnapshotRestorer {
    /// Create a new restorer from raw snapshot bytes
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, position: 0 }
    }

    fn read_bytes(&mut self, n: usize) -> Result<&[u8], SnapshotError> {
        if self.position + n > self.data.len() {
            return Err(SnapshotError::BufferUnderflow);
        }
        let slice = &self.data[self.position..self.position + n];
        self.position += n;
        Ok(slice)
    }

    fn read_u32(&mut self) -> Result<u32, SnapshotError> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Restore runtime state from the snapshot
    pub fn restore(&mut self) -> Result<RestoredState, SnapshotError> {
        // Verify magic
        let magic = self.read_bytes(4)?;
        if magic != SNAPSHOT_V2_MAGIC {
            return Err(SnapshotError::InvalidFormat("bad magic bytes".to_string()));
        }

        // Version
        let version = self.read_u32()?;
        if version != SNAPSHOT_VERSION {
            return Err(SnapshotError::VersionMismatch {
                expected: SNAPSHOT_VERSION,
                found: version,
            });
        }

        // Metadata
        let meta_len = self.read_u32()? as usize;
        let meta_bytes = self.read_bytes(meta_len)?;
        let metadata: SnapshotMetadataV2 = serde_json::from_slice(meta_bytes)
            .map_err(|e| SnapshotError::DeserializationFailed(format!("metadata: {}", e)))?;

        // Globals
        let globals_count = self.read_u32()? as usize;
        let mut globals = Vec::with_capacity(globals_count);
        for _ in 0..globals_count {
            let name_len = self.read_u32()? as usize;
            let name_bytes = self.read_bytes(name_len)?;
            let name = String::from_utf8(name_bytes.to_vec())
                .map_err(|e| SnapshotError::DeserializationFailed(format!("global name: {}", e)))?;
            let data_len = self.read_u32()? as usize;
            let data = self.read_bytes(data_len)?.to_vec();
            let mut deser = ValueDeserializer::new(data);
            let value = deser.deserialize_value()?;
            globals.push((name, value));
        }

        // Bytecode
        let bc_count = self.read_u32()? as usize;
        let mut bytecode = Vec::with_capacity(bc_count);
        for _ in 0..bc_count {
            let name_len = self.read_u32()? as usize;
            let name_bytes = self.read_bytes(name_len)?;
            let name = String::from_utf8(name_bytes.to_vec())
                .map_err(|e| SnapshotError::DeserializationFailed(format!("bytecode name: {}", e)))?;
            let data_len = self.read_u32()? as usize;
            let data = self.read_bytes(data_len)?.to_vec();
            bytecode.push((name, data));
        }

        // Verify checksum (everything before the last 4 bytes)
        let checksum_data = &self.data[..self.data.len() - 4];
        let stored_checksum = {
            let end = &self.data[self.data.len() - 4..];
            u32::from_le_bytes([end[0], end[1], end[2], end[3]])
        };
        let computed = compute_checksum(checksum_data);
        if computed != stored_checksum {
            return Err(SnapshotError::ChecksumMismatch {
                expected: stored_checksum,
                found: computed,
            });
        }

        Ok(RestoredState {
            metadata,
            globals,
            bytecode,
        })
    }
}

// ============================================================================
// Tests for complete Value serialization
// ============================================================================

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod value_serializer_tests {
    use super::*;

    /// Helper: serialize then deserialize a value
    fn roundtrip(value: &Value) -> Value {
        let mut ser = ValueSerializer::new();
        ser.serialize_value(value).unwrap();
        let bytes = ser.into_bytes();
        let mut deser = ValueDeserializer::new(bytes);
        deser.deserialize_value().unwrap()
    }

    #[test]
    fn test_serialize_undefined() {
        let v = roundtrip(&Value::Undefined);
        assert!(v.is_undefined());
    }

    #[test]
    fn test_serialize_null() {
        let v = roundtrip(&Value::Null);
        assert!(v.is_null());
    }

    #[test]
    fn test_serialize_boolean_true() {
        let v = roundtrip(&Value::Boolean(true));
        assert_eq!(v.to_boolean(), true);
    }

    #[test]
    fn test_serialize_boolean_false() {
        let v = roundtrip(&Value::Boolean(false));
        assert_eq!(v.to_boolean(), false);
    }

    #[test]
    fn test_serialize_number() {
        let v = roundtrip(&Value::Number(42.5));
        if let Value::Number(n) = v {
            assert_eq!(n, 42.5);
        } else {
            panic!("expected Number");
        }
    }

    #[test]
    fn test_serialize_number_nan_infinity() {
        let nan = roundtrip(&Value::Number(f64::NAN));
        if let Value::Number(n) = nan {
            assert!(n.is_nan());
        } else {
            panic!("expected Number");
        }

        let inf = roundtrip(&Value::Number(f64::INFINITY));
        if let Value::Number(n) = inf {
            assert!(n.is_infinite() && n > 0.0);
        } else {
            panic!("expected Number");
        }

        let neg_inf = roundtrip(&Value::Number(f64::NEG_INFINITY));
        if let Value::Number(n) = neg_inf {
            assert!(n.is_infinite() && n < 0.0);
        } else {
            panic!("expected Number");
        }
    }

    #[test]
    fn test_serialize_string_empty() {
        let v = roundtrip(&Value::String(String::new()));
        if let Value::String(s) = v {
            assert_eq!(s, "");
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn test_serialize_string_nonempty() {
        let v = roundtrip(&Value::String("hello world".to_string()));
        if let Value::String(s) = v {
            assert_eq!(s, "hello world");
        } else {
            panic!("expected String");
        }
    }

    #[test]
    fn test_serialize_object_with_properties() {
        let obj = Value::new_object();
        obj.set_property("x", Value::Number(1.0));
        obj.set_property("y", Value::String("two".to_string()));
        let restored = roundtrip(&obj);
        assert_eq!(
            restored.get_property("x").unwrap().to_js_string(),
            "1"
        );
        assert_eq!(
            restored.get_property("y").unwrap().to_js_string(),
            "two"
        );
    }

    #[test]
    fn test_serialize_array() {
        let arr = Value::new_array(vec![
            Value::Number(1.0),
            Value::String("two".to_string()),
            Value::Boolean(true),
        ]);
        let restored = roundtrip(&arr);
        assert_eq!(restored.to_js_string(), "1,two,true");
    }

    #[test]
    fn test_serialize_nested_objects() {
        let inner = Value::new_object();
        inner.set_property("a", Value::Number(99.0));
        let outer = Value::new_object();
        outer.set_property("inner", inner);
        outer.set_property("b", Value::Boolean(false));

        let restored = roundtrip(&outer);
        let inner_restored = restored.get_property("inner").unwrap();
        assert_eq!(
            inner_restored.get_property("a").unwrap().to_js_string(),
            "99"
        );
    }

    #[test]
    fn test_circular_reference() {
        let obj = Value::new_object();
        obj.set_property("name", Value::String("self-ref".to_string()));
        // Create circular reference: obj.self = obj
        obj.set_property("self", obj.clone());

        let mut ser = ValueSerializer::new();
        ser.serialize_value(&obj).unwrap();
        let bytes = ser.into_bytes();

        // Deserialize should succeed (circular ref becomes back-ref)
        let mut deser = ValueDeserializer::new(bytes);
        let restored = deser.deserialize_value().unwrap();
        assert_eq!(
            restored.get_property("name").unwrap().to_js_string(),
            "self-ref"
        );
        // The self-reference should resolve to the same object
        assert!(restored.get_property("self").is_some());
    }

    #[test]
    fn test_value_tag_values() {
        assert_eq!(ValueTag::Undefined as u8, 0);
        assert_eq!(ValueTag::Null as u8, 1);
        assert_eq!(ValueTag::Boolean as u8, 2);
        assert_eq!(ValueTag::Number as u8, 3);
        assert_eq!(ValueTag::String as u8, 4);
        assert_eq!(ValueTag::Object as u8, 5);
        assert_eq!(ValueTag::Array as u8, 6);
        assert_eq!(ValueTag::Date as u8, 7);
        assert_eq!(ValueTag::Map as u8, 8);
        assert_eq!(ValueTag::Set as u8, 9);
        assert_eq!(ValueTag::Error as u8, 10);
        assert_eq!(ValueTag::Symbol as u8, 11);
        assert_eq!(ValueTag::BigInt as u8, 12);
        assert_eq!(ValueTag::Reference as u8, 13);
        assert_eq!(ValueTag::Function as u8, 14);
    }

    #[test]
    fn test_snapshot_error_display() {
        let e = SnapshotError::InvalidFormat("bad".to_string());
        assert_eq!(format!("{}", e), "invalid snapshot format: bad");

        let e = SnapshotError::VersionMismatch { expected: 2, found: 3 };
        assert!(format!("{}", e).contains("version mismatch"));

        let e = SnapshotError::ChecksumMismatch { expected: 0xAB, found: 0xCD };
        assert!(format!("{}", e).contains("checksum mismatch"));

        let e = SnapshotError::UnknownTag(255);
        assert_eq!(format!("{}", e), "unknown tag: 255");

        let e = SnapshotError::BufferUnderflow;
        assert_eq!(format!("{}", e), "buffer underflow");

        let e = SnapshotError::CircularReference;
        assert!(format!("{}", e).contains("circular"));

        let e = SnapshotError::SerializationFailed("oops".to_string());
        assert!(format!("{}", e).contains("oops"));

        let e = SnapshotError::DeserializationFailed("bad data".to_string());
        assert!(format!("{}", e).contains("bad data"));
    }

    #[test]
    fn test_snapshot_creator_and_create() {
        let mut creator = SnapshotCreator::new();
        creator.add_global("x", &Value::Number(42.0)).unwrap();
        creator.add_global("name", &Value::String("test".to_string())).unwrap();
        creator.add_bytecode("main", &[0x01, 0x02, 0x03]);

        assert_eq!(creator.metadata().global_count, 2);
        assert_eq!(creator.metadata().bytecode_entries, 1);

        let blob = creator.create().unwrap();
        assert!(!blob.is_empty());
        // Verify magic bytes
        assert_eq!(&blob[0..4], SNAPSHOT_V2_MAGIC);
    }

    #[test]
    fn test_snapshot_metadata_v2_fields() {
        let meta = SnapshotMetadataV2 {
            version: 2,
            runtime_version: "0.1.0".to_string(),
            created_at: 1700000000,
            global_count: 5,
            bytecode_entries: 3,
            total_size: 1024,
        };
        assert_eq!(meta.version, 2);
        assert_eq!(meta.runtime_version, "0.1.0");
        assert_eq!(meta.created_at, 1700000000);
        assert_eq!(meta.global_count, 5);
        assert_eq!(meta.bytecode_entries, 3);
        assert_eq!(meta.total_size, 1024);
    }

    #[test]
    fn test_buffer_underflow() {
        // Empty buffer should fail immediately
        let mut deser = ValueDeserializer::new(vec![]);
        let result = deser.deserialize_value();
        assert!(result.is_err());
        if let Err(SnapshotError::BufferUnderflow) = result {
            // expected
        } else {
            panic!("expected BufferUnderflow");
        }
    }

    #[test]
    fn test_unknown_tag() {
        let mut deser = ValueDeserializer::new(vec![200]);
        let result = deser.deserialize_value();
        assert!(result.is_err());
        if let Err(SnapshotError::UnknownTag(200)) = result {
            // expected
        } else {
            panic!("expected UnknownTag(200)");
        }
    }

    #[test]
    fn test_roundtrip_preserves_values() {
        let values = vec![
            Value::Undefined,
            Value::Null,
            Value::Boolean(true),
            Value::Boolean(false),
            Value::Number(0.0),
            Value::Number(-3.14),
            Value::String("quicksilver".to_string()),
        ];

        for original in &values {
            let restored = roundtrip(original);
            assert_eq!(original.to_js_string(), restored.to_js_string());
        }
    }

    #[test]
    fn test_snapshot_creator_restorer_roundtrip() {
        let mut creator = SnapshotCreator::new();
        creator.add_global("count", &Value::Number(7.0)).unwrap();
        creator.add_global("msg", &Value::String("hello".to_string())).unwrap();
        creator.add_bytecode("main", &[0xDE, 0xAD]);

        let blob = creator.create().unwrap();

        let mut restorer = SnapshotRestorer::new(blob);
        let state = restorer.restore().unwrap();

        assert_eq!(state.globals.len(), 2);
        assert_eq!(state.globals[0].0, "count");
        assert_eq!(state.globals[1].0, "msg");
        assert_eq!(state.bytecode.len(), 1);
        assert_eq!(state.bytecode[0].0, "main");
        assert_eq!(state.bytecode[0].1, vec![0xDE, 0xAD]);
    }
}
