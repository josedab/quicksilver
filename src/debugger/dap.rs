//! Debug Adapter Protocol (DAP) Implementation
//!
//! This module implements the Debug Adapter Protocol, enabling integration
//! with VS Code and other DAP-compatible IDEs for time-travel debugging.
//!
//! Reference: <https://microsoft.github.io/debug-adapter-protocol/>

use super::TimeTravelDebugger;
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

/// DAP Message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DAPMessage {
    #[serde(rename = "request")]
    Request(DAPRequest),
    #[serde(rename = "response")]
    Response(DAPResponse),
    #[serde(rename = "event")]
    Event(DAPEvent),
}

/// DAP Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAPRequest {
    pub seq: i64,
    pub command: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// DAP Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAPResponse {
    pub seq: i64,
    pub request_seq: i64,
    pub success: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// DAP Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAPEvent {
    pub seq: i64,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// DAP Capabilities returned during initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_step_back: bool,
    pub supports_restart_request: bool,
    pub supports_goto_targets_request: bool,
    pub supports_evaluate_for_hovers: bool,
    pub supports_set_variable: bool,
    pub supports_completions_request: bool,
    pub supports_exception_info_request: bool,
    pub supports_loaded_sources_request: bool,
    pub supports_terminate_request: bool,
    pub supports_reverse_request: bool,
    /// Custom: supports time-travel debugging
    pub supports_time_travel: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            supports_configuration_done_request: true,
            supports_step_back: true, // Key feature!
            supports_restart_request: true,
            supports_goto_targets_request: true,
            supports_evaluate_for_hovers: true,
            supports_set_variable: false, // Not supported in time-travel mode
            supports_completions_request: true,
            supports_exception_info_request: true,
            supports_loaded_sources_request: true,
            supports_terminate_request: true,
            supports_reverse_request: true, // Another key feature!
            supports_time_travel: true,
        }
    }
}

/// Stack frame representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    pub source: Option<Source>,
    pub line: i64,
    pub column: i64,
    pub end_line: Option<i64>,
    pub end_column: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<String>,
}

/// Source representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub name: Option<String>,
    pub path: Option<String>,
    pub source_reference: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<String>,
}

/// Variable representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub var_type: Option<String>,
    pub variables_reference: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<i64>,
}

/// Scope representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub name: String,
    pub presentation_hint: Option<String>,
    pub variables_reference: i64,
    pub named_variables: Option<i64>,
    pub expensive: bool,
}

/// Breakpoint representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DAPBreakpoint {
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    pub source: Option<Source>,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub end_line: Option<i64>,
    pub end_column: Option<i64>,
}

/// Thread representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

/// Debug Adapter Protocol server (single-threaded)
pub struct DAPServer {
    debugger: Rc<RefCell<TimeTravelDebugger>>,
    seq_counter: i64,
    initialized: bool,
    variables_ref_counter: i64,
    variables_map: HashMap<i64, Vec<Variable>>,
}

impl DAPServer {
    /// Create a new DAP server with a debugger instance
    pub fn new(debugger: Rc<RefCell<TimeTravelDebugger>>) -> Self {
        Self {
            debugger,
            seq_counter: 0,
            initialized: false,
            variables_ref_counter: 1,
            variables_map: HashMap::default(),
        }
    }

    /// Get next sequence number
    fn next_seq(&mut self) -> i64 {
        self.seq_counter += 1;
        self.seq_counter
    }

    /// Handle a DAP request and return a response
    pub fn handle_request(&mut self, request: DAPRequest) -> DAPResponse {
        match request.command.as_str() {
            "initialize" => self.handle_initialize(request),
            "configurationDone" => self.handle_configuration_done(request),
            "launch" => self.handle_launch(request),
            "attach" => self.handle_attach(request),
            "disconnect" => self.handle_disconnect(request),
            "terminate" => self.handle_terminate(request),
            "setBreakpoints" => self.handle_set_breakpoints(request),
            "threads" => self.handle_threads(request),
            "stackTrace" => self.handle_stack_trace(request),
            "scopes" => self.handle_scopes(request),
            "variables" => self.handle_variables(request),
            "continue" => self.handle_continue(request),
            "next" => self.handle_next(request),
            "stepIn" => self.handle_step_in(request),
            "stepOut" => self.handle_step_out(request),
            "stepBack" => self.handle_step_back(request),
            "reverseContinue" => self.handle_reverse_continue(request),
            "restart" => self.handle_restart(request),
            "pause" => self.handle_pause(request),
            "evaluate" => self.handle_evaluate(request),
            "source" => self.handle_source(request),
            "loadedSources" => self.handle_loaded_sources(request),
            // Custom time-travel commands
            "timeTravelGoto" => self.handle_time_travel_goto(request),
            "timeTravelHistory" => self.handle_time_travel_history(request),
            "timeTravelChanges" => self.handle_time_travel_changes(request),
            _ => {
                let cmd = request.command.clone();
                self.error_response(request, format!("Unknown command: {}", cmd))
            }
        }
    }

    /// Handle initialize request
    fn handle_initialize(&mut self, request: DAPRequest) -> DAPResponse {
        self.initialized = true;
        let seq = self.next_seq();

        let capabilities = Capabilities::default();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::to_value(capabilities).unwrap()),
        }
    }

    /// Handle configuration done request
    fn handle_configuration_done(&mut self, request: DAPRequest) -> DAPResponse {
        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle launch request
    fn handle_launch(&mut self, request: DAPRequest) -> DAPResponse {
        // Extract program path from arguments
        if let Some(program) = request.arguments.get("program") {
            if let Some(path) = program.as_str() {
                // Load source into debugger
                if let Ok(source) = std::fs::read_to_string(path) {
                    self.debugger.borrow_mut().load_source(path, &source);
                }
            }
        }

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle attach request
    fn handle_attach(&mut self, request: DAPRequest) -> DAPResponse {
        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle disconnect request
    fn handle_disconnect(&mut self, request: DAPRequest) -> DAPResponse {
        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle terminate request
    fn handle_terminate(&mut self, request: DAPRequest) -> DAPResponse {
        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle setBreakpoints request
    fn handle_set_breakpoints(&mut self, request: DAPRequest) -> DAPResponse {
        let mut breakpoints = Vec::new();

        if let Some(bps) = request.arguments.get("breakpoints") {
            if let Some(bp_array) = bps.as_array() {
                let mut debugger = self.debugger.borrow_mut();

                for bp in bp_array {
                    if let Some(line) = bp.get("line").and_then(|l| l.as_i64()) {
                        let condition = bp.get("condition")
                            .and_then(|c| c.as_str())
                            .map(|s| s.to_string());

                        let id = debugger.add_breakpoint(line as u32, condition);

                        breakpoints.push(DAPBreakpoint {
                            id: Some(id as i64),
                            verified: true,
                            message: None,
                            source: None,
                            line: Some(line),
                            column: None,
                            end_line: None,
                            end_column: None,
                        });
                    }
                }
            }
        }

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({ "breakpoints": breakpoints })),
        }
    }

    /// Handle threads request
    fn handle_threads(&mut self, request: DAPRequest) -> DAPResponse {
        // Single-threaded JavaScript
        let threads = vec![Thread {
            id: 1,
            name: "main".to_string(),
        }];

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({ "threads": threads })),
        }
    }

    /// Handle stackTrace request
    fn handle_stack_trace(&mut self, request: DAPRequest) -> DAPResponse {
        let debugger = self.debugger.borrow();
        let mut frames = Vec::new();

        if let Some(record) = debugger.current() {
            frames.push(StackFrame {
                id: 1,
                name: record.description.clone(),
                source: Some(Source {
                    name: Some(debugger.recording_info().filename.clone()),
                    path: Some(debugger.recording_info().filename.clone()),
                    source_reference: None,
                    presentation_hint: None,
                }),
                line: record.line as i64,
                column: 1,
                end_line: None,
                end_column: None,
                module_id: None,
                presentation_hint: None,
            });
        }
        drop(debugger);

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({
                "stackFrames": frames,
                "totalFrames": frames.len()
            })),
        }
    }

    /// Handle scopes request
    fn handle_scopes(&mut self, request: DAPRequest) -> DAPResponse {
        let locals_ref = self.variables_ref_counter;
        self.variables_ref_counter += 1;

        // Build variables for locals
        let debugger = self.debugger.borrow();
        if let Some(record) = debugger.current() {
            let vars: Vec<Variable> = record.locals.iter().map(|(name, value)| {
                Variable {
                    name: name.clone(),
                    value: value.to_js_string(),
                    var_type: Some(value.type_of().to_string()),
                    variables_reference: 0, // No nested variables for primitives
                    named_variables: None,
                    indexed_variables: None,
                }
            }).collect();
            self.variables_map.insert(locals_ref, vars);
        }
        drop(debugger);

        let scopes = vec![
            Scope {
                name: "Locals".to_string(),
                presentation_hint: Some("locals".to_string()),
                variables_reference: locals_ref,
                named_variables: None,
                expensive: false,
            },
        ];

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({ "scopes": scopes })),
        }
    }

    /// Handle variables request
    fn handle_variables(&mut self, request: DAPRequest) -> DAPResponse {
        let var_ref = request.arguments.get("variablesReference")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let variables = self.variables_map.get(&var_ref)
            .cloned()
            .unwrap_or_default();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({ "variables": variables })),
        }
    }

    /// Handle continue request
    fn handle_continue(&mut self, request: DAPRequest) -> DAPResponse {
        // In time-travel debugging, continue moves forward
        self.debugger.borrow_mut().replay_end();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({ "allThreadsContinued": true })),
        }
    }

    /// Handle next request (step forward)
    fn handle_next(&mut self, request: DAPRequest) -> DAPResponse {
        self.debugger.borrow_mut().step_forward();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle stepIn request
    fn handle_step_in(&mut self, request: DAPRequest) -> DAPResponse {
        // Same as next for now
        self.handle_next(request)
    }

    /// Handle stepOut request
    fn handle_step_out(&mut self, request: DAPRequest) -> DAPResponse {
        // Same as next for now
        self.handle_next(request)
    }

    /// Handle stepBack request - THE KEY TIME-TRAVEL FEATURE!
    fn handle_step_back(&mut self, request: DAPRequest) -> DAPResponse {
        self.debugger.borrow_mut().step_back();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle reverseContinue request
    fn handle_reverse_continue(&mut self, request: DAPRequest) -> DAPResponse {
        // Move to beginning of history
        self.debugger.borrow_mut().replay_reset();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle restart request
    fn handle_restart(&mut self, request: DAPRequest) -> DAPResponse {
        self.debugger.borrow_mut().replay_reset();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle pause request
    fn handle_pause(&mut self, request: DAPRequest) -> DAPResponse {
        self.debugger.borrow_mut().pause();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: None,
        }
    }

    /// Handle evaluate request
    fn handle_evaluate(&mut self, request: DAPRequest) -> DAPResponse {
        let expr = request.arguments.get("expression")
            .and_then(|e| e.as_str())
            .unwrap_or("");

        // Look up variable in current scope
        let debugger = self.debugger.borrow();
        let result = if let Some(record) = debugger.current() {
            record.locals.get(expr)
                .map(|v| v.to_js_string())
                .unwrap_or_else(|| format!("undefined ({})", expr))
        } else {
            "undefined".to_string()
        };
        drop(debugger);

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({
                "result": result,
                "variablesReference": 0
            })),
        }
    }

    /// Handle source request
    fn handle_source(&mut self, request: DAPRequest) -> DAPResponse {
        let filename = self.debugger.borrow().recording_info().filename.clone();

        // Read source file
        let content = std::fs::read_to_string(&filename)
            .unwrap_or_default();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({
                "content": content
            })),
        }
    }

    /// Handle loadedSources request
    fn handle_loaded_sources(&mut self, request: DAPRequest) -> DAPResponse {
        let filename = self.debugger.borrow().recording_info().filename.clone();

        let sources = vec![Source {
            name: Some(filename.clone()),
            path: Some(filename),
            source_reference: None,
            presentation_hint: None,
        }];

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({ "sources": sources })),
        }
    }

    // Custom time-travel commands

    /// Handle time travel goto - jump to a specific step
    fn handle_time_travel_goto(&mut self, request: DAPRequest) -> DAPResponse {
        let step = request.arguments.get("step")
            .and_then(|s| s.as_u64())
            .unwrap_or(0);

        let success = self.debugger.borrow_mut().jump_to(step).is_some();

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success,
            command: request.command,
            message: if success { None } else { Some("Step not found".to_string()) },
            body: None,
        }
    }

    /// Handle time travel history - get execution history
    fn handle_time_travel_history(&mut self, request: DAPRequest) -> DAPResponse {
        let debugger = self.debugger.borrow();
        let info = debugger.recording_info();

        #[derive(Serialize)]
        struct HistoryEntry {
            step: u64,
            line: u32,
            description: String,
            is_current: bool,
        }

        let current_pos = info.current_position;

        let entries = if let Some(record) = debugger.current() {
            vec![HistoryEntry {
                step: record.step,
                line: record.line,
                description: record.description.clone(),
                is_current: true,
            }]
        } else {
            vec![]
        };

        let total_steps = info.total_steps;
        drop(debugger);

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({
                "entries": entries,
                "totalSteps": total_steps,
                "currentPosition": current_pos
            })),
        }
    }

    /// Handle time travel changes - find when a variable changed
    fn handle_time_travel_changes(&mut self, request: DAPRequest) -> DAPResponse {
        let var_name = request.arguments.get("variable")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let debugger = self.debugger.borrow();
        let changes = debugger.find_variable_changes(var_name);

        #[derive(Serialize)]
        struct ChangeEntry {
            step: u64,
            value: String,
        }

        let entries: Vec<ChangeEntry> = changes.iter().map(|(step, value)| {
            ChangeEntry {
                step: *step,
                value: value.to_js_string(),
            }
        }).collect();
        drop(debugger);

        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: true,
            command: request.command,
            message: None,
            body: Some(serde_json::json!({
                "variable": var_name,
                "changes": entries
            })),
        }
    }

    /// Create an error response
    fn error_response(&mut self, request: DAPRequest, message: String) -> DAPResponse {
        let seq = self.next_seq();
        DAPResponse {
            seq,
            request_seq: request.seq,
            success: false,
            command: request.command,
            message: Some(message),
            body: None,
        }
    }

    /// Create a stopped event
    pub fn stopped_event(&mut self, reason: &str, description: Option<&str>) -> DAPEvent {
        let seq = self.next_seq();
        DAPEvent {
            seq,
            event: "stopped".to_string(),
            body: Some(serde_json::json!({
                "reason": reason,
                "description": description,
                "threadId": 1,
                "allThreadsStopped": true
            })),
        }
    }

    /// Create an initialized event
    pub fn initialized_event(&mut self) -> DAPEvent {
        let seq = self.next_seq();
        DAPEvent {
            seq,
            event: "initialized".to_string(),
            body: None,
        }
    }

    /// Create a terminated event
    pub fn terminated_event(&mut self) -> DAPEvent {
        let seq = self.next_seq();
        DAPEvent {
            seq,
            event: "terminated".to_string(),
            body: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dap_server_creation() {
        let debugger = Rc::new(RefCell::new(TimeTravelDebugger::new()));
        let _server = DAPServer::new(debugger);
    }

    #[test]
    fn test_capabilities() {
        let caps = Capabilities::default();
        assert!(caps.supports_step_back);
        assert!(caps.supports_time_travel);
        assert!(caps.supports_reverse_request);
    }

    #[test]
    fn test_handle_initialize() {
        let debugger = Rc::new(RefCell::new(TimeTravelDebugger::new()));
        let mut server = DAPServer::new(debugger);

        let request = DAPRequest {
            seq: 1,
            command: "initialize".to_string(),
            arguments: serde_json::json!({}),
        };

        let response = server.handle_request(request);
        assert!(response.success);
    }

    #[test]
    fn test_handle_threads() {
        let debugger = Rc::new(RefCell::new(TimeTravelDebugger::new()));
        let mut server = DAPServer::new(debugger);

        let request = DAPRequest {
            seq: 1,
            command: "threads".to_string(),
            arguments: serde_json::json!({}),
        };

        let response = server.handle_request(request);
        assert!(response.success);
        assert!(response.body.is_some());
    }
}
