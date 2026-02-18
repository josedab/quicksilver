//! Source Map V3 generation and consumption for Quicksilver
//!
//! Implements the [Source Map V3 specification](https://sourcemaps.info/spec.html)
//! for mapping generated JavaScript bytecode positions back to original source locations.
//!
//! # Features
//!
//! - **VLQ encoding/decoding**: Base64 VLQ codec for compact mapping representation
//! - **SourceMapBuilder**: Incremental construction of source maps
//! - **SourceMapConsumer**: Parse and query source maps for position lookups
//! - **MappedStackTrace**: Rewrite stack traces using source maps

use crate::error::{Error, Result, StackFrame};
use serde::{Deserialize, Serialize};

// Base64 VLQ alphabet
const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const VLQ_BASE_SHIFT: u8 = 5;
const VLQ_BASE: i64 = 1 << VLQ_BASE_SHIFT;
const VLQ_BASE_MASK: i64 = VLQ_BASE - 1;
const VLQ_CONTINUATION_BIT: i64 = VLQ_BASE;

fn base64_decode_char(c: u8) -> Option<i64> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as i64),
        b'a'..=b'z' => Some((c - b'a' + 26) as i64),
        b'0'..=b'9' => Some((c - b'0' + 52) as i64),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Encode an integer as a Base64 VLQ string.
///
/// The sign is encoded in bit 0, and the remaining value is encoded
/// in groups of 5 bits with bit 5 as a continuation flag.
pub fn vlq_encode(value: i64) -> String {
    let mut result = String::new();
    // Convert to VLQ signed representation: bit 0 is sign
    let mut vlq = if value < 0 {
        ((-value) << 1) + 1
    } else {
        value << 1
    };

    loop {
        let mut digit = vlq & VLQ_BASE_MASK;
        vlq >>= VLQ_BASE_SHIFT;
        if vlq > 0 {
            digit |= VLQ_CONTINUATION_BIT;
        }
        result.push(BASE64_CHARS[digit as usize] as char);
        if vlq == 0 {
            break;
        }
    }

    result
}

/// Decode a Base64 VLQ string into a sequence of integers.
pub fn vlq_decode(input: &str) -> Vec<i64> {
    let mut values = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let mut value: i64 = 0;
        let mut shift: u32 = 0;

        loop {
            if i >= bytes.len() {
                break;
            }
            let digit = match base64_decode_char(bytes[i]) {
                Some(d) => d,
                None => break,
            };
            i += 1;

            let digit_value = digit & VLQ_BASE_MASK;
            value += digit_value << shift;
            shift += VLQ_BASE_SHIFT as u32;

            if (digit & VLQ_CONTINUATION_BIT) == 0 {
                break;
            }
        }

        // Bit 0 is sign
        let is_negative = (value & 1) == 1;
        value >>= 1;
        if is_negative {
            value = -value;
        }
        values.push(value);
    }

    values
}

/// V3 source map representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMap {
    /// Source map version (always 3).
    pub version: u32,
    /// The generated file this source map is associated with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// An optional root for source URLs.
    #[serde(rename = "sourceRoot", skip_serializing_if = "Option::is_none")]
    pub source_root: Option<String>,
    /// Original source file names.
    pub sources: Vec<String>,
    /// Optional original source contents, parallel to `sources`.
    #[serde(rename = "sourcesContent", skip_serializing_if = "Option::is_none")]
    pub sources_content: Option<Vec<Option<String>>>,
    /// Symbol names referenced by mappings.
    pub names: Vec<String>,
    /// VLQ-encoded mappings string.
    pub mappings: String,
}

impl SourceMap {
    /// Serialize to JSON string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| Error::InternalError(format!("source map serialization failed: {}", e)))
    }

    /// Deserialize from JSON string.
    pub fn from_json(json: &str) -> Result<Self> {
        let map: SourceMap = serde_json::from_str(json)
            .map_err(|e| Error::InternalError(format!("source map parsing failed: {}", e)))?;
        if map.version != 3 {
            return Err(Error::InternalError(format!(
                "unsupported source map version: {}",
                map.version
            )));
        }
        Ok(map)
    }
}

/// A single source mapping entry.
#[derive(Debug, Clone)]
pub struct Mapping {
    /// 0-based line in the generated output.
    pub generated_line: u32,
    /// 0-based column in the generated output.
    pub generated_column: u32,
    /// Index into `sources` (if mapping to original source).
    pub source: Option<u32>,
    /// 0-based line in the original source.
    pub original_line: Option<u32>,
    /// 0-based column in the original source.
    pub original_column: Option<u32>,
    /// Index into `names` (if mapping a named symbol).
    pub name: Option<u32>,
}

/// Internal segment used during building.
#[derive(Debug, Clone)]
struct Segment {
    generated_column: u32,
    source: Option<u32>,
    original_line: Option<u32>,
    original_column: Option<u32>,
    name: Option<u32>,
}

/// Builder for incremental source map construction.
pub struct SourceMapBuilder {
    file: Option<String>,
    source_root: Option<String>,
    sources: Vec<String>,
    sources_content: Vec<Option<String>>,
    names: Vec<String>,
    segments: Vec<Vec<Segment>>,
}

impl SourceMapBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            file: None,
            source_root: None,
            sources: Vec::new(),
            sources_content: Vec::new(),
            names: Vec::new(),
            segments: Vec::new(),
        }
    }

    /// Set the generated file name.
    pub fn set_file(&mut self, file: &str) {
        self.file = Some(file.to_string());
    }

    /// Add a source file and return its index.
    pub fn add_source(&mut self, source: &str) -> u32 {
        if let Some(idx) = self.sources.iter().position(|s| s == source) {
            return idx as u32;
        }
        let idx = self.sources.len() as u32;
        self.sources.push(source.to_string());
        self.sources_content.push(None);
        idx
    }

    /// Set the content for a source at the given index.
    pub fn add_source_content(&mut self, idx: u32, content: &str) {
        let idx = idx as usize;
        if idx < self.sources_content.len() {
            self.sources_content[idx] = Some(content.to_string());
        }
    }

    /// Add a symbol name and return its index.
    pub fn add_name(&mut self, name: &str) -> u32 {
        if let Some(idx) = self.names.iter().position(|n| n == name) {
            return idx as u32;
        }
        let idx = self.names.len() as u32;
        self.names.push(name.to_string());
        idx
    }

    /// Add a mapping.
    pub fn add_mapping(&mut self, mapping: Mapping) {
        let line = mapping.generated_line as usize;
        // Ensure we have enough line entries
        while self.segments.len() <= line {
            self.segments.push(Vec::new());
        }
        self.segments[line].push(Segment {
            generated_column: mapping.generated_column,
            source: mapping.source,
            original_line: mapping.original_line,
            original_column: mapping.original_column,
            name: mapping.name,
        });
    }

    /// Consume the builder and produce a finished `SourceMap`.
    pub fn build(mut self) -> SourceMap {
        // Sort segments within each line by generated column
        for segs in &mut self.segments {
            segs.sort_by_key(|s| s.generated_column);
        }

        let mappings = self.encode_mappings();

        let sources_content = if self.sources_content.iter().any(|c| c.is_some()) {
            Some(self.sources_content)
        } else {
            None
        };

        SourceMap {
            version: 3,
            file: self.file,
            source_root: self.source_root,
            sources: self.sources,
            sources_content,
            names: self.names,
            mappings,
        }
    }

    fn encode_mappings(&self) -> String {
        let mut result = String::new();
        let mut prev_gen_col: i64;
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;
        let mut prev_name: i64 = 0;

        for (line_idx, line_segs) in self.segments.iter().enumerate() {
            if line_idx > 0 {
                result.push(';');
            }
            prev_gen_col = 0;

            for (seg_idx, seg) in line_segs.iter().enumerate() {
                if seg_idx > 0 {
                    result.push(',');
                }

                // Field 1: generated column (relative to previous in same line)
                let gen_col = seg.generated_column as i64;
                result.push_str(&vlq_encode(gen_col - prev_gen_col));
                prev_gen_col = gen_col;

                // Fields 2-4: source, original line, original column
                if let (Some(source), Some(orig_line), Some(orig_col)) =
                    (seg.source, seg.original_line, seg.original_column)
                {
                    let source = source as i64;
                    let orig_line = orig_line as i64;
                    let orig_col = orig_col as i64;

                    result.push_str(&vlq_encode(source - prev_source));
                    result.push_str(&vlq_encode(orig_line - prev_orig_line));
                    result.push_str(&vlq_encode(orig_col - prev_orig_col));
                    prev_source = source;
                    prev_orig_line = orig_line;
                    prev_orig_col = orig_col;

                    // Field 5: name (optional)
                    if let Some(name) = seg.name {
                        let name = name as i64;
                        result.push_str(&vlq_encode(name - prev_name));
                        prev_name = name;
                    }
                }
            }
        }

        result
    }
}

/// A decoded mapping with all fields resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedMapping {
    pub generated_line: u32,
    pub generated_column: u32,
    pub source: Option<u32>,
    pub original_line: Option<u32>,
    pub original_column: Option<u32>,
    pub name: Option<u32>,
}

/// Result of looking up the original position for a generated location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalPosition {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub name: Option<String>,
}

/// Result of looking up the generated position for an original location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedPosition {
    pub line: u32,
    pub column: u32,
}

/// Consumer that parses and queries source maps.
pub struct SourceMapConsumer {
    source_map: SourceMap,
    decoded_mappings: Vec<DecodedMapping>,
}

impl SourceMapConsumer {
    /// Create a consumer from a parsed `SourceMap`, decoding all mappings.
    pub fn new(source_map: SourceMap) -> Self {
        let decoded_mappings = Self::decode_mappings(&source_map.mappings);
        Self {
            source_map,
            decoded_mappings,
        }
    }

    /// Parse a JSON source map string and create a consumer.
    pub fn from_json(json: &str) -> Result<Self> {
        let source_map = SourceMap::from_json(json)?;
        Ok(Self::new(source_map))
    }

    /// Look up the original source position for a generated location (0-based).
    pub fn original_position_for(&self, line: u32, column: u32) -> Option<OriginalPosition> {
        // Find the best matching mapping: same line, closest column <= requested
        let mut best: Option<&DecodedMapping> = None;
        for m in &self.decoded_mappings {
            if m.generated_line == line && m.generated_column <= column && m.source.is_some() {
                match best {
                    Some(b) if m.generated_column > b.generated_column => best = Some(m),
                    None => best = Some(m),
                    _ => {}
                }
            }
        }
        best.and_then(|m| {
            let source_idx = m.source? as usize;
            let source = self.source_map.sources.get(source_idx)?.clone();
            Some(OriginalPosition {
                source,
                line: m.original_line?,
                column: m.original_column?,
                name: m.name.and_then(|n| self.source_map.names.get(n as usize).cloned()),
            })
        })
    }

    /// Look up the generated position for an original source location (0-based).
    pub fn generated_position_for(
        &self,
        source: &str,
        line: u32,
        column: u32,
    ) -> Option<GeneratedPosition> {
        let source_idx = self.source_map.sources.iter().position(|s| s == source)? as u32;
        let mut best: Option<&DecodedMapping> = None;
        for m in &self.decoded_mappings {
            if m.source == Some(source_idx)
                && m.original_line == Some(line)
                && m.original_column.is_some_and(|c| c <= column)
            {
                match best {
                    Some(b) if m.original_column > b.original_column => best = Some(m),
                    None => best = Some(m),
                    _ => {}
                }
            }
        }
        best.map(|m| GeneratedPosition {
            line: m.generated_line,
            column: m.generated_column,
        })
    }

    /// Return all decoded mappings.
    pub fn all_mappings(&self) -> &[DecodedMapping] {
        &self.decoded_mappings
    }

    fn decode_mappings(mappings: &str) -> Vec<DecodedMapping> {
        let mut result = Vec::new();
        let mut gen_line: u32 = 0;
        let mut prev_gen_col: i64;
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;
        let mut prev_name: i64 = 0;

        for line_str in mappings.split(';') {
            prev_gen_col = 0;
            if !line_str.is_empty() {
                for segment_str in line_str.split(',') {
                    if segment_str.is_empty() {
                        continue;
                    }
                    let fields = vlq_decode(segment_str);
                    if fields.is_empty() {
                        continue;
                    }

                    prev_gen_col += fields[0];
                    let gen_col = prev_gen_col as u32;

                    let (source, orig_line, orig_col, name) = if fields.len() >= 4 {
                        prev_source += fields[1];
                        prev_orig_line += fields[2];
                        prev_orig_col += fields[3];

                        let name = if fields.len() >= 5 {
                            prev_name += fields[4];
                            Some(prev_name as u32)
                        } else {
                            None
                        };

                        (
                            Some(prev_source as u32),
                            Some(prev_orig_line as u32),
                            Some(prev_orig_col as u32),
                            name,
                        )
                    } else {
                        (None, None, None, None)
                    };

                    result.push(DecodedMapping {
                        generated_line: gen_line,
                        generated_column: gen_col,
                        source,
                        original_line: orig_line,
                        original_column: orig_col,
                        name,
                    });
                }
            }
            gen_line += 1;
        }

        result
    }
}

/// A single frame in a mapped stack trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedFrame {
    /// Function name (may be remapped via source map names).
    pub function_name: String,
    /// Original source file.
    pub source: Option<String>,
    /// Original line (0-based).
    pub line: u32,
    /// Original column (0-based).
    pub column: u32,
}

/// Utility for remapping stack traces through source maps.
pub struct MappedStackTrace;

impl MappedStackTrace {
    /// Remap a slice of `StackFrame`s using one or more `SourceMapConsumer`s.
    ///
    /// For each frame, every consumer is tried in order. The first match wins.
    /// Frames with no match are passed through with their original positions
    /// (converted from 1-based to 0-based).
    pub fn remap(frames: &[StackFrame], maps: &[SourceMapConsumer]) -> Vec<MappedFrame> {
        frames
            .iter()
            .map(|frame| {
                // StackFrame uses 1-based lines/columns; source maps use 0-based
                let line_0 = frame.line.saturating_sub(1);
                let col_0 = frame.column.saturating_sub(1);

                for consumer in maps {
                    if let Some(pos) = consumer.original_position_for(line_0, col_0) {
                        return MappedFrame {
                            function_name: pos.name.unwrap_or_else(|| frame.function_name.clone()),
                            source: Some(pos.source),
                            line: pos.line,
                            column: pos.column,
                        };
                    }
                }

                // No match — pass through
                MappedFrame {
                    function_name: frame.function_name.clone(),
                    source: frame.file_name.clone(),
                    line: line_0,
                    column: col_0,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── VLQ encode/decode ──────────────────────────────────────────────

    #[test]
    fn vlq_encode_zero() {
        assert_eq!(vlq_encode(0), "A");
    }

    #[test]
    fn vlq_encode_positive_small() {
        // 1 => signed representation: 1<<1 = 2 => base64[2] = 'C'
        assert_eq!(vlq_encode(1), "C");
    }

    #[test]
    fn vlq_encode_negative() {
        // -1 => signed: (1<<1)+1 = 3 => base64[3] = 'D'
        assert_eq!(vlq_encode(-1), "D");
    }

    #[test]
    fn vlq_encode_large_value() {
        // 16 => signed: 32 => needs continuation: first digit = 32|0 = 'g' (index 32+cont), second digit = 1 = 'B'
        let encoded = vlq_encode(16);
        let decoded = vlq_decode(&encoded);
        assert_eq!(decoded, vec![16]);
    }

    #[test]
    fn vlq_roundtrip_values() {
        let values = vec![0, 1, -1, 5, -5, 15, -15, 100, -100, 1000, -1000];
        for v in &values {
            let encoded = vlq_encode(*v);
            let decoded = vlq_decode(&encoded);
            assert_eq!(decoded, vec![*v], "roundtrip failed for {}", v);
        }
    }

    #[test]
    fn vlq_decode_multiple_values() {
        // Encode several values concatenated, decode them all back
        let values = vec![0, 5, 10, -3];
        let encoded: String = values.iter().map(|v| vlq_encode(*v)).collect();
        let decoded = vlq_decode(&encoded);
        assert_eq!(decoded, values);
    }

    // ── Builder / SourceMap ────────────────────────────────────────────

    #[test]
    fn builder_basic() {
        let mut builder = SourceMapBuilder::new();
        builder.set_file("out.js");
        let src = builder.add_source("input.js");
        builder.add_source_content(src, "var x = 1;");

        builder.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source: Some(src),
            original_line: Some(0),
            original_column: Some(0),
            name: None,
        });

        let map = builder.build();
        assert_eq!(map.version, 3);
        assert_eq!(map.file.as_deref(), Some("out.js"));
        assert_eq!(map.sources, vec!["input.js"]);
        assert!(map.sources_content.is_some());
        assert!(!map.mappings.is_empty());
    }

    #[test]
    fn builder_deduplicates_sources_and_names() {
        let mut builder = SourceMapBuilder::new();
        let s1 = builder.add_source("a.js");
        let s2 = builder.add_source("a.js");
        assert_eq!(s1, s2);

        let n1 = builder.add_name("foo");
        let n2 = builder.add_name("foo");
        assert_eq!(n1, n2);
    }

    #[test]
    fn builder_multiple_lines() {
        let mut builder = SourceMapBuilder::new();
        let src = builder.add_source("app.js");

        for line in 0..3 {
            builder.add_mapping(Mapping {
                generated_line: line,
                generated_column: 0,
                source: Some(src),
                original_line: Some(line),
                original_column: Some(0),
                name: None,
            });
        }

        let map = builder.build();
        // Should have semicolons separating lines
        assert_eq!(map.mappings.matches(';').count(), 2);
    }

    // ── Consumer / position lookup ─────────────────────────────────────

    #[test]
    fn consumer_roundtrip() {
        let mut builder = SourceMapBuilder::new();
        let src = builder.add_source("test.js");
        let name = builder.add_name("myFunc");

        builder.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 4,
            source: Some(src),
            original_line: Some(2),
            original_column: Some(8),
            name: Some(name),
        });

        let map = builder.build();
        let consumer = SourceMapConsumer::new(map);

        let pos = consumer.original_position_for(0, 4).unwrap();
        assert_eq!(pos.source, "test.js");
        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 8);
        assert_eq!(pos.name.as_deref(), Some("myFunc"));
    }

    #[test]
    fn consumer_generated_position_for() {
        let mut builder = SourceMapBuilder::new();
        let src = builder.add_source("lib.js");

        builder.add_mapping(Mapping {
            generated_line: 5,
            generated_column: 10,
            source: Some(src),
            original_line: Some(3),
            original_column: Some(0),
            name: None,
        });

        let map = builder.build();
        let consumer = SourceMapConsumer::new(map);

        let gen = consumer.generated_position_for("lib.js", 3, 0).unwrap();
        assert_eq!(gen.line, 5);
        assert_eq!(gen.column, 10);
    }

    #[test]
    fn consumer_no_match_returns_none() {
        let mut builder = SourceMapBuilder::new();
        let src = builder.add_source("x.js");
        builder.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source: Some(src),
            original_line: Some(0),
            original_column: Some(0),
            name: None,
        });

        let map = builder.build();
        let consumer = SourceMapConsumer::new(map);

        assert!(consumer.original_position_for(99, 0).is_none());
        assert!(consumer.generated_position_for("nonexistent.js", 0, 0).is_none());
    }

    #[test]
    fn consumer_all_mappings() {
        let mut builder = SourceMapBuilder::new();
        let src = builder.add_source("a.js");
        builder.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source: Some(src),
            original_line: Some(0),
            original_column: Some(0),
            name: None,
        });
        builder.add_mapping(Mapping {
            generated_line: 1,
            generated_column: 0,
            source: Some(src),
            original_line: Some(1),
            original_column: Some(0),
            name: None,
        });

        let map = builder.build();
        let consumer = SourceMapConsumer::new(map);
        assert_eq!(consumer.all_mappings().len(), 2);
    }

    // ── JSON serialization ─────────────────────────────────────────────

    #[test]
    fn json_roundtrip() {
        let mut builder = SourceMapBuilder::new();
        builder.set_file("bundle.js");
        let src = builder.add_source("index.js");
        builder.add_source_content(src, "console.log('hi');");
        let name = builder.add_name("log");

        builder.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source: Some(src),
            original_line: Some(0),
            original_column: Some(0),
            name: Some(name),
        });

        let map = builder.build();
        let json = map.to_json().unwrap();
        let consumer = SourceMapConsumer::from_json(&json).unwrap();

        let pos = consumer.original_position_for(0, 0).unwrap();
        assert_eq!(pos.source, "index.js");
        assert_eq!(pos.name.as_deref(), Some("log"));
    }

    #[test]
    fn from_json_rejects_wrong_version() {
        let json = r#"{"version":2,"sources":[],"names":[],"mappings":""}"#;
        assert!(SourceMap::from_json(json).is_err());
    }

    // ── Stack trace remapping ──────────────────────────────────────────

    #[test]
    fn remap_stack_trace() {
        let mut builder = SourceMapBuilder::new();
        let src = builder.add_source("original.ts");
        let name = builder.add_name("handleClick");

        builder.add_mapping(Mapping {
            generated_line: 9,
            generated_column: 4,
            source: Some(src),
            original_line: Some(20),
            original_column: Some(2),
            name: Some(name),
        });

        let map = builder.build();
        let consumer = SourceMapConsumer::new(map);

        // StackFrame uses 1-based values
        let frames = vec![StackFrame::new("anonymous", 10, 5).with_file("bundle.js")];
        let mapped = MappedStackTrace::remap(&frames, &[consumer]);

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].function_name, "handleClick");
        assert_eq!(mapped[0].source.as_deref(), Some("original.ts"));
        assert_eq!(mapped[0].line, 20);
        assert_eq!(mapped[0].column, 2);
    }

    #[test]
    fn remap_stack_trace_no_match() {
        let builder = SourceMapBuilder::new();
        let map = builder.build();
        let consumer = SourceMapConsumer::new(map);

        let frames = vec![StackFrame::new("foo", 1, 1)];
        let mapped = MappedStackTrace::remap(&frames, &[consumer]);

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].function_name, "foo");
        assert_eq!(mapped[0].line, 0);
        assert_eq!(mapped[0].column, 0);
    }
}
