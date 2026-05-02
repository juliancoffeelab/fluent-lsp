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
fn goto_definition_from_translation_resolves_to_origin_fluent_file() {
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
        initialize["result"]["capabilities"]["inlayHintProvider"]["resolveProvider"],
        Value::Bool(false)
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
    let nested_uri = format!("file://{}", nested_path.display());

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": nested_uri,
                "languageId": "fluent",
                "version": 1,
                "text": nested_text
            }
        }
    }));

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
fn references_from_origin_resolve_to_translated_fluent_files() {
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
    let nested_uri = format!("file://{}", nested_path.display());

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": nested_uri,
                "languageId": "fluent",
                "version": 1,
                "text": nested_text
            }
        }
    }));

    assert_references(
        &mut lsp,
        14,
        &nested_path,
        position_of(&nested_text, "label = Save"),
        &[
            ReferenceExpectation::new("locales/es/dialogs/menu.ftl", 1, 5),
            ReferenceExpectation::new("locales/fr/dialogs/menu.ftl", 1, 5),
        ],
    );
}

#[test]
fn hover_from_translation_shows_source_and_local_comments_only() {
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
        "```ftl\n# Shown on the welcome screen\n```\n\n---\n\n```ftl\n# Texto que ve la persona usuaria al entrar\n```",
        1,
        0,
    );

    assert_hover(
        &mut lsp,
        22,
        &source_path,
        position_of(&source_text, "tooltip =\n        { $platform ->"),
        "```ftl\n# Primary install action in the downloads panel\n```\n\n---\n\n```ftl\n# Accion principal en la pantalla de descargas\n```",
        20,
        5,
    );
}

#[test]
fn hover_from_origin_file_shows_origin_comments_only() {
    let root = fixture_root();
    let source_path = root.join("locales/en/dialogs/menu.ftl");
    let source_uri = format!("file://{}", source_path.display());
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
        31,
        &source_path,
        position_of(&source_text, "label = Save"),
        "```ftl\n### Shared menu copy\n## File menu\n# Primary action\n```",
        4,
        5,
    );
}

#[test]
fn inlay_hints_show_source_previews_for_messages_and_attributes() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_path = root.join("locales/en/app.ftl");
    let origin_uri = format!("file://{}", origin_path.display());
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 35,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 35);
    assert_eq!(
        initialize["result"]["capabilities"]["inlayHintProvider"]["resolveProvider"],
        Value::Bool(false)
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
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": origin_uri,
                "languageId": "fluent",
                "version": 1,
                "text": origin_text
            }
        }
    }));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 36,
        "method": "textDocument/inlayHint",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 40, "character": 0 }
            }
        }
    }));

    let hints = lsp.recv();
    assert_eq!(hints["id"], 36);
    let items = hints["result"]
        .as_array()
        .expect("expected inlay hint array");
    assert!(items.iter().any(|item| {
        item["label"] == Value::String("src: Welcome".to_string())
            && item["position"]["line"].as_u64() == Some(1)
            && item["position"]["character"].as_u64() == Some(26)
    }));
    assert!(items.iter().any(|item| {
        item["label"] == Value::String("src: .label = Launch".to_string())
            && item["position"]["line"].as_u64() == Some(5)
            && item["position"]["character"].as_u64() == Some(19)
    }));
    assert!(items.iter().any(|item| {
        item["label"]
            == Value::String(
                "src [platform=*, tone=*]: Press Ctrl + C to copy the download link now."
                    .to_string(),
            )
            && item["position"]["line"].as_u64() == Some(8)
            && item["position"]["character"].as_u64() == Some(14)
    }));
    assert!(items.iter().any(|item| {
        item["label"]
            == Value::String(
                "src [platform=*, tone=*]: .tooltip = Install the latest desktop build now."
                    .to_string(),
            )
            && item["position"]["line"].as_u64() == Some(20)
            && item["position"]["character"].as_u64() == Some(14)
    }));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 37,
        "method": "textDocument/inlayHint",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": 8, "character": 14 },
                "end": { "line": 8, "character": 15 }
            }
        }
    }));

    let filtered_hints = lsp.recv();
    assert_eq!(filtered_hints["id"], 37);
    let filtered_items = filtered_hints["result"]
        .as_array()
        .expect("expected filtered inlay hint array");
    assert_eq!(filtered_items.len(), 1);
    assert_eq!(
        filtered_items[0]["label"],
        Value::String(
            "src [platform=*, tone=*]: Press Ctrl + C to copy the download link now.".to_string(),
        )
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 38,
        "method": "textDocument/inlayHint",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 40, "character": 0 }
            }
        }
    }));

    let origin_hints = lsp.recv();
    assert_eq!(origin_hints["id"], 38);
    let origin_items = origin_hints["result"]
        .as_array()
        .expect("expected origin inlay hint array");
    assert!(origin_items.iter().any(|item| {
        item["label"] == Value::String("src: Welcome".to_string())
            && item["position"]["line"].as_u64() == Some(1)
            && item["position"]["character"].as_u64() == Some(23)
    }));
}

#[test]
fn code_lens_opens_full_selector_combinations_document() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
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

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = lsp.recv();
    assert_eq!(lenses["id"], 41);
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
        Value::String("Show all 4 selector combinations".to_string())
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
    assert!(
        document_uri.contains("fluent-lsp-selector-combinations-")
            && document_uri.ends_with("-install-hint.md"),
        "unexpected temp document uri: {document_uri}"
    );
    let document_path = document_uri
        .strip_prefix("file://")
        .expect("expected file uri for temp document");
    let document_text = std::fs::read_to_string(document_path).expect("read temp document");
    assert_eq!(
        document_text,
        "# Selector combinations for `install-hint`\n\nLanguage: `es`\n\nLogical file: `app`\n\nSource:\n```ftl\ninstall-hint =\n    { $platform ->\n        [macos] Presiona Command\n       *[other] Presiona Ctrl\n    } + C { $tone ->\n        [calm] para copiar el enlace de descarga.\n       *[direct] para copiar el enlace de descarga ahora.\n    }\n```\n\nStatic combinations:\n- `$platform=macos`, `$tone=calm`: `Presiona Command + C para copiar el enlace de descarga.`\n- `$platform=macos`, `$tone=direct`: `Presiona Command + C para copiar el enlace de descarga ahora.`\n- `$platform=other`, `$tone=calm`: `Presiona Ctrl + C para copiar el enlace de descarga.`\n- `$platform=other`, `$tone=direct`: `Presiona Ctrl + C para copiar el enlace de descarga ahora.`"
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {
            "success": true
        }
    }));

    let response = lsp.recv();
    assert_eq!(response["id"], 42);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn code_lens_opens_full_selector_combinations_document_for_attribute() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_uri = format!("file://{}", source_path.display());
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 50,
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
    assert_eq!(initialize["id"], 50);

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

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 51,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));

    let lenses = lsp.recv();
    assert_eq!(lenses["id"], 51);
    let items = lenses["result"]
        .as_array()
        .expect("expected code lens array");
    let attribute_lens = items
        .iter()
        .find(|item| item["range"]["start"]["line"].as_u64() == Some(20))
        .expect("missing download-action.tooltip codelens");

    let command = attribute_lens["command"].clone();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 52,
        "method": "workspace/executeCommand",
        "params": {
            "command": command["command"],
            "arguments": command["arguments"]
        }
    }));

    let request = lsp.recv();
    assert_eq!(request["method"], "window/showDocument");
    let document_uri = request["params"]["uri"]
        .as_str()
        .expect("showDocument uri must be a string");
    assert!(
        document_uri.ends_with("-download-action_tooltip.md"),
        "unexpected temp document uri: {document_uri}"
    );
    let document_path = document_uri
        .strip_prefix("file://")
        .expect("expected file uri for temp document");
    let document_text = std::fs::read_to_string(document_path).expect("read temp document");
    assert_eq!(
        document_text,
        "# Selector combinations for `download-action.tooltip`\n\nLanguage: `es`\n\nLogical file: `app`\n\n```ftl\n# Accion principal en la pantalla de descargas\n```\n\nSource:\n```ftl\n.tooltip =\n    { $platform ->\n        [macos] Instala la build firmada para macOS\n       *[other] Instala la build de escritorio mas reciente\n    } { $tone ->\n        [calm] cuando te venga bien.\n       *[direct] ahora.\n    }\n```\n\nStatic combinations:\n- `$platform=macos`, `$tone=calm`: `Instala la build firmada para macOS cuando te venga bien.`\n- `$platform=macos`, `$tone=direct`: `Instala la build firmada para macOS ahora.`\n- `$platform=other`, `$tone=calm`: `Instala la build de escritorio mas reciente cuando te venga bien.`\n- `$platform=other`, `$tone=direct`: `Instala la build de escritorio mas reciente ahora.`"
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {
            "success": true
        }
    }));

    let response = lsp.recv();
    assert_eq!(response["id"], 52);
    assert_eq!(response["result"], Value::Null);
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
}

fn position_of(source: &str, needle: &str) -> (u32, u32) {
    let offset = source.find(needle).expect("needle not found in source");
    let prefix = &source[..offset];
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).unwrap();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let character = u32::try_from(source[line_start..offset].chars().count()).unwrap();
    (line, character)
}
