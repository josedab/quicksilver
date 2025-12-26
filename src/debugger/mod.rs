//! Time-Travel Debugger for Quicksilver
//!
//! This module provides a revolutionary debugging experience that allows
//! developers to step backwards through code execution, not just forwards.
//!
//! # Features
//! - Record execution history
//! - Step forward and backward through execution
//! - Inspect variables at any point in time
//! - Set breakpoints
//! - Find when variables changed
//!
//! # Example
//! ```text
//! quicksilver debug script.js
//! (ttdb) back    # Step backward in time
//! (ttdb) next    # Step forward
//! (ttdb) p x     # Print variable x
//! ```

use crate::bytecode::Opcode;
use crate::Value;
use rustc_hash::FxHashMap as HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

/// Maximum number of execution records to keep in history
const MAX_HISTORY_SIZE: usize = 10000;

/// Magic bytes for debug recording files
const RECORDING_MAGIC: &[u8; 4] = b"TTDR";

/// Recording file version
const RECORDING_VERSION: u32 = 1;

/// A snapshot of VM state at a single point in execution
#[derive(Clone)]
pub struct ExecutionRecord {
    /// Unique step number
    pub step: u64,
    /// The opcode that was executed
    pub opcode: Option<Opcode>,
    /// Instruction pointer before execution
    pub ip: usize,
    /// Source line number
    pub line: u32,
    /// Stack snapshot (top 10 values)
    pub stack_top: Vec<Value>,
    /// Stack size
    pub stack_size: usize,
    /// Local variables snapshot
    pub locals: HashMap<String, Value>,
    /// Global variables that changed
    pub globals_changed: Vec<(String, Value)>,
    /// Description of what happened
    pub description: String,
}

/// Breakpoint configuration
#[derive(Clone)]
pub struct Breakpoint {
    /// Line number to break at
    pub line: u32,
    /// Optional condition expression
    pub condition: Option<String>,
    /// Is this breakpoint enabled?
    pub enabled: bool,
    /// Hit count
    pub hit_count: u64,
}

/// The Time-Travel Debugger
#[derive(Clone)]
pub struct TimeTravelDebugger {
    /// Execution history
    history: Vec<ExecutionRecord>,
    /// Current position in history (for replay)
    current_position: usize,
    /// Is recording enabled?
    recording: bool,
    /// Breakpoints by line number
    breakpoints: HashMap<u32, Breakpoint>,
    /// Next breakpoint ID
    next_bp_id: u32,
    /// Watch expressions
    watches: Vec<String>,
    /// Step counter
    step_counter: u64,
    /// Is debugger paused?
    paused: bool,
    /// Source code lines (for display)
    source_lines: Vec<String>,
    /// Current file being debugged
    current_file: String,
}

impl TimeTravelDebugger {
    /// Create a new debugger instance
    pub fn new() -> Self {
        Self {
            history: Vec::with_capacity(MAX_HISTORY_SIZE),
            current_position: 0,
            recording: true,
            breakpoints: HashMap::default(),
            next_bp_id: 1,
            watches: Vec::new(),
            step_counter: 0,
            paused: false,
            source_lines: Vec::new(),
            current_file: String::new(),
        }
    }

    /// Load source code for display
    pub fn load_source(&mut self, filename: &str, source: &str) {
        self.current_file = filename.to_string();
        self.source_lines = source.lines().map(|s| s.to_string()).collect();
    }

    /// Record an execution step
    pub fn record_step(
        &mut self,
        opcode: Option<Opcode>,
        ip: usize,
        line: u32,
        stack: &[Value],
        locals: &HashMap<String, Value>,
        description: &str,
    ) {
        if !self.recording {
            return;
        }

        self.step_counter += 1;

        let record = ExecutionRecord {
            step: self.step_counter,
            opcode,
            ip,
            line,
            stack_top: stack.iter().rev().take(10).cloned().collect(),
            stack_size: stack.len(),
            locals: locals.clone(),
            globals_changed: Vec::new(),
            description: description.to_string(),
        };

        // Maintain max history size
        if self.history.len() >= MAX_HISTORY_SIZE {
            self.history.remove(0);
        }

        self.history.push(record);
        self.current_position = self.history.len() - 1;
    }

    /// Check if we should break at this line
    pub fn should_break(&mut self, line: u32) -> bool {
        if let Some(bp) = self.breakpoints.get_mut(&line) {
            if bp.enabled {
                bp.hit_count += 1;
                return true;
            }
        }
        false
    }

    /// Step backward in execution history
    pub fn step_back(&mut self) -> Option<&ExecutionRecord> {
        if self.current_position > 0 {
            self.current_position -= 1;
            Some(&self.history[self.current_position])
        } else {
            None
        }
    }

    /// Step forward in execution history
    pub fn step_forward(&mut self) -> Option<&ExecutionRecord> {
        if self.current_position < self.history.len() - 1 {
            self.current_position += 1;
            Some(&self.history[self.current_position])
        } else {
            None
        }
    }

    /// Get current execution record
    pub fn current(&self) -> Option<&ExecutionRecord> {
        self.history.get(self.current_position)
    }

    /// Jump to a specific step
    pub fn jump_to(&mut self, step: u64) -> Option<&ExecutionRecord> {
        for (i, record) in self.history.iter().enumerate() {
            if record.step == step {
                self.current_position = i;
                return Some(record);
            }
        }
        None
    }

    /// Find when a variable changed
    pub fn find_variable_changes(&self, var_name: &str) -> Vec<(u64, Value)> {
        let mut changes = Vec::new();
        let mut last_value: Option<Value> = None;

        for record in &self.history {
            if let Some(value) = record.locals.get(var_name) {
                let changed = match &last_value {
                    None => true,
                    Some(v) => !v.strict_equals(value),
                };

                if changed {
                    changes.push((record.step, value.clone()));
                    last_value = Some(value.clone());
                }
            }
        }

        changes
    }

    /// Add a breakpoint
    pub fn add_breakpoint(&mut self, line: u32, condition: Option<String>) -> u32 {
        let id = self.next_bp_id;
        self.next_bp_id += 1;

        self.breakpoints.insert(
            line,
            Breakpoint {
                line,
                condition,
                enabled: true,
                hit_count: 0,
            },
        );

        id
    }

    /// Remove a breakpoint
    pub fn remove_breakpoint(&mut self, line: u32) -> bool {
        self.breakpoints.remove(&line).is_some()
    }

    /// Add a watch expression
    pub fn add_watch(&mut self, expr: String) {
        self.watches.push(expr);
    }

    /// Get execution statistics
    pub fn stats(&self) -> DebuggerStats {
        DebuggerStats {
            total_steps: self.step_counter,
            history_size: self.history.len(),
            breakpoint_count: self.breakpoints.len(),
            watch_count: self.watches.len(),
        }
    }

    /// Interactive debugger REPL
    pub fn run_interactive(&mut self) {
        println!();
        println!("╔═══════════════════════════════════════════════════════════════╗");
        println!("║       🕐 Quicksilver Time-Travel Debugger                     ║");
        println!("╠═══════════════════════════════════════════════════════════════╣");
        println!("║  Commands:                                                    ║");
        println!("║    n, next      - Step forward                               ║");
        println!("║    b, back      - Step backward ⭐                           ║");
        println!("║    c, continue  - Continue to next breakpoint                ║");
        println!("║    p <expr>     - Print expression/variable                  ║");
        println!("║    w <var>      - Watch variable                             ║");
        println!("║    bp <line>    - Set breakpoint at line                     ║");
        println!("║    history      - Show execution history                     ║");
        println!("║    rewind <n>   - Rewind n steps                             ║");
        println!("║    changes <v>  - Show when variable v changed               ║");
        println!("║    stack        - Show current stack                         ║");
        println!("║    locals       - Show local variables                       ║");
        println!("║    help         - Show this help                             ║");
        println!("║    quit         - Exit debugger                              ║");
        println!("╚═══════════════════════════════════════════════════════════════╝");
        println!();

        self.show_current_state();

        let stdin = io::stdin();
        let mut stdout = io::stdout();

        loop {
            print!("(ttdb) ");
            stdout.flush().unwrap();

            let mut input = String::new();
            if stdin.lock().read_line(&mut input).is_err() {
                break;
            }

            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            let parts: Vec<&str> = input.split_whitespace().collect();
            let command = parts[0];
            let args: Vec<&str> = parts[1..].to_vec();

            match command {
                "n" | "next" => {
                    if let Some(record) = self.step_forward() {
                        let (step, line, desc) = (record.step, record.line, record.description.clone());
                        println!("→ Step {} (line {}): {}", step, line, desc);
                        self.show_source_context(line);
                    } else {
                        println!("⚠ Already at the latest step");
                    }
                }
                "b" | "back" => {
                    if let Some(record) = self.step_back() {
                        let (step, line, desc) = (record.step, record.line, record.description.clone());
                        println!("← Rewinding to step {} (line {}): {}", step, line, desc);
                        self.show_source_context(line);
                    } else {
                        println!("⚠ Already at the beginning of history");
                    }
                }
                "rewind" => {
                    if let Some(n_str) = args.first() {
                        if let Ok(n) = n_str.parse::<usize>() {
                            for _ in 0..n {
                                if self.step_back().is_none() {
                                    println!("⚠ Reached beginning of history");
                                    break;
                                }
                            }
                            if let Some(record) = self.current() {
                                let (step, line) = (record.step, record.line);
                                println!("← Rewound to step {} (line {})", step, line);
                                self.show_source_context(line);
                            }
                        }
                    } else {
                        println!("Usage: rewind <n>");
                    }
                }
                "p" | "print" => {
                    if let Some(var_name) = args.first() {
                        if let Some(record) = self.current() {
                            if let Some(value) = record.locals.get(*var_name) {
                                println!("{} = {}", var_name, value.to_js_string());
                            } else {
                                println!("{} is not defined in current scope", var_name);
                            }
                        }
                    } else {
                        println!("Usage: p <variable>");
                    }
                }
                "w" | "watch" => {
                    if let Some(var_name) = args.first() {
                        self.add_watch(var_name.to_string());
                        println!("Watching: {}", var_name);
                    } else {
                        println!("Current watches:");
                        for (i, w) in self.watches.iter().enumerate() {
                            println!("  {}: {}", i + 1, w);
                        }
                    }
                }
                "bp" | "breakpoint" => {
                    if let Some(line_str) = args.first() {
                        if let Ok(line) = line_str.parse::<u32>() {
                            self.add_breakpoint(line, None);
                            println!("Breakpoint set at line {}", line);
                        }
                    } else {
                        println!("Breakpoints:");
                        for (line, bp) in &self.breakpoints {
                            let status = if bp.enabled { "enabled" } else { "disabled" };
                            println!("  Line {}: {} (hit {} times)", line, status, bp.hit_count);
                        }
                    }
                }
                "history" => {
                    let start = self.current_position.saturating_sub(5);
                    let end = (self.current_position + 6).min(self.history.len());

                    println!("Execution history:");
                    for i in start..end {
                        let record = &self.history[i];
                        let marker = if i == self.current_position { "→" } else { " " };
                        println!("{} [{}] Line {}: {}", marker, record.step, record.line, record.description);
                    }
                }
                "changes" => {
                    if let Some(var_name) = args.first() {
                        let changes = self.find_variable_changes(var_name);
                        if changes.is_empty() {
                            println!("No changes found for '{}'", var_name);
                        } else {
                            println!("Changes to '{}':", var_name);
                            for (step, value) in changes {
                                println!("  Step {}: {}", step, value.to_js_string());
                            }
                        }
                    } else {
                        println!("Usage: changes <variable>");
                    }
                }
                "stack" => {
                    if let Some(record) = self.current() {
                        println!("Stack (size={}):", record.stack_size);
                        for (i, val) in record.stack_top.iter().enumerate() {
                            println!("  [{}]: {}", i, val.to_js_string());
                        }
                    }
                }
                "locals" => {
                    if let Some(record) = self.current() {
                        println!("Local variables:");
                        for (name, value) in &record.locals {
                            println!("  {} = {}", name, value.to_js_string());
                        }
                    }
                }
                "help" => {
                    println!("Time-Travel Debugger Commands:");
                    println!("  n, next       - Step forward in execution");
                    println!("  b, back       - Step backward in execution ⭐");
                    println!("  rewind <n>    - Go back n steps");
                    println!("  p <var>       - Print variable value");
                    println!("  w <var>       - Add watch on variable");
                    println!("  bp <line>     - Set breakpoint at line");
                    println!("  history       - Show execution history");
                    println!("  changes <var> - Show when variable changed");
                    println!("  stack         - Show current stack");
                    println!("  locals        - Show local variables");
                    println!("  quit          - Exit debugger");
                }
                "q" | "quit" | "exit" => {
                    println!("Exiting debugger.");
                    break;
                }
                "c" | "continue" => {
                    println!("Continuing execution...");
                    self.paused = false;
                    break;
                }
                _ => {
                    println!("Unknown command: {}. Type 'help' for available commands.", command);
                }
            }
        }
    }

    fn show_current_state(&self) {
        if let Some(record) = self.current() {
            println!("📍 Step {} | Line {} | Stack size: {}",
                record.step, record.line, record.stack_size);
            self.show_source_context(record.line);
        }
    }

    fn show_source_context(&self, line: u32) {
        if self.source_lines.is_empty() {
            return;
        }

        let line_idx = line as usize;
        let start = line_idx.saturating_sub(2);
        let end = (line_idx + 3).min(self.source_lines.len());

        println!();
        for i in start..end {
            let marker = if i + 1 == line_idx { "→" } else { " " };
            let line_content = self.source_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            println!("{} {:4} │ {}", marker, i + 1, line_content);
        }
        println!();
    }

    /// Get history length
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Pause the debugger
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Check if paused
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    // ============================================================
    // Record/Replay Functionality
    // ============================================================

    /// Save the current debug session to a file
    pub fn save_recording<P: AsRef<Path>>(&self, path: P) -> Result<(), RecordingError> {
        let file = File::create(path)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        let mut writer = BufWriter::new(file);

        // Write magic and version
        writer.write_all(RECORDING_MAGIC)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        writer.write_all(&RECORDING_VERSION.to_le_bytes())
            .map_err(|e| RecordingError::IoError(e.to_string()))?;

        // Write metadata
        write_string(&mut writer, &self.current_file)?;
        writer.write_all(&(self.source_lines.len() as u32).to_le_bytes())
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        for line in &self.source_lines {
            write_string(&mut writer, line)?;
        }

        // Write history
        writer.write_all(&(self.history.len() as u32).to_le_bytes())
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        for record in &self.history {
            write_record(&mut writer, record)?;
        }

        // Write breakpoints
        writer.write_all(&(self.breakpoints.len() as u32).to_le_bytes())
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        for (line, bp) in &self.breakpoints {
            writer.write_all(&line.to_le_bytes())
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            writer.write_all(&[bp.enabled as u8])
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            writer.write_all(&bp.hit_count.to_le_bytes())
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            let has_condition = bp.condition.is_some();
            writer.write_all(&[has_condition as u8])
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            if let Some(ref cond) = bp.condition {
                write_string(&mut writer, cond)?;
            }
        }

        writer.flush()
            .map_err(|e| RecordingError::IoError(e.to_string()))?;

        Ok(())
    }

    /// Load a debug session from a file
    pub fn load_recording<P: AsRef<Path>>(path: P) -> Result<Self, RecordingError> {
        let file = File::open(path)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        let mut reader = BufReader::new(file);

        // Read and verify magic
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        if &magic != RECORDING_MAGIC {
            return Err(RecordingError::InvalidFormat("Invalid recording file".to_string()));
        }

        // Read version
        let mut version_bytes = [0u8; 4];
        reader.read_exact(&mut version_bytes)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        let version = u32::from_le_bytes(version_bytes);
        if version > RECORDING_VERSION {
            return Err(RecordingError::UnsupportedVersion(version));
        }

        // Read metadata
        let current_file = read_string(&mut reader)?;
        let mut source_count_bytes = [0u8; 4];
        reader.read_exact(&mut source_count_bytes)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        let source_count = u32::from_le_bytes(source_count_bytes) as usize;
        let mut source_lines = Vec::with_capacity(source_count);
        for _ in 0..source_count {
            source_lines.push(read_string(&mut reader)?);
        }

        // Read history
        let mut history_count_bytes = [0u8; 4];
        reader.read_exact(&mut history_count_bytes)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        let history_count = u32::from_le_bytes(history_count_bytes) as usize;
        let mut history = Vec::with_capacity(history_count);
        for _ in 0..history_count {
            history.push(read_record(&mut reader)?);
        }

        // Read breakpoints
        let mut bp_count_bytes = [0u8; 4];
        reader.read_exact(&mut bp_count_bytes)
            .map_err(|e| RecordingError::IoError(e.to_string()))?;
        let bp_count = u32::from_le_bytes(bp_count_bytes) as usize;
        let mut breakpoints = HashMap::with_capacity_and_hasher(bp_count, Default::default());
        for _ in 0..bp_count {
            let mut line_bytes = [0u8; 4];
            reader.read_exact(&mut line_bytes)
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            let line = u32::from_le_bytes(line_bytes);

            let mut enabled_byte = [0u8; 1];
            reader.read_exact(&mut enabled_byte)
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            let enabled = enabled_byte[0] != 0;

            let mut hit_count_bytes = [0u8; 8];
            reader.read_exact(&mut hit_count_bytes)
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            let hit_count = u64::from_le_bytes(hit_count_bytes);

            let mut has_condition_byte = [0u8; 1];
            reader.read_exact(&mut has_condition_byte)
                .map_err(|e| RecordingError::IoError(e.to_string()))?;
            let condition = if has_condition_byte[0] != 0 {
                Some(read_string(&mut reader)?)
            } else {
                None
            };

            breakpoints.insert(line, Breakpoint {
                line,
                condition,
                enabled,
                hit_count,
            });
        }

        let step_counter = history.last().map(|r| r.step).unwrap_or(0);
        let current_position = if history.is_empty() { 0 } else { history.len() - 1 };
        let next_bp_id = (breakpoints.len() + 1) as u32;

        Ok(Self {
            history,
            current_position,
            recording: false, // Loaded recordings start in replay mode
            breakpoints,
            next_bp_id,
            watches: Vec::new(),
            step_counter,
            paused: true,
            source_lines,
            current_file,
        })
    }

    /// Replay mode - step through recorded execution
    pub fn replay_step_forward(&mut self) -> Option<&ExecutionRecord> {
        if self.current_position < self.history.len() - 1 {
            self.current_position += 1;
            Some(&self.history[self.current_position])
        } else {
            None
        }
    }

    /// Replay mode - step through recorded execution backwards
    pub fn replay_step_back(&mut self) -> Option<&ExecutionRecord> {
        if self.current_position > 0 {
            self.current_position -= 1;
            Some(&self.history[self.current_position])
        } else {
            None
        }
    }

    /// Reset replay to beginning
    pub fn replay_reset(&mut self) {
        self.current_position = 0;
    }

    /// Jump to end of recording
    pub fn replay_end(&mut self) {
        if !self.history.is_empty() {
            self.current_position = self.history.len() - 1;
        }
    }

    /// Get recording info
    pub fn recording_info(&self) -> RecordingInfo {
        RecordingInfo {
            filename: self.current_file.clone(),
            total_steps: self.step_counter,
            history_size: self.history.len(),
            source_lines: self.source_lines.len(),
            breakpoint_count: self.breakpoints.len(),
            current_position: self.current_position,
        }
    }
}

/// Recording error types
#[derive(Debug, Clone)]
pub enum RecordingError {
    IoError(String),
    InvalidFormat(String),
    UnsupportedVersion(u32),
}

impl std::fmt::Display for RecordingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
            Self::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            Self::UnsupportedVersion(v) => write!(f, "Unsupported version: {}", v),
        }
    }
}

impl std::error::Error for RecordingError {}

/// Information about a recording
#[derive(Debug, Clone)]
pub struct RecordingInfo {
    pub filename: String,
    pub total_steps: u64,
    pub history_size: usize,
    pub source_lines: usize,
    pub breakpoint_count: usize,
    pub current_position: usize,
}

// Helper functions for serialization

fn write_string<W: Write>(writer: &mut W, s: &str) -> Result<(), RecordingError> {
    let bytes = s.as_bytes();
    writer.write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    writer.write_all(bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    Ok(())
}

fn read_string<R: Read>(reader: &mut R) -> Result<String, RecordingError> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    let len = u32::from_le_bytes(len_bytes) as usize;

    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;

    String::from_utf8(bytes)
        .map_err(|e| RecordingError::InvalidFormat(e.to_string()))
}

fn write_record<W: Write>(writer: &mut W, record: &ExecutionRecord) -> Result<(), RecordingError> {
    // Write step and ip
    writer.write_all(&record.step.to_le_bytes())
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    writer.write_all(&(record.ip as u32).to_le_bytes())
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    writer.write_all(&record.line.to_le_bytes())
        .map_err(|e| RecordingError::IoError(e.to_string()))?;

    // Write opcode
    let opcode_byte = record.opcode.map(|o| o as u8).unwrap_or(255);
    writer.write_all(&[opcode_byte])
        .map_err(|e| RecordingError::IoError(e.to_string()))?;

    // Write stack info
    writer.write_all(&(record.stack_size as u32).to_le_bytes())
        .map_err(|e| RecordingError::IoError(e.to_string()))?;

    // Write description
    write_string(writer, &record.description)?;

    // Write locals count (simplified - just names for replay)
    writer.write_all(&(record.locals.len() as u32).to_le_bytes())
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    for (name, value) in &record.locals {
        write_string(writer, name)?;
        write_string(writer, &value.to_js_string())?;
    }

    Ok(())
}

fn read_record<R: Read>(reader: &mut R) -> Result<ExecutionRecord, RecordingError> {
    let mut step_bytes = [0u8; 8];
    reader.read_exact(&mut step_bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    let step = u64::from_le_bytes(step_bytes);

    let mut ip_bytes = [0u8; 4];
    reader.read_exact(&mut ip_bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    let ip = u32::from_le_bytes(ip_bytes) as usize;

    let mut line_bytes = [0u8; 4];
    reader.read_exact(&mut line_bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    let line = u32::from_le_bytes(line_bytes);

    let mut opcode_byte = [0u8; 1];
    reader.read_exact(&mut opcode_byte)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    let opcode = if opcode_byte[0] == 255 {
        None
    } else {
        Opcode::from_u8(opcode_byte[0])
    };

    let mut stack_size_bytes = [0u8; 4];
    reader.read_exact(&mut stack_size_bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    let stack_size = u32::from_le_bytes(stack_size_bytes) as usize;

    let description = read_string(reader)?;

    let mut locals_count_bytes = [0u8; 4];
    reader.read_exact(&mut locals_count_bytes)
        .map_err(|e| RecordingError::IoError(e.to_string()))?;
    let locals_count = u32::from_le_bytes(locals_count_bytes) as usize;

    let mut locals = HashMap::with_capacity_and_hasher(locals_count, Default::default());
    for _ in 0..locals_count {
        let name = read_string(reader)?;
        let value_str = read_string(reader)?;
        // Simplified: store as string value for replay
        locals.insert(name, Value::String(value_str));
    }

    Ok(ExecutionRecord {
        step,
        opcode,
        ip,
        line,
        stack_top: Vec::new(), // Not stored in recording
        stack_size,
        locals,
        globals_changed: Vec::new(),
        description,
    })
}

impl Default for TimeTravelDebugger {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the debugger
pub struct DebuggerStats {
    pub total_steps: u64,
    pub history_size: usize,
    pub breakpoint_count: usize,
    pub watch_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debugger_creation() {
        let debugger = TimeTravelDebugger::new();
        assert_eq!(debugger.history_len(), 0);
        assert!(debugger.current().is_none());
    }

    #[test]
    fn test_record_and_navigate() {
        let mut debugger = TimeTravelDebugger::new();

        // Record some steps
        debugger.record_step(None, 0, 1, &[], &HashMap::default(), "Step 1");
        debugger.record_step(None, 1, 2, &[], &HashMap::default(), "Step 2");
        debugger.record_step(None, 2, 3, &[], &HashMap::default(), "Step 3");

        assert_eq!(debugger.history_len(), 3);

        // Should be at step 3
        assert_eq!(debugger.current().unwrap().step, 3);

        // Step back
        let record = debugger.step_back().unwrap();
        assert_eq!(record.step, 2);

        // Step back again
        let record = debugger.step_back().unwrap();
        assert_eq!(record.step, 1);

        // Can't go back further
        assert!(debugger.step_back().is_none());

        // Step forward
        let record = debugger.step_forward().unwrap();
        assert_eq!(record.step, 2);
    }

    #[test]
    fn test_breakpoints() {
        let mut debugger = TimeTravelDebugger::new();

        debugger.add_breakpoint(10, None);
        assert!(debugger.should_break(10));
        assert!(!debugger.should_break(5));
    }

    #[test]
    fn test_variable_changes() {
        let mut debugger = TimeTravelDebugger::new();

        let mut locals1 = HashMap::default();
        locals1.insert("x".to_string(), Value::Number(1.0));

        let mut locals2 = HashMap::default();
        locals2.insert("x".to_string(), Value::Number(2.0));

        let mut locals3 = HashMap::default();
        locals3.insert("x".to_string(), Value::Number(2.0)); // Same as before

        let mut locals4 = HashMap::default();
        locals4.insert("x".to_string(), Value::Number(5.0));

        debugger.record_step(None, 0, 1, &[], &locals1, "");
        debugger.record_step(None, 1, 2, &[], &locals2, "");
        debugger.record_step(None, 2, 3, &[], &locals3, "");
        debugger.record_step(None, 3, 4, &[], &locals4, "");

        let changes = debugger.find_variable_changes("x");
        assert_eq!(changes.len(), 3); // Initial, 1->2, 2->5
    }
}
