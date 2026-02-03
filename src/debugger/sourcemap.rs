//! Source Map Support
//!
//! Implements the Source Map v3 specification for mapping compiled/minified
//! JavaScript back to original source locations. Essential for IDE debugging.
//!
//! # Spec Reference
//! https://sourcemaps.info/spec.html

use rustc_hash::FxHashMap as HashMap;
use std::path::Path;

/// A parsed source map (v3 format)
#[derive(Debug, Clone)]
pub struct SourceMap {
    /// Source map version (always 3)
    pub version: u32,
    /// Generated file name
    pub file: Option<String>,
    /// Source root prefix
    pub source_root: Option<String>,
    /// Original source file names
    pub sources: Vec<String>,
    /// Optional source contents (for inline source maps)
    pub sources_content: Vec<Option<String>>,
    /// Symbolic names referenced in mappings
    pub names: Vec<String>,
    /// Decoded mappings: generated_line → Vec<Mapping>
    pub mappings: Vec<Vec<Mapping>>,
}

/// A single mapping entry
#[derive(Debug, Clone, Copy)]
pub struct Mapping {
    /// Column in the generated file
    pub generated_column: u32,
    /// Index into `sources`
    pub source_index: u32,
    /// Line in the original source (0-based)
    pub original_line: u32,
    /// Column in the original source (0-based)
    pub original_column: u32,
    /// Optional index into `names`
    pub name_index: Option<u32>,
}

/// Result of looking up a generated position
#[derive(Debug, Clone)]
pub struct OriginalPosition {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub name: Option<String>,
}

/// Result of looking up an original position
#[derive(Debug, Clone)]
pub struct GeneratedPosition {
    pub line: u32,
    pub column: u32,
}

impl SourceMap {
    /// Parse a source map from JSON
    pub fn from_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| format!("Invalid source map JSON: {}", e))?;

        let version = value.get("version")
            .and_then(|v| v.as_u64())
            .ok_or("Missing version field")? as u32;

        if version != 3 {
            return Err(format!("Unsupported source map version: {}", version));
        }

        let file = value.get("file").and_then(|v| v.as_str()).map(String::from);
        let source_root = value.get("sourceRoot").and_then(|v| v.as_str()).map(String::from);

        let sources = value.get("sources")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let sources_content = value.get("sourcesContent")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let names = value.get("names")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let mappings_str = value.get("mappings")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mappings = Self::decode_mappings(mappings_str)?;

        Ok(Self {
            version,
            file,
            source_root,
            sources,
            sources_content,
            names,
            mappings,
        })
    }

    /// Decode VLQ-encoded mappings string
    fn decode_mappings(mappings: &str) -> Result<Vec<Vec<Mapping>>, String> {
        let mut result = Vec::new();
        let mut source_index: i64 = 0;
        let mut original_line: i64 = 0;
        let mut original_column: i64 = 0;
        let mut name_index: i64 = 0;

        for line in mappings.split(';') {
            let mut line_mappings = Vec::new();
            let mut generated_column: i64 = 0;

            if line.is_empty() {
                result.push(line_mappings);
                continue;
            }

            for segment in line.split(',') {
                if segment.is_empty() {
                    continue;
                }
                let values = decode_vlq(segment)?;
                if values.is_empty() {
                    continue;
                }

                generated_column += values[0];

                if values.len() >= 4 {
                    source_index += values[1];
                    original_line += values[2];
                    original_column += values[3];

                    let name = if values.len() >= 5 {
                        name_index += values[4];
                        Some(name_index as u32)
                    } else {
                        None
                    };

                    line_mappings.push(Mapping {
                        generated_column: generated_column as u32,
                        source_index: source_index as u32,
                        original_line: original_line as u32,
                        original_column: original_column as u32,
                        name_index: name,
                    });
                }
            }
            result.push(line_mappings);
        }

        Ok(result)
    }

    /// Look up the original position for a generated position
    pub fn original_position_for(&self, line: u32, column: u32) -> Option<OriginalPosition> {
        let line_mappings = self.mappings.get(line as usize)?;

        // Binary search for the closest column
        let mapping = line_mappings.iter()
            .rev()
            .find(|m| m.generated_column <= column)?;

        let source = self.sources.get(mapping.source_index as usize)?.clone();
        let source = if let Some(ref root) = self.source_root {
            format!("{}{}", root, source)
        } else {
            source
        };

        let name = mapping.name_index
            .and_then(|idx| self.names.get(idx as usize))
            .cloned();

        Some(OriginalPosition {
            source,
            line: mapping.original_line,
            column: mapping.original_column,
            name,
        })
    }

    /// Look up the generated position for an original position
    pub fn generated_position_for(&self, source: &str, line: u32, column: u32) -> Option<GeneratedPosition> {
        let source_index = self.sources.iter().position(|s| s == source)? as u32;

        for (gen_line, line_mappings) in self.mappings.iter().enumerate() {
            for mapping in line_mappings {
                if mapping.source_index == source_index
                    && mapping.original_line == line
                    && mapping.original_column <= column
                {
                    return Some(GeneratedPosition {
                        line: gen_line as u32,
                        column: mapping.generated_column,
                    });
                }
            }
        }
        None
    }

    /// Get the original source content for a source file
    pub fn source_content(&self, source_index: usize) -> Option<&str> {
        self.sources_content.get(source_index)?.as_deref()
    }

    /// Load a source map from a file
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read source map: {}", e))?;
        Self::from_json(&content)
    }

    /// Try to detect and load a source map from a JS file's `//# sourceMappingURL=` comment
    pub fn from_js_file(js_path: &Path) -> Result<Option<Self>, String> {
        let content = std::fs::read_to_string(js_path)
            .map_err(|e| format!("Failed to read JS file: {}", e))?;

        // Look for sourceMappingURL
        for line in content.lines().rev() {
            let trimmed = line.trim();
            if let Some(url) = trimmed.strip_prefix("//# sourceMappingURL=") {
                if url.starts_with("data:application/json;base64,") {
                    return Err("Inline base64 source maps not yet supported".to_string());
                } else {
                    // External source map file
                    let map_path = js_path.parent()
                        .unwrap_or(Path::new("."))
                        .join(url.trim());
                    return Self::from_file(&map_path).map(Some);
                }
            }
        }
        Ok(None)
    }
}

/// Source map registry for managing multiple source maps
#[derive(Debug, Default)]
pub struct SourceMapRegistry {
    maps: HashMap<String, SourceMap>,
}

impl SourceMapRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a source map for a generated file
    pub fn register(&mut self, generated_file: String, map: SourceMap) {
        self.maps.insert(generated_file, map);
    }

    /// Look up original position across all registered maps
    pub fn original_position(&self, file: &str, line: u32, column: u32) -> Option<OriginalPosition> {
        self.maps.get(file)?.original_position_for(line, column)
    }

    /// Get the source map for a file
    pub fn get(&self, file: &str) -> Option<&SourceMap> {
        self.maps.get(file)
    }
}

// ==================== VLQ Decoding ====================

const VLQ_BASE_SHIFT: i64 = 5;
const VLQ_BASE: i64 = 1 << VLQ_BASE_SHIFT;
const VLQ_BASE_MASK: i64 = VLQ_BASE - 1;
const VLQ_CONTINUATION_BIT: i64 = VLQ_BASE;

/// Decode a base64-VLQ encoded string into a sequence of values
fn decode_vlq(input: &str) -> Result<Vec<i64>, String> {
    let mut result = Vec::new();
    let mut shift = 0;
    let mut value: i64 = 0;

    for ch in input.chars() {
        let digit = base64_char_to_int(ch)
            .ok_or_else(|| format!("Invalid VLQ character: {}", ch))? as i64;

        value += (digit & VLQ_BASE_MASK) << shift;

        if (digit & VLQ_CONTINUATION_BIT) != 0 {
            shift += VLQ_BASE_SHIFT;
        } else {
            // Sign is in the least significant bit
            let is_negative = (value & 1) != 0;
            let abs_value = value >> 1;
            result.push(if is_negative { -abs_value } else { abs_value });
            value = 0;
            shift = 0;
        }
    }

    Ok(result)
}

/// Encode values as VLQ string
#[allow(dead_code)]
fn encode_vlq(values: &[i64]) -> String {
    let mut result = String::new();
    for &value in values {
        let mut vlq = if value < 0 { (-value << 1) + 1 } else { value << 1 };
        loop {
            let digit = vlq & VLQ_BASE_MASK;
            vlq >>= VLQ_BASE_SHIFT;
            let ch = if vlq > 0 {
                int_to_base64_char((digit | VLQ_CONTINUATION_BIT) as u8)
            } else {
                int_to_base64_char(digit as u8)
            };
            if let Some(c) = ch {
                result.push(c);
            }
            if vlq == 0 { break; }
        }
    }
    result
}

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_char_to_int(ch: char) -> Option<u8> {
    BASE64_CHARS.iter().position(|&c| c == ch as u8).map(|i| i as u8)
}

#[allow(dead_code)]
fn int_to_base64_char(val: u8) -> Option<char> {
    BASE64_CHARS.get(val as usize).map(|&b| b as char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vlq_decode() {
        assert_eq!(decode_vlq("A").unwrap(), vec![0]);
        assert_eq!(decode_vlq("C").unwrap(), vec![1]);
        assert_eq!(decode_vlq("D").unwrap(), vec![-1]);
        assert_eq!(decode_vlq("AAAA").unwrap(), vec![0, 0, 0, 0]);
    }

    #[test]
    fn test_vlq_roundtrip() {
        let values = vec![0, 5, -3, 100, -50];
        let encoded = encode_vlq(&values);
        let decoded = decode_vlq(&encoded).unwrap();
        assert_eq!(values, decoded);
    }

    #[test]
    fn test_source_map_parse() {
        let json = r#"{
            "version": 3,
            "file": "out.js",
            "sourceRoot": "",
            "sources": ["input.ts"],
            "names": ["greet"],
            "mappings": "AAAA,SAASA;IACL,OAAO"
        }"#;

        let map = SourceMap::from_json(json).unwrap();
        assert_eq!(map.version, 3);
        assert_eq!(map.file, Some("out.js".to_string()));
        assert_eq!(map.sources, vec!["input.ts"]);
        assert_eq!(map.names, vec!["greet"]);
        assert!(!map.mappings.is_empty());
    }

    #[test]
    fn test_source_map_empty_mappings() {
        let json = r#"{"version":3,"sources":[],"names":[],"mappings":""}"#;
        let map = SourceMap::from_json(json).unwrap();
        assert!(map.sources.is_empty());
    }

    #[test]
    fn test_source_map_lookup() {
        let json = r#"{
            "version": 3,
            "sources": ["app.ts"],
            "names": [],
            "mappings": "AAAA;AACA"
        }"#;
        let map = SourceMap::from_json(json).unwrap();

        // Line 0, column 0 should map to app.ts
        if let Some(pos) = map.original_position_for(0, 0) {
            assert_eq!(pos.source, "app.ts");
            assert_eq!(pos.line, 0);
        }
    }

    #[test]
    fn test_source_map_with_source_root() {
        let json = r#"{
            "version": 3,
            "sourceRoot": "src/",
            "sources": ["app.ts"],
            "names": [],
            "mappings": "AAAA"
        }"#;
        let map = SourceMap::from_json(json).unwrap();
        if let Some(pos) = map.original_position_for(0, 0) {
            assert_eq!(pos.source, "src/app.ts");
        }
    }

    #[test]
    fn test_source_map_registry() {
        let json = r#"{"version":3,"sources":["src.js"],"names":[],"mappings":"AAAA"}"#;
        let map = SourceMap::from_json(json).unwrap();

        let mut registry = SourceMapRegistry::new();
        registry.register("bundle.js".to_string(), map);

        assert!(registry.get("bundle.js").is_some());
        assert!(registry.get("other.js").is_none());
    }

    #[test]
    fn test_invalid_version() {
        let json = r#"{"version":2,"sources":[],"names":[],"mappings":""}"#;
        assert!(SourceMap::from_json(json).is_err());
    }

    #[test]
    fn test_base64_roundtrip() {
        for i in 0..64u8 {
            let ch = int_to_base64_char(i).unwrap();
            assert_eq!(base64_char_to_int(ch), Some(i));
        }
    }
}
