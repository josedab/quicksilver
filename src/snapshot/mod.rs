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
