//! `rwpm mcp` end to end: the real binary, speaking JSON-RPC over its stdin and stdout.
//!
//! The data folders are redirected into a temp dir. The dirs crate honours these variables
//! on Linux only, so the test is Linux-only; the first test checks the redirect worked
//! before anything is written.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Session {
    fn start(home: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rwpm"))
            .arg("mcp")
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn rwpm mcp");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut session = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = session.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "rwpm-test", "version": "0" },
            }),
        );
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        session.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        session
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{message}").expect("write request");
        stdin.flush().expect("flush");
    }

    /// Sends a request and returns its reply. Every stdout line must be JSON: nothing else
    /// may reach the protocol stream.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read reply");
            assert!(read > 0, "rwpm mcp closed stdout before answering {method}");
            let reply: Value =
                serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"));
            if reply["id"] == json!(id) {
                return reply;
            }
        }
    }

    /// Calls a tool; returns whether it failed and the first text block.
    fn tool(&mut self, name: &str, arguments: Value) -> (bool, String) {
        let reply = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        (result["isError"] == json!(true), text.to_owned())
    }

    fn json_tool(&mut self, name: &str, arguments: Value) -> Value {
        let (failed, text) = self.tool(name, arguments);
        assert!(!failed, "{name} failed: {text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} text not JSON ({e}): {text}"))
    }

    fn finish(mut self) {
        drop(self.stdin.take());
        let status = self.child.wait().expect("wait");
        assert!(status.success(), "rwpm mcp exited with {status}");
    }
}

#[test]
fn it_lists_its_tools_and_works_inside_the_sandbox() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut s = Session::start(tmp.path());

    let tools = s.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for wanted in [
        "list_profiles",
        "get_profile",
        "profile_mods",
        "profile_health",
        "last_run",
        "game_install",
        "create_profile",
        "clone_profile",
        "rename_profile",
    ] {
        assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
    }
    for forbidden in ["delete_profile", "launch", "run"] {
        assert!(!names.contains(&forbidden), "{forbidden} must not exist");
    }

    // The redirect worked: the built-in profile points inside the temp dir.
    let listed = s.json_tool("list_profiles", json!({}));
    let default_dir = listed["profiles"][0]["save_data_dir"]
        .as_str()
        .expect("default save dir");
    assert!(
        default_dir.starts_with(tmp.path().to_str().expect("utf-8")),
        "sandbox leaked: {default_dir}"
    );

    let made = s.json_tool("create_profile", json!({ "name": "MCP test" }));
    let id = made["id"].as_str().expect("id").to_owned();
    let renamed = s.json_tool("rename_profile", json!({ "id": id, "name": "MCP renamed" }));
    assert_eq!(renamed["name"], "MCP renamed");
    // A real RimWorld running on this machine makes the clone refuse; that is correct too.
    let (failed, text) = s.tool(
        "clone_profile",
        json!({ "source_id": id, "name": "MCP copy" }),
    );
    let copied = !failed;
    if copied {
        assert_eq!(
            serde_json::from_str::<Value>(&text).expect("json")["name"],
            "MCP copy"
        );
    } else {
        assert!(text.contains("RimWorld is running"), "{text}");
    }

    let (failed, text) = s.tool("get_profile", json!({ "id": "does-not-exist" }));
    assert!(failed, "{text}");
    let after = s.json_tool("list_profiles", json!({}));
    let expected = if copied { 3 } else { 2 };
    assert_eq!(after["profiles"].as_array().expect("array").len(), expected);
    s.finish();
}
