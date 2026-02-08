//! Interactive REPL (Read-Eval-Print Loop)
//!
//! A feature-rich interactive JavaScript shell with:
//! - Command history (persisted across sessions)
//! - Tab completion for JavaScript keywords, globals, and properties
//! - Syntax highlighting
//! - Multi-line input support
//! - Special commands (.help, .clear, .exit, etc.)
//! - Pretty-printed output
//! - Error display with context

//! **Status:** ⚠️ Partial — Basic REPL, limited completion

use crate::{Runtime, Value};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

/// REPL configuration
#[derive(Debug, Clone)]
pub struct ReplConfig {
    /// History file path
    pub history_file: Option<PathBuf>,
    /// Maximum history entries
    pub history_size: usize,
    /// Prompt string
    pub prompt: String,
    /// Continuation prompt (for multi-line input)
    pub continuation_prompt: String,
    /// Enable colors
    pub colors: bool,
    /// Show execution time
    pub show_timing: bool,
}

impl Default for ReplConfig {
    fn default() -> Self {
        let history_file = home_dir().map(|h| h.join(".quicksilver_history"));

        Self {
            history_file,
            history_size: 1000,
            prompt: "qs> ".to_string(),
            continuation_prompt: "... ".to_string(),
            colors: true,
            show_timing: false,
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// JavaScript keywords for completion
const JS_KEYWORDS: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue",
    "debugger", "default", "delete", "do", "else", "export", "extends", "false",
    "finally", "for", "function", "if", "import", "in", "instanceof", "let",
    "new", "null", "of", "return", "static", "super", "switch", "this", "throw",
    "true", "try", "typeof", "undefined", "var", "void", "while", "with", "yield",
];

/// Built-in global objects for completion
const JS_GLOBALS: &[&str] = &[
    "Array", "ArrayBuffer", "BigInt", "Boolean", "console", "DataView", "Date",
    "Error", "EvalError", "Float32Array", "Float64Array", "Function", "Infinity",
    "Int8Array", "Int16Array", "Int32Array", "JSON", "Map", "Math", "NaN",
    "Number", "Object", "Promise", "Proxy", "RangeError", "ReferenceError",
    "RegExp", "Set", "String", "Symbol", "SyntaxError", "TypeError", "URIError",
    "Uint8Array", "Uint8ClampedArray", "Uint16Array", "Uint32Array", "URL",
    "URLSearchParams", "WeakMap", "WeakSet", "globalThis", "isFinite", "isNaN",
    "parseFloat", "parseInt", "decodeURI", "decodeURIComponent", "encodeURI",
    "encodeURIComponent", "setTimeout", "setInterval", "clearTimeout", "clearInterval",
];

/// REPL commands for completion
const REPL_COMMANDS: &[&str] = &[
    ".help", ".h", ".clear", ".cls", ".exit", ".quit", ".q",
    ".timing", ".load", ".env", ".reset",
];

/// Common object methods for completion
const COMMON_METHODS: &[&str] = &[
    "toString", "valueOf", "hasOwnProperty", "isPrototypeOf",
    "propertyIsEnumerable", "toLocaleString", "constructor",
];

/// Array methods for completion
const ARRAY_METHODS: &[&str] = &[
    "length", "push", "pop", "shift", "unshift", "slice", "splice", "concat",
    "join", "reverse", "sort", "indexOf", "lastIndexOf", "forEach", "map",
    "filter", "reduce", "reduceRight", "every", "some", "find", "findIndex",
    "includes", "flat", "flatMap", "fill", "copyWithin", "entries", "keys",
    "values", "at", "from", "isArray", "of",
];

/// String methods for completion
const STRING_METHODS: &[&str] = &[
    "length", "charAt", "charCodeAt", "codePointAt", "concat", "endsWith",
    "includes", "indexOf", "lastIndexOf", "localeCompare", "match", "matchAll",
    "normalize", "padEnd", "padStart", "repeat", "replace", "replaceAll",
    "search", "slice", "split", "startsWith", "substring", "toLowerCase",
    "toUpperCase", "trim", "trimEnd", "trimStart", "at", "fromCharCode",
    "fromCodePoint", "raw",
];

/// Object static methods for completion
const OBJECT_METHODS: &[&str] = &[
    "assign", "create", "defineProperty", "defineProperties", "entries",
    "freeze", "fromEntries", "getOwnPropertyDescriptor", "getOwnPropertyDescriptors",
    "getOwnPropertyNames", "getOwnPropertySymbols", "getPrototypeOf", "hasOwn",
    "is", "isExtensible", "isFrozen", "isSealed", "keys", "preventExtensions",
    "seal", "setPrototypeOf", "values",
];

/// Math properties and methods for completion
const MATH_PROPS: &[&str] = &[
    "E", "LN10", "LN2", "LOG10E", "LOG2E", "PI", "SQRT1_2", "SQRT2",
    "abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh",
    "cbrt", "ceil", "clz32", "cos", "cosh", "exp", "expm1", "floor",
    "fround", "hypot", "imul", "log", "log10", "log1p", "log2", "max",
    "min", "pow", "random", "round", "sign", "sin", "sinh", "sqrt",
    "tan", "tanh", "trunc",
];

/// Console methods for completion
const CONSOLE_METHODS: &[&str] = &[
    "log", "info", "warn", "error", "debug", "trace", "assert", "clear",
    "count", "countReset", "dir", "dirxml", "group", "groupCollapsed",
    "groupEnd", "table", "time", "timeEnd", "timeLog",
];

/// JSON methods for completion
const JSON_METHODS: &[&str] = &["parse", "stringify"];

/// A custom hint based on completion
#[derive(Debug)]
struct CompletionHint {
    text: String,
    complete_up_to: usize,
}

impl Hint for CompletionHint {
    fn display(&self) -> &str {
        &self.text
    }

    fn completion(&self) -> Option<&str> {
        if self.complete_up_to > 0 {
            Some(&self.text[..self.complete_up_to])
        } else {
            None
        }
    }
}

/// REPL helper for validation, completion, hints, and highlighting
struct ReplHelper {
    user_vars: Rc<RefCell<HashSet<String>>>,
    colors_enabled: bool,
}

impl ReplHelper {
    fn new(colors_enabled: bool) -> Self {
        Self {
            user_vars: Rc::new(RefCell::new(HashSet::new())),
            colors_enabled,
        }
    }

    fn get_user_vars(&self) -> Rc<RefCell<HashSet<String>>> {
        Rc::clone(&self.user_vars)
    }

    /// Extract the word being typed for completion
    fn extract_word<'a>(&self, line: &'a str, pos: usize) -> (usize, &'a str) {
        let line_up_to_pos = &line[..pos];

        // Find the start of the current word
        let start = line_up_to_pos
            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '$' && c != '.')
            .map(|i| i + 1)
            .unwrap_or(0);

        (start, &line[start..pos])
    }

    /// Get completions for a given word
    fn get_completions(&self, word: &str) -> Vec<Pair> {
        let mut completions = Vec::new();

        // Handle property access (e.g., "console." or "Math.")
        if let Some(dot_pos) = word.rfind('.') {
            let obj_name = &word[..dot_pos];
            let prop_prefix = &word[dot_pos + 1..];

            let methods: &[&str] = match obj_name {
                "console" => CONSOLE_METHODS,
                "Math" => MATH_PROPS,
                "JSON" => JSON_METHODS,
                "Object" => OBJECT_METHODS,
                "Array" => ARRAY_METHODS,
                "String" => STRING_METHODS,
                _ => COMMON_METHODS,
            };

            for method in methods {
                if method.starts_with(prop_prefix) {
                    completions.push(Pair {
                        display: (*method).to_string(),
                        replacement: format!("{}.{}", obj_name, method),
                    });
                }
            }

            return completions;
        }

        // Handle REPL commands
        if word.starts_with('.') {
            for cmd in REPL_COMMANDS {
                if cmd.starts_with(word) {
                    completions.push(Pair {
                        display: (*cmd).to_string(),
                        replacement: (*cmd).to_string(),
                    });
                }
            }
            return completions;
        }

        // Handle JavaScript keywords
        for keyword in JS_KEYWORDS {
            if keyword.starts_with(word) {
                completions.push(Pair {
                    display: (*keyword).to_string(),
                    replacement: (*keyword).to_string(),
                });
            }
        }

        // Handle global objects
        for global in JS_GLOBALS {
            if global.starts_with(word) {
                completions.push(Pair {
                    display: (*global).to_string(),
                    replacement: (*global).to_string(),
                });
            }
        }

        // Handle user-defined variables
        for var in self.user_vars.borrow().iter() {
            if var.starts_with(word) {
                completions.push(Pair {
                    display: var.clone(),
                    replacement: var.clone(),
                });
            }
        }

        completions
    }
}

impl Helper for ReplHelper {}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let (start, word) = self.extract_word(line, pos);

        if word.is_empty() {
            return Ok((pos, Vec::new()));
        }

        let completions = self.get_completions(word);
        Ok((start, completions))
    }
}

impl Hinter for ReplHelper {
    type Hint = CompletionHint;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        if pos < line.len() {
            return None;
        }

        let (_, word) = self.extract_word(line, pos);
        if word.is_empty() || word.len() < 2 {
            return None;
        }

        // Get completions and use the first one as a hint
        let completions = self.get_completions(word);
        completions.first().map(|c| {
            let suffix = &c.replacement[word.len()..];
            CompletionHint {
                text: suffix.to_string(),
                complete_up_to: suffix.len(),
            }
        })
    }
}

impl Highlighter for ReplHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if !self.colors_enabled {
            return Cow::Borrowed(line);
        }

        let mut result = String::with_capacity(line.len() * 2);
        let mut chars = line.chars().peekable();
        let mut current_word = String::new();

        while let Some(ch) = chars.next() {
            if ch.is_alphanumeric() || ch == '_' || ch == '$' {
                current_word.push(ch);
            } else {
                // Flush the current word with highlighting
                if !current_word.is_empty() {
                    result.push_str(&self.highlight_word(&current_word));
                    current_word.clear();
                }

                // Handle special characters
                match ch {
                    '"' | '\'' | '`' => {
                        // String literal
                        let quote = ch;
                        let mut string_content = String::new();
                        string_content.push(ch);

                        while let Some(&next) = chars.peek() {
                            string_content.push(chars.next().unwrap());
                            if next == quote && !string_content.ends_with("\\") {
                                break;
                            }
                        }
                        result.push_str("\x1b[32m"); // Green for strings
                        result.push_str(&string_content);
                        result.push_str("\x1b[0m");
                    }
                    '0'..='9' => {
                        // Number
                        let mut number = String::new();
                        number.push(ch);
                        while let Some(&next) = chars.peek() {
                            if next.is_ascii_digit() || next == '.' || next == 'e' || next == 'E' || next == 'x' || next == 'X' || next == 'n' {
                                number.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        result.push_str("\x1b[33m"); // Yellow for numbers
                        result.push_str(&number);
                        result.push_str("\x1b[0m");
                    }
                    '/' if chars.peek() == Some(&'/') => {
                        // Single-line comment
                        result.push_str("\x1b[90m"); // Gray for comments
                        result.push('/');
                        for c in chars.by_ref() {
                            result.push(c);
                        }
                        result.push_str("\x1b[0m");
                    }
                    '(' | ')' | '{' | '}' | '[' | ']' => {
                        result.push_str("\x1b[1m"); // Bold for brackets
                        result.push(ch);
                        result.push_str("\x1b[0m");
                    }
                    _ => {
                        result.push(ch);
                    }
                }
            }
        }

        // Flush any remaining word
        if !current_word.is_empty() {
            result.push_str(&self.highlight_word(&current_word));
        }

        Cow::Owned(result)
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        if self.colors_enabled {
            Cow::Owned(format!("\x1b[90m{}\x1b[0m", hint))
        } else {
            Cow::Borrowed(hint)
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        self.colors_enabled
    }
}

impl ReplHelper {
    fn highlight_word(&self, word: &str) -> String {
        // Check if it's a keyword
        if JS_KEYWORDS.contains(&word) {
            return format!("\x1b[35m{}\x1b[0m", word); // Magenta for keywords
        }

        // Check if it's a built-in global
        if JS_GLOBALS.contains(&word) {
            return format!("\x1b[36m{}\x1b[0m", word); // Cyan for globals
        }

        // Check for boolean/null literals
        if word == "true" || word == "false" || word == "null" || word == "undefined" {
            return format!("\x1b[33m{}\x1b[0m", word); // Yellow for literals
        }

        word.to_string()
    }
}

impl Validator for ReplHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        let input = ctx.input();

        // Check for balanced brackets
        let mut brace_count = 0i32;
        let mut paren_count = 0i32;
        let mut bracket_count = 0i32;
        let mut in_string = false;
        let mut string_char = ' ';
        let mut escape_next = false;

        for ch in input.chars() {
            if escape_next {
                escape_next = false;
                continue;
            }

            if ch == '\\' && in_string {
                escape_next = true;
                continue;
            }

            if !in_string {
                match ch {
                    '"' | '\'' | '`' => {
                        in_string = true;
                        string_char = ch;
                    }
                    '{' => brace_count += 1,
                    '}' => brace_count -= 1,
                    '(' => paren_count += 1,
                    ')' => paren_count -= 1,
                    '[' => bracket_count += 1,
                    ']' => bracket_count -= 1,
                    _ => {}
                }
            } else if ch == string_char {
                in_string = false;
            }
        }

        // If brackets are unbalanced, request more input
        if brace_count > 0 || paren_count > 0 || bracket_count > 0 || in_string {
            Ok(ValidationResult::Incomplete)
        } else {
            Ok(ValidationResult::Valid(None))
        }
    }
}

/// Special REPL commands
#[derive(Debug, Clone, PartialEq)]
pub enum ReplCommand {
    Help,
    Clear,
    Exit,
    Timing(bool),
    Load(String),
    Env,
    Reset,
}

impl ReplCommand {
    fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if !input.starts_with('.') {
            return None;
        }

        let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
        let cmd = parts[0].to_lowercase();
        let arg = parts.get(1).map(|s| s.to_string());

        match cmd.as_str() {
            "help" | "h" => Some(Self::Help),
            "clear" | "cls" => Some(Self::Clear),
            "exit" | "quit" | "q" => Some(Self::Exit),
            "timing" => {
                let enabled = arg.as_deref().unwrap_or("on") != "off";
                Some(Self::Timing(enabled))
            }
            "load" => arg.map(Self::Load),
            "env" => Some(Self::Env),
            "reset" => Some(Self::Reset),
            _ => None,
        }
    }
}

/// The REPL instance
pub struct Repl {
    config: ReplConfig,
    runtime: Runtime,
    editor: Editor<ReplHelper, rustyline::history::DefaultHistory>,
    show_timing: bool,
    user_vars: Rc<RefCell<HashSet<String>>>,
}

impl Repl {
    /// Create a new REPL with default configuration
    pub fn new() -> Result<Self, ReplError> {
        Self::with_config(ReplConfig::default())
    }

    /// Create a new REPL with custom configuration
    pub fn with_config(config: ReplConfig) -> Result<Self, ReplError> {
        let mut editor = Editor::new()
            .map_err(|e| ReplError::InitError(e.to_string()))?;

        let helper = ReplHelper::new(config.colors);
        let user_vars = helper.get_user_vars();
        editor.set_helper(Some(helper));

        // Load history if available
        if let Some(ref history_file) = config.history_file {
            let _ = editor.load_history(history_file);
        }

        Ok(Self {
            config,
            runtime: Runtime::new(),
            editor,
            show_timing: false,
            user_vars,
        })
    }

    /// Run the REPL
    pub fn run(&mut self) -> Result<(), ReplError> {
        self.print_banner();

        loop {
            match self.read_input() {
                Ok(input) => {
                    if input.trim().is_empty() {
                        continue;
                    }

                    // Check for special commands
                    if let Some(cmd) = ReplCommand::parse(&input) {
                        match self.execute_command(cmd) {
                            Ok(true) => continue,
                            Ok(false) => break, // Exit command
                            Err(e) => {
                                self.print_error(&e.to_string());
                                continue;
                            }
                        }
                    }

                    // Evaluate JavaScript
                    self.eval_and_print(&input);
                }
                Err(ReplError::Interrupted) => {
                    println!("\n(To exit, type .exit or press Ctrl+D)");
                }
                Err(ReplError::Eof) => {
                    println!("\nGoodbye!");
                    break;
                }
                Err(e) => {
                    self.print_error(&e.to_string());
                }
            }
        }

        // Save history
        if let Some(ref history_file) = self.config.history_file {
            let _ = self.editor.save_history(history_file);
        }

        Ok(())
    }

    fn print_banner(&self) {
        println!("Quicksilver JavaScript Runtime v{}", crate::VERSION);
        println!("Type .help for available commands\n");
    }

    fn read_input(&mut self) -> Result<String, ReplError> {
        let prompt = &self.config.prompt;
        match self.editor.readline(prompt) {
            Ok(line) => {
                let _ = self.editor.add_history_entry(&line);
                Ok(line)
            }
            Err(ReadlineError::Interrupted) => Err(ReplError::Interrupted),
            Err(ReadlineError::Eof) => Err(ReplError::Eof),
            Err(e) => Err(ReplError::IoError(e.to_string())),
        }
    }

    fn eval_and_print(&mut self, code: &str) {
        // Extract variable names from code for completion
        self.extract_variable_names(code);

        let start = std::time::Instant::now();

        match self.runtime.eval(code) {
            Ok(value) => {
                let elapsed = start.elapsed();

                // Print result
                let output = self.format_value(&value);
                if output != "undefined" {
                    println!("{}", output);
                }

                // Print timing if enabled
                if self.show_timing {
                    println!("\x1b[90m({:.3}ms)\x1b[0m", elapsed.as_secs_f64() * 1000.0);
                }
            }
            Err(e) => {
                self.print_error(&e.to_string());
            }
        }
    }

    /// Extract variable names from code for completion
    fn extract_variable_names(&mut self, code: &str) {
        // Simple regex-like pattern matching for variable declarations
        // Matches: let/const/var identifier, function identifier, class identifier
        let patterns = [
            ("let ", true),
            ("const ", true),
            ("var ", true),
            ("function ", true),
            ("class ", true),
        ];

        for line in code.lines() {
            let trimmed = line.trim();
            for (pattern, _) in &patterns {
                if let Some(rest) = trimmed.strip_prefix(pattern) {
                    // Extract the identifier (first word)
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                        .collect();
                    if !name.is_empty() {
                        self.user_vars.borrow_mut().insert(name);
                    }
                }
            }
        }
    }

    fn format_value(&self, value: &Value) -> String {
        match value {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Boolean(b) => {
                if self.config.colors {
                    format!("\x1b[33m{}\x1b[0m", b)
                } else {
                    b.to_string()
                }
            }
            Value::Number(n) => {
                if self.config.colors {
                    format!("\x1b[33m{}\x1b[0m", n)
                } else {
                    n.to_string()
                }
            }
            Value::String(s) => {
                if self.config.colors {
                    format!("\x1b[32m'{}'\x1b[0m", s)
                } else {
                    format!("'{}'", s)
                }
            }
            Value::Object(obj) => {
                let borrowed = obj.borrow();
                match &borrowed.kind {
                    crate::runtime::ObjectKind::Array(arr) => {
                        let items: Vec<String> = arr.iter()
                            .take(10)
                            .map(|v| self.format_value(v))
                            .collect();
                        let suffix = if arr.len() > 10 {
                            format!(", ... ({} more)", arr.len() - 10)
                        } else {
                            String::new()
                        };
                        format!("[ {}{} ]", items.join(", "), suffix)
                    }
                    crate::runtime::ObjectKind::Function(_) => {
                        if self.config.colors {
                            "\x1b[36m[Function]\x1b[0m".to_string()
                        } else {
                            "[Function]".to_string()
                        }
                    }
                    crate::runtime::ObjectKind::NativeFunction { name, .. } => {
                        if self.config.colors {
                            format!("\x1b[36m[Function: {}]\x1b[0m", name)
                        } else {
                            format!("[Function: {}]", name)
                        }
                    }
                    crate::runtime::ObjectKind::Class { name, .. } => {
                        if self.config.colors {
                            format!("\x1b[36m[Class: {}]\x1b[0m", name)
                        } else {
                            format!("[Class: {}]", name)
                        }
                    }
                    crate::runtime::ObjectKind::Date(ts) => {
                        format!("Date({})", ts)
                    }
                    crate::runtime::ObjectKind::Map(entries) => {
                        format!("Map({})", entries.len())
                    }
                    crate::runtime::ObjectKind::Set(items) => {
                        format!("Set({})", items.len())
                    }
                    crate::runtime::ObjectKind::Error { name, message } => {
                        if self.config.colors {
                            format!("\x1b[31m{}: {}\x1b[0m", name, message)
                        } else {
                            format!("{}: {}", name, message)
                        }
                    }
                    _ => {
                        // Generic object
                        let props = &borrowed.properties;
                        if props.is_empty() {
                            "{}".to_string()
                        } else {
                            let items: Vec<String> = props.iter()
                                .take(5)
                                .map(|(k, v)| format!("{}: {}", k, self.format_value(v)))
                                .collect();
                            let suffix = if props.len() > 5 {
                                format!(", ... ({} more)", props.len() - 5)
                            } else {
                                String::new()
                            };
                            format!("{{ {}{} }}", items.join(", "), suffix)
                        }
                    }
                }
            }
            Value::Symbol(id) => format!("Symbol({})", id),
            Value::BigInt(n) => {
                if self.config.colors {
                    format!("\x1b[33m{}n\x1b[0m", n)
                } else {
                    format!("{}n", n)
                }
            }
        }
    }

    fn print_error(&self, message: &str) {
        if self.config.colors {
            println!("\x1b[31mError: {}\x1b[0m", message);
        } else {
            println!("Error: {}", message);
        }
    }

    fn execute_command(&mut self, cmd: ReplCommand) -> Result<bool, ReplError> {
        match cmd {
            ReplCommand::Help => {
                self.print_help();
                Ok(true)
            }
            ReplCommand::Clear => {
                // Clear screen
                print!("\x1b[2J\x1b[H");
                Ok(true)
            }
            ReplCommand::Exit => Ok(false),
            ReplCommand::Timing(enabled) => {
                self.show_timing = enabled;
                println!("Timing display: {}", if enabled { "on" } else { "off" });
                Ok(true)
            }
            ReplCommand::Load(path) => {
                let code = std::fs::read_to_string(&path)
                    .map_err(|e| ReplError::IoError(e.to_string()))?;
                self.eval_and_print(&code);
                Ok(true)
            }
            ReplCommand::Env => {
                println!("Runtime environment:");
                println!("  Version: {}", crate::VERSION);
                println!("  Platform: {}", std::env::consts::OS);
                println!("  Arch: {}", std::env::consts::ARCH);
                Ok(true)
            }
            ReplCommand::Reset => {
                self.runtime = Runtime::new();
                self.user_vars.borrow_mut().clear();
                println!("Runtime reset.");
                Ok(true)
            }
        }
    }

    fn print_help(&self) {
        println!("Available commands:");
        println!("  .help, .h       Show this help message");
        println!("  .clear, .cls    Clear the screen");
        println!("  .exit, .quit    Exit the REPL");
        println!("  .timing [on|off] Toggle execution timing");
        println!("  .load <file>    Load and execute a JavaScript file");
        println!("  .env            Show environment info");
        println!("  .reset          Reset the runtime state");
        println!();
        println!("Features:");
        println!("  - Tab completion for keywords, globals, and methods");
        println!("  - Syntax highlighting as you type");
        println!("  - Inline hints for completions");
        println!("  - Command history (persisted across sessions)");
        println!();
        println!("Tips:");
        println!("  - Press Tab to complete keywords and identifiers");
        println!("  - Press Ctrl+C to cancel current input");
        println!("  - Press Ctrl+D to exit");
        println!("  - Use arrow keys to navigate history");
        println!("  - Multi-line input is supported (unbalanced braces continue input)");
    }
}

/// REPL errors
#[derive(Debug)]
pub enum ReplError {
    InitError(String),
    IoError(String),
    Interrupted,
    Eof,
}

impl std::fmt::Display for ReplError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitError(msg) => write!(f, "initialization error: {}", msg),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
            Self::Interrupted => write!(f, "interrupted"),
            Self::Eof => write!(f, "end of input"),
        }
    }
}

impl std::error::Error for ReplError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_parsing() {
        assert_eq!(ReplCommand::parse(".help"), Some(ReplCommand::Help));
        assert_eq!(ReplCommand::parse(".exit"), Some(ReplCommand::Exit));
        assert_eq!(ReplCommand::parse(".clear"), Some(ReplCommand::Clear));
        assert_eq!(ReplCommand::parse(".timing on"), Some(ReplCommand::Timing(true)));
        assert_eq!(ReplCommand::parse(".timing off"), Some(ReplCommand::Timing(false)));
        assert_eq!(ReplCommand::parse(".load foo.js"), Some(ReplCommand::Load("foo.js".to_string())));
        assert_eq!(ReplCommand::parse("notacommand"), None);
    }

    #[test]
    fn test_repl_config_default() {
        let config = ReplConfig::default();
        assert_eq!(config.prompt, "qs> ");
        assert!(config.colors);
        assert!(!config.show_timing);
    }
}
