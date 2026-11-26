//! `rewind-os-cli mcp`: a Model Context Protocol server on stdin/stdout, so an assistant can
//! use the same engine tools as the app.
//!
//! Limits, so an assistant can never do more than the person allowed:
//! - Tools that read a path only accept paths inside a `--automation-read-root` (or a write
//!   root). Without roots, they are refused.
//! - `files.trash` and `dev.clean` only accept paths inside a `--automation-write-root`, and
//!   the engine's safety guard still checks every path.
//! - Other tools that change the computer (cleanup, startup, settings, apps, uninstall,
//!   maintenance) are offered only with `--allow-system-changes`.
//! - Restarting the app as administrator is never offered.

use rewind_os_engine::{Engine, Request, TOOLS, Task, ToolSpec};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
    pub allow_system_changes: bool,
}

/// Tools never offered, whatever the flags.
const NEVER: &[&str] = &["system.relaunch_elevated", "system.run_elevated"];
/// Tools whose arguments name paths to read.
const READS_PATHS: &[&str] = &[
    "disk.scan",
    "disk.list_files",
    "disk.duplicates",
    "dev.find",
    "files.reveal",
];
/// Tools whose arguments name paths to remove.
const WRITES_PATHS: &[&str] = &["files.trash", "dev.clean"];

/// MCP tool names allow letters, digits, `_` and `-`: `disk.scan` is offered as `disk_scan`.
pub fn mcp_name(id: &str) -> String {
    id.replace('.', "_")
}

fn engine_id(mcp: &str) -> Option<&'static str> {
    TOOLS.iter().map(|t| t.id).find(|id| mcp_name(id) == mcp)
}

impl Policy {
    /// Whether the tool is offered at all on this computer, with these flags.
    pub fn offers(&self, tool: &ToolSpec) -> bool {
        if NEVER.contains(&tool.id) || !tool.available() {
            return false;
        }
        if WRITES_PATHS.contains(&tool.id) {
            return !self.write_roots.is_empty();
        }
        if READS_PATHS.contains(&tool.id) {
            return !self.read_roots.is_empty() || !self.write_roots.is_empty();
        }
        !tool.changes_system || self.allow_system_changes
    }

    /// Why the call may not run, if it may not.
    pub fn refusal(&self, tool: &str, arguments: &Value) -> Option<String> {
        let paths = |key: &str| -> Vec<PathBuf> {
            match arguments.get(key) {
                Some(Value::String(s)) => vec![PathBuf::from(s)],
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .collect(),
                _ => Vec::new(),
            }
        };
        let (wanted, roots, kind): (Vec<PathBuf>, Vec<&PathBuf>, &str) = match tool {
            "disk.scan" | "disk.duplicates" | "dev.find" => (
                paths("root"),
                self.read_roots.iter().chain(&self.write_roots).collect(),
                "--automation-read-root",
            ),
            "disk.list_files" => (
                paths("dir"),
                self.read_roots.iter().chain(&self.write_roots).collect(),
                "--automation-read-root",
            ),
            "files.reveal" => (
                paths("path"),
                self.read_roots.iter().chain(&self.write_roots).collect(),
                "--automation-read-root",
            ),
            "files.trash" | "dev.clean" => (
                paths("paths"),
                self.write_roots.iter().collect(),
                "--automation-write-root",
            ),
            _ => return None,
        };
        if wanted.is_empty() {
            return Some("a path is required".to_owned());
        }
        wanted
            .iter()
            .find(|p| !roots.iter().any(|root| inside(p, root)))
            .map(|p| format!("{} is outside every {kind}", p.display()))
    }
}

/// Whether `path` is `root` or below it, after resolving links and `..`.
pub fn inside(path: &Path, root: &Path) -> bool {
    let resolve = |p: &Path| std::fs::canonicalize(p).ok();
    match (resolve(path), resolve(root)) {
        (Some(p), Some(r)) => p.starts_with(r),
        // A path that does not exist yet cannot be read or removed; refuse it.
        _ => false,
    }
}

/// JSON Schema of each tool's arguments.
fn input_schema(id: &str) -> Value {
    let string = json!({ "type": "string" });
    let strings = json!({ "type": "array", "items": { "type": "string" } });
    let boolean = json!({ "type": "boolean" });
    let object = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    match id {
        "disk.scan" => object(json!({ "root": string }), &["root"]),
        "disk.list_files" => object(json!({ "dir": string }), &["dir"]),
        "disk.duplicates" => object(
            json!({ "root": string, "min_bytes": { "type": "integer", "minimum": 1 } }),
            &["root"],
        ),
        "files.trash" | "dev.clean" => object(json!({ "paths": strings }), &["paths"]),
        "dev.find" => object(json!({ "root": string }), &["root"]),
        "files.reveal" => object(json!({ "path": string }), &["path"]),
        "cleanup.run" => object(json!({ "categories": strings }), &["categories"]),
        "programs.uninstall" => object(json!({ "key": string }), &["key"]),
        "startup.set" => object(
            json!({ "id": string, "enabled": boolean }),
            &["id", "enabled"],
        ),
        "tweaks.set" => object(
            json!({ "id": string, "applied": boolean }),
            &["id", "applied"],
        ),
        "apps.remove" => object(json!({ "ids": strings }), &["ids"]),
        "processes.close" => object(
            json!({
                "name": string,
                "pids": { "type": "array", "items": { "type": "integer", "minimum": 0 } },
                "force": boolean
            }),
            &["name", "pids"],
        ),
        "maintenance.hibernation" => object(json!({ "enabled": boolean }), &["enabled"]),
        _ => object(json!({}), &[]),
    }
}

fn tools_list(policy: &Policy) -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .filter(|t| policy.offers(t))
        .map(|t| {
            json!({
                "name": mcp_name(t.id),
                "description": t.summary,
                "inputSchema": input_schema(t.id),
                "annotations": {
                    "readOnlyHint": !t.changes_system,
                    "destructiveHint": t.changes_system,
                },
            })
        })
        .collect();
    json!({ "tools": tools })
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn call(engine: &Engine, policy: &Policy, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(id) = engine_id(name) else {
        return text_result(format!("unknown tool: {name}"), true);
    };
    let Some(spec) = rewind_os_engine::tool(id) else {
        return text_result(format!("unknown tool: {name}"), true);
    };
    if !policy.offers(spec) {
        return text_result(
            format!("{id} is not allowed in this session (see `rewind-os-cli mcp --help`)"),
            true,
        );
    }
    if let Some(why) = policy.refusal(id, &arguments) {
        return text_result(format!("refused: {why}"), true);
    }
    let mut request = match arguments {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    request.insert("tool".to_owned(), Value::String(id.to_owned()));
    let request: Request = match serde_json::from_value(Value::Object(request)) {
        Ok(r) => r,
        Err(err) => return text_result(format!("invalid arguments: {err}"), true),
    };
    match engine.run(request, &Task::new()) {
        Ok(response) => match serde_json::to_string_pretty(&response) {
            Ok(text) => text_result(text, false),
            Err(err) => text_result(err.to_string(), true),
        },
        Err(err) => text_result(err.to_string(), true),
    }
}

/// Answers one JSON-RPC message. `None` for notifications, which get no answer.
pub fn handle(engine: &Engine, policy: &Policy, line: &str) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(m) => m,
        Err(err) => {
            return Some(json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("parse error: {err}") },
            }));
        }
    };
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "rewind-os", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Rewind OS: disk analysis, junk cleanup, leftovers, startup programs and settings. Changes go through the app's safety guard; removed files go to the recycle bin.",
        }),
        "ping" => json!({}),
        "tools/list" => tools_list(policy),
        "tools/call" => call(engine, policy, &params),
        other => {
            return Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("method not found: {other}") },
            }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Serves until stdin closes. Every line in is one JSON-RPC message; every answer is one line.
pub fn serve(policy: &Policy) -> std::io::Result<()> {
    let engine = Engine::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(answer) = handle(&engine, policy, &line) {
            writeln!(stdout, "{answer}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(policy: &Policy, message: Value) -> Value {
        handle(&Engine::new(), policy, &message.to_string()).unwrap_or(Value::Null)
    }

    fn offered(policy: &Policy) -> Vec<String> {
        ask(
            policy,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        )["result"]["tools"]
            .as_array()
            .map(|t| {
                t.iter()
                    .filter_map(|x| x["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn by_default_only_tools_that_change_nothing_are_offered() {
        let names = offered(&Policy::default());
        assert!(names.contains(&"system_overview".to_owned()));
        for never in [
            "files_trash",
            "cleanup_run",
            "disk_scan",
            "system_relaunch_elevated",
        ] {
            assert!(
                !names.contains(&never.to_owned()),
                "{never} offered by default"
            );
        }
    }

    #[test]
    fn changes_need_their_own_permission_and_relaunching_never_appears() {
        let dir = tempfile::tempdir().unwrap();
        let policy = Policy {
            write_roots: vec![dir.path().to_path_buf()],
            allow_system_changes: true,
            ..Policy::default()
        };
        let names = offered(&policy);
        assert!(names.contains(&"files_trash".to_owned()));
        assert!(names.contains(&"disk_scan".to_owned()));
        assert!(!names.contains(&"system_relaunch_elevated".to_owned()));
        assert!(names.iter().all(|n| {
            n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        }));
    }

    #[test]
    fn paths_outside_the_roots_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "x").unwrap();
        let policy = Policy {
            read_roots: vec![root.path().to_path_buf()],
            ..Policy::default()
        };
        let inner = root.path().join("a.txt");
        assert!(
            policy
                .refusal("disk.list_files", &json!({"dir": root.path()}))
                .is_none()
        );
        assert!(
            policy
                .refusal("disk.scan", &json!({"root": outside.path()}))
                .is_some()
        );
        let sneaky = root
            .path()
            .join("..")
            .join(outside.path().file_name().unwrap());
        assert!(
            policy
                .refusal("disk.scan", &json!({"root": sneaky}))
                .is_some()
        );
        // Reading somewhere is not leave to delete there.
        assert!(
            policy
                .refusal("files.trash", &json!({"paths": [inner]}))
                .is_some()
        );
    }

    #[test]
    fn a_tool_call_runs_through_the_engine() {
        let answer = ask(
            &Policy::default(),
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"system_overview","arguments":{}}}),
        );
        assert_eq!(answer["id"], 7);
        assert_eq!(answer["result"]["isError"], false);
        let refused = ask(
            &Policy::default(),
            json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"cleanup_run","arguments":{"categories":[]}}}),
        );
        assert_eq!(refused["result"]["isError"], true);
    }

    #[test]
    fn notifications_get_no_answer() {
        let none = handle(
            &Engine::new(),
            &Policy::default(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert!(none.is_none());
    }
}
