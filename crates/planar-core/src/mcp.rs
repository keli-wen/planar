//! Pure read-only Model Context Protocol (`MCP 2025-03-26`) server engine for Planar.
//!
//! Shared across both transports:
//! - **Stdio** (`planar mcp`): newline-delimited JSON-RPC over `stdin`/`stdout` with zero `stdout` pollution.
//! - **Streamable HTTP** (`POST /mcp` on `Planar.app` and `planar serve`).
//!
//! Exposes strictly read-only progressive-disclosure context (`planar_status`, `planar_line`,
//! `planar_session`, `planar_search`), `@`-mentionable resources (`planar://status`,
//! `planar://line/{slug}`), and the guided `planar-plan` planning prompt.

use chrono::Local;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::db::{MAX_L1_BATCH_LINES, MAX_L2_BATCH_SESSIONS};
use crate::error::{PlanarError, Result};
use crate::model::{ONE_OFF_LINE_ID, SearchQuery, SessionFocus};
use crate::render::{render_l0_status, render_l1_lines, render_l2_sessions, render_search_results};
use crate::server::AppState;

/// Default MCP protocol revision negotiated by Planar.
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] =
    ["2025-03-26", "2025-06-18", "2025-11-25", "2024-11-05"];

const JSONRPC_PARSE_ERROR: i64 = -32700;
const JSONRPC_INVALID_REQUEST: i64 = -32600;
const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;
const JSONRPC_INVALID_PARAMS: i64 = -32602;
const JSONRPC_INTERNAL_ERROR: i64 = -32603;
const MCP_RESOURCE_NOT_FOUND: i64 = -32002;

/// Outcome of dispatching a raw JSON-RPC payload through the MCP protocol handler.
#[derive(Debug, Clone, PartialEq)]
pub enum McpDispatchOutcome {
    /// One JSON-RPC response object (or batch response array) to send back to the caller.
    Response(Value),
    /// Notification-only payload requiring no JSON-RPC response (`202 Accepted` over HTTP, silent over Stdio).
    NoResponse,
    /// Malformed JSON payload (`-32700 Parse error`).
    ParseError(Value),
}

/// Runs the newline-delimited JSON-RPC `2025-03-26` MCP loop over `reader` and `writer`.
///
/// Diagnostics are never written to `writer` (`stdout`); each outgoing JSON-RPC frame is
/// serialized onto a single compact line followed by `\n` and flushed immediately.
pub async fn serve_stdio<R, W>(state: AppState, mut reader: R, mut writer: W) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf).await?;
        if bytes_read == 0 {
            break;
        }
        let trimmed = line_buf.trim();
        if trimmed.is_empty() {
            continue;
        }

        let payload_opt = match handle_mcp_json_bytes(&state, trimmed.as_bytes()).await {
            McpDispatchOutcome::Response(val) | McpDispatchOutcome::ParseError(val) => Some(val),
            McpDispatchOutcome::NoResponse => None,
        };

        if let Some(resp_val) = payload_opt {
            let mut encoded = serde_json::to_vec(&resp_val)?;
            encoded.push(b'\n');
            writer.write_all(&encoded).await?;
            writer.flush().await?;
        }
    }
    Ok(())
}

/// Parses and dispatches a single JSON-RPC object or JSON-RPC batch array.
pub async fn handle_mcp_json_bytes(state: &AppState, raw: &[u8]) -> McpDispatchOutcome {
    let parsed: Value = match serde_json::from_slice(raw) {
        Ok(val) => val,
        Err(err) => {
            return McpDispatchOutcome::ParseError(jsonrpc_error(
                &Value::Null,
                JSONRPC_PARSE_ERROR,
                format!("Parse error: {err}"),
            ));
        }
    };

    match parsed {
        Value::Object(obj) => match handle_single_message(state, Value::Object(obj)).await {
            Some(resp) => McpDispatchOutcome::Response(resp),
            None => McpDispatchOutcome::NoResponse,
        },
        Value::Array(items) => {
            if items.is_empty() {
                return McpDispatchOutcome::Response(jsonrpc_error(
                    &Value::Null,
                    JSONRPC_INVALID_REQUEST,
                    "Invalid Request: empty batch",
                ));
            }
            let mut responses = Vec::with_capacity(items.len());
            for item in items {
                if let Some(resp) = handle_single_message(state, item).await {
                    responses.push(resp);
                }
            }
            if responses.is_empty() {
                McpDispatchOutcome::NoResponse
            } else {
                McpDispatchOutcome::Response(Value::Array(responses))
            }
        }
        _ => McpDispatchOutcome::Response(jsonrpc_error(
            &Value::Null,
            JSONRPC_INVALID_REQUEST,
            "Invalid Request: expected JSON-RPC object or batch array",
        )),
    }
}

async fn handle_single_message(state: &AppState, msg: Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(jsonrpc_error(
            &Value::Null,
            JSONRPC_INVALID_REQUEST,
            "Invalid Request: batch entry must be an object",
        ));
    };

    let has_id = obj.contains_key("id");
    let req_id = obj.get("id").cloned().unwrap_or(Value::Null);

    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(jsonrpc_error(
            &req_id,
            JSONRPC_INVALID_REQUEST,
            "Invalid Request: `jsonrpc` must be \"2.0\"",
        ));
    }

    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // Client-sent JSON-RPC responses (no `method`, has `result` or `error`) produce no reply.
        if obj.contains_key("result") || obj.contains_key("error") {
            return None;
        }
        return Some(jsonrpc_error(
            &req_id,
            JSONRPC_INVALID_REQUEST,
            "Invalid Request: missing string `method`",
        ));
    };

    // JSON-RPC notifications omit `id` and must never emit a response frame.
    if !has_id {
        return None;
    }

    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));

    let dispatched = match method {
        "initialize" => Ok(handle_initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => handle_tools_call(state, &params).await,
        "resources/list" => handle_resources_list(state).await,
        "resources/templates/list" => Ok(handle_resource_templates_list()),
        "resources/read" => handle_resources_read(state, &params).await,
        "prompts/list" => Ok(handle_prompts_list()),
        "prompts/get" => handle_prompts_get(&params),
        _ => Err(RpcFailure {
            code: JSONRPC_METHOD_NOT_FOUND,
            message: format!("Method not found: `{method}`"),
        }),
    };

    Some(match dispatched {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": result,
        }),
        Err(err) => jsonrpc_error(&req_id, err.code, err.message),
    })
}

struct RpcFailure {
    code: i64,
    message: String,
}

fn jsonrpc_error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        }
    })
}

fn handle_initialize(params: &Value) -> Value {
    let negotiated = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .and_then(|requested| {
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .copied()
                .find(|&v| v == requested)
        })
        .unwrap_or(MCP_PROTOCOL_VERSION);

    json!({
        "protocolVersion": negotiated,
        "capabilities": {
            "tools": {
                "listChanged": false
            },
            "resources": {
                "subscribe": false,
                "listChanged": false
            },
            "prompts": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "planar",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Planar is a read-only 2D (Lines × Time) work-plane context engine across Claude Code, Codex, Antigravity, and DSH. Traverse context progressively: (1) `planar_status` (L0 overview + workspace `cwd` match + live sibling sessions), (2) `planar_line` (batch 1..=8 Lines for Line Briefs, digests, and dependencies), and (3) `planar_session` (bounded batch 1..=3 tool-stripped Head-Middle-Tail slices; pass `focus: \"handoff\"` to omit `head` and save tokens) or `planar_search` (local FTS5 recall)."
    })
}

fn read_only_tool_annotations(title: &str) -> Value {
    json!({
        "title": title,
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    })
}

fn handle_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "planar_status",
                "description": "L0 progressive-disclosure plane snapshot (~300-600 tokens): active Lines of work with priority, status, active agent time, and token/cost totals, optional `cwd` workspace matching, live sibling `has_open_turn` sessions, Sync queue, and cross-session dependencies.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "days": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 30,
                            "default": 7,
                            "description": "Lookback window in days (1..=30, default 7)."
                        },
                        "cwd": {
                            "type": "string",
                            "description": "Optional caller workspace path to highlight matching Lines and live sibling sessions in the same repository."
                        }
                    },
                    "additionalProperties": false
                },
                "annotations": read_only_tool_annotations("Planar Status (L0)")
            },
            {
                "name": "planar_line",
                "description": "L1 batch Line drill-down (`1..=8` Lines, partial-match tolerant). Resolves each entry by exact slug `id` or case-insensitive Line `name`, returning human `Line Brief` (`description`), priority, status, active agent time, ordered session digests (`title`, `stopped_at`, `left_open`), and upstream/downstream session dependencies.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "lines": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "maxItems": MAX_L1_BATCH_LINES,
                            "description": "1 to 8 Line slug IDs or case-insensitive Line names to fetch in one round-trip."
                        },
                        "limit_per_line": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 50,
                            "default": 8,
                            "description": "Maximum recent sessions returned per Line (default 8)."
                        }
                    },
                    "required": ["lines"],
                    "additionalProperties": false
                },
                "annotations": read_only_tool_annotations("Planar Line Drill-Down (L1)")
            },
            {
                "name": "planar_session",
                "description": "L2 bounded-batch trajectory slicer (`1..=3` sessions). Resolves exact session IDs or unambiguous ID prefixes/suffixes, strips all raw tool outputs and internal digest prompts, and returns Head-Middle-Tail dialogue slices plus raw `source_path`. Pass `focus: \"handoff\"` to omit `head` and halve token usage.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_ids": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "maxItems": MAX_L2_BATCH_SESSIONS,
                            "description": "1 to 3 session IDs (or unambiguous ID prefixes/suffixes) to inspect."
                        },
                        "focus": {
                            "type": "string",
                            "enum": ["full", "handoff"],
                            "default": "full",
                            "description": "`full` returns Head + Middle user prompts + Tail; `handoff` returns stored Digest + Middle user prompts + Tail."
                        }
                    },
                    "required": ["session_ids"],
                    "additionalProperties": false
                },
                "annotations": read_only_tool_annotations("Planar Session Slicer (L2)")
            },
            {
                "name": "planar_search",
                "description": "Local SQLite FTS5 full-text search across session `title`, `stopped_at`, `left_open`, and `first_user_prompt`, returning ranked compact hits without external embedding calls.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Keywords or phrase to search across session digests and opening prompts."
                        },
                        "line": {
                            "type": "string",
                            "description": "Optional Line slug ID or name filter."
                        },
                        "days": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 3650,
                            "description": "Optional lookback window in days."
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 50,
                            "default": 10,
                            "description": "Maximum ranked hits to return (default 10, max 50)."
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                },
                "annotations": read_only_tool_annotations("Planar FTS5 Search")
            }
        ]
    })
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct StatusToolArgs {
    days: Option<u32>,
    cwd: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LineToolArgs {
    lines: Vec<String>,
    limit_per_line: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionToolArgs {
    session_ids: Vec<String>,
    focus: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchToolArgs {
    query: String,
    line: Option<String>,
    days: Option<u32>,
    limit: Option<usize>,
}

fn tool_text_result(text: &str, is_error: bool) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "isError": is_error
    })
}

async fn refresh_trajectories_throttled(state: &AppState) {
    if let Err(err) = state.maybe_throttled_scan(false).await {
        eprintln!("planar mcp incremental scan warning: {err}");
    }
}

async fn handle_tools_call(
    state: &AppState,
    params: &Value,
) -> std::result::Result<Value, RpcFailure> {
    let Some(tool_name) = params.get("name").and_then(Value::as_str) else {
        return Err(RpcFailure {
            code: JSONRPC_INVALID_PARAMS,
            message: "Invalid params: `name` is required for `tools/call`".into(),
        });
    };

    if !matches!(
        tool_name,
        "planar_status" | "planar_line" | "planar_session" | "planar_search"
    ) {
        return Err(RpcFailure {
            code: JSONRPC_INVALID_PARAMS,
            message: format!(
                "Unknown tool `{tool_name}` (Planar MCP v1 is strictly read-only: `planar_status`, `planar_line`, `planar_session`, `planar_search`)"
            ),
        });
    }

    refresh_trajectories_throttled(state).await;

    let raw_args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let outcome: Result<String> = match tool_name {
        "planar_status" => match serde_json::from_value::<StatusToolArgs>(raw_args) {
            Ok(args) => {
                if args.days == Some(0) || args.days.is_some_and(|d| d > 30) {
                    Err(PlanarError::InvalidInput(
                        "`days` must be between 1 and 30".into(),
                    ))
                } else {
                    let days = args.days.map_or(7_usize, |d| d as usize);
                    let cwd = args.cwd;
                    state
                        .with_store(move |store| {
                            let snap = store.build_status_snapshot_with_cwd(
                                days,
                                cwd.as_deref(),
                                Local::now(),
                            )?;
                            Ok(render_l0_status(&snap))
                        })
                        .await
                }
            }
            Err(err) => Err(PlanarError::InvalidInput(format!(
                "invalid `planar_status` arguments: {err}"
            ))),
        },
        "planar_line" => match serde_json::from_value::<LineToolArgs>(raw_args) {
            Ok(args) => {
                if args.limit_per_line == Some(0) || args.limit_per_line.is_some_and(|n| n > 50) {
                    Err(PlanarError::InvalidInput(
                        "`limit_per_line` must be between 1 and 50".into(),
                    ))
                } else {
                    let lines = args.lines;
                    let limit_per_line = args.limit_per_line.unwrap_or(8);
                    state
                        .with_store(move |store| {
                            let resp = store.query_lines(&lines, limit_per_line, Local::now())?;
                            Ok(render_l1_lines(&resp))
                        })
                        .await
                }
            }
            Err(err) => Err(PlanarError::InvalidInput(format!(
                "invalid `planar_line` arguments: {err}"
            ))),
        },
        "planar_session" => match serde_json::from_value::<SessionToolArgs>(raw_args) {
            Ok(args) => {
                let focus_res = match args.focus.as_deref() {
                    None => Ok(SessionFocus::Full),
                    Some(raw_f) => SessionFocus::parse_str(raw_f).ok_or_else(|| {
                        PlanarError::InvalidInput(format!(
                            "invalid `focus` value `{raw_f}`: expected `full` or `handoff`"
                        ))
                    }),
                };
                match focus_res {
                    Ok(focus) => {
                        let sids = args.session_ids;
                        state
                            .with_store(move |store| {
                                let resp = store.query_sessions(&sids, focus, Local::now())?;
                                Ok(render_l2_sessions(&resp))
                            })
                            .await
                    }
                    Err(err) => Err(err),
                }
            }
            Err(err) => Err(PlanarError::InvalidInput(format!(
                "invalid `planar_session` arguments: {err}"
            ))),
        },
        "planar_search" => match serde_json::from_value::<SearchToolArgs>(raw_args) {
            Ok(args) => {
                if args.days == Some(0) || args.days.is_some_and(|d| d > 3650) {
                    Err(PlanarError::InvalidInput(
                        "`days` must be between 1 and 3650".into(),
                    ))
                } else if args.limit == Some(0) || args.limit.is_some_and(|l| l > 50) {
                    Err(PlanarError::InvalidInput(
                        "`limit` must be between 1 and 50".into(),
                    ))
                } else {
                    let req = SearchQuery {
                        query: args.query,
                        line: args.line,
                        days: args.days.map(|d| d as usize),
                        limit: args.limit.unwrap_or(10),
                    };
                    state
                        .with_store(move |store| {
                            let resp = store.search_sessions(&req, Local::now())?;
                            Ok(render_search_results(&resp))
                        })
                        .await
                }
            }
            Err(err) => Err(PlanarError::InvalidInput(format!(
                "invalid `planar_search` arguments: {err}"
            ))),
        },
        _ => unreachable!(),
    };

    match outcome {
        Ok(text) => Ok(tool_text_result(&text, false)),
        Err(PlanarError::InvalidInput(msg) | PlanarError::NotFound(msg)) => {
            Ok(tool_text_result(&format!("Error: {msg}"), true))
        }
        Err(other) => Err(RpcFailure {
            code: JSONRPC_INTERNAL_ERROR,
            message: other.to_string(),
        }),
    }
}

async fn handle_resources_list(state: &AppState) -> std::result::Result<Value, RpcFailure> {
    refresh_trajectories_throttled(state).await;

    let catalog = state
        .with_store(|store| store.list_line_catalog())
        .await
        .map_err(|err| RpcFailure {
            code: JSONRPC_INTERNAL_ERROR,
            message: err.to_string(),
        })?;

    let mut resources = Vec::with_capacity(catalog.len() + 2);
    resources.push(json!({
        "uri": "planar://status",
        "name": "Planar Plane Status (L0)",
        "description": "7-day Lines × Time overview, active agent time, Sync queue, and session dependencies.",
        "mimeType": "text/markdown"
    }));

    for entry in catalog {
        let desc = if entry.description.trim().is_empty() {
            format!("Line `{}` ({} sessions)", entry.name, entry.session_count)
        } else {
            format!(
                "Line `{}` ({} sessions) — {}",
                entry.name,
                entry.session_count,
                entry.description.trim()
            )
        };
        resources.push(json!({
            "uri": format!("planar://line/{}", percent_encode_segment(&entry.id)),
            "name": format!("Line: {}", entry.name),
            "description": desc,
            "mimeType": "text/markdown"
        }));
    }

    resources.push(json!({
        "uri": format!("planar://line/{ONE_OFF_LINE_ID}"),
        "name": "Line: One-off",
        "description": "System bucket for standalone one-off agent tasks.",
        "mimeType": "text/markdown"
    }));

    Ok(json!({ "resources": resources }))
}

fn handle_resource_templates_list() -> Value {
    json!({
        "resourceTemplates": [
            {
                "uriTemplate": "planar://line/{slug}",
                "name": "Planar Line Drill-Down (L1)",
                "description": "Line Brief (`description`), priority, active agent time, ordered session digests, and dependency links for a Line slug or name.",
                "mimeType": "text/markdown"
            }
        ]
    })
}

async fn handle_resources_read(
    state: &AppState,
    params: &Value,
) -> std::result::Result<Value, RpcFailure> {
    let Some(uri) = params.get("uri").and_then(Value::as_str).map(str::trim) else {
        return Err(RpcFailure {
            code: JSONRPC_INVALID_PARAMS,
            message: "Invalid params: `uri` is required for `resources/read`".into(),
        });
    };

    refresh_trajectories_throttled(state).await;

    if uri == "planar://status" {
        let text = state
            .with_store(|store| {
                let snap = store.build_status_snapshot(7, Local::now())?;
                Ok(render_l0_status(&snap))
            })
            .await
            .map_err(|err| RpcFailure {
                code: JSONRPC_INTERNAL_ERROR,
                message: err.to_string(),
            })?;
        return Ok(json!({
            "contents": [
                {
                    "uri": uri,
                    "mimeType": "text/markdown",
                    "text": text
                }
            ]
        }));
    }

    if let Some(raw_slug) = uri.strip_prefix("planar://line/") {
        let decoded_slug = percent_decode_segment(raw_slug);
        if decoded_slug.trim().is_empty() {
            return Err(RpcFailure {
                code: JSONRPC_INVALID_PARAMS,
                message: "Invalid params: empty line slug in `planar://line/{slug}`".into(),
            });
        }
        let target = decoded_slug.trim().to_string();
        let resp = state
            .with_store(move |store| store.query_lines(&[target], 8, Local::now()))
            .await
            .map_err(|err| RpcFailure {
                code: JSONRPC_INTERNAL_ERROR,
                message: err.to_string(),
            })?;
        if resp.lines.is_empty() {
            return Err(RpcFailure {
                code: MCP_RESOURCE_NOT_FOUND,
                message: format!("Resource not found: `{uri}`"),
            });
        }
        let text = render_l1_lines(&resp);
        return Ok(json!({
            "contents": [
                {
                    "uri": uri,
                    "mimeType": "text/markdown",
                    "text": text
                }
            ]
        }));
    }

    Err(RpcFailure {
        code: MCP_RESOURCE_NOT_FOUND,
        message: format!("Resource not found: `{uri}`"),
    })
}

fn handle_prompts_list() -> Value {
    json!({
        "prompts": [
            {
                "name": "planar-plan",
                "description": "Guided progressive-disclosure planning workflow (`L0 planar_status -> batch L1 planar_line -> optional L2 planar_session`) producing a sharp next-action synthesis.",
                "arguments": [
                    {
                        "name": "days",
                        "description": "Lookback window in days (default: 7).",
                        "required": false
                    },
                    {
                        "name": "cwd",
                        "description": "Caller workspace path to highlight matching Lines and live sibling sessions.",
                        "required": false
                    },
                    {
                        "name": "focus_line",
                        "description": "Optional specific Line slug or name to center planning around.",
                        "required": false
                    }
                ]
            }
        ]
    })
}

fn handle_prompts_get(params: &Value) -> std::result::Result<Value, RpcFailure> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(RpcFailure {
            code: JSONRPC_INVALID_PARAMS,
            message: "Invalid params: `name` is required for `prompts/get`".into(),
        });
    };
    if name != "planar-plan" {
        return Err(RpcFailure {
            code: JSONRPC_INVALID_PARAMS,
            message: format!("Unknown prompt `{name}`"),
        });
    }

    let args = params.get("arguments");
    let days = args
        .and_then(|a| a.get("days"))
        .and_then(|v| {
            v.as_str()
                .map(ToString::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "7".to_string());
    let cwd_clause = args
        .and_then(|a| a.get("cwd"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(String::new, |c| format!(" with `cwd: \"{c}\"`"));
    let focus_clause = args
        .and_then(|a| a.get("focus_line"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(String::new, |fl| {
            format!(" Include `{fl}` in your L1 batch.")
        });

    let prompt_text = format!(
        "Review my recent multi-harness agent work using Planar's progressive-disclosure MCP tools in at most 2-3 tool calls (< 3k tokens) — never read raw `.jsonl` transcripts directly:\n\n\
1. **L0 Plane Snapshot**: Call `planar_status` (`days: {days}`{cwd_clause}) to inspect active Lines, priorities, live `has_open_turn` sibling sessions, and Priority vs. Time drift.\n\
2. **Batch L1 Line Drill-Down**: Call `planar_line` once with an array of relevant active / `P1 Urgent` / `P2 High` / `cwd`-matched Line slugs (`1..=8` lines in a single batch).{focus_clause} Pay close attention to each Line's human `Line Brief`, session `Stopped at` / `Left open` notes, and upstream/downstream dependencies.\n\
3. **Optional Bounded L2 Handoff**: Only if a key blocker or handoff requires deeper context, call `planar_session` (`1..=3` session IDs, `focus: \"handoff\"`), or `planar_search` for keyword recall.\n\n\
Synthesize a concise **Planning Brief** with three sections:\n\
1. **Unblocked / 80%-done sessions to close**: Concrete threads whose upstream dependency settled or have a small `Left open` step.\n\
2. **Priority vs. Time drift**: Any mismatch between declared `P1`/`P2` priorities (or `Line Brief` intent) and where agent time actually went.\n\
3. **Concrete next session to resume or spawn**: One recommended single focus (citing exact Line + session ID) that minimizes context thrashing and avoids colliding with live sibling sessions."
    );

    Ok(json!({
        "description": "Guided Planar progressive-disclosure planning rubric",
        "messages": [
            {
                "role": "user",
                "content": {
                    "type": "text",
                    "text": prompt_text
                }
            }
        ]
    }))
}

fn percent_encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn percent_decode_segment(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0_usize;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            decoded.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;
    use tokio::io::BufReader;

    use super::*;
    use crate::db::{LineUpdate, Store};
    use crate::env::HarnessRoots;
    use crate::model::INTERNAL_DIGEST_SENTINEL;

    fn test_roots(base: &Path) -> HarnessRoots {
        HarnessRoots {
            claude_projects: base.join("claude"),
            codex_sessions: base.join("codex").join("sessions"),
            codex_archived: base.join("codex").join("archived_sessions"),
            antigravity_brain: base.join("antigravity"),
            dsh_sessions: base.join("dsh").join("sessions"),
        }
    }

    #[tokio::test]
    async fn mcp_stdio_lifecycle_tools_resources_prompts_and_auto_scan() {
        let dir = TempDir::new().unwrap();
        let roots = test_roots(dir.path());
        let sess_dir = roots.codex_sessions.join("2026").join("11").join("20");
        fs::create_dir_all(&sess_dir).unwrap();

        let now = Local::now();
        let t1 = (now - chrono::Duration::minutes(10)).to_rfc3339();
        let t2 = (now - chrono::Duration::minutes(8)).to_rfc3339();
        let t3 = (now - chrono::Duration::minutes(5)).to_rfc3339();

        let rollout = format!(
            concat!(
                "{{\"timestamp\":\"{t1}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"mcp-live-1\",\"cwd\":\"/tmp/planar-repo\",\"source\":\"cli\"}}}}\n",
                "{{\"timestamp\":\"{t1}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\"}}}}\n",
                "{{\"timestamp\":\"{t1}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Audit MCP Streamable HTTP transport\"}}]}}}}\n",
                "{{\"timestamp\":\"{t2}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"custom_tool_call_output\",\"output\":[{{\"text\":\"SECRET_RAW_TOOL_DUMP_777\"}}]}}}}\n",
                "{{\"timestamp\":\"{t2}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{sentinel} hidden internal digest prompt\"}}]}}}}\n",
                "{{\"timestamp\":\"{t3}\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"Verified 2025-03-26 Stdio and HTTP parity.\"}}]}}}}\n"
            ),
            t1 = t1,
            t2 = t2,
            t3 = t3,
            sentinel = INTERNAL_DIGEST_SENTINEL,
        );
        fs::write(sess_dir.join("rollout-mcp.jsonl"), rollout).unwrap();

        let db_path = dir.path().join("planar.db");
        {
            let store = Store::open(&db_path).unwrap();
            let lid = store
                .create_line_with_description("Planar", "Core Rust + Tauri + MCP engine")
                .unwrap();
            store
                .update_line(
                    &lid,
                    &LineUpdate {
                        priority: Some(1),
                        ..LineUpdate::default()
                    },
                )
                .unwrap();
        }

        // Hold a second live Store connection open concurrently to prove WAL multi-connection safety.
        let _concurrent_store = Store::open(&db_path).unwrap();

        let state = AppState::new(db_path.clone(), roots);

        let requests = [
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "test-harness", "version": "1.0.0" }
                }
            }),
            // Notification must produce ZERO output frames on stdout.
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "ping"
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/list"
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "planar_status",
                    "arguments": { "days": 7, "cwd": "/tmp/planar-repo" }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "planar_line",
                    "arguments": { "lines": ["Planar", "nonexistent-slug"], "limit_per_line": 5 }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "planar_session",
                    "arguments": { "session_ids": ["mcp-live-1"], "focus": "handoff" }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": {
                    "name": "planar_search",
                    "arguments": { "query": "Streamable HTTP", "days": 7 }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {
                    "name": "planar_session",
                    "arguments": { "session_ids": ["s1", "s2", "s3", "s4"] }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "resources/list"
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "resources/templates/list"
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "resources/read",
                "params": { "uri": "planar://line/planar" }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 12,
                "method": "prompts/list"
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 13,
                "method": "prompts/get",
                "params": {
                    "name": "planar-plan",
                    "arguments": { "days": "7", "focus_line": "planar" }
                }
            }),
        ];

        let mut input_bytes = Vec::new();
        for req in &requests {
            input_bytes.extend_from_slice(serde_json::to_string(req).unwrap().as_bytes());
            input_bytes.push(b'\n');
        }

        let mut output_bytes = Vec::new();
        serve_stdio(
            state,
            BufReader::new(input_bytes.as_slice()),
            &mut output_bytes,
        )
        .await
        .unwrap();

        let output_str = std::str::from_utf8(&output_bytes).unwrap();
        let responses: Vec<Value> = output_str
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect();

        // 14 input frames minus 1 notification = exactly 13 JSON-RPC response lines!
        assert_eq!(responses.len(), 13);

        // id=1 initialize
        assert_eq!(responses[0]["id"], 1);
        assert_eq!(responses[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(responses[0]["result"]["serverInfo"]["name"], "planar");

        // id=2 ping
        assert_eq!(responses[1]["id"], 2);
        assert_eq!(responses[1]["result"], json!({}));

        // id=3 tools/list: strictly 4 read-only tools, zero `oneOf`/`anyOf`
        let tools = responses[2]["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
        let tools_raw = serde_json::to_string(tools).unwrap();
        assert!(!tools_raw.contains("oneOf"));
        assert!(!tools_raw.contains("anyOf"));
        for t in tools {
            assert_eq!(t["annotations"]["readOnlyHint"], true);
            assert_eq!(t["annotations"]["destructiveHint"], false);
        }

        // id=4 planar_status triggered auto-scan and detected live open-turn session in same cwd!
        let status_txt = responses[3]["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(responses[3]["result"]["isError"], false);
        assert!(status_txt.contains("codex:mcp-live-1"));
        assert!(status_txt.contains("same cwd"));
        assert!(status_txt.contains("open turn"));

        // id=5 planar_line partial-match tolerant + returns Line Brief (`description`)
        let line_txt = responses[4]["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(responses[4]["result"]["isError"], false);
        assert!(line_txt.contains("Core Rust + Tauri + MCP engine"));
        assert!(line_txt.contains("Not found: `nonexistent-slug`"));

        // id=6 planar_session handoff strips raw tool payloads & internal sentinel
        let sess_txt = responses[5]["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(responses[5]["result"]["isError"], false);
        assert!(sess_txt.contains("Verified 2025-03-26 Stdio and HTTP parity."));
        assert!(sess_txt.contains("Resume: `codex resume 'mcp-live-1'`"));
        assert!(!sess_txt.contains("HEAD (Opening turns)"));
        assert!(!sess_txt.contains("SECRET_RAW_TOOL_DUMP_777"));
        assert!(!sess_txt.contains(INTERNAL_DIGEST_SENTINEL));

        // id=7 planar_search FTS5 hit
        let search_txt = responses[6]["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(responses[6]["result"]["isError"], false);
        assert!(search_txt.contains("codex:mcp-live-1"));

        // id=8 >3 sessions returns tool execution error (`isError: true`)
        assert_eq!(responses[7]["result"]["isError"], true);
        assert!(
            responses[7]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("between 1 and 3")
        );

        // id=9..13 resources & prompts
        assert!(
            responses[8]["result"]["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["uri"] == "planar://status")
        );
        assert_eq!(
            responses[9]["result"]["resourceTemplates"][0]["uriTemplate"],
            "planar://line/{slug}"
        );
        assert!(
            responses[10]["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Core Rust + Tauri + MCP engine")
        );
        assert_eq!(responses[11]["result"]["prompts"][0]["name"], "planar-plan");
        assert!(
            responses[12]["result"]["messages"][0]["content"]["text"]
                .as_str()
                .unwrap()
                .contains("Unblocked / 80%-done sessions to close")
        );
    }
}
