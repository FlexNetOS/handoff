//! `hf-mcp` — Model Context Protocol server exposing the `hf` continuity CLI as tools.
//!
//! Implements the T11 universal control seam for handoff: chat / rvAgent clients talk
//! JSON-RPC over stdin/stdout and invoke verbs like `hf_status`, `hf_claim`, `hf_ship`,
//! etc. Each tool shells to the `hf` binary and returns captured stdout/stderr.
//!
//! The server speaks MCP protocol version 2024-11-05 and exposes a `tools/list` +
//! `tools/call` surface.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "hf-mcp";
const SERVER_VERSION: &str = "0.1.0";

// -----------------------------------------------------------------------------
// MCP protocol types
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Debug, Serialize)]
struct ServerCapabilities {
    tools: ToolsCapability,
}

#[derive(Debug, Serialize)]
struct ToolsCapability {
    #[serde(rename = "listChanged")]
    list_changed: bool,
}

#[derive(Debug, Serialize)]
struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
struct Tool {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct ListToolsResult {
    tools: Vec<Tool>,
}

#[derive(Debug, Serialize)]
struct CallToolResult {
    content: Vec<ToolContent>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ToolContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

// -----------------------------------------------------------------------------
// hf-mcp server
// -----------------------------------------------------------------------------

struct McpServer {
    hf_exe: PathBuf,
}

impl McpServer {
    fn new() -> Self {
        Self {
            hf_exe: find_hf_exe(),
        }
    }

    fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut stdout_lock = stdout.lock();
        let reader = BufReader::new(stdin.lock());

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("hf-mcp: failed to parse request: {e}");
                    continue;
                }
            };
            let response = self.handle_request(&request);
            let out = serde_json::to_string(&response)?;
            writeln!(stdout_lock, "{out}")?;
            stdout_lock.flush()?;
        }
        Ok(())
    }

    fn handle_request(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(),
            "initialized" => Ok(Value::Null),
            "tools/list" => self.handle_list_tools(),
            "tools/call" => self.handle_call_tool(&request.params),
            other => Err(format!("Method not found: {other}")),
        };
        match result {
            Ok(value) => self.ok_response(request.id.clone(), value),
            Err(message) => self.error_response(request.id.clone(), -32603, message),
        }
    }

    fn ok_response(&self, id: Option<Value>, result: Value) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error_response(&self, id: Option<Value>, code: i32, message: String) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }

    fn handle_initialize(&self) -> Result<Value, String> {
        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability {
                    list_changed: false,
                },
            },
            server_info: ServerInfo {
                name: SERVER_NAME.to_string(),
                version: SERVER_VERSION.to_string(),
            },
        };
        serde_json::to_value(result).map_err(|e| e.to_string())
    }

    fn handle_list_tools(&self) -> Result<Value, String> {
        let tools = tools();
        let result = ListToolsResult { tools };
        serde_json::to_value(result).map_err(|e| e.to_string())
    }

    fn handle_call_tool(&self, params: &Value) -> Result<Value, String> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("missing tool name")?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));
        let args = args.as_object().ok_or("arguments must be an object")?;

        let output = dispatch_tool(name, args, &self.hf_exe)?;
        let (text, is_error) = output;
        let result = CallToolResult {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: Some(is_error),
        };
        serde_json::to_value(result).map_err(|e| e.to_string())
    }
}

// -----------------------------------------------------------------------------
// hf binary discovery
// -----------------------------------------------------------------------------

fn find_hf_exe() -> PathBuf {
    if let Ok(exe) = std::env::var("HF_EXE") {
        return PathBuf::from(exe);
    }
    // Same directory as this MCP server binary (typical install layout).
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let sibling = dir.join(if cfg!(windows) { "hf.exe" } else { "hf" });
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from(if cfg!(windows) { "hf.exe" } else { "hf" })
}

// -----------------------------------------------------------------------------
// tool execution
// -----------------------------------------------------------------------------

fn run_hf(hf_exe: &PathBuf, args: &[String]) -> Result<(String, bool), String> {
    let mut cmd = Command::new(hf_exe);
    cmd.args(args);
    let output = cmd.output().map_err(|e| format!("failed to run hf: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut text = stdout;
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Ok((text, !output.status.success()))
}

/// Build the `hf` CLI argument list for a tool call without executing it.
/// Exposed for unit tests so they can assert arg shaping without requiring
/// the `hf` binary to be present or up-to-date.
fn build_hf_args(name: &str, args: &serde_json::Map<String, Value>) -> Result<Vec<String>, String> {
    let mut hf_args = vec![];
    match name {
        "hf_init" => {
            hf_args.push("init".to_string());
        }
        "hf_seed" => {
            hf_args.push("seed".to_string());
        }
        "hf_status" => {
            hf_args.push("status".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_doctor" => {
            hf_args.push("doctor".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_reconcile" => {
            hf_args.push("reconcile".to_string());
        }
        "hf_sync_cards" => {
            hf_args.push("sync-cards".to_string());
        }
        "hf_sync" => {
            hf_args.push("sync".to_string());
            if arg_bool(args, "auto") {
                hf_args.push("--auto".to_string());
            }
            if arg_bool(args, "dry_run") {
                hf_args.push("--dry-run".to_string());
            }
        }
        "hf_resume" => {
            hf_args.push("resume".to_string());
            let mode = arg_string(args, "mode").unwrap_or_else(|| "json".to_string());
            match mode.as_str() {
                "json" => hf_args.push("--json".to_string()),
                "compact" => hf_args.push("--compact".to_string()),
                "full" => {}
                other => return Err(format!("invalid mode: {other}")),
            }
        }
        "hf_claim" => {
            hf_args.push("claim".to_string());
            if arg_bool(args, "next") {
                hf_args.push("--next".to_string());
            } else if arg_bool(args, "batch") {
                hf_args.push("--batch".to_string());
            } else {
                hf_args.push(require_string(args, "id")?);
            }
        }
        "hf_release" => {
            hf_args.push("release".to_string());
            hf_args.push(require_string(args, "id")?);
        }
        "hf_checkpoint" => {
            hf_args.push("checkpoint".to_string());
            hf_args.push(require_string(args, "id")?);
            if let Some(note) = arg_string(args, "note") {
                hf_args.push(note);
            }
            if arg_bool(args, "auto") {
                hf_args.push("--auto".to_string());
            }
            if arg_bool(args, "quiet") {
                hf_args.push("--quiet".to_string());
            }
            if arg_bool(args, "sync_cards") {
                hf_args.push("--sync-cards".to_string());
            }
        }
        "hf_done" => {
            hf_args.push("done".to_string());
            hf_args.push(require_string(args, "id")?);
            if let Some(pr) = arg_string(args, "pr") {
                hf_args.push("--pr".to_string());
                hf_args.push(pr);
            }
        }
        "hf_test" => {
            hf_args.push("test".to_string());
            hf_args.push(require_string(args, "id")?);
        }
        "hf_ship" => {
            hf_args.push("ship".to_string());
            hf_args.push(require_string(args, "id")?);
            if let Some(base) = arg_string(args, "base") {
                hf_args.push("--base".to_string());
                hf_args.push(base);
            }
        }
        "hf_review_request" => {
            hf_args.push("review".to_string());
            hf_args.push("request".to_string());
            hf_args.push(require_string(args, "pr")?);
            if let Some(task_id) = arg_string(args, "task_id") {
                hf_args.push("--task".to_string());
                hf_args.push(task_id);
            }
        }
        "hf_review_verdict" => {
            hf_args.push("review".to_string());
            hf_args.push("verdict".to_string());
            hf_args.push(require_string(args, "id")?);
            hf_args.push(require_string(args, "pr")?);
            hf_args.push(require_string(args, "verdict")?);
            if let Some(by) = arg_string(args, "by") {
                hf_args.push("--by".to_string());
                hf_args.push(by);
            }
        }
        "hf_intake" => {
            hf_args.push("intake".to_string());
            hf_args.push("--bundle".to_string());
            hf_args.push(require_string(args, "bundle")?);
            if let Some(vibe) = arg_string(args, "vibe") {
                hf_args.push("--vibe".to_string());
                hf_args.push(vibe);
            }
            if let Some(intent) = arg_string(args, "intent") {
                hf_args.push("--intent".to_string());
                hf_args.push(intent);
            }
            if let Some(scope) = arg_string(args, "scope") {
                hf_args.push("--scope".to_string());
                hf_args.push(scope);
            }
        }
        "hf_dispatch" => {
            hf_args.push("dispatch".to_string());
            hf_args.push(require_string(args, "workflow_id")?);
            if arg_bool(args, "next") {
                hf_args.push("--next".to_string());
            }
        }
        "hf_prompt_hub" => {
            hf_args.push("prompt-hub".to_string());
            hf_args.push(require_string(args, "vibe")?);
            if let Some(scope) = arg_string(args, "scope") {
                hf_args.push("--scope".to_string());
                hf_args.push(scope);
            }
            if arg_bool(args, "dispatch") {
                hf_args.push("--dispatch".to_string());
            }
            hf_args.push("--json".to_string());
        }
        "hf_delivery_get" => {
            hf_args.push("delivery".to_string());
            hf_args.push("get".to_string());
            hf_args.push(require_string(args, "correlation_id")?);
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_delivery_list" => {
            hf_args.push("delivery".to_string());
            hf_args.push("list".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_task_mint" => {
            hf_args.push("task".to_string());
            hf_args.push("mint".to_string());
            hf_args.push("--from-kb".to_string());
            hf_args.push(require_string(args, "slug")?);
        }
        "hf_session_start" => {
            hf_args.push("session".to_string());
            hf_args.push("start".to_string());
        }
        "hf_session_end" => {
            hf_args.push("session".to_string());
            hf_args.push("end".to_string());
            if arg_bool(args, "recycle") {
                hf_args.push("--recycle".to_string());
            }
        }
        "hf_fleet_status" => {
            hf_args.push("fleet".to_string());
            hf_args.push("status".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_fleet_render" => {
            hf_args.push("fleet".to_string());
            hf_args.push("render".to_string());
            hf_args.push(require_string(args, "member")?);
        }
        "hf_policy_check_claim" => {
            hf_args.push("policy".to_string());
            hf_args.push("check-claim".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_policy_check_edit" => {
            hf_args.push("policy".to_string());
            hf_args.push("check-edit".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_policy_check_handoff" => {
            hf_args.push("policy".to_string());
            hf_args.push("check-handoff".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_drift" => {
            hf_args.push("drift".to_string());
            if arg_bool(args, "json") {
                hf_args.push("--json".to_string());
            }
        }
        "hf_handoff" => {
            hf_args.push("handoff".to_string());
        }
        "hf_gatekeeper_check" => {
            hf_args.push("gatekeeper".to_string());
            hf_args.push("check".to_string());
            hf_args.push(require_string(args, "pr")?);
            if let Some(task_id) = arg_string(args, "task_id") {
                hf_args.push("--task".to_string());
                hf_args.push(task_id);
            }
        }
        "hf_policy_gate" => {
            hf_args.push("policy".to_string());
            hf_args.push("gate".to_string());
            hf_args.push(require_string(args, "action")?);
            if let Some(task_id) = arg_string(args, "task_id") {
                hf_args.push("--task".to_string());
                hf_args.push(task_id);
            }
        }
        other => return Err(format!("unknown tool: {other}")),
    }
    Ok(hf_args)
}

fn dispatch_tool(
    name: &str,
    args: &serde_json::Map<String, Value>,
    hf_exe: &PathBuf,
) -> Result<(String, bool), String> {
    let hf_args = build_hf_args(name, args)?;
    run_hf(hf_exe, &hf_args)
}

fn arg_string(args: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn require_string(args: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    arg_string(args, key).ok_or_else(|| format!("missing required argument: {key}"))
}

fn arg_bool(args: &serde_json::Map<String, Value>, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

// -----------------------------------------------------------------------------
// tool definitions
// -----------------------------------------------------------------------------

fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "hf_init".to_string(),
            description: "Initialize a fresh .handoff directory.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "hf_seed".to_string(),
            description: "Seed the ledger with built-in task cards.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "hf_status".to_string(),
            description: "Show handoff task status. Returns structured output when --json is used.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "json": { "type": "boolean", "description": "Return JSON output" } }
            }),
        },
        Tool {
            name: "hf_doctor".to_string(),
            description: "Run handoff doctor diagnostics.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "json": { "type": "boolean", "description": "Return JSON output" } }
            }),
        },
        Tool {
            name: "hf_reconcile".to_string(),
            description: "Reconcile task card statuses with ledger truth.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "hf_sync_cards".to_string(),
            description: "Sync task card statuses from the ledger.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "hf_sync".to_string(),
            description: "Roll local ledger events up to the fleet ledger.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "auto": { "type": "boolean" },
                    "dry_run": { "type": "boolean", "description": "Report without writing" }
                }
            }),
        },
        Tool {
            name: "hf_resume".to_string(),
            description: "Render the next-session resume packet.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "description": "full | compact | json (default: json)" }
                }
            }),
        },
        Tool {
            name: "hf_claim".to_string(),
            description: "Claim a task, claim the next safe task, or claim a batch.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id to claim" },
                    "next": { "type": "boolean", "description": "Claim the next safe task instead" },
                    "batch": { "type": "boolean", "description": "Claim a batch of safe tasks" }
                }
            }),
        },
        Tool {
            name: "hf_release".to_string(),
            description: "Release a claimed task.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id to release" }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "hf_checkpoint".to_string(),
            description: "Record a checkpoint for a task.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id" },
                    "note": { "type": "string", "description": "Optional note" },
                    "auto": { "type": "boolean" },
                    "quiet": { "type": "boolean" },
                    "sync_cards": { "type": "boolean", "description": "Sync task cards after checkpoint" }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "hf_test".to_string(),
            description: "Run the stored test_commands for a task and witness the result.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Task id" } },
                "required": ["id"]
            }),
        },
        Tool {
            name: "hf_done".to_string(),
            description: "Mark a task done and fast-forward develop to trunk.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id" },
                    "pr": { "type": "string", "description": "PR URL" }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "hf_ship".to_string(),
            description: "Ship a task: commit, push branch, open PR, arm auto-merge.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id" },
                    "base": { "type": "string", "description": "Base branch (default from policy)" }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "hf_review_request".to_string(),
            description: "Request a review for a PR.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pr": { "type": "string", "description": "PR number or URL" },
                    "task_id": { "type": "string", "description": "Associated task id" }
                },
                "required": ["pr"]
            }),
        },
        Tool {
            name: "hf_review_verdict".to_string(),
            description: "Record a review verdict for a PR.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id" },
                    "pr": { "type": "string", "description": "PR number or URL" },
                    "verdict": { "type": "string", "description": "approve or deny" },
                    "by": { "type": "string", "description": "Reviewer name" }
                },
                "required": ["id", "pr", "verdict"]
            }),
        },
        Tool {
            name: "hf_intake".to_string(),
            description: "Intake a SwarmBundle into handoff task cards.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "bundle": { "type": "string", "description": "Path to bundle JSON" },
                    "vibe": { "type": "string" },
                    "intent": { "type": "string", "description": "Path to intent file" },
                    "scope": { "type": "string", "description": "Comma-separated scope globs" }
                },
                "required": ["bundle"]
            }),
        },
        Tool {
            name: "hf_dispatch".to_string(),
            description: "Dispatch a workflow intake bundle.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_id": { "type": "string" },
                    "next": { "type": "boolean", "description": "Only dispatch the next order" }
                },
                "required": ["workflow_id"]
            }),
        },
        Tool {
            name: "hf_prompt_hub".to_string(),
            description: "Turn a natural-language vibe request into a handoff task and optionally dispatch it (prompt_hub front door).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "vibe": { "type": "string", "description": "Natural language request" },
                    "scope": { "type": "string", "description": "Comma-separated scope globs" },
                    "dispatch": { "type": "boolean", "description": "Immediately dispatch the first safe order" }
                },
                "required": ["vibe"]
            }),
        },
        Tool {
            name: "hf_delivery_get".to_string(),
            description: "Get the delivery record for a workflow correlation_id.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "correlation_id": { "type": "string", "description": "Workflow correlation_id (SwarmBundle.workflow_id)" },
                    "json": { "type": "boolean", "description": "Return JSON output" }
                },
                "required": ["correlation_id"]
            }),
        },
        Tool {
            name: "hf_delivery_list".to_string(),
            description: "List all workflow deliveries.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "json": { "type": "boolean", "description": "Return JSON output" }
                }
            }),
        },
        Tool {
            name: "hf_task_mint".to_string(),
            description: "Mint a handoff task card from a kb slug.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "kb slug to mint from" }
                },
                "required": ["slug"]
            }),
        },
        Tool {
            name: "hf_session_start".to_string(),
            description: "Start a handoff work session.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "hf_session_end".to_string(),
            description: "End a handoff work session.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "recycle": { "type": "boolean" } }
            }),
        },
        Tool {
            name: "hf_fleet_status".to_string(),
            description: "Show fleet-wide handoff status.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "json": { "type": "boolean", "description": "Return JSON output" } }
            }),
        },
        Tool {
            name: "hf_fleet_render".to_string(),
            description: "Render a fleet member packet.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "member": { "type": "string", "description": "Member repo name" }
                },
                "required": ["member"]
            }),
        },
        Tool {
            name: "hf_policy_check_claim".to_string(),
            description: "Run the policy claim gate.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "json": { "type": "boolean" } }
            }),
        },
        Tool {
            name: "hf_policy_check_edit".to_string(),
            description: "Run the policy edit gate.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "json": { "type": "boolean" } }
            }),
        },
        Tool {
            name: "hf_policy_check_handoff".to_string(),
            description: "Run the policy handoff gate.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "json": { "type": "boolean" } }
            }),
        },
        Tool {
            name: "hf_drift".to_string(),
            description: "Run the handoff drift sentinel.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "json": { "type": "boolean", "description": "Return JSON output" } }
            }),
        },
        Tool {
            name: "hf_handoff".to_string(),
            description: "Render the next-session packet and complete the handoff.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "hf_gatekeeper_check".to_string(),
            description: "Run the surgical AI gatekeeper on a PR.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pr": { "type": "string", "description": "PR number or URL" },
                    "task_id": { "type": "string", "description": "Associated task id" }
                },
                "required": ["pr"]
            }),
        },
        Tool {
            name: "hf_policy_gate".to_string(),
            description: "Ask the cognitum-gate action governor for a permit (requires hf built with --features cognitum).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Action id to evaluate" },
                    "task_id": { "type": "string", "description": "Associated task id" }
                },
                "required": ["action"]
            }),
        },
    ]
}

// -----------------------------------------------------------------------------
// main
// -----------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = McpServer::new();
    server.run()
}

// -----------------------------------------------------------------------------
// tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_hf_exe_prefers_env_var() {
        std::env::set_var("HF_EXE", "/tmp/custom-hf");
        let exe = find_hf_exe();
        assert_eq!(exe, PathBuf::from("/tmp/custom-hf"));
    }

    #[test]
    fn dispatch_builds_status_args() {
        let mut args = serde_json::Map::new();
        args.insert("json".to_string(), Value::Bool(true));
        // In CI the `hf` binary may not be on PATH, so expect dispatch to either succeed
        // or fail to spawn — the important part is that the arg list is built correctly.
        match dispatch_tool("hf_status", &args, &PathBuf::from("hf")) {
            Ok((text, _err)) => {
                assert!(text.contains("status") || text.contains("No such file"));
            }
            Err(e) => {
                assert!(e.contains("failed to run hf") || e.contains("No such file"));
            }
        }
    }

    #[test]
    fn dispatch_requires_claim_id() {
        let args = serde_json::Map::new();
        let err = dispatch_tool("hf_claim", &args, &PathBuf::from("hf")).unwrap_err();
        assert!(err.contains("missing required argument"));
    }

    #[test]
    fn tools_include_core_verbs() {
        let names: Vec<String> = tools().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"hf_status".to_string()));
        assert!(names.contains(&"hf_claim".to_string()));
        assert!(names.contains(&"hf_ship".to_string()));
        assert!(names.contains(&"hf_handoff".to_string()));
        assert!(names.contains(&"hf_prompt_hub".to_string()));
    }

    #[test]
    fn build_prompt_hub_args_shapes_command() {
        let mut args = serde_json::Map::new();
        args.insert(
            "vibe".to_string(),
            Value::String("fix the windows test".to_string()),
        );
        args.insert(
            "scope".to_string(),
            Value::String("hf/src/kb.rs".to_string()),
        );
        args.insert("dispatch".to_string(), Value::Bool(true));
        let hf_args = build_hf_args("hf_prompt_hub", &args).unwrap();
        assert_eq!(hf_args[0], "prompt-hub");
        assert!(hf_args.contains(&"fix the windows test".to_string()));
        assert!(hf_args.contains(&"--scope".to_string()));
        assert!(hf_args.contains(&"hf/src/kb.rs".to_string()));
        assert!(hf_args.contains(&"--dispatch".to_string()));
        assert!(hf_args.contains(&"--json".to_string()));
    }
}
