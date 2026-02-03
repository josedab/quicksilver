//! HTTP Server for Time-Travel Debugger Web UI
//!
//! This module provides a lightweight HTTP server that exposes the debugger
//! functionality via a REST API. The server is single-threaded to maintain
//! compatibility with the non-Send TimeTravelDebugger (which contains Value
//! types that use Rc<RefCell>).

use super::TimeTravelDebugger;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::io::{Read, Write, BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::time::Duration;

/// Default port for the debugger server
pub const DEFAULT_PORT: u16 = 9229;

/// API response format
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

/// Debugger state for API
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebuggerState {
    pub current_step: u64,
    pub total_steps: u64,
    pub current_line: u32,
    pub current_position: usize,
    pub history_size: usize,
    pub is_paused: bool,
    pub filename: String,
}

/// Execution record for API
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordResponse {
    pub step: u64,
    pub line: u32,
    pub description: String,
    pub stack_size: usize,
    pub locals: Vec<VariableInfo>,
}

/// Variable info for API
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableInfo {
    pub name: String,
    pub value: String,
    pub var_type: String,
}

/// History entry for API
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub step: u64,
    pub line: u32,
    pub description: String,
    pub is_current: bool,
}

/// Variable change entry for API
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    pub step: u64,
    pub old_value: Option<String>,
    pub new_value: String,
}

/// Breakpoint info for API
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointInfo {
    pub line: u32,
    pub enabled: bool,
    pub hit_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// Source file info for API
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub filename: String,
    pub lines: Vec<String>,
    pub total_lines: usize,
}

/// HTTP Debugger Server (single-threaded)
///
/// This server handles connections sequentially rather than spawning threads,
/// because TimeTravelDebugger contains Value types that use Rc<RefCell> and
/// cannot be sent between threads.
pub struct DebugServer {
    debugger: Rc<RefCell<TimeTravelDebugger>>,
    port: u16,
    running: Rc<RefCell<bool>>,
}

impl DebugServer {
    /// Create a new debug server
    pub fn new(debugger: TimeTravelDebugger, port: u16) -> Self {
        Self {
            debugger: Rc::new(RefCell::new(debugger)),
            port,
            running: Rc::new(RefCell::new(false)),
        }
    }

    /// Get the server URL
    pub fn url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    /// Start the server (blocking, single-threaded)
    pub fn start(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", self.port))?;

        // Set non-blocking mode with short timeout for graceful shutdown
        listener.set_nonblocking(true)?;

        println!("╔═══════════════════════════════════════════════════════════════╗");
        println!("║  🕐 Quicksilver Time-Travel Debugger Server                   ║");
        println!("╠═══════════════════════════════════════════════════════════════╣");
        println!("║  Server running at: {}                       ║", self.url());
        println!("║  API Endpoints:                                               ║");
        println!("║    GET  /api/state       - Get debugger state                 ║");
        println!("║    GET  /api/current     - Get current record                 ║");
        println!("║    GET  /api/history     - Get execution history              ║");
        println!("║    POST /api/step/next   - Step forward                       ║");
        println!("║    POST /api/step/back   - Step backward                      ║");
        println!("║    POST /api/goto/:step  - Jump to step                       ║");
        println!("║    GET  /api/changes/:var - Get variable changes              ║");
        println!("║    GET  /api/source      - Get source code                    ║");
        println!("║    GET  /api/breakpoints - Get breakpoints                    ║");
        println!("║    POST /api/breakpoints - Add breakpoint                     ║");
        println!("║    GET  /                - Web UI                             ║");
        println!("╚═══════════════════════════════════════════════════════════════╝");

        *self.running.borrow_mut() = true;

        loop {
            if !*self.running.borrow() {
                break;
            }

            match listener.accept() {
                Ok((stream, _)) => {
                    // Handle connection synchronously (single-threaded)
                    if let Err(e) = handle_connection(stream, &self.debugger) {
                        eprintln!("Connection error: {}", e);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection ready, sleep briefly and continue
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Stop the server
    pub fn stop(&self) {
        *self.running.borrow_mut() = false;
    }

    /// Get the debugger reference
    pub fn debugger(&self) -> Rc<RefCell<TimeTravelDebugger>> {
        Rc::clone(&self.debugger)
    }
}

/// Handle a single HTTP connection
fn handle_connection(
    mut stream: TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return send_error(&mut stream, 400, "Bad Request");
    }

    let method = parts[0];
    let path = parts[1];

    // Read headers
    let mut content_length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header.trim().is_empty() {
            break;
        }
        if header.to_lowercase().starts_with("content-length:") {
            content_length = header.split(':').nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
        }
    }

    // Read body if present
    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf)?;
        body = String::from_utf8_lossy(&buf).to_string();
    }

    // Route the request
    match (method, path) {
        ("OPTIONS", _) => send_cors_preflight(&mut stream),
        ("GET", "/") => send_html(&mut stream, &get_web_ui()),
        ("GET", "/api/state") => handle_get_state(&mut stream, debugger),
        ("GET", "/api/current") => handle_get_current(&mut stream, debugger),
        ("GET", "/api/history") => handle_get_history(&mut stream, debugger),
        ("GET", "/api/source") => handle_get_source(&mut stream, debugger),
        ("GET", "/api/breakpoints") => handle_get_breakpoints(&mut stream, debugger),
        ("GET", "/json") | ("GET", "/json/list") => handle_cdp_discover(&mut stream, debugger),
        ("POST", "/api/step/next") => handle_step_next(&mut stream, debugger),
        ("POST", "/api/step/back") => handle_step_back(&mut stream, debugger),
        ("POST", "/api/breakpoints") => handle_add_breakpoint(&mut stream, debugger, &body),
        _ if path.starts_with("/api/goto/") => {
            let step_str = path.trim_start_matches("/api/goto/");
            if let Ok(step) = step_str.parse::<u64>() {
                handle_goto(&mut stream, debugger, step)
            } else {
                send_error(&mut stream, 400, "Invalid step number")
            }
        }
        _ if path.starts_with("/api/changes/") => {
            let var_name = path.trim_start_matches("/api/changes/");
            handle_get_changes(&mut stream, debugger, var_name)
        }
        _ => send_error(&mut stream, 404, "Not Found"),
    }
}

fn send_json<T: Serialize>(stream: &mut TcpStream, data: &T) -> std::io::Result<()> {
    let json = serde_json::to_string(data).unwrap();
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/json\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        json.len(),
        json
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn send_html(stream: &mut TcpStream, html: &str) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        html.len(),
        html
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn send_error(stream: &mut TcpStream, code: u16, message: &str) -> std::io::Result<()> {
    let body = format!("{{\"error\": \"{}\"}}", message);
    let status = match code {
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        code, status, body.len(), body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn send_cors_preflight(stream: &mut TcpStream) -> std::io::Result<()> {
    let response = "HTTP/1.1 204 No Content\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Access-Control-Max-Age: 86400\r\n\
         Content-Length: 0\r\n\
         \r\n";
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// Chrome DevTools Protocol discovery endpoint (`/json` and `/json/list`).
/// Returns a target descriptor compatible with Chrome DevTools.
fn handle_cdp_discover(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let dbg = debugger.borrow();
    let info = dbg.recording_info();
    let target = CdpTarget {
        description: "Quicksilver JavaScript runtime".to_string(),
        devtools_frontend_url: String::new(),
        id: "quicksilver-main".to_string(),
        title: if info.filename.is_empty() { "quicksilver".to_string() } else { info.filename.clone() },
        target_type: "node".to_string(),
        url: info.filename,
        websocket_debugger_url: String::new(),
    };
    send_json(stream, &vec![target])
}

/// Minimal Chrome DevTools Protocol target descriptor.
#[derive(Debug, Serialize)]
struct CdpTarget {
    pub description: String,
    #[serde(rename = "devtoolsFrontendUrl")]
    pub devtools_frontend_url: String,
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub target_type: String,
    pub url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_debugger_url: String,
}

fn handle_get_state(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let dbg = debugger.borrow();
    let info = dbg.recording_info();

    let state = DebuggerState {
        current_step: dbg.current().map(|r| r.step).unwrap_or(0),
        total_steps: info.total_steps,
        current_line: dbg.current().map(|r| r.line).unwrap_or(0),
        current_position: info.current_position,
        history_size: info.history_size,
        is_paused: dbg.is_paused(),
        filename: info.filename,
    };

    send_json(stream, &ApiResponse::success(state))
}

fn handle_get_current(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let dbg = debugger.borrow();

    if let Some(record) = dbg.current() {
        let response = RecordResponse {
            step: record.step,
            line: record.line,
            description: record.description.clone(),
            stack_size: record.stack_size,
            locals: record.locals.iter().map(|(name, value)| {
                VariableInfo {
                    name: name.clone(),
                    value: value.to_js_string(),
                    var_type: value.type_of().to_string(),
                }
            }).collect(),
        };
        send_json(stream, &ApiResponse::success(response))
    } else {
        send_json(stream, &ApiResponse::<()>::error("No current record"))
    }
}

fn handle_get_history(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let dbg = debugger.borrow();
    let _info = dbg.recording_info();

    // Note: We can't easily iterate history from RecordingInfo
    // This is a simplified response with just the current entry
    let entries: Vec<HistoryEntry> = vec![HistoryEntry {
        step: dbg.current().map(|r| r.step).unwrap_or(0),
        line: dbg.current().map(|r| r.line).unwrap_or(0),
        description: dbg.current().map(|r| r.description.clone()).unwrap_or_default(),
        is_current: true,
    }];

    send_json(stream, &ApiResponse::success(entries))
}

fn handle_get_source(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let dbg = debugger.borrow();
    let info = dbg.recording_info();

    // Try to read source file
    let lines = std::fs::read_to_string(&info.filename)
        .map(|s| s.lines().map(|l| l.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();

    let source = SourceInfo {
        filename: info.filename,
        total_lines: lines.len(),
        lines,
    };

    send_json(stream, &ApiResponse::success(source))
}

fn handle_get_breakpoints(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let _dbg = debugger.borrow();

    // Note: We'd need to expose breakpoints from TimeTravelDebugger
    // For now, return empty list
    let breakpoints: Vec<BreakpointInfo> = vec![];

    send_json(stream, &ApiResponse::success(breakpoints))
}

fn handle_step_next(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let mut dbg = debugger.borrow_mut();

    if let Some(record) = dbg.step_forward() {
        let response = RecordResponse {
            step: record.step,
            line: record.line,
            description: record.description.clone(),
            stack_size: record.stack_size,
            locals: record.locals.iter().map(|(name, value)| {
                VariableInfo {
                    name: name.clone(),
                    value: value.to_js_string(),
                    var_type: value.type_of().to_string(),
                }
            }).collect(),
        };
        send_json(stream, &ApiResponse::success(response))
    } else {
        send_json(stream, &ApiResponse::<()>::error("Already at end of history"))
    }
}

fn handle_step_back(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
) -> std::io::Result<()> {
    let mut dbg = debugger.borrow_mut();

    if let Some(record) = dbg.step_back() {
        let response = RecordResponse {
            step: record.step,
            line: record.line,
            description: record.description.clone(),
            stack_size: record.stack_size,
            locals: record.locals.iter().map(|(name, value)| {
                VariableInfo {
                    name: name.clone(),
                    value: value.to_js_string(),
                    var_type: value.type_of().to_string(),
                }
            }).collect(),
        };
        send_json(stream, &ApiResponse::success(response))
    } else {
        send_json(stream, &ApiResponse::<()>::error("Already at beginning of history"))
    }
}

fn handle_goto(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
    step: u64,
) -> std::io::Result<()> {
    let mut dbg = debugger.borrow_mut();

    if let Some(record) = dbg.jump_to(step) {
        let response = RecordResponse {
            step: record.step,
            line: record.line,
            description: record.description.clone(),
            stack_size: record.stack_size,
            locals: record.locals.iter().map(|(name, value)| {
                VariableInfo {
                    name: name.clone(),
                    value: value.to_js_string(),
                    var_type: value.type_of().to_string(),
                }
            }).collect(),
        };
        send_json(stream, &ApiResponse::success(response))
    } else {
        send_json(stream, &ApiResponse::<()>::error("Step not found"))
    }
}

fn handle_get_changes(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
    var_name: &str,
) -> std::io::Result<()> {
    let dbg = debugger.borrow();
    let changes = dbg.find_variable_changes(var_name);

    let entries: Vec<ChangeEntry> = changes.iter().map(|(step, value)| {
        ChangeEntry {
            step: *step,
            old_value: None, // We don't track old value in find_variable_changes
            new_value: value.to_js_string(),
        }
    }).collect();

    send_json(stream, &ApiResponse::success(entries))
}

fn handle_add_breakpoint(
    stream: &mut TcpStream,
    debugger: &Rc<RefCell<TimeTravelDebugger>>,
    body: &str,
) -> std::io::Result<()> {
    #[derive(Deserialize)]
    struct AddBreakpointRequest {
        line: u32,
        condition: Option<String>,
    }

    if let Ok(req) = serde_json::from_str::<AddBreakpointRequest>(body) {
        let mut dbg = debugger.borrow_mut();
        let _id = dbg.add_breakpoint(req.line, req.condition.clone());

        let bp = BreakpointInfo {
            line: req.line,
            enabled: true,
            hit_count: 0,
            condition: req.condition,
        };
        send_json(stream, &ApiResponse::success(bp))
    } else {
        send_json(stream, &ApiResponse::<()>::error("Invalid request body"))
    }
}

/// Get the embedded web UI HTML
fn get_web_ui() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Quicksilver Time-Travel Debugger</title>
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            background: #1e1e2e;
            color: #cdd6f4;
            min-height: 100vh;
        }
        .container {
            max-width: 1400px;
            margin: 0 auto;
            padding: 20px;
        }
        header {
            background: #313244;
            padding: 20px;
            border-radius: 8px;
            margin-bottom: 20px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        h1 {
            font-size: 1.5rem;
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .controls {
            display: flex;
            gap: 10px;
        }
        button {
            background: #89b4fa;
            color: #1e1e2e;
            border: none;
            padding: 10px 20px;
            border-radius: 6px;
            cursor: pointer;
            font-weight: 600;
            transition: all 0.2s;
        }
        button:hover { background: #b4befe; }
        button:disabled { background: #45475a; cursor: not-allowed; }
        button.back { background: #f9e2af; }
        button.back:hover { background: #fab387; }
        .grid {
            display: grid;
            grid-template-columns: 1fr 400px;
            gap: 20px;
        }
        .panel {
            background: #313244;
            border-radius: 8px;
            padding: 15px;
        }
        .panel-title {
            font-size: 0.9rem;
            color: #a6adc8;
            margin-bottom: 15px;
            text-transform: uppercase;
            letter-spacing: 1px;
        }
        .timeline {
            height: 60px;
            background: #1e1e2e;
            border-radius: 4px;
            margin-bottom: 20px;
            position: relative;
            overflow: hidden;
        }
        .timeline-track {
            position: absolute;
            top: 50%;
            left: 10px;
            right: 10px;
            height: 4px;
            background: #45475a;
            transform: translateY(-50%);
            border-radius: 2px;
        }
        .timeline-position {
            position: absolute;
            top: 50%;
            width: 16px;
            height: 16px;
            background: #89b4fa;
            border-radius: 50%;
            transform: translate(-50%, -50%);
            transition: left 0.2s;
        }
        .timeline-info {
            position: absolute;
            bottom: 5px;
            left: 50%;
            transform: translateX(-50%);
            font-size: 0.8rem;
            color: #a6adc8;
        }
        .source {
            font-family: 'JetBrains Mono', 'Fira Code', monospace;
            font-size: 0.85rem;
            overflow: auto;
            max-height: 400px;
        }
        .source-line {
            display: flex;
            padding: 2px 10px;
            line-height: 1.6;
        }
        .source-line:hover { background: #45475a; }
        .source-line.current { background: #45475a; }
        .source-line.current::before {
            content: '→';
            color: #f9e2af;
            margin-right: 10px;
        }
        .line-number {
            color: #6c7086;
            width: 40px;
            text-align: right;
            margin-right: 15px;
            user-select: none;
        }
        .line-content { flex: 1; }
        .variables {
            font-family: 'JetBrains Mono', 'Fira Code', monospace;
            font-size: 0.85rem;
        }
        .variable {
            display: flex;
            justify-content: space-between;
            padding: 8px 0;
            border-bottom: 1px solid #45475a;
        }
        .variable:last-child { border-bottom: none; }
        .var-name { color: #89b4fa; }
        .var-value { color: #a6e3a1; }
        .var-type { color: #6c7086; font-size: 0.75rem; }
        .step-info {
            display: flex;
            gap: 20px;
            margin-bottom: 20px;
        }
        .step-info-item {
            background: #1e1e2e;
            padding: 10px 15px;
            border-radius: 4px;
        }
        .step-info-label { color: #6c7086; font-size: 0.75rem; }
        .step-info-value { font-size: 1.2rem; color: #f9e2af; }
        .history {
            max-height: 300px;
            overflow-y: auto;
        }
        .history-entry {
            padding: 8px;
            border-radius: 4px;
            margin-bottom: 4px;
            cursor: pointer;
            display: flex;
            gap: 10px;
        }
        .history-entry:hover { background: #45475a; }
        .history-entry.current { background: #45475a; border-left: 3px solid #89b4fa; }
        .history-step { color: #6c7086; width: 50px; }
        .history-desc { flex: 1; }
        @keyframes pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.5; }
        }
        .loading { animation: pulse 1s infinite; }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>🕐 Time-Travel Debugger</h1>
            <div class="controls">
                <button class="back" onclick="stepBack()" id="btnBack">⏪ Back</button>
                <button onclick="stepForward()" id="btnNext">Next ⏩</button>
            </div>
        </header>

        <div class="timeline panel">
            <div class="timeline-track"></div>
            <div class="timeline-position" id="timelinePos" style="left: 10px;"></div>
            <div class="timeline-info" id="timelineInfo">Step 0 / 0</div>
        </div>

        <div class="step-info">
            <div class="step-info-item">
                <div class="step-info-label">Current Step</div>
                <div class="step-info-value" id="currentStep">-</div>
            </div>
            <div class="step-info-item">
                <div class="step-info-label">Line</div>
                <div class="step-info-value" id="currentLine">-</div>
            </div>
            <div class="step-info-item">
                <div class="step-info-label">Stack Size</div>
                <div class="step-info-value" id="stackSize">-</div>
            </div>
        </div>

        <div class="grid">
            <div class="panel">
                <div class="panel-title">Source</div>
                <div class="source" id="source">
                    <div class="loading">Loading source...</div>
                </div>
            </div>
            <div>
                <div class="panel" style="margin-bottom: 20px;">
                    <div class="panel-title">Variables</div>
                    <div class="variables" id="variables">
                        <div class="loading">Loading...</div>
                    </div>
                </div>
                <div class="panel">
                    <div class="panel-title">History</div>
                    <div class="history" id="history">
                        <div class="loading">Loading...</div>
                    </div>
                </div>
            </div>
        </div>
    </div>

    <script>
        let state = { currentStep: 0, totalSteps: 0, currentLine: 0 };
        let sourceLines = [];

        async function fetchState() {
            try {
                const res = await fetch('/api/state');
                const data = await res.json();
                if (data.success) {
                    state = data.data;
                    updateUI();
                }
            } catch (e) { console.error('Failed to fetch state:', e); }
        }

        async function fetchCurrent() {
            try {
                const res = await fetch('/api/current');
                const data = await res.json();
                if (data.success) {
                    updateCurrentRecord(data.data);
                }
            } catch (e) { console.error('Failed to fetch current:', e); }
        }

        async function fetchSource() {
            try {
                const res = await fetch('/api/source');
                const data = await res.json();
                if (data.success) {
                    sourceLines = data.data.lines;
                    renderSource();
                }
            } catch (e) { console.error('Failed to fetch source:', e); }
        }

        async function stepForward() {
            try {
                const res = await fetch('/api/step/next', { method: 'POST' });
                const data = await res.json();
                if (data.success) {
                    updateCurrentRecord(data.data);
                }
                await fetchState();
            } catch (e) { console.error('Failed to step forward:', e); }
        }

        async function stepBack() {
            try {
                const res = await fetch('/api/step/back', { method: 'POST' });
                const data = await res.json();
                if (data.success) {
                    updateCurrentRecord(data.data);
                }
                await fetchState();
            } catch (e) { console.error('Failed to step back:', e); }
        }

        function updateUI() {
            // Timeline
            const pos = state.totalSteps > 0
                ? 10 + (state.currentPosition / state.totalSteps) * 80
                : 10;
            document.getElementById('timelinePos').style.left = pos + '%';
            document.getElementById('timelineInfo').textContent =
                `Step ${state.currentPosition + 1} / ${state.totalSteps}`;

            // Step info
            document.getElementById('currentStep').textContent = state.currentStep;
            document.getElementById('currentLine').textContent = state.currentLine;

            // Highlight current line in source
            renderSource();
        }

        function updateCurrentRecord(record) {
            state.currentStep = record.step;
            state.currentLine = record.line;
            document.getElementById('currentStep').textContent = record.step;
            document.getElementById('currentLine').textContent = record.line;
            document.getElementById('stackSize').textContent = record.stackSize;

            // Variables
            const varsEl = document.getElementById('variables');
            if (record.locals.length === 0) {
                varsEl.innerHTML = '<div style="color: #6c7086;">No variables</div>';
            } else {
                varsEl.innerHTML = record.locals.map(v => `
                    <div class="variable">
                        <span class="var-name">${v.name}</span>
                        <span>
                            <span class="var-value">${v.value}</span>
                            <span class="var-type">${v.varType}</span>
                        </span>
                    </div>
                `).join('');
            }

            // Update source highlighting
            renderSource();
        }

        function renderSource() {
            const sourceEl = document.getElementById('source');
            if (sourceLines.length === 0) {
                sourceEl.innerHTML = '<div style="color: #6c7086;">No source loaded</div>';
                return;
            }

            sourceEl.innerHTML = sourceLines.map((line, i) => `
                <div class="source-line ${i + 1 === state.currentLine ? 'current' : ''}">
                    <span class="line-number">${i + 1}</span>
                    <span class="line-content">${escapeHtml(line) || ' '}</span>
                </div>
            `).join('');

            // Scroll to current line
            const currentLineEl = sourceEl.querySelector('.current');
            if (currentLineEl) {
                currentLineEl.scrollIntoView({ block: 'center', behavior: 'smooth' });
            }
        }

        function escapeHtml(text) {
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }

        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => {
            if (e.key === 'ArrowRight' || e.key === 'n') stepForward();
            if (e.key === 'ArrowLeft' || e.key === 'b') stepBack();
        });

        // Initialize
        fetchState();
        fetchCurrent();
        fetchSource();

        // Auto-refresh
        setInterval(fetchState, 2000);
    </script>
</body>
</html>"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_response() {
        let success: ApiResponse<String> = ApiResponse::success("test".to_string());
        assert!(success.success);
        assert_eq!(success.data, Some("test".to_string()));

        let error: ApiResponse<()> = ApiResponse::error("test error");
        assert!(!error.success);
        assert_eq!(error.error, Some("test error".to_string()));
    }

    #[test]
    fn test_server_url() {
        let debugger = TimeTravelDebugger::new();
        let server = DebugServer::new(debugger, 9229);
        assert_eq!(server.url(), "http://localhost:9229");
    }

    #[test]
    fn test_cdp_target_serialization() {
        let target = CdpTarget {
            description: "test".to_string(),
            devtools_frontend_url: String::new(),
            id: "t1".to_string(),
            title: "main.js".to_string(),
            target_type: "node".to_string(),
            url: "main.js".to_string(),
            websocket_debugger_url: String::new(),
        };
        let json = serde_json::to_string(&target).unwrap();
        assert!(json.contains("\"type\":\"node\""));
        assert!(json.contains("\"devtoolsFrontendUrl\""));
        assert!(json.contains("\"webSocketDebuggerUrl\""));
    }

    #[test]
    fn test_api_response_data() {
        let resp: ApiResponse<Vec<String>> = ApiResponse::success(vec!["a".into(), "b".into()]);
        assert!(resp.success);
        assert_eq!(resp.data.unwrap().len(), 2);
        assert!(resp.error.is_none());
    }
}
