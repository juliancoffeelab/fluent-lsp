use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use fluent_syntax::parser;
use serde_json::{Value, json};
use std::convert::TryFrom;
use tempfile::{TempDir, tempdir};
use unic_langid::LanguageIdentifier;

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
    _serial_guard: MutexGuard<'static, ()>,
}

impl LspProcess {
    fn start() -> Self {
        let serial_guard = lsp_process_mutex()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            _serial_guard: serial_guard,
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
            let bytes_read = self.stdout.read_line(&mut line).unwrap();
            if bytes_read == 0 {
                let status = self.child.try_wait().unwrap();
                panic!("unexpected EOF from fluent-lsp; child status: {status:?}");
            }
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

fn lsp_process_mutex() -> &'static Mutex<()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

impl Drop for LspProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn goto_definition_from_translation_resolves_to_origin_fluent_file() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
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
    assert!(
        initialize["result"]["capabilities"]
            .get("inlayHintProvider")
            .is_none()
    );
    assert_eq!(
        initialize["result"]["capabilities"]["codeLensProvider"]["resolveProvider"],
        Value::Bool(false)
    );
    assert_eq!(
        initialize["result"]["capabilities"]["executeCommandProvider"]["commands"][0],
        Value::String("fluent-lsp.showSelectorCombinations".to_string())
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

    open_document(&mut lsp, source_path.as_path(), &source_text);

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
        position_of(&source_text, "brand-name ="),
        &root.join("locales/en/app.ftl"),
        8,
        1,
    );

    assert_definition(
        &mut lsp,
        4,
        &source_path,
        position_of(&source_text, "label = Lanzar"),
        &root.join("locales/en/app.ftl"),
        13,
        5,
    );

    let nested_path = root.join("locales/es/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();
    open_document(&mut lsp, &nested_path, &nested_text);

    assert_definition(
        &mut lsp,
        5,
        &nested_path,
        position_of(&nested_text, "label = Guardar"),
        &root.join("locales/en/dialogs/menu.ftl"),
        4,
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

    let definition = recv_response(lsp, request_id);
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

fn assert_definition_is_absent(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
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

    let definition = recv_response(lsp, request_id);
    assert_eq!(definition["result"], Value::Null);
}

#[test]
fn definition_rejects_non_file_uris_with_invalid_params() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 6);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": "untitled://scratch" },
            "position": { "line": 0, "character": 0 }
        }
    }));

    let response = recv_response(&mut lsp, 7);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("expected a file URI".to_string())
    );
}

#[test]
fn definition_rejects_files_outside_the_configured_workspace() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 6_006);
    let temp = tempdir().unwrap();
    let outside_path = temp.path().join("outside.ftl");
    std::fs::write(&outside_path, "hello = Outside\n").unwrap();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_007,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", outside_path.display()) },
            "position": { "line": 0, "character": 0 }
        }
    }));

    let response = recv_response(&mut lsp, 6_007);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String(format!(
            "document is outside the configured Fluent workspace: {}",
            outside_path.display()
        ))
    );
}

#[test]
fn goto_definition_from_origin_file_returns_no_location() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 6_100);
    open_document(&mut lsp, &source_path, &source_text);

    assert_definition_is_absent(
        &mut lsp,
        6_101,
        &source_path,
        position_of(&source_text, "welcome-title"),
    );
}

#[test]
fn goto_definition_returns_no_location_for_translation_key_missing_in_origin() {
    let workspace = temp_workspace(&[
        ("locales/en/app.ftl", "welcome-title = Welcome\n"),
        (
            "locales/es/app.ftl",
            "welcome-title = Bienvenido\nlocal-only = Solo local\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_102);
    open_document(&mut lsp, &source_path, &source_text);

    assert_definition_is_absent(
        &mut lsp,
        6_103,
        &source_path,
        position_of(&source_text, "local-only"),
    );
}

#[test]
fn goto_definition_returns_no_location_when_origin_counterpart_file_is_missing() {
    let workspace = temp_workspace(&[("locales/es/only.ftl", "orphan-title = Huerfano\n")]);
    let source_path = workspace.path().join("locales/es/only.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_104);
    open_document(&mut lsp, &source_path, &source_text);

    assert_definition_is_absent(
        &mut lsp,
        6_105,
        &source_path,
        position_of(&source_text, "orphan-title"),
    );
}

#[test]
fn goto_definition_returns_no_location_for_translation_attribute_missing_in_origin() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "download-action =\n    .label = Download\n",
        ),
        (
            "locales/es/app.ftl",
            "download-action =\n    .label = Descargar\n    .tooltip = Descarga esta build\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_130);
    open_document(&mut lsp, &source_path, &source_text);

    assert_definition_is_absent(
        &mut lsp,
        6_131,
        &source_path,
        position_of(&source_text, ".tooltip ="),
    );
}

#[test]
fn completion_rejects_files_outside_the_configured_workspace() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 8);
    let temp = tempdir().unwrap();
    let outside_path = temp.path().join("outside.ftl");
    std::fs::write(&outside_path, "hello = Outside\n").unwrap();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", outside_path.display()) },
            "position": { "line": 0, "character": 2 }
        }
    }));

    let response = recv_response(&mut lsp, 9);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String(format!(
            "document is outside the configured Fluent workspace: {}",
            outside_path.display()
        )),
        "unexpected error response: {response:?}"
    );
}

#[test]
fn completion_rejects_non_file_uris_with_invalid_params() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 8_100);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 8_101,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": "untitled://scratch" },
            "position": { "line": 0, "character": 0 }
        }
    }));

    let response = recv_response(&mut lsp, 8_101);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("expected a file URI".to_string())
    );
}

#[test]
fn references_from_origin_resolve_to_translated_fluent_files() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
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

    open_document(&mut lsp, &source_path, &source_text);

    assert_references(
        &mut lsp,
        11,
        &source_path,
        position_of(&source_text, "welcome-title"),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 1, 0),
            ReferenceExpectation::new("locales/fr/app.ftl", 0, 0),
        ],
    );

    assert_references(
        &mut lsp,
        12,
        &source_path,
        position_of(&source_text, "brand-name ="),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 3, 1),
            ReferenceExpectation::new("locales/fr/app.ftl", 2, 1),
        ],
    );

    assert_references(
        &mut lsp,
        13,
        &source_path,
        position_of(&source_text, "label = Launch"),
        &[
            ReferenceExpectation::new("locales/es/app.ftl", 5, 5),
            ReferenceExpectation::new("locales/fr/app.ftl", 4, 5),
        ],
    );

    let nested_path = root.join("locales/en/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();

    open_document(&mut lsp, &nested_path, &nested_text);

    assert_references(
        &mut lsp,
        14,
        &nested_path,
        position_of(&nested_text, "label = Save"),
        &[
            ReferenceExpectation::new("locales/es/dialogs/menu.ftl", 1, 5),
            ReferenceExpectation::new("locales/fr/dialogs/menu.ftl", 1, 5),
            ReferenceExpectation::new("locales/lv/dialogs/menu.ftl", 1, 5),
        ],
    );
}

#[test]
fn references_from_origin_return_empty_list_when_no_translation_matches() {
    let workspace = temp_workspace(&[
        ("locales/en/app.ftl", "orphan-title = Welcome\n"),
        ("locales/es/app.ftl", "welcome-title = Bienvenido\n"),
        ("locales/fr/app.ftl", "welcome-title = Bienvenue\n"),
    ]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_106);
    open_document(&mut lsp, &source_path, &source_text);

    assert_references_are_empty(
        &mut lsp,
        6_107,
        &source_path,
        position_of(&source_text, "orphan-title"),
    );
}

#[test]
fn references_from_origin_attribute_return_empty_list_when_no_translation_matches() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "download-action =\n    .label = Download\n    .tooltip = Download this build\n",
        ),
        (
            "locales/es/app.ftl",
            "download-action =\n    .label = Descargar\n",
        ),
    ]);
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_132);
    open_document(&mut lsp, &origin_path, &origin_text);

    assert_references_are_empty(
        &mut lsp,
        6_133,
        &origin_path,
        position_of(&origin_text, ".tooltip ="),
    );
}

#[test]
fn references_from_translation_file_return_no_result() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 6_108);
    open_document(&mut lsp, &source_path, &source_text);

    assert_references_are_absent(
        &mut lsp,
        6_109,
        &source_path,
        position_of(&source_text, "welcome-title"),
    );
}

#[test]
fn references_reject_non_file_uris_with_invalid_params() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 6_120);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_121,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": "untitled://scratch" },
            "position": { "line": 0, "character": 0 },
            "context": { "includeDeclaration": false }
        }
    }));

    let response = recv_response(&mut lsp, 6_121);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("expected a file URI".to_string())
    );
}

#[test]
fn references_reject_files_outside_the_configured_workspace() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 6_122);
    let temp = tempdir().unwrap();
    let outside_path = temp.path().join("outside.ftl");
    std::fs::write(&outside_path, "hello = Outside\n").unwrap();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_123,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", outside_path.display()) },
            "position": { "line": 0, "character": 0 },
            "context": { "includeDeclaration": false }
        }
    }));

    let response = recv_response(&mut lsp, 6_123);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String(format!(
            "document is outside the configured Fluent workspace: {}",
            outside_path.display()
        ))
    );
}

#[test]
fn initialized_builds_index_and_reports_progress_when_supported() {
    let root = fixture_root();
    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 140,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {
                "window": {
                    "workDoneProgress": true
                }
            }
        }
    }));
    assert_eq!(lsp.recv()["id"], 140);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    assert_eq!(
        recv_notification(&mut lsp, "window/logMessage")["method"],
        "window/logMessage"
    );
    let begin = recv_notification(&mut lsp, "$/progress");
    assert_eq!(
        begin["params"]["token"],
        Value::String("fluent-lsp-index".to_string())
    );
    assert_eq!(
        begin["params"]["value"]["kind"],
        Value::String("begin".to_string())
    );
    let mut saw_report = false;
    loop {
        let progress = recv_notification(&mut lsp, "$/progress");
        match progress["params"]["value"]["kind"].as_str() {
            Some("report") => saw_report = true,
            Some("end") => break,
            other => panic!("unexpected progress kind: {other:?}"),
        }
    }
    assert!(saw_report, "index build should report per-file progress");
}

#[test]
fn log_trace_reports_index_and_request_timings_when_enabled() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 153,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "trace": "messages",
            "capabilities": {}
        }
    }));
    assert_eq!(lsp.recv()["id"], 153);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let _ = recv_notification(&mut lsp, "window/logMessage");
    let index_trace = recv_notification(&mut lsp, "$/logTrace");
    let index_trace_message = index_trace["params"]["message"].as_str().unwrap();
    let (index_prefix, index_elapsed_ms) = index_trace_message
        .split_once(" elapsed_ms=")
        .expect("workspace/index trace should include elapsed_ms");
    assert_eq!(index_prefix, "fluent-lsp timing operation=workspace/index");
    assert!(
        index_elapsed_ms.parse::<f64>().is_ok(),
        "unexpected workspace/index elapsed_ms payload: {index_trace:?}"
    );
    assert!(index_trace["params"].get("verbose").is_none());

    send_open_document(&mut lsp, &source_path, &source_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 154,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": {
                "line": position_of(&source_text, "welcome-title").0,
                "character": position_of(&source_text, "welcome-title").1
            }
        }
    }));
    let request_trace = recv_notification(&mut lsp, "$/logTrace");
    let _ = recv_response(&mut lsp, 154);
    let request_trace_message = request_trace["params"]["message"].as_str().unwrap();
    let (request_prefix, request_elapsed_ms) = request_trace_message
        .split_once(" elapsed_ms=")
        .expect("definition trace should include elapsed_ms");
    assert_eq!(
        request_prefix,
        "fluent-lsp timing operation=textDocument/definition"
    );
    assert!(
        request_elapsed_ms.parse::<f64>().is_ok(),
        "unexpected definition elapsed_ms payload: {request_trace:?}"
    );
    assert!(request_trace["params"].get("verbose").is_none());
}

#[test]
fn verbose_log_trace_includes_request_details() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 155,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "trace": "verbose",
            "capabilities": {}
        }
    }));
    assert_eq!(lsp.recv()["id"], 155);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let _ = recv_notification(&mut lsp, "window/logMessage");
    let index_trace = recv_notification(&mut lsp, "$/logTrace");
    assert!(
        index_trace["params"]["verbose"]
            .as_str()
            .is_some_and(|verbose| verbose.starts_with("files=")),
        "unexpected index trace: {index_trace:?}"
    );

    send_open_document(&mut lsp, &source_path, &source_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 156,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": {
                "line": position_of(&source_text, "welcome-title").0,
                "character": position_of(&source_text, "welcome-title").1
            }
        }
    }));
    let request_trace = recv_notification(&mut lsp, "$/logTrace");
    let _ = recv_response(&mut lsp, 156);
    assert_eq!(
        request_trace["params"]["verbose"],
        Value::String("hit=true".to_string())
    );
}

#[test]
fn set_trace_enables_request_timings_after_initialize() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 157,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));
    assert_eq!(lsp.recv()["id"], 157);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let _ = recv_notification(&mut lsp, "window/logMessage");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "$/setTrace",
        "params": {
            "value": "verbose"
        }
    }));
    let trace_ack = recv_notification(&mut lsp, "$/logTrace");
    assert_eq!(
        trace_ack["params"]["message"],
        Value::String("fluent-lsp trace updated value=verbose".to_string())
    );
    assert!(
        trace_ack["params"]["verbose"].is_null(),
        "unexpected verbose payload for trace ack: {trace_ack:?}"
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 158,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": {
                "line": position_of(&source_text, "welcome-title").0,
                "character": position_of(&source_text, "welcome-title").1
            }
        }
    }));
    let request_trace = recv_notification(&mut lsp, "$/logTrace");
    let _ = recv_response(&mut lsp, 158);
    let request_trace_message = request_trace["params"]["message"].as_str().unwrap();
    let (request_prefix, request_elapsed_ms) = request_trace_message
        .split_once(" elapsed_ms=")
        .expect("definition trace should include elapsed_ms");
    assert_eq!(
        request_prefix,
        "fluent-lsp timing operation=textDocument/definition"
    );
    assert!(
        request_elapsed_ms.parse::<f64>().is_ok(),
        "unexpected definition elapsed_ms payload: {request_trace:?}"
    );
    assert_eq!(
        request_trace["params"]["verbose"],
        Value::String("hit=true".to_string())
    );
}

#[test]
fn indexed_requests_reflect_live_origin_changes_without_restart() {
    let workspace = completion_workspace();
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let translation_text = "hello = Hola\n\nfresh-key = Fresco\n";
    let updated_origin = concat!(
        "hello = Hello\n",
        "\n",
        "# Fresh origin docs\n",
        "fresh-key = Fresh origin value\n",
    );

    let mut lsp = initialized_lsp(workspace.path(), 141);
    send_open_document(&mut lsp, &origin_path, updated_origin);
    send_open_document(&mut lsp, &translation_path, translation_text);

    assert_definition(
        &mut lsp,
        142,
        &translation_path,
        position_of(translation_text, "fresh-key"),
        &origin_path,
        3,
        0,
    );
    let hover = request_hover(
        &mut lsp,
        144,
        &translation_path,
        position_of(translation_text, "fresh-key"),
    );
    assert_eq!(
        extract_ftl_blocks(hover["result"]["contents"]["value"].as_str().unwrap()),
        vec!["# Fresh origin docs".to_string()]
    );

    let completion_text = "fresh";
    send_change_document(&mut lsp, &translation_path, 2, completion_text);
    let labels = request_completion_labels(
        &mut lsp,
        143,
        &translation_path,
        position_after(completion_text, "fresh"),
    );
    assert_eq!(labels, vec!["fresh-key".to_string()]);
}

#[test]
fn indexed_requests_reflect_live_translation_changes_and_dirty_close_reverts() {
    let workspace = completion_workspace();
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let disk_translation = "hello-world = Hola\n";
    std::fs::write(&translation_path, disk_translation).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 145);
    open_document(&mut lsp, &origin_path, &origin_text);

    assert_references(
        &mut lsp,
        146,
        &origin_path,
        position_of(&origin_text, "download-action"),
        &[],
    );

    send_open_document(&mut lsp, &translation_path, disk_translation);
    let dirty_translation = "hello = Hola\n\ndownload-action = Descargar\n";
    send_change_document(&mut lsp, &translation_path, 2, dirty_translation);

    assert_references(
        &mut lsp,
        147,
        &origin_path,
        position_of(&origin_text, "download-action"),
        &[ReferenceExpectation::new("locales/es/app.ftl", 2, 0)],
    );

    send_close_document(&mut lsp, &translation_path);
    let _ = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_references(
        &mut lsp,
        148,
        &origin_path,
        position_of(&origin_text, "download-action"),
        &[],
    );
}

#[test]
fn indexed_references_pick_up_disk_file_adds_and_deletes() {
    let workspace = completion_workspace();
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let new_translation = workspace.path().join("locales/fr/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 149);
    open_document(&mut lsp, &origin_path, &origin_text);
    assert_references(
        &mut lsp,
        150,
        &origin_path,
        position_of(&origin_text, "hello-world"),
        &[],
    );

    std::fs::create_dir_all(new_translation.parent().unwrap()).unwrap();
    std::fs::write(&new_translation, "hello-world = Bonjour\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_references(
        &mut lsp,
        151,
        &origin_path,
        position_of(&origin_text, "hello-world"),
        &[ReferenceExpectation::new("locales/fr/app.ftl", 0, 0)],
    );

    std::fs::remove_file(&new_translation).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_references(
        &mut lsp,
        152,
        &origin_path,
        position_of(&origin_text, "hello-world"),
        &[],
    );
}

#[test]
fn local_only_file_warning_updates_when_origin_counterpart_appears() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es")).unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();
    let mut lsp = initialized_lsp(workspace.path(), 152);
    let local_path = workspace.path().join("locales/es/only.ftl");
    let local_text = "local-only = Solo local\n";
    std::fs::write(&local_path, local_text).unwrap();
    send_open_document(&mut lsp, &local_path, local_text);
    send_save_document(&mut lsp, &local_path, Some(local_text));
    let warning = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = warning["params"]["diagnostics"].as_array().unwrap();
    let counterpart_warning = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic["message"]
                == Value::String(
                    "Translation file has no origin-language counterpart for `only`".to_string(),
                )
        })
        .expect("missing missing-origin-counterpart diagnostic");
    assert_eq!(counterpart_warning["severity"], Value::from(2));
    assert_eq!(counterpart_warning["range"]["start"]["line"], Value::from(0));
    assert_eq!(counterpart_warning["range"]["start"]["character"], Value::from(0));

    std::fs::create_dir_all(workspace.path().join("locales/en")).unwrap();
    std::fs::write(
        workspace.path().join("locales/en/only.ftl"),
        "local-only = Origin now exists\n",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let warning_text =
        Value::String("Translation file has no origin-language counterpart for `only`".to_string());
    loop {
        let cleared = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if cleared["params"]["uri"] != Value::String(format!("file://{}", local_path.display())) {
            continue;
        }
        let diagnostics = cleared["params"]["diagnostics"].as_array().unwrap();
        if diagnostics
            .iter()
            .all(|diagnostic| diagnostic["message"] != warning_text)
        {
            break;
        }
    }
}

#[test]
fn translation_only_keys_warn_and_clear_when_origin_adds_counterparts() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es")).unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();

    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let origin_text = "shared = Hello\nmenu =\n    .label = Save\n";
    let translation_text = "shared = Hola\nextra = Solo local\nmenu =\n    .label = Guardar\n    .tooltip = Solo aqui\n";
    std::fs::write(&origin_path, origin_text).unwrap();
    std::fs::write(&translation_path, translation_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 153);
    send_open_document(&mut lsp, &translation_path, translation_text);
    send_save_document(&mut lsp, &translation_path, Some(translation_text));

    let warning = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = warning["params"]["diagnostics"].as_array().unwrap();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == Value::String(
                "Translation entry `extra` has no origin-language counterpart".to_string(),
            )
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == Value::String(
                "Translation attribute `menu.tooltip` has no origin-language counterpart"
                    .to_string(),
            )
    }));
    let extra = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic["message"]
                == Value::String(
                    "Translation entry `extra` has no origin-language counterpart".to_string(),
                )
        })
        .unwrap();
    assert_eq!(extra["severity"], Value::from(2));
    assert_eq!(
        extra["range"]["start"]["line"],
        Value::from(position_of(translation_text, "extra").0)
    );
    assert_eq!(
        extra["range"]["start"]["character"],
        Value::from(position_of(translation_text, "extra").1)
    );
    let tooltip = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic["message"]
                == Value::String(
                    "Translation attribute `menu.tooltip` has no origin-language counterpart"
                        .to_string(),
                )
        })
        .unwrap();
    assert_eq!(tooltip["severity"], Value::from(2));
    assert_eq!(
        tooltip["range"]["start"]["line"],
        Value::from(position_of(translation_text, "tooltip").0)
    );
    assert_eq!(
        tooltip["range"]["start"]["character"],
        Value::from(position_of(translation_text, "tooltip").1)
    );

    std::fs::write(
        &origin_path,
        "shared = Hello\nextra = Origin now exists\nmenu =\n    .label = Save\n    .tooltip = Origin tooltip\n",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let extra_text =
        Value::String("Translation entry `extra` has no origin-language counterpart".to_string());
    let tooltip_text = Value::String(
        "Translation attribute `menu.tooltip` has no origin-language counterpart".to_string(),
    );
    loop {
        let cleared = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if cleared["params"]["uri"]
            != Value::String(format!("file://{}", translation_path.display()))
        {
            continue;
        }
        let diagnostics = cleared["params"]["diagnostics"].as_array().unwrap();
        if diagnostics.iter().all(|diagnostic| {
            diagnostic["message"] != extra_text && diagnostic["message"] != tooltip_text
        }) {
            break;
        }
    }
}

#[test]
fn translation_only_warnings_are_absent_for_matching_translation_files() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "shared = Hello\nmenu =\n    .label = Save\n    .tooltip = Origin tooltip\n",
        ),
        (
            "locales/es/app.ftl",
            "shared = Hola\nmenu =\n    .label = Guardar\n    .tooltip = Tooltip local\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_206);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(notification["params"]["diagnostics"], Value::Array(Vec::new()));
}

#[test]
fn completion_from_translation_uses_origin_language_keys_and_attributes() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let menu_path = workspace.path().join("locales/es/dialogs/menu.ftl");

    let mut lsp = initialized_lsp(workspace.path(), 14);

    let top_level_text = "welcome-title = Bienvenido\n\ndown";
    send_open_document(&mut lsp, &app_path, top_level_text);
    let labels = request_completion_labels(
        &mut lsp,
        15,
        &app_path,
        position_after(top_level_text, "down"),
    );
    assert_eq!(
        labels,
        vec!["download-action".to_string(), "download-count".to_string()]
    );

    let attribute_text = "menu-save =\n    .l\n";
    send_open_document(&mut lsp, &menu_path, attribute_text);
    let attribute_labels = request_completion_labels(
        &mut lsp,
        16,
        &menu_path,
        position_after(attribute_text, ".l"),
    );
    assert_eq!(attribute_labels, vec![".label".to_string()]);

    let bare_dot_text = "menu-save =\n    .\n";
    send_open_document(&mut lsp, &menu_path, bare_dot_text);
    let all_attribute_labels =
        request_completion_labels(&mut lsp, 17, &menu_path, position_after(bare_dot_text, "."));
    assert_eq!(
        all_attribute_labels,
        vec![".label".to_string(), ".tooltip".to_string()]
    );
}

#[test]
fn completion_omits_already_present_top_level_keys() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let source = "download-action = Descargar\n\ndown";

    let mut lsp = initialized_lsp(workspace.path(), 17_100);
    send_open_document(&mut lsp, &app_path, source);

    let labels = request_completion_labels(
        &mut lsp,
        17_101,
        &app_path,
        position_after(source, "down"),
    );
    assert_eq!(labels, vec!["download-count".to_string()]);
}

#[test]
fn completion_omits_already_present_attributes() {
    let workspace = completion_workspace();
    let menu_path = workspace.path().join("locales/es/dialogs/menu.ftl");
    let source = "menu-save =\n    .label = Guardar\n    .\n";

    let mut lsp = initialized_lsp(workspace.path(), 17_102);
    send_open_document(&mut lsp, &menu_path, source);

    let labels = request_completion_labels(
        &mut lsp,
        17_103,
        &menu_path,
        position_after(source, "."),
    );
    assert_eq!(labels, vec![".tooltip".to_string()]);
}

#[test]
fn completion_uses_nested_origin_counterpart_and_skips_origin_files() {
    let workspace = completion_workspace();
    let nested_translation = workspace.path().join("locales/es/dialogs/menu.ftl");
    let origin_app = workspace.path().join("locales/en/app.ftl");
    let nested_text = "menu-save =\n    .t\n";

    let mut lsp = initialized_lsp(workspace.path(), 18);

    send_open_document(&mut lsp, &nested_translation, nested_text);
    let nested_labels = request_completion_labels(
        &mut lsp,
        19,
        &nested_translation,
        position_after(nested_text, ".t"),
    );
    assert_eq!(nested_labels, vec![".tooltip".to_string()]);

    let origin_labels = request_completion_labels(
        &mut lsp,
        20,
        &origin_app,
        position_after("download-action = Download\n", "download-action"),
    );
    assert!(origin_labels.is_empty());
}

#[test]
fn completion_returns_empty_results_for_nested_origin_files() {
    let workspace = completion_workspace();
    let origin_menu = workspace.path().join("locales/en/dialogs/menu.ftl");
    let source = "menu-save =\n    .t\n";

    let mut lsp = initialized_lsp(workspace.path(), 20_100);
    send_open_document(&mut lsp, &origin_menu, source);
    let labels = request_completion_labels(
        &mut lsp,
        20_101,
        &origin_menu,
        position_after(source, ".t"),
    );
    assert!(labels.is_empty());
}

#[test]
fn completion_returns_empty_results_for_unmatched_prefixes() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let source = "welcome-title = Bienvenido\n\nzzz";

    let mut lsp = initialized_lsp(workspace.path(), 21);
    send_open_document(&mut lsp, &app_path, source);

    let labels = request_completion_labels(&mut lsp, 22, &app_path, position_after(source, "zzz"));
    assert!(labels.is_empty());
}

#[test]
fn completion_returns_empty_results_inside_comments() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let source = "welcome-title = Bienvenido\n\n# down";

    let mut lsp = initialized_lsp(workspace.path(), 22_102);
    send_open_document(&mut lsp, &app_path, source);

    let labels = request_completion_labels(
        &mut lsp,
        22_103,
        &app_path,
        position_after(source, "down"),
    );
    assert!(labels.is_empty());
}

#[test]
fn completion_returns_empty_results_without_origin_counterpart_file() {
    let workspace = temp_workspace(&[("locales/es/only.ftl", "fresh\n")]);
    let source_path = workspace.path().join("locales/es/only.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 22_100);
    open_document(&mut lsp, &source_path, &source_text);

    let labels = request_completion_labels(
        &mut lsp,
        22_101,
        &source_path,
        position_after(&source_text, "fresh"),
    );
    assert!(labels.is_empty());
}

#[test]
fn completion_items_include_origin_documentation_for_keys_and_attributes() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let menu_path = workspace.path().join("locales/es/dialogs/menu.ftl");

    let mut lsp = initialized_lsp(workspace.path(), 23);

    let commented_text = "welcome-title = Bienvenido\n\ncommented";
    send_open_document(&mut lsp, &app_path, commented_text);
    let key_items = request_completion_items(
        &mut lsp,
        24,
        &app_path,
        position_after(commented_text, "commented"),
    );
    let key_item = key_items
        .iter()
        .find(|item| item["label"] == Value::String("commented-preview".to_string()))
        .expect("missing completion item for commented-preview");
    assert_eq!(
        key_item["documentation"]["kind"],
        Value::String("markdown".to_string())
    );
    let key_docs = key_item["documentation"]["value"]
        .as_str()
        .expect("expected markdown completion docs");
    assert_eq!(
        key_docs,
        "```ftl\n# Completion doc coverage\n# Keep this note in completion hover\n```\n\n---\n\n```ftl\ncommented-preview = Preview text for completion docs.\n```"
    );

    let attribute_text = "menu-save =\n    .t\n";
    send_open_document(&mut lsp, &menu_path, attribute_text);
    let attribute_items = request_completion_items(
        &mut lsp,
        25,
        &menu_path,
        position_after(attribute_text, ".t"),
    );
    let attribute_item = attribute_items
        .iter()
        .find(|item| item["label"] == Value::String(".tooltip".to_string()))
        .expect("missing completion item for .tooltip");
    assert_eq!(
        attribute_item["documentation"]["kind"],
        Value::String("markdown".to_string())
    );
    let attribute_docs = attribute_item["documentation"]["value"]
        .as_str()
        .expect("expected markdown attribute docs");
    assert_eq!(
        attribute_docs,
        "```ftl\n# Menu completion documentation\n# Keep this entry visible in completion hover\n```\n\n---\n\n```ftl\n.tooltip = Save this file\n```"
    );
}

#[test]
fn code_action_fills_missing_translation_entries_with_parseable_stubs() {
    let workspace = missing_entry_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 30);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        31,
        &source_path,
        position_of(&source_text, "hello = Hola Mundo"),
    );
    let action = find_code_action(&actions, "Add missing keys and attributes from source");
    assert_eq!(action["kind"], Value::String("quickfix".to_string()));

    let updated = apply_code_action_edit(
        &source_text,
        action,
        &format!("file://{}", source_path.display()),
    );
    assert_fluent_parses(&updated);
    assert_eq!(
        updated,
        concat!(
            "hello = Hola Mundo\n",
            "menu-save =\n",
            "    .label = Guardar\n",
            "    .tooltip = { \"\" }\n",
            "\n",
            "sync-status = { \"\" }\n",
        )
    );
    assert_eq!(runtime_message_text(&updated, "es", "hello"), "Hola Mundo");
}

#[test]
fn code_action_copies_missing_translation_entries_with_markers() {
    let workspace = missing_entry_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 40);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        41,
        &source_path,
        position_of(&source_text, "hello = Hola Mundo"),
    );
    let action = find_code_action(&actions, "Copy missing keys and attributes from source");
    assert_eq!(action["kind"], Value::String("quickfix".to_string()));

    let updated = apply_code_action_edit(
        &source_text,
        action,
        &format!("file://{}", source_path.display()),
    );
    assert_fluent_parses(&updated);
    assert_eq!(
        updated,
        concat!(
            "hello = Hola Mundo\n",
            "# [LSP-COPY .tooltip]\n",
            "menu-save =\n",
            "    .label = Guardar\n",
            "    .tooltip = Save this file\n",
            "\n",
            "# [LSP-COPY]\n",
            "sync-status = Sync ready\n",
        )
    );
    assert_eq!(runtime_message_text(&updated, "es", "hello"), "Hola Mundo");
    assert_eq!(
        runtime_message_text(&updated, "es", "menu-save.tooltip"),
        runtime_message_text(&origin_text, "en", "menu-save.tooltip")
    );
    assert_eq!(
        runtime_message_text(&updated, "es", "sync-status"),
        runtime_message_text(&origin_text, "en", "sync-status")
    );
}

#[test]
fn whole_file_missing_entry_actions_are_absent_when_translation_is_complete() {
    let workspace = missing_entry_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = concat!(
        "hello = Hola Mundo\n",
        "menu-save =\n",
        "    .label = Guardar\n",
        "    .tooltip = Guarda este archivo\n",
        "\n",
        "sync-status = Sincronizacion lista\n",
    );
    std::fs::write(&source_path, source_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 50);
    open_document(&mut lsp, &source_path, source_text);

    let actions = request_code_actions_allow_empty(
        &mut lsp,
        51,
        &source_path,
        position_of(source_text, "hello = Hola Mundo"),
    );
    assert!(
        actions.iter().all(|action| {
            action["title"]
                != Value::String("Add missing keys and attributes from source".to_string())
                && action["title"]
                    != Value::String("Copy missing keys and attributes from source".to_string())
        }),
        "unexpected whole-file missing-entry action(s): {actions:?}"
    );
}

#[test]
fn whole_file_missing_entry_actions_are_absent_without_origin_counterpart_file() {
    let workspace = temp_workspace(&[("locales/es/only.ftl", "hello = Hola Mundo\n")]);
    let source_path = workspace.path().join("locales/es/only.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_200);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions_allow_empty(
        &mut lsp,
        5_201,
        &source_path,
        position_of(&source_text, "hello = Hola Mundo"),
    );
    assert!(actions.iter().all(|action| {
        action["title"] != Value::String("Add missing keys and attributes from source".to_string())
            && action["title"]
                != Value::String("Copy missing keys and attributes from source".to_string())
    }));
}

#[test]
fn lsp_copy_marker_diagnostics_publish_on_save_and_clear_after_removal() {
    let workspace = copy_marker_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let cleaned = concat!(
        "hello = Hola Mundo\n",
        "\n",
        "download-action =\n",
        "    .label = Descargar\n",
        "    .tooltip = Download this build\n",
    );

    let mut lsp = initialized_lsp(workspace.path(), 60);
    open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    let marker_diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic["message"]
                == Value::String("Entry still contains an `# [LSP-COPY]` marker".to_string())
        })
        .collect::<Vec<_>>();
    assert_eq!(marker_diagnostics.len(), 2);
    assert_fluent_parses(&source_text);
    assert_eq!(
        marker_diagnostics[0]["message"],
        Value::String("Entry still contains an `# [LSP-COPY]` marker".to_string())
    );
    assert_eq!(
        marker_diagnostics[0]["range"]["start"]["line"],
        Value::from(0)
    );
    assert_eq!(
        marker_diagnostics[0]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        marker_diagnostics[1]["range"]["start"]["line"],
        Value::from(6)
    );
    assert_eq!(
        marker_diagnostics[1]["range"]["start"]["character"],
        Value::from(5)
    );

    send_change_document(&mut lsp, &source_path, 2, cleaned);
    send_save_document(&mut lsp, &source_path, Some(cleaned));
    let cleared = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(cleared["params"]["diagnostics"], Value::Array(Vec::new()));
    assert_fluent_parses(cleaned);
    assert_eq!(
        runtime_message_text(cleaned, "es", "download-action.tooltip"),
        "Download this build"
    );
}

#[test]
fn hover_on_copied_attribute_does_not_surface_lsp_copy_marker_comments() {
    let workspace = copy_marker_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text = std::fs::read_to_string(workspace.path().join("locales/en/app.ftl")).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 60_100);
    open_document(&mut lsp, &source_path, &source_text);

    let key_hover = request_hover(
        &mut lsp,
        60_101,
        &source_path,
        position_of(&source_text, ".tooltip = Download this build"),
    );
    let key_value = key_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_hover_block_matches(key_value, 0, &origin_text, "en", "download-action.tooltip", &[]);
    assert_hover_block_matches(key_value, 1, &source_text, "es", "download-action.tooltip", &[]);

    let body_hover = request_hover(
        &mut lsp,
        60_102,
        &source_path,
        position_of(&source_text, "Download this build"),
    );
    let body_value = body_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_hover_block_matches(body_value, 0, &origin_text, "en", "download-action.tooltip", &[]);
    assert_hover_block_matches(body_value, 1, &source_text, "es", "download-action.tooltip", &[]);
}

#[test]
fn code_action_copies_single_stub_message_without_touching_other_entries() {
    let workspace = single_key_copy_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 70);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        71,
        &source_path,
        position_of(&source_text, "hello = { \"\" }"),
    );
    let action = find_code_action(&actions, "Copy `hello` from source");
    let updated = apply_code_action_edit(
        &source_text,
        action,
        &format!("file://{}", source_path.display()),
    );

    assert_fluent_parses(&updated);
    assert_eq!(
        updated,
        concat!(
            "# [LSP-COPY]\n",
            "hello = Hello World\n",
            "download-action =\n",
            "    .label = Descargar\n",
            "\n",
            "sync-status = { \"\" }\n",
        )
    );
    assert_eq!(
        runtime_message_text(&updated, "es", "hello"),
        runtime_message_text(&origin_text, "en", "hello")
    );
}

#[test]
fn code_action_copies_missing_attributes_for_selected_message_only() {
    let workspace = single_key_copy_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 80);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        81,
        &source_path,
        position_of(&source_text, "download-action ="),
    );
    let action = find_code_action(
        &actions,
        "Copy missing attributes for `download-action` from source",
    );
    let updated = apply_code_action_edit(
        &source_text,
        action,
        &format!("file://{}", source_path.display()),
    );

    assert_fluent_parses(&updated);
    assert_eq!(
        updated,
        concat!(
            "hello = { \"\" }\n",
            "# [LSP-COPY .tooltip]\n",
            "download-action =\n",
            "    .label = Descargar\n",
            "    .tooltip = Download this build\n",
            "\n",
            "sync-status = { \"\" }\n",
        )
    );
    assert_eq!(
        runtime_message_text(&updated, "es", "download-action.tooltip"),
        runtime_message_text(&origin_text, "en", "download-action.tooltip")
    );
}

#[test]
fn single_message_copy_actions_are_absent_for_complete_entries() {
    let workspace = single_key_copy_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = concat!(
        "hello = Hola Mundo\n",
        "download-action =\n",
        "    .label = Descargar\n",
        "    .tooltip = Descarga esta build\n",
        "\n",
        "sync-status = Sincronizacion lista\n",
    );
    std::fs::write(&source_path, source_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 90);
    open_document(&mut lsp, &source_path, source_text);

    let hello_actions = request_code_actions_allow_empty(
        &mut lsp,
        91,
        &source_path,
        position_of(source_text, "hello = Hola Mundo"),
    );
    assert!(
        hello_actions
            .iter()
            .all(|action| action["title"] != Value::String("Copy `hello` from source".to_string()))
    );

    let download_actions = request_code_actions_allow_empty(
        &mut lsp,
        92,
        &source_path,
        position_of(source_text, "download-action ="),
    );
    assert!(download_actions.iter().all(|action| {
        action["title"]
            != Value::String(
                "Copy missing attributes for `download-action` from source".to_string(),
            )
    }));
}

#[test]
fn single_message_copy_action_is_absent_when_selected_key_has_no_origin_counterpart() {
    let workspace = temp_workspace(&[
        ("locales/en/app.ftl", "shared = Hello\n"),
        ("locales/es/app.ftl", "shared = Hola\nlocal-only = { \"\" }\n"),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_202);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions_allow_empty(
        &mut lsp,
        5_203,
        &source_path,
        position_of(&source_text, "local-only = { \"\" }"),
    );
    assert!(
        actions
            .iter()
            .all(|action| action["title"] != Value::String("Copy `local-only` from source".to_string()))
    );
}

#[test]
fn missing_attribute_copy_action_is_absent_when_selected_message_has_no_origin_counterpart() {
    let workspace = temp_workspace(&[
        ("locales/en/app.ftl", "shared = Hello\n"),
        (
            "locales/es/app.ftl",
            "shared = Hola\norphan =\n    .label = Huerfano\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_204);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions_allow_empty(
        &mut lsp,
        5_205,
        &source_path,
        position_of(&source_text, "orphan ="),
    );
    assert!(actions.iter().all(|action| {
        action["title"]
            != Value::String("Copy missing attributes for `orphan` from source".to_string())
    }));
}

#[test]
fn origin_files_do_not_offer_translation_only_missing_entry_quick_fixes() {
    let workspace = missing_entry_workspace();
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 9_300);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions_allow_empty(
        &mut lsp,
        9_301,
        &source_path,
        position_of(&source_text, "hello = Hello World"),
    );
    assert!(actions.iter().all(|action| {
        action["title"]
            != Value::String("Copy missing strings in file".to_string())
            && action["title"] != Value::String("Copy missing string `hello`".to_string())
            && action["title"]
                != Value::String(
                    "Copy missing attribute `download-action.tooltip`".to_string(),
                )
    }));
}

#[test]
fn hover_from_translation_shows_local_formatted_messages() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text = std::fs::read_to_string(root.join("locales/en/app.ftl")).unwrap();

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

    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of(&source_text, "commented-preview");
    assert_hover(
        &mut lsp,
        21,
        &source_path,
        key_position,
        "```ftl\n# Comment-only hover coverage\n# Keep this translator guidance visible on key hover\n```\n\n---\n\n```ftl\n# Cobertura de hover con comentarios\n# Mantener visible esta nota para traduccion en el hover de clave\n```",
        key_position.0,
        0,
    );

    let body_hover = assert_hover(
        &mut lsp,
        22,
        &source_path,
        position_of(&source_text, "Abre la build mas reciente de"),
        "```ftl\nOpen the latest Nightly build and pick up where you left off.\n```\n\n---\n\n```ftl\nAbre la build mas reciente de Nightly y sigue donde lo dejaste.\n```",
        2,
        0,
    );
    assert_hover_block_matches(&body_hover, 0, &origin_text, "en", "welcome-body", &[]);
    assert_hover_block_matches(&body_hover, 1, &source_text, "es", "welcome-body", &[]);

    let empty_key_position = position_of(&source_text, "empty-preview");
    let empty_hover = assert_hover(
        &mut lsp,
        221,
        &source_path,
        position_of(&source_text, "\"\""),
        "```ftl\nEnglish empty preview fallback.\n```\n\n---\n\n```ftl\n<empty>\n```",
        empty_key_position.0,
        0,
    );
    assert_eq!(
        extract_ftl_blocks(&empty_hover),
        vec![
            "English empty preview fallback.".to_string(),
            "<empty>".to_string()
        ]
    );

    let selector_hover = assert_hover(
        &mut lsp,
        23,
        &source_path,
        position_of(&source_text, "[female] ella"),
        "`$gender=female`, `$count=*`\n\n```ftl\nCopy the download link for her account on 2 devices now.\n```\n\n---\n\n`$gender=female`, `$count=*`\n\n```ftl\nCopia el enlace de descarga para la cuenta de ella en 2 dispositivos ahora.\n```",
        8,
        0,
    );
    assert_hover_block_matches(&selector_hover, 0, &origin_text, "en", "install-hint", &[("$gender", "female")]);
    assert_hover_block_matches(&selector_hover, 1, &source_text, "es", "install-hint", &[("$gender", "female")]);

    let attribute_hover = assert_hover(
        &mut lsp,
        24,
        &source_path,
        position_of(
            &source_text,
            "Instala la build recomendada para la cuenta de",
        ),
        "`$gender=*`, `$count=*`\n\n```ftl\nInstall the recommended build for their account on 2 devices now.\n```\n\n---\n\n`$gender=*`, `$count=*`\n\n```ftl\nInstala la build recomendada para la cuenta de elle en 2 dispositivos ahora.\n```",
        21,
        5,
    );
    assert_hover_block_matches(&attribute_hover, 0, &origin_text, "en", "download-action.tooltip", &[]);
    assert_hover_block_matches(&attribute_hover, 1, &source_text, "es", "download-action.tooltip", &[]);

    let post_selector_hover = assert_hover(
        &mut lsp,
        25,
        &source_path,
        position_of(&source_text, "en { $count } { $count ->"),
        "`$gender=other`, `$count=*`\n\n```ftl\nCopy the download link for their account on 2 devices now.\n```\n\n---\n\n`$gender=other`, `$count=*`\n\n```ftl\nCopia el enlace de descarga para la cuenta de elle en 2 dispositivos ahora.\n```",
        8,
        0,
    );
    assert_hover_block_matches(&post_selector_hover, 0, &origin_text, "en", "install-hint", &[("$gender", "other")]);
    assert_hover_block_matches(&post_selector_hover, 1, &source_text, "es", "install-hint", &[("$gender", "other")]);

    let second_selector_hover = assert_hover(
        &mut lsp,
        26,
        &source_path,
        position_of(&source_text, "[one] dispositivo"),
        "`$gender=other`, `$count=one`\n\n```ftl\nCopy the download link for their account on 1 device now.\n```\n\n---\n\n`$gender=other`, `$count=one`\n\n```ftl\nCopia el enlace de descarga para la cuenta de elle en 1 dispositivo ahora.\n```",
        8,
        0,
    );
    assert_hover_block_matches(
        &second_selector_hover,
        0,
        &origin_text,
        "en",
        "install-hint",
        &[("$gender", "other"), ("$count", "one")],
    );
    assert_hover_block_matches(
        &second_selector_hover,
        1,
        &source_text,
        "es",
        "install-hint",
        &[("$gender", "other"), ("$count", "one")],
    );
}

#[test]
fn hover_from_translation_matches_available_selector_variables_across_source_and_local() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 27,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 27);
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

    open_document(&mut lsp, &source_path, &source_text);

    assert_hover(
        &mut lsp,
        28,
        &source_path,
        position_of(&source_text, "[female] ella misma"),
        "`$platform=*`, `$count=*`\n\n```ftl\nSummary for mobile users with 2 packages ready.\n```\n\n---\n\n`$gender=female`, `$count=*`\n\n```ftl\nResumen para ella misma con 2 paquetes listo.\n```",
        50,
        0,
    );

    assert_hover(
        &mut lsp,
        29,
        &source_path,
        position_of(&source_text, "[0] ningun paquete"),
        "`$platform=*`, `$count=0`\n\n```ftl\nSummary for mobile users with no packages ready.\n```\n\n---\n\n`$gender=other`, `$count=0`\n\n```ftl\nResumen para elle misme con ningun paquete listo.\n```",
        50,
        0,
    );

    assert_hover(
        &mut lsp,
        30,
        &source_path,
        position_of(&source_text, "[1] un paquete"),
        "`$platform=*`, `$count=1`\n\n```ftl\nSummary for mobile users with one package ready.\n```\n\n---\n\n`$gender=other`, `$count=1`\n\n```ftl\nResumen para elle misme con un paquete listo.\n```",
        50,
        0,
    );
}

#[test]
fn hover_from_latvian_translation_preserves_zero_category_selectors() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 31);
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

    open_document(&mut lsp, &source_path, &source_text);

    assert_hover(
        &mut lsp,
        32,
        &source_path,
        position_of(&source_text, "[zero] neviena pakotne nav gatava"),
        "`$count=zero`\n\n```ftl\nZero summary: no packages ready.\n```\n\n---\n\n`$count=zero`\n\n```ftl\nKopsavilkums ar neviena pakotne nav gatava.\n```",
        0,
        0,
    );

    assert_hover(
        &mut lsp,
        33,
        &source_path,
        position_of(&source_text, "[one] viena pakotne ir gatava"),
        "`$count=one`\n\n```ftl\nZero summary: one package ready.\n```\n\n---\n\n`$count=one`\n\n```ftl\nKopsavilkums ar viena pakotne ir gatava.\n```",
        0,
        0,
    );
}

#[test]
fn hover_from_origin_file_shows_formatted_attribute_text() {
    let root = fixture_root();
    let source_path = root.join("locales/en/dialogs/menu.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 30);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    open_document(&mut lsp, &source_path, &source_text);

    let key_hover = assert_hover(
        &mut lsp,
        31,
        &source_path,
        position_of(&source_text, "label = Save"),
        "```ftl\n### Shared menu copy\n## File menu\n# Primary action\n```",
        4,
        5,
    );
    assert_eq!(
        extract_ftl_blocks(&key_hover),
        vec!["### Shared menu copy\n## File menu\n# Primary action".to_string()]
    );

    let body_hover = request_hover(
        &mut lsp,
        32,
        &source_path,
        position_of(&source_text, "Save changes before closing the window"),
    );
    let body_value = body_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_hover_block_matches(body_value, 0, &source_text, "en", "menu-save.tooltip", &[]);
}

#[test]
fn hover_from_origin_file_shows_one_body_preview_block_for_top_level_message() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 6_124);
    open_document(&mut lsp, &source_path, &source_text);

    let hover = request_hover(
        &mut lsp,
        6_125,
        &source_path,
        position_of(&source_text, "Preview text for hover comments."),
    );
    let value = hover["result"]["contents"]["value"].as_str().unwrap();
    assert_eq!(
        extract_ftl_blocks(value),
        vec!["Preview text for hover comments.".to_string()]
    );
}

#[test]
fn hover_rejects_non_file_uris_with_invalid_params() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 6_126);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_127,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": "untitled://scratch" },
            "position": { "line": 0, "character": 0 }
        }
    }));

    let response = recv_response(&mut lsp, 6_127);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("expected a file URI".to_string())
    );
}

#[test]
fn hover_rejects_files_outside_the_configured_workspace() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 6_128);
    let temp = tempdir().unwrap();
    let outside_path = temp.path().join("outside.ftl");
    std::fs::write(&outside_path, "hello = Outside\n").unwrap();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_129,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", outside_path.display()) },
            "position": { "line": 0, "character": 0 }
        }
    }));

    let response = recv_response(&mut lsp, 6_129);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String(format!(
            "document is outside the configured Fluent workspace: {}",
            outside_path.display()
        ))
    );
}

#[test]
fn hover_key_and_attribute_show_comment_context_across_locale_files() {
    let root = fixture_root();
    let top_level_cases = [
        (
            "locales/en/app.ftl",
            "commented-preview",
            &[
                "# Comment-only hover coverage\n# Keep this translator guidance visible on key hover",
            ][..],
        ),
        (
            "locales/es/app.ftl",
            "commented-preview",
            &[
                "# Comment-only hover coverage\n# Keep this translator guidance visible on key hover",
                "# Cobertura de hover con comentarios\n# Mantener visible esta nota para traduccion en el hover de clave",
            ][..],
        ),
        (
            "locales/fr/app.ftl",
            "commented-preview",
            &[
                "# Comment-only hover coverage\n# Keep this translator guidance visible on key hover",
                "# Couverture hover pour les commentaires\n# Garder cette note visible sur le hover de cle",
            ][..],
        ),
        (
            "locales/lv/app.ftl",
            "commented-preview",
            &[
                "# Comment-only hover coverage\n# Keep this translator guidance visible on key hover",
                "# Hover komentaru parklajums\n# Saglabat so piezimi redzamu atslegas hover skata",
            ][..],
        ),
        (
            "locales/uk/app.ftl",
            "commented-preview",
            &[
                "# Comment-only hover coverage\n# Keep this translator guidance visible on key hover",
                "# Перевірка hover-коментарів\n# Тримайте цю примітку видимою у hover для ключа",
            ][..],
        ),
    ];
    let nested_cases = [
        (
            "locales/en/dialogs/menu.ftl",
            "commented-menu =",
            &[
                "# Attribute hover comment coverage\n# Keep this menu note visible on attribute key hover",
            ][..],
        ),
        (
            "locales/es/dialogs/menu.ftl",
            "commented-menu =",
            &[
                "# Attribute hover comment coverage\n# Keep this menu note visible on attribute key hover",
                "# Cobertura de comentarios para hover de atributo\n# Mantener visible esta nota en el hover de la clave del atributo",
            ][..],
        ),
        (
            "locales/fr/dialogs/menu.ftl",
            "commented-menu =",
            &[
                "# Attribute hover comment coverage\n# Keep this menu note visible on attribute key hover",
                "# Couverture de commentaire pour hover d attribut\n# Garder cette note visible sur le hover de la cle d attribut",
            ][..],
        ),
    ];

    let mut lsp = LspProcess::start();
    initialize_lsp(&mut lsp, &root, 34);

    for (index, (relative_path, needle, expected_blocks)) in top_level_cases.iter().enumerate() {
        let path = root.join(relative_path);
        let source = std::fs::read_to_string(&path).unwrap();
        open_document(&mut lsp, &path, &source);
        let expected_hover = comment_hover_markdown(expected_blocks);
        let hover = assert_hover(
            &mut lsp,
            35 + i64::try_from(index).unwrap(),
            &path,
            position_of(&source, needle),
            &expected_hover,
            position_of(&source, needle).0,
            0,
        );
        assert_eq!(
            extract_ftl_blocks(&hover),
            expected_blocks
                .iter()
                .map(|block| (*block).to_string())
                .collect::<Vec<_>>()
        );
    }

    for (index, (relative_path, needle, expected_blocks)) in nested_cases.iter().enumerate() {
        let path = root.join(relative_path);
        let source = std::fs::read_to_string(&path).unwrap();
        open_document(&mut lsp, &path, &source);
        let expected_hover = comment_hover_markdown(expected_blocks);
        let hover = assert_hover(
            &mut lsp,
            45 + i64::try_from(index).unwrap(),
            &path,
            position_of(&source, needle),
            &expected_hover,
            position_of(&source, needle).0,
            0,
        );
        assert_eq!(
            extract_ftl_blocks(&hover),
            expected_blocks
                .iter()
                .map(|block| (*block).to_string())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn hover_key_with_origin_comments_only_shows_one_origin_comment_block() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "# Comment-only hover coverage\n# Keep this translator guidance visible on key hover\ncommented-preview = Preview text for hover comments.\n",
        ),
        (
            "locales/es/app.ftl",
            "commented-preview = Texto de vista previa para comentarios de hover.\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_110);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of(&source_text, "commented-preview");
    assert_hover(
        &mut lsp,
        6_111,
        &source_path,
        key_position,
        "```ftl\n# Comment-only hover coverage\n# Keep this translator guidance visible on key hover\n```",
        key_position.0,
        0,
    );
}

#[test]
fn hover_key_with_local_comments_only_shows_one_local_comment_block() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "local-note = Preview text for hover comments.\n",
        ),
        (
            "locales/es/app.ftl",
            "# Cobertura de hover con comentarios\n# Mantener visible esta nota para traduccion en el hover de clave\nlocal-note = Texto de vista previa para comentarios de hover.\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_112);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of(&source_text, "local-note");
    assert_hover(
        &mut lsp,
        6_113,
        &source_path,
        key_position,
        "```ftl\n# Cobertura de hover con comentarios\n# Mantener visible esta nota para traduccion en el hover de clave\n```",
        key_position.0,
        0,
    );
}

#[test]
fn hover_key_without_comments_returns_no_hover() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "plain-note = Preview text for hover comments.\n",
        ),
        (
            "locales/es/app.ftl",
            "plain-note = Texto de vista previa para comentarios de hover.\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_114);
    open_document(&mut lsp, &source_path, &source_text);

    let hover = request_hover(
        &mut lsp,
        6_115,
        &source_path,
        position_of(&source_text, "plain-note"),
    );
    assert_eq!(hover["result"], Value::Null);
}

#[test]
fn hover_body_without_origin_message_shows_one_local_preview_block() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "welcome-body = Open the latest build.\n",
        ),
        ("locales/es/app.ftl", "local-only = Texto solo local.\n"),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_116);
    open_document(&mut lsp, &source_path, &source_text);

    assert_hover(
        &mut lsp,
        6_117,
        &source_path,
        position_of(&source_text, "Texto solo local."),
        "```ftl\nTexto solo local.\n```",
        0,
        0,
    );
}

#[test]
fn hover_selector_without_origin_selectors_leaves_origin_block_headerless() {
    let workspace = temp_workspace(&[
        ("locales/en/app.ftl", "download-state = Download ready.\n"),
        (
            "locales/es/app.ftl",
            "download-state =\n    { $count ->\n        [one] Descarga lista.\n       *[other] Descargas listas.\n    }\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_118);
    open_document(&mut lsp, &source_path, &source_text);

    assert_hover(
        &mut lsp,
        6_119,
        &source_path,
        position_of(&source_text, "[one] Descarga lista."),
        "```ftl\nDownload ready.\n```\n\n---\n\n`$count=one`\n\n```ftl\nDescarga lista.\n```",
        0,
        0,
    );
}

#[test]
fn hover_selector_without_origin_message_shows_one_local_selector_block() {
    let workspace = temp_workspace(&[
        ("locales/en/app.ftl", "welcome = Hello.\n"),
        (
            "locales/es/app.ftl",
            "download-state =\n    { $count ->\n        [one] Descarga lista.\n       *[other] Descargas listas.\n    }\n",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_134);
    open_document(&mut lsp, &source_path, &source_text);

    assert_hover(
        &mut lsp,
        6_135,
        &source_path,
        position_of(&source_text, "[one] Descarga lista."),
        "`$count=one`\n\n```ftl\nDescarga lista.\n```",
        0,
        0,
    );
}

#[test]
fn hover_body_preview_stays_semantic_across_translation_locales() {
    let root = fixture_root();
    let origin_path = root.join("locales/en/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    let translation_cases = [
        (
            "locales/es/app.ftl",
            "Texto de vista previa para comentarios de hover.",
        ),
        (
            "locales/fr/app.ftl",
            "Texte d apercu pour les commentaires de hover.",
        ),
        ("locales/lv/app.ftl", "Hover komentaru prieksskata teksts."),
        (
            "locales/uk/app.ftl",
            "Текст попереднього перегляду для hover-коментарів.",
        ),
    ];

    let mut lsp = LspProcess::start();
    initialize_lsp(&mut lsp, &root, 60);

    for (index, (relative_path, body_needle)) in translation_cases.iter().enumerate() {
        let path = root.join(relative_path);
        let source = std::fs::read_to_string(&path).unwrap();
        open_document(&mut lsp, &path, &source);
        let hover = request_hover(
            &mut lsp,
            61 + i64::try_from(index).unwrap(),
            &path,
            position_of(&source, body_needle),
        );
        let value = hover["result"]["contents"]["value"].as_str().unwrap();
        assert_hover_block_matches(value, 0, &origin_text, "en", "commented-preview", &[]);
        assert_hover_block_matches(value, 1, &source, &relative_path[8..10], "commented-preview", &[]);
    }
}

#[test]
fn hover_on_uncommented_key_does_not_fall_back_to_body_preview() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();
    initialize_lsp(&mut lsp, &root, 62);
    open_document(&mut lsp, &source_path, &source_text);

    let key_hover = request_hover(
        &mut lsp,
        63,
        &source_path,
        position_of(&source_text, "zero-rollout"),
    );
    assert_eq!(
        key_hover["result"],
        Value::Null,
        "key hover should not degrade into body preview: {key_hover:?}"
    );

    let body_hover = request_hover(
        &mut lsp,
        64,
        &source_path,
        position_of(&source_text, "Zero summary"),
    );
    let body_value = body_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_hover_block_matches(body_value, 0, &source_text, "en", "zero-rollout", &[]);
}

#[test]
fn code_lens_opens_full_selector_combinations_document() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {
                "window": {
                    "showDocument": {
                        "support": true
                    }
                }
            }
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 40);
    assert_eq!(
        initialize["result"]["capabilities"]["codeLensProvider"]["resolveProvider"],
        Value::Bool(false)
    );
    assert_eq!(
        initialize["result"]["capabilities"]["executeCommandProvider"]["commands"][0],
        Value::String("fluent-lsp.showSelectorCombinations".to_string())
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    open_document(&mut lsp, &source_path, &source_text);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = recv_response(&mut lsp, 41);
    let items = lenses["result"]
        .as_array()
        .expect("expected code lens array");
    assert!(items.len() >= 3);
    let install_hint_lens = items
        .iter()
        .find(|item| item["range"]["start"]["line"].as_u64() == Some(8))
        .expect("missing install-hint codelens");
    assert_eq!(
        install_hint_lens["command"]["title"],
        Value::String("Show all 6 selector combinations".to_string())
    );
    assert_eq!(
        install_hint_lens["command"]["command"],
        Value::String("fluent-lsp.showSelectorCombinations".to_string())
    );
    assert_eq!(
        install_hint_lens["range"]["start"]["character"],
        Value::from(0)
    );

    let command = install_hint_lens["command"].clone();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "workspace/executeCommand",
        "params": {
            "command": command["command"],
            "arguments": command["arguments"]
        }
    }));

    let request = lsp.recv();
    assert_eq!(request["method"], "window/showDocument");
    assert_eq!(request["params"]["external"], Value::Bool(false));
    assert_eq!(request["params"]["takeFocus"], Value::Bool(true));
    assert_eq!(
        request["params"]["selection"]["start"]["line"],
        Value::from(0)
    );
    assert_eq!(
        request["params"]["selection"]["start"]["character"],
        Value::from(0)
    );

    let document_uri = request["params"]["uri"]
        .as_str()
        .expect("showDocument uri must be a string");
    let document_file_name = Path::new(
        document_uri
            .strip_prefix("file://")
            .expect("expected file uri for temp document"),
    )
    .file_name()
    .and_then(|name| name.to_str())
    .expect("temp document file name should be valid UTF-8");
    assert!(
        document_file_name
            .strip_prefix("fluent-lsp-selector-combinations-")
            .is_some_and(|tail| tail.ends_with("-install-hint.md")),
        "unexpected temp document uri: {document_uri}"
    );
    let document_path = document_uri
        .strip_prefix("file://")
        .expect("expected file uri for temp document");
    let document_text = std::fs::read_to_string(document_path).expect("read temp document");
    let expected_document = concat!(
        "# Selector combinations for `install-hint`\n\n",
        "Current language: `es`\n\n",
        "Source language: `en`\n\n",
        "Logical file: `app`\n\n",
        "Source text:\n\n",
        "```ftl\n",
        "# Shortcut reminder near the download button\n",
        "```\n\n",
        "```ftl\n",
        "install-hint =\n",
        "    Copy the download link for { $gender ->\n",
        "        [female] her\n",
        "        [male] his\n",
        "       *[other] their\n",
        "    } account on { $count } { $count ->\n",
        "        [one] device\n",
        "       *[other] devices\n",
        "    } now.\n",
        "```\n\n",
        "Source language combinations:\n",
        "`$gender=female`, `$count=one`\n",
        "```ftl\nCopy the download link for her account on { $count } device now.\n```\n",
        "`$gender=female`, `$count=other`\n",
        "```ftl\nCopy the download link for her account on { $count } devices now.\n```\n",
        "`$gender=male`, `$count=one`\n",
        "```ftl\nCopy the download link for his account on { $count } device now.\n```\n",
        "`$gender=male`, `$count=other`\n",
        "```ftl\nCopy the download link for his account on { $count } devices now.\n```\n",
        "`$gender=other`, `$count=one`\n",
        "```ftl\nCopy the download link for their account on { $count } device now.\n```\n",
        "`$gender=other`, `$count=other`\n",
        "```ftl\nCopy the download link for their account on { $count } devices now.\n```\n\n",
        "Current text:\n\n",
        "```ftl\n",
        "install-hint =\n",
        "    Copia el enlace de descarga para la cuenta de { $gender ->\n",
        "        [female] ella\n",
        "        [male] el\n",
        "       *[other] elle\n",
        "    } en { $count } { $count ->\n",
        "        [one] dispositivo\n",
        "       *[other] dispositivos\n",
        "    } ahora.\n",
        "```\n\n",
        "Current language combinations:\n",
        "`$gender=female`, `$count=one`\n",
        "```ftl\nCopia el enlace de descarga para la cuenta de ella en { $count } dispositivo ahora.\n```\n",
        "`$gender=female`, `$count=other`\n",
        "```ftl\nCopia el enlace de descarga para la cuenta de ella en { $count } dispositivos ahora.\n```\n",
        "`$gender=male`, `$count=one`\n",
        "```ftl\nCopia el enlace de descarga para la cuenta de el en { $count } dispositivo ahora.\n```\n",
        "`$gender=male`, `$count=other`\n",
        "```ftl\nCopia el enlace de descarga para la cuenta de el en { $count } dispositivos ahora.\n```\n",
        "`$gender=other`, `$count=one`\n",
        "```ftl\nCopia el enlace de descarga para la cuenta de elle en { $count } dispositivo ahora.\n```\n",
        "`$gender=other`, `$count=other`\n",
        "```ftl\nCopia el enlace de descarga para la cuenta de elle en { $count } dispositivos ahora.\n```",
    );
    assert_eq!(document_text, expected_document);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {
            "success": true
        }
    }));

    let response = recv_response(&mut lsp, 42);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn code_lens_returns_empty_list_for_files_without_selector_combinations() {
    let workspace = temp_workspace(&[
        ("locales/en/app.ftl", "hello = Hello\n"),
        ("locales/es/app.ftl", "hello = Hola\n"),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 42_100);
    open_document(&mut lsp, &source_path, &source_text);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_101,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = recv_response(&mut lsp, 42_101);
    assert_eq!(lenses["result"], Value::Array(Vec::new()));
}

#[test]
fn execute_command_rejects_unknown_command() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 42_102);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_103,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.unknown",
            "arguments": []
        }
    }));

    let response = recv_response(&mut lsp, 42_103);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("unknown command: fluent-lsp.unknown".to_string())
    );
}

#[test]
fn execute_command_rejects_missing_document_uri_argument() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 42_104);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_105,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": []
        }
    }));

    let response = recv_response(&mut lsp, 42_105);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("missing document URI argument".to_string())
    );
}

#[test]
fn execute_command_rejects_missing_fluent_key_argument() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 42_106);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_107,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": [
                format!("file://{}", fixture_root().join("locales/es/app.ftl").display())
            ]
        }
    }));

    let response = recv_response(&mut lsp, 42_107);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("missing Fluent key argument".to_string())
    );
}

#[test]
fn execute_command_rejects_non_file_document_uris() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 42_108);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_109,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": ["untitled://scratch", "install-hint"]
        }
    }));

    let response = recv_response(&mut lsp, 42_109);
    assert_eq!(response["error"]["code"], Value::from(-32602));
    assert_eq!(
        response["error"]["message"],
        Value::String("document URI must point to a file".to_string())
    );
}

#[test]
fn code_action_generates_prefix_selector_by_default() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 70);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        71,
        &source_path,
        position_of(&source_text, "coins-line"),
    );
    let actions = rewrite_actions_only(actions);
    assert_eq!(actions.len(), 3);
    let action = find_code_action(&actions, "Generate number selector (prefix)");
    assert_eq!(
        action["title"],
        Value::String("Generate number selector (prefix)".to_string())
    );
    assert_eq!(
        action["kind"],
        Value::String("refactor.rewrite".to_string())
    );
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Tienes { $coins } { $coins ->\n    [one] monedas.\n    *[other] monedas.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_returns_all_styles_for_variable_occurrence() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 72);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        73,
        &source_path,
        position_of(&source_text, "{ $coins }"),
    );
    let actions = rewrite_actions_only(actions);
    assert_eq!(actions.len(), 3);
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Generate number selector from $coins (prefix)".to_string())));
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Generate number selector from $coins (whole)".to_string())));
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Generate number selector from $coins (suffix)".to_string())));
}

#[test]
fn code_action_uses_client_selector_style_setting() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 74);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "workspace/didChangeConfiguration",
        "params": {
            "settings": {
                "fluent-lsp": {
                    "selector_style": "whole"
                }
            }
        }
    }));
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        75,
        &source_path,
        position_of(&source_text, "{ $coins }"),
    );
    let actions = rewrite_actions_only(actions);
    let action = &actions[0];
    assert_eq!(
        action["title"],
        Value::String("Generate number selector from $coins (whole)".to_string())
    );
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "{ $coins ->\n    [one] Tienes { $coins } monedas.\n    *[other] Tienes { $coins } monedas.\n}"
                .to_string()
        )
    );
}

#[test]
fn file_config_selector_style_overrides_client_setting() {
    let fixture = fixture_root();
    let temp = tempdir().unwrap();
    copy_dir(&fixture, temp.path());
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\nselector_style = \"whole\"\n",
    )
    .unwrap();

    let source_path = temp.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 76);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "workspace/didChangeConfiguration",
        "params": {
            "settings": {
                "fluent-lsp": {
                    "selector_style": "prefix"
                }
            }
        }
    }));
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        77,
        &source_path,
        position_of(&source_text, "coins-line"),
    );
    let actions = rewrite_actions_only(actions);
    let action = &actions[0];
    assert_eq!(
        action["title"],
        Value::String("Generate number selector (whole)".to_string())
    );
}

#[test]
fn code_action_uses_snippet_text_edit_when_supported() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp_with_capabilities(
        &root,
        78,
        json!({
            "workspace": {
                "workspaceEdit": {
                    "documentChanges": true,
                    "snippetEditSupport": true
                }
            }
        }),
    );
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        79,
        &source_path,
        position_of(&source_text, "coins-line"),
    );
    let action = find_code_action(&actions, "Generate number selector (prefix)");
    assert_eq!(
        action["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            "Tienes { \\$${1:coins} } { \\$${1:coins} ->\n    [one] monedas.\n    *[other] monedas.\n}"
                .to_string()
        )
    );
    assert!(action["edit"]["changes"].is_null());
}

#[test]
fn code_action_generates_whole_snippet_when_no_variable_exists() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp_with_capabilities(
        &root,
        80,
        json!({
            "workspace": {
                "workspaceEdit": {
                    "documentChanges": true,
                    "snippetEditSupport": true
                }
            }
        }),
    );
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        81,
        &source_path,
        position_of(&source_text, "plain-count"),
    );
    let actions = rewrite_actions_only(actions);
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0]["title"],
        Value::String("Generate number selector (whole)".to_string())
    );
    assert_eq!(
        actions[0]["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            "{ \\$${1:count} ->\n    [one] Monedas disponibles.\n    *[other] Monedas disponibles.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_supports_attributes() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp_with_capabilities(
        &root,
        82,
        json!({
            "workspace": {
                "workspaceEdit": {
                    "documentChanges": true,
                    "snippetEditSupport": true
                }
            }
        }),
    );
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        83,
        &source_path,
        position_of(&source_text, "$files"),
    );
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Generate number selector from $files (prefix)".to_string())));
}

#[test]
fn code_action_uses_enclosing_function_placeable_as_generation_anchor() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp_with_capabilities(
        &root,
        84,
        json!({
            "workspace": {
                "workspaceEdit": {
                    "documentChanges": true,
                    "snippetEditSupport": true
                }
            }
        }),
    );
    open_document(&mut lsp, &source_path, &source_text);

    let key_actions = request_code_actions(
        &mut lsp,
        85,
        &source_path,
        position_of(&source_text, "formatted-download"),
    );
    let key_actions = rewrite_actions_only(key_actions);
    assert_eq!(key_actions.len(), 3);
    let key_prefix = find_code_action(&key_actions, "Generate number selector (prefix)");
    assert_eq!(
        key_prefix["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            "Descarga { NUMBER(\\$${1:downloads}) } { \\$${1:downloads} ->\n    [one] archivos.\n    *[other] archivos.\n}"
                .to_string()
        )
    );

    let variable_actions = request_code_actions(
        &mut lsp,
        91,
        &source_path,
        position_of(&source_text, "$downloads"),
    );
    let variable_actions = rewrite_actions_only(variable_actions);
    assert_eq!(variable_actions.len(), 3);
    assert_eq!(
        find_code_action(&variable_actions, "Generate number selector from $downloads (prefix)")["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            "Descarga { NUMBER($downloads) } { $downloads ->\n    [one] archivos.\n    *[other] archivos.\n}"
                .to_string()
        )
    );

    let function_actions = request_code_actions(
        &mut lsp,
        104,
        &source_path,
        position_of(&source_text, "NUMBER($downloads)"),
    );
    let function_actions = rewrite_actions_only(function_actions);
    assert_eq!(function_actions.len(), 3);
    assert_eq!(
        find_code_action(&function_actions, "Generate number selector from NUMBER($downloads) (prefix)")["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            "Descarga { NUMBER($downloads) } { NUMBER($downloads) ->\n    [one] archivos.\n    *[other] archivos.\n}"
                .to_string()
        )
    );

    let deep_variable_actions = request_code_actions(
        &mut lsp,
        105,
        &source_path,
        position_of(&source_text, "WRAP(NUMBER($downloads))"),
    );
    let deep_variable_actions = rewrite_actions_only(deep_variable_actions);
    assert_eq!(deep_variable_actions.len(), 3);
    assert_eq!(
        find_code_action(&deep_variable_actions, "Generate number selector from WRAP(NUMBER($downloads)) (prefix)")["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            "Descarga { WRAP(NUMBER($downloads)) } { WRAP(NUMBER($downloads)) ->\n    [one] archivos.\n    *[other] archivos.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_keeps_punctuation_attached_in_prefix_generation() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 92);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        93,
        &source_path,
        position_of(&source_text, "coins-period"),
    );
    let action = find_code_action(&actions, "Generate number selector (prefix)");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String("Tienes { $coins } { $coins ->\n    [one].\n    *[other].\n}".to_string())
    );
}

#[test]
fn code_action_generation_is_absent_when_message_already_has_selector() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 116);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        117,
        &source_path,
        position_of(&source_text, "whole-coins"),
    );
    let mut titles = rewrite_actions_only(actions)
        .into_iter()
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    titles.sort();

    assert_eq!(
        titles,
        vec![
            "Convert selector to prefix form".to_string(),
            "Convert selector to suffix form".to_string(),
        ]
    );
}

#[test]
fn code_action_generation_is_absent_when_attribute_already_has_selector() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 118);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        119,
        &source_path,
        position_of(&source_text, "{ $files ->"),
    );
    let mut titles = rewrite_actions_only(actions)
        .into_iter()
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    titles.sort();

    assert_eq!(
        titles,
        vec![
            "Convert selector to prefix form".to_string(),
            "Convert selector to suffix form".to_string(),
        ]
    );
}

#[test]
fn code_action_rewrites_whole_selector_to_prefix_and_suffix() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 94);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        95,
        &source_path,
        position_of(&source_text, "whole-coins"),
    );
    assert!(
        actions.iter().any(|action| action["title"]
            == Value::String("Convert selector to prefix form".to_string()))
    );
    assert!(
        actions.iter().any(|action| action["title"]
            == Value::String("Convert selector to suffix form".to_string()))
    );

    let prefix = find_code_action(&actions, "Convert selector to prefix form");
    assert_eq!(
        prefix["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Tienes { $coins } { $coins ->\n    [one] moneda.\n    *[other] monedas.\n}"
                .to_string()
        )
    );

    let suffix = find_code_action(&actions, "Convert selector to suffix form");
    assert_eq!(
        suffix["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Tienes { $coins ->\n    [one] { $coins } moneda.\n    *[other] { $coins } monedas.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_rewrites_prefix_selector_to_whole() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 96);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        97,
        &source_path,
        position_of(&source_text, "prefix-coins"),
    );
    let action = find_code_action(&actions, "Convert selector to whole form");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "{ $coins ->\n    [one] Tienes { $coins } moneda.\n    *[other] Tienes { $coins } monedas.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_rewrites_suffix_selector_to_whole_and_prefix() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 98);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        99,
        &source_path,
        position_of(&source_text, "suffix-coins"),
    );
    assert!(actions.iter().any(
        |action| action["title"] == Value::String("Convert selector to whole form".to_string())
    ));
    assert!(
        actions.iter().any(|action| action["title"]
            == Value::String("Convert selector to prefix form".to_string()))
    );
}

#[test]
fn code_action_bare_suffix_like_selector_only_offers_prefix() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 100);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        101,
        &source_path,
        position_of(&source_text, "bare-suffix-coins"),
    );
    let actions = rewrite_actions_only(actions);
    assert_eq!(actions.len(), 1);
    let action = find_code_action(&actions, "Convert selector to prefix form");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "{ $coins } { $coins ->\n    [one] moneda.\n    *[other] monedas.\n}".to_string()
        )
    );
}

#[test]
fn code_action_rewrites_nested_whole_selector_inside_variant() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 102);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        103,
        &source_path,
        position_of_nth(&source_text, "Ella tiene", 2),
    );
    let action = find_code_action(&actions, "Convert selector to prefix form");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Ella tiene { $coins } { $coins ->\n            [one] moneda.\n            *[other] monedas.\n        }"
                .to_string()
        )
    );
    assert!(actions.iter().any(|candidate| candidate["title"]
        == Value::String("Convert selector to suffix form".to_string())));
}

#[test]
fn code_action_rewrite_is_absent_when_message_has_no_selector() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 120);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        121,
        &source_path,
        position_of(&source_text, "coins-line"),
    );
    let mut titles = rewrite_actions_only(actions)
        .into_iter()
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    titles.sort();

    assert_eq!(
        titles,
        vec![
            "Generate number selector (prefix)".to_string(),
            "Generate number selector (suffix)".to_string(),
            "Generate number selector (whole)".to_string(),
        ]
    );
}

#[test]
fn code_action_rewrite_is_absent_when_attribute_has_no_selector() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 122);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        123,
        &source_path,
        position_of(&source_text, "$files"),
    );
    let mut titles = rewrite_actions_only(actions)
        .into_iter()
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    titles.sort();

    assert_eq!(
        titles,
        vec![
            "Generate number selector from $files (prefix)".to_string(),
            "Generate number selector from $files (suffix)".to_string(),
            "Generate number selector from $files (whole)".to_string(),
        ]
    );
}

#[test]
fn code_action_rewrites_selector_inside_attribute_value() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 110);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        111,
        &source_path,
        position_of(&source_text, "{ $files ->"),
    );
    let prefix = find_code_action(&actions, "Convert selector to prefix form");
    assert_eq!(
        prefix["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Descarga { $files } { $files ->\n        [one] archivo.\n        *[other] archivos.\n    }"
                .to_string()
        )
    );
    let suffix = find_code_action(&actions, "Convert selector to suffix form");
    assert_eq!(
        suffix["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Descarga { $files ->\n        [one] { $files } archivo.\n        *[other] { $files } archivos.\n    }"
                .to_string()
        )
    );
}

#[test]
fn attribute_rewrite_range_does_not_consume_comments_or_attribute_key() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 112);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        113,
        &source_path,
        position_of(&source_text, "{ $files ->"),
    );
    let prefix = find_code_action(&actions, "Convert selector to prefix form");
    let edit = &prefix["edit"]["changes"][format!("file://{}", source_path.display())][0];
    assert_eq!(edit["range"]["start"]["line"], Value::from(73));
    assert_eq!(edit["range"]["start"]["character"], Value::from(15));
}

#[test]
fn code_action_rewrites_selected_count_selector_with_trailing_suffix_text() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 106);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        107,
        &source_path,
        position_of(&source_text, "{ $count ->"),
    );
    assert!(actions.iter().any(
        |action| action["title"] == Value::String("Convert selector to whole form".to_string())
    ));
    let suffix = find_code_action(&actions, "Convert selector to suffix form");
    assert_eq!(
        suffix["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count ->\n    [one] { $count } device now.\n    *[other] { $count } devices now.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_rewrites_selected_gender_selector_to_whole_with_nested_count_preserved() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 108);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        109,
        &source_path,
        position_of(&source_text, "{ $gender ->"),
    );
    assert_eq!(actions.len(), 1);
    let whole = find_code_action(&actions, "Convert selector to whole form");
    assert_eq!(
        whole["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            " { $gender ->\n    [female] Copy the download link for her account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n    [male] Copy the download link for his account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n    *[other] Copy the download link for their account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n}"
                .to_string()
        )
    );
}

#[test]
fn code_action_is_hidden_for_ambiguous_message_keys() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 86);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions_allow_empty(
        &mut lsp,
        87,
        &source_path,
        position_of(&source_text, "range-summary"),
    );
    let actions = rewrite_actions_only(actions);
    assert!(actions.is_empty());

    let nested_actions = request_code_actions_allow_empty(
        &mut lsp,
        88,
        &source_path,
        position_of(&source_text, "nested-coins"),
    );
    let nested_actions = rewrite_actions_only(nested_actions);
    assert!(nested_actions.is_empty());
}

#[test]
fn diagnostics_are_absent_by_default() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 114);
    open_document(&mut lsp, &source_path, &source_text);
}

#[test]
fn diagnostics_report_unsupported_and_missing_categories_when_enabled() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 115);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": true,
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["message"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();

    assert_eq!(diagnostics.len(), 4);
    let mut sorted_messages = messages.clone();
    sorted_messages.sort();
    assert_eq!(
        sorted_messages,
        vec![
            "Numeric selector for `lv` is missing category `one`".to_string(),
            "Numeric selector for `lv` is missing category `zero`".to_string(),
            "Numeric selector for `lv` is missing category `zero`".to_string(),
            "`few` is not a supported plural category for `lv`".to_string(),
        ]
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| *message == "Numeric selector for `lv` is missing category `zero`")
            .count(),
        2
    );

    let unsupported = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic["message"]
                == Value::String("`few` is not a supported plural category for `lv`".to_string())
        })
        .unwrap();
    assert_eq!(unsupported["severity"], Value::from(1));
    assert_eq!(
        unsupported["range"]["start"]["line"],
        Value::from(position_of(&source_text, "few").0)
    );
    assert_eq!(
        unsupported["range"]["start"]["character"],
        Value::from(position_of(&source_text, "few").1)
    );
}

#[test]
fn diagnostics_report_selector_style_mismatches_when_enabled() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 116);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_selector_style_mismatch": true,
                "selector_style": "prefix"
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["message"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();

    let mut sorted_messages = messages;
    sorted_messages.sort();
    assert_eq!(
        sorted_messages,
        vec![
            "Selector style is `suffix`, but workspace prefers `prefix`".to_string(),
            "Selector style is `whole`, but workspace prefers `prefix`".to_string(),
        ]
    );
}

#[test]
fn file_config_overrides_client_style_diagnostic_settings() {
    let temp = tempdir().unwrap();
    copy_dir(&fixture_root(), temp.path());
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\nselector_style = \"whole\"\nwarn_on_selector_style_mismatch = true\n",
    )
    .unwrap();

    let source_path = temp.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 117);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_selector_style_mismatch": false,
                "selector_style": "prefix"
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["message"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();

    let mut sorted_messages = messages;
    sorted_messages.sort();
    assert_eq!(
        sorted_messages,
        vec![
            "Selector style is `prefix`, but workspace prefers `whole`".to_string(),
            "Selector style is `suffix`, but workspace prefers `whole`".to_string(),
        ]
    );
}

#[test]
fn diagnostics_report_invalid_numeric_identifier_keys_when_enabled() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = "bad-key =\n    { $count ->\n        [admins] nope\n        [one] ok\n       *[other] ok\n    }\n";

    let mut lsp = initialized_lsp(&root, 118);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["message"],
        Value::String(
            "`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories"
                .to_string(),
        )
    );
    assert_eq!(diagnostics[0]["severity"], Value::from(1));
}

#[test]
fn diagnostics_ignore_non_numeric_admin_other_selector() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text =
        "bad-key =\n    { $count ->\n        [admins] nope\n       *[other] ok\n    }\n";

    let mut lsp = initialized_lsp(&root, 122);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": true,
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn diagnostics_do_not_warn_for_complete_numeric_selectors_or_matching_style() {
    let temp = tempdir().unwrap();
    copy_dir(&fixture_root(), temp.path());
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\nwarn_on_missing_plural_categories = true\nwarn_on_selector_style_mismatch = true\nselector_style = \"whole\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/match.ftl"),
        "match-rollout =\n    { $count ->\n        [one] one package\n       *[other] { $count } packages\n    }\n",
    )
    .unwrap();

    let source_path = temp.path().join("locales/en/match.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 123);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn parse_error_diagnostics_publish_on_save_and_clear_after_fix() {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();

    let source_path = temp.path().join("locales/en/broken.ftl");
    let invalid_source = "welcome-title = Welcome\n\ng@Rb@ge = broken\n";
    std::fs::write(&source_path, invalid_source).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 124);
    send_open_document(&mut lsp, &source_path, invalid_source);
    send_save_document(&mut lsp, &source_path, None);

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["message"],
        Value::String("Fluent syntax error: Expected a token starting with \"=\"".to_string())
    );
    assert_eq!(diagnostics[0]["severity"], Value::from(1));
    assert_eq!(
        diagnostics[0]["range"]["start"]["line"],
        Value::from(position_of(invalid_source, "@").0)
    );
    assert_eq!(
        diagnostics[0]["range"]["start"]["character"],
        Value::from(position_of(invalid_source, "@").1)
    );

    let valid_source = "welcome-title = Welcome\n\ngarbage = broken\n";
    send_change_document(&mut lsp, &source_path, 2, valid_source);
    send_save_document(&mut lsp, &source_path, None);

    let cleared = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(cleared["params"]["diagnostics"], Value::Array(Vec::new()));
}

#[test]
fn diagnostics_report_local_selector_style_mismatches_when_enabled() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n       *[fallback] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

    let mut lsp = initialized_lsp(&root, 119);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_selector_style_mismatch": true,
                "selector_style": "prefix"
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["message"],
        Value::String("Selector style is `whole`, but workspace prefers `prefix`".to_string())
    );
}

#[test]
fn diagnostics_use_unicode_plural_categories_for_ukrainian() {
    let root = fixture_root();
    let source_path = root.join("locales/uk/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 120);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": true,
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["message"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        messages,
        vec!["Numeric selector for `uk` is missing category `one`".to_string()]
    );
}

#[test]
fn did_close_clears_document_diagnostics() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 121);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": true,
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));
    let _ = recv_notification(&mut lsp, "textDocument/publishDiagnostics");

    send_close_document(&mut lsp, &source_path);
    let notification = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["uri"],
        Value::String(format!("file://{}", source_path.display()))
    );
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn code_action_preserves_nested_selector_when_generating_inside_variant() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 89);
    open_document(&mut lsp, &source_path, &source_text);

    let actions = request_code_actions(
        &mut lsp,
        90,
        &source_path,
        position_of_nth(&source_text, "$coins", 2),
    );
    let action = find_code_action(&actions, "Generate number selector from $coins (prefix)");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())][0]["newText"],
        Value::String(
            "Ella tiene { $coins } { $coins ->\n            [one] monedas.\n            *[other] monedas.\n        }"
                .to_string()
        )
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

fn initialized_lsp(root: &Path, request_id: i64) -> LspProcess {
    initialized_lsp_with_capabilities(root, request_id, json!({}))
}

fn initialized_lsp_with_capabilities(
    root: &Path,
    request_id: i64,
    capabilities: Value,
) -> LspProcess {
    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": capabilities
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], request_id);
    assert_eq!(
        initialize["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
        Value::Array(vec![
            Value::String("quickfix".to_string()),
            Value::String("refactor.rewrite".to_string()),
        ])
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");

    lsp
}

fn send_open_document(lsp: &mut LspProcess, path: &Path, text: &str) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": format!("file://{}", path.display()),
                "languageId": "fluent",
                "version": 1,
                "text": text
            }
        }
    }));
}

fn send_save_document(lsp: &mut LspProcess, path: &Path, text: Option<&str>) {
    let params = if let Some(text) = text {
        json!({
            "textDocument": {
                "uri": format!("file://{}", path.display())
            },
            "text": text
        })
    } else {
        json!({
            "textDocument": {
                "uri": format!("file://{}", path.display())
            }
        })
    };
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didSave",
        "params": params
    }));
}

fn send_change_document(lsp: &mut LspProcess, path: &Path, version: i32, text: &str) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": format!("file://{}", path.display()),
                "version": version
            },
            "contentChanges": [
                {
                    "text": text
                }
            ]
        }
    }));
}

fn send_close_document(lsp: &mut LspProcess, path: &Path) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": {
            "textDocument": {
                "uri": format!("file://{}", path.display())
            }
        }
    }));
}

fn open_document(lsp: &mut LspProcess, path: &Path, text: &str) {
    send_open_document(lsp, path, text);
    send_save_document(lsp, path, Some(text));
    loop {
        let notification = recv_notification(lsp, "textDocument/publishDiagnostics");
        if notification["params"]["uri"] == Value::String(format!("file://{}", path.display())) {
            break;
        }
    }
}

fn recv_notification(lsp: &mut LspProcess, method: &str) -> Value {
    loop {
        let message = lsp.recv();
        if message["method"] == Value::String(method.to_string()) {
            return message;
        }
    }
}

fn recv_response(lsp: &mut LspProcess, request_id: i64) -> Value {
    loop {
        let message = lsp.recv();
        if message["id"] == Value::from(request_id) {
            return message;
        }
    }
}

fn change_configuration(lsp: &mut LspProcess, settings: Value) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "workspace/didChangeConfiguration",
        "params": {
            "settings": settings
        }
    }));
}

fn request_completion_labels(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
) -> Vec<String> {
    request_completion_items(lsp, request_id, source_path, position)
        .into_iter()
        .filter_map(|item| item["label"].as_str().map(ToString::to_string))
        .collect()
}

fn request_completion_items(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
) -> Vec<Value> {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));

    let response = recv_response(lsp, request_id);
    if response["result"].is_array() {
        response["result"].as_array().cloned().unwrap_or_default()
    } else {
        response["result"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

fn request_code_actions(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
) -> Vec<Value> {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": position.0, "character": position.1 },
                "end": { "line": position.0, "character": position.1 }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));

    let response = recv_response(lsp, request_id);
    response["result"]
        .as_array()
        .expect("expected code action array")
        .clone()
}

fn request_code_actions_allow_empty(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
) -> Vec<Value> {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": position.0, "character": position.1 },
                "end": { "line": position.0, "character": position.1 }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));

    let response = recv_response(lsp, request_id);
    response["result"].as_array().cloned().unwrap_or_default()
}

fn find_code_action<'a>(actions: &'a [Value], title: &str) -> &'a Value {
    actions
        .iter()
        .find(|action| action["title"] == Value::String(title.to_string()))
        .unwrap_or_else(|| panic!("missing code action: {title}"))
}

fn copy_dir(source: &Path, dest: &Path) {
    std::fs::create_dir_all(dest).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &dest_path);
        } else {
            std::fs::copy(&source_path, &dest_path).unwrap();
        }
    }
}

fn temp_workspace(files: &[(&str, &str)]) -> TempDir {
    let temp = tempdir().unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();

    for (relative_path, contents) in files {
        let path = temp.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    temp
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

    let references = recv_response(lsp, request_id);
    let items = references["result"]
        .as_array()
        .expect("expected references array");
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

fn assert_references_are_empty(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
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

    let references = recv_response(lsp, request_id);
    assert_eq!(references["result"], Value::Array(Vec::new()));
}

fn assert_references_are_absent(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
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

    let references = recv_response(lsp, request_id);
    assert_eq!(references["result"], Value::Null);
}

fn assert_hover(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
    expected_value: &str,
    expected_line: u32,
    expected_character: u32,
) -> String {
    let hover = request_hover(lsp, request_id, source_path, position);
    assert_eq!(
        hover["result"]["contents"]["kind"],
        Value::String("markdown".to_string())
    );
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
    hover["result"]["contents"]["value"]
        .as_str()
        .unwrap()
        .to_string()
}

fn request_hover(
    lsp: &mut LspProcess,
    request_id: i64,
    source_path: &Path,
    position: (u32, u32),
) -> Value {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));

    recv_response(lsp, request_id)
}

fn assert_fluent_parses(source: &str) {
    if let Err((_, errors)) = parser::parse(source) {
        panic!("failed to parse Fluent source with {errors:?}\n{source}");
    }
}

fn runtime_message_text(source: &str, locale: &str, key: &str) -> String {
    runtime_message_text_with_overrides(source, locale, key, &[])
}

fn runtime_message_text_with_overrides(
    source: &str,
    locale: &str,
    key: &str,
    overrides: &[(&str, &str)],
) -> String {
    let resource = FluentResource::try_new(source.to_string()).unwrap_or_else(|(_, errors)| {
        panic!("failed to build FluentResource with {errors:?}\n{source}")
    });
    let parsed = parser::parse(source)
        .unwrap_or_else(|(_, errors)| panic!("failed to parse Fluent source with {errors:?}\n{source}"))
        ;
    let parsed_pattern = find_runtime_pattern(&parsed, key)
        .unwrap_or_else(|| panic!("missing parsed pattern `{key}` in runtime source"));
    let locale: LanguageIdentifier = locale.parse().expect("valid language identifier");
    let mut bundle = FluentBundle::new(vec![locale]);
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .unwrap_or_else(|errors| panic!("failed to add Fluent resource to bundle: {errors:?}"));

    let (message_key, attribute_key) = split_runtime_key(key);
    let message = bundle
        .get_message(message_key)
        .unwrap_or_else(|| panic!("missing message `{message_key}` in runtime bundle"));
    let pattern = if let Some(attribute_key) = attribute_key {
        message
            .attributes()
            .find(|attribute| attribute.id() == attribute_key)
            .unwrap_or_else(|| panic!("missing attribute `{attribute_key}` on `{message_key}`"))
            .value()
    } else {
        message
            .value()
            .unwrap_or_else(|| panic!("message `{message_key}` has no value"))
    };
    let override_map = overrides
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect::<HashMap<_, _>>();
    let mut args = FluentArgs::new();
    collect_runtime_selector_args(parsed_pattern, &override_map, &mut args);
    let mut errors = Vec::new();
    let rendered = bundle
        .format_pattern(pattern, Some(&args), &mut errors)
        .into_owned();
    assert!(
        errors.is_empty(),
        "runtime formatting errors for `{key}`: {errors:?}"
    );
    rendered
}

fn find_runtime_pattern<'a>(
    resource: &'a fluent_syntax::ast::Resource<&'a str>,
    key: &str,
) -> Option<&'a fluent_syntax::ast::Pattern<&'a str>> {
    let (entry_key, attribute_key) = split_runtime_key(key);
    resource.body.iter().find_map(|entry| match entry {
        fluent_syntax::ast::Entry::Message(message) if entry_key == message.id.name => {
            if let Some(attribute_key) = attribute_key {
                message
                    .attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| &attribute.value)
            } else {
                message.value.as_ref()
            }
        }
        fluent_syntax::ast::Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
            if let Some(attribute_key) = attribute_key {
                term.attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| &attribute.value)
            } else {
                Some(&term.value)
            }
        }
        _ => None,
    })
}

fn collect_runtime_selector_args(
    pattern: &fluent_syntax::ast::Pattern<&str>,
    overrides: &HashMap<String, String>,
    args: &mut FluentArgs<'_>,
) {
    for element in &pattern.elements {
        let fluent_syntax::ast::PatternElement::Placeable { expression, .. } = element else {
            continue;
        };
        collect_runtime_selector_args_from_expression(expression, overrides, args);
    }
}

fn collect_runtime_selector_args_from_expression(
    expression: &fluent_syntax::ast::Expression<&str>,
    overrides: &HashMap<String, String>,
    args: &mut FluentArgs<'_>,
) {
    let fluent_syntax::ast::Expression::Select {
        selector, variants, ..
    } = expression
    else {
        return;
    };
    let fluent_syntax::ast::InlineExpression::VariableReference { id, .. } = selector else {
        return;
    };

    let selector_name = format!("${}", id.name);
    let selected_key = overrides
        .get(&selector_name)
        .cloned()
        .or_else(|| {
            variants
                .iter()
                .find(|variant| variant.default)
                .or_else(|| variants.first())
                .map(|variant| runtime_variant_key(&variant.key))
        })
        .unwrap_or_else(|| panic!("missing selectable variant for `{selector_name}`"));
    args.set(id.name.to_string(), runtime_selector_value(&selected_key));

    let selected_variant = variants
        .iter()
        .find(|variant| runtime_variant_key(&variant.key) == selected_key)
        .or_else(|| variants.iter().find(|variant| variant.default))
        .or_else(|| variants.first())
        .unwrap_or_else(|| panic!("missing selected variant `{selected_key}` for `{selector_name}`"));
    collect_runtime_selector_args(&selected_variant.value, overrides, args);
}

fn runtime_variant_key(key: &fluent_syntax::ast::VariantKey<&str>) -> String {
    match key {
        fluent_syntax::ast::VariantKey::Identifier { name, .. } => (*name).to_string(),
        fluent_syntax::ast::VariantKey::NumberLiteral { value, .. } => (*value).to_string(),
    }
}

fn runtime_selector_value(selected_key: &str) -> FluentValue<'static> {
    match selected_key {
        "zero" => 0i64.into(),
        "one" => 1i64.into(),
        "two" => 2i64.into(),
        "few" => 3i64.into(),
        "many" => 5i64.into(),
        "other" => 2i64.into(),
        _ => selected_key
            .parse::<i64>()
            .map(FluentValue::from)
            .unwrap_or_else(|_| selected_key.to_string().into()),
    }
}

fn split_runtime_key(key: &str) -> (&str, Option<&str>) {
    match key.rsplit_once('.') {
        Some((message_key, attribute_key)) if !attribute_key.is_empty() => {
            (message_key, Some(attribute_key))
        }
        _ => (key, None),
    }
}

fn extract_ftl_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = markdown;
    let opener = "```ftl\n";
    while let Some(start) = rest.find(opener) {
        let after_start = &rest[start + opener.len()..];
        let end = after_start
            .find("\n```")
            .expect("unterminated ftl markdown block");
        blocks.push(after_start[..end].to_string());
        rest = &after_start[end + "\n```".len()..];
    }
    blocks
}

fn assert_hover_block_matches(
    markdown: &str,
    block_index: usize,
    source: &str,
    locale: &str,
    key: &str,
    overrides: &[(&str, &str)],
) {
    assert_fluent_parses(source);
    let expected = runtime_message_text_with_overrides(source, locale, key, overrides);
    let blocks = extract_ftl_blocks(markdown);
    let actual = blocks
        .get(block_index)
        .unwrap_or_else(|| panic!("missing hover block {block_index} in {markdown}"));
    assert_eq!(actual, &expected);
}

fn comment_hover_markdown(blocks: &[&str]) -> String {
    blocks
        .into_iter()
        .map(|block| format!("```ftl\n{block}\n```"))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

fn initialize_lsp(lsp: &mut LspProcess, root: &Path, request_id: i64) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));
    let initialize = lsp.recv();
    assert_eq!(initialize["id"], request_id);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let initialized_log = lsp.recv();
    assert_eq!(initialized_log["method"], "window/logMessage");
}

fn completion_workspace() -> TempDir {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("locales/en/dialogs")).unwrap();
    std::fs::create_dir_all(temp.path().join("locales/es/dialogs")).unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        "hello-world = Hello\n\
download-action = Download\n\
download-count = Download count\n\
\n\
# Completion doc coverage\n\
# Keep this note in completion hover\n\
commented-preview = Preview text for completion docs.\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/dialogs/menu.ftl"),
        "# Menu completion documentation\n\
# Keep this entry visible in completion hover\n\
menu-save =\n    .label = Save\n    .tooltip = Save this file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        "welcome-title = Bienvenido\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/dialogs/menu.ftl"),
        "menu-save =\n    .label = Guardar\n",
    )
    .unwrap();
    temp
}

fn missing_entry_workspace() -> TempDir {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(temp.path().join("locales/es")).unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        concat!(
            "hello = Hello World\n",
            "\n",
            "menu-save =\n",
            "    .label = Save\n",
            "    .tooltip = Save this file\n",
            "\n",
            "sync-status = Sync ready\n",
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        concat!(
            "hello = Hola Mundo\n",
            "menu-save =\n",
            "    .label = Guardar\n",
        ),
    )
    .unwrap();
    temp
}

fn copy_marker_workspace() -> TempDir {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(temp.path().join("locales/es")).unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        concat!(
            "hello = Hello World\n",
            "\n",
            "download-action =\n",
            "    .label = Download\n",
            "    .tooltip = Download this build\n",
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        concat!(
            "# [LSP-COPY]\n",
            "hello = Hola Mundo\n",
            "\n",
            "# [LSP-COPY .tooltip]\n",
            "download-action =\n",
            "    .label = Descargar\n",
            "    .tooltip = Download this build\n",
        ),
    )
    .unwrap();
    temp
}

fn single_key_copy_workspace() -> TempDir {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(temp.path().join("locales/es")).unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        concat!(
            "hello = Hello World\n",
            "download-action =\n",
            "    .label = Download\n",
            "    .tooltip = Download this build\n",
            "\n",
            "sync-status = Sync ready\n",
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        concat!(
            "hello = { \"\" }\n",
            "download-action =\n",
            "    .label = Descargar\n",
            "\n",
            "sync-status = { \"\" }\n",
        ),
    )
    .unwrap();
    temp
}

fn apply_code_action_edit(source: &str, action: &Value, target_uri: &str) -> String {
    let mut updated = source.to_string();
    let mut edits = action["edit"]["changes"][target_uri]
        .as_array()
        .cloned()
        .expect("expected workspace edit changes");
    edits.sort_by_key(|text_edit| {
        (
            std::cmp::Reverse(text_edit["range"]["start"]["line"].as_u64().unwrap()),
            std::cmp::Reverse(text_edit["range"]["start"]["character"].as_u64().unwrap()),
            std::cmp::Reverse(text_edit["range"]["end"]["line"].as_u64().unwrap()),
            std::cmp::Reverse(text_edit["range"]["end"]["character"].as_u64().unwrap()),
        )
    });

    for text_edit in edits {
        let start = position_to_offset(
            &updated,
            (
                text_edit["range"]["start"]["line"].as_u64().unwrap() as u32,
                text_edit["range"]["start"]["character"].as_u64().unwrap() as u32,
            ),
        );
        let end = position_to_offset(
            &updated,
            (
                text_edit["range"]["end"]["line"].as_u64().unwrap() as u32,
                text_edit["range"]["end"]["character"].as_u64().unwrap() as u32,
            ),
        );
        updated.replace_range(start..end, text_edit["newText"].as_str().unwrap());
    }

    updated
}

fn rewrite_actions_only(actions: Vec<Value>) -> Vec<Value> {
    actions
        .into_iter()
        .filter(|action| action["kind"] == Value::String("refactor.rewrite".to_string()))
        .collect()
}

fn position_to_offset(source: &str, position: (u32, u32)) -> usize {
    let mut offset = 0usize;
    let mut line = 0u32;
    let mut character = 0u32;
    for ch in source.chars() {
        if line == position.0 && character == position.1 {
            return offset;
        }
        offset += ch.len_utf8();
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }
    if line == position.0 && character == position.1 {
        offset
    } else {
        panic!("position {position:?} is outside source")
    }
}

fn position_of(source: &str, needle: &str) -> (u32, u32) {
    position_of_nth(source, needle, 1)
}

fn position_after(source: &str, needle: &str) -> (u32, u32) {
    let (line, character) = position_of(source, needle);
    (
        line,
        character + u32::try_from(needle.chars().count()).unwrap(),
    )
}

fn position_of_nth(source: &str, needle: &str, instance: usize) -> (u32, u32) {
    let mut search_offset = 0usize;
    let mut found_offset = None;
    for _ in 0..instance {
        let relative = source[search_offset..]
            .find(needle)
            .expect("needle not found in source");
        let absolute = search_offset + relative;
        found_offset = Some(absolute);
        search_offset = absolute + needle.len();
    }
    let offset = found_offset.expect("expected at least one match");
    let prefix = &source[..offset];
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).unwrap();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let character = u32::try_from(source[line_start..offset].chars().count()).unwrap();
    (line, character)
}
