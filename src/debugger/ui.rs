//! Rich Terminal UI for Time-Travel Debugger
//!
//! This module provides an enhanced terminal-based UI with:
//! - Timeline visualization
//! - Call stack display
//! - Variable diff view
//! - Memory inspection

use super::{TimeTravelDebugger, ExecutionRecord};
use crate::Value;
use rustc_hash::FxHashMap as HashMap;
use std::fmt::Write as FmtWrite;

/// Call frame information
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// Function name
    pub name: String,
    /// Source line
    pub line: u32,
    /// Local variables in this frame
    pub locals: HashMap<String, Value>,
    /// Arguments passed to this function
    pub arguments: Vec<Value>,
}

/// Call stack representation
#[derive(Debug, Clone, Default)]
pub struct CallStack {
    /// Stack of call frames
    frames: Vec<CallFrame>,
}

impl CallStack {
    /// Create a new empty call stack
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Push a new call frame
    pub fn push(&mut self, frame: CallFrame) {
        self.frames.push(frame);
    }

    /// Pop the top frame
    pub fn pop(&mut self) -> Option<CallFrame> {
        self.frames.pop()
    }

    /// Get the current (top) frame
    pub fn current(&self) -> Option<&CallFrame> {
        self.frames.last()
    }

    /// Get the call stack depth
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Iterate over frames (top to bottom)
    pub fn iter(&self) -> impl Iterator<Item = &CallFrame> {
        self.frames.iter().rev()
    }

    /// Get all frames
    pub fn frames(&self) -> &[CallFrame] {
        &self.frames
    }
}

/// Difference in a variable between steps
#[derive(Debug, Clone)]
pub struct VariableDiff {
    /// Variable name
    pub name: String,
    /// Previous value (None if newly created)
    pub old_value: Option<Value>,
    /// Current value (None if deleted)
    pub new_value: Option<Value>,
    /// Type of change
    pub change_type: ChangeType,
}

/// Type of change to a variable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// Variable was created
    Created,
    /// Variable was modified
    Modified,
    /// Variable was deleted
    Deleted,
    /// Variable was unchanged
    Unchanged,
}

/// Compute the diff between two execution records
pub fn compute_diff(
    old: Option<&ExecutionRecord>,
    new: &ExecutionRecord,
) -> Vec<VariableDiff> {
    let mut diffs = Vec::new();

    let default_locals = HashMap::default();
    let old_locals = old.map(|r| &r.locals).unwrap_or(&default_locals);
    let new_locals = &new.locals;

    // Check for created and modified variables
    for (name, new_value) in new_locals {
        if let Some(old_value) = old_locals.get(name) {
            if !old_value.strict_equals(new_value) {
                diffs.push(VariableDiff {
                    name: name.clone(),
                    old_value: Some(old_value.clone()),
                    new_value: Some(new_value.clone()),
                    change_type: ChangeType::Modified,
                });
            }
        } else {
            diffs.push(VariableDiff {
                name: name.clone(),
                old_value: None,
                new_value: Some(new_value.clone()),
                change_type: ChangeType::Created,
            });
        }
    }

    // Check for deleted variables
    for (name, old_value) in old_locals {
        if !new_locals.contains_key(name) {
            diffs.push(VariableDiff {
                name: name.clone(),
                old_value: Some(old_value.clone()),
                new_value: None,
                change_type: ChangeType::Deleted,
            });
        }
    }

    diffs
}

/// Timeline representation for visualization
#[derive(Debug)]
pub struct Timeline {
    /// Total number of steps
    pub total_steps: u64,
    /// Current position
    pub current_position: usize,
    /// Width of the timeline in characters
    pub width: usize,
    /// Marked positions (breakpoints, etc.)
    pub markers: Vec<TimelineMarker>,
}

/// A marker on the timeline
#[derive(Debug, Clone)]
pub struct TimelineMarker {
    /// Step number
    pub step: u64,
    /// Marker type
    pub marker_type: MarkerType,
    /// Label
    pub label: String,
}

/// Type of timeline marker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerType {
    /// Breakpoint
    Breakpoint,
    /// Error occurred
    Error,
    /// Variable changed
    VariableChange,
    /// Function call
    FunctionCall,
    /// Function return
    FunctionReturn,
}

impl Timeline {
    /// Create a new timeline
    pub fn new(total_steps: u64, current_position: usize, width: usize) -> Self {
        Self {
            total_steps,
            current_position,
            width,
            markers: Vec::new(),
        }
    }

    /// Add a marker
    pub fn add_marker(&mut self, step: u64, marker_type: MarkerType, label: &str) {
        self.markers.push(TimelineMarker {
            step,
            marker_type,
            label: label.to_string(),
        });
    }

    /// Render the timeline as a string
    pub fn render(&self) -> String {
        let mut output = String::new();

        // Top border
        writeln!(output, "╔{}╗", "═".repeat(self.width)).unwrap();

        // Timeline bar
        let scale = if self.total_steps > 0 {
            (self.width - 2) as f64 / self.total_steps as f64
        } else {
            0.0
        };

        let mut timeline_chars: Vec<char> = vec!['─'; self.width - 2];

        // Add markers
        for marker in &self.markers {
            let pos = (marker.step as f64 * scale) as usize;
            if pos < timeline_chars.len() {
                timeline_chars[pos] = match marker.marker_type {
                    MarkerType::Breakpoint => '●',
                    MarkerType::Error => '✗',
                    MarkerType::VariableChange => '◆',
                    MarkerType::FunctionCall => '▸',
                    MarkerType::FunctionReturn => '◂',
                };
            }
        }

        // Add current position
        let current_pos = (self.current_position as f64 * scale) as usize;
        if current_pos < timeline_chars.len() {
            timeline_chars[current_pos] = '▼';
        }

        write!(output, "║").unwrap();
        for c in timeline_chars {
            write!(output, "{}", c).unwrap();
        }
        writeln!(output, "║").unwrap();

        // Position indicator line
        write!(output, "║").unwrap();
        for i in 0..(self.width - 2) {
            if i == current_pos {
                write!(output, "│").unwrap();
            } else {
                write!(output, " ").unwrap();
            }
        }
        writeln!(output, "║").unwrap();

        // Step numbers
        let left_label = "1";
        let right_label = format!("{}", self.total_steps);
        let middle_label = format!("Step {} / {}", self.current_position + 1, self.total_steps);

        let padding = (self.width - 2 - middle_label.len()) / 2;
        write!(output, "║{}", left_label).unwrap();
        let remaining = self.width - 2 - left_label.len() - right_label.len();
        let left_pad = (remaining - middle_label.len()) / 2;
        let right_pad = remaining - left_pad - middle_label.len();
        write!(output, "{}", " ".repeat(left_pad)).unwrap();
        write!(output, "{}", middle_label).unwrap();
        write!(output, "{}", " ".repeat(right_pad)).unwrap();
        writeln!(output, "{}║", right_label).unwrap();

        // Bottom border
        writeln!(output, "╚{}╝", "═".repeat(self.width)).unwrap();

        output
    }
}

/// Rich debugger UI
pub struct DebuggerUI {
    /// The underlying debugger
    debugger: TimeTravelDebugger,
    /// Call stack tracker
    call_stack: CallStack,
    /// Terminal width
    terminal_width: usize,
    /// Show timeline
    show_timeline: bool,
    /// Show diff
    show_diff: bool,
    /// Show call stack
    show_call_stack: bool,
    /// Previous record for diff calculation
    previous_record: Option<ExecutionRecord>,
}

impl DebuggerUI {
    /// Create a new debugger UI
    pub fn new(debugger: TimeTravelDebugger) -> Self {
        Self {
            debugger,
            call_stack: CallStack::new(),
            terminal_width: 80,
            show_timeline: true,
            show_diff: true,
            show_call_stack: true,
            previous_record: None,
        }
    }

    /// Set terminal width
    pub fn set_width(&mut self, width: usize) {
        self.terminal_width = width;
    }

    /// Toggle timeline display
    pub fn toggle_timeline(&mut self) {
        self.show_timeline = !self.show_timeline;
    }

    /// Toggle diff display
    pub fn toggle_diff(&mut self) {
        self.show_diff = !self.show_diff;
    }

    /// Toggle call stack display
    pub fn toggle_call_stack(&mut self) {
        self.show_call_stack = !self.show_call_stack;
    }

    /// Step forward and update UI
    pub fn step_forward(&mut self) -> Option<&ExecutionRecord> {
        self.previous_record = self.debugger.current().cloned();
        self.debugger.step_forward()
    }

    /// Step backward and update UI
    pub fn step_back(&mut self) -> Option<&ExecutionRecord> {
        self.previous_record = self.debugger.current().cloned();
        self.debugger.step_back()
    }

    /// Get current execution record
    pub fn current(&self) -> Option<&ExecutionRecord> {
        self.debugger.current()
    }

    /// Render the full UI
    pub fn render(&self) -> String {
        let mut output = String::new();

        // Header
        writeln!(output, "\n{}", "═".repeat(self.terminal_width)).unwrap();
        writeln!(output, "   🕐 Quicksilver Time-Travel Debugger").unwrap();
        writeln!(output, "{}\n", "═".repeat(self.terminal_width)).unwrap();

        // Timeline
        if self.show_timeline {
            let info = self.debugger.recording_info();
            let timeline = Timeline::new(
                info.total_steps,
                info.current_position,
                self.terminal_width.min(60),
            );
            output.push_str(&timeline.render());
            output.push('\n');
        }

        // Current state
        if let Some(record) = self.debugger.current() {
            // Step info
            writeln!(output, "📍 Step {} | Line {} | {}",
                record.step, record.line, record.description).unwrap();
            output.push('\n');

            // Source context
            output.push_str(&self.render_source_context(record.line));
            output.push('\n');

            // Call stack
            if self.show_call_stack && !self.call_stack.frames.is_empty() {
                output.push_str(&self.render_call_stack());
                output.push('\n');
            }

            // Variables
            output.push_str(&self.render_variables(record));
            output.push('\n');

            // Diff
            if self.show_diff {
                if let Some(ref prev) = self.previous_record {
                    let diffs = compute_diff(Some(prev), record);
                    if !diffs.is_empty() {
                        output.push_str(&self.render_diff(&diffs));
                        output.push('\n');
                    }
                }
            }
        } else {
            writeln!(output, "No execution history available").unwrap();
        }

        // Footer with commands
        writeln!(output, "{}", "─".repeat(self.terminal_width)).unwrap();
        writeln!(output, "Commands: n=next  b=back  t=timeline  d=diff  s=stack  q=quit").unwrap();

        output
    }

    /// Render source context around a line
    fn render_source_context(&self, line: u32) -> String {
        let mut output = String::new();
        let info = self.debugger.recording_info();

        if info.source_lines == 0 {
            return output;
        }

        writeln!(output, "┌─ Source ─────────────────────────────────────┐").unwrap();

        let line_idx = (line as usize).saturating_sub(1);
        let start = line_idx.saturating_sub(2);
        let end = (line_idx + 3).min(info.source_lines);

        // We need access to source lines, but RecordingInfo doesn't have them
        // This would need refactoring - for now just show line numbers
        for i in start..end {
            let marker = if i == line_idx { "→" } else { " " };
            let bg = if i == line_idx { "▶" } else { " " };
            writeln!(output, "│{} {:4} │{}", marker, i + 1, bg).unwrap();
        }

        writeln!(output, "└──────────────────────────────────────────────┘").unwrap();

        output
    }

    /// Render the call stack
    fn render_call_stack(&self) -> String {
        let mut output = String::new();

        writeln!(output, "┌─ Call Stack ─────────────────────────────────┐").unwrap();

        for (i, frame) in self.call_stack.iter().enumerate() {
            let indent = "  ".repeat(i);
            writeln!(output, "│ {}{}() at line {}", indent, frame.name, frame.line).unwrap();
        }

        if self.call_stack.depth() == 0 {
            writeln!(output, "│ (empty)").unwrap();
        }

        writeln!(output, "└──────────────────────────────────────────────┘").unwrap();

        output
    }

    /// Render variables for a record
    fn render_variables(&self, record: &ExecutionRecord) -> String {
        let mut output = String::new();

        writeln!(output, "┌─ Variables ───────────────────────────────────┐").unwrap();

        if record.locals.is_empty() {
            writeln!(output, "│ (no local variables)").unwrap();
        } else {
            let mut vars: Vec<_> = record.locals.iter().collect();
            vars.sort_by(|a, b| a.0.cmp(b.0));

            for (name, value) in vars {
                let type_str = value.type_of();
                let value_str = value.to_js_string();
                let truncated = if value_str.len() > 30 {
                    format!("{}...", &value_str[..27])
                } else {
                    value_str
                };
                writeln!(output, "│ {} = {} ({})", name, truncated, type_str).unwrap();
            }
        }

        writeln!(output, "└───────────────────────────────────────────────┘").unwrap();

        output
    }

    /// Render a diff between steps
    fn render_diff(&self, diffs: &[VariableDiff]) -> String {
        let mut output = String::new();

        writeln!(output, "┌─ Changes ─────────────────────────────────────┐").unwrap();

        for diff in diffs {
            match diff.change_type {
                ChangeType::Created => {
                    let val = diff.new_value.as_ref()
                        .map(|v| v.to_js_string())
                        .unwrap_or_default();
                    writeln!(output, "│ + {} = {}", diff.name, val).unwrap();
                }
                ChangeType::Modified => {
                    let old = diff.old_value.as_ref()
                        .map(|v| v.to_js_string())
                        .unwrap_or_default();
                    let new = diff.new_value.as_ref()
                        .map(|v| v.to_js_string())
                        .unwrap_or_default();
                    writeln!(output, "│ ~ {} : {} → {}", diff.name, old, new).unwrap();
                }
                ChangeType::Deleted => {
                    let val = diff.old_value.as_ref()
                        .map(|v| v.to_js_string())
                        .unwrap_or_default();
                    writeln!(output, "│ - {} (was {})", diff.name, val).unwrap();
                }
                ChangeType::Unchanged => {}
            }
        }

        if diffs.is_empty() || diffs.iter().all(|d| d.change_type == ChangeType::Unchanged) {
            writeln!(output, "│ (no changes)").unwrap();
        }

        writeln!(output, "└───────────────────────────────────────────────┘").unwrap();

        output
    }

    /// Get the underlying debugger
    pub fn debugger(&self) -> &TimeTravelDebugger {
        &self.debugger
    }

    /// Get mutable access to the debugger
    pub fn debugger_mut(&mut self) -> &mut TimeTravelDebugger {
        &mut self.debugger
    }

    /// Record a function call
    pub fn record_call(&mut self, name: &str, line: u32, locals: HashMap<String, Value>, args: Vec<Value>) {
        self.call_stack.push(CallFrame {
            name: name.to_string(),
            line,
            locals,
            arguments: args,
        });
    }

    /// Record a function return
    pub fn record_return(&mut self) -> Option<CallFrame> {
        self.call_stack.pop()
    }
}

/// Heat map showing which lines were executed most
#[derive(Debug)]
pub struct ExecutionHeatMap {
    /// Execution count per line
    line_counts: HashMap<u32, u64>,
    /// Maximum execution count
    max_count: u64,
}

impl ExecutionHeatMap {
    /// Create a new heat map
    pub fn new() -> Self {
        Self {
            line_counts: HashMap::default(),
            max_count: 0,
        }
    }

    /// Record execution at a line
    pub fn record(&mut self, line: u32) {
        let count = self.line_counts.entry(line).or_insert(0);
        *count += 1;
        if *count > self.max_count {
            self.max_count = *count;
        }
    }

    /// Get execution count for a line
    pub fn count(&self, line: u32) -> u64 {
        self.line_counts.get(&line).copied().unwrap_or(0)
    }

    /// Get heat level (0.0 - 1.0) for a line
    pub fn heat(&self, line: u32) -> f64 {
        if self.max_count == 0 {
            return 0.0;
        }
        self.count(line) as f64 / self.max_count as f64
    }

    /// Get color code for a heat level
    pub fn heat_color(&self, line: u32) -> &'static str {
        let heat = self.heat(line);
        if heat == 0.0 {
            ""
        } else if heat < 0.2 {
            "\x1b[38;5;22m"  // Dark green
        } else if heat < 0.4 {
            "\x1b[38;5;28m"  // Green
        } else if heat < 0.6 {
            "\x1b[38;5;226m" // Yellow
        } else if heat < 0.8 {
            "\x1b[38;5;208m" // Orange
        } else {
            "\x1b[38;5;196m" // Red (hot)
        }
    }
}

impl Default for ExecutionHeatMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_stack() {
        let mut stack = CallStack::new();
        assert_eq!(stack.depth(), 0);

        stack.push(CallFrame {
            name: "main".to_string(),
            line: 1,
            locals: HashMap::default(),
            arguments: vec![],
        });
        assert_eq!(stack.depth(), 1);

        stack.push(CallFrame {
            name: "foo".to_string(),
            line: 10,
            locals: HashMap::default(),
            arguments: vec![Value::Number(42.0)],
        });
        assert_eq!(stack.depth(), 2);

        let frame = stack.pop().unwrap();
        assert_eq!(frame.name, "foo");
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_compute_diff() {
        let mut old_locals = HashMap::default();
        old_locals.insert("x".to_string(), Value::Number(1.0));
        old_locals.insert("y".to_string(), Value::String("hello".to_string()));

        let mut new_locals = HashMap::default();
        new_locals.insert("x".to_string(), Value::Number(2.0)); // Modified
        new_locals.insert("z".to_string(), Value::Boolean(true)); // Created
        // y is deleted

        let old_record = ExecutionRecord {
            step: 1,
            opcode: None,
            ip: 0,
            line: 1,
            stack_top: vec![],
            stack_size: 0,
            locals: old_locals,
            globals_changed: vec![],
            description: "old".to_string(),
        };

        let new_record = ExecutionRecord {
            step: 2,
            opcode: None,
            ip: 1,
            line: 2,
            stack_top: vec![],
            stack_size: 0,
            locals: new_locals,
            globals_changed: vec![],
            description: "new".to_string(),
        };

        let diffs = compute_diff(Some(&old_record), &new_record);
        assert_eq!(diffs.len(), 3);

        let created = diffs.iter().find(|d| d.change_type == ChangeType::Created).unwrap();
        assert_eq!(created.name, "z");

        let modified = diffs.iter().find(|d| d.change_type == ChangeType::Modified).unwrap();
        assert_eq!(modified.name, "x");

        let deleted = diffs.iter().find(|d| d.change_type == ChangeType::Deleted).unwrap();
        assert_eq!(deleted.name, "y");
    }

    #[test]
    fn test_timeline() {
        let mut timeline = Timeline::new(100, 50, 40);
        timeline.add_marker(25, MarkerType::Breakpoint, "bp1");
        timeline.add_marker(75, MarkerType::Error, "error");

        let rendered = timeline.render();
        assert!(rendered.contains("Step"));
        assert!(rendered.contains("100"));
    }

    #[test]
    fn test_heat_map() {
        let mut heat_map = ExecutionHeatMap::new();

        // Line 5 executed 10 times
        for _ in 0..10 {
            heat_map.record(5);
        }

        // Line 10 executed 5 times
        for _ in 0..5 {
            heat_map.record(10);
        }

        assert_eq!(heat_map.count(5), 10);
        assert_eq!(heat_map.count(10), 5);
        assert_eq!(heat_map.heat(5), 1.0);
        assert_eq!(heat_map.heat(10), 0.5);
        assert_eq!(heat_map.heat(1), 0.0);
    }
}
