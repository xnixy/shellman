//! A minimal MCP client for 1Password's local server (`1password-mcp`, which the 1Password
//! desktop app provides): JSON-RPC over the child's stdin and stdout, one request per line.
//! Requests are never printed: one of them carries the private key.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Client {
    pub fn start() -> Result<Self, String> {
        let mut child = Command::new("1password-mcp")
            .env("OP_MCP_SOURCE", "plugin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start 1password-mcp ({e}); is the 1Password app installed, with 'Integrate with MCP clients' on?"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut c = Client { child, stdin, stdout, next: 1 };
        c.request("initialize", json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "shellman-xtask", "version": "0.1.0" }
        }))?;
        c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        Ok(c)
    }

    fn send(&mut self, v: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(v).map_err(|e| e.to_string())?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).and_then(|_| self.stdin.flush()).map_err(|e| format!("1password-mcp went away ({e})"))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        loop {
            let mut line = String::new();
            if self.stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("1password-mcp ended without answering".into());
            }
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
            // Notifications and requests from the server (no matching id) are skipped.
            if v.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(e) = v.get("error") {
                return Err(format!("1Password: {}", e.get("message").and_then(Value::as_str).unwrap_or("error")));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Call a tool; its text answer, parsed as JSON when it is JSON.
    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        let r = self.request("tools/call", json!({ "name": tool, "arguments": args }))?;
        let text: String = r.get("content").and_then(Value::as_array).map(|a| a.iter().filter_map(|c| c.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        if r.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(format!("1Password {tool}: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The first string found under any of `keys`, anywhere in `v`.
pub fn find_str(v: &Value, keys: &[&str]) -> Option<String> {
    match v {
        Value::Object(m) => {
            for k in keys {
                if let Some(s) = m.get(*k).and_then(Value::as_str) {
                    return Some(s.to_string());
                }
            }
            m.values().find_map(|x| find_str(x, keys))
        }
        Value::Array(a) => a.iter().find_map(|x| find_str(x, keys)),
        _ => None,
    }
}

/// Every object in `v` (at any depth) that has a string `key`.
pub fn objects_with<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a serde_json::Map<String, Value>>) {
    match v {
        Value::Object(m) => {
            if m.get(key).is_some_and(Value::is_string) {
                out.push(m);
            }
            for x in m.values() {
                objects_with(x, key, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| objects_with(x, key, out)),
        _ => {}
    }
}
