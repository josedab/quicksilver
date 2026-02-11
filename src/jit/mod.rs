//! JIT Compilation Tier with Type Profiling
//!
//! Provides type profiling infrastructure, hot function detection, and
//! specialized fast-path compilation for frequently executed bytecode.
//! This serves as the foundation for a baseline JIT compiler.

//! **Status:** 🧪 Experimental — Basic JIT compilation framework — not production-ready

use crate::bytecode::Chunk;
use rustc_hash::FxHashMap as HashMap;
use std::time::Duration;

/// Type feedback collected during interpretation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservedType {
    Undefined,
    Null,
    Boolean,
    Int32,
    Float64,
    String,
    Object,
    Array,
    Function,
    BigInt,
    Symbol,
    Mixed,
}

impl ObservedType {
    /// Merge two observed types (becomes Mixed if different)
    pub fn merge(self, other: ObservedType) -> ObservedType {
        if self == other {
            self
        } else {
            ObservedType::Mixed
        }
    }

    /// Check if this type is a number type
    pub fn is_numeric(&self) -> bool {
        matches!(self, ObservedType::Int32 | ObservedType::Float64)
    }
}

/// Type profile for a single bytecode instruction
#[derive(Debug, Clone)]
pub struct TypeProfile {
    /// Observed operand types (left, right for binary ops)
    pub operand_types: Vec<ObservedType>,
    /// Observed result type
    pub result_type: ObservedType,
    /// Number of times this profile has been recorded
    pub sample_count: u64,
    /// Whether the type is stable (monomorphic)
    pub is_stable: bool,
}

impl TypeProfile {
    pub fn new() -> Self {
        Self {
            operand_types: Vec::new(),
            result_type: ObservedType::Undefined,
            sample_count: 0,
            is_stable: true,
        }
    }

    /// Record a type observation
    pub fn record(&mut self, operands: &[ObservedType], result: ObservedType) {
        self.sample_count += 1;
        if self.sample_count == 1 {
            self.operand_types = operands.to_vec();
            self.result_type = result;
        } else {
            // Check if types match previous observations
            if self.operand_types.len() == operands.len() {
                for (i, ty) in operands.iter().enumerate() {
                    let merged = self.operand_types[i].merge(*ty);
                    if merged == ObservedType::Mixed {
                        self.is_stable = false;
                    }
                    self.operand_types[i] = merged;
                }
            } else {
                self.is_stable = false;
            }
            let merged = self.result_type.merge(result);
            if merged == ObservedType::Mixed {
                self.is_stable = false;
            }
            self.result_type = merged;
        }
    }
}

impl Default for TypeProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// Execution counter for a function/chunk
#[derive(Debug, Clone)]
pub struct ExecutionCounter {
    /// Total number of invocations
    pub invocation_count: u64,
    /// Total number of bytecode operations executed
    pub operation_count: u64,
    /// Total time spent executing
    pub total_time: Duration,
    /// Number of times this function triggered deoptimization
    pub deopt_count: u32,
    /// Current compilation tier
    pub tier: CompilationTier,
    /// Whether this function is a candidate for JIT compilation
    pub jit_candidate: bool,
}

impl ExecutionCounter {
    pub fn new() -> Self {
        Self {
            invocation_count: 0,
            operation_count: 0,
            total_time: Duration::ZERO,
            deopt_count: 0,
            tier: CompilationTier::Interpreter,
            jit_candidate: false,
        }
    }

    /// Record a function invocation
    pub fn record_invocation(&mut self, duration: Duration, ops: u64) {
        self.invocation_count += 1;
        self.operation_count += ops;
        self.total_time += duration;
        self.jit_candidate = self.should_compile();
    }

    /// Check if this function should be compiled to a higher tier
    fn should_compile(&self) -> bool {
        match self.tier {
            CompilationTier::Interpreter => {
                self.invocation_count >= HOT_FUNCTION_THRESHOLD
                    || self.operation_count >= HOT_LOOP_THRESHOLD
            }
            CompilationTier::Baseline => {
                self.invocation_count >= OPTIMIZED_THRESHOLD && self.deopt_count < MAX_DEOPT_COUNT
            }
            CompilationTier::Optimized => false,
        }
    }

    /// Average time per invocation
    pub fn avg_time(&self) -> Duration {
        if self.invocation_count == 0 {
            Duration::ZERO
        } else {
            self.total_time / self.invocation_count as u32
        }
    }
}

impl Default for ExecutionCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Compilation tier for a function
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompilationTier {
    /// Interpreted (baseline)
    Interpreter,
    /// Baseline compiled with type guards
    Baseline,
    /// Fully optimized with inlining
    Optimized,
}

/// Thresholds for tiered compilation
const HOT_FUNCTION_THRESHOLD: u64 = 1000;
const HOT_LOOP_THRESHOLD: u64 = 10_000;
const OPTIMIZED_THRESHOLD: u64 = 10_000;
const MAX_DEOPT_COUNT: u32 = 5;

/// Specialized fast-path operations for common type patterns
#[derive(Debug, Clone)]
pub enum SpecializedOp {
    /// Integer addition (both operands are Int32)
    IntAdd,
    /// Float addition (both operands are Float64)
    FloatAdd,
    /// Integer subtraction
    IntSub,
    /// Float subtraction
    FloatSub,
    /// Integer multiplication
    IntMul,
    /// Float multiplication
    FloatMul,
    /// Integer comparison (less than)
    IntLt,
    /// Float comparison (less than)
    FloatLt,
    /// Integer equality
    IntEq,
    /// String concatenation
    StringConcat,
    /// String equality
    StringEq,
    /// Array element access (Int32 index)
    ArrayGetInt,
    /// Array element set (Int32 index)
    ArraySetInt,
    /// Object property access (known offset)
    ObjectGetCached { offset: u32 },
    /// Object property set (known offset)
    ObjectSetCached { offset: u32 },
    /// Known function call (monomorphic)
    DirectCall { function_id: u32 },
    /// Integer increment (++i pattern)
    IntIncrement,
    /// Integer decrement (--i pattern)
    IntDecrement,
    /// Fallback to generic operation
    Generic,
}

/// Compiled fast-path for a basic block
#[derive(Debug, Clone)]
pub struct CompiledBlock {
    /// Starting bytecode offset
    pub start_offset: usize,
    /// End bytecode offset (exclusive)
    pub end_offset: usize,
    /// Specialized operations
    pub ops: Vec<SpecializedOp>,
    /// Type guards that must pass for fast path to be valid
    pub guards: Vec<TypeGuard>,
    /// Number of successful fast-path executions
    pub hit_count: u64,
    /// Number of fallbacks to interpreter
    pub miss_count: u64,
}

impl CompiledBlock {
    /// Check if this block should be invalidated (too many misses)
    pub fn should_invalidate(&self) -> bool {
        let total = self.hit_count + self.miss_count;
        total > 100 && (self.miss_count as f64 / total as f64) > 0.2
    }
}

/// A type guard that must be satisfied for a specialized path
#[derive(Debug, Clone)]
pub struct TypeGuard {
    /// Stack index to check
    pub stack_index: usize,
    /// Expected type
    pub expected_type: ObservedType,
}

/// The type profiler collects type information during interpretation
pub struct TypeProfiler {
    /// Per-instruction type profiles, keyed by (chunk_id, instruction_offset)
    profiles: HashMap<(u64, usize), TypeProfile>,
    /// Per-function execution counters, keyed by chunk_id
    counters: HashMap<u64, ExecutionCounter>,
    /// Compiled fast-path blocks
    compiled_blocks: HashMap<u64, Vec<CompiledBlock>>,
    /// Whether profiling is enabled
    enabled: bool,
    /// Next chunk ID to assign
    next_chunk_id: u64,
    /// Statistics
    stats: ProfilerStats,
}

/// Profiler statistics
#[derive(Debug, Clone, Default)]
pub struct ProfilerStats {
    pub total_profiles: usize,
    pub stable_profiles: usize,
    pub mixed_profiles: usize,
    pub functions_profiled: usize,
    pub hot_functions: usize,
    pub compiled_blocks: usize,
    pub fast_path_hits: u64,
    pub fast_path_misses: u64,
}

impl TypeProfiler {
    pub fn new() -> Self {
        Self {
            profiles: HashMap::default(),
            counters: HashMap::default(),
            compiled_blocks: HashMap::default(),
            enabled: true,
            next_chunk_id: 0,
            stats: ProfilerStats::default(),
        }
    }

    /// Assign a unique ID to a chunk for profiling
    pub fn register_chunk(&mut self, _chunk: &Chunk) -> u64 {
        let id = self.next_chunk_id;
        self.next_chunk_id += 1;
        self.counters.insert(id, ExecutionCounter::new());
        id
    }

    /// Record a type observation for an instruction
    pub fn record_types(
        &mut self,
        chunk_id: u64,
        offset: usize,
        operands: &[ObservedType],
        result: ObservedType,
    ) {
        if !self.enabled {
            return;
        }
        let profile = self
            .profiles
            .entry((chunk_id, offset))
            .or_default();
        profile.record(operands, result);
    }

    /// Record a function invocation
    pub fn record_invocation(&mut self, chunk_id: u64, duration: Duration, ops: u64) {
        if let Some(counter) = self.counters.get_mut(&chunk_id) {
            counter.record_invocation(duration, ops);
        }
    }

    /// Get functions that are candidates for compilation
    pub fn hot_functions(&self) -> Vec<u64> {
        self.counters
            .iter()
            .filter(|(_, counter)| counter.jit_candidate)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Generate specialized operations for a hot function
    pub fn compile_fast_paths(&mut self, chunk_id: u64) -> Option<Vec<CompiledBlock>> {
        let counter = self.counters.get_mut(&chunk_id)?;
        if counter.tier >= CompilationTier::Baseline {
            return None;
        }

        // Collect all stable profiles for this chunk
        let stable_profiles: Vec<_> = self
            .profiles
            .iter()
            .filter(|((cid, _), profile)| *cid == chunk_id && profile.is_stable && profile.sample_count > 10)
            .map(|((_, offset), profile)| (*offset, profile.clone()))
            .collect();

        if stable_profiles.is_empty() {
            return None;
        }

        // Generate specialized blocks for contiguous stable regions
        let mut blocks = Vec::new();
        let mut current_block_ops = Vec::new();
        let mut current_guards = Vec::new();
        let mut block_start = stable_profiles.first().map(|(o, _)| *o).unwrap_or(0);

        for (offset, profile) in &stable_profiles {
            let specialized = Self::specialize_operation(profile);
            if let SpecializedOp::Generic = specialized {
                // End current block if we hit a non-specializable op
                if !current_block_ops.is_empty() {
                    blocks.push(CompiledBlock {
                        start_offset: block_start,
                        end_offset: *offset,
                        ops: std::mem::take(&mut current_block_ops),
                        guards: std::mem::take(&mut current_guards),
                        hit_count: 0,
                        miss_count: 0,
                    });
                }
                block_start = offset + 1;
            } else {
                if current_block_ops.is_empty() {
                    block_start = *offset;
                }
                // Add type guards for this op
                for (i, ty) in profile.operand_types.iter().enumerate() {
                    current_guards.push(TypeGuard {
                        stack_index: i,
                        expected_type: *ty,
                    });
                }
                current_block_ops.push(specialized);
            }
        }

        // Close final block
        if !current_block_ops.is_empty() {
            let last_offset = stable_profiles.last().map(|(o, _)| *o).unwrap_or(0);
            blocks.push(CompiledBlock {
                start_offset: block_start,
                end_offset: last_offset + 1,
                ops: current_block_ops,
                guards: current_guards,
                hit_count: 0,
                miss_count: 0,
            });
        }

        if !blocks.is_empty() {
            counter.tier = CompilationTier::Baseline;
            let block_count = blocks.len();
            self.compiled_blocks.insert(chunk_id, blocks.clone());
            self.stats.compiled_blocks += block_count;
            Some(blocks)
        } else {
            None
        }
    }

    /// Map a type profile to a specialized operation
    fn specialize_operation(profile: &TypeProfile) -> SpecializedOp {
        if profile.operand_types.len() == 2 {
            let left = profile.operand_types[0];
            let right = profile.operand_types[1];

            match (left, right, profile.result_type) {
                (ObservedType::Int32, ObservedType::Int32, ObservedType::Int32) => {
                    // Could be add, sub, mul - would need opcode context
                    SpecializedOp::IntAdd
                }
                (ObservedType::Float64, ObservedType::Float64, ObservedType::Float64) => {
                    SpecializedOp::FloatAdd
                }
                (ObservedType::String, ObservedType::String, ObservedType::String) => {
                    SpecializedOp::StringConcat
                }
                (ObservedType::Int32, ObservedType::Int32, ObservedType::Boolean) => {
                    SpecializedOp::IntLt
                }
                (ObservedType::String, ObservedType::String, ObservedType::Boolean) => {
                    SpecializedOp::StringEq
                }
                (ObservedType::Array, ObservedType::Int32, _) => SpecializedOp::ArrayGetInt,
                _ => SpecializedOp::Generic,
            }
        } else if profile.operand_types.len() == 1 {
            match (profile.operand_types[0], profile.result_type) {
                (ObservedType::Int32, ObservedType::Int32) => SpecializedOp::IntIncrement,
                _ => SpecializedOp::Generic,
            }
        } else {
            SpecializedOp::Generic
        }
    }

    /// Record a deoptimization event
    pub fn record_deopt(&mut self, chunk_id: u64) {
        if let Some(counter) = self.counters.get_mut(&chunk_id) {
            counter.deopt_count += 1;
            if counter.deopt_count >= MAX_DEOPT_COUNT {
                // Too many deopts - revert to interpreter
                counter.tier = CompilationTier::Interpreter;
                counter.jit_candidate = false;
                self.compiled_blocks.remove(&chunk_id);
            }
        }
    }

    /// Get compiled blocks for a function
    pub fn get_compiled_blocks(&self, chunk_id: u64) -> Option<&Vec<CompiledBlock>> {
        self.compiled_blocks.get(&chunk_id)
    }

    /// Record fast-path hit/miss
    pub fn record_fast_path_result(&mut self, chunk_id: u64, block_index: usize, hit: bool) {
        if let Some(blocks) = self.compiled_blocks.get_mut(&chunk_id) {
            if let Some(block) = blocks.get_mut(block_index) {
                if hit {
                    block.hit_count += 1;
                    self.stats.fast_path_hits += 1;
                } else {
                    block.miss_count += 1;
                    self.stats.fast_path_misses += 1;
                }

                // Invalidate blocks with too many misses
                if block.should_invalidate() {
                    blocks.remove(block_index);
                }
            }
        }
    }

    /// Get profiling statistics
    pub fn stats(&self) -> ProfilerStats {
        let mut stats = self.stats.clone();
        stats.total_profiles = self.profiles.len();
        stats.stable_profiles = self.profiles.values().filter(|p| p.is_stable).count();
        stats.mixed_profiles = self.profiles.values().filter(|p| !p.is_stable).count();
        stats.functions_profiled = self.counters.len();
        stats.hot_functions = self.counters.values().filter(|c| c.jit_candidate).count();
        stats
    }

    /// Enable/disable profiling
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Get the compilation tier for a function
    pub fn get_tier(&self, chunk_id: u64) -> CompilationTier {
        self.counters
            .get(&chunk_id)
            .map(|c| c.tier)
            .unwrap_or(CompilationTier::Interpreter)
    }

    /// Get type profile for a specific instruction
    pub fn get_profile(&self, chunk_id: u64, offset: usize) -> Option<&TypeProfile> {
        self.profiles.get(&(chunk_id, offset))
    }

    /// Reset all profiles (used after major code changes)
    pub fn reset(&mut self) {
        self.profiles.clear();
        self.compiled_blocks.clear();
        for counter in self.counters.values_mut() {
            *counter = ExecutionCounter::new();
        }
        self.stats = ProfilerStats::default();
    }

    /// Get a compilation summary for diagnostics
    pub fn compilation_summary(&self) -> CompilationSummary {
        let stats = self.stats();
        let total_blocks: usize = self.compiled_blocks.values().map(|v| v.len()).sum();
        let total_hits: u64 = self.compiled_blocks.values()
            .flat_map(|v| v.iter())
            .map(|b| b.hit_count)
            .sum();
        let total_misses: u64 = self.compiled_blocks.values()
            .flat_map(|v| v.iter())
            .map(|b| b.miss_count)
            .sum();

        CompilationSummary {
            functions_total: stats.functions_profiled,
            functions_hot: stats.hot_functions,
            functions_baseline: self.counters.values().filter(|c| c.tier == CompilationTier::Baseline).count(),
            functions_optimized: self.counters.values().filter(|c| c.tier == CompilationTier::Optimized).count(),
            compiled_blocks: total_blocks,
            stable_profiles: stats.stable_profiles,
            mixed_profiles: stats.mixed_profiles,
            fast_path_hit_rate: if total_hits + total_misses > 0 {
                total_hits as f64 / (total_hits + total_misses) as f64 * 100.0
            } else {
                0.0
            },
            total_deopts: self.counters.values().map(|c| c.deopt_count as u64).sum(),
        }
    }
}

/// Summary of JIT compilation state for diagnostics
#[derive(Debug, Clone)]
pub struct CompilationSummary {
    pub functions_total: usize,
    pub functions_hot: usize,
    pub functions_baseline: usize,
    pub functions_optimized: usize,
    pub compiled_blocks: usize,
    pub stable_profiles: usize,
    pub mixed_profiles: usize,
    pub fast_path_hit_rate: f64,
    pub total_deopts: u64,
}

impl std::fmt::Display for CompilationSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== JIT Compilation Summary ===")?;
        writeln!(f, "Functions: {} total, {} hot, {} baseline, {} optimized",
            self.functions_total, self.functions_hot, self.functions_baseline, self.functions_optimized)?;
        writeln!(f, "Compiled blocks: {}", self.compiled_blocks)?;
        writeln!(f, "Type profiles: {} stable, {} mixed", self.stable_profiles, self.mixed_profiles)?;
        writeln!(f, "Fast-path hit rate: {:.1}%", self.fast_path_hit_rate)?;
        writeln!(f, "Deoptimizations: {}", self.total_deopts)?;
        Ok(())
    }
}

// ==================== Inline Cache ====================

/// Inline cache entry for property access
#[derive(Debug, Clone)]
pub struct InlineCacheEntry {
    /// Cached shape/hidden class ID of the object
    pub shape_id: u64,
    /// Offset into the object's property storage
    pub offset: u32,
    /// Number of hits
    pub hits: u64,
    /// Number of misses
    pub misses: u64,
}

impl InlineCacheEntry {
    pub fn new(shape_id: u64, offset: u32) -> Self {
        Self { shape_id, offset, hits: 0, misses: 0 }
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 * 100.0 }
    }
}

/// Inline cache state for a single access site
#[derive(Debug, Clone)]
pub enum InlineCacheState {
    /// No cached information yet
    Uninitialized,
    /// Single shape observed (monomorphic — fast path)
    Monomorphic(InlineCacheEntry),
    /// 2-4 shapes observed (polymorphic — PIC)
    Polymorphic(Vec<InlineCacheEntry>),
    /// Too many shapes observed (megamorphic — use hash lookup)
    Megamorphic,
}

const MAX_POLYMORPHIC_ENTRIES: usize = 4;

impl InlineCacheState {
    pub fn new() -> Self {
        Self::Uninitialized
    }

    /// Lookup a shape in the cache, returning the property offset if cached
    pub fn lookup(&mut self, shape_id: u64) -> Option<u32> {
        match self {
            Self::Uninitialized => None,
            Self::Monomorphic(entry) => {
                if entry.shape_id == shape_id {
                    entry.hits += 1;
                    Some(entry.offset)
                } else {
                    entry.misses += 1;
                    None
                }
            }
            Self::Polymorphic(entries) => {
                for entry in entries.iter_mut() {
                    if entry.shape_id == shape_id {
                        entry.hits += 1;
                        return Some(entry.offset);
                    }
                }
                if let Some(first) = entries.first_mut() {
                    first.misses += 1;
                }
                None
            }
            Self::Megamorphic => None,
        }
    }

    /// Update the cache with a new shape→offset mapping
    pub fn update(&mut self, shape_id: u64, offset: u32) {
        match self {
            Self::Uninitialized => {
                *self = Self::Monomorphic(InlineCacheEntry::new(shape_id, offset));
            }
            Self::Monomorphic(entry) => {
                if entry.shape_id != shape_id {
                    let old = entry.clone();
                    *self = Self::Polymorphic(vec![
                        old,
                        InlineCacheEntry::new(shape_id, offset),
                    ]);
                }
            }
            Self::Polymorphic(entries) => {
                if !entries.iter().any(|e| e.shape_id == shape_id) {
                    if entries.len() >= MAX_POLYMORPHIC_ENTRIES {
                        *self = Self::Megamorphic;
                    } else {
                        entries.push(InlineCacheEntry::new(shape_id, offset));
                    }
                }
            }
            Self::Megamorphic => {} // Already megamorphic
        }
    }

    pub fn is_monomorphic(&self) -> bool {
        matches!(self, Self::Monomorphic(_))
    }

    pub fn is_megamorphic(&self) -> bool {
        matches!(self, Self::Megamorphic)
    }
}

impl Default for InlineCacheState {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for TypeProfiler {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Optimization Hints ====================

/// Hints from the profiler to guide compilation decisions
#[derive(Debug, Clone)]
pub enum OptimizationHint {
    /// Function is monomorphic — can specialize for these types
    Monomorphic { chunk_id: u64, param_types: Vec<ObservedType> },
    /// Loop is hot — consider unrolling or specialized iteration
    HotLoop { chunk_id: u64, loop_ip: usize, iteration_count: u64 },
    /// Property access is stable — can use inline cache
    StablePropertyAccess { chunk_id: u64, ip: usize, shape_id: u64 },
    /// Function is pure (no side effects) — can cache results
    PureFunction { chunk_id: u64 },
    /// Function should be deoptimized back to interpreter
    Deoptimize { chunk_id: u64, reason: String },
}

/// Collects optimization hints from profiler state
pub fn collect_optimization_hints(profiler: &TypeProfiler) -> Vec<OptimizationHint> {
    let mut hints = Vec::new();
    let summary = profiler.compilation_summary();

    // Suggest deopt for functions with high miss rates
    if summary.fast_path_hit_rate < 50.0 && summary.compiled_blocks > 0 {
        hints.push(OptimizationHint::Deoptimize {
            chunk_id: 0,
            reason: format!("Fast-path hit rate too low: {:.1}%", summary.fast_path_hit_rate),
        });
    }

    hints
}

// ==================== Baseline Compiler ====================

/// IR instruction for the baseline compiler. These represent type-specialized
/// operations that can be executed more efficiently than generic bytecode.
#[derive(Debug, Clone)]
pub enum IrInstruction {
    /// Load a 64-bit float constant
    LoadFloat(f64),
    /// Load a 32-bit integer constant
    LoadInt(i32),
    /// Load a string constant
    LoadString(String),
    /// Load a local variable by slot index
    LoadLocal(u16),
    /// Store to a local variable by slot index
    StoreLocal(u16),
    /// Integer add (both operands guaranteed Int32)
    IAdd,
    /// Float add (both operands guaranteed Float64)
    FAdd,
    /// Integer subtract
    ISub,
    /// Float subtract
    FSub,
    /// Integer multiply
    IMul,
    /// Float multiply
    FMul,
    /// Integer divide (with zero-check guard)
    IDiv,
    /// Float divide
    FDiv,
    /// Integer comparison (less than)
    ILessThan,
    /// Float comparison (less than)
    FLessThan,
    /// Integer equality
    IEqual,
    /// String concatenation
    StringConcat,
    /// Type guard: check that top-of-stack matches expected type, deopt if not
    Guard(ObservedType),
    /// Unconditional jump to IR offset
    Jump(usize),
    /// Conditional jump (pop boolean, jump if false)
    JumpIfFalse(usize),
    /// Return the top-of-stack value
    Return,
    /// Deoptimize: fall back to interpreter at the given bytecode IP
    Deoptimize(usize),
    /// Increment an integer local in-place
    IncrementLocal(u16),
    /// Decrement an integer local in-place
    DecrementLocal(u16),
    /// No-op (placeholder)
    Nop,
}

/// A compiled IR function produced by the baseline compiler.
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    /// The original bytecode chunk ID this was compiled from
    pub chunk_id: u64,
    /// IR instruction stream
    pub instructions: Vec<IrInstruction>,
    /// Number of local slots needed
    pub local_count: u16,
    /// Type assumptions for parameters (guards inserted at entry)
    pub param_guards: Vec<ObservedType>,
    /// Bytecode IP to resume at if deoptimization occurs
    pub deopt_ip: usize,
    /// Compilation tier
    pub tier: CompilationTier,
}

/// The baseline compiler translates profiled bytecode into type-specialized IR.
pub struct BaselineCompiler {
    /// Compiled functions keyed by chunk_id
    compiled: HashMap<u64, CompiledFunction>,
}

impl BaselineCompiler {
    pub fn new() -> Self {
        Self {
            compiled: HashMap::default(),
        }
    }

    /// Compile a function's fast path based on type profile data.
    /// Returns None if the function can't be profitably compiled.
    pub fn compile(
        &mut self,
        chunk_id: u64,
        chunk: &Chunk,
        profiler: &TypeProfiler,
    ) -> Option<&CompiledFunction> {
        if profiler.get_tier(chunk_id) < CompilationTier::Baseline {
            return None;
        }

        let blocks = profiler.get_compiled_blocks(chunk_id)?;
        let mut instructions = Vec::new();

        // Emit entry guards for parameter types
        let mut param_guards = Vec::new();
        if let Some(first_block) = blocks.first() {
            for guard in &first_block.guards {
                param_guards.push(guard.expected_type);
                instructions.push(IrInstruction::LoadLocal(guard.stack_index as u16));
                instructions.push(IrInstruction::Guard(guard.expected_type));
            }
        }

        // Translate each compiled block's specialized ops to IR
        for block in blocks {
            for op in &block.ops {
                match op {
                    SpecializedOp::IntAdd => instructions.push(IrInstruction::IAdd),
                    SpecializedOp::FloatAdd => instructions.push(IrInstruction::FAdd),
                    SpecializedOp::IntSub => instructions.push(IrInstruction::ISub),
                    SpecializedOp::FloatSub => instructions.push(IrInstruction::FSub),
                    SpecializedOp::IntMul => instructions.push(IrInstruction::IMul),
                    SpecializedOp::FloatMul => instructions.push(IrInstruction::FMul),
                    SpecializedOp::IntLt => instructions.push(IrInstruction::ILessThan),
                    SpecializedOp::IntEq => instructions.push(IrInstruction::IEqual),
                    SpecializedOp::StringConcat => instructions.push(IrInstruction::StringConcat),
                    SpecializedOp::StringEq => instructions.push(IrInstruction::IEqual),
                    SpecializedOp::IntIncrement => {
                        instructions.push(IrInstruction::LoadInt(1));
                        instructions.push(IrInstruction::IAdd);
                    }
                    SpecializedOp::IntDecrement => {
                        instructions.push(IrInstruction::LoadInt(1));
                        instructions.push(IrInstruction::ISub);
                    }
                    _ => {
                        // Non-specializable ops trigger deopt
                        instructions.push(IrInstruction::Deoptimize(block.end_offset));
                    }
                }
            }
        }

        instructions.push(IrInstruction::Return);

        let local_count = chunk.code.first().copied().unwrap_or(0) as u16;
        let compiled = CompiledFunction {
            chunk_id,
            instructions,
            local_count,
            param_guards,
            deopt_ip: 0,
            tier: CompilationTier::Baseline,
        };

        self.compiled.insert(chunk_id, compiled);
        self.compiled.get(&chunk_id)
    }

    /// Get a previously compiled function
    pub fn get_compiled(&self, chunk_id: u64) -> Option<&CompiledFunction> {
        self.compiled.get(&chunk_id)
    }

    /// Invalidate a compiled function (after deopt threshold exceeded)
    pub fn invalidate(&mut self, chunk_id: u64) {
        self.compiled.remove(&chunk_id);
    }

    /// Count of compiled functions
    pub fn compiled_count(&self) -> usize {
        self.compiled.len()
    }
}

impl Default for BaselineCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a compiled IR function on a simple register-like stack machine.
/// Returns the result value or None if deoptimization was triggered.
pub fn execute_ir(compiled: &CompiledFunction, args: &[f64]) -> Option<f64> {
    let mut stack: Vec<f64> = Vec::with_capacity(16);
    let mut locals: Vec<f64> = vec![0.0; compiled.local_count as usize];

    // Initialize locals from arguments
    for (i, &arg) in args.iter().enumerate() {
        if i < locals.len() {
            locals[i] = arg;
        }
    }

    let mut ip = 0;
    let instructions = &compiled.instructions;
    let max_iterations = 100_000;
    let mut iterations = 0;

    while ip < instructions.len() && iterations < max_iterations {
        iterations += 1;
        match &instructions[ip] {
            IrInstruction::LoadFloat(f) => stack.push(*f),
            IrInstruction::LoadInt(i) => stack.push(*i as f64),
            IrInstruction::LoadLocal(slot) => {
                stack.push(locals.get(*slot as usize).copied().unwrap_or(0.0));
            }
            IrInstruction::StoreLocal(slot) => {
                let val = stack.pop().unwrap_or(0.0);
                if (*slot as usize) < locals.len() {
                    locals[*slot as usize] = val;
                }
            }
            IrInstruction::IAdd | IrInstruction::FAdd => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                stack.push(a + b);
            }
            IrInstruction::ISub | IrInstruction::FSub => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                stack.push(a - b);
            }
            IrInstruction::IMul | IrInstruction::FMul => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                stack.push(a * b);
            }
            IrInstruction::IDiv | IrInstruction::FDiv => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                if b == 0.0 {
                    return None; // Deopt on division by zero
                }
                stack.push(a / b);
            }
            IrInstruction::ILessThan | IrInstruction::FLessThan => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                stack.push(if a < b { 1.0 } else { 0.0 });
            }
            IrInstruction::IEqual => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                stack.push(if (a - b).abs() < f64::EPSILON { 1.0 } else { 0.0 });
            }
            IrInstruction::Jump(target) => {
                ip = *target;
                continue;
            }
            IrInstruction::JumpIfFalse(target) => {
                let cond = stack.pop().unwrap_or(0.0);
                if cond == 0.0 {
                    ip = *target;
                    continue;
                }
            }
            IrInstruction::IncrementLocal(slot) => {
                if (*slot as usize) < locals.len() {
                    locals[*slot as usize] += 1.0;
                }
            }
            IrInstruction::DecrementLocal(slot) => {
                if (*slot as usize) < locals.len() {
                    locals[*slot as usize] -= 1.0;
                }
            }
            IrInstruction::Return => {
                return stack.pop().or(Some(0.0));
            }
            IrInstruction::Deoptimize(_) => return None,
            IrInstruction::Guard(_) | IrInstruction::Nop | IrInstruction::StringConcat
            | IrInstruction::LoadString(_) => {}
        }
        ip += 1;
    }

    stack.pop().or(Some(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observed_type_merge() {
        assert_eq!(ObservedType::Int32.merge(ObservedType::Int32), ObservedType::Int32);
        assert_eq!(ObservedType::Int32.merge(ObservedType::Float64), ObservedType::Mixed);
        assert_eq!(ObservedType::String.merge(ObservedType::String), ObservedType::String);
    }

    #[test]
    fn test_type_profile_stable() {
        let mut profile = TypeProfile::new();
        for _ in 0..100 {
            profile.record(&[ObservedType::Int32, ObservedType::Int32], ObservedType::Int32);
        }
        assert!(profile.is_stable);
        assert_eq!(profile.sample_count, 100);
        assert_eq!(profile.operand_types, vec![ObservedType::Int32, ObservedType::Int32]);
    }

    #[test]
    fn test_type_profile_mixed() {
        let mut profile = TypeProfile::new();
        profile.record(&[ObservedType::Int32, ObservedType::Int32], ObservedType::Int32);
        profile.record(&[ObservedType::String, ObservedType::String], ObservedType::String);
        assert!(!profile.is_stable);
    }

    #[test]
    fn test_execution_counter() {
        let mut counter = ExecutionCounter::new();
        for _ in 0..1001 {
            counter.record_invocation(Duration::from_micros(10), 100);
        }
        assert!(counter.jit_candidate);
        assert_eq!(counter.tier, CompilationTier::Interpreter);
    }

    #[test]
    fn test_profiler_hot_detection() {
        let mut profiler = TypeProfiler::new();
        let chunk = Chunk::new();
        let chunk_id = profiler.register_chunk(&chunk);

        for _ in 0..1001 {
            profiler.record_invocation(chunk_id, Duration::from_micros(10), 100);
        }

        let hot = profiler.hot_functions();
        assert!(hot.contains(&chunk_id));
    }

    #[test]
    fn test_fast_path_compilation() {
        let mut profiler = TypeProfiler::new();
        let chunk = Chunk::new();
        let chunk_id = profiler.register_chunk(&chunk);

        // Record many stable observations at the same offsets (need >10 per offset)
        for offset in 0..5 {
            for _ in 0..15 {
                profiler.record_types(
                    chunk_id,
                    offset,
                    &[ObservedType::Int32, ObservedType::Int32],
                    ObservedType::Int32,
                );
            }
        }

        // Make the function hot
        for _ in 0..1001 {
            profiler.record_invocation(chunk_id, Duration::from_micros(10), 100);
        }

        let blocks = profiler.compile_fast_paths(chunk_id);
        assert!(blocks.is_some());
        assert_eq!(profiler.get_tier(chunk_id), CompilationTier::Baseline);
    }

    #[test]
    fn test_deopt_handling() {
        let mut profiler = TypeProfiler::new();
        let chunk = Chunk::new();
        let chunk_id = profiler.register_chunk(&chunk);

        for _ in 0..5 {
            profiler.record_deopt(chunk_id);
        }

        assert_eq!(profiler.get_tier(chunk_id), CompilationTier::Interpreter);
    }

    #[test]
    fn test_compiled_block_invalidation() {
        let block = CompiledBlock {
            start_offset: 0,
            end_offset: 10,
            ops: vec![SpecializedOp::IntAdd],
            guards: vec![],
            hit_count: 50,
            miss_count: 60, // > 20% miss rate
        };
        assert!(block.should_invalidate());
    }

    #[test]
    fn test_profiler_stats() {
        let mut profiler = TypeProfiler::new();
        let chunk = Chunk::new();
        let chunk_id = profiler.register_chunk(&chunk);

        profiler.record_types(chunk_id, 0, &[ObservedType::Int32], ObservedType::Int32);
        let stats = profiler.stats();
        assert_eq!(stats.total_profiles, 1);
        assert_eq!(stats.functions_profiled, 1);
    }

    #[test]
    fn test_inline_cache_monomorphic() {
        let mut ic = InlineCacheState::new();
        assert!(matches!(ic, InlineCacheState::Uninitialized));

        ic.update(42, 0);
        assert!(ic.is_monomorphic());

        assert_eq!(ic.lookup(42), Some(0));
        assert_eq!(ic.lookup(99), None);
    }

    #[test]
    fn test_inline_cache_polymorphic() {
        let mut ic = InlineCacheState::new();
        ic.update(1, 0);
        ic.update(2, 1);
        assert!(matches!(ic, InlineCacheState::Polymorphic(_)));

        assert_eq!(ic.lookup(1), Some(0));
        assert_eq!(ic.lookup(2), Some(1));
    }

    #[test]
    fn test_inline_cache_megamorphic() {
        let mut ic = InlineCacheState::new();
        for i in 0..5 {
            ic.update(i, i as u32);
        }
        assert!(ic.is_megamorphic());
        assert_eq!(ic.lookup(1), None); // Megamorphic always misses
    }

    #[test]
    fn test_compilation_summary() {
        let mut profiler = TypeProfiler::new();
        let chunk = Chunk::new();
        let chunk_id = profiler.register_chunk(&chunk);

        for offset in 0..3 {
            for _ in 0..20 {
                profiler.record_types(chunk_id, offset, &[ObservedType::Int32, ObservedType::Int32], ObservedType::Int32);
            }
        }
        for _ in 0..1001 {
            profiler.record_invocation(chunk_id, Duration::from_micros(1), 10);
        }
        profiler.compile_fast_paths(chunk_id);

        let summary = profiler.compilation_summary();
        assert_eq!(summary.functions_total, 1);
        assert!(summary.functions_baseline > 0 || summary.functions_hot > 0);
        assert!(summary.compiled_blocks > 0);
        let formatted = format!("{}", summary);
        assert!(formatted.contains("JIT Compilation Summary"));
    }

    #[test]
    fn test_baseline_compiler_new() {
        let compiler = BaselineCompiler::new();
        assert_eq!(compiler.compiled_count(), 0);
    }

    #[test]
    fn test_execute_ir_simple_add() {
        let compiled = CompiledFunction {
            chunk_id: 0,
            instructions: vec![
                IrInstruction::LoadLocal(0),
                IrInstruction::LoadLocal(1),
                IrInstruction::IAdd,
                IrInstruction::Return,
            ],
            local_count: 2,
            param_guards: vec![],
            deopt_ip: 0,
            tier: CompilationTier::Baseline,
        };
        let result = execute_ir(&compiled, &[10.0, 32.0]);
        assert_eq!(result, Some(42.0));
    }

    #[test]
    fn test_execute_ir_arithmetic_chain() {
        let compiled = CompiledFunction {
            chunk_id: 0,
            instructions: vec![
                IrInstruction::LoadLocal(0),
                IrInstruction::LoadLocal(1),
                IrInstruction::IMul,
                IrInstruction::LoadInt(1),
                IrInstruction::IAdd,
                IrInstruction::Return,
            ],
            local_count: 2,
            param_guards: vec![],
            deopt_ip: 0,
            tier: CompilationTier::Baseline,
        };
        // (5 * 8) + 1 = 41
        let result = execute_ir(&compiled, &[5.0, 8.0]);
        assert_eq!(result, Some(41.0));
    }

    #[test]
    fn test_execute_ir_division_by_zero_deopts() {
        let compiled = CompiledFunction {
            chunk_id: 0,
            instructions: vec![
                IrInstruction::LoadLocal(0),
                IrInstruction::LoadLocal(1),
                IrInstruction::IDiv,
                IrInstruction::Return,
            ],
            local_count: 2,
            param_guards: vec![],
            deopt_ip: 0,
            tier: CompilationTier::Baseline,
        };
        let result = execute_ir(&compiled, &[10.0, 0.0]);
        assert_eq!(result, None); // Deopt on div by zero
    }

    #[test]
    fn test_execute_ir_comparison() {
        let compiled = CompiledFunction {
            chunk_id: 0,
            instructions: vec![
                IrInstruction::LoadLocal(0),
                IrInstruction::LoadLocal(1),
                IrInstruction::ILessThan,
                IrInstruction::Return,
            ],
            local_count: 2,
            param_guards: vec![],
            deopt_ip: 0,
            tier: CompilationTier::Baseline,
        };
        assert_eq!(execute_ir(&compiled, &[3.0, 5.0]), Some(1.0)); // 3 < 5 = true
        assert_eq!(execute_ir(&compiled, &[5.0, 3.0]), Some(0.0)); // 5 < 3 = false
    }

    #[test]
    fn test_execute_ir_increment_local() {
        let compiled = CompiledFunction {
            chunk_id: 0,
            instructions: vec![
                IrInstruction::IncrementLocal(0),
                IrInstruction::IncrementLocal(0),
                IrInstruction::IncrementLocal(0),
                IrInstruction::LoadLocal(0),
                IrInstruction::Return,
            ],
            local_count: 1,
            param_guards: vec![],
            deopt_ip: 0,
            tier: CompilationTier::Baseline,
        };
        assert_eq!(execute_ir(&compiled, &[0.0]), Some(3.0));
    }

    #[test]
    fn test_baseline_compiler_invalidate() {
        let mut compiler = BaselineCompiler::new();
        compiler.compiled.insert(42, CompiledFunction {
            chunk_id: 42,
            instructions: vec![IrInstruction::Return],
            local_count: 0,
            param_guards: vec![],
            deopt_ip: 0,
            tier: CompilationTier::Baseline,
        });
        assert_eq!(compiler.compiled_count(), 1);
        compiler.invalidate(42);
        assert_eq!(compiler.compiled_count(), 0);
    }
}
