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
}
