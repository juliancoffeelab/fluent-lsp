use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use std::convert::TryFrom;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("workspace")
}

struct LspProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LspProcess {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fluent-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start fluent-lsp");

        Self {
            stdin: child.stdin.take().expect("missing stdin"),
            stdout: BufReader::new(child.stdout.take().expect("missing stdout")),
            child,
        }
    }

    fn send(&mut self, message: &Value) {
        let body = serde_json::to_vec(message).expect("serialize LSP message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.stdin.write_all(&body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(length) = line.strip_prefix("Content-Length: ") {
                content_length = Some(length.trim().parse::<usize>().unwrap());
            }
        }

        let mut body = vec![0; content_length.expect("missing content length")];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).expect("parse LSP response")
    }
}

impl Drop for LspProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn goto_definition_from_translation_resolves_to_english_fluent_file() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 1);
    assert_eq!(
        initialize["result"]["capabilities"]["definitionProvider"],
        Value::Bool(true)
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));

    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": source_uri,
                "languageId": "fluent",
                "version": 1,
                "text": source_text
            }
        }
    }));

    assert_definition(
        &mut lsp,
        2,
        &source_path,
        position_of(&source_text, "welcome-title"),
        &root.join("locales/en/app.ftl"),
        0,
        0,
    );

    assert_definition(
        &mut lsp,
        3,
        &source_path,
        position_of(&source_text, "brand-name"),
        &root.join("locales/en/app.ftl"),
        1,
        1,
    );

    assert_definition(
        &mut lsp,
        4,
        &source_path,
        position_of(&source_text, "label ="),
        &root.join("locales/en/app.ftl"),
        3,
        5,
    );
}

fn assert_definition(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
    expected_target: &Path,
    expected_line: u32,
    expected_character: u32,
) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));

    let definition = lsp.recv();
    assert_eq!(definition["id"], request_id);
    assert_eq!(
        definition["result"]["uri"],
        Value::String(format!("file://{}", expected_target.display()))
    );
    assert_eq!(
        definition["result"]["range"]["start"]["line"],
        Value::from(expected_line),
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        Value::from(expected_character),
    );
}

fn position_of(source: &str, needle: &str) -> (u32, u32) {
    let offset = source.find(needle).expect("needle not found in source");
    let prefix = &source[..offset];
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).unwrap();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let character = u32::try_from(source[line_start..offset].chars().count()).unwrap();
    (line, character)
}
