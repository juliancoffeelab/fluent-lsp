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
    assert_eq!(
        initialize["result"]["capabilities"]["hoverProvider"],
        Value::Bool(true)
    );
    assert_eq!(
        initialize["result"]["capabilities"]["referencesProvider"],
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
        1,
        0,
    );

    assert_definition(
        &mut lsp,
        3,
        &source_path,
        position_of(&source_text, "brand-name"),
        &root.join("locales/en/app.ftl"),
        5,
        1,
    );

    assert_definition(
        &mut lsp,
        4,
        &source_path,
        position_of(&source_text, "label ="),
        &root.join("locales/en/app.ftl"),
        10,
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

#[test]
fn references_from_english_resolve_to_translated_fluent_files() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 10);
    assert_eq!(
        initialize["result"]["capabilities"]["referencesProvider"],
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

    assert_references(
        &mut lsp,
        11,
        &source_path,
        position_of(&source_text, "welcome-title"),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 0, 0),
            ReferenceExpectation::new("locales/fr/app.ftl", 0, 0),
        ],
    );

    assert_references(
        &mut lsp,
        12,
        &source_path,
        position_of(&source_text, "brand-name"),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 1, 1),
            ReferenceExpectation::new("locales/fr/app.ftl", 1, 1),
        ],
    );

    assert_references(
        &mut lsp,
        13,
        &source_path,
        position_of(&source_text, "label ="),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 3, 5),
            ReferenceExpectation::new("locales/fr/app.ftl", 3, 5),
        ],
    );
}

#[test]
fn hover_from_translation_shows_english_entry_and_comments() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 20);
    assert_eq!(
        initialize["result"]["capabilities"]["hoverProvider"],
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

    assert_hover(
        &mut lsp,
        21,
        &source_path,
        position_of(&source_text, "welcome-title"),
        "```ftl\n# Shown on the welcome screen\nwelcome-title = Welcome\n```",
        0,
        0,
    );

    assert_hover(
        &mut lsp,
        22,
        &source_path,
        position_of(&source_text, "label ="),
        "```ftl\n# CTA copy\n# Keep it short\nbutton-copy =\n    .label = Launch\n```",
        3,
        5,
    );

    assert_hover(
        &mut lsp,
        23,
        &source_path,
        position_of(&source_text, "install-hint"),
        "```ftl\n# Shortcut combinations\ninstall-hint =\n    { $platform ->\n        [macos] Press Command\n       *[other] Press Ctrl\n    } + { $action ->\n        [copy] C\n       *[paste] V\n    }\n```\n\nStatic combinations:\n- `$platform=macos`, `$action=copy`: `Press Command + C`\n- `$platform=macos`, `$action=paste`: `Press Command + V`\n- `$platform=other`, `$action=copy`: `Press Ctrl + C`\n- `...`: 1 more",
        4,
        0,
    );
}

struct ReferenceExpectation<'a> {
    relative_path: &'a str,
    line: u32,
    character: u32,
}

impl<'a> ReferenceExpectation<'a> {
    fn new(relative_path: &'a str, line: u32, character: u32) -> Self {
        Self {
            relative_path,
            line,
            character,
        }
    }
}

fn assert_references(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
    expected: &[ReferenceExpectation<'_>],
) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 },
            "context": { "includeDeclaration": false }
        }
    }));

    let references = lsp.recv();
    assert_eq!(references["id"], request_id);
    let items = references["result"].as_array().expect("expected references array");
    assert_eq!(items.len(), expected.len());

    for (item, expected) in items.iter().zip(expected.iter()) {
        let uri = item["uri"].as_str().unwrap();
        assert!(
            uri.ends_with(expected.relative_path),
            "unexpected reference uri: {uri}"
        );
        assert_eq!(item["range"]["start"]["line"], Value::from(expected.line));
        assert_eq!(
            item["range"]["start"]["character"],
            Value::from(expected.character)
        );
    }
}

fn assert_hover(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
    expected_value: &str,
    expected_line: u32,
    expected_character: u32,
) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));

    let hover = lsp.recv();
    assert_eq!(hover["id"], request_id);
    assert_eq!(hover["result"]["contents"]["kind"], Value::String("markdown".to_string()));
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(expected_value.to_string())
    );
    assert_eq!(
        hover["result"]["range"]["start"]["line"],
        Value::from(expected_line),
    );
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
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
