//! Source Maps & Profiler Integration
//!
//! Provides source map support for mapping bytecode locations back to original
//! JavaScript/TypeScript source, plus a built-in CPU and memory profiler with
//! flame graph generation.
//!
//! # Example
//! ```text
//! let mut profiler = Profiler::new(ProfilerConfig::default());
//! profiler.start();
//! // ... run code ...
//! profiler.stop();
//! let report = profiler.report();
//! println!("{}", report.flame_graph_text());
//! ```

//! **Status:** ⚠️ Partial — CPU and memory profiling

use rustc_hash::FxHashMap as HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use crate::error::Result;

/// A mapping from generated position to original source position
#[derive(Debug, Clone)]
pub struct SourceMapping {
    /// Generated line (0-indexed)
    pub gen_line: u32,
    /// Generated column (0-indexed)
    pub gen_column: u32,
    /// Original source file index
    pub source_idx: u32,
    /// Original line (0-indexed)
    pub orig_line: u32,
    /// Original column (0-indexed)
    pub orig_column: u32,
    /// Original name index (if any)
    pub name_idx: Option<u32>,
}

/// A complete source map
#[derive(Debug, Clone)]
pub struct SourceMap {
    /// Version (always 3)
    pub version: u32,
    /// Source file names
    pub sources: Vec<String>,
    /// Source content (optional, for inline sources)
    pub sources_content: Vec<Option<String>>,
    /// Symbol names referenced by mappings
    pub names: Vec<String>,
    /// The mappings
    pub mappings: Vec<SourceMapping>,
    /// Generated file name
    pub file: Option<String>,
    /// Source root prefix
    pub source_root: Option<String>,
}

impl SourceMap {
    /// Create a new empty source map
    pub fn new() -> Self {
        Self {
            version: 3,
            sources: Vec::new(),
            sources_content: Vec::new(),
            names: Vec::new(),
            mappings: Vec::new(),
            file: None,
            source_root: None,
        }
    }

    /// Add a source file and return its index
    pub fn add_source(&mut self, path: &str) -> u32 {
        let idx = self.sources.len() as u32;
        self.sources.push(path.to_string());
        self.sources_content.push(None);
        idx
    }

    /// Add a source file with inline content
    pub fn add_source_with_content(&mut self, path: &str, content: &str) -> u32 {
        let idx = self.sources.len() as u32;
        self.sources.push(path.to_string());
        self.sources_content.push(Some(content.to_string()));
        idx
    }

    /// Add a name and return its index
    pub fn add_name(&mut self, name: &str) -> u32 {
        let idx = self.names.len() as u32;
        self.names.push(name.to_string());
        idx
    }

    /// Add a mapping
    pub fn add_mapping(&mut self, mapping: SourceMapping) {
        self.mappings.push(mapping);
    }

    /// Look up the original position for a generated position
    pub fn find_original(&self, gen_line: u32, gen_column: u32) -> Option<&SourceMapping> {
        // Find the closest mapping that doesn't exceed the target position
        let mut best: Option<&SourceMapping> = None;

        for mapping in &self.mappings {
            if mapping.gen_line == gen_line && mapping.gen_column <= gen_column {
                match best {
                    None => best = Some(mapping),
                    Some(prev) if mapping.gen_column > prev.gen_column => best = Some(mapping),
                    _ => {}
                }
            }
        }

        best
    }

    /// Resolve a mapping to a human-readable location string
    pub fn resolve_location(&self, gen_line: u32, gen_column: u32) -> Option<String> {
        self.find_original(gen_line, gen_column).map(|m| {
            let source = self.sources.get(m.source_idx as usize)
                .map(|s| s.as_str())
                .unwrap_or("<unknown>");
            let name = m.name_idx
                .and_then(|idx| self.names.get(idx as usize))
                .map(|n| format!(" ({})", n))
                .unwrap_or_default();
            format!("{}:{}:{}{}", source, m.orig_line + 1, m.orig_column + 1, name)
        })
    }

    /// Parse a VLQ-encoded source map from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        // Simple JSON parsing for source maps
        let mut map = SourceMap::new();

        // Extract "sources" array
        if let Some(sources_start) = json.find("\"sources\"") {
            if let Some(arr_start) = json[sources_start..].find('[') {
                let arr_begin = sources_start + arr_start;
                if let Some(arr_end) = json[arr_begin..].find(']') {
                    let arr_content = &json[arr_begin + 1..arr_begin + arr_end];
                    for source in arr_content.split(',') {
                        let source = source.trim().trim_matches('"');
                        if !source.is_empty() {
                            map.add_source(source);
                        }
                    }
                }
            }
        }

        // Extract "names" array
        if let Some(names_start) = json.find("\"names\"") {
            if let Some(arr_start) = json[names_start..].find('[') {
                let arr_begin = names_start + arr_start;
                if let Some(arr_end) = json[arr_begin..].find(']') {
                    let arr_content = &json[arr_begin + 1..arr_begin + arr_end];
                    for name in arr_content.split(',') {
                        let name = name.trim().trim_matches('"');
                        if !name.is_empty() {
                            map.add_name(name);
                        }
                    }
                }
            }
        }

        // Extract "mappings" string and decode VLQ
        if let Some(mappings_start) = json.find("\"mappings\"") {
            if let Some(val_start) = json[mappings_start..].find(':') {
                let after_colon = mappings_start + val_start + 1;
                let trimmed = json[after_colon..].trim();
                if let Some(stripped) = trimmed.strip_prefix('"') {
                    if let Some(end) = stripped.find('"') {
                        let mappings_str = &stripped[..end];
                        map.decode_vlq_mappings(mappings_str);
                    }
                }
            }
        }

        Ok(map)
    }

    /// Decode VLQ-encoded mappings string
    fn decode_vlq_mappings(&mut self, mappings: &str) {
        let mut gen_line: u32 = 0;
        let mut prev_col: i64 = 0;
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;

        for group in mappings.split(';') {
            let mut gen_column: u32;

            if group.is_empty() {
                gen_line += 1;
                continue;
            }

            for segment in group.split(',') {
                let values = decode_vlq(segment);
                if values.is_empty() { continue; }

                let col_delta = values[0];
                prev_col += col_delta;
                gen_column = prev_col as u32;

                if values.len() >= 4 {
                    prev_source += values[1];
                    prev_orig_line += values[2];
                    prev_orig_col += values[3];

                    let name_idx = if values.len() >= 5 {
                        Some(values[4] as u32)
                    } else {
                        None
                    };

                    self.mappings.push(SourceMapping {
                        gen_line,
                        gen_column,
                        source_idx: prev_source as u32,
                        orig_line: prev_orig_line as u32,
                        orig_column: prev_orig_col as u32,
                        name_idx,
                    });
                }
            }

            gen_line += 1;
        }
    }

    /// Encode mappings to VLQ string
    pub fn to_vlq_mappings(&self) -> String {
        if self.mappings.is_empty() {
            return String::new();
        }

        let mut result = String::new();
        let mut prev_gen_line: u32 = 0;
        let mut prev_col: i64 = 0;
        let mut prev_source: i64 = 0;
        let mut prev_orig_line: i64 = 0;
        let mut prev_orig_col: i64 = 0;

        let mut sorted = self.mappings.clone();
        sorted.sort_by_key(|m| (m.gen_line, m.gen_column));

        let mut first_in_line = true;

        for mapping in &sorted {
            // Add semicolons for skipped lines
            while prev_gen_line < mapping.gen_line {
                result.push(';');
                prev_gen_line += 1;
                prev_col = 0;
                first_in_line = true;
            }

            if !first_in_line {
                result.push(',');
            }

            let col_delta = mapping.gen_column as i64 - prev_col;
            prev_col = mapping.gen_column as i64;

            let source_delta = mapping.source_idx as i64 - prev_source;
            prev_source = mapping.source_idx as i64;

            let line_delta = mapping.orig_line as i64 - prev_orig_line;
            prev_orig_line = mapping.orig_line as i64;

            let orig_col_delta = mapping.orig_column as i64 - prev_orig_col;
            prev_orig_col = mapping.orig_column as i64;

            result.push_str(&encode_vlq(col_delta));
            result.push_str(&encode_vlq(source_delta));
            result.push_str(&encode_vlq(line_delta));
            result.push_str(&encode_vlq(orig_col_delta));

            first_in_line = false;
        }

        result
    }
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode a VLQ-encoded value
fn decode_vlq(segment: &str) -> Vec<i64> {
    let mut values = Vec::new();
    let mut shift = 0;
    let mut value: i64 = 0;

    for ch in segment.chars() {
        let digit = match vlq_char_to_digit(ch) {
            Some(d) => d,
            None => break,
        };

        let has_continuation = digit & 32 != 0;
        let digit_value = (digit & 31) as i64;
        value += digit_value << shift;
        shift += 5;

        if !has_continuation {
            // Sign is in the least significant bit
            let is_negative = value & 1 != 0;
            let abs_value = value >> 1;
            values.push(if is_negative { -abs_value } else { abs_value });
            value = 0;
            shift = 0;
        }
    }

    values
}

/// Encode a value as VLQ
fn encode_vlq(value: i64) -> String {
    let mut result = String::new();
    let mut vlq = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    };

    loop {
        let mut digit = (vlq & 31) as u8;
        vlq >>= 5;
        if vlq > 0 {
            digit |= 32; // continuation bit
        }
        result.push(vlq_digit_to_char(digit));
        if vlq == 0 { break; }
    }

    result
}

/// Convert a Base64 VLQ character to its numeric value
fn vlq_char_to_digit(ch: char) -> Option<u8> {
    match ch {
        'A'..='Z' => Some(ch as u8 - b'A'),
        'a'..='z' => Some(ch as u8 - b'a' + 26),
        '0'..='9' => Some(ch as u8 - b'0' + 52),
        '+' => Some(62),
        '/' => Some(63),
        _ => None,
    }
}

/// Convert a numeric value to a Base64 VLQ character
fn vlq_digit_to_char(digit: u8) -> char {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    CHARS[digit as usize] as char
}

// ============================================================================
// Profiler
// ============================================================================

/// Profiler configuration
#[derive(Debug, Clone)]
pub struct ProfilerConfig {
    /// Enable CPU profiling
    pub cpu_profiling: bool,
    /// Enable memory profiling
    pub memory_profiling: bool,
    /// Sampling interval for CPU profiler
    pub sample_interval: Duration,
    /// Maximum number of stack frames to capture
    pub max_stack_depth: usize,
    /// Whether to include native frames
    pub include_native_frames: bool,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            cpu_profiling: true,
            memory_profiling: true,
            sample_interval: Duration::from_micros(100),
            max_stack_depth: 128,
            include_native_frames: false,
        }
    }
}

/// A single stack frame in a profile sample
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ProfileFrame {
    /// Function name
    pub function_name: String,
    /// Source file (if known)
    pub file: Option<String>,
    /// Line number
    pub line: u32,
    /// Column number
    pub column: u32,
    /// Whether this is a native function
    pub is_native: bool,
}

impl fmt::Display for ProfileFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_native {
            write!(f, "{} (native)", self.function_name)
        } else if let Some(ref file) = self.file {
            write!(f, "{} ({}:{}:{})", self.function_name, file, self.line, self.column)
        } else {
            write!(f, "{}", self.function_name)
        }
    }
}

/// A CPU profile sample
#[derive(Debug, Clone)]
pub struct CpuSample {
    /// Timestamp relative to profiler start
    pub timestamp: Duration,
    /// Stack frames (bottom to top)
    pub stack: Vec<ProfileFrame>,
}

/// Memory allocation event
#[derive(Debug, Clone)]
pub struct AllocationEvent {
    /// Timestamp relative to profiler start
    pub timestamp: Duration,
    /// Size in bytes
    pub size: usize,
    /// Allocation type
    pub kind: AllocationType,
    /// Stack at time of allocation
    pub stack: Vec<ProfileFrame>,
}

/// Type of memory allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationType {
    /// Object allocation
    Object,
    /// String allocation
    StringAlloc,
    /// Array allocation
    Array,
    /// Function closure
    Closure,
    /// Other allocation
    Other,
}

impl fmt::Display for AllocationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AllocationType::Object => write!(f, "Object"),
            AllocationType::StringAlloc => write!(f, "String"),
            AllocationType::Array => write!(f, "Array"),
            AllocationType::Closure => write!(f, "Closure"),
            AllocationType::Other => write!(f, "Other"),
        }
    }
}

/// A node in the flame graph tree
#[derive(Debug, Clone)]
pub struct FlameNode {
    /// Frame info
    pub frame: ProfileFrame,
    /// Self time (time spent in this function, not children)
    pub self_time: Duration,
    /// Total time (including children)
    pub total_time: Duration,
    /// Number of samples that hit this node
    pub hit_count: u64,
    /// Child nodes
    pub children: Vec<FlameNode>,
}

impl FlameNode {
    fn new(frame: ProfileFrame) -> Self {
        Self {
            frame,
            self_time: Duration::ZERO,
            total_time: Duration::ZERO,
            hit_count: 0,
            children: Vec::new(),
        }
    }

    /// Format as a text flame graph (folded stacks format)
    pub fn to_folded_stacks(&self, prefix: &str, output: &mut String) {
        let path = if prefix.is_empty() {
            self.frame.function_name.clone()
        } else {
            format!("{};{}", prefix, self.frame.function_name)
        };

        if self.hit_count > 0 {
            output.push_str(&format!("{} {}\n", path, self.hit_count));
        }

        for child in &self.children {
            child.to_folded_stacks(&path, output);
        }
    }
}

/// Function-level profiling statistics
#[derive(Debug, Clone)]
pub struct FunctionStats {
    /// Function name
    pub name: String,
    /// Total time spent in this function (inclusive)
    pub total_time: Duration,
    /// Self time (exclusive of called functions)
    pub self_time: Duration,
    /// Number of calls
    pub call_count: u64,
    /// Average time per call
    pub avg_time: Duration,
    /// Total allocations attributed to this function
    pub allocations: usize,
    /// Total bytes allocated
    pub bytes_allocated: usize,
}

/// Profile report containing all profiling data
#[derive(Debug)]
pub struct ProfileReport {
    /// Total profiling duration
    pub duration: Duration,
    /// CPU samples collected
    pub cpu_samples: Vec<CpuSample>,
    /// Memory allocation events
    pub allocation_events: Vec<AllocationEvent>,
    /// Function-level statistics
    pub function_stats: Vec<FunctionStats>,
    /// Flame graph root nodes
    pub flame_roots: Vec<FlameNode>,
    /// Total memory allocated
    pub total_allocated: usize,
    /// Peak memory usage
    pub peak_memory: usize,
}

impl ProfileReport {
    /// Generate folded stacks text (compatible with flamegraph tools)
    pub fn flame_graph_text(&self) -> String {
        let mut output = String::new();
        for root in &self.flame_roots {
            root.to_folded_stacks("", &mut output);
        }
        output
    }

    /// Get top N functions by self time
    pub fn top_functions_by_self_time(&self, n: usize) -> Vec<&FunctionStats> {
        let mut stats: Vec<&FunctionStats> = self.function_stats.iter().collect();
        stats.sort_by(|a, b| b.self_time.cmp(&a.self_time));
        stats.truncate(n);
        stats
    }

    /// Get top N functions by total time
    pub fn top_functions_by_total_time(&self, n: usize) -> Vec<&FunctionStats> {
        let mut stats: Vec<&FunctionStats> = self.function_stats.iter().collect();
        stats.sort_by(|a, b| b.total_time.cmp(&a.total_time));
        stats.truncate(n);
        stats
    }

    /// Get top N functions by allocation count
    pub fn top_functions_by_allocations(&self, n: usize) -> Vec<&FunctionStats> {
        let mut stats: Vec<&FunctionStats> = self.function_stats.iter().collect();
        stats.sort_by(|a, b| b.allocations.cmp(&a.allocations));
        stats.truncate(n);
        stats
    }

    /// Format a summary of the profile report
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("Profile Duration: {:?}\n", self.duration));
        s.push_str(&format!("CPU Samples: {}\n", self.cpu_samples.len()));
        s.push_str(&format!("Allocation Events: {}\n", self.allocation_events.len()));
        s.push_str(&format!("Total Allocated: {} bytes\n", self.total_allocated));
        s.push_str(&format!("Peak Memory: {} bytes\n", self.peak_memory));
        s.push_str(&format!("Functions Profiled: {}\n", self.function_stats.len()));

        if !self.function_stats.is_empty() {
            s.push_str("\nTop Functions by Self Time:\n");
            for (i, func) in self.top_functions_by_self_time(10).iter().enumerate() {
                s.push_str(&format!(
                    "  {}. {} - self: {:?}, total: {:?}, calls: {}\n",
                    i + 1, func.name, func.self_time, func.total_time, func.call_count
                ));
            }
        }

        s
    }
}

/// The main profiler that collects runtime performance data
pub struct Profiler {
    /// Configuration
    config: ProfilerConfig,
    /// Whether profiling is active
    active: bool,
    /// Profiler start time
    start_time: Option<Instant>,
    /// Collected CPU samples
    cpu_samples: Vec<CpuSample>,
    /// Collected allocation events
    allocation_events: Vec<AllocationEvent>,
    /// Running function call tracker: function_name -> (call_count, total_duration, self_duration)
    function_tracker: HashMap<String, (u64, Duration, Duration)>,
    /// Memory tracker: function_name -> (alloc_count, total_bytes)
    memory_tracker: HashMap<String, (usize, usize)>,
    /// Current call stack for tracking enter/exit
    call_stack: Vec<(String, Instant)>,
    /// Peak memory observed
    peak_memory: usize,
    /// Current memory usage
    current_memory: usize,
    /// Total allocated
    total_allocated: usize,
}

impl Profiler {
    /// Create a new profiler
    pub fn new(config: ProfilerConfig) -> Self {
        Self {
            config,
            active: false,
            start_time: None,
            cpu_samples: Vec::new(),
            allocation_events: Vec::new(),
            function_tracker: HashMap::default(),
            memory_tracker: HashMap::default(),
            call_stack: Vec::new(),
            peak_memory: 0,
            current_memory: 0,
            total_allocated: 0,
        }
    }

    /// Start profiling
    pub fn start(&mut self) {
        self.active = true;
        self.start_time = Some(Instant::now());
    }

    /// Stop profiling
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Check if profiling is active
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Record entering a function
    pub fn enter_function(&mut self, name: &str) {
        if !self.active { return; }
        self.call_stack.push((name.to_string(), Instant::now()));
    }

    /// Record exiting a function
    pub fn exit_function(&mut self, name: &str) {
        if !self.active { return; }

        if let Some((stack_name, enter_time)) = self.call_stack.pop() {
            if stack_name != name { return; } // mismatched, ignore

            let elapsed = enter_time.elapsed();

            let entry = self.function_tracker
                .entry(name.to_string())
                .or_insert((0, Duration::ZERO, Duration::ZERO));
            entry.0 += 1;
            entry.1 += elapsed;
            // Self time: subtract time spent in direct children that were tracked
            // (approximation - actual self time accounting needs more infrastructure)
            entry.2 += elapsed;
        }
    }

    /// Record a CPU sample with the current stack
    pub fn record_sample(&mut self, stack: Vec<ProfileFrame>) {
        if !self.active { return; }

        let timestamp = self.start_time
            .map(|s| s.elapsed())
            .unwrap_or_default();

        self.cpu_samples.push(CpuSample { timestamp, stack });
    }

    /// Record a memory allocation
    pub fn record_allocation(&mut self, size: usize, kind: AllocationType, stack: Vec<ProfileFrame>) {
        if !self.active { return; }

        self.total_allocated += size;
        self.current_memory += size;
        if self.current_memory > self.peak_memory {
            self.peak_memory = self.current_memory;
        }

        // Track per-function allocations
        if let Some(frame) = stack.last() {
            let entry = self.memory_tracker
                .entry(frame.function_name.clone())
                .or_insert((0, 0));
            entry.0 += 1;
            entry.1 += size;
        }

        if self.config.memory_profiling {
            let timestamp = self.start_time
                .map(|s| s.elapsed())
                .unwrap_or_default();

            self.allocation_events.push(AllocationEvent {
                timestamp,
                size,
                kind,
                stack,
            });
        }
    }

    /// Record a memory deallocation
    pub fn record_deallocation(&mut self, size: usize) {
        if !self.active { return; }
        self.current_memory = self.current_memory.saturating_sub(size);
    }

    /// Generate the profile report
    pub fn report(&self) -> ProfileReport {
        let duration = self.start_time
            .map(|s| s.elapsed())
            .unwrap_or_default();

        // Build function stats
        let mut function_stats: Vec<FunctionStats> = self.function_tracker.iter()
            .map(|(name, (count, total, self_time))| {
                let (allocs, bytes) = self.memory_tracker
                    .get(name)
                    .copied()
                    .unwrap_or((0, 0));

                FunctionStats {
                    name: name.clone(),
                    total_time: *total,
                    self_time: *self_time,
                    call_count: *count,
                    avg_time: if *count > 0 { *total / *count as u32 } else { Duration::ZERO },
                    allocations: allocs,
                    bytes_allocated: bytes,
                }
            })
            .collect();

        function_stats.sort_by(|a, b| b.self_time.cmp(&a.self_time));

        // Build flame graph from CPU samples
        let flame_roots = self.build_flame_graph();

        ProfileReport {
            duration,
            cpu_samples: self.cpu_samples.clone(),
            allocation_events: self.allocation_events.clone(),
            function_stats,
            flame_roots,
            total_allocated: self.total_allocated,
            peak_memory: self.peak_memory,
        }
    }

    /// Build flame graph from CPU samples
    fn build_flame_graph(&self) -> Vec<FlameNode> {
        let mut roots: Vec<FlameNode> = Vec::new();

        for sample in &self.cpu_samples {
            if sample.stack.is_empty() { continue; }
            Self::insert_sample(&mut roots, &sample.stack, 0);
        }

        roots
    }

    /// Insert a sample into the flame graph tree
    fn insert_sample(nodes: &mut Vec<FlameNode>, stack: &[ProfileFrame], depth: usize) {
        if depth >= stack.len() { return; }

        let frame = &stack[depth];

        // Find or create node for this frame
        let node_idx = nodes.iter().position(|n| n.frame == *frame);
        let node_idx = match node_idx {
            Some(idx) => idx,
            None => {
                nodes.push(FlameNode::new(frame.clone()));
                nodes.len() - 1
            }
        };

        nodes[node_idx].hit_count += 1;

        if depth + 1 < stack.len() {
            Self::insert_sample(&mut nodes[node_idx].children, stack, depth + 1);
        }
    }

    /// Reset the profiler, clearing all collected data
    pub fn reset(&mut self) {
        self.active = false;
        self.start_time = None;
        self.cpu_samples.clear();
        self.allocation_events.clear();
        self.function_tracker.clear();
        self.memory_tracker.clear();
        self.call_stack.clear();
        self.peak_memory = 0;
        self.current_memory = 0;
        self.total_allocated = 0;
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new(ProfilerConfig::default())
    }
}

/// Source map builder for mapping bytecode back to source
pub struct SourceMapBuilder {
    source_map: SourceMap,
    current_source: u32,
}

impl SourceMapBuilder {
    /// Create a new source map builder
    pub fn new(source_file: &str) -> Self {
        let mut map = SourceMap::new();
        let idx = map.add_source(source_file);
        Self {
            source_map: map,
            current_source: idx,
        }
    }

    /// Add a bytecode-to-source mapping
    pub fn add_mapping(
        &mut self,
        bytecode_offset: u32,
        source_line: u32,
        source_column: u32,
        name: Option<&str>,
    ) {
        let name_idx = name.map(|n| self.source_map.add_name(n));
        self.source_map.add_mapping(SourceMapping {
            gen_line: 0, // bytecode is single "line"
            gen_column: bytecode_offset,
            source_idx: self.current_source,
            orig_line: source_line,
            orig_column: source_column,
            name_idx,
        });
    }

    /// Build the final source map
    pub fn build(self) -> SourceMap {
        self.source_map
    }
}

/// Resolve a bytecode offset to a source location using a source map
pub fn resolve_bytecode_location(
    source_map: &SourceMap,
    bytecode_offset: u32,
) -> Option<String> {
    source_map.resolve_location(0, bytecode_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_map_creation() {
        let mut map = SourceMap::new();
        let idx = map.add_source("test.js");
        assert_eq!(idx, 0);
        assert_eq!(map.sources[0], "test.js");
    }

    #[test]
    fn test_source_map_find_original() {
        let mut map = SourceMap::new();
        map.add_source("test.js");
        map.add_mapping(SourceMapping {
            gen_line: 0,
            gen_column: 0,
            source_idx: 0,
            orig_line: 5,
            orig_column: 10,
            name_idx: None,
        });
        map.add_mapping(SourceMapping {
            gen_line: 0,
            gen_column: 10,
            source_idx: 0,
            orig_line: 6,
            orig_column: 0,
            name_idx: None,
        });

        let found = map.find_original(0, 5).unwrap();
        assert_eq!(found.orig_line, 5);
        assert_eq!(found.orig_column, 10);

        let found = map.find_original(0, 15).unwrap();
        assert_eq!(found.orig_line, 6);
    }

    #[test]
    fn test_source_map_resolve_location() {
        let mut map = SourceMap::new();
        map.add_source("app.js");
        let name_idx = map.add_name("myFunc");
        map.add_mapping(SourceMapping {
            gen_line: 0,
            gen_column: 0,
            source_idx: 0,
            orig_line: 10,
            orig_column: 4,
            name_idx: Some(name_idx),
        });

        let loc = map.resolve_location(0, 0).unwrap();
        assert_eq!(loc, "app.js:11:5 (myFunc)");
    }

    #[test]
    fn test_vlq_encode_decode() {
        let test_cases = vec![0i64, 1, -1, 5, -5, 100, -100, 1000];
        for val in test_cases {
            let encoded = encode_vlq(val);
            let decoded = decode_vlq(&encoded);
            assert_eq!(decoded.len(), 1);
            assert_eq!(decoded[0], val, "VLQ roundtrip failed for {}", val);
        }
    }

    #[test]
    fn test_vlq_mappings_roundtrip() {
        let mut map = SourceMap::new();
        map.add_source("test.js");
        map.add_mapping(SourceMapping {
            gen_line: 0,
            gen_column: 0,
            source_idx: 0,
            orig_line: 0,
            orig_column: 0,
            name_idx: None,
        });
        map.add_mapping(SourceMapping {
            gen_line: 0,
            gen_column: 5,
            source_idx: 0,
            orig_line: 0,
            orig_column: 5,
            name_idx: None,
        });
        map.add_mapping(SourceMapping {
            gen_line: 1,
            gen_column: 0,
            source_idx: 0,
            orig_line: 1,
            orig_column: 0,
            name_idx: None,
        });

        let encoded = map.to_vlq_mappings();
        assert!(!encoded.is_empty());

        let mut map2 = SourceMap::new();
        map2.add_source("test.js");
        map2.decode_vlq_mappings(&encoded);
        assert_eq!(map2.mappings.len(), 3);
    }

    #[test]
    fn test_profiler_lifecycle() {
        let mut profiler = Profiler::new(ProfilerConfig::default());
        assert!(!profiler.is_active());

        profiler.start();
        assert!(profiler.is_active());

        profiler.enter_function("main");
        profiler.enter_function("helper");
        profiler.exit_function("helper");
        profiler.exit_function("main");

        profiler.stop();
        assert!(!profiler.is_active());

        let report = profiler.report();
        assert!(!report.function_stats.is_empty());
    }

    #[test]
    fn test_profiler_function_tracking() {
        let mut profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        for _ in 0..5 {
            profiler.enter_function("hotFunc");
            profiler.exit_function("hotFunc");
        }

        profiler.stop();
        let report = profiler.report();

        let hot_func = report.function_stats.iter().find(|f| f.name == "hotFunc");
        assert!(hot_func.is_some());
        assert_eq!(hot_func.unwrap().call_count, 5);
    }

    #[test]
    fn test_profiler_memory_tracking() {
        let mut profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        let frame = ProfileFrame {
            function_name: "allocator".to_string(),
            file: Some("test.js".to_string()),
            line: 1,
            column: 0,
            is_native: false,
        };

        profiler.record_allocation(1024, AllocationType::Object, vec![frame.clone()]);
        profiler.record_allocation(2048, AllocationType::Array, vec![frame]);
        profiler.record_deallocation(1024);

        profiler.stop();
        let report = profiler.report();

        assert_eq!(report.total_allocated, 3072);
        assert_eq!(report.peak_memory, 3072);
        assert_eq!(report.allocation_events.len(), 2);
    }

    #[test]
    fn test_profiler_cpu_samples() {
        let mut profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        let stack = vec![
            ProfileFrame { function_name: "main".to_string(), file: None, line: 1, column: 0, is_native: false },
            ProfileFrame { function_name: "compute".to_string(), file: None, line: 10, column: 0, is_native: false },
        ];

        profiler.record_sample(stack.clone());
        profiler.record_sample(stack.clone());
        profiler.record_sample(stack);

        profiler.stop();
        let report = profiler.report();

        assert_eq!(report.cpu_samples.len(), 3);
        assert!(!report.flame_roots.is_empty());

        let text = report.flame_graph_text();
        assert!(text.contains("main"));
    }

    #[test]
    fn test_profiler_reset() {
        let mut profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();
        profiler.enter_function("test");
        profiler.exit_function("test");
        profiler.record_allocation(100, AllocationType::Other, vec![]);

        profiler.reset();
        assert!(!profiler.is_active());

        let report = profiler.report();
        assert!(report.function_stats.is_empty());
        assert_eq!(report.total_allocated, 0);
    }

    #[test]
    fn test_source_map_builder() {
        let mut builder = SourceMapBuilder::new("app.ts");
        builder.add_mapping(0, 0, 0, Some("init"));
        builder.add_mapping(10, 5, 4, Some("render"));
        builder.add_mapping(20, 10, 0, None);

        let map = builder.build();
        assert_eq!(map.sources.len(), 1);
        assert_eq!(map.mappings.len(), 3);
        assert_eq!(map.names.len(), 2);
    }

    #[test]
    fn test_flame_node_folded_stacks() {
        let mut root = FlameNode::new(ProfileFrame {
            function_name: "main".to_string(),
            file: None,
            line: 0,
            column: 0,
            is_native: false,
        });
        root.hit_count = 10;

        let mut child = FlameNode::new(ProfileFrame {
            function_name: "render".to_string(),
            file: None,
            line: 0,
            column: 0,
            is_native: false,
        });
        child.hit_count = 7;
        root.children.push(child);

        let mut output = String::new();
        root.to_folded_stacks("", &mut output);
        assert!(output.contains("main 10"));
        assert!(output.contains("main;render 7"));
    }

    #[test]
    fn test_profile_report_summary() {
        let report = ProfileReport {
            duration: Duration::from_secs(1),
            cpu_samples: vec![],
            allocation_events: vec![],
            function_stats: vec![
                FunctionStats {
                    name: "hot".to_string(),
                    total_time: Duration::from_millis(500),
                    self_time: Duration::from_millis(300),
                    call_count: 100,
                    avg_time: Duration::from_millis(5),
                    allocations: 50,
                    bytes_allocated: 4096,
                },
            ],
            flame_roots: vec![],
            total_allocated: 4096,
            peak_memory: 2048,
        };

        let summary = report.summary();
        assert!(summary.contains("Profile Duration"));
        assert!(summary.contains("hot"));
    }

    #[test]
    fn test_profile_frame_display() {
        let native = ProfileFrame {
            function_name: "parseInt".to_string(),
            file: None,
            line: 0,
            column: 0,
            is_native: true,
        };
        assert!(format!("{}", native).contains("native"));

        let js = ProfileFrame {
            function_name: "myFunc".to_string(),
            file: Some("app.js".to_string()),
            line: 42,
            column: 8,
            is_native: false,
        };
        assert!(format!("{}", js).contains("app.js:42:8"));
    }
}
