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
//! - Debug Adapter Protocol (DAP) support for IDE integration
//! - Web-based debugger UI
//! - Call stack tracking
//! - Variable diff view
//!
//! # Example
//! ```text
//! quicksilver debug script.js
//! (ttdb) back    # Step backward in time
//! (ttdb) next    # Step forward
//! (ttdb) p x     # Print variable x
//! ```
//!
//! # Web UI
//! ```text
//! quicksilver debug --serve script.js
//! # Opens http://localhost:9229 with interactive debugger UI
//! ```
//!
//! # VS Code Integration
//! The debugger supports the Debug Adapter Protocol (DAP), allowing integration
//! with VS Code and other compatible IDEs. Key features:
//! - Step backward (time-travel!)
//! - Reverse continue
//! - Variable inspection at any point in time
//! - Find when variables changed

//!
//! **Status:** ✅ Complete — Time-travel debugger with DAP protocol and TUI

pub mod dap;
pub mod server;
pub mod sourcemap;
pub mod ui;

pub use dap::{DAPServer, DAPMessage, DAPRequest, DAPResponse, DAPEvent, Capabilities};
pub use server::{DebugServer, DEFAULT_PORT};
pub use sourcemap::{SourceMap, SourceMapRegistry, OriginalPosition, GeneratedPosition};
pub use ui::{DebuggerUI, CallStack, CallFrame, VariableDiff, ChangeType, Timeline, ExecutionHeatMap};

use crate::bytecode::Opcode;
use crate::Value;
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::{Hash, Hasher};
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

/// Enhanced breakpoint with conditions and logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalBreakpoint {
    pub line: usize,
    pub condition: Option<String>,
    pub hit_condition: Option<HitCondition>,
    pub log_message: Option<String>,
    pub enabled: bool,
    pub hit_count: u64,
}

/// Hit condition for conditional breakpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HitCondition {
    /// Break when hit count equals N
    Equal(u64),
    /// Break when hit count is multiple of N
    Multiple(u64),
    /// Break when hit count >= N
    GreaterEqual(u64),
}

/// Watch expression that tracks a variable or expression over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchExpression {
    pub id: usize,
    pub expression: String,
    pub current_value: Option<String>,
    pub history: Vec<WatchHistoryEntry>,
}

/// A single entry in a watch expression's history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchHistoryEntry {
    pub step: usize,
    pub value: String,
    pub changed: bool,
}

/// Recording metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMetadata {
    pub format_version: u32,
    pub source_file: Option<String>,
    pub total_steps: usize,
    pub start_time: u64,
    pub end_time: u64,
    pub breakpoint_count: usize,
    pub watch_count: usize,
    pub checksum: u64,
}

/// Delta-based execution record (stores only changes from previous step)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaRecord {
    pub step: usize,
    pub opcode: String,
    pub ip: usize,
    pub line: Option<usize>,
    /// Only variables that changed from previous step
    pub changed_locals: Vec<(String, String)>,
    pub changed_globals: Vec<(String, String)>,
    /// Stack changes (push/pop operations)
    pub stack_delta: StackDelta,
}

/// Represents changes to the stack between steps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StackDelta {
    Push(Vec<String>),
    Pop(usize),
    Replace(Vec<String>),
    NoChange,
}

/// Full state reconstructed from delta records
#[derive(Debug, Clone)]
pub struct ReconstructedState {
    pub step: usize,
    pub locals: std::collections::HashMap<String, String>,
    pub globals: std::collections::HashMap<String, String>,
    pub stack: Vec<String>,
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
    /// Conditional breakpoints (by id)
    conditional_breakpoints: Vec<ConditionalBreakpoint>,
    /// Next conditional breakpoint ID
    next_cond_bp_id: usize,
    /// Structured watch expressions
    watch_expressions: Vec<WatchExpression>,
    /// Next watch expression ID
    next_watch_id: usize,
    /// Delta-based execution records
    delta_history: Vec<DeltaRecord>,
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
            conditional_breakpoints: Vec::new(),
            next_cond_bp_id: 1,
            watch_expressions: Vec::new(),
            next_watch_id: 1,
            delta_history: Vec::new(),
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
            conditional_breakpoints: Vec::new(),
            next_cond_bp_id: 1,
            watch_expressions: Vec::new(),
            next_watch_id: 1,
            delta_history: Vec::new(),
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

    // ============================================================
    // Conditional Breakpoints
    // ============================================================

    /// Add a conditional breakpoint, returns its id
    pub fn add_conditional_breakpoint(&mut self, bp: ConditionalBreakpoint) -> usize {
        let id = self.next_cond_bp_id;
        self.next_cond_bp_id += 1;
        self.conditional_breakpoints.push(bp);
        id
    }

    /// Check if execution should break at the given line
    pub fn should_break_at_line(&self, line: usize, variables: &std::collections::HashMap<String, String>) -> bool {
        for bp in &self.conditional_breakpoints {
            if bp.line != line || !bp.enabled {
                continue;
            }

            // Evaluate condition if present
            if let Some(ref cond) = bp.condition {
                if !evaluate_simple_condition(cond, variables) {
                    continue;
                }
            }

            // Evaluate hit condition if present
            if let Some(ref hc) = bp.hit_condition {
                let count = bp.hit_count;
                match hc {
                    HitCondition::Equal(n) => {
                        if count != *n {
                            continue;
                        }
                    }
                    HitCondition::Multiple(n) => {
                        if *n == 0 || count % *n != 0 {
                            continue;
                        }
                    }
                    HitCondition::GreaterEqual(n) => {
                        if count < *n {
                            continue;
                        }
                    }
                }
            }

            return true;
        }
        false
    }

    /// Update a conditional breakpoint by id (1-based)
    pub fn update_breakpoint(&mut self, id: usize, bp: ConditionalBreakpoint) {
        // IDs are 1-based, issued sequentially
        if let Some(existing) = self.conditional_breakpoints.iter_mut().find(|_| {
            false // placeholder
        }) {
            let _ = existing;
        }
        // Find by position: id maps to index (id - 1) if within range, else search
        for (i, existing) in self.conditional_breakpoints.iter_mut().enumerate() {
            if i + 1 == id {
                *existing = bp;
                return;
            }
        }
    }

    // ============================================================
    // Watch Expressions
    // ============================================================

    /// Add a watch expression, returns its id
    pub fn add_watch_expression(&mut self, expr: &str) -> usize {
        let id = self.next_watch_id;
        self.next_watch_id += 1;
        self.watch_expressions.push(WatchExpression {
            id,
            expression: expr.to_string(),
            current_value: None,
            history: Vec::new(),
        });
        id
    }

    /// Remove a watch expression by id
    pub fn remove_watch_expression(&mut self, id: usize) -> bool {
        let len_before = self.watch_expressions.len();
        self.watch_expressions.retain(|w| w.id != id);
        self.watch_expressions.len() < len_before
    }

    /// Update all watches with current variable state
    pub fn update_watches(&mut self, step: usize, variables: &std::collections::HashMap<String, String>) {
        for watch in &mut self.watch_expressions {
            let new_value = variables.get(&watch.expression).cloned();
            let value_str = new_value.clone().unwrap_or_else(|| "undefined".to_string());

            let changed = watch.current_value.as_ref() != Some(&value_str);
            watch.history.push(WatchHistoryEntry {
                step,
                value: value_str.clone(),
                changed,
            });
            watch.current_value = Some(value_str);
        }
    }

    /// Get the history for a specific watch expression
    pub fn get_watch_history(&self, id: usize) -> Option<&[WatchHistoryEntry]> {
        self.watch_expressions
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.history.as_slice())
    }

    /// Get all watch expressions
    pub fn watches(&self) -> &[WatchExpression] {
        &self.watch_expressions
    }

    // ============================================================
    // Recording Metadata & Export
    // ============================================================

    /// Generate recording metadata
    pub fn recording_metadata(&self) -> RecordingMetadata {
        let start_time = self.history.first().map(|r| r.step).unwrap_or(0);
        let end_time = self.history.last().map(|r| r.step).unwrap_or(0);

        // Compute a simple checksum over step numbers
        let mut hasher = DefaultHasher::new();
        for record in &self.history {
            record.step.hash(&mut hasher);
            record.line.hash(&mut hasher);
        }
        let checksum = hasher.finish();

        RecordingMetadata {
            format_version: RECORDING_VERSION,
            source_file: if self.current_file.is_empty() {
                None
            } else {
                Some(self.current_file.clone())
            },
            total_steps: self.history.len(),
            start_time,
            end_time,
            breakpoint_count: self.breakpoints.len() + self.conditional_breakpoints.len(),
            watch_count: self.watch_expressions.len(),
            checksum,
        }
    }

    /// Export recording as JSON for web viewers
    pub fn export_recording_json(&self) -> String {
        let metadata = self.recording_metadata();
        let mut steps = Vec::new();
        for record in &self.history {
            let locals: serde_json::Map<String, serde_json::Value> = record
                .locals
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.to_js_string())))
                .collect();
            steps.push(serde_json::json!({
                "step": record.step,
                "ip": record.ip,
                "line": record.line,
                "stack_size": record.stack_size,
                "description": record.description,
                "locals": locals,
            }));
        }

        let json = serde_json::json!({
            "format_version": metadata.format_version,
            "source_file": metadata.source_file,
            "total_steps": metadata.total_steps,
            "checksum": metadata.checksum,
            "steps": steps,
        });

        serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string())
    }

    /// Search history for steps where query matches variable names or values
    pub fn search_history(&self, query: &str) -> Vec<usize> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();
        for (i, record) in self.history.iter().enumerate() {
            for (name, value) in &record.locals {
                if name.to_lowercase().contains(&query_lower)
                    || value.to_js_string().to_lowercase().contains(&query_lower)
                {
                    results.push(i);
                    break;
                }
            }
        }
        results
    }

    // ============================================================
    // Delta Recording
    // ============================================================

    /// Record a delta-based execution step
    pub fn record_delta(&mut self, record: DeltaRecord) {
        self.delta_history.push(record);
    }

    /// Get the delta history
    pub fn delta_history(&self) -> &[DeltaRecord] {
        &self.delta_history
    }

    /// Reconstruct full state at a given step by replaying deltas from the start
    pub fn reconstruct_state_at(&self, step: usize) -> Option<ReconstructedState> {
        if self.delta_history.is_empty() {
            return None;
        }

        let mut locals = std::collections::HashMap::new();
        let mut globals = std::collections::HashMap::new();
        let mut stack: Vec<String> = Vec::new();

        for delta in &self.delta_history {
            if delta.step > step {
                break;
            }

            for (k, v) in &delta.changed_locals {
                locals.insert(k.clone(), v.clone());
            }
            for (k, v) in &delta.changed_globals {
                globals.insert(k.clone(), v.clone());
            }

            match &delta.stack_delta {
                StackDelta::Push(values) => {
                    stack.extend(values.iter().cloned());
                }
                StackDelta::Pop(n) => {
                    let new_len = stack.len().saturating_sub(*n);
                    stack.truncate(new_len);
                }
                StackDelta::Replace(values) => {
                    stack = values.clone();
                }
                StackDelta::NoChange => {}
            }
        }

        // Check that we actually had data for the requested step
        let found = self.delta_history.iter().any(|d| d.step == step);
        if !found && step > 0 {
            // Return state up to the closest step <= requested step
            if let Some(last) = self.delta_history.last() {
                if step > last.step {
                    return None;
                }
            }
        }

        Some(ReconstructedState {
            step,
            locals,
            globals,
            stack,
        })
    }

    /// Calculate approximate memory usage of delta history in bytes
    pub fn delta_memory_usage(&self) -> usize {
        let mut total = std::mem::size_of::<Vec<DeltaRecord>>(); // Vec overhead
        for delta in &self.delta_history {
            total += std::mem::size_of::<DeltaRecord>();
            total += delta.opcode.len();
            for (k, v) in &delta.changed_locals {
                total += k.len() + v.len();
            }
            for (k, v) in &delta.changed_globals {
                total += k.len() + v.len();
            }
            match &delta.stack_delta {
                StackDelta::Push(values) | StackDelta::Replace(values) => {
                    for v in values {
                        total += v.len();
                    }
                }
                _ => {}
            }
        }
        total
    }
}
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

/// Evaluate a simple condition string against variables.
/// Supports basic comparisons like "x == 5", "name == hello", "x > 3", "x < 10", "x != 0".
fn evaluate_simple_condition(condition: &str, variables: &std::collections::HashMap<String, String>) -> bool {
    let condition = condition.trim();

    // Try operators: ==, !=, >=, <=, >, <
    let ops: &[(&str, fn(&str, &str) -> bool)] = &[
        ("==", |a, b| a == b),
        ("!=", |a, b| a != b),
        (">=", |a, b| {
            a.parse::<f64>()
                .and_then(|av| b.parse::<f64>().map(|bv| av >= bv))
                .unwrap_or(false)
        }),
        ("<=", |a, b| {
            a.parse::<f64>()
                .and_then(|av| b.parse::<f64>().map(|bv| av <= bv))
                .unwrap_or(false)
        }),
        (">", |a, b| {
            a.parse::<f64>()
                .and_then(|av| b.parse::<f64>().map(|bv| av > bv))
                .unwrap_or(false)
        }),
        ("<", |a, b| {
            a.parse::<f64>()
                .and_then(|av| b.parse::<f64>().map(|bv| av < bv))
                .unwrap_or(false)
        }),
    ];

    for (op, cmp) in ops {
        if let Some(idx) = condition.find(op) {
            let var_name = condition[..idx].trim();
            let expected = condition[idx + op.len()..].trim();
            if let Some(actual) = variables.get(var_name) {
                return cmp(actual, expected);
            }
            return false;
        }
    }

    // If no operator, check if the variable is truthy (exists and non-empty/non-zero)
    if let Some(val) = variables.get(condition) {
        return !val.is_empty() && val != "0" && val != "false" && val != "undefined" && val != "null";
    }
    false
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

/// TUI panel layout for the debugger dashboard
#[derive(Debug, Clone)]
pub struct TuiPanel {
    /// Panel title
    pub title: String,
    /// Lines of content
    pub lines: Vec<String>,
    /// Whether this panel is focused
    pub focused: bool,
}

impl TimeTravelDebugger {
    /// Render the source code panel with current line highlighted
    pub fn render_source_panel(&self, context_lines: usize) -> TuiPanel {
        let mut lines = Vec::new();
        if let Some(record) = self.current() {
            let current_line = record.line as usize;

            if !self.source_lines.is_empty() {
                let start = current_line.saturating_sub(context_lines + 1);
                let end = (current_line + context_lines).min(self.source_lines.len());

                for i in start..end {
                    let marker = if i + 1 == current_line { "▶" } else { " " };
                    lines.push(format!("{} {:>4} │ {}", marker,
                        i + 1,
                        self.source_lines.get(i).map(|s| s.as_str()).unwrap_or("")));
                }
            } else {
                lines.push("  (no source loaded)".to_string());
            }
        } else {
            lines.push("  (not started)".to_string());
        }

        TuiPanel { title: "Source".to_string(), lines, focused: true }
    }

    /// Render the variables panel showing current local variables
    pub fn render_variables_panel(&self) -> TuiPanel {
        let mut lines = Vec::new();
        if let Some(record) = self.current() {
            let mut sorted: Vec<_> = record.locals.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            for (name, value) in sorted {
                lines.push(format!("  {} = {}", name, value.to_js_string()));
            }
            if lines.is_empty() {
                lines.push("  (no local variables)".to_string());
            }
        } else {
            lines.push("  (not running)".to_string());
        }
        TuiPanel { title: "Variables".to_string(), lines, focused: false }
    }

    /// Render the call stack panel
    pub fn render_stack_panel(&self) -> TuiPanel {
        let mut lines = Vec::new();
        if let Some(record) = self.current() {
            let stack = &record.stack_top;
            let start = stack.len().saturating_sub(10);
            for (i, val) in stack[start..].iter().enumerate().rev() {
                lines.push(format!("  [{}] {}", start + i, val.to_js_string()));
            }
            if lines.is_empty() {
                lines.push("  (empty stack)".to_string());
            }
        } else {
            lines.push("  (not running)".to_string());
        }
        TuiPanel { title: "Stack".to_string(), lines, focused: false }
    }

    /// Render the timeline panel showing execution progress
    pub fn render_timeline_panel(&self) -> TuiPanel {
        let mut lines = Vec::new();
        let total = self.history.len();
        let current = self.current_position;

        if total > 0 {
            let progress = if total > 1 { current as f64 / (total - 1) as f64 } else { 1.0 };
            let bar_width: usize = 40;
            let filled = (progress * bar_width as f64) as usize;
            let empty = bar_width - filled;
            let bar = format!("{}{}",
                "█".repeat(filled),
                "░".repeat(empty)
            );
            lines.push(format!("  Step {}/{} [{}]", current + 1, total, bar));

            let mut bp_marks = Vec::new();
            for bp in self.breakpoints.values() {
                bp_marks.push(format!("  ● Line {} {}", bp.line,
                    if bp.enabled { "(enabled)" } else { "(disabled)" }));
            }
            if !bp_marks.is_empty() {
                lines.push(String::new());
                lines.push("  Breakpoints:".to_string());
                lines.extend(bp_marks);
            }
        } else {
            lines.push("  (no history)".to_string());
        }

        TuiPanel { title: "Timeline".to_string(), lines, focused: false }
    }

    /// Render a complete TUI dashboard as a string
    pub fn render_dashboard(&self) -> String {
        let source = self.render_source_panel(5);
        let vars = self.render_variables_panel();
        let _stack = self.render_stack_panel();
        let timeline = self.render_timeline_panel();
        let stats = self.stats();

        let width = 70;
        let bar = "─".repeat(width);
        let dbar = "═".repeat(width);

        // Char-safe truncation helper
        let trunc = |s: &str, max: usize| -> String {
            let chars: Vec<char> = s.chars().collect();
            if chars.len() > max {
                chars[..max].iter().collect()
            } else {
                s.to_string()
            }
        };

        let mut s = String::new();
        s.push_str(&format!("╔{}╗\n", dbar));
        s.push_str(&format!("║{:^70}║\n", "QUICKSILVER TIME-TRAVEL DEBUGGER"));
        s.push_str(&format!("╠{}╣\n", dbar));

        // Source panel
        s.push_str(&format!("║ {:<68} ║\n", format!("Source: {}", self.current_file)));
        s.push_str(&format!("╟{}╢\n", bar));
        for line in &source.lines {
            s.push_str(&format!("║ {:<68} ║\n", trunc(line, 68)));
        }

        // Variables panel
        s.push_str(&format!("╟{}╢\n", bar));
        s.push_str(&format!("║ {:<68} ║\n", "Variables"));
        s.push_str(&format!("╟{}╢\n", bar));
        for line in &vars.lines {
            s.push_str(&format!("║ {:<68} ║\n", trunc(line, 68)));
        }

        // Timeline
        s.push_str(&format!("╟{}╢\n", bar));
        s.push_str(&format!("║ {:<68} ║\n", "Timeline"));
        s.push_str(&format!("╟{}╢\n", bar));
        for line in &timeline.lines {
            s.push_str(&format!("║ {:<68} ║\n", trunc(line, 68)));
        }

        // Stats bar
        s.push_str(&format!("╟{}╢\n", bar));
        s.push_str(&format!(
            "║ Steps: {:<6} Breakpoints: {:<6} Watches: {:<22} ║\n",
            stats.total_steps, stats.breakpoint_count, stats.watch_count));
        s.push_str(&format!("╚{}╝\n", dbar));

        s
    }
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

    #[test]
    fn test_tui_source_panel() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.load_source("test.js", "let x = 1;\nlet y = 2;\nlet z = 3;\n");
        debugger.record_step(None, 0, 2, &[], &HashMap::default(), "");
        let panel = debugger.render_source_panel(1);
        assert_eq!(panel.title, "Source");
        assert!(!panel.lines.is_empty());
    }

    #[test]
    fn test_tui_variables_panel() {
        let mut debugger = TimeTravelDebugger::new();
        let mut locals = HashMap::default();
        locals.insert("x".to_string(), Value::Number(42.0));
        debugger.record_step(None, 0, 1, &[], &locals, "");
        let panel = debugger.render_variables_panel();
        assert_eq!(panel.title, "Variables");
        assert!(panel.lines.iter().any(|l| l.contains("x") && l.contains("42")));
    }

    #[test]
    fn test_tui_timeline_panel() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.record_step(None, 0, 1, &[], &HashMap::default(), "");
        debugger.record_step(None, 1, 2, &[], &HashMap::default(), "");
        let panel = debugger.render_timeline_panel();
        assert!(panel.lines.iter().any(|l| l.contains("Step")));
    }

    #[test]
    fn test_tui_dashboard() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.load_source("test.js", "let x = 1;\nconsole.log(x);\n");
        let mut locals = HashMap::default();
        locals.insert("x".to_string(), Value::Number(1.0));
        debugger.record_step(None, 0, 1, &[], &locals, "");
        let dashboard = debugger.render_dashboard();
        assert!(dashboard.contains("TIME-TRAVEL DEBUGGER"));
        assert!(dashboard.contains("Source"));
        assert!(dashboard.contains("Variables"));
    }

    #[test]
    fn test_tui_empty_state() {
        let debugger = TimeTravelDebugger::new();
        let dashboard = debugger.render_dashboard();
        assert!(dashboard.contains("not started"));
    }

    // ============================================================
    // Conditional Breakpoint Tests
    // ============================================================

    #[test]
    fn test_conditional_breakpoint_creation() {
        let mut debugger = TimeTravelDebugger::new();
        let bp = ConditionalBreakpoint {
            line: 10,
            condition: Some("x == 5".to_string()),
            hit_condition: None,
            log_message: Some("Hit line 10".to_string()),
            enabled: true,
            hit_count: 0,
        };
        let id = debugger.add_conditional_breakpoint(bp);
        assert_eq!(id, 1);
        let id2 = debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 20,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
            hit_count: 0,
        });
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_conditional_breakpoint_condition_eval() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 10,
            condition: Some("x == 5".to_string()),
            hit_condition: None,
            log_message: None,
            enabled: true,
            hit_count: 0,
        });

        let mut vars = std::collections::HashMap::new();
        vars.insert("x".to_string(), "5".to_string());
        assert!(debugger.should_break_at_line(10, &vars));

        vars.insert("x".to_string(), "3".to_string());
        assert!(!debugger.should_break_at_line(10, &vars));
    }

    #[test]
    fn test_conditional_breakpoint_disabled() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 10,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: false,
            hit_count: 0,
        });
        let vars = std::collections::HashMap::new();
        assert!(!debugger.should_break_at_line(10, &vars));
    }

    #[test]
    fn test_hit_condition_equal() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 5,
            condition: None,
            hit_condition: Some(HitCondition::Equal(3)),
            log_message: None,
            enabled: true,
            hit_count: 3,
        });
        let vars = std::collections::HashMap::new();
        assert!(debugger.should_break_at_line(5, &vars));

        // Change hit count so it no longer matches
        debugger.conditional_breakpoints[0].hit_count = 2;
        assert!(!debugger.should_break_at_line(5, &vars));
    }

    #[test]
    fn test_hit_condition_multiple() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 5,
            condition: None,
            hit_condition: Some(HitCondition::Multiple(4)),
            log_message: None,
            enabled: true,
            hit_count: 8,
        });
        let vars = std::collections::HashMap::new();
        assert!(debugger.should_break_at_line(5, &vars));

        debugger.conditional_breakpoints[0].hit_count = 7;
        assert!(!debugger.should_break_at_line(5, &vars));
    }

    #[test]
    fn test_hit_condition_greater_equal() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 5,
            condition: None,
            hit_condition: Some(HitCondition::GreaterEqual(5)),
            log_message: None,
            enabled: true,
            hit_count: 5,
        });
        let vars = std::collections::HashMap::new();
        assert!(debugger.should_break_at_line(5, &vars));

        debugger.conditional_breakpoints[0].hit_count = 4;
        assert!(!debugger.should_break_at_line(5, &vars));

        debugger.conditional_breakpoints[0].hit_count = 10;
        assert!(debugger.should_break_at_line(5, &vars));
    }

    #[test]
    fn test_update_conditional_breakpoint() {
        let mut debugger = TimeTravelDebugger::new();
        let id = debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 10,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
            hit_count: 0,
        });
        debugger.update_breakpoint(id, ConditionalBreakpoint {
            line: 20,
            condition: Some("y > 3".to_string()),
            hit_condition: None,
            log_message: None,
            enabled: true,
            hit_count: 0,
        });
        assert_eq!(debugger.conditional_breakpoints[0].line, 20);
        assert_eq!(debugger.conditional_breakpoints[0].condition, Some("y > 3".to_string()));
    }

    // ============================================================
    // Watch Expression Tests
    // ============================================================

    #[test]
    fn test_watch_expression_add_remove() {
        let mut debugger = TimeTravelDebugger::new();
        let id1 = debugger.add_watch_expression("x");
        let id2 = debugger.add_watch_expression("y");
        assert_eq!(debugger.watches().len(), 2);

        assert!(debugger.remove_watch_expression(id1));
        assert_eq!(debugger.watches().len(), 1);
        assert_eq!(debugger.watches()[0].expression, "y");

        assert!(!debugger.remove_watch_expression(id1)); // already removed
        assert!(debugger.remove_watch_expression(id2));
        assert!(debugger.watches().is_empty());
    }

    #[test]
    fn test_watch_expression_update() {
        let mut debugger = TimeTravelDebugger::new();
        let id = debugger.add_watch_expression("counter");

        let mut vars = std::collections::HashMap::new();
        vars.insert("counter".to_string(), "0".to_string());
        debugger.update_watches(1, &vars);

        vars.insert("counter".to_string(), "1".to_string());
        debugger.update_watches(2, &vars);

        vars.insert("counter".to_string(), "1".to_string());
        debugger.update_watches(3, &vars); // no change

        let history = debugger.get_watch_history(id).unwrap();
        assert_eq!(history.len(), 3);
        assert!(history[0].changed); // initial is always a change from None
        assert!(history[1].changed);
        assert!(!history[2].changed); // same value
    }

    #[test]
    fn test_watch_expression_history_undefined() {
        let mut debugger = TimeTravelDebugger::new();
        let id = debugger.add_watch_expression("missing_var");

        let vars = std::collections::HashMap::new();
        debugger.update_watches(1, &vars);

        let history = debugger.get_watch_history(id).unwrap();
        assert_eq!(history[0].value, "undefined");
    }

    #[test]
    fn test_get_watch_history_invalid_id() {
        let debugger = TimeTravelDebugger::new();
        assert!(debugger.get_watch_history(999).is_none());
    }

    // ============================================================
    // Recording Metadata & Export Tests
    // ============================================================

    #[test]
    fn test_recording_metadata() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.load_source("test.js", "let x = 1;\n");
        debugger.record_step(None, 0, 1, &[], &HashMap::default(), "init");
        debugger.record_step(None, 1, 2, &[], &HashMap::default(), "step");
        debugger.add_breakpoint(10, None);

        let meta = debugger.recording_metadata();
        assert_eq!(meta.format_version, RECORDING_VERSION);
        assert_eq!(meta.source_file, Some("test.js".to_string()));
        assert_eq!(meta.total_steps, 2);
        assert_eq!(meta.breakpoint_count, 1);
        assert_eq!(meta.watch_count, 0);
        assert!(meta.checksum != 0);
    }

    #[test]
    fn test_export_recording_json() {
        let mut debugger = TimeTravelDebugger::new();
        let mut locals = HashMap::default();
        locals.insert("x".to_string(), Value::Number(42.0));
        debugger.record_step(None, 0, 1, &[], &locals, "set x");

        let json = debugger.export_recording_json();
        assert!(json.contains("\"format_version\""));
        assert!(json.contains("\"steps\""));
        assert!(json.contains("set x"));
        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total_steps"], 1);
    }

    #[test]
    fn test_search_history() {
        let mut debugger = TimeTravelDebugger::new();

        let mut locals1 = HashMap::default();
        locals1.insert("name".to_string(), Value::String("Alice".to_string()));
        debugger.record_step(None, 0, 1, &[], &locals1, "");

        let mut locals2 = HashMap::default();
        locals2.insert("count".to_string(), Value::Number(42.0));
        debugger.record_step(None, 1, 2, &[], &locals2, "");

        let mut locals3 = HashMap::default();
        locals3.insert("name".to_string(), Value::String("Bob".to_string()));
        debugger.record_step(None, 2, 3, &[], &locals3, "");

        // Search by variable name
        let results = debugger.search_history("name");
        assert_eq!(results.len(), 2);

        // Search by value
        let results = debugger.search_history("Alice");
        assert_eq!(results.len(), 1);

        // No match
        let results = debugger.search_history("nonexistent");
        assert!(results.is_empty());
    }

    // ============================================================
    // Delta Recording Tests
    // ============================================================

    #[test]
    fn test_delta_record_and_retrieve() {
        let mut debugger = TimeTravelDebugger::new();
        let delta = DeltaRecord {
            step: 1,
            opcode: "Push".to_string(),
            ip: 0,
            line: Some(1),
            changed_locals: vec![("x".to_string(), "1".to_string())],
            changed_globals: vec![],
            stack_delta: StackDelta::Push(vec!["1".to_string()]),
        };
        debugger.record_delta(delta);
        assert_eq!(debugger.delta_history().len(), 1);
        assert_eq!(debugger.delta_history()[0].step, 1);
    }

    #[test]
    fn test_reconstruct_state_at() {
        let mut debugger = TimeTravelDebugger::new();

        debugger.record_delta(DeltaRecord {
            step: 0,
            opcode: "Push".to_string(),
            ip: 0,
            line: Some(1),
            changed_locals: vec![("x".to_string(), "1".to_string())],
            changed_globals: vec![("global_a".to_string(), "true".to_string())],
            stack_delta: StackDelta::Push(vec!["1".to_string()]),
        });
        debugger.record_delta(DeltaRecord {
            step: 1,
            opcode: "SetLocal".to_string(),
            ip: 1,
            line: Some(2),
            changed_locals: vec![("x".to_string(), "2".to_string()), ("y".to_string(), "10".to_string())],
            changed_globals: vec![],
            stack_delta: StackDelta::Push(vec!["2".to_string()]),
        });
        debugger.record_delta(DeltaRecord {
            step: 2,
            opcode: "Pop".to_string(),
            ip: 2,
            line: Some(3),
            changed_locals: vec![],
            changed_globals: vec![],
            stack_delta: StackDelta::Pop(1),
        });

        // At step 0: x=1, stack=[1]
        let state = debugger.reconstruct_state_at(0).unwrap();
        assert_eq!(state.locals.get("x"), Some(&"1".to_string()));
        assert_eq!(state.stack, vec!["1".to_string()]);
        assert_eq!(state.globals.get("global_a"), Some(&"true".to_string()));

        // At step 1: x=2, y=10, stack=[1, 2]
        let state = debugger.reconstruct_state_at(1).unwrap();
        assert_eq!(state.locals.get("x"), Some(&"2".to_string()));
        assert_eq!(state.locals.get("y"), Some(&"10".to_string()));
        assert_eq!(state.stack, vec!["1".to_string(), "2".to_string()]);

        // At step 2: stack=[1] (popped one)
        let state = debugger.reconstruct_state_at(2).unwrap();
        assert_eq!(state.stack, vec!["1".to_string()]);
    }

    #[test]
    fn test_reconstruct_state_empty_history() {
        let debugger = TimeTravelDebugger::new();
        assert!(debugger.reconstruct_state_at(0).is_none());
    }

    #[test]
    fn test_delta_memory_usage() {
        let mut debugger = TimeTravelDebugger::new();
        let base_usage = debugger.delta_memory_usage();

        debugger.record_delta(DeltaRecord {
            step: 0,
            opcode: "Push".to_string(),
            ip: 0,
            line: Some(1),
            changed_locals: vec![("x".to_string(), "1".to_string())],
            changed_globals: vec![],
            stack_delta: StackDelta::Push(vec!["hello".to_string()]),
        });

        let usage_after = debugger.delta_memory_usage();
        assert!(usage_after > base_usage);
    }

    #[test]
    fn test_stack_delta_replace() {
        let mut debugger = TimeTravelDebugger::new();

        debugger.record_delta(DeltaRecord {
            step: 0,
            opcode: "Push".to_string(),
            ip: 0,
            line: Some(1),
            changed_locals: vec![],
            changed_globals: vec![],
            stack_delta: StackDelta::Push(vec!["a".to_string(), "b".to_string()]),
        });
        debugger.record_delta(DeltaRecord {
            step: 1,
            opcode: "Replace".to_string(),
            ip: 1,
            line: Some(2),
            changed_locals: vec![],
            changed_globals: vec![],
            stack_delta: StackDelta::Replace(vec!["x".to_string(), "y".to_string(), "z".to_string()]),
        });

        let state = debugger.reconstruct_state_at(1).unwrap();
        assert_eq!(state.stack, vec!["x".to_string(), "y".to_string(), "z".to_string()]);
    }

    #[test]
    fn test_stack_delta_no_change() {
        let mut debugger = TimeTravelDebugger::new();

        debugger.record_delta(DeltaRecord {
            step: 0,
            opcode: "Push".to_string(),
            ip: 0,
            line: Some(1),
            changed_locals: vec![],
            changed_globals: vec![],
            stack_delta: StackDelta::Push(vec!["42".to_string()]),
        });
        debugger.record_delta(DeltaRecord {
            step: 1,
            opcode: "Nop".to_string(),
            ip: 1,
            line: Some(2),
            changed_locals: vec![],
            changed_globals: vec![],
            stack_delta: StackDelta::NoChange,
        });

        let state = debugger.reconstruct_state_at(1).unwrap();
        assert_eq!(state.stack, vec!["42".to_string()]);
    }

    #[test]
    fn test_condition_eval_greater_than() {
        let mut debugger = TimeTravelDebugger::new();
        debugger.add_conditional_breakpoint(ConditionalBreakpoint {
            line: 5,
            condition: Some("x > 3".to_string()),
            hit_condition: None,
            log_message: None,
            enabled: true,
            hit_count: 0,
        });

        let mut vars = std::collections::HashMap::new();
        vars.insert("x".to_string(), "5".to_string());
        assert!(debugger.should_break_at_line(5, &vars));

        vars.insert("x".to_string(), "2".to_string());
        assert!(!debugger.should_break_at_line(5, &vars));
    }
}
