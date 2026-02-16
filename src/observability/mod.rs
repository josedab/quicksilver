//! Built-in Observability: Tracing, Metrics, and Profiling
//!
//! Provides OpenTelemetry-compatible tracing and metrics out of the box,
//! with zero-cost abstractions when disabled. Every async operation
//! automatically gets trace context.
//!
//! # Example
//! ```text
//! const tracer = Telemetry.tracer("my-service");
//!
//! async function handleRequest(req) {
//!   return tracer.span("handleRequest", async (span) => {
//!     span.setAttribute("user.id", req.userId);
//!     const result = await processRequest(req);
//!     span.setStatus("ok");
//!     return result;
//!   });
//! }
//! ```

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicUsize, Ordering}};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Unique identifier for traces
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(pub u128);

/// Unique identifier for spans within a trace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId(pub u64);

impl TraceId {
    pub fn new() -> Self {
        // In production, use a proper random generator
        Self(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos())
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl SpanId {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

/// Span status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStatus {
    Unset,
    Ok,
    Error,
}

/// Span kind (client, server, producer, consumer, internal)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
}

/// A span representing a unit of work
#[derive(Debug, Clone)]
pub struct Span {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    pub name: String,
    pub kind: SpanKind,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub status: SpanStatus,
    pub attributes: HashMap<String, AttributeValue>,
    pub events: Vec<SpanEvent>,
}

/// Attribute value types
#[derive(Debug, Clone)]
pub enum AttributeValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    StringArray(Vec<String>),
    IntArray(Vec<i64>),
}

/// An event within a span
#[derive(Debug, Clone)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: Instant,
    pub attributes: HashMap<String, AttributeValue>,
}

impl Span {
    pub fn new(name: &str, trace_id: TraceId, parent_span_id: Option<SpanId>) -> Self {
        Self {
            trace_id,
            span_id: SpanId::new(),
            parent_span_id,
            name: name.to_string(),
            kind: SpanKind::Internal,
            start_time: Instant::now(),
            end_time: None,
            status: SpanStatus::Unset,
            attributes: HashMap::default(),
            events: Vec::new(),
        }
    }

    pub fn set_attribute(&mut self, key: &str, value: AttributeValue) {
        self.attributes.insert(key.to_string(), value);
    }

    pub fn set_string_attribute(&mut self, key: &str, value: &str) {
        self.set_attribute(key, AttributeValue::String(value.to_string()));
    }

    pub fn set_int_attribute(&mut self, key: &str, value: i64) {
        self.set_attribute(key, AttributeValue::Int(value));
    }

    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    pub fn add_event(&mut self, name: &str) {
        self.events.push(SpanEvent {
            name: name.to_string(),
            timestamp: Instant::now(),
            attributes: HashMap::default(),
        });
    }

    pub fn end(&mut self) {
        self.end_time = Some(Instant::now());
    }

    pub fn duration(&self) -> Duration {
        self.end_time.unwrap_or_else(Instant::now).duration_since(self.start_time)
    }
}

/// Tracer for creating spans
#[derive(Debug, Clone)]
pub struct Tracer {
    name: String,
    collector: Arc<SpanCollector>,
}

impl Tracer {
    pub fn new(name: &str, collector: Arc<SpanCollector>) -> Self {
        Self {
            name: name.to_string(),
            collector,
        }
    }

    /// Start a new trace
    pub fn start_trace(&self, name: &str) -> SpanGuard {
        let trace_id = TraceId::new();
        let span = Span::new(name, trace_id, None);
        SpanGuard {
            span,
            collector: Arc::clone(&self.collector),
        }
    }

    /// Start a span with parent context
    pub fn start_span(&self, name: &str, trace_id: TraceId, parent_span_id: SpanId) -> SpanGuard {
        let span = Span::new(name, trace_id, Some(parent_span_id));
        SpanGuard {
            span,
            collector: Arc::clone(&self.collector),
        }
    }

    /// Get the tracer name
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// RAII guard for spans - automatically ends and exports on drop
pub struct SpanGuard {
    span: Span,
    collector: Arc<SpanCollector>,
}

impl SpanGuard {
    pub fn set_attribute(&mut self, key: &str, value: AttributeValue) {
        self.span.set_attribute(key, value);
    }

    pub fn set_string_attribute(&mut self, key: &str, value: &str) {
        self.span.set_string_attribute(key, value);
    }

    pub fn set_int_attribute(&mut self, key: &str, value: i64) {
        self.span.set_int_attribute(key, value);
    }

    pub fn set_status(&mut self, status: SpanStatus) {
        self.span.set_status(status);
    }

    pub fn add_event(&mut self, name: &str) {
        self.span.add_event(name);
    }

    pub fn trace_id(&self) -> TraceId {
        self.span.trace_id
    }

    pub fn span_id(&self) -> SpanId {
        self.span.span_id
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        self.span.end();
        self.collector.collect(self.span.clone());
    }
}

/// Collects and exports spans
#[derive(Debug)]
pub struct SpanCollector {
    spans: Mutex<Vec<Span>>,
    exporters: Mutex<Vec<Box<dyn SpanExporter>>>,
}

impl SpanCollector {
    pub fn new() -> Self {
        Self {
            spans: Mutex::new(Vec::new()),
            exporters: Mutex::new(Vec::new()),
        }
    }

    pub fn collect(&self, span: Span) {
        let exporters = self.exporters.lock().unwrap();
        for exporter in exporters.iter() {
            exporter.export(&span);
        }

        self.spans.lock().unwrap().push(span);
    }

    pub fn add_exporter(&self, exporter: Box<dyn SpanExporter>) {
        self.exporters.lock().unwrap().push(exporter);
    }

    pub fn spans(&self) -> Vec<Span> {
        self.spans.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.spans.lock().unwrap().clear();
    }
}

impl Default for SpanCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for exporting spans
pub trait SpanExporter: Send + Sync + std::fmt::Debug {
    fn export(&self, span: &Span);
}

/// Console exporter for development
#[derive(Debug)]
pub struct ConsoleExporter;

impl SpanExporter for ConsoleExporter {
    fn export(&self, span: &Span) {
        println!(
            "[TRACE] {} | {} | {:?} | {:?}",
            span.name,
            span.duration().as_micros(),
            span.status,
            span.attributes
        );
    }
}

/// Metrics system
#[derive(Debug)]
pub struct MetricsRegistry {
    counters: Mutex<HashMap<String, Arc<Counter>>>,
    gauges: Mutex<HashMap<String, Arc<Gauge>>>,
    histograms: Mutex<HashMap<String, Arc<Histogram>>>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            counters: Mutex::new(HashMap::default()),
            gauges: Mutex::new(HashMap::default()),
            histograms: Mutex::new(HashMap::default()),
        }
    }

    pub fn counter(&self, name: &str) -> Arc<Counter> {
        let mut counters = self.counters.lock().unwrap();
        counters
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Counter::new(name)))
            .clone()
    }

    pub fn gauge(&self, name: &str) -> Arc<Gauge> {
        let mut gauges = self.gauges.lock().unwrap();
        gauges
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Gauge::new(name)))
            .clone()
    }

    pub fn histogram(&self, name: &str) -> Arc<Histogram> {
        let mut histograms = self.histograms.lock().unwrap();
        histograms
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Histogram::new(name)))
            .clone()
    }

    /// Export all metrics as JSON
    pub fn export_json(&self) -> String {
        let mut result = String::from("{\n");

        // Counters
        result.push_str("  \"counters\": {\n");
        let counters = self.counters.lock().unwrap();
        for (i, (name, counter)) in counters.iter().enumerate() {
            result.push_str(&format!("    \"{}\": {}", name, counter.get()));
            if i < counters.len() - 1 {
                result.push(',');
            }
            result.push('\n');
        }
        result.push_str("  },\n");

        // Gauges
        result.push_str("  \"gauges\": {\n");
        let gauges = self.gauges.lock().unwrap();
        for (i, (name, gauge)) in gauges.iter().enumerate() {
            result.push_str(&format!("    \"{}\": {}", name, gauge.get()));
            if i < gauges.len() - 1 {
                result.push(',');
            }
            result.push('\n');
        }
        result.push_str("  },\n");

        // Histograms
        result.push_str("  \"histograms\": {\n");
        let histograms = self.histograms.lock().unwrap();
        for (i, (name, histogram)) in histograms.iter().enumerate() {
            let stats = histogram.stats();
            result.push_str(&format!(
                "    \"{}\": {{\"count\": {}, \"sum\": {}, \"min\": {}, \"max\": {}, \"avg\": {}}}",
                name, stats.count, stats.sum, stats.min, stats.max, stats.avg
            ));
            if i < histograms.len() - 1 {
                result.push(',');
            }
            result.push('\n');
        }
        result.push_str("  }\n");

        result.push('}');
        result
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A counter that can only go up
#[derive(Debug)]
pub struct Counter {
    name: String,
    value: AtomicU64,
}

impl Counter {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            value: AtomicU64::new(0),
        }
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::SeqCst);
    }

    pub fn add(&self, n: u64) {
        self.value.fetch_add(n, Ordering::SeqCst);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::SeqCst)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A gauge that can go up or down
#[derive(Debug)]
pub struct Gauge {
    name: String,
    value: AtomicU64,
}

impl Gauge {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            value: AtomicU64::new(0),
        }
    }

    pub fn set(&self, value: f64) {
        self.value.store(value.to_bits(), Ordering::SeqCst);
    }

    pub fn get(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::SeqCst))
    }

    pub fn inc(&self) {
        let current = self.get();
        self.set(current + 1.0);
    }

    pub fn dec(&self) {
        let current = self.get();
        self.set(current - 1.0);
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A histogram for measuring distributions
#[derive(Debug)]
pub struct Histogram {
    name: String,
    values: Mutex<Vec<f64>>,
    count: AtomicUsize,
    sum: Mutex<f64>,
}

impl Histogram {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            values: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
            sum: Mutex::new(0.0),
        }
    }

    pub fn observe(&self, value: f64) {
        self.values.lock().unwrap().push(value);
        self.count.fetch_add(1, Ordering::SeqCst);
        *self.sum.lock().unwrap() += value;
    }

    pub fn stats(&self) -> HistogramStats {
        let values = self.values.lock().unwrap();
        let count = values.len();
        let sum = *self.sum.lock().unwrap();

        if count == 0 {
            return HistogramStats {
                count: 0,
                sum: 0.0,
                min: 0.0,
                max: 0.0,
                avg: 0.0,
            };
        }

        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let avg = sum / count as f64;

        HistogramStats {
            count,
            sum,
            min,
            max,
            avg,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone)]
pub struct HistogramStats {
    pub count: usize,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

/// Profiler for measuring function execution times
#[derive(Debug)]
pub struct Profiler {
    timings: Mutex<HashMap<String, Vec<Duration>>>,
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            timings: Mutex::new(HashMap::default()),
        }
    }

    /// Start timing a section
    pub fn start(&self, _name: &str) -> ProfilerGuard {
        ProfilerGuard {
            start: Instant::now(),
        }
    }

    /// Record a timing
    pub fn record(&self, name: &str, duration: Duration) {
        self.timings
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .push(duration);
    }

    /// Get timing statistics
    pub fn stats(&self, name: &str) -> Option<ProfileStats> {
        let timings = self.timings.lock().unwrap();
        let durations = timings.get(name)?;

        if durations.is_empty() {
            return None;
        }

        let total: Duration = durations.iter().sum();
        let count = durations.len();
        let min = *durations.iter().min()?;
        let max = *durations.iter().max()?;
        let avg = total / count as u32;

        Some(ProfileStats {
            count,
            total,
            min,
            max,
            avg,
        })
    }

    /// Clear all timings
    pub fn clear(&self) {
        self.timings.lock().unwrap().clear();
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProfilerGuard {
    start: Instant,
}

impl ProfilerGuard {
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

#[derive(Debug, Clone)]
pub struct ProfileStats {
    pub count: usize,
    pub total: Duration,
    pub min: Duration,
    pub max: Duration,
    pub avg: Duration,
}

// ===== OTLP Exporter =====

/// Serializable representation of a span for export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportableSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub status: String,
    pub attributes: Vec<(String, String)>,
    pub duration_us: u64,
}

impl ExportableSpan {
    pub fn from_span(span: &Span) -> Self {
        let duration = span.duration();
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let duration_nanos = duration.as_nanos() as u64;

        ExportableSpan {
            trace_id: format!("{:032x}", span.trace_id.0),
            span_id: format!("{:016x}", span.span_id.0),
            parent_span_id: span.parent_span_id.map(|id| format!("{:016x}", id.0)),
            name: span.name.clone(),
            kind: match span.kind {
                SpanKind::Internal => "SPAN_KIND_INTERNAL",
                SpanKind::Server => "SPAN_KIND_SERVER",
                SpanKind::Client => "SPAN_KIND_CLIENT",
                SpanKind::Producer => "SPAN_KIND_PRODUCER",
                SpanKind::Consumer => "SPAN_KIND_CONSUMER",
            }
            .to_string(),
            start_time_unix_nano: now_nanos.saturating_sub(duration_nanos),
            end_time_unix_nano: now_nanos,
            status: match span.status {
                SpanStatus::Unset => "STATUS_CODE_UNSET",
                SpanStatus::Ok => "STATUS_CODE_OK",
                SpanStatus::Error => "STATUS_CODE_ERROR",
            }
            .to_string(),
            attributes: span
                .attributes
                .iter()
                .map(|(k, v)| (k.clone(), format!("{:?}", v)))
                .collect(),
            duration_us: duration.as_micros() as u64,
        }
    }
}

/// OpenTelemetry Protocol (OTLP) exporter
/// Serializes spans to OTLP JSON format for export to collectors
pub struct OtlpExporter {
    endpoint: String,
    service_name: String,
    headers: HashMap<String, String>,
    batch_size: usize,
    buffered_spans: Mutex<Vec<ExportableSpan>>,
}

impl std::fmt::Debug for OtlpExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtlpExporter")
            .field("endpoint", &self.endpoint)
            .field("service_name", &self.service_name)
            .field("batch_size", &self.batch_size)
            .finish()
    }
}

impl OtlpExporter {
    pub fn new(endpoint: &str, service_name: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            service_name: service_name.to_string(),
            headers: HashMap::default(),
            batch_size: 100,
            buffered_spans: Mutex::new(Vec::new()),
        }
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Serialize buffered spans to OTLP JSON format
    pub fn flush(&mut self) -> String {
        let spans = self.buffered_spans.get_mut().unwrap();
        let drained: Vec<ExportableSpan> = std::mem::take(spans);

        let span_objects: Vec<serde_json::Value> = drained
            .iter()
            .map(|s| {
                let attrs: Vec<serde_json::Value> = s
                    .attributes
                    .iter()
                    .map(|(k, v)| {
                        serde_json::json!({
                            "key": k,
                            "value": { "stringValue": v }
                        })
                    })
                    .collect();

                let mut span_obj = serde_json::json!({
                    "traceId": s.trace_id,
                    "spanId": s.span_id,
                    "name": s.name,
                    "kind": s.kind,
                    "startTimeUnixNano": s.start_time_unix_nano.to_string(),
                    "endTimeUnixNano": s.end_time_unix_nano.to_string(),
                    "status": { "code": s.status },
                    "attributes": attrs
                });

                if let Some(ref parent) = s.parent_span_id {
                    span_obj.as_object_mut().unwrap().insert(
                        "parentSpanId".to_string(),
                        serde_json::Value::String(parent.clone()),
                    );
                }

                span_obj
            })
            .collect();

        let otlp = serde_json::json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": { "stringValue": self.service_name }
                    }]
                },
                "scopeSpans": [{
                    "spans": span_objects
                }]
            }]
        });

        serde_json::to_string_pretty(&otlp).unwrap_or_default()
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}

impl SpanExporter for OtlpExporter {
    fn export(&self, span: &Span) {
        let exportable = ExportableSpan::from_span(span);
        self.buffered_spans.lock().unwrap().push(exportable);
    }
}

// ===== Auto-Instrumentation =====

/// Category of instrumentation event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrumentationCategory {
    GarbageCollection,
    Compilation,
    IO,
    Promise,
    Timer,
    ModuleLoad,
    FunctionCall,
}

impl std::fmt::Display for InstrumentationCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GarbageCollection => write!(f, "gc"),
            Self::Compilation => write!(f, "compilation"),
            Self::IO => write!(f, "io"),
            Self::Promise => write!(f, "promise"),
            Self::Timer => write!(f, "timer"),
            Self::ModuleLoad => write!(f, "module_load"),
            Self::FunctionCall => write!(f, "function_call"),
        }
    }
}

/// An instrumentation event recorded by the runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentationEvent {
    pub category: InstrumentationCategory,
    pub name: String,
    pub duration_us: u64,
    pub timestamp: u64,
    pub attributes: std::collections::HashMap<String, String>,
}

/// Summary of instrumentation events
#[derive(Debug, Clone)]
pub struct InstrumentationSummary {
    pub total_events: usize,
    pub total_duration_us: u64,
    pub events_by_category: std::collections::HashMap<String, usize>,
}

/// Runtime auto-instrumentation configuration
#[derive(Debug, Clone)]
pub struct AutoInstrumentation {
    pub instrument_gc: bool,
    pub instrument_compilation: bool,
    pub instrument_io: bool,
    pub instrument_promises: bool,
    pub instrument_timers: bool,
    enabled: bool,
    events: Vec<InstrumentationEvent>,
}

impl AutoInstrumentation {
    pub fn new() -> Self {
        Self {
            instrument_gc: true,
            instrument_compilation: true,
            instrument_io: true,
            instrument_promises: true,
            instrument_timers: true,
            enabled: true,
            events: Vec::new(),
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn record_event(&mut self, event: InstrumentationEvent) {
        if !self.enabled {
            return;
        }
        let category_enabled = match event.category {
            InstrumentationCategory::GarbageCollection => self.instrument_gc,
            InstrumentationCategory::Compilation => self.instrument_compilation,
            InstrumentationCategory::IO => self.instrument_io,
            InstrumentationCategory::Promise => self.instrument_promises,
            InstrumentationCategory::Timer => self.instrument_timers,
            InstrumentationCategory::ModuleLoad | InstrumentationCategory::FunctionCall => true,
        };
        if category_enabled {
            self.events.push(event);
        }
    }

    pub fn events(&self) -> &[InstrumentationEvent] {
        &self.events
    }

    pub fn events_by_category(&self, cat: InstrumentationCategory) -> Vec<&InstrumentationEvent> {
        self.events.iter().filter(|e| e.category == cat).collect()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn summary(&self) -> InstrumentationSummary {
        let mut events_by_category = std::collections::HashMap::new();
        let mut total_duration_us = 0u64;

        for event in &self.events {
            let key = event.category.to_string();
            *events_by_category.entry(key).or_insert(0usize) += 1;
            total_duration_us += event.duration_us;
        }

        InstrumentationSummary {
            total_events: self.events.len(),
            total_duration_us,
            events_by_category,
        }
    }
}

impl Default for AutoInstrumentation {
    fn default() -> Self {
        Self::new()
    }
}

// ===== Async Context Propagation =====

/// Trace context for propagation across async boundaries (W3C Trace Context)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub trace_flags: u8,
    pub trace_state: Vec<(String, String)>,
}

impl TraceContext {
    pub fn new(trace_id: &str, span_id: &str) -> Self {
        Self {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            parent_span_id: None,
            trace_flags: 0x01,
            trace_state: Vec::new(),
        }
    }

    /// Format as W3C traceparent header: `00-{trace_id}-{span_id}-{flags}`
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.trace_flags
        )
    }

    /// Parse W3C traceparent header
    pub fn from_traceparent(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }
        let trace_flags = u8::from_str_radix(parts[3], 16).ok()?;

        Some(Self {
            trace_id: parts[1].to_string(),
            span_id: parts[2].to_string(),
            parent_span_id: None,
            trace_flags,
            trace_state: Vec::new(),
        })
    }

    /// Format as W3C tracestate header
    pub fn to_tracestate(&self) -> String {
        self.trace_state
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Create a child context with a new span_id
    pub fn child_context(&self) -> Self {
        let new_span_id = SpanId::new();
        Self {
            trace_id: self.trace_id.clone(),
            span_id: format!("{:016x}", new_span_id.0),
            parent_span_id: Some(self.span_id.clone()),
            trace_flags: self.trace_flags,
            trace_state: self.trace_state.clone(),
        }
    }
}

// ===== Runtime Metrics Collector =====

/// Snapshot of all runtime metrics at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub gc_collections: u64,
    pub heap_size_bytes: f64,
    pub promises_created: u64,
    pub promises_settled: u64,
    pub active_timers: f64,
    pub bytecode_compiled_bytes: u64,
    pub total_instructions_executed: u64,
}

/// Collects runtime-level metrics automatically
pub struct RuntimeMetrics {
    pub gc_collections: Counter,
    pub gc_duration_us: Histogram,
    pub heap_size_bytes: Gauge,
    pub event_loop_lag_us: Histogram,
    pub promises_created: Counter,
    pub promises_settled: Counter,
    pub active_timers: Gauge,
    pub bytecode_compiled_bytes: Counter,
    pub total_instructions_executed: Counter,
}

impl RuntimeMetrics {
    pub fn new() -> Self {
        Self {
            gc_collections: Counter::new("gc_collections_total"),
            gc_duration_us: Histogram::new("gc_duration_microseconds"),
            heap_size_bytes: Gauge::new("heap_size_bytes"),
            event_loop_lag_us: Histogram::new("event_loop_lag_microseconds"),
            promises_created: Counter::new("promises_created_total"),
            promises_settled: Counter::new("promises_settled_total"),
            active_timers: Gauge::new("active_timers"),
            bytecode_compiled_bytes: Counter::new("bytecode_compiled_bytes_total"),
            total_instructions_executed: Counter::new("instructions_executed_total"),
        }
    }

    pub fn record_gc(&self, duration_us: u64) {
        self.gc_collections.inc();
        self.gc_duration_us.observe(duration_us as f64);
    }

    pub fn record_event_loop_tick(&self, lag_us: u64) {
        self.event_loop_lag_us.observe(lag_us as f64);
    }

    pub fn update_heap_size(&self, bytes: u64) {
        self.heap_size_bytes.set(bytes as f64);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            gc_collections: self.gc_collections.get(),
            heap_size_bytes: self.heap_size_bytes.get(),
            promises_created: self.promises_created.get(),
            promises_settled: self.promises_settled.get(),
            active_timers: self.active_timers.get(),
            bytecode_compiled_bytes: self.bytecode_compiled_bytes.get(),
            total_instructions_executed: self.total_instructions_executed.get(),
        }
    }

    /// Export metrics in Prometheus text exposition format
    pub fn to_prometheus_text(&self) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "# HELP gc_collections_total Total number of garbage collections\n\
             # TYPE gc_collections_total counter\n\
             gc_collections_total {}\n\n",
            self.gc_collections.get()
        ));

        output.push_str(&format!(
            "# HELP promises_created_total Total promises created\n\
             # TYPE promises_created_total counter\n\
             promises_created_total {}\n\n",
            self.promises_created.get()
        ));

        output.push_str(&format!(
            "# HELP promises_settled_total Total promises settled\n\
             # TYPE promises_settled_total counter\n\
             promises_settled_total {}\n\n",
            self.promises_settled.get()
        ));

        output.push_str(&format!(
            "# HELP bytecode_compiled_bytes_total Total bytecode compiled\n\
             # TYPE bytecode_compiled_bytes_total counter\n\
             bytecode_compiled_bytes_total {}\n\n",
            self.bytecode_compiled_bytes.get()
        ));

        output.push_str(&format!(
            "# HELP instructions_executed_total Total instructions executed\n\
             # TYPE instructions_executed_total counter\n\
             instructions_executed_total {}\n\n",
            self.total_instructions_executed.get()
        ));

        output.push_str(&format!(
            "# HELP heap_size_bytes Current heap size in bytes\n\
             # TYPE heap_size_bytes gauge\n\
             heap_size_bytes {}\n\n",
            self.heap_size_bytes.get()
        ));

        output.push_str(&format!(
            "# HELP active_timers Number of active timers\n\
             # TYPE active_timers gauge\n\
             active_timers {}\n\n",
            self.active_timers.get()
        ));

        let gc_stats = self.gc_duration_us.stats();
        output.push_str(&format!(
            "# HELP gc_duration_microseconds GC pause duration\n\
             # TYPE gc_duration_microseconds histogram\n\
             gc_duration_microseconds_count {}\n\
             gc_duration_microseconds_sum {}\n\n",
            gc_stats.count, gc_stats.sum
        ));

        let lag_stats = self.event_loop_lag_us.stats();
        output.push_str(&format!(
            "# HELP event_loop_lag_microseconds Event loop lag\n\
             # TYPE event_loop_lag_microseconds histogram\n\
             event_loop_lag_microseconds_count {}\n\
             event_loop_lag_microseconds_sum {}\n",
            lag_stats.count, lag_stats.sum
        ));

        output
    }
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RuntimeMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeMetrics")
            .field("gc_collections", &self.gc_collections.get())
            .field("heap_size_bytes", &self.heap_size_bytes.get())
            .field("promises_created", &self.promises_created.get())
            .field("promises_settled", &self.promises_settled.get())
            .field("active_timers", &self.active_timers.get())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_creation() {
        let collector = Arc::new(SpanCollector::new());
        let tracer = Tracer::new("test", collector.clone());

        {
            let mut span = tracer.start_trace("test-operation");
            span.set_string_attribute("key", "value");
            span.set_status(SpanStatus::Ok);
        }

        let spans = collector.spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "test-operation");
        assert_eq!(spans[0].status, SpanStatus::Ok);
    }

    #[test]
    fn test_counter() {
        let counter = Counter::new("requests");
        counter.inc();
        counter.inc();
        counter.add(3);
        assert_eq!(counter.get(), 5);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new("temperature");
        gauge.set(20.5);
        assert_eq!(gauge.get(), 20.5);
        gauge.inc();
        assert_eq!(gauge.get(), 21.5);
    }

    #[test]
    fn test_histogram() {
        let histogram = Histogram::new("latency");
        histogram.observe(10.0);
        histogram.observe(20.0);
        histogram.observe(30.0);

        let stats = histogram.stats();
        assert_eq!(stats.count, 3);
        assert_eq!(stats.sum, 60.0);
        assert_eq!(stats.min, 10.0);
        assert_eq!(stats.max, 30.0);
        assert_eq!(stats.avg, 20.0);
    }

    #[test]
    fn test_otlp_exporter_creation_and_flush() {
        let mut exporter = OtlpExporter::new("http://localhost:4318", "test-service")
            .with_header("Authorization", "Bearer token123")
            .with_batch_size(50);

        assert_eq!(exporter.endpoint(), "http://localhost:4318");
        assert_eq!(exporter.service_name(), "test-service");

        let trace_id = TraceId::new();
        let mut span = Span::new("test-op", trace_id, None);
        span.set_string_attribute("http.method", "GET");
        span.set_status(SpanStatus::Ok);
        span.end();

        exporter.export(&span);

        let json = exporter.flush();
        assert!(json.contains("resourceSpans"));
        assert!(json.contains("test-service"));
        assert!(json.contains("test-op"));
        assert!(json.contains("STATUS_CODE_OK"));

        // After flush, buffer should be empty
        let json2 = exporter.flush();
        let parsed: serde_json::Value = serde_json::from_str(&json2).unwrap();
        let spans = &parsed["resourceSpans"][0]["scopeSpans"][0]["spans"];
        assert!(spans.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_auto_instrumentation_record_and_filter() {
        let mut instr = AutoInstrumentation::new();

        instr.record_event(InstrumentationEvent {
            category: InstrumentationCategory::GarbageCollection,
            name: "minor-gc".to_string(),
            duration_us: 500,
            timestamp: 1000,
            attributes: std::collections::HashMap::new(),
        });

        instr.record_event(InstrumentationEvent {
            category: InstrumentationCategory::Compilation,
            name: "compile-main".to_string(),
            duration_us: 2000,
            timestamp: 2000,
            attributes: std::collections::HashMap::new(),
        });

        instr.record_event(InstrumentationEvent {
            category: InstrumentationCategory::GarbageCollection,
            name: "major-gc".to_string(),
            duration_us: 1500,
            timestamp: 3000,
            attributes: std::collections::HashMap::new(),
        });

        assert_eq!(instr.events().len(), 3);

        let gc_events = instr.events_by_category(InstrumentationCategory::GarbageCollection);
        assert_eq!(gc_events.len(), 2);

        let comp_events = instr.events_by_category(InstrumentationCategory::Compilation);
        assert_eq!(comp_events.len(), 1);

        // Disabled instrumentation should not record
        instr.disable();
        instr.record_event(InstrumentationEvent {
            category: InstrumentationCategory::IO,
            name: "read-file".to_string(),
            duration_us: 100,
            timestamp: 4000,
            attributes: std::collections::HashMap::new(),
        });
        assert_eq!(instr.events().len(), 3);

        instr.clear();
        assert_eq!(instr.events().len(), 0);
    }

    #[test]
    fn test_trace_context_serialization() {
        let ctx = TraceContext::new(
            "0af7651916cd43dd8448eb211c80319c",
            "00f067aa0ba902b7",
        );

        let traceparent = ctx.to_traceparent();
        assert_eq!(
            traceparent,
            "00-0af7651916cd43dd8448eb211c80319c-00f067aa0ba902b7-01"
        );

        // Round-trip
        let parsed = TraceContext::from_traceparent(&traceparent).unwrap();
        assert_eq!(parsed.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(parsed.span_id, "00f067aa0ba902b7");
        assert_eq!(parsed.trace_flags, 0x01);

        // Invalid headers
        assert!(TraceContext::from_traceparent("invalid").is_none());
        assert!(TraceContext::from_traceparent("01-abc-def-00").is_none());
    }

    #[test]
    fn test_trace_context_parent_child() {
        let parent = TraceContext::new(
            "0af7651916cd43dd8448eb211c80319c",
            "00f067aa0ba902b7",
        );

        let child = parent.child_context();
        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.span_id, parent.span_id);
        assert_eq!(child.parent_span_id, Some(parent.span_id.clone()));
        assert_eq!(child.trace_flags, parent.trace_flags);
    }

    #[test]
    fn test_trace_context_tracestate() {
        let mut ctx = TraceContext::new("abc", "def");
        ctx.trace_state = vec![
            ("vendor1".to_string(), "value1".to_string()),
            ("vendor2".to_string(), "value2".to_string()),
        ];

        let tracestate = ctx.to_tracestate();
        assert_eq!(tracestate, "vendor1=value1,vendor2=value2");
    }

    #[test]
    fn test_runtime_metrics_recording_and_snapshot() {
        let metrics = RuntimeMetrics::new();

        metrics.record_gc(500);
        metrics.record_gc(300);
        metrics.update_heap_size(1024 * 1024);
        metrics.promises_created.inc();
        metrics.promises_created.inc();
        metrics.promises_settled.inc();
        metrics.active_timers.set(3.0);
        metrics.bytecode_compiled_bytes.add(4096);
        metrics.total_instructions_executed.add(10000);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.gc_collections, 2);
        assert_eq!(snapshot.heap_size_bytes, 1048576.0);
        assert_eq!(snapshot.promises_created, 2);
        assert_eq!(snapshot.promises_settled, 1);
        assert_eq!(snapshot.active_timers, 3.0);
        assert_eq!(snapshot.bytecode_compiled_bytes, 4096);
        assert_eq!(snapshot.total_instructions_executed, 10000);
    }

    #[test]
    fn test_prometheus_text_format() {
        let metrics = RuntimeMetrics::new();
        metrics.record_gc(500);
        metrics.update_heap_size(2048);
        metrics.promises_created.add(10);

        let text = metrics.to_prometheus_text();
        assert!(text.contains("# TYPE gc_collections_total counter"));
        assert!(text.contains("gc_collections_total 1"));
        assert!(text.contains("# TYPE heap_size_bytes gauge"));
        assert!(text.contains("heap_size_bytes 2048"));
        assert!(text.contains("promises_created_total 10"));
        assert!(text.contains("gc_duration_microseconds_count 1"));
        assert!(text.contains("gc_duration_microseconds_sum 500"));
    }

    #[test]
    fn test_instrumentation_summary() {
        let mut instr = AutoInstrumentation::new();

        for i in 0..5u64 {
            instr.record_event(InstrumentationEvent {
                category: InstrumentationCategory::GarbageCollection,
                name: format!("gc-{}", i),
                duration_us: 100 * (i + 1),
                timestamp: i * 1000,
                attributes: std::collections::HashMap::new(),
            });
        }

        for i in 0..3u64 {
            instr.record_event(InstrumentationEvent {
                category: InstrumentationCategory::Compilation,
                name: format!("compile-{}", i),
                duration_us: 500,
                timestamp: 10000 + i * 1000,
                attributes: std::collections::HashMap::new(),
            });
        }

        let summary = instr.summary();
        assert_eq!(summary.total_events, 8);
        // GC: 100+200+300+400+500=1500, Compilation: 500*3=1500
        assert_eq!(summary.total_duration_us, 3000);
        assert_eq!(summary.events_by_category.get("gc"), Some(&5));
        assert_eq!(summary.events_by_category.get("compilation"), Some(&3));
    }
}
