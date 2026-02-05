//! Source Map V3 encoding/decoding and rich error diagnostics.
//!
//! Provides a complete Source Map V3 implementation with VLQ encoding,
//! plus Rust-style diagnostic rendering for errors and warnings.

//! **Status:** ⚠️ Partial — Language server diagnostics

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// VLQ Encoder / Decoder
// ---------------------------------------------------------------------------

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_decode_char(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Base64 VLQ encoder/decoder for source maps.
pub struct VlqEncoder;

impl VlqEncoder {
    /// Encode a signed integer as a Base64 VLQ string.
    pub fn encode_vlq(value: i64) -> String {
        let mut vlq = if value < 0 {
            ((-value) << 1) | 1
        } else {
            value << 1
        } as u64;

        let mut result = String::new();
        loop {
            let mut digit = (vlq & 0x1f) as u8;
            vlq >>= 5;
            if vlq > 0 {
                digit |= 0x20; // continuation bit
            }
            result.push(BASE64_CHARS[digit as usize] as char);
            if vlq == 0 {
                break;
            }
        }
        result
    }

    /// Decode a Base64 VLQ string into a vector of signed integers.
    pub fn decode_vlq(input: &str) -> Vec<i64> {
        let mut values = Vec::new();
        let mut shift = 0u32;
        let mut accum: u64 = 0;

        for byte in input.bytes() {
            let Some(digit) = base64_decode_char(byte) else {
                break;
            };
            let has_continuation = (digit & 0x20) != 0;
            let value_bits = (digit & 0x1f) as u64;
            accum |= value_bits << shift;
            shift += 5;

            if !has_continuation {
                let is_negative = (accum & 1) == 1;
                let magnitude = (accum >> 1) as i64;
                values.push(if is_negative { -magnitude } else { magnitude });
                accum = 0;
                shift = 0;
            }
        }
        values
    }
}

// ---------------------------------------------------------------------------
// Original Position
// ---------------------------------------------------------------------------

/// Result of a source map lookup – the original position in source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalPosition {
    /// Original source file path.
    pub source: String,
    /// Original line (0-indexed).
    pub line: u32,
    /// Original column (0-indexed).
    pub column: u32,
    /// Original symbol name, if any.
    pub name: Option<String>,
}

// ---------------------------------------------------------------------------
// Source Map V3
// ---------------------------------------------------------------------------

/// A Source Map V3 encoder/decoder.
#[derive(Debug, Clone)]
pub struct SourceMapV3 {
    /// Version number (always 3).
    pub version: u32,
    /// Name of the generated file.
    pub file: Option<String>,
    /// Source root prefix.
    pub source_root: Option<String>,
    /// Source file paths.
    pub sources: Vec<String>,
    /// Optional inline source contents.
    pub sources_content: Vec<Option<String>>,
    /// Symbol names referenced by mappings.
    pub names: Vec<String>,
    /// VLQ-encoded mappings string.
    pub mappings: String,
    /// Decoded mapping segments for lookup.
    decoded_segments: Vec<DecodedSegment>,
}

#[derive(Debug, Clone)]
struct DecodedSegment {
    gen_line: u32,
    gen_column: u32,
    source_idx: Option<u32>,
    orig_line: Option<u32>,
    orig_column: Option<u32>,
    name_idx: Option<u32>,
}

impl SourceMapV3 {
    /// Create a new, empty source map.
    pub fn new() -> Self {
        Self {
            version: 3,
            file: None,
            source_root: None,
            sources: Vec::new(),
            sources_content: Vec::new(),
            names: Vec::new(),
            mappings: String::new(),
            decoded_segments: Vec::new(),
        }
    }

    /// Add a source file and return its index.
    pub fn add_source(&mut self, path: &str) -> u32 {
        let idx = self.sources.len() as u32;
        self.sources.push(path.to_string());
        self.sources_content.push(None);
        idx
    }

    /// Add a name and return its index.
    pub fn add_name(&mut self, name: &str) -> u32 {
        let idx = self.names.len() as u32;
        self.names.push(name.to_string());
        idx
    }

    /// Add a mapping segment.
    pub fn add_mapping(
        &mut self,
        gen_line: u32,
        gen_column: u32,
        source_idx: u32,
        orig_line: u32,
        orig_column: u32,
        name_idx: Option<u32>,
    ) {
        self.decoded_segments.push(DecodedSegment {
            gen_line,
            gen_column,
            source_idx: Some(source_idx),
            orig_line: Some(orig_line),
            orig_column: Some(orig_column),
            name_idx,
        });
    }

    /// Encode the source map as a JSON string (Source Map V3 format).
    pub fn encode(&self) -> String {
        let mappings_str = self.encode_mappings();

        let sources_json: Vec<String> = self
            .sources
            .iter()
            .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();

        let names_json: Vec<String> = self
            .names
            .iter()
            .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();

        let sources_content_json: Vec<String> = self
            .sources_content
            .iter()
            .map(|opt| match opt {
                Some(s) => format!(
                    "\"{}\"",
                    s.replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\n', "\\n")
                ),
                None => "null".to_string(),
            })
            .collect();

        let mut json = String::from("{\n");
        json.push_str("  \"version\": 3,\n");
        if let Some(ref f) = self.file {
            json.push_str(&format!("  \"file\": \"{}\",\n", f));
        }
        if let Some(ref sr) = self.source_root {
            json.push_str(&format!("  \"sourceRoot\": \"{}\",\n", sr));
        }
        json.push_str(&format!("  \"sources\": [{}],\n", sources_json.join(", ")));
        json.push_str(&format!(
            "  \"sourcesContent\": [{}],\n",
            sources_content_json.join(", ")
        ));
        json.push_str(&format!("  \"names\": [{}],\n", names_json.join(", ")));
        json.push_str(&format!("  \"mappings\": \"{}\"\n", mappings_str));
        json.push('}');
        json
    }

    fn encode_mappings(&self) -> String {
        if self.decoded_segments.is_empty() {
            return String::new();
        }

        let mut sorted = self.decoded_segments.clone();
        sorted.sort_by(|a, b| a.gen_line.cmp(&b.gen_line).then(a.gen_column.cmp(&b.gen_column)));

        let mut result = String::new();
        let mut prev_gen_line: u32 = 0;
        let mut prev_gen_col: i64 = 0;
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;
        let mut prev_name: i64 = 0;
        let mut first_in_line = true;

        for seg in &sorted {
            // Emit semicolons for skipped lines
            while prev_gen_line < seg.gen_line {
                result.push(';');
                prev_gen_line += 1;
                prev_gen_col = 0;
                first_in_line = true;
            }

            if !first_in_line {
                result.push(',');
            }
            first_in_line = false;

            // Field 1: generated column (relative)
            let gen_col = seg.gen_column as i64;
            result.push_str(&VlqEncoder::encode_vlq(gen_col - prev_gen_col));
            prev_gen_col = gen_col;

            if let (Some(src), Some(ol), Some(oc)) =
                (seg.source_idx, seg.orig_line, seg.orig_column)
            {
                let src_i = src as i64;
                let ol_i = ol as i64;
                let oc_i = oc as i64;

                // Field 2: source index (relative)
                result.push_str(&VlqEncoder::encode_vlq(src_i - prev_source));
                prev_source = src_i;

                // Field 3: original line (relative)
                result.push_str(&VlqEncoder::encode_vlq(ol_i - prev_orig_line));
                prev_orig_line = ol_i;

                // Field 4: original column (relative)
                result.push_str(&VlqEncoder::encode_vlq(oc_i - prev_orig_col));
                prev_orig_col = oc_i;

                // Field 5: name index (relative, optional)
                if let Some(ni) = seg.name_idx {
                    let ni_i = ni as i64;
                    result.push_str(&VlqEncoder::encode_vlq(ni_i - prev_name));
                    prev_name = ni_i;
                }
            }
        }
        result
    }

    /// Decode a Source Map V3 JSON string.
    pub fn decode(json: &str) -> Result<SourceMapV3> {
        let parsed: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            Error::InternalError(format!("Invalid source map JSON: {}", e))
        })?;

        let version = parsed
            .get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as u32;

        if version != 3 {
            return Err(Error::InternalError(format!(
                "Unsupported source map version: {}",
                version
            )));
        }

        let file = parsed.get("file").and_then(|v| v.as_str()).map(String::from);
        let source_root = parsed
            .get("sourceRoot")
            .and_then(|v| v.as_str())
            .map(String::from);

        let sources: Vec<String> = parsed
            .get("sources")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let sources_content: Vec<Option<String>> = parsed
            .get("sourcesContent")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let names: Vec<String> = parsed
            .get("names")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let mappings_str = parsed
            .get("mappings")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let decoded_segments =
            Self::decode_mappings_str(mappings_str);

        Ok(SourceMapV3 {
            version,
            file,
            source_root,
            sources,
            sources_content,
            names,
            mappings: mappings_str.to_string(),
            decoded_segments,
        })
    }

    #[allow(unused_assignments)]
    fn decode_mappings_str(mappings: &str) -> Vec<DecodedSegment> {
        let mut segments = Vec::new();
        let mut gen_line: u32 = 0;
        let mut prev_gen_col: i64 = 0;
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;
        let mut prev_name: i64 = 0;

        for group in mappings.split(';') {
            prev_gen_col = 0; // reset per-line relative column
            if !group.is_empty() {
                for segment_str in group.split(',') {
                    if segment_str.is_empty() {
                        continue;
                    }
                    let fields = VlqEncoder::decode_vlq(segment_str);
                    if fields.is_empty() {
                        continue;
                    }

                    let gen_column = (prev_gen_col + fields[0]) as u32;
                    prev_gen_col += fields[0];

                    let (source_idx, orig_line, orig_column, name_idx) = if fields.len() >= 4 {
                        prev_source += fields[1];
                        prev_orig_line += fields[2];
                        prev_orig_col += fields[3];
                        let ni = if fields.len() >= 5 {
                            prev_name += fields[4];
                            Some(prev_name as u32)
                        } else {
                            None
                        };
                        (
                            Some(prev_source as u32),
                            Some(prev_orig_line as u32),
                            Some(prev_orig_col as u32),
                            ni,
                        )
                    } else {
                        (None, None, None, None)
                    };

                    segments.push(DecodedSegment {
                        gen_line,
                        gen_column,
                        source_idx,
                        orig_line,
                        orig_column,
                        name_idx,
                    });
                }
            }
            gen_line += 1;
        }
        segments
    }

    /// Look up the original position for a generated line/column (both 0-indexed).
    pub fn lookup(&self, gen_line: u32, gen_column: u32) -> Option<OriginalPosition> {
        // Find the best matching segment: same line, largest column <= gen_column
        let mut best: Option<&DecodedSegment> = None;
        for seg in &self.decoded_segments {
            if seg.gen_line == gen_line && seg.gen_column <= gen_column {
                match best {
                    Some(prev) if seg.gen_column > prev.gen_column => best = Some(seg),
                    None => best = Some(seg),
                    _ => {}
                }
            }
        }

        best.and_then(|seg| {
            let source_idx = seg.source_idx? as usize;
            let source = self.sources.get(source_idx)?.clone();
            Some(OriginalPosition {
                source,
                line: seg.orig_line?,
                column: seg.orig_column?,
                name: seg
                    .name_idx
                    .and_then(|i| self.names.get(i as usize).cloned()),
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Diagnostic types
// ---------------------------------------------------------------------------

/// An error with full source location for diagnostic rendering.
#[derive(Debug, Clone)]
pub struct DiagnosticError {
    pub message: String,
    pub source_file: String,
    pub source_code: String,
    pub line: usize,
    pub column: usize,
    pub length: usize,
    pub hint: Option<String>,
    pub notes: Vec<String>,
}

/// A warning with full source location for diagnostic rendering.
#[derive(Debug, Clone)]
pub struct DiagnosticWarning {
    pub message: String,
    pub source_file: String,
    pub source_code: String,
    pub line: usize,
    pub column: usize,
    pub length: usize,
    pub hint: Option<String>,
    pub notes: Vec<String>,
}

/// Diagnostic – either an error or a warning.
#[derive(Debug, Clone)]
pub enum Diagnostic {
    Error(DiagnosticError),
    Warning(DiagnosticWarning),
}

// ---------------------------------------------------------------------------
// Diagnostic Renderer
// ---------------------------------------------------------------------------

/// Renders rich, Rust-style diagnostic messages.
pub struct DiagnosticRenderer;

impl DiagnosticRenderer {
    /// Render a diagnostic error as a multi-line string.
    pub fn render_error(error: &DiagnosticError) -> String {
        Self::render_impl("error", &error.message, &error.source_file, &error.source_code,
            error.line, error.column, error.length, &error.hint, &error.notes)
    }

    /// Render a diagnostic warning as a multi-line string.
    pub fn render_warning(warning: &DiagnosticWarning) -> String {
        Self::render_impl("warning", &warning.message, &warning.source_file, &warning.source_code,
            warning.line, warning.column, warning.length, &warning.hint, &warning.notes)
    }

    /// Render a `Diagnostic` enum value.
    pub fn render(diagnostic: &Diagnostic) -> String {
        match diagnostic {
            Diagnostic::Error(e) => Self::render_error(e),
            Diagnostic::Warning(w) => Self::render_warning(w),
        }
    }

    fn render_impl(
        severity: &str,
        message: &str,
        source_file: &str,
        source_code: &str,
        line: usize,
        column: usize,
        length: usize,
        hint: &Option<String>,
        notes: &[String],
    ) -> String {
        let lines: Vec<&str> = source_code.lines().collect();
        let line_idx = line.saturating_sub(1);
        let line_num_width = format!("{}", line).len().max(2);

        let mut out = String::new();

        // Header
        out.push_str(&format!("{}: {}\n", severity, message));

        // Location
        out.push_str(&format!(
            "{:>width$}--> {}:{}:{}\n",
            "",
            source_file,
            line,
            column,
            width = line_num_width
        ));

        // Empty gutter
        out.push_str(&format!("{:>width$} |\n", "", width = line_num_width));

        // Source line
        if let Some(src_line) = lines.get(line_idx) {
            out.push_str(&format!(
                "{:>width$} | {}\n",
                line,
                src_line,
                width = line_num_width
            ));

            // Underline
            let underline_len = if length > 0 { length } else { 1 };
            let padding = " ".repeat(column.saturating_sub(1));
            let carets = "^".repeat(underline_len);

            // Build underline suffix from hint message (first short note)
            let underline_msg = if let Some(ref h) = hint {
                format!(" {}", h)
            } else {
                String::new()
            };

            out.push_str(&format!(
                "{:>width$} | {}{}{}\n",
                "",
                padding,
                carets,
                underline_msg,
                width = line_num_width
            ));
        }

        // Notes
        if !notes.is_empty() {
            out.push_str(&format!("{:>width$} |\n", "", width = line_num_width));
            for note in notes {
                out.push_str(&format!(
                    "{:>width$} = help: {}\n",
                    "",
                    note,
                    width = line_num_width
                ));
            }
        }

        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- VLQ tests ----------------------------------------------------------

    #[test]
    fn test_vlq_encode_zero() {
        assert_eq!(VlqEncoder::encode_vlq(0), "A");
    }

    #[test]
    fn test_vlq_encode_positive() {
        // 1 → VLQ unsigned 2 → binary 00010 → 'C'
        assert_eq!(VlqEncoder::encode_vlq(1), "C");
    }

    #[test]
    fn test_vlq_encode_negative() {
        // -1 → VLQ unsigned 3 → binary 00011 → 'D'
        assert_eq!(VlqEncoder::encode_vlq(-1), "D");
    }

    #[test]
    fn test_vlq_encode_large_positive() {
        // 16 → VLQ unsigned 32 → needs two digits
        let encoded = VlqEncoder::encode_vlq(16);
        let decoded = VlqEncoder::decode_vlq(&encoded);
        assert_eq!(decoded, vec![16]);
    }

    #[test]
    fn test_vlq_decode_roundtrip() {
        let values = vec![0, 1, -1, 5, -5, 100, -100, 1000, -1000];
        for &v in &values {
            let encoded = VlqEncoder::encode_vlq(v);
            let decoded = VlqEncoder::decode_vlq(&encoded);
            assert_eq!(decoded, vec![v], "roundtrip failed for {}", v);
        }
    }

    #[test]
    fn test_vlq_decode_multiple_values() {
        // Concatenate encodings of several values
        let vals = vec![0i64, 3, -7, 42];
        let encoded: String = vals.iter().map(|&v| VlqEncoder::encode_vlq(v)).collect();
        let decoded = VlqEncoder::decode_vlq(&encoded);
        assert_eq!(decoded, vals);
    }

    // -- Source Map V3 tests ------------------------------------------------

    #[test]
    fn test_source_map_encode_decode_roundtrip() {
        let mut sm = SourceMapV3::new();
        sm.file = Some("out.js".to_string());
        let src_idx = sm.add_source("input.js");
        let name_idx = sm.add_name("foo");
        sm.add_mapping(0, 0, src_idx, 0, 0, Some(name_idx));
        sm.add_mapping(0, 5, src_idx, 1, 3, None);
        sm.add_mapping(1, 0, src_idx, 2, 0, None);

        let json = sm.encode();
        let decoded = SourceMapV3::decode(&json).expect("decode failed");

        assert_eq!(decoded.version, 3);
        assert_eq!(decoded.file.as_deref(), Some("out.js"));
        assert_eq!(decoded.sources, vec!["input.js"]);
        assert_eq!(decoded.names, vec!["foo"]);
    }

    #[test]
    fn test_source_map_lookup() {
        let mut sm = SourceMapV3::new();
        let src = sm.add_source("app.js");
        sm.add_mapping(0, 0, src, 10, 5, None);
        sm.add_mapping(0, 8, src, 10, 20, None);
        sm.add_mapping(1, 0, src, 15, 0, None);

        // Exact match
        let pos = sm.lookup(0, 0).expect("lookup(0,0)");
        assert_eq!(pos.line, 10);
        assert_eq!(pos.column, 5);
        assert_eq!(pos.source, "app.js");

        // Between two segments on same line – should pick the earlier one
        let pos2 = sm.lookup(0, 4).expect("lookup(0,4)");
        assert_eq!(pos2.line, 10);
        assert_eq!(pos2.column, 5);

        // Exact match on second segment
        let pos3 = sm.lookup(0, 8).expect("lookup(0,8)");
        assert_eq!(pos3.line, 10);
        assert_eq!(pos3.column, 20);

        // No match
        assert!(sm.lookup(5, 0).is_none());
    }

    #[test]
    fn test_source_map_lookup_after_decode() {
        let mut sm = SourceMapV3::new();
        let src = sm.add_source("lib.ts");
        let name = sm.add_name("greet");
        sm.add_mapping(0, 0, src, 3, 4, Some(name));

        let json = sm.encode();
        let decoded = SourceMapV3::decode(&json).unwrap();
        let pos = decoded.lookup(0, 0).unwrap();
        assert_eq!(pos.source, "lib.ts");
        assert_eq!(pos.line, 3);
        assert_eq!(pos.column, 4);
        assert_eq!(pos.name.as_deref(), Some("greet"));
    }

    #[test]
    fn test_decode_invalid_version() {
        let json = r#"{"version": 2, "sources": [], "names": [], "mappings": ""}"#;
        assert!(SourceMapV3::decode(json).is_err());
    }

    // -- Diagnostic rendering tests -----------------------------------------

    #[test]
    fn test_render_error_basic() {
        let err = DiagnosticError {
            message: "Cannot assign to constant variable".to_string(),
            source_file: "script.js".to_string(),
            source_code: "const x = 5;\nx = 10;".to_string(),
            line: 2,
            column: 1,
            length: 6,
            hint: Some("cannot reassign `const` variable".to_string()),
            notes: vec!["declare with `let` instead of `const` to allow reassignment".to_string()],
        };
        let rendered = DiagnosticRenderer::render_error(&err);

        assert!(rendered.contains("error: Cannot assign to constant variable"));
        assert!(rendered.contains("--> script.js:2:1"));
        assert!(rendered.contains("x = 10;"));
        assert!(rendered.contains("^^^^^^"));
        assert!(rendered.contains("cannot reassign `const` variable"));
        assert!(rendered.contains("= help: declare with `let`"));
    }

    #[test]
    fn test_render_warning() {
        let warn = DiagnosticWarning {
            message: "Unused variable 'y'".to_string(),
            source_file: "app.js".to_string(),
            source_code: "let y = 42;".to_string(),
            line: 1,
            column: 5,
            length: 1,
            hint: None,
            notes: vec![],
        };
        let rendered = DiagnosticRenderer::render_warning(&warn);
        assert!(rendered.contains("warning: Unused variable 'y'"));
        assert!(rendered.contains("--> app.js:1:5"));
        assert!(rendered.contains("let y = 42;"));
    }

    #[test]
    fn test_render_diagnostic_enum() {
        let diag = Diagnostic::Error(DiagnosticError {
            message: "test".to_string(),
            source_file: "a.js".to_string(),
            source_code: "x".to_string(),
            line: 1,
            column: 1,
            length: 1,
            hint: None,
            notes: vec![],
        });
        let rendered = DiagnosticRenderer::render(&diag);
        assert!(rendered.starts_with("error:"));
    }
}
