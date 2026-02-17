//! WebGPU compute shader abstraction layer for Quicksilver
//!
//! Provides GPU compute shader types and a simulated execution backend.
//! The API surface mirrors WebGPU/wgpu concepts (devices, buffers, pipelines,
//! compute passes) so that a future `wgpu` integration can be swapped in
//! without changing the public interface.
//!
//! Currently all GPU work is executed on the CPU; the module is an abstraction
//! layer that validates inputs, tracks statistics, and performs the math in
//! plain Rust.

use crate::error::{Error, Result};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// Unique identifier for a GPU buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BufferId(pub u64);

/// Unique identifier for a compute pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PipelineId(pub u64);

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// GPU hardware vendor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Unknown(String),
}

impl fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuVendor::Nvidia => write!(f, "NVIDIA"),
            GpuVendor::Amd => write!(f, "AMD"),
            GpuVendor::Intel => write!(f, "Intel"),
            GpuVendor::Apple => write!(f, "Apple"),
            GpuVendor::Unknown(s) => write!(f, "{}", s),
        }
    }
}

/// GPU device type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuDeviceType {
    DiscreteGpu,
    IntegratedGpu,
    Cpu,
    VirtualGpu,
}

/// Optional GPU features that may or may not be supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuFeature {
    Float16,
    Float64,
    TimestampQuery,
    PipelineStatistics,
}

/// Type of a shader binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingType {
    StorageBuffer { read_only: bool },
    UniformBuffer,
    Sampler,
    Texture,
}

// ---------------------------------------------------------------------------
// BufferUsage (bitflags-like)
// ---------------------------------------------------------------------------

/// Bitflags-style buffer usage descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BufferUsage(pub u32);

impl BufferUsage {
    pub const STORAGE: Self = Self(1);
    pub const UNIFORM: Self = Self(2);
    pub const MAP_READ: Self = Self(4);
    pub const MAP_WRITE: Self = Self(8);
    pub const COPY_SRC: Self = Self(16);
    pub const COPY_DST: Self = Self(32);

    /// Returns `true` when `self` contains all bits in `other`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for BufferUsage {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

// ---------------------------------------------------------------------------
// Limits / stats
// ---------------------------------------------------------------------------

/// Hardware limits reported by the GPU device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuLimits {
    pub max_buffer_size: u64,
    pub max_compute_workgroups_per_dimension: u32,
    pub max_compute_invocations_per_workgroup: u32,
    pub max_compute_workgroup_size_x: u32,
    pub max_compute_workgroup_size_y: u32,
    pub max_compute_workgroup_size_z: u32,
    pub max_bind_groups: u32,
    pub max_storage_buffers_per_shader_stage: u32,
}

impl Default for GpuLimits {
    fn default() -> Self {
        Self {
            max_buffer_size: 256 * 1024 * 1024, // 256 MiB
            max_compute_workgroups_per_dimension: 65535,
            max_compute_invocations_per_workgroup: 256,
            max_compute_workgroup_size_x: 256,
            max_compute_workgroup_size_y: 256,
            max_compute_workgroup_size_z: 64,
            max_bind_groups: 4,
            max_storage_buffers_per_shader_stage: 8,
        }
    }
}

/// Cumulative statistics for GPU operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuStats {
    pub buffers_created: u64,
    pub buffers_destroyed: u64,
    pub pipelines_created: u64,
    pub pipelines_destroyed: u64,
    pub dispatches: u64,
    pub bytes_written: u64,
    pub bytes_read: u64,
}

// ---------------------------------------------------------------------------
// Buffer
// ---------------------------------------------------------------------------

/// A GPU memory buffer (CPU-side simulation).
#[derive(Debug, Clone)]
pub struct GpuBuffer {
    pub id: BufferId,
    pub size: u64,
    pub usage: BufferUsage,
    /// CPU-side backing storage.
    pub data: Vec<u8>,
    pub mapped: bool,
}

// ---------------------------------------------------------------------------
// Shader / pipeline types
// ---------------------------------------------------------------------------

/// A single binding declared in a shader module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaderBinding {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub binding_type: BindingType,
}

/// A compiled shader module (WGSL source).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShaderModule {
    pub source: String,
    pub entry_point: String,
    pub bindings: Vec<ShaderBinding>,
}

/// A single entry inside a bind group.
#[derive(Debug, Clone)]
pub struct BindGroupEntry {
    pub binding: u32,
    pub buffer_id: BufferId,
}

/// A bind group that maps shader bindings to buffers.
#[derive(Debug, Clone)]
pub struct BindGroup {
    pub entries: Vec<BindGroupEntry>,
}

/// A compute pipeline encapsulating a shader and its configuration.
#[derive(Debug, Clone)]
pub struct ComputePipeline {
    pub id: PipelineId,
    pub shader: ShaderModule,
    pub bind_groups: Vec<BindGroup>,
    pub workgroup_size: [u32; 3],
}

// ---------------------------------------------------------------------------
// Compute pass
// ---------------------------------------------------------------------------

/// Describes a single compute dispatch.
#[derive(Debug, Clone)]
pub struct ComputePass {
    pub pipeline_id: PipelineId,
    pub bind_groups: Vec<u32>,
    /// Workgroup counts in [x, y, z].
    pub dispatch: [u32; 3],
}

// ---------------------------------------------------------------------------
// GpuDevice
// ---------------------------------------------------------------------------

/// Represents a (simulated) GPU device.
pub struct GpuDevice {
    pub name: String,
    pub vendor: GpuVendor,
    pub device_type: GpuDeviceType,
    pub limits: GpuLimits,
    pub features: Vec<GpuFeature>,
    pub buffers: HashMap<BufferId, GpuBuffer>,
    pub pipelines: HashMap<PipelineId, ComputePipeline>,
    pub stats: GpuStats,
    next_buffer_id: u64,
    next_pipeline_id: u64,
}

impl fmt::Debug for GpuDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuDevice")
            .field("name", &self.name)
            .field("vendor", &self.vendor)
            .field("device_type", &self.device_type)
            .field("buffers", &self.buffers.len())
            .field("pipelines", &self.pipelines.len())
            .finish()
    }
}

impl GpuDevice {
    /// Create a new simulated GPU device.
    pub fn new(name: impl Into<String>, vendor: GpuVendor, device_type: GpuDeviceType) -> Self {
        Self {
            name: name.into(),
            vendor,
            device_type,
            limits: GpuLimits::default(),
            features: vec![
                GpuFeature::Float16,
                GpuFeature::Float64,
                GpuFeature::TimestampQuery,
                GpuFeature::PipelineStatistics,
            ],
            buffers: HashMap::default(),
            pipelines: HashMap::default(),
            stats: GpuStats::default(),
            next_buffer_id: 1,
            next_pipeline_id: 1,
        }
    }

    // -- buffer operations --------------------------------------------------

    /// Allocate a new buffer of `size` bytes.
    pub fn create_buffer(&mut self, size: u64, usage: BufferUsage) -> Result<BufferId> {
        if size > self.limits.max_buffer_size {
            return Err(Error::InternalError(format!(
                "GPU buffer size {} exceeds limit {}",
                size, self.limits.max_buffer_size
            )));
        }
        let id = BufferId(self.next_buffer_id);
        self.next_buffer_id += 1;
        let buf = GpuBuffer {
            id,
            size,
            usage,
            data: vec![0u8; size as usize],
            mapped: false,
        };
        self.buffers.insert(id, buf);
        self.stats.buffers_created += 1;
        Ok(id)
    }

    /// Destroy an existing buffer.
    pub fn destroy_buffer(&mut self, id: BufferId) -> Result<()> {
        self.buffers
            .remove(&id)
            .ok_or_else(|| Error::InternalError(format!("GPU buffer {:?} not found", id)))?;
        self.stats.buffers_destroyed += 1;
        Ok(())
    }

    /// Write raw bytes into a buffer at `offset`.
    pub fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()> {
        let buf = self
            .buffers
            .get_mut(&id)
            .ok_or_else(|| Error::InternalError(format!("GPU buffer {:?} not found", id)))?;
        let start = offset as usize;
        let end = start + data.len();
        if end > buf.data.len() {
            return Err(Error::InternalError("GPU write out of bounds".into()));
        }
        buf.data[start..end].copy_from_slice(data);
        self.stats.bytes_written += data.len() as u64;
        Ok(())
    }

    /// Read raw bytes from a buffer at `offset`.
    pub fn read_buffer(&self, id: BufferId, offset: u64, len: usize) -> Result<Vec<u8>> {
        let buf = self
            .buffers
            .get(&id)
            .ok_or_else(|| Error::InternalError(format!("GPU buffer {:?} not found", id)))?;
        let start = offset as usize;
        let end = start + len;
        if end > buf.data.len() {
            return Err(Error::InternalError("GPU read out of bounds".into()));
        }
        // NB: stats is not &mut self here; tracked at higher level if needed
        Ok(buf.data[start..end].to_vec())
    }

    /// Convenience: write a slice of `f32` values into a buffer.
    pub fn write_buffer_f32(&mut self, id: BufferId, offset: u64, values: &[f32]) -> Result<()> {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.write_buffer(id, offset, &bytes)
    }

    /// Convenience: read `count` `f32` values from a buffer.
    pub fn read_buffer_f32(&self, id: BufferId, offset: u64, count: usize) -> Result<Vec<f32>> {
        let bytes = self.read_buffer(id, offset, count * 4)?;
        let values = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(values)
    }

    // -- pipeline operations ------------------------------------------------

    /// Create a compute pipeline from a shader module.
    pub fn create_compute_pipeline(
        &mut self,
        shader: ShaderModule,
        bind_groups: Vec<BindGroup>,
        workgroup_size: [u32; 3],
    ) -> Result<PipelineId> {
        if workgroup_size[0] > self.limits.max_compute_workgroup_size_x
            || workgroup_size[1] > self.limits.max_compute_workgroup_size_y
            || workgroup_size[2] > self.limits.max_compute_workgroup_size_z
        {
            return Err(Error::InternalError(
                "GPU workgroup size exceeds device limits".into(),
            ));
        }
        let invocations =
            workgroup_size[0] as u64 * workgroup_size[1] as u64 * workgroup_size[2] as u64;
        if invocations > self.limits.max_compute_invocations_per_workgroup as u64 {
            return Err(Error::InternalError(
                "GPU workgroup invocations exceed device limit".into(),
            ));
        }
        let id = PipelineId(self.next_pipeline_id);
        self.next_pipeline_id += 1;
        let pipeline = ComputePipeline {
            id,
            shader,
            bind_groups,
            workgroup_size,
        };
        self.pipelines.insert(id, pipeline);
        self.stats.pipelines_created += 1;
        Ok(id)
    }

    /// Destroy a compute pipeline.
    pub fn destroy_pipeline(&mut self, id: PipelineId) -> Result<()> {
        self.pipelines
            .remove(&id)
            .ok_or_else(|| Error::InternalError(format!("GPU pipeline {:?} not found", id)))?;
        self.stats.pipelines_destroyed += 1;
        Ok(())
    }

    // -- dispatch -----------------------------------------------------------

    /// Record and immediately execute a compute dispatch (simulated).
    pub fn dispatch_compute(
        &mut self,
        pipeline_id: PipelineId,
        workgroups: [u32; 3],
    ) -> Result<()> {
        if !self.pipelines.contains_key(&pipeline_id) {
            return Err(Error::InternalError(format!(
                "GPU pipeline {:?} not found",
                pipeline_id
            )));
        }
        for &dim in &workgroups {
            if dim > self.limits.max_compute_workgroups_per_dimension {
                return Err(Error::InternalError(
                    "GPU workgroup count exceeds device limits".into(),
                ));
            }
        }
        self.stats.dispatches += 1;
        Ok(())
    }

    /// Submit a `ComputePass` for execution (simulated).
    pub fn submit_pass(&mut self, pass: &ComputePass) -> Result<()> {
        self.dispatch_compute(pass.pipeline_id, pass.dispatch)
    }

    // -- simulated operations -----------------------------------------------

    /// CPU-simulated matrix multiply: C = A × B.
    ///
    /// `a` is m×n, `b` is n×k, result is m×k (row-major).
    pub fn matrix_multiply(
        &self,
        a: &[f32],
        b: &[f32],
        m: usize,
        n: usize,
        k: usize,
    ) -> Result<Vec<f32>> {
        if a.len() != m * n {
            return Err(Error::InternalError(format!(
                "GPU matrix_multiply: `a` length {} != m*n ({}*{}={})",
                a.len(),
                m,
                n,
                m * n
            )));
        }
        if b.len() != n * k {
            return Err(Error::InternalError(format!(
                "GPU matrix_multiply: `b` length {} != n*k ({}*{}={})",
                b.len(),
                n,
                k,
                n * k
            )));
        }
        let mut c = vec![0.0f32; m * k];
        for i in 0..m {
            for j in 0..k {
                let mut sum = 0.0f32;
                for p in 0..n {
                    sum += a[i * n + p] * b[p * k + j];
                }
                c[i * k + j] = sum;
            }
        }
        Ok(c)
    }

    /// CPU-simulated element-wise vector addition.
    pub fn vector_add(&self, a: &[f32], b: &[f32]) -> Result<Vec<f32>> {
        if a.len() != b.len() {
            return Err(Error::InternalError(
                "GPU vector_add: length mismatch".into(),
            ));
        }
        Ok(a.iter().zip(b.iter()).map(|(x, y)| x + y).collect())
    }
}

// ---------------------------------------------------------------------------
// GpuRuntime – high-level API for JS integration
// ---------------------------------------------------------------------------

/// High-level GPU runtime intended for JavaScript integration.
///
/// Wraps a [`GpuDevice`] and provides ergonomic helpers for common
/// compute operations (matrix multiply, vector math).
pub struct GpuRuntime {
    device: Option<GpuDevice>,
    auto_device: bool,
}

impl fmt::Debug for GpuRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuRuntime")
            .field("initialized", &self.device.is_some())
            .field("auto_device", &self.auto_device)
            .finish()
    }
}

impl GpuRuntime {
    /// Create a new, uninitialised GPU runtime.
    pub fn new() -> Self {
        Self {
            device: None,
            auto_device: true,
        }
    }

    /// Initialise the runtime by creating a simulated GPU device.
    pub fn initialize(&mut self) -> Result<()> {
        let device = GpuDevice::new(
            "Quicksilver Simulated GPU",
            GpuVendor::Unknown("Simulated".into()),
            GpuDeviceType::Cpu,
        );
        self.device = Some(device);
        Ok(())
    }

    /// Return a reference to the underlying device, if initialised.
    pub fn device(&self) -> Option<&GpuDevice> {
        self.device.as_ref()
    }

    /// Return a mutable reference to the underlying device.
    pub fn device_mut(&mut self) -> Option<&mut GpuDevice> {
        self.device.as_mut()
    }

    fn require_device(&self) -> Result<&GpuDevice> {
        self.device
            .as_ref()
            .ok_or_else(|| Error::InternalError("GPU runtime not initialised".into()))
    }

    /// Matrix multiply C = A × B (row-major, m×n * n×k → m×k).
    pub fn matrix_multiply(
        &mut self,
        a: &[f32],
        b: &[f32],
        m: usize,
        n: usize,
        k: usize,
    ) -> Result<Vec<f32>> {
        let device = self
            .device
            .as_mut()
            .ok_or_else(|| Error::InternalError("GPU runtime not initialised".into()))?;
        device.stats.dispatches += 1;
        device.matrix_multiply(a, b, m, n, k)
    }

    /// Element-wise vector addition.
    pub fn vector_add(&self, a: &[f32], b: &[f32]) -> Result<Vec<f32>> {
        self.require_device()?.vector_add(a, b)
    }

    /// Scale every element by `scalar`.
    pub fn vector_scale(&self, a: &[f32], scalar: f32) -> Result<Vec<f32>> {
        self.require_device()?;
        Ok(a.iter().map(|v| v * scalar).collect())
    }

    /// Sum all elements.
    pub fn reduce_sum(&self, a: &[f32]) -> Result<f32> {
        self.require_device()?;
        Ok(a.iter().sum())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn simulated_device() -> GpuDevice {
        GpuDevice::new("Test GPU", GpuVendor::Nvidia, GpuDeviceType::DiscreteGpu)
    }

    // -- device creation ----------------------------------------------------

    #[test]
    fn test_device_creation() {
        let dev = simulated_device();
        assert_eq!(dev.name, "Test GPU");
        assert_eq!(dev.vendor, GpuVendor::Nvidia);
        assert_eq!(dev.device_type, GpuDeviceType::DiscreteGpu);
        assert!(dev.buffers.is_empty());
        assert!(dev.pipelines.is_empty());
    }

    #[test]
    fn test_device_default_features() {
        let dev = simulated_device();
        assert!(dev.features.contains(&GpuFeature::Float16));
        assert!(dev.features.contains(&GpuFeature::Float64));
        assert!(dev.features.contains(&GpuFeature::TimestampQuery));
        assert!(dev.features.contains(&GpuFeature::PipelineStatistics));
    }

    #[test]
    fn test_device_limits_defaults() {
        let limits = GpuLimits::default();
        assert_eq!(limits.max_buffer_size, 256 * 1024 * 1024);
        assert_eq!(limits.max_compute_workgroups_per_dimension, 65535);
        assert_eq!(limits.max_bind_groups, 4);
    }

    // -- buffer operations --------------------------------------------------

    #[test]
    fn test_create_buffer() {
        let mut dev = simulated_device();
        let id = dev.create_buffer(1024, BufferUsage::STORAGE).unwrap();
        assert_eq!(dev.buffers.len(), 1);
        assert_eq!(dev.buffers[&id].size, 1024);
        assert_eq!(dev.stats.buffers_created, 1);
    }

    #[test]
    fn test_create_buffer_exceeds_limit() {
        let mut dev = simulated_device();
        let big = dev.limits.max_buffer_size + 1;
        assert!(dev.create_buffer(big, BufferUsage::STORAGE).is_err());
    }

    #[test]
    fn test_destroy_buffer() {
        let mut dev = simulated_device();
        let id = dev.create_buffer(64, BufferUsage::STORAGE).unwrap();
        dev.destroy_buffer(id).unwrap();
        assert!(dev.buffers.is_empty());
        assert_eq!(dev.stats.buffers_destroyed, 1);
    }

    #[test]
    fn test_destroy_nonexistent_buffer() {
        let mut dev = simulated_device();
        assert!(dev.destroy_buffer(BufferId(999)).is_err());
    }

    #[test]
    fn test_write_and_read_buffer() {
        let mut dev = simulated_device();
        let id = dev.create_buffer(16, BufferUsage::STORAGE).unwrap();
        dev.write_buffer(id, 0, &[1, 2, 3, 4]).unwrap();
        let out = dev.read_buffer(id, 0, 4).unwrap();
        assert_eq!(out, vec![1, 2, 3, 4]);
        assert_eq!(dev.stats.bytes_written, 4);
    }

    #[test]
    fn test_write_buffer_out_of_bounds() {
        let mut dev = simulated_device();
        let id = dev.create_buffer(4, BufferUsage::STORAGE).unwrap();
        assert!(dev.write_buffer(id, 0, &[0u8; 8]).is_err());
    }

    #[test]
    fn test_read_buffer_f32() {
        let mut dev = simulated_device();
        let id = dev.create_buffer(16, BufferUsage::STORAGE).unwrap();
        dev.write_buffer_f32(id, 0, &[1.0, 2.0, 3.0, 4.0]).unwrap();
        let vals = dev.read_buffer_f32(id, 0, 4).unwrap();
        assert_eq!(vals, vec![1.0, 2.0, 3.0, 4.0]);
    }

    // -- pipeline creation --------------------------------------------------

    #[test]
    fn test_create_pipeline() {
        let mut dev = simulated_device();
        let shader = ShaderModule {
            source: "@compute @workgroup_size(64) fn main() {}".into(),
            entry_point: "main".into(),
            bindings: vec![],
        };
        let pid = dev
            .create_compute_pipeline(shader, vec![], [64, 1, 1])
            .unwrap();
        assert!(dev.pipelines.contains_key(&pid));
        assert_eq!(dev.stats.pipelines_created, 1);
    }

    #[test]
    fn test_create_pipeline_exceeds_workgroup() {
        let mut dev = simulated_device();
        let shader = ShaderModule {
            source: String::new(),
            entry_point: "main".into(),
            bindings: vec![],
        };
        let res = dev.create_compute_pipeline(shader, vec![], [512, 1, 1]);
        assert!(res.is_err());
    }

    #[test]
    fn test_destroy_pipeline() {
        let mut dev = simulated_device();
        let shader = ShaderModule {
            source: String::new(),
            entry_point: "main".into(),
            bindings: vec![],
        };
        let pid = dev
            .create_compute_pipeline(shader, vec![], [64, 1, 1])
            .unwrap();
        dev.destroy_pipeline(pid).unwrap();
        assert!(dev.pipelines.is_empty());
        assert_eq!(dev.stats.pipelines_destroyed, 1);
    }

    // -- dispatch -----------------------------------------------------------

    #[test]
    fn test_dispatch_compute() {
        let mut dev = simulated_device();
        let shader = ShaderModule {
            source: String::new(),
            entry_point: "main".into(),
            bindings: vec![],
        };
        let pid = dev
            .create_compute_pipeline(shader, vec![], [64, 1, 1])
            .unwrap();
        dev.dispatch_compute(pid, [4, 1, 1]).unwrap();
        assert_eq!(dev.stats.dispatches, 1);
    }

    #[test]
    fn test_submit_pass() {
        let mut dev = simulated_device();
        let shader = ShaderModule {
            source: String::new(),
            entry_point: "main".into(),
            bindings: vec![],
        };
        let pid = dev
            .create_compute_pipeline(shader, vec![], [64, 1, 1])
            .unwrap();
        let pass = ComputePass {
            pipeline_id: pid,
            bind_groups: vec![0],
            dispatch: [2, 2, 1],
        };
        dev.submit_pass(&pass).unwrap();
        assert_eq!(dev.stats.dispatches, 1);
    }

    // -- simulated math -----------------------------------------------------

    #[test]
    fn test_matrix_multiply() {
        let dev = simulated_device();
        // 2×3 * 3×2 = 2×2
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let b = vec![7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
        let c = dev.matrix_multiply(&a, &b, 2, 3, 2).unwrap();
        assert_eq!(c, vec![58.0, 64.0, 139.0, 154.0]);
    }

    #[test]
    fn test_matrix_multiply_dimension_mismatch() {
        let dev = simulated_device();
        assert!(dev.matrix_multiply(&[1.0, 2.0], &[3.0], 2, 2, 1).is_err());
    }

    #[test]
    fn test_vector_add() {
        let dev = simulated_device();
        let r = dev.vector_add(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]).unwrap();
        assert_eq!(r, vec![5.0, 7.0, 9.0]);
    }

    #[test]
    fn test_vector_add_length_mismatch() {
        let dev = simulated_device();
        assert!(dev.vector_add(&[1.0], &[2.0, 3.0]).is_err());
    }

    // -- GpuRuntime ---------------------------------------------------------

    #[test]
    fn test_runtime_not_initialised() {
        let rt = GpuRuntime::new();
        assert!(rt.vector_add(&[1.0], &[2.0]).is_err());
    }

    #[test]
    fn test_runtime_initialise_and_ops() {
        let mut rt = GpuRuntime::new();
        rt.initialize().unwrap();
        assert!(rt.device().is_some());

        let sum = rt.vector_add(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
        assert_eq!(sum, vec![4.0, 6.0]);

        let scaled = rt.vector_scale(&[2.0, 4.0], 0.5).unwrap();
        assert_eq!(scaled, vec![1.0, 2.0]);

        let total = rt.reduce_sum(&[1.0, 2.0, 3.0]).unwrap();
        assert!((total - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_runtime_matrix_multiply() {
        let mut rt = GpuRuntime::new();
        rt.initialize().unwrap();
        // identity multiply: I × v
        let a = vec![1.0, 0.0, 0.0, 1.0]; // 2×2 identity
        let b = vec![5.0, 6.0]; // 2×1
        let c = rt.matrix_multiply(&a, &b, 2, 2, 1).unwrap();
        assert_eq!(c, vec![5.0, 6.0]);
    }

    // -- buffer usage flags -------------------------------------------------

    #[test]
    fn test_buffer_usage_bitor() {
        let usage = BufferUsage::STORAGE | BufferUsage::MAP_READ;
        assert!(usage.contains(BufferUsage::STORAGE));
        assert!(usage.contains(BufferUsage::MAP_READ));
        assert!(!usage.contains(BufferUsage::UNIFORM));
    }

    // -- stats tracking -----------------------------------------------------

    #[test]
    fn test_stats_tracking() {
        let mut dev = simulated_device();
        let id1 = dev.create_buffer(64, BufferUsage::STORAGE).unwrap();
        let _id2 = dev.create_buffer(64, BufferUsage::STORAGE).unwrap();
        dev.destroy_buffer(id1).unwrap();
        assert_eq!(dev.stats.buffers_created, 2);
        assert_eq!(dev.stats.buffers_destroyed, 1);
    }
}
