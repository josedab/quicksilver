//! Language Server Protocol implementation for Quicksilver
//!
//! Provides IDE integration through the Language Server Protocol (LSP),
//! offering diagnostics, completions, hover info, and document synchronization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 Protocol Layer
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<JsonRpcId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// LSP Types
// ---------------------------------------------------------------------------

/// Position in a document (0-indexed)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<DiagnosticSeverity>,
    pub code: Option<String>,
    pub source: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: Option<CompletionItemKind>,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompletionItemKind {
    Text = 1,
    Method = 2,
    Function = 3,
    Constructor = 4,
    Field = 5,
    Variable = 6,
    Class = 7,
    Interface = 8,
    Module = 9,
    Property = 10,
    Value = 12,
    Keyword = 14,
    Snippet = 15,
    Constant = 21,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hover {
    pub contents: MarkupContent,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkupContent {
    pub kind: String,
    pub value: String,
}

/// Server capabilities advertised to the client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub text_document_sync: Option<TextDocumentSyncKind>,
    pub completion_provider: Option<CompletionOptions>,
    pub hover_provider: Option<bool>,
    pub diagnostic_provider: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TextDocumentSyncKind {
    None = 0,
    Full = 1,
    Incremental = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionOptions {
    pub trigger_characters: Option<Vec<String>>,
    pub resolve_provider: Option<bool>,
}

// ---------------------------------------------------------------------------
// Document Manager
// ---------------------------------------------------------------------------

/// Manages open documents and their content
pub struct DocumentManager {
    documents: HashMap<String, DocumentState>,
}

pub struct DocumentState {
    pub uri: String,
    pub content: String,
    pub version: i32,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentManager {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: &str, content: &str, version: i32) {
        self.documents.insert(
            uri.to_string(),
            DocumentState {
                uri: uri.to_string(),
                content: content.to_string(),
                version,
                diagnostics: Vec::new(),
            },
        );
    }

    pub fn update(&mut self, uri: &str, content: &str, version: i32) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.content = content.to_string();
            doc.version = version;
        }
    }

    pub fn close(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &str) -> Option<&DocumentState> {
        self.documents.get(uri)
    }

    pub fn get_content(&self, uri: &str) -> Option<&str> {
        self.documents.get(uri).map(|d| d.content.as_str())
    }
}

// ---------------------------------------------------------------------------
// Diagnostic Provider
// ---------------------------------------------------------------------------

/// Provides diagnostics (errors/warnings) by parsing source code
pub struct DiagnosticProvider;

impl DiagnosticProvider {
    /// Parse source with the Quicksilver lexer+parser and convert errors to LSP diagnostics.
    pub fn compute_diagnostics(source: &str, _uri: &str) -> Vec<Diagnostic> {
        use crate::parser::Parser;

        let mut diagnostics = Vec::new();

        // Parser::new runs the lexer internally; a failure here means a lexer error.
        let mut parser = match Parser::new(source) {
            Ok(p) => p,
            Err(e) => {
                diagnostics.push(Self::error_to_diagnostic(&e));
                return diagnostics;
            }
        };

        // Use recovery mode to collect as many errors as possible.
        let (_program, errors) = parser.parse_program_with_recovery();
        for err in &errors {
            diagnostics.push(Self::error_to_diagnostic(err));
        }

        diagnostics
    }

    fn error_to_diagnostic(err: &crate::error::Error) -> Diagnostic {
        use crate::error::Error;

        let (message, line, col) = match err {
            Error::LexerError {
                message, location, ..
            } => (message.clone(), location.line, location.column),
            Error::ParseError {
                message, location, ..
            } => (message.clone(), location.line, location.column),
            other => (other.to_string(), 1, 1),
        };

        // LSP positions are 0-indexed; Quicksilver locations are 1-indexed.
        let pos = Position {
            line: line.saturating_sub(1),
            character: col.saturating_sub(1),
        };

        Diagnostic {
            range: Range {
                start: pos,
                end: pos,
            },
            severity: Some(DiagnosticSeverity::Error),
            code: Some("quicksilver".to_string()),
            source: Some("quicksilver".to_string()),
            message,
        }
    }
}

// ---------------------------------------------------------------------------
// Completion Provider
// ---------------------------------------------------------------------------

/// Provides auto-completion suggestions
pub struct CompletionProvider;

impl CompletionProvider {
    pub fn completions_at(source: &str, _position: Position) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        items.extend(Self::keyword_completions());
        items.extend(Self::builtin_completions());
        items.extend(Self::identifier_completions(source));
        items
    }

    pub fn keyword_completions() -> Vec<CompletionItem> {
        const KEYWORDS: &[&str] = &[
            "break", "case", "catch", "class", "const", "continue", "debugger", "default",
            "delete", "do", "else", "export", "extends", "false", "finally", "for", "function",
            "if", "import", "in", "instanceof", "let", "new", "null", "return", "super",
            "switch", "this", "throw", "true", "try", "typeof", "undefined", "var", "void",
            "while", "with", "yield", "async", "await", "of",
        ];
        KEYWORDS
            .iter()
            .map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::Keyword),
                detail: Some("keyword".to_string()),
                documentation: None,
                insert_text: Some(kw.to_string()),
            })
            .collect()
    }

    pub fn builtin_completions() -> Vec<CompletionItem> {
        let builtins: &[(&str, CompletionItemKind, &str)] = &[
            ("console", CompletionItemKind::Module, "Built-in console object for logging"),
            ("Math", CompletionItemKind::Module, "Built-in Math object"),
            ("JSON", CompletionItemKind::Module, "Built-in JSON object"),
            ("Object", CompletionItemKind::Class, "Built-in Object constructor"),
            ("Array", CompletionItemKind::Class, "Built-in Array constructor"),
            ("String", CompletionItemKind::Class, "Built-in String constructor"),
            ("Number", CompletionItemKind::Class, "Built-in Number constructor"),
            ("Boolean", CompletionItemKind::Class, "Built-in Boolean constructor"),
            ("Date", CompletionItemKind::Class, "Built-in Date constructor"),
            ("Map", CompletionItemKind::Class, "Built-in Map constructor"),
            ("Set", CompletionItemKind::Class, "Built-in Set constructor"),
            ("WeakMap", CompletionItemKind::Class, "Built-in WeakMap constructor"),
            ("WeakSet", CompletionItemKind::Class, "Built-in WeakSet constructor"),
            ("Promise", CompletionItemKind::Class, "Built-in Promise constructor"),
            ("Symbol", CompletionItemKind::Class, "Built-in Symbol constructor"),
            ("RegExp", CompletionItemKind::Class, "Built-in RegExp constructor"),
            ("Error", CompletionItemKind::Class, "Built-in Error constructor"),
            ("parseInt", CompletionItemKind::Function, "Parse a string to an integer"),
            ("parseFloat", CompletionItemKind::Function, "Parse a string to a float"),
            ("isNaN", CompletionItemKind::Function, "Check if a value is NaN"),
            ("isFinite", CompletionItemKind::Function, "Check if a value is finite"),
            ("undefined", CompletionItemKind::Constant, "The undefined value"),
            ("null", CompletionItemKind::Constant, "The null value"),
            ("true", CompletionItemKind::Constant, "Boolean true"),
            ("false", CompletionItemKind::Constant, "Boolean false"),
            ("NaN", CompletionItemKind::Constant, "Not a Number"),
            ("Infinity", CompletionItemKind::Constant, "Positive Infinity"),
        ];
        builtins
            .iter()
            .map(|(name, kind, doc)| CompletionItem {
                label: name.to_string(),
                kind: Some(*kind),
                detail: Some(doc.to_string()),
                documentation: Some(doc.to_string()),
                insert_text: Some(name.to_string()),
            })
            .collect()
    }

    /// Extract identifiers from the source text (simple word-boundary scan).
    pub fn identifier_completions(source: &str) -> Vec<CompletionItem> {
        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();

        for word in source.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$') {
            if word.is_empty() {
                continue;
            }
            // Must start with a letter, underscore, or dollar sign
            let first = word.chars().next().unwrap();
            if !first.is_alphabetic() && first != '_' && first != '$' {
                continue;
            }
            if seen.insert(word.to_string()) {
                items.push(CompletionItem {
                    label: word.to_string(),
                    kind: Some(CompletionItemKind::Variable),
                    detail: Some("identifier".to_string()),
                    documentation: None,
                    insert_text: Some(word.to_string()),
                });
            }
        }

        items
    }
}

// ---------------------------------------------------------------------------
// Hover Provider
// ---------------------------------------------------------------------------

/// Provides hover information
pub struct HoverProvider;

impl HoverProvider {
    pub fn hover_at(source: &str, position: Position) -> Option<Hover> {
        let word = Self::word_at(source, position)?;
        let doc = Self::builtin_docs(&word)?;
        Some(Hover {
            contents: MarkupContent {
                kind: "markdown".to_string(),
                value: doc,
            },
            range: None,
        })
    }

    pub fn builtin_docs(name: &str) -> Option<String> {
        match name {
            "console" => Some(
                "Built-in console object for logging. Methods: log, warn, error, info, debug, trace"
                    .to_string(),
            ),
            "Math" => Some(
                "Built-in Math object. Methods: abs, ceil, floor, round, max, min, random, sqrt, pow, log, sin, cos, tan"
                    .to_string(),
            ),
            "JSON" => {
                Some("Built-in JSON object. Methods: parse, stringify".to_string())
            }
            "Object" => Some(
                "Built-in Object constructor. Methods: keys, values, entries, assign, freeze, create"
                    .to_string(),
            ),
            "Array" => Some(
                "Built-in Array constructor. Methods: isArray, from, of".to_string(),
            ),
            "String" => Some(
                "Built-in String constructor. Methods: fromCharCode, fromCodePoint".to_string(),
            ),
            "Number" => Some(
                "Built-in Number constructor. Properties: MAX_SAFE_INTEGER, MIN_SAFE_INTEGER, EPSILON, isFinite, isInteger, isNaN, parseFloat, parseInt"
                    .to_string(),
            ),
            "Boolean" => {
                Some("Built-in Boolean constructor".to_string())
            }
            "Date" => Some(
                "Built-in Date constructor. Methods: now, parse, UTC".to_string(),
            ),
            "Map" => Some(
                "Built-in Map constructor. A keyed collection of values.".to_string(),
            ),
            "Set" => Some(
                "Built-in Set constructor. A collection of unique values.".to_string(),
            ),
            "Promise" => Some(
                "Built-in Promise constructor. Methods: resolve, reject, all, race, allSettled, any"
                    .to_string(),
            ),
            "Error" => Some(
                "Built-in Error constructor. Properties: message, stack, name".to_string(),
            ),
            "parseInt" => Some(
                "parseInt(string, radix?) — Parse a string argument and return an integer."
                    .to_string(),
            ),
            "parseFloat" => Some(
                "parseFloat(string) — Parse a string argument and return a floating-point number."
                    .to_string(),
            ),
            "isNaN" => Some(
                "isNaN(value) — Determine whether a value is NaN.".to_string(),
            ),
            "isFinite" => Some(
                "isFinite(value) — Determine whether a value is a finite number.".to_string(),
            ),
            "undefined" => Some("The primitive value undefined.".to_string()),
            "null" => Some("The primitive value null.".to_string()),
            "NaN" => Some("Not-a-Number. A special IEEE 754 floating-point value.".to_string()),
            "Infinity" => Some("Positive Infinity.".to_string()),
            _ => None,
        }
    }

    /// Extract the word under the given position from `source`.
    fn word_at(source: &str, position: Position) -> Option<String> {
        let line_str = source.lines().nth(position.line as usize)?;
        let col = position.character as usize;
        if col > line_str.len() {
            return None;
        }

        let bytes = line_str.as_bytes();
        let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';

        // find start
        let mut start = col;
        while start > 0 && is_ident(bytes[start - 1]) {
            start -= 1;
        }
        // find end
        let mut end = col;
        while end < bytes.len() && is_ident(bytes[end]) {
            end += 1;
        }

        if start == end {
            return None;
        }
        Some(line_str[start..end].to_string())
    }
}

// ---------------------------------------------------------------------------
// LSP Server
// ---------------------------------------------------------------------------

/// The LSP server handles incoming requests and produces responses
pub struct LspServer {
    documents: DocumentManager,
    initialized: bool,
    capabilities: ServerCapabilities,
}

impl LspServer {
    pub fn new() -> Self {
        Self {
            documents: DocumentManager::new(),
            initialized: false,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncKind::Full),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    resolve_provider: Some(false),
                }),
                hover_provider: Some(true),
                diagnostic_provider: Some(true),
            },
        }
    }

    /// Main dispatch for incoming JSON-RPC messages.
    pub fn handle_message(&mut self, msg: &JsonRpcMessage) -> Option<JsonRpcMessage> {
        let method = msg.method.as_deref()?;
        match method {
            "initialize" => {
                let id = msg.id.clone()?;
                Some(self.handle_initialize(id))
            }
            "initialized" => {
                self.handle_initialized();
                None
            }
            "shutdown" => {
                let id = msg.id.clone()?;
                Some(self.handle_shutdown(id))
            }
            "textDocument/didOpen" => {
                if let Some(params) = &msg.params {
                    self.handle_text_document_did_open(params);
                }
                None
            }
            "textDocument/didChange" => {
                if let Some(params) = &msg.params {
                    self.handle_text_document_did_change(params);
                }
                None
            }
            "textDocument/didClose" => {
                if let Some(params) = &msg.params {
                    self.handle_text_document_did_close(params);
                }
                None
            }
            "textDocument/completion" => {
                let id = msg.id.clone()?;
                let params = msg.params.as_ref()?;
                Some(self.handle_completion(id, params))
            }
            "textDocument/hover" => {
                let id = msg.id.clone()?;
                let params = msg.params.as_ref()?;
                Some(self.handle_hover(id, params))
            }
            _ => {
                let id = msg.id.clone()?;
                Some(Self::make_error(id, -32601, &format!("Method not found: {}", method)))
            }
        }
    }

    pub fn handle_initialize(&mut self, id: JsonRpcId) -> JsonRpcMessage {
        self.initialized = true;
        let result = serde_json::json!({
            "capabilities": self.capabilities,
            "serverInfo": {
                "name": "quicksilver-lsp",
                "version": crate::VERSION,
            }
        });
        Self::make_response(id, result)
    }

    pub fn handle_initialized(&mut self) {
        // No-op; server is ready.
    }

    pub fn handle_shutdown(&mut self, id: JsonRpcId) -> JsonRpcMessage {
        self.initialized = false;
        Self::make_response(id, serde_json::Value::Null)
    }

    pub fn handle_text_document_did_open(&mut self, params: &serde_json::Value) {
        if let Some(td) = params.get("textDocument") {
            let uri = td.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let text = td.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let version = td.get("version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            self.documents.open(uri, text, version);
            let diags = DiagnosticProvider::compute_diagnostics(text, uri);
            if let Some(doc) = self.documents.documents.get_mut(uri) {
                doc.diagnostics = diags;
            }
        }
    }

    pub fn handle_text_document_did_change(&mut self, params: &serde_json::Value) {
        if let Some(td) = params.get("textDocument") {
            let uri = td.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let version = td.get("version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            // Full sync: take the last content change.
            let text = params
                .get("contentChanges")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.last())
                .and_then(|c| c.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            self.documents.update(uri, text, version);
            let diags = DiagnosticProvider::compute_diagnostics(text, uri);
            if let Some(doc) = self.documents.documents.get_mut(uri) {
                doc.diagnostics = diags;
            }
        }
    }

    pub fn handle_text_document_did_close(&mut self, params: &serde_json::Value) {
        if let Some(td) = params.get("textDocument") {
            let uri = td.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            self.documents.close(uri);
        }
    }

    pub fn handle_completion(
        &mut self,
        id: JsonRpcId,
        params: &serde_json::Value,
    ) -> JsonRpcMessage {
        let uri = params
            .pointer("/textDocument/uri")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let position = Self::extract_position(params);

        let items = if let Some(content) = self.documents.get_content(uri) {
            CompletionProvider::completions_at(content, position)
        } else {
            CompletionProvider::completions_at("", position)
        };

        Self::make_response(id, serde_json::to_value(&items).unwrap_or_default())
    }

    pub fn handle_hover(
        &mut self,
        id: JsonRpcId,
        params: &serde_json::Value,
    ) -> JsonRpcMessage {
        let uri = params
            .pointer("/textDocument/uri")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let position = Self::extract_position(params);

        let hover = self
            .documents
            .get_content(uri)
            .and_then(|content| HoverProvider::hover_at(content, position));

        match hover {
            Some(h) => Self::make_response(id, serde_json::to_value(&h).unwrap_or_default()),
            None => Self::make_response(id, serde_json::Value::Null),
        }
    }

    /// Return pending diagnostics for all open documents (for notification publishing).
    pub fn get_pending_diagnostics(&self) -> Vec<(String, Vec<Diagnostic>)> {
        self.documents
            .documents
            .iter()
            .filter(|(_, doc)| !doc.diagnostics.is_empty())
            .map(|(uri, doc)| (uri.clone(), doc.diagnostics.clone()))
            .collect()
    }

    // -- helpers -------------------------------------------------------------

    pub fn make_response(id: JsonRpcId, result: serde_json::Value) -> JsonRpcMessage {
        JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        }
    }

    pub fn make_error(id: JsonRpcId, code: i64, message: &str) -> JsonRpcMessage {
        JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: None,
            params: None,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }

    fn extract_position(params: &serde_json::Value) -> Position {
        let line = params
            .pointer("/position/line")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let character = params
            .pointer("/position/character")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        Position { line, character }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- JSON-RPC serialization -------------------------------------------

    #[test]
    fn test_jsonrpc_message_serialization() {
        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Number(1)),
            method: Some("initialize".to_string()),
            params: Some(serde_json::json!({})),
            result: None,
            error: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"initialize\""));
        // result and error should be absent (skip_serializing_if)
        assert!(!json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_jsonrpc_message_deserialization() {
        let json = r#"{"jsonrpc":"2.0","id":42,"method":"shutdown"}"#;
        let msg: JsonRpcMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.jsonrpc, "2.0");
        assert!(msg.method.as_deref() == Some("shutdown"));
        match msg.id {
            Some(JsonRpcId::Number(n)) => assert_eq!(n, 42),
            _ => panic!("expected numeric id"),
        }
    }

    // -- Position / Range serialization -----------------------------------

    #[test]
    fn test_position_range_serialization() {
        let range = Range {
            start: Position {
                line: 1,
                character: 5,
            },
            end: Position {
                line: 1,
                character: 10,
            },
        };
        let json = serde_json::to_value(&range).unwrap();
        assert_eq!(json["start"]["line"], 1);
        assert_eq!(json["end"]["character"], 10);
    }

    // -- LspServer creation -----------------------------------------------

    #[test]
    fn test_lsp_server_creation() {
        let server = LspServer::new();
        assert!(!server.initialized);
        assert!(server.capabilities.hover_provider == Some(true));
        assert!(server.capabilities.diagnostic_provider == Some(true));
    }

    // -- Initialize handshake ---------------------------------------------

    #[test]
    fn test_initialize_handshake() {
        let mut server = LspServer::new();
        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Number(1)),
            method: Some("initialize".to_string()),
            params: Some(serde_json::json!({})),
            result: None,
            error: None,
        };
        let resp = server.handle_message(&msg).unwrap();
        assert!(resp.result.is_some());
        let caps = &resp.result.unwrap();
        assert!(caps["capabilities"]["hover_provider"].as_bool() == Some(true));
        assert!(server.initialized);
    }

    // -- Document open / close / update -----------------------------------

    #[test]
    fn test_document_open_close() {
        let mut dm = DocumentManager::new();
        dm.open("file:///a.js", "let x = 1;", 1);
        assert!(dm.get("file:///a.js").is_some());
        assert_eq!(dm.get_content("file:///a.js").unwrap(), "let x = 1;");
        dm.close("file:///a.js");
        assert!(dm.get("file:///a.js").is_none());
    }

    #[test]
    fn test_document_update() {
        let mut dm = DocumentManager::new();
        dm.open("file:///a.js", "let x = 1;", 1);
        dm.update("file:///a.js", "let x = 2;", 2);
        assert_eq!(dm.get_content("file:///a.js").unwrap(), "let x = 2;");
        assert_eq!(dm.get("file:///a.js").unwrap().version, 2);
    }

    // -- Document version tracking ----------------------------------------

    #[test]
    fn test_document_version_tracking() {
        let mut dm = DocumentManager::new();
        dm.open("file:///v.js", "v1", 1);
        assert_eq!(dm.get("file:///v.js").unwrap().version, 1);
        dm.update("file:///v.js", "v2", 5);
        assert_eq!(dm.get("file:///v.js").unwrap().version, 5);
    }

    // -- Multiple documents -----------------------------------------------

    #[test]
    fn test_multiple_documents() {
        let mut dm = DocumentManager::new();
        dm.open("file:///a.js", "let a;", 1);
        dm.open("file:///b.js", "let b;", 1);
        assert!(dm.get("file:///a.js").is_some());
        assert!(dm.get("file:///b.js").is_some());
        dm.close("file:///a.js");
        assert!(dm.get("file:///a.js").is_none());
        assert!(dm.get("file:///b.js").is_some());
    }

    // -- Diagnostics: parse errors ----------------------------------------

    #[test]
    fn test_diagnostic_parse_error() {
        let diags = DiagnosticProvider::compute_diagnostics("let x = ;", "file:///bad.js");
        assert!(!diags.is_empty(), "Expected at least one diagnostic for invalid JS");
        assert!(matches!(
            diags[0].severity,
            Some(DiagnosticSeverity::Error)
        ));
    }

    // -- Diagnostics: valid JS --------------------------------------------

    #[test]
    fn test_diagnostic_valid_js() {
        let diags = DiagnosticProvider::compute_diagnostics("let x = 42;", "file:///ok.js");
        assert!(diags.is_empty(), "Valid JS should produce no diagnostics");
    }

    // -- Completion: keywords ---------------------------------------------

    #[test]
    fn test_completion_keywords() {
        let kws = CompletionProvider::keyword_completions();
        let labels: Vec<&str> = kws.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"function"));
        assert!(labels.contains(&"const"));
        assert!(labels.contains(&"return"));
    }

    // -- Completion: builtins ---------------------------------------------

    #[test]
    fn test_completion_builtins() {
        let builtins = CompletionProvider::builtin_completions();
        let labels: Vec<&str> = builtins.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"console"));
        assert!(labels.contains(&"Math"));
        assert!(labels.contains(&"JSON"));
        assert!(labels.contains(&"parseInt"));
    }

    // -- Completion: identifiers from source ------------------------------

    #[test]
    fn test_completion_identifiers() {
        let ids = CompletionProvider::identifier_completions("let myVar = 1; const foo = myVar;");
        let labels: Vec<&str> = ids.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"myVar"));
        assert!(labels.contains(&"foo"));
    }

    // -- Hover: known builtin ---------------------------------------------

    #[test]
    fn test_hover_console() {
        let source = "console.log('hello');";
        let hover = HoverProvider::hover_at(source, Position { line: 0, character: 3 });
        assert!(hover.is_some());
        let h = hover.unwrap();
        assert!(h.contents.value.contains("console"));
        assert!(h.contents.value.contains("log"));
    }

    // -- Hover: unknown identifier ----------------------------------------

    #[test]
    fn test_hover_unknown() {
        let source = "myCustomVar;";
        let hover = HoverProvider::hover_at(source, Position { line: 0, character: 2 });
        assert!(hover.is_none());
    }

    // -- Shutdown response ------------------------------------------------

    #[test]
    fn test_shutdown_response() {
        let mut server = LspServer::new();
        server.initialized = true;
        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Number(99)),
            method: Some("shutdown".to_string()),
            params: None,
            result: None,
            error: None,
        };
        let resp = server.handle_message(&msg).unwrap();
        assert!(resp.result == Some(serde_json::Value::Null));
        assert!(!server.initialized);
    }

    // -- Error response for unknown method --------------------------------

    #[test]
    fn test_unknown_method_error() {
        let mut server = LspServer::new();
        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Number(7)),
            method: Some("nonexistent/method".to_string()),
            params: None,
            result: None,
            error: None,
        };
        let resp = server.handle_message(&msg).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32601);
    }
}
