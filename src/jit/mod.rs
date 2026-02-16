//! JIT Compilation Tier with Type Profiling
//!
//! Provides type profiling infrastructure, hot function detection, and
//! specialized fast-path compilation for frequently executed bytecode.
//! This serves as the foundation for a baseline JIT compiler.

//! **Status:** 🧪 Experimental — Basic JIT compilation framework — not production-ready

pub mod codegen;

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

// ==================== Type Feedback System ====================

/// Type feedback collected during interpretation for optimization
#[derive(Debug, Clone)]
pub struct TypeFeedback {
    /// Type profiles per bytecode offset
    profiles: HashMap<usize, TypeFeedbackProfile>,
    /// Total observations
    total_observations: u64,
}

/// Type profile for a single bytecode site within the TypeFeedback system.
/// Tracks observed types and classifies the site as monomorphic, polymorphic,
/// or megamorphic based on the number of distinct types seen.
#[derive(Debug, Clone)]
pub struct TypeFeedbackProfile {
    /// Observed types at this location
    pub observed_types: Vec<ObservedType>,
    /// Number of times this site was executed
    pub execution_count: u64,
    /// Whether the site is monomorphic (single type)
    pub is_monomorphic: bool,
    /// Whether the site is polymorphic (2-4 types)
    pub is_polymorphic: bool,
    /// Whether the site is megamorphic (>4 types, give up specialization)
    pub is_megamorphic: bool,
}

impl TypeFeedback {
    pub fn new() -> Self {
        Self {
            profiles: HashMap::default(),
            total_observations: 0,
        }
    }

    /// Record a type observation at a bytecode offset
    pub fn record(&mut self, offset: usize, ty: ObservedType) {
        self.total_observations += 1;
        let profile = self.profiles.entry(offset).or_insert_with(|| TypeFeedbackProfile {
            observed_types: Vec::new(),
            execution_count: 0,
            is_monomorphic: false,
            is_polymorphic: false,
            is_megamorphic: false,
        });
        profile.execution_count += 1;
        if !profile.observed_types.contains(&ty) {
            profile.observed_types.push(ty);
        }
        let count = profile.observed_types.len();
        profile.is_monomorphic = count == 1;
        profile.is_polymorphic = (2..=4).contains(&count);
        profile.is_megamorphic = count > 4;
    }

    /// Get profile at a specific offset
    pub fn profile_at(&self, offset: usize) -> Option<&TypeFeedbackProfile> {
        self.profiles.get(&offset)
    }

    /// List all monomorphic offsets
    pub fn monomorphic_sites(&self) -> Vec<usize> {
        self.profiles
            .iter()
            .filter(|(_, p)| p.is_monomorphic)
            .map(|(offset, _)| *offset)
            .collect()
    }

    /// List all polymorphic offsets
    pub fn polymorphic_sites(&self) -> Vec<usize> {
        self.profiles
            .iter()
            .filter(|(_, p)| p.is_polymorphic)
            .map(|(offset, _)| *offset)
            .collect()
    }

    /// List all megamorphic offsets
    pub fn megamorphic_sites(&self) -> Vec<usize> {
        self.profiles
            .iter()
            .filter(|(_, p)| p.is_megamorphic)
            .map(|(offset, _)| *offset)
            .collect()
    }

    /// Reset all feedback
    pub fn reset(&mut self) {
        self.profiles.clear();
        self.total_observations = 0;
    }
}

impl Default for TypeFeedback {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Inline Cache (Keyed) System ====================

/// Inline cache for fast property access, keyed by shape hash
#[derive(Debug, Clone)]
pub struct InlineCache {
    entries: Vec<ICEntry>,
    max_entries: usize,
    hits: u64,
    misses: u64,
}

/// A single inline cache entry
#[derive(Debug, Clone)]
pub struct ICEntry {
    /// The shape/hidden class key (property name + object structure hash)
    pub shape_key: u64,
    /// Cached property offset
    pub offset: usize,
    /// Whether this is a direct property or prototype chain lookup
    pub is_own: bool,
    /// Hit count for this entry
    pub hit_count: u64,
}

/// Inline cache statistics
#[derive(Debug, Clone)]
pub struct ICStats {
    pub total_lookups: u64,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
    pub entry_count: usize,
}

impl InlineCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
            hits: 0,
            misses: 0,
        }
    }

    /// Lookup a shape key, returns cached offset if found
    pub fn lookup(&mut self, shape_key: u64) -> Option<usize> {
        for entry in &mut self.entries {
            if entry.shape_key == shape_key {
                entry.hit_count += 1;
                self.hits += 1;
                return Some(entry.offset);
            }
        }
        self.misses += 1;
        None
    }

    /// Insert a new cache entry, evicting least-used if at capacity
    pub fn insert(&mut self, shape_key: u64, offset: usize, is_own: bool) {
        for entry in &mut self.entries {
            if entry.shape_key == shape_key {
                entry.offset = offset;
                entry.is_own = is_own;
                return;
            }
        }
        if self.entries.len() >= self.max_entries {
            if let Some(min_idx) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.hit_count)
                .map(|(i, _)| i)
            {
                self.entries.remove(min_idx);
            }
        }
        self.entries.push(ICEntry {
            shape_key,
            offset,
            is_own,
            hit_count: 0,
        });
    }

    /// Invalidate (remove) an entry by shape key
    pub fn invalidate(&mut self, shape_key: u64) {
        self.entries.retain(|e| e.shape_key != shape_key);
    }

    /// Clear all entries and reset counters
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// Get the hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> ICStats {
        ICStats {
            total_lookups: self.hits + self.misses,
            hits: self.hits,
            misses: self.misses,
            hit_rate: self.hit_rate(),
            entry_count: self.entries.len(),
        }
    }
}

// ==================== Tiered Compilation Manager ====================

/// Tiered compilation manager
pub struct TieredCompilation {
    /// Execution counts per function
    execution_counts: HashMap<String, u64>,
    /// Compilation tier per function
    compilation_tiers: HashMap<String, CompilationTier>,
    /// Thresholds for tier promotion
    tier1_threshold: u64,
    tier2_threshold: u64,
    /// Functions marked for compilation
    compilation_queue: Vec<CompilationRequest>,
}

/// A request to compile a function to a higher tier
#[derive(Debug, Clone)]
pub struct CompilationRequest {
    pub function_name: String,
    pub target_tier: CompilationTier,
    pub type_feedback: Option<TypeFeedback>,
    pub priority: u32,
}

/// Statistics for tiered compilation
#[derive(Debug, Clone)]
pub struct TieredStats {
    pub interpreter_count: usize,
    pub baseline_count: usize,
    pub optimized_count: usize,
    pub total_compilations: u64,
    pub pending_compilations: usize,
}

impl TieredCompilation {
    pub fn new(tier1: u64, tier2: u64) -> Self {
        Self {
            execution_counts: HashMap::default(),
            compilation_tiers: HashMap::default(),
            tier1_threshold: tier1,
            tier2_threshold: tier2,
            compilation_queue: Vec::new(),
        }
    }

    /// Record a function execution and maybe queue for compilation
    pub fn record_execution(&mut self, func: &str) {
        let count = self.execution_counts.entry(func.to_string()).or_insert(0);
        *count += 1;
        let count_val = *count;
        let current = self.current_tier(func);
        let already_queued = self.compilation_queue.iter().any(|r| r.function_name == func);
        if already_queued {
            return;
        }
        match current {
            CompilationTier::Interpreter if count_val >= self.tier1_threshold => {
                self.compilation_queue.push(CompilationRequest {
                    function_name: func.to_string(),
                    target_tier: CompilationTier::Baseline,
                    type_feedback: None,
                    priority: 1,
                });
            }
            CompilationTier::Baseline if count_val >= self.tier2_threshold => {
                self.compilation_queue.push(CompilationRequest {
                    function_name: func.to_string(),
                    target_tier: CompilationTier::Optimized,
                    type_feedback: None,
                    priority: 2,
                });
            }
            _ => {}
        }
    }

    /// Get the current compilation tier for a function
    pub fn current_tier(&self, func: &str) -> CompilationTier {
        self.compilation_tiers
            .get(func)
            .copied()
            .unwrap_or(CompilationTier::Interpreter)
    }

    /// Promote a function to a higher tier
    pub fn promote(&mut self, func: &str, tier: CompilationTier) {
        self.compilation_tiers.insert(func.to_string(), tier);
    }

    /// Get pending compilation requests
    pub fn pending_compilations(&self) -> &[CompilationRequest] {
        &self.compilation_queue
    }

    /// Drain the compilation queue, returning all pending requests
    pub fn drain_compilation_queue(&mut self) -> Vec<CompilationRequest> {
        std::mem::take(&mut self.compilation_queue)
    }

    /// Get compilation statistics
    pub fn stats(&self) -> TieredStats {
        let mut interpreter_count = 0;
        let mut baseline_count = 0;
        let mut optimized_count = 0;
        for tier in self.compilation_tiers.values() {
            match tier {
                CompilationTier::Interpreter => interpreter_count += 1,
                CompilationTier::Baseline => baseline_count += 1,
                CompilationTier::Optimized => optimized_count += 1,
            }
        }
        TieredStats {
            interpreter_count,
            baseline_count,
            optimized_count,
            total_compilations: (baseline_count + optimized_count) as u64,
            pending_compilations: self.compilation_queue.len(),
        }
    }
}

// ==================== Deoptimization Support ====================

/// Deoptimization tracking for JIT bailouts
pub struct DeoptimizationTracker {
    /// Deoptimization events
    events: Vec<DeoptEvent>,
    /// Functions that have been deoptimized (name → count)
    deoptimized: HashMap<String, u32>,
    /// Max deoptimizations before giving up on JIT for a function
    max_deopts: u32,
}

/// A deoptimization event
#[derive(Debug, Clone)]
pub struct DeoptEvent {
    pub function_name: String,
    pub reason: DeoptReason,
    pub bytecode_offset: usize,
    pub timestamp: u64,
}

/// Reasons for deoptimization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeoptReason {
    TypeMismatch,
    HiddenClassChange,
    OverflowCheck,
    BoundCheck,
    UnexpectedValue,
    StackOverflow,
    Debugger,
}

impl DeoptimizationTracker {
    pub fn new(max_deopts: u32) -> Self {
        Self {
            events: Vec::new(),
            deoptimized: HashMap::default(),
            max_deopts,
        }
    }

    /// Record a deoptimization event
    pub fn record_deopt(&mut self, event: DeoptEvent) {
        let count = self.deoptimized.entry(event.function_name.clone()).or_insert(0);
        *count += 1;
        self.events.push(event);
    }

    /// Check if a function should still be JIT compiled (false if too many deopts)
    pub fn should_jit(&self, func: &str) -> bool {
        self.deopt_count(func) < self.max_deopts
    }

    /// Get the deopt count for a function
    pub fn deopt_count(&self, func: &str) -> u32 {
        self.deoptimized.get(func).copied().unwrap_or(0)
    }

    /// Get all deopt events
    pub fn events(&self) -> &[DeoptEvent] {
        &self.events
    }

    /// Reset all tracking
    pub fn reset(&mut self) {
        self.events.clear();
        self.deoptimized.clear();
    }
}

// ==================== SSA Intermediate Representation ====================

/// Virtual register in SSA form
pub type SsaReg = u32;

/// Basic block identifier in SSA form
pub type SsaBlockId = u32;

/// SSA IR value types for constants
#[derive(Debug, Clone)]
pub enum SsaValue {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
}

/// SSA IR type for type guards
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsaType {
    Integer,
    Float,
    String,
    Boolean,
    Object,
    Array,
    Function,
    Any,
}

/// JIT IR instruction in SSA (Static Single Assignment) form
#[derive(Debug, Clone)]
pub enum SsaInstr {
    // Constants
    LoadConst(SsaReg, SsaValue),
    LoadUndefined(SsaReg),
    LoadNull(SsaReg),

    // Arithmetic (dst, lhs, rhs)
    Add(SsaReg, SsaReg, SsaReg),
    Sub(SsaReg, SsaReg, SsaReg),
    Mul(SsaReg, SsaReg, SsaReg),
    Div(SsaReg, SsaReg, SsaReg),
    Mod(SsaReg, SsaReg, SsaReg),
    Neg(SsaReg, SsaReg),

    // Integer-specialized (when type feedback indicates integer)
    AddInt(SsaReg, SsaReg, SsaReg),
    SubInt(SsaReg, SsaReg, SsaReg),
    MulInt(SsaReg, SsaReg, SsaReg),

    // Comparison (dst, lhs, rhs)
    Equal(SsaReg, SsaReg, SsaReg),
    StrictEqual(SsaReg, SsaReg, SsaReg),
    LessThan(SsaReg, SsaReg, SsaReg),
    GreaterThan(SsaReg, SsaReg, SsaReg),
    LessEqual(SsaReg, SsaReg, SsaReg),
    GreaterEqual(SsaReg, SsaReg, SsaReg),

    // Logical
    Not(SsaReg, SsaReg),

    // Variables
    LoadLocal(SsaReg, u32),
    StoreLocal(u32, SsaReg),
    LoadGlobal(SsaReg, String),
    StoreGlobal(String, SsaReg),

    // Object operations
    GetProperty(SsaReg, SsaReg, String),
    SetProperty(SsaReg, String, SsaReg),

    // Function calls
    Call(SsaReg, SsaReg, Vec<SsaReg>),
    Return(Option<SsaReg>),

    // Control flow
    Jump(SsaBlockId),
    Branch(SsaReg, SsaBlockId, SsaBlockId),

    // Type guards (for speculative optimization)
    SsaTypeGuard(SsaReg, SsaType, SsaBlockId),

    // Phi function (SSA)
    Phi(SsaReg, Vec<(SsaBlockId, SsaReg)>),
}

/// A basic block in the SSA IR
#[derive(Debug, Clone)]
pub struct SsaBlock {
    pub id: SsaBlockId,
    pub instructions: Vec<SsaInstr>,
    pub predecessors: Vec<SsaBlockId>,
    pub successors: Vec<SsaBlockId>,
}

/// A complete SSA IR function
#[derive(Debug, Clone)]
pub struct SsaFunction {
    pub name: String,
    pub blocks: Vec<SsaBlock>,
    pub num_registers: u32,
    pub num_params: u32,
    pub num_locals: u32,
    pub entry_block: SsaBlockId,
}

// ==================== Bytecode → SSA IR Translator ====================

/// Translates Quicksilver bytecode to SSA IR
pub struct SsaTranslator {
    blocks: Vec<SsaBlock>,
    current_block: SsaBlockId,
    next_reg: u32,
    next_block: u32,
    /// Maps stack positions to registers
    stack_map: Vec<SsaReg>,
}

impl SsaTranslator {
    pub fn new() -> Self {
        let entry = SsaBlock {
            id: 0,
            instructions: Vec::new(),
            predecessors: Vec::new(),
            successors: Vec::new(),
        };
        Self {
            blocks: vec![entry],
            current_block: 0,
            next_reg: 0,
            next_block: 1,
            stack_map: Vec::new(),
        }
    }

    /// Allocate a new virtual register
    pub fn alloc_reg(&mut self) -> SsaReg {
        let reg = self.next_reg;
        self.next_reg += 1;
        reg
    }

    /// Create a new basic block and return its ID
    pub fn new_block(&mut self) -> SsaBlockId {
        let id = self.next_block;
        self.next_block += 1;
        self.blocks.push(SsaBlock {
            id,
            instructions: Vec::new(),
            predecessors: Vec::new(),
            successors: Vec::new(),
        });
        id
    }

    /// Push a register onto the virtual stack
    pub fn push_reg(&mut self, reg: SsaReg) {
        self.stack_map.push(reg);
    }

    /// Pop a register from the virtual stack
    pub fn pop_reg(&mut self) -> SsaReg {
        self.stack_map.pop().unwrap_or(0)
    }

    /// Emit an instruction into the current block
    pub fn emit(&mut self, inst: SsaInstr) {
        if let Some(block) = self.blocks.iter_mut().find(|b| b.id == self.current_block) {
            block.instructions.push(inst);
        }
    }

    /// Read a u16 operand from the bytecode stream at `ip`
    fn read_u16(opcodes: &[u8], ip: usize) -> u16 {
        let hi = *opcodes.get(ip).unwrap_or(&0) as u16;
        let lo = *opcodes.get(ip + 1).unwrap_or(&0) as u16;
        (hi << 8) | lo
    }

    /// Read a signed i16 operand from the bytecode stream at `ip`
    fn read_i16(opcodes: &[u8], ip: usize) -> i16 {
        Self::read_u16(opcodes, ip) as i16
    }

    /// Translate bytecode to SSA IR
    pub fn translate(&mut self, opcodes: &[u8], constants: &[crate::Value]) -> SsaFunction {
        use crate::bytecode::Opcode;

        let mut ip = 0;
        while ip < opcodes.len() {
            let Some(op) = Opcode::from_u8(opcodes[ip]) else {
                ip += 1;
                continue;
            };
            ip += 1;

            match op {
                Opcode::Constant => {
                    let idx = Self::read_u16(opcodes, ip) as usize;
                    ip += 2;
                    let dst = self.alloc_reg();
                    let val = if idx < constants.len() {
                        match &constants[idx] {
                            crate::Value::Number(n) => {
                                if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                                    SsaValue::Integer(*n as i64)
                                } else {
                                    SsaValue::Float(*n)
                                }
                            }
                            crate::Value::String(s) => SsaValue::String(s.clone()),
                            crate::Value::Boolean(b) => SsaValue::Boolean(*b),
                            _ => SsaValue::Float(0.0),
                        }
                    } else {
                        SsaValue::Float(0.0)
                    };
                    self.emit(SsaInstr::LoadConst(dst, val));
                    self.push_reg(dst);
                }
                Opcode::Add => {
                    let rhs = self.pop_reg();
                    let lhs = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::Add(dst, lhs, rhs));
                    self.push_reg(dst);
                }
                Opcode::Sub => {
                    let rhs = self.pop_reg();
                    let lhs = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::Sub(dst, lhs, rhs));
                    self.push_reg(dst);
                }
                Opcode::Mul => {
                    let rhs = self.pop_reg();
                    let lhs = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::Mul(dst, lhs, rhs));
                    self.push_reg(dst);
                }
                Opcode::Div => {
                    let rhs = self.pop_reg();
                    let lhs = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::Div(dst, lhs, rhs));
                    self.push_reg(dst);
                }
                Opcode::GetLocal => {
                    let idx = Self::read_u16(opcodes, ip) as u32;
                    ip += 2;
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::LoadLocal(dst, idx));
                    self.push_reg(dst);
                }
                Opcode::SetLocal => {
                    let idx = Self::read_u16(opcodes, ip) as u32;
                    ip += 2;
                    let src = self.pop_reg();
                    self.emit(SsaInstr::StoreLocal(idx, src));
                }
                Opcode::Jump => {
                    let offset = Self::read_i16(opcodes, ip);
                    ip += 2;
                    let target_block = self.new_block();
                    self.emit(SsaInstr::Jump(target_block));
                    // Link successors/predecessors
                    let cur = self.current_block;
                    if let Some(b) = self.blocks.iter_mut().find(|b| b.id == cur) {
                        b.successors.push(target_block);
                    }
                    if let Some(b) = self.blocks.iter_mut().find(|b| b.id == target_block) {
                        b.predecessors.push(cur);
                    }
                    self.current_block = target_block;
                    let _ = offset; // offset used for block linkage metadata
                }
                Opcode::JumpIfFalse => {
                    let offset = Self::read_i16(opcodes, ip);
                    ip += 2;
                    let cond = self.pop_reg();
                    let true_block = self.new_block();
                    let false_block = self.new_block();
                    self.emit(SsaInstr::Branch(cond, true_block, false_block));
                    let cur = self.current_block;
                    if let Some(b) = self.blocks.iter_mut().find(|b| b.id == cur) {
                        b.successors.push(true_block);
                        b.successors.push(false_block);
                    }
                    if let Some(b) = self.blocks.iter_mut().find(|b| b.id == true_block) {
                        b.predecessors.push(cur);
                    }
                    if let Some(b) = self.blocks.iter_mut().find(|b| b.id == false_block) {
                        b.predecessors.push(cur);
                    }
                    self.current_block = true_block;
                    let _ = offset;
                }
                Opcode::Return => {
                    let val = if !self.stack_map.is_empty() {
                        Some(self.pop_reg())
                    } else {
                        None
                    };
                    self.emit(SsaInstr::Return(val));
                }
                Opcode::Call => {
                    let arg_count = Self::read_u16(opcodes, ip) as usize;
                    ip += 2;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.pop_reg());
                    }
                    args.reverse();
                    let func_reg = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::Call(dst, func_reg, args));
                    self.push_reg(dst);
                }
                Opcode::Eq => {
                    let rhs = self.pop_reg();
                    let lhs = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::Equal(dst, lhs, rhs));
                    self.push_reg(dst);
                }
                Opcode::Lt => {
                    let rhs = self.pop_reg();
                    let lhs = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::LessThan(dst, lhs, rhs));
                    self.push_reg(dst);
                }
                Opcode::Gt => {
                    let rhs = self.pop_reg();
                    let lhs = self.pop_reg();
                    let dst = self.alloc_reg();
                    self.emit(SsaInstr::GreaterThan(dst, lhs, rhs));
                    self.push_reg(dst);
                }
                _ => {
                    // Skip operands for unhandled opcodes
                    // Most opcodes we don't handle have 0 or 2 byte operands
                }
            }
        }

        SsaFunction {
            name: String::new(),
            blocks: std::mem::take(&mut self.blocks),
            num_registers: self.next_reg,
            num_params: 0,
            num_locals: 0,
            entry_block: 0,
        }
    }
}

impl Default for SsaTranslator {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== SSA IR Optimizer ====================

/// Optimizes SSA IR functions
pub struct SsaOptimizer {
    passes_applied: Vec<String>,
}

impl SsaOptimizer {
    pub fn new() -> Self {
        Self {
            passes_applied: Vec::new(),
        }
    }

    /// Apply all optimization passes
    pub fn optimize(&mut self, func: &mut SsaFunction) {
        let folded = self.constant_fold(func);
        if folded > 0 {
            self.passes_applied.push(format!("constant_fold({})", folded));
        }
        let eliminated = self.dead_code_eliminate(func);
        if eliminated > 0 {
            self.passes_applied.push(format!("dead_code_eliminate({})", eliminated));
        }
        let propagated = self.copy_propagation(func);
        if propagated > 0 {
            self.passes_applied.push(format!("copy_propagation({})", propagated));
        }
    }

    /// Fold constant expressions
    pub fn constant_fold(&self, func: &mut SsaFunction) -> usize {
        let mut folded = 0;

        // First pass: collect constant definitions (reg → value)
        let mut const_map: HashMap<SsaReg, SsaValue> = HashMap::default();
        for block in &func.blocks {
            for instr in &block.instructions {
                if let SsaInstr::LoadConst(reg, val) = instr {
                    const_map.insert(*reg, val.clone());
                }
            }
        }

        // Second pass: fold arithmetic on constants
        for block in &mut func.blocks {
            let mut i = 0;
            while i < block.instructions.len() {
                let replacement = match &block.instructions[i] {
                    SsaInstr::AddInt(dst, lhs, rhs) | SsaInstr::Add(dst, lhs, rhs) => {
                        match (const_map.get(lhs), const_map.get(rhs)) {
                            (Some(SsaValue::Integer(a)), Some(SsaValue::Integer(b))) => {
                                let result = SsaValue::Integer(a.wrapping_add(*b));
                                const_map.insert(*dst, result.clone());
                                Some(SsaInstr::LoadConst(*dst, result))
                            }
                            (Some(SsaValue::Float(a)), Some(SsaValue::Float(b))) => {
                                let result = SsaValue::Float(a + b);
                                const_map.insert(*dst, result.clone());
                                Some(SsaInstr::LoadConst(*dst, result))
                            }
                            _ => None,
                        }
                    }
                    SsaInstr::SubInt(dst, lhs, rhs) | SsaInstr::Sub(dst, lhs, rhs) => {
                        match (const_map.get(lhs), const_map.get(rhs)) {
                            (Some(SsaValue::Integer(a)), Some(SsaValue::Integer(b))) => {
                                let result = SsaValue::Integer(a.wrapping_sub(*b));
                                const_map.insert(*dst, result.clone());
                                Some(SsaInstr::LoadConst(*dst, result))
                            }
                            _ => None,
                        }
                    }
                    SsaInstr::MulInt(dst, lhs, rhs) | SsaInstr::Mul(dst, lhs, rhs) => {
                        match (const_map.get(lhs), const_map.get(rhs)) {
                            (Some(SsaValue::Integer(a)), Some(SsaValue::Integer(b))) => {
                                let result = SsaValue::Integer(a.wrapping_mul(*b));
                                const_map.insert(*dst, result.clone());
                                Some(SsaInstr::LoadConst(*dst, result))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(new_instr) = replacement {
                    block.instructions[i] = new_instr;
                    folded += 1;
                }
                i += 1;
            }
        }

        folded
    }

    /// Remove unreachable blocks and unused instructions
    pub fn dead_code_eliminate(&self, func: &mut SsaFunction) -> usize {
        if func.blocks.is_empty() {
            return 0;
        }

        // Find reachable blocks via BFS from entry
        let mut reachable = std::collections::HashSet::new();
        let mut worklist = vec![func.entry_block];
        while let Some(bid) = worklist.pop() {
            if !reachable.insert(bid) {
                continue;
            }
            if let Some(block) = func.blocks.iter().find(|b| b.id == bid) {
                for &succ in &block.successors {
                    worklist.push(succ);
                }
                // Also follow jump/branch targets in instructions
                for instr in &block.instructions {
                    match instr {
                        SsaInstr::Jump(target) => worklist.push(*target),
                        SsaInstr::Branch(_, t, f) => {
                            worklist.push(*t);
                            worklist.push(*f);
                        }
                        _ => {}
                    }
                }
            }
        }

        let before = func.blocks.iter().map(|b| b.instructions.len()).sum::<usize>() + func.blocks.len();
        func.blocks.retain(|b| reachable.contains(&b.id));
        let after = func.blocks.iter().map(|b| b.instructions.len()).sum::<usize>() + func.blocks.len();

        before.saturating_sub(after)
    }

    /// Replace uses of copies with the original value
    pub fn copy_propagation(&self, func: &mut SsaFunction) -> usize {
        // Build a map: if LoadLocal(dst, idx) and later StoreLocal(idx, src),
        // we can propagate. For SSA, track LoadConst copies.
        let mut copy_map: HashMap<SsaReg, SsaReg> = HashMap::default();
        let mut propagated = 0;

        // Identify copies: LoadLocal(dst, idx) where another reg already holds that local
        // For simplicity, track Phi nodes with single input as copies
        for block in &func.blocks {
            for instr in &block.instructions {
                if let SsaInstr::Phi(dst, sources) = instr {
                    if sources.len() == 1 {
                        copy_map.insert(*dst, sources[0].1);
                    }
                }
            }
        }

        if copy_map.is_empty() {
            return 0;
        }

        // Replace uses
        fn resolve(copy_map: &HashMap<SsaReg, SsaReg>, reg: SsaReg) -> SsaReg {
            let mut r = reg;
            let mut depth = 0;
            while let Some(&mapped) = copy_map.get(&r) {
                r = mapped;
                depth += 1;
                if depth > 100 { break; }
            }
            r
        }

        for block in &mut func.blocks {
            for instr in &mut block.instructions {
                match instr {
                    SsaInstr::Add(_, lhs, rhs)
                    | SsaInstr::Sub(_, lhs, rhs)
                    | SsaInstr::Mul(_, lhs, rhs)
                    | SsaInstr::Div(_, lhs, rhs)
                    | SsaInstr::Mod(_, lhs, rhs)
                    | SsaInstr::AddInt(_, lhs, rhs)
                    | SsaInstr::SubInt(_, lhs, rhs)
                    | SsaInstr::MulInt(_, lhs, rhs)
                    | SsaInstr::Equal(_, lhs, rhs)
                    | SsaInstr::StrictEqual(_, lhs, rhs)
                    | SsaInstr::LessThan(_, lhs, rhs)
                    | SsaInstr::GreaterThan(_, lhs, rhs)
                    | SsaInstr::LessEqual(_, lhs, rhs)
                    | SsaInstr::GreaterEqual(_, lhs, rhs) => {
                        let new_lhs = resolve(&copy_map, *lhs);
                        let new_rhs = resolve(&copy_map, *rhs);
                        if new_lhs != *lhs || new_rhs != *rhs {
                            *lhs = new_lhs;
                            *rhs = new_rhs;
                            propagated += 1;
                        }
                    }
                    SsaInstr::Not(_, src) | SsaInstr::Neg(_, src) => {
                        let new_src = resolve(&copy_map, *src);
                        if new_src != *src {
                            *src = new_src;
                            propagated += 1;
                        }
                    }
                    SsaInstr::Return(Some(reg)) => {
                        let new_reg = resolve(&copy_map, *reg);
                        if new_reg != *reg {
                            *reg = new_reg;
                            propagated += 1;
                        }
                    }
                    SsaInstr::Branch(cond, _, _) => {
                        let new_cond = resolve(&copy_map, *cond);
                        if new_cond != *cond {
                            *cond = new_cond;
                            propagated += 1;
                        }
                    }
                    _ => {}
                }
            }
        }

        propagated
    }

    /// Get the list of passes applied
    pub fn passes_applied(&self) -> &[String] {
        &self.passes_applied
    }
}

impl Default for SsaOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Register Allocator ====================

/// Allocation result for a virtual register
#[derive(Debug, Clone, Copy)]
pub enum RegAllocation {
    Register(u32),
    Spill(u32),
}

/// Simple linear scan register allocator
pub struct RegisterAllocator {
    /// Available physical registers
    num_physical_regs: u32,
    /// Map from virtual register to physical register or spill slot
    allocation: HashMap<SsaReg, RegAllocation>,
    /// Spill count
    spills: u32,
}

impl RegisterAllocator {
    pub fn new(num_physical_regs: u32) -> Self {
        Self {
            num_physical_regs,
            allocation: HashMap::default(),
            spills: 0,
        }
    }

    /// Perform linear scan register allocation
    pub fn allocate(&mut self, func: &SsaFunction) -> HashMap<SsaReg, RegAllocation> {
        self.allocation.clear();
        self.spills = 0;

        // Collect all virtual registers used, in order of first appearance
        let mut all_regs: Vec<SsaReg> = Vec::new();
        for block in &func.blocks {
            for instr in &block.instructions {
                for reg in Self::regs_in_instr(instr) {
                    if !all_regs.contains(&reg) {
                        all_regs.push(reg);
                    }
                }
            }
        }

        // Compute live ranges (simplified: first use to last use index)
        let mut first_use: HashMap<SsaReg, usize> = HashMap::default();
        let mut last_use: HashMap<SsaReg, usize> = HashMap::default();
        let mut idx = 0;
        for block in &func.blocks {
            for instr in &block.instructions {
                for reg in Self::regs_in_instr(instr) {
                    first_use.entry(reg).or_insert(idx);
                    last_use.insert(reg, idx);
                }
                idx += 1;
            }
        }

        // Sort by first use
        all_regs.sort_by_key(|r| first_use.get(r).copied().unwrap_or(0));

        // Linear scan: active list of (reg, end_point, phys_reg)
        let mut active: Vec<(SsaReg, usize, u32)> = Vec::new();
        let mut free_regs: Vec<u32> = (0..self.num_physical_regs).rev().collect();

        for vreg in &all_regs {
            let start = first_use.get(vreg).copied().unwrap_or(0);
            let end = last_use.get(vreg).copied().unwrap_or(0);

            // Expire old intervals
            active.retain(|&(r, end_point, phys)| {
                if end_point < start {
                    free_regs.push(phys);
                    let _ = r;
                    false
                } else {
                    true
                }
            });

            if let Some(phys) = free_regs.pop() {
                self.allocation.insert(*vreg, RegAllocation::Register(phys));
                active.push((*vreg, end, phys));
                active.sort_by_key(|&(_, e, _)| e);
            } else {
                // Spill
                let spill_slot = self.spills;
                self.spills += 1;
                self.allocation.insert(*vreg, RegAllocation::Spill(spill_slot));
            }
        }

        self.allocation.clone()
    }

    /// Get the spill count after allocation
    pub fn spill_count(&self) -> u32 {
        self.spills
    }

    /// Extract all registers referenced in an instruction
    fn regs_in_instr(instr: &SsaInstr) -> Vec<SsaReg> {
        match instr {
            SsaInstr::LoadConst(d, _) | SsaInstr::LoadUndefined(d) | SsaInstr::LoadNull(d) => vec![*d],
            SsaInstr::Add(d, l, r)
            | SsaInstr::Sub(d, l, r)
            | SsaInstr::Mul(d, l, r)
            | SsaInstr::Div(d, l, r)
            | SsaInstr::Mod(d, l, r)
            | SsaInstr::AddInt(d, l, r)
            | SsaInstr::SubInt(d, l, r)
            | SsaInstr::MulInt(d, l, r)
            | SsaInstr::Equal(d, l, r)
            | SsaInstr::StrictEqual(d, l, r)
            | SsaInstr::LessThan(d, l, r)
            | SsaInstr::GreaterThan(d, l, r)
            | SsaInstr::LessEqual(d, l, r)
            | SsaInstr::GreaterEqual(d, l, r) => vec![*d, *l, *r],
            SsaInstr::Neg(d, s) | SsaInstr::Not(d, s) => vec![*d, *s],
            SsaInstr::LoadLocal(d, _) => vec![*d],
            SsaInstr::StoreLocal(_, s) => vec![*s],
            SsaInstr::LoadGlobal(d, _) => vec![*d],
            SsaInstr::StoreGlobal(_, s) => vec![*s],
            SsaInstr::GetProperty(d, o, _) => vec![*d, *o],
            SsaInstr::SetProperty(o, _, v) => vec![*o, *v],
            SsaInstr::Call(d, f, args) => {
                let mut regs = vec![*d, *f];
                regs.extend(args);
                regs
            }
            SsaInstr::Return(Some(r)) => vec![*r],
            SsaInstr::Return(None) => vec![],
            SsaInstr::Jump(_) => vec![],
            SsaInstr::Branch(c, _, _) => vec![*c],
            SsaInstr::SsaTypeGuard(r, _, _) => vec![*r],
            SsaInstr::Phi(d, sources) => {
                let mut regs = vec![*d];
                for (_, r) in sources {
                    regs.push(*r);
                }
                regs
            }
        }
    }
}

// ==================== JIT Compilation Stats ====================

/// Statistics for the SSA IR compilation pipeline
#[derive(Debug, Clone, Default)]
pub struct JitCompilationStats {
    pub functions_translated: u64,
    pub total_ir_instructions: u64,
    pub optimizations_applied: u64,
    pub constants_folded: u64,
    pub dead_code_removed: u64,
    pub copies_propagated: u64,
    pub registers_allocated: u64,
    pub spills: u64,
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
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

    // ==================== TypeFeedback Tests ====================

    #[test]
    fn test_type_feedback_new() {
        let feedback = TypeFeedback::new();
        assert_eq!(feedback.total_observations, 0);
        assert!(feedback.monomorphic_sites().is_empty());
        assert!(feedback.polymorphic_sites().is_empty());
        assert!(feedback.megamorphic_sites().is_empty());
    }

    #[test]
    fn test_type_feedback_record_monomorphic() {
        let mut feedback = TypeFeedback::new();
        feedback.record(0, ObservedType::Int32);
        feedback.record(0, ObservedType::Int32);
        feedback.record(0, ObservedType::Int32);

        let profile = feedback.profile_at(0).unwrap();
        assert!(profile.is_monomorphic);
        assert!(!profile.is_polymorphic);
        assert!(!profile.is_megamorphic);
        assert_eq!(profile.execution_count, 3);
        assert_eq!(profile.observed_types.len(), 1);
    }

    #[test]
    fn test_type_feedback_record_polymorphic() {
        let mut feedback = TypeFeedback::new();
        feedback.record(0, ObservedType::Int32);
        feedback.record(0, ObservedType::Float64);
        feedback.record(0, ObservedType::String);

        let profile = feedback.profile_at(0).unwrap();
        assert!(!profile.is_monomorphic);
        assert!(profile.is_polymorphic);
        assert!(!profile.is_megamorphic);
        assert_eq!(profile.observed_types.len(), 3);
    }

    #[test]
    fn test_type_feedback_record_megamorphic() {
        let mut feedback = TypeFeedback::new();
        feedback.record(0, ObservedType::Int32);
        feedback.record(0, ObservedType::Float64);
        feedback.record(0, ObservedType::String);
        feedback.record(0, ObservedType::Boolean);
        feedback.record(0, ObservedType::Object);

        let profile = feedback.profile_at(0).unwrap();
        assert!(!profile.is_monomorphic);
        assert!(!profile.is_polymorphic);
        assert!(profile.is_megamorphic);
        assert_eq!(profile.observed_types.len(), 5);
    }

    #[test]
    fn test_type_feedback_site_lists() {
        let mut feedback = TypeFeedback::new();
        // Offset 0: monomorphic
        feedback.record(0, ObservedType::Int32);
        // Offset 1: polymorphic
        feedback.record(1, ObservedType::Int32);
        feedback.record(1, ObservedType::String);
        // Offset 2: megamorphic
        for ty in &[
            ObservedType::Int32,
            ObservedType::Float64,
            ObservedType::String,
            ObservedType::Boolean,
            ObservedType::Object,
        ] {
            feedback.record(2, *ty);
        }

        assert!(feedback.monomorphic_sites().contains(&0));
        assert!(feedback.polymorphic_sites().contains(&1));
        assert!(feedback.megamorphic_sites().contains(&2));
    }

    #[test]
    fn test_type_feedback_reset() {
        let mut feedback = TypeFeedback::new();
        feedback.record(0, ObservedType::Int32);
        feedback.record(1, ObservedType::String);
        assert_eq!(feedback.total_observations, 2);

        feedback.reset();
        assert_eq!(feedback.total_observations, 0);
        assert!(feedback.profile_at(0).is_none());
        assert!(feedback.monomorphic_sites().is_empty());
    }

    // ==================== InlineCache Tests ====================

    #[test]
    fn test_ic_new() {
        let cache = InlineCache::new(4);
        assert_eq!(cache.hit_rate(), 0.0);
        let stats = cache.stats();
        assert_eq!(stats.entry_count, 0);
        assert_eq!(stats.total_lookups, 0);
    }

    #[test]
    fn test_ic_insert_and_lookup() {
        let mut cache = InlineCache::new(4);
        cache.insert(42, 10, true);

        assert_eq!(cache.lookup(42), Some(10));
        assert_eq!(cache.lookup(99), None);
    }

    #[test]
    fn test_ic_hit_rate() {
        let mut cache = InlineCache::new(4);
        cache.insert(42, 10, true);

        for _ in 0..3 {
            cache.lookup(42);
        }
        cache.lookup(99);

        assert_eq!(cache.hit_rate(), 0.75);
        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.total_lookups, 4);
    }

    #[test]
    fn test_ic_invalidate() {
        let mut cache = InlineCache::new(4);
        cache.insert(42, 10, true);
        cache.insert(43, 20, false);

        cache.invalidate(42);
        assert_eq!(cache.lookup(42), None);
        assert_eq!(cache.lookup(43), Some(20));
    }

    #[test]
    fn test_ic_clear() {
        let mut cache = InlineCache::new(4);
        cache.insert(42, 10, true);
        cache.lookup(42);

        cache.clear();
        let stats = cache.stats();
        assert_eq!(stats.entry_count, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_ic_eviction() {
        let mut cache = InlineCache::new(2);
        cache.insert(1, 10, true);
        cache.insert(2, 20, true);
        cache.insert(3, 30, true);

        assert_eq!(cache.stats().entry_count, 2);
        assert_eq!(cache.lookup(3), Some(30));
    }

    #[test]
    fn test_ic_stats() {
        let mut cache = InlineCache::new(4);
        cache.insert(1, 10, true);
        cache.insert(2, 20, false);

        let stats = cache.stats();
        assert_eq!(stats.entry_count, 2);
        assert_eq!(stats.total_lookups, 0);
        assert_eq!(stats.hit_rate, 0.0);
    }

    // ==================== TieredCompilation Tests ====================

    #[test]
    fn test_tiered_compilation_new() {
        let tc = TieredCompilation::new(100, 1000);
        assert_eq!(tc.current_tier("foo"), CompilationTier::Interpreter);
        assert!(tc.pending_compilations().is_empty());
    }

    #[test]
    fn test_tiered_compilation_record_execution() {
        let mut tc = TieredCompilation::new(100, 1000);
        for _ in 0..100 {
            tc.record_execution("hot_func");
        }
        assert!(!tc.pending_compilations().is_empty());
        assert_eq!(
            tc.pending_compilations()[0].target_tier,
            CompilationTier::Baseline
        );
    }

    #[test]
    fn test_tiered_compilation_promote() {
        let mut tc = TieredCompilation::new(100, 1000);
        tc.promote("foo", CompilationTier::Baseline);
        assert_eq!(tc.current_tier("foo"), CompilationTier::Baseline);

        tc.promote("foo", CompilationTier::Optimized);
        assert_eq!(tc.current_tier("foo"), CompilationTier::Optimized);
    }

    #[test]
    fn test_tiered_compilation_drain_queue() {
        let mut tc = TieredCompilation::new(100, 1000);
        for _ in 0..100 {
            tc.record_execution("func_a");
        }

        let queue = tc.drain_compilation_queue();
        assert!(!queue.is_empty());
        assert!(tc.pending_compilations().is_empty());
    }

    #[test]
    fn test_tiered_compilation_tier2_promotion() {
        let mut tc = TieredCompilation::new(100, 1000);
        tc.promote("hot", CompilationTier::Baseline);
        for _ in 0..1000 {
            tc.record_execution("hot");
        }

        let queue = tc.pending_compilations();
        assert!(queue
            .iter()
            .any(|r| r.target_tier == CompilationTier::Optimized));
    }

    #[test]
    fn test_tiered_compilation_stats() {
        let mut tc = TieredCompilation::new(100, 1000);
        tc.promote("a", CompilationTier::Baseline);
        tc.promote("b", CompilationTier::Optimized);

        let stats = tc.stats();
        assert_eq!(stats.baseline_count, 1);
        assert_eq!(stats.optimized_count, 1);
        assert_eq!(stats.total_compilations, 2);
    }

    #[test]
    fn test_tiered_compilation_no_duplicate_queue() {
        let mut tc = TieredCompilation::new(10, 100);
        for _ in 0..50 {
            tc.record_execution("func");
        }
        // Should only have one request despite exceeding threshold many times
        assert_eq!(tc.pending_compilations().len(), 1);
    }

    // ==================== DeoptimizationTracker Tests ====================

    #[test]
    fn test_deopt_tracker_new() {
        let tracker = DeoptimizationTracker::new(5);
        assert!(tracker.should_jit("foo"));
        assert_eq!(tracker.deopt_count("foo"), 0);
        assert!(tracker.events().is_empty());
    }

    #[test]
    fn test_deopt_tracker_record() {
        let mut tracker = DeoptimizationTracker::new(5);
        tracker.record_deopt(DeoptEvent {
            function_name: "foo".to_string(),
            reason: DeoptReason::TypeMismatch,
            bytecode_offset: 10,
            timestamp: 1,
        });

        assert_eq!(tracker.deopt_count("foo"), 1);
        assert!(tracker.should_jit("foo"));
        assert_eq!(tracker.events().len(), 1);
    }

    #[test]
    fn test_deopt_tracker_threshold() {
        let mut tracker = DeoptimizationTracker::new(3);
        for i in 0..3 {
            tracker.record_deopt(DeoptEvent {
                function_name: "flaky".to_string(),
                reason: DeoptReason::TypeMismatch,
                bytecode_offset: i,
                timestamp: i as u64,
            });
        }

        assert!(!tracker.should_jit("flaky"));
        assert!(tracker.should_jit("other_func"));
    }

    #[test]
    fn test_deopt_tracker_reset() {
        let mut tracker = DeoptimizationTracker::new(3);
        tracker.record_deopt(DeoptEvent {
            function_name: "foo".to_string(),
            reason: DeoptReason::HiddenClassChange,
            bytecode_offset: 0,
            timestamp: 0,
        });

        tracker.reset();
        assert_eq!(tracker.deopt_count("foo"), 0);
        assert!(tracker.events().is_empty());
        assert!(tracker.should_jit("foo"));
    }

    #[test]
    fn test_deopt_reasons() {
        let mut tracker = DeoptimizationTracker::new(10);
        let reasons = [
            DeoptReason::TypeMismatch,
            DeoptReason::HiddenClassChange,
            DeoptReason::OverflowCheck,
            DeoptReason::BoundCheck,
            DeoptReason::UnexpectedValue,
            DeoptReason::StackOverflow,
            DeoptReason::Debugger,
        ];
        for (i, reason) in reasons.iter().enumerate() {
            tracker.record_deopt(DeoptEvent {
                function_name: format!("func_{}", i),
                reason: *reason,
                bytecode_offset: i,
                timestamp: i as u64,
            });
        }
        assert_eq!(tracker.events().len(), 7);
    }

    // ==================== SSA IR Tests ====================

    #[test]
    fn test_ssa_instr_creation_and_debug() {
        let instr = SsaInstr::LoadConst(0, SsaValue::Integer(42));
        let dbg = format!("{:?}", instr);
        assert!(dbg.contains("LoadConst"));
        assert!(dbg.contains("42"));

        let add = SsaInstr::Add(2, 0, 1);
        let dbg = format!("{:?}", add);
        assert!(dbg.contains("Add"));
    }

    #[test]
    fn test_ssa_block_creation() {
        let block = SsaBlock {
            id: 0,
            instructions: vec![
                SsaInstr::LoadConst(0, SsaValue::Integer(1)),
                SsaInstr::LoadConst(1, SsaValue::Integer(2)),
                SsaInstr::Add(2, 0, 1),
            ],
            predecessors: vec![],
            successors: vec![1],
        };
        assert_eq!(block.id, 0);
        assert_eq!(block.instructions.len(), 3);
        assert_eq!(block.successors, vec![1]);
    }

    #[test]
    fn test_ssa_function_creation() {
        let block = SsaBlock {
            id: 0,
            instructions: vec![SsaInstr::Return(None)],
            predecessors: vec![],
            successors: vec![],
        };
        let func = SsaFunction {
            name: "test".to_string(),
            blocks: vec![block],
            num_registers: 0,
            num_params: 0,
            num_locals: 0,
            entry_block: 0,
        };
        assert_eq!(func.name, "test");
        assert_eq!(func.blocks.len(), 1);
        assert_eq!(func.entry_block, 0);
    }

    #[test]
    fn test_ssa_translator_constant() {
        use crate::bytecode::Opcode;
        let mut translator = SsaTranslator::new();
        // Bytecode: Constant 0, Return
        let opcodes = vec![
            Opcode::Constant as u8, 0x00, 0x00,
            Opcode::Return as u8,
        ];
        let constants = vec![crate::Value::Number(42.0)];
        let func = translator.translate(&opcodes, &constants);
        assert!(!func.blocks.is_empty());
        // Should have a LoadConst and Return
        let instrs = &func.blocks[0].instructions;
        assert!(instrs.len() >= 2);
        assert!(matches!(&instrs[0], SsaInstr::LoadConst(0, SsaValue::Integer(42))));
        assert!(matches!(&instrs[1], SsaInstr::Return(Some(0))));
    }

    #[test]
    fn test_ssa_translator_add() {
        use crate::bytecode::Opcode;
        let mut translator = SsaTranslator::new();
        // Bytecode: Constant 0, Constant 1, Add, Return
        let opcodes = vec![
            Opcode::Constant as u8, 0x00, 0x00,
            Opcode::Constant as u8, 0x00, 0x01,
            Opcode::Add as u8,
            Opcode::Return as u8,
        ];
        let constants = vec![crate::Value::Number(3.0), crate::Value::Number(4.0)];
        let func = translator.translate(&opcodes, &constants);
        let instrs = &func.blocks[0].instructions;
        // LoadConst(0, 3), LoadConst(1, 4), Add(2, 0, 1), Return(Some(2))
        assert!(instrs.len() >= 4);
        assert!(matches!(&instrs[2], SsaInstr::Add(2, 0, 1)));
    }

    #[test]
    fn test_ssa_translator_local_access() {
        use crate::bytecode::Opcode;
        let mut translator = SsaTranslator::new();
        // Bytecode: Constant 0, SetLocal 0, GetLocal 0, Return
        let opcodes = vec![
            Opcode::Constant as u8, 0x00, 0x00,
            Opcode::SetLocal as u8, 0x00, 0x00,
            Opcode::GetLocal as u8, 0x00, 0x00,
            Opcode::Return as u8,
        ];
        let constants = vec![crate::Value::Number(99.0)];
        let func = translator.translate(&opcodes, &constants);
        let instrs = &func.blocks[0].instructions;
        assert!(instrs.len() >= 4);
        assert!(matches!(&instrs[1], SsaInstr::StoreLocal(0, 0)));
        assert!(matches!(&instrs[2], SsaInstr::LoadLocal(1, 0)));
    }

    #[test]
    fn test_ssa_translator_stack_management() {
        let mut translator = SsaTranslator::new();
        let r0 = translator.alloc_reg();
        let r1 = translator.alloc_reg();
        translator.push_reg(r0);
        translator.push_reg(r1);
        assert_eq!(translator.pop_reg(), r1);
        assert_eq!(translator.pop_reg(), r0);
        // Popping empty returns 0
        assert_eq!(translator.pop_reg(), 0);
    }

    #[test]
    fn test_ssa_optimizer_constant_fold() {
        let optimizer = SsaOptimizer::new();
        let mut func = SsaFunction {
            name: "fold_test".to_string(),
            blocks: vec![SsaBlock {
                id: 0,
                instructions: vec![
                    SsaInstr::LoadConst(0, SsaValue::Integer(3)),
                    SsaInstr::LoadConst(1, SsaValue::Integer(4)),
                    SsaInstr::AddInt(2, 0, 1),
                    SsaInstr::Return(Some(2)),
                ],
                predecessors: vec![],
                successors: vec![],
            }],
            num_registers: 3,
            num_params: 0,
            num_locals: 0,
            entry_block: 0,
        };
        let folded = optimizer.constant_fold(&mut func);
        assert_eq!(folded, 1);
        // The AddInt should be replaced with LoadConst(2, 7)
        assert!(matches!(
            &func.blocks[0].instructions[2],
            SsaInstr::LoadConst(2, SsaValue::Integer(7))
        ));
    }

    #[test]
    fn test_ssa_optimizer_dce_unreachable_block() {
        let optimizer = SsaOptimizer::new();
        let mut func = SsaFunction {
            name: "dce_test".to_string(),
            blocks: vec![
                SsaBlock {
                    id: 0,
                    instructions: vec![SsaInstr::Return(None)],
                    predecessors: vec![],
                    successors: vec![],
                },
                SsaBlock {
                    id: 1,
                    instructions: vec![
                        SsaInstr::LoadConst(0, SsaValue::Integer(999)),
                        SsaInstr::Return(Some(0)),
                    ],
                    predecessors: vec![],
                    successors: vec![],
                },
            ],
            num_registers: 1,
            num_params: 0,
            num_locals: 0,
            entry_block: 0,
        };
        let eliminated = optimizer.dead_code_eliminate(&mut func);
        assert!(eliminated > 0);
        assert_eq!(func.blocks.len(), 1);
        assert_eq!(func.blocks[0].id, 0);
    }

    #[test]
    fn test_ssa_optimizer_copy_propagation() {
        let optimizer = SsaOptimizer::new();
        let mut func = SsaFunction {
            name: "copy_prop".to_string(),
            blocks: vec![SsaBlock {
                id: 0,
                instructions: vec![
                    SsaInstr::LoadConst(0, SsaValue::Integer(5)),
                    // Phi with single input = copy: r1 = r0
                    SsaInstr::Phi(1, vec![(0, 0)]),
                    // Add r2 = r1 + r1 → should become r0 + r0
                    SsaInstr::Add(2, 1, 1),
                    SsaInstr::Return(Some(2)),
                ],
                predecessors: vec![],
                successors: vec![],
            }],
            num_registers: 3,
            num_params: 0,
            num_locals: 0,
            entry_block: 0,
        };
        let propagated = optimizer.copy_propagation(&mut func);
        assert!(propagated > 0);
        // r1 uses in Add should be replaced with r0
        if let SsaInstr::Add(_, lhs, rhs) = &func.blocks[0].instructions[2] {
            assert_eq!(*lhs, 0);
            assert_eq!(*rhs, 0);
        } else {
            panic!("Expected Add instruction");
        }
    }

    #[test]
    fn test_register_allocator_basic() {
        let mut alloc = RegisterAllocator::new(8);
        let func = SsaFunction {
            name: "alloc_test".to_string(),
            blocks: vec![SsaBlock {
                id: 0,
                instructions: vec![
                    SsaInstr::LoadConst(0, SsaValue::Integer(1)),
                    SsaInstr::LoadConst(1, SsaValue::Integer(2)),
                    SsaInstr::Add(2, 0, 1),
                    SsaInstr::Return(Some(2)),
                ],
                predecessors: vec![],
                successors: vec![],
            }],
            num_registers: 3,
            num_params: 0,
            num_locals: 0,
            entry_block: 0,
        };
        let result = alloc.allocate(&func);
        assert_eq!(result.len(), 3);
        assert_eq!(alloc.spill_count(), 0);
        // All should be physical registers
        for (_, alloc_result) in &result {
            assert!(matches!(alloc_result, RegAllocation::Register(_)));
        }
    }

    #[test]
    fn test_register_allocator_spill() {
        let mut alloc = RegisterAllocator::new(2);
        // Create a function that uses many registers simultaneously
        let func = SsaFunction {
            name: "spill_test".to_string(),
            blocks: vec![SsaBlock {
                id: 0,
                instructions: vec![
                    SsaInstr::LoadConst(0, SsaValue::Integer(1)),
                    SsaInstr::LoadConst(1, SsaValue::Integer(2)),
                    SsaInstr::LoadConst(2, SsaValue::Integer(3)),
                    SsaInstr::LoadConst(3, SsaValue::Integer(4)),
                    // Use all 4 regs at the end so they're all live
                    SsaInstr::Add(4, 0, 1),
                    SsaInstr::Add(5, 2, 3),
                    SsaInstr::Add(6, 4, 5),
                    SsaInstr::Return(Some(6)),
                ],
                predecessors: vec![],
                successors: vec![],
            }],
            num_registers: 7,
            num_params: 0,
            num_locals: 0,
            entry_block: 0,
        };
        let result = alloc.allocate(&func);
        assert!(result.len() > 0);
        assert!(alloc.spill_count() > 0);
        // At least some should be spilled
        assert!(result.values().any(|a| matches!(a, RegAllocation::Spill(_))));
    }

    #[test]
    fn test_ssa_type_equality() {
        assert_eq!(SsaType::Integer, SsaType::Integer);
        assert_ne!(SsaType::Integer, SsaType::Float);
        assert_ne!(SsaType::String, SsaType::Boolean);
        assert_eq!(SsaType::Any, SsaType::Any);
        assert_ne!(SsaType::Object, SsaType::Array);
    }

    #[test]
    fn test_ssa_value_variants() {
        let int_val = SsaValue::Integer(42);
        assert!(matches!(int_val, SsaValue::Integer(42)));

        let float_val = SsaValue::Float(3.14);
        assert!(matches!(float_val, SsaValue::Float(f) if (f - 3.14).abs() < f64::EPSILON));

        let str_val = SsaValue::String("hello".to_string());
        assert!(matches!(str_val, SsaValue::String(ref s) if s == "hello"));

        let bool_val = SsaValue::Boolean(true);
        assert!(matches!(bool_val, SsaValue::Boolean(true)));
    }

    #[test]
    fn test_ssa_type_guard_instruction() {
        let guard = SsaInstr::SsaTypeGuard(0, SsaType::Integer, 1);
        let dbg = format!("{:?}", guard);
        assert!(dbg.contains("SsaTypeGuard"));
        assert!(dbg.contains("Integer"));
    }

    #[test]
    fn test_ssa_phi_function() {
        let phi = SsaInstr::Phi(3, vec![(0, 1), (1, 2)]);
        let dbg = format!("{:?}", phi);
        assert!(dbg.contains("Phi"));
        if let SsaInstr::Phi(dst, sources) = &phi {
            assert_eq!(*dst, 3);
            assert_eq!(sources.len(), 2);
            assert_eq!(sources[0], (0, 1));
            assert_eq!(sources[1], (1, 2));
        }
    }

    #[test]
    fn test_jit_compilation_stats() {
        let mut stats = JitCompilationStats::default();
        assert_eq!(stats.functions_translated, 0);
        assert_eq!(stats.spills, 0);

        stats.functions_translated = 5;
        stats.total_ir_instructions = 100;
        stats.constants_folded = 10;
        stats.dead_code_removed = 3;
        stats.copies_propagated = 2;
        stats.registers_allocated = 50;
        stats.spills = 4;
        stats.optimizations_applied = 15;

        assert_eq!(stats.functions_translated, 5);
        assert_eq!(stats.optimizations_applied, 15);
        assert_eq!(stats.spills, 4);
    }
}
