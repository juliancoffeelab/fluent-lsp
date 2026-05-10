use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};

use fluent_bundle::{FluentBundle, FluentResource};
use fluent_lsp::render_fluent_preview_text;
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
            stdout: BufReader::new(
                child.stdout.take().expect("missing stdout"),
            ),
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
                panic!(
                    "unexpected EOF from fluent-lsp; child status: {status:?}"
                );
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
        initialize["result"]["capabilities"]["executeCommandProvider"]["commands"]
            [0],
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

    let welcome_title_position =
        position_of_nth(&source_text, "welcome-title", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": welcome_title_position.0, "character": welcome_title_position.1 }
        }
    }));
    let welcome_title_definition = recv_response(&mut lsp, 2);
    assert_eq!(
        welcome_title_definition["result"]["uri"],
        Value::String(format!(
            "file://{}",
            root.join("locales/en/app.ftl").display()
        ))
    );
    assert_eq!(
        welcome_title_definition["result"]["range"]["start"]["line"],
        Value::from(1)
    );
    assert_eq!(
        welcome_title_definition["result"]["range"]["start"]["character"],
        Value::from(0)
    );

    let brand_name_position = position_of_nth(&source_text, "brand-name =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": brand_name_position.0, "character": brand_name_position.1 }
        }
    }));
    let brand_name_definition = recv_response(&mut lsp, 3);
    assert_eq!(
        brand_name_definition["result"]["uri"],
        Value::String(format!(
            "file://{}",
            root.join("locales/en/app.ftl").display()
        ))
    );
    assert_eq!(
        brand_name_definition["result"]["range"]["start"]["line"],
        Value::from(8)
    );
    assert_eq!(
        brand_name_definition["result"]["range"]["start"]["character"],
        Value::from(1)
    );

    let launch_label_position =
        position_of_nth(&source_text, "label = Lanzar", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": launch_label_position.0, "character": launch_label_position.1 }
        }
    }));
    let launch_label_definition = recv_response(&mut lsp, 4);
    assert_eq!(
        launch_label_definition["result"]["uri"],
        Value::String(format!(
            "file://{}",
            root.join("locales/en/app.ftl").display()
        ))
    );
    assert_eq!(
        launch_label_definition["result"]["range"]["start"]["line"],
        Value::from(13)
    );
    assert_eq!(
        launch_label_definition["result"]["range"]["start"]["character"],
        Value::from(5)
    );

    let nested_path = root.join("locales/es/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();
    open_document(&mut lsp, &nested_path, &nested_text);

    let save_label_position =
        position_of_nth(&nested_text, "label = Guardar", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_path.display()) },
            "position": { "line": save_label_position.0, "character": save_label_position.1 }
        }
    }));
    let save_label_definition = recv_response(&mut lsp, 5);
    assert_eq!(
        save_label_definition["result"]["uri"],
        Value::String(format!(
            "file://{}",
            root.join("locales/en/dialogs/menu.ftl").display()
        ))
    );
    assert_eq!(
        save_label_definition["result"]["range"]["start"]["line"],
        Value::from(4)
    );
    assert_eq!(
        save_label_definition["result"]["range"]["start"]["character"],
        Value::from(5)
    );
}

#[test]
fn initialize_returns_exact_capability_contract() {
    let root = fixture_root();
    let mut lsp = LspProcess::start();

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_200,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));

    let initialize = lsp.recv();
    assert_eq!(initialize["id"], 6_200);
    assert_eq!(
        initialize["result"]["capabilities"],
        json!({
            "definitionProvider": true,
            "referencesProvider": true,
            "hoverProvider": true,
            "completionProvider": {
                "triggerCharacters": ["."]
            },
            "codeActionProvider": {
                "codeActionKinds": ["quickfix", "refactor.rewrite"],
                "resolveProvider": false
            },
            "codeLensProvider": {
                "resolveProvider": false
            },
            "executeCommandProvider": {
                "commands": ["fluent-lsp.showSelectorCombinations"]
            },
            "textDocumentSync": {
                "openClose": true,
                "change": 1,
                "save": true
            }
        })
    );
    assert!(
        initialize["result"]["capabilities"]
            .get("renameProvider")
            .is_none()
    );
    assert!(
        initialize["result"]["capabilities"]
            .get("semanticTokensProvider")
            .is_none()
    );
    assert!(
        initialize["result"]["capabilities"]
            .get("inlayHintProvider")
            .is_none()
    );
    assert!(
        initialize["result"]["capabilities"]
            .get("documentSymbolProvider")
            .is_none()
    );
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
    std::fs::write(
        &outside_path,
        r#"hello = Outside
"#,
    )
    .unwrap();

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

    let position = position_of_nth(&source_text, "welcome-title", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_101,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let response = recv_response(&mut lsp, 6_101);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn goto_definition_returns_no_location_for_translation_key_missing_in_origin() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"welcome-title = Welcome
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"welcome-title = Bienvenido
local-only = Solo local
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_102);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "local-only", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_103,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let response = recv_response(&mut lsp, 6_103);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn goto_definition_returns_no_location_when_origin_counterpart_file_is_missing()
{
    let workspace = temp_workspace(&[(
        "locales/es/only.ftl",
        r#"orphan-title = Huerfano
"#,
    )]);
    let source_path = workspace.path().join("locales/es/only.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_104);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "orphan-title", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_105,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let response = recv_response(&mut lsp, 6_105);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn goto_definition_returns_no_location_for_translation_attribute_missing_in_origin()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"download-action =
    .label = Download
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"download-action =
    .label = Descargar
    .tooltip = Descarga esta build
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_130);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, ".tooltip =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_131,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let response = recv_response(&mut lsp, 6_131);
    assert_eq!(response["result"], Value::Null);
}

#[test]
fn goto_definition_top_level_key_position_matrix_and_comment_context_are_exact()
{
    let workspace = temp_workspace(&[
        (
            "locales/en/first.ftl",
            r#"first-key = Origin first
"#,
        ),
        (
            "locales/es/first.ftl",
            r#"first-key = Local first
"#,
        ),
        (
            "locales/en/last.ftl",
            r#"head = Origin head
last-key = Origin last
"#,
        ),
        (
            "locales/es/last.ftl",
            r#"head = Encabezado local
last-key = Ultima local
"#,
        ),
        (
            "locales/en/file-comment.ftl",
            r#"file-key = Origin file key
"#,
        ),
        (
            "locales/es/file-comment.ftl",
            r#"### Resource docs
file-key = Clave archivo
"#,
        ),
        (
            "locales/en/group-comment.ftl",
            r#"intro = Intro origin
group-key = Group origin
"#,
        ),
        (
            "locales/es/group-comment.ftl",
            r#"intro = Intro local
## Group docs
group-key = Clave grupo
"#,
        ),
        (
            "locales/en/spaced.ftl",
            r#"spaced-key = Origin spaced
"#,
        ),
        (
            "locales/es/spaced.ftl",
            r#"  spaced-key = Local spaced
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_140);

    let first_path = workspace.path().join("locales/es/first.ftl");
    let first_origin = workspace.path().join("locales/en/first.ftl");
    let first_text = std::fs::read_to_string(&first_path).unwrap();
    open_document(&mut lsp, &first_path, &first_text);
    let first_key_start = position_of(&first_text, "first-key");
    let first_positions = [
        ("first-char", first_key_start),
        ("middle-char", (first_key_start.0, first_key_start.1 + 4)),
        ("last-char", (first_key_start.0, first_key_start.1 + 8)),
    ];
    for (offset, (_label, position)) in first_positions.into_iter().enumerate()
    {
        let request_id = 6_141 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": format!("file://{}", first_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(
            response["result"],
            json!({
                "uri": format!("file://{}", first_origin.display()),
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 9 }
                }
            })
        );
    }

    let first_equals = position_of(&first_text, "=");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_144,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", first_path.display()) },
            "position": { "line": first_equals.0, "character": first_equals.1 }
        }
    }));
    let first_equals_response = recv_response(&mut lsp, 6_144);
    assert_eq!(first_equals_response["result"], Value::Null);

    let spaced_path = workspace.path().join("locales/es/spaced.ftl");
    let spaced_text = std::fs::read_to_string(&spaced_path).unwrap();
    open_document(&mut lsp, &spaced_path, &spaced_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_145,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", spaced_path.display()) },
            "position": { "line": 0, "character": 0 }
        }
    }));
    let spaced_response = recv_response(&mut lsp, 6_145);
    assert_eq!(spaced_response["result"], Value::Null);

    let file_comment_path =
        workspace.path().join("locales/es/file-comment.ftl");
    let file_comment_origin =
        workspace.path().join("locales/en/file-comment.ftl");
    let file_comment_text =
        std::fs::read_to_string(&file_comment_path).unwrap();
    open_document(&mut lsp, &file_comment_path, &file_comment_text);
    let file_comment_position = position_of(&file_comment_text, "file-key");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_146,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", file_comment_path.display()) },
            "position": {
                "line": file_comment_position.0,
                "character": file_comment_position.1
            }
        }
    }));
    let file_comment_response = recv_response(&mut lsp, 6_146);
    assert_eq!(
        file_comment_response["result"],
        json!({
            "uri": format!("file://{}", file_comment_origin.display()),
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 8 }
            }
        })
    );

    let group_comment_path =
        workspace.path().join("locales/es/group-comment.ftl");
    let group_comment_origin =
        workspace.path().join("locales/en/group-comment.ftl");
    let group_comment_text =
        std::fs::read_to_string(&group_comment_path).unwrap();
    open_document(&mut lsp, &group_comment_path, &group_comment_text);
    let group_comment_position = position_of(&group_comment_text, "group-key");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_147,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", group_comment_path.display()) },
            "position": {
                "line": group_comment_position.0,
                "character": group_comment_position.1
            }
        }
    }));
    let group_comment_response = recv_response(&mut lsp, 6_147);
    assert_eq!(
        group_comment_response["result"],
        json!({
            "uri": format!("file://{}", group_comment_origin.display()),
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 9 }
            }
        })
    );

    let last_path = workspace.path().join("locales/es/last.ftl");
    let last_origin = workspace.path().join("locales/en/last.ftl");
    let last_text = std::fs::read_to_string(&last_path).unwrap();
    open_document(&mut lsp, &last_path, &last_text);
    let last_position = position_of(&last_text, "last-key");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_148,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", last_path.display()) },
            "position": {
                "line": last_position.0,
                "character": last_position.1
            }
        }
    }));
    let last_response = recv_response(&mut lsp, 6_148);
    assert_eq!(
        last_response["result"],
        json!({
            "uri": format!("file://{}", last_origin.display()),
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 8 }
            }
        })
    );
}

#[test]
fn goto_definition_term_position_matrix_and_comment_context_are_exact() {
    let workspace = temp_workspace(&[
        (
            "locales/en/first-term.ftl",
            r#"-brand-name = Origin brand
"#,
        ),
        (
            "locales/es/first-term.ftl",
            r#"-brand-name = Marca local
"#,
        ),
        (
            "locales/en/last-term.ftl",
            r#"-alias = Alias origin
-brand-name = Origin last brand
"#,
        ),
        (
            "locales/es/last-term.ftl",
            r#"-alias = Alias local
-brand-name = Marca final
"#,
        ),
        (
            "locales/en/file-term.ftl",
            r#"-file-term = Origin file term
"#,
        ),
        (
            "locales/es/file-term.ftl",
            r#"### Resource docs
-file-term = Termino archivo
"#,
        ),
        (
            "locales/en/group-term.ftl",
            r#"-lead = Lead origin
-brand-name = Group origin
-brand-name-extra = Decoy origin
"#,
        ),
        (
            "locales/es/group-term.ftl",
            r#"-lead = Lead local
## Group docs
-brand-name = Marca grupo
"#,
        ),
        (
            "locales/en/spaced-term.ftl",
            r#"-spaced-term = Origin spaced term
"#,
        ),
        (
            "locales/es/spaced-term.ftl",
            r#"  -spaced-term = Local spaced term
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_150);

    let first_path = workspace.path().join("locales/es/first-term.ftl");
    let first_origin = workspace.path().join("locales/en/first-term.ftl");
    let first_text = std::fs::read_to_string(&first_path).unwrap();
    open_document(&mut lsp, &first_path, &first_text);
    let dash_position = position_of(&first_text, "-brand-name");
    let first_alpha = position_of(&first_text, "brand-name");
    let last_character = (first_alpha.0, first_alpha.1 + 9);
    let term_positions = [dash_position, first_alpha, last_character];
    for (offset, position) in term_positions.into_iter().enumerate() {
        let request_id = 6_151 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": format!("file://{}", first_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(
            response["result"],
            json!({
                "uri": format!("file://{}", first_origin.display()),
                "range": {
                    "start": { "line": 0, "character": 1 },
                    "end": { "line": 0, "character": 11 }
                }
            })
        );
    }

    let first_equals = position_of(&first_text, "=");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_154,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", first_path.display()) },
            "position": { "line": first_equals.0, "character": first_equals.1 }
        }
    }));
    let first_equals_response = recv_response(&mut lsp, 6_154);
    assert_eq!(first_equals_response["result"], Value::Null);

    let spaced_path = workspace.path().join("locales/es/spaced-term.ftl");
    let spaced_text = std::fs::read_to_string(&spaced_path).unwrap();
    open_document(&mut lsp, &spaced_path, &spaced_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_155,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", spaced_path.display()) },
            "position": { "line": 0, "character": 0 }
        }
    }));
    let spaced_response = recv_response(&mut lsp, 6_155);
    assert_eq!(spaced_response["result"], Value::Null);

    let file_term_path = workspace.path().join("locales/es/file-term.ftl");
    let file_term_origin = workspace.path().join("locales/en/file-term.ftl");
    let file_term_text = std::fs::read_to_string(&file_term_path).unwrap();
    open_document(&mut lsp, &file_term_path, &file_term_text);
    let file_term_position = position_of(&file_term_text, "file-term");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_156,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", file_term_path.display()) },
            "position": {
                "line": file_term_position.0,
                "character": file_term_position.1
            }
        }
    }));
    let file_term_response = recv_response(&mut lsp, 6_156);
    assert_eq!(
        file_term_response["result"],
        json!({
            "uri": format!("file://{}", file_term_origin.display()),
            "range": {
                "start": { "line": 0, "character": 1 },
                "end": { "line": 0, "character": 10 }
            }
        })
    );

    let group_term_path = workspace.path().join("locales/es/group-term.ftl");
    let group_term_origin = workspace.path().join("locales/en/group-term.ftl");
    let group_term_text = std::fs::read_to_string(&group_term_path).unwrap();
    open_document(&mut lsp, &group_term_path, &group_term_text);
    let group_term_position = position_of(&group_term_text, "brand-name");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_157,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", group_term_path.display()) },
            "position": {
                "line": group_term_position.0,
                "character": group_term_position.1
            }
        }
    }));
    let group_term_response = recv_response(&mut lsp, 6_157);
    assert_eq!(
        group_term_response["result"],
        json!({
            "uri": format!("file://{}", group_term_origin.display()),
            "range": {
                "start": { "line": 1, "character": 1 },
                "end": { "line": 1, "character": 11 }
            }
        })
    );

    let last_path = workspace.path().join("locales/es/last-term.ftl");
    let last_origin = workspace.path().join("locales/en/last-term.ftl");
    let last_text = std::fs::read_to_string(&last_path).unwrap();
    open_document(&mut lsp, &last_path, &last_text);
    let last_position = position_of(&last_text, "brand-name");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_158,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", last_path.display()) },
            "position": {
                "line": last_position.0,
                "character": last_position.1
            }
        }
    }));
    let last_response = recv_response(&mut lsp, 6_158);
    assert_eq!(
        last_response["result"],
        json!({
            "uri": format!("file://{}", last_origin.display()),
            "range": {
                "start": { "line": 1, "character": 1 },
                "end": { "line": 1, "character": 11 }
            }
        })
    );
}

#[test]
fn goto_definition_no_location_outcomes_cover_remaining_matrix_cases() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"shared-key = Origin key
-origin-term = Origin term
-shared-term = Shared origin term
button =
    .label = Origin label
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"shared-key = Clave local
-origin-term = Termino origen
-shared-term = Termino compartido
-local-term = Termino local
button =
    .label = Etiqueta local
"#,
        ),
        (
            "locales/es/dialogs/orphan.ftl",
            r#"orphan-key = Huerfano
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_160);

    let origin_path = workspace.path().join("locales/en/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    open_document(&mut lsp, &origin_path, &origin_text);

    let origin_term_position = position_of(&origin_text, "origin-term");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_161,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": {
                "line": origin_term_position.0,
                "character": origin_term_position.1
            }
        }
    }));
    let origin_term_response = recv_response(&mut lsp, 6_161);
    assert_eq!(origin_term_response["result"], Value::Null);

    let origin_attribute_position = position_of(&origin_text, ".label =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_162,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": {
                "line": origin_attribute_position.0,
                "character": origin_attribute_position.1
            }
        }
    }));
    let origin_attribute_response = recv_response(&mut lsp, 6_162);
    assert_eq!(origin_attribute_response["result"], Value::Null);

    let translation_path = workspace.path().join("locales/es/app.ftl");
    let translation_text = std::fs::read_to_string(&translation_path).unwrap();
    open_document(&mut lsp, &translation_path, &translation_text);

    let local_term_position = position_of(&translation_text, "local-term");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_163,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": {
                "line": local_term_position.0,
                "character": local_term_position.1
            }
        }
    }));
    let local_term_response = recv_response(&mut lsp, 6_163);
    assert_eq!(local_term_response["result"], Value::Null);

    let orphan_path = workspace.path().join("locales/es/dialogs/orphan.ftl");
    let orphan_text = std::fs::read_to_string(&orphan_path).unwrap();
    open_document(&mut lsp, &orphan_path, &orphan_text);
    let orphan_position = position_of(&orphan_text, "orphan-key");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_164,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", orphan_path.display()) },
            "position": {
                "line": orphan_position.0,
                "character": orphan_position.1
            }
        }
    }));
    let orphan_response = recv_response(&mut lsp, 6_164);
    assert_eq!(orphan_response["result"], Value::Null);
}

#[test]
fn completion_rejects_files_outside_the_configured_workspace() {
    let root = fixture_root();
    let mut lsp = initialized_lsp(&root, 8);
    let temp = tempdir().unwrap();
    let outside_path = temp.path().join("outside.ftl");
    std::fs::write(
        &outside_path,
        r#"hello = Outside
"#,
    )
    .unwrap();

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

    let welcome_title_position =
        position_of_nth(&source_text, "welcome-title", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": welcome_title_position.0, "character": welcome_title_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let welcome_title_references = recv_response(&mut lsp, 11);
    assert_eq!(
        welcome_title_references["result"],
        json!([
            {
                "uri": format!("file://{}", root.join("locales/es/app.ftl").display()),
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 13 }
                }
            },
            {
                "uri": format!("file://{}", root.join("locales/fr/app.ftl").display()),
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 13 }
                }
            }
        ])
    );

    let brand_name_position = position_of_nth(&source_text, "brand-name =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": brand_name_position.0, "character": brand_name_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let brand_name_references = recv_response(&mut lsp, 12);
    assert_eq!(
        brand_name_references["result"],
        json!([
            {
                "uri": format!("file://{}", root.join("locales/es/app.ftl").display()),
                "range": {
                    "start": { "line": 3, "character": 1 },
                    "end": { "line": 3, "character": 11 }
                }
            },
            {
                "uri": format!("file://{}", root.join("locales/fr/app.ftl").display()),
                "range": {
                    "start": { "line": 2, "character": 1 },
                    "end": { "line": 2, "character": 11 }
                }
            }
        ])
    );

    let launch_label_position =
        position_of_nth(&source_text, "label = Launch", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": launch_label_position.0, "character": launch_label_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let launch_label_references = recv_response(&mut lsp, 13);
    assert_eq!(
        launch_label_references["result"],
        json!([
            {
                "uri": format!("file://{}", root.join("locales/es/app.ftl").display()),
                "range": {
                    "start": { "line": 5, "character": 5 },
                    "end": { "line": 5, "character": 10 }
                }
            },
            {
                "uri": format!("file://{}", root.join("locales/fr/app.ftl").display()),
                "range": {
                    "start": { "line": 4, "character": 5 },
                    "end": { "line": 4, "character": 10 }
                }
            }
        ])
    );

    let nested_path = root.join("locales/en/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();

    open_document(&mut lsp, &nested_path, &nested_text);

    let save_label_position = position_of_nth(&nested_text, "label = Save", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 14,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_path.display()) },
            "position": { "line": save_label_position.0, "character": save_label_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let save_label_references = recv_response(&mut lsp, 14);
    assert_eq!(
        save_label_references["result"],
        json!([
            {
                "uri": format!("file://{}", root.join("locales/es/dialogs/menu.ftl").display()),
                "range": {
                    "start": { "line": 1, "character": 5 },
                    "end": { "line": 1, "character": 10 }
                }
            },
            {
                "uri": format!("file://{}", root.join("locales/fr/dialogs/menu.ftl").display()),
                "range": {
                    "start": { "line": 1, "character": 5 },
                    "end": { "line": 1, "character": 10 }
                }
            },
            {
                "uri": format!("file://{}", root.join("locales/lv/dialogs/menu.ftl").display()),
                "range": {
                    "start": { "line": 1, "character": 5 },
                    "end": { "line": 1, "character": 10 }
                }
            }
        ])
    );
}

#[test]
fn references_from_origin_return_empty_list_when_no_translation_matches() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"orphan-title = Welcome
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"welcome-title = Bienvenido
"#,
        ),
        (
            "locales/fr/app.ftl",
            r#"welcome-title = Bienvenue
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_106);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "orphan-title", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_107,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let response = recv_response(&mut lsp, 6_107);
    assert_eq!(response["result"], Value::Array(Vec::new()));
}

#[test]
fn references_from_origin_attribute_return_empty_list_when_no_translation_matches()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"download-action =
    .label = Download
    .tooltip = Download this build
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"download-action =
    .label = Descargar
"#,
        ),
    ]);
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_132);
    open_document(&mut lsp, &origin_path, &origin_text);

    let position = position_of_nth(&origin_text, ".tooltip =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_133,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": position.0, "character": position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let response = recv_response(&mut lsp, 6_133);
    assert_eq!(response["result"], Value::Array(Vec::new()));
}

#[test]
fn references_from_translation_file_return_no_result() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 6_108);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "welcome-title", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_109,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let response = recv_response(&mut lsp, 6_109);
    assert_eq!(response["result"], Value::Null);
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
    std::fs::write(
        &outside_path,
        r#"hello = Outside
"#,
    )
    .unwrap();

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
fn references_origin_top_level_key_position_matrix_and_exact_results() {
    let workspace = temp_workspace(&[
        (
            "locales/en/ref-top.ftl",
            r#"stable-key = Origin stable
"#,
        ),
        (
            "locales/es/ref-top.ftl",
            r#"### Reference docs
stable-key = Clave estable
"#,
        ),
        (
            "locales/fr/ref-top.ftl",
            r#"intro = Introduction
stable-key = Cle stable
"#,
        ),
        (
            "locales/uk/ref-top.ftl",
            r#"other-key = Insha
"#,
        ),
        (
            "locales/en/last-ref.ftl",
            r#"head = Head origin
last-key = Origin last
"#,
        ),
        (
            "locales/es/last-ref.ftl",
            r#"head = Cabeza local
last-key = Ultima local
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_170);

    let origin_path = workspace.path().join("locales/en/ref-top.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    open_document(&mut lsp, &origin_path, &origin_text);
    let key_start = position_of(&origin_text, "stable-key");
    let positions = [
        key_start,
        (key_start.0, key_start.1 + 5),
        (key_start.0, key_start.1 + 9),
    ];
    for (offset, position) in positions.into_iter().enumerate() {
        let request_id = 6_171 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": format!("file://{}", origin_path.display()) },
                "position": { "line": position.0, "character": position.1 },
                "context": { "includeDeclaration": true }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(
            response["result"],
            json!([
                {
                    "uri": format!("file://{}", origin_path.display()),
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 10 }
                    }
                },
                {
                    "uri": format!("file://{}", workspace.path().join("locales/es/ref-top.ftl").display()),
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 10 }
                    }
                },
                {
                    "uri": format!("file://{}", workspace.path().join("locales/fr/ref-top.ftl").display()),
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 10 }
                    }
                }
            ])
        );
    }

    let last_origin_path = workspace.path().join("locales/en/last-ref.ftl");
    let last_origin_text = std::fs::read_to_string(&last_origin_path).unwrap();
    open_document(&mut lsp, &last_origin_path, &last_origin_text);
    let last_position = position_of(&last_origin_text, "last-key");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_174,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", last_origin_path.display()) },
            "position": { "line": last_position.0, "character": last_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let last_response = recv_response(&mut lsp, 6_174);
    assert_eq!(
        last_response["result"],
        json!([{
            "uri": format!("file://{}", workspace.path().join("locales/es/last-ref.ftl").display()),
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 8 }
            }
        }])
    );
}

#[test]
fn references_origin_term_position_matrix_and_exact_results() {
    let workspace = temp_workspace(&[
        (
            "locales/en/ref-term.ftl",
            r#"-brand-name = Origin brand
"#,
        ),
        (
            "locales/es/ref-term.ftl",
            r#"### Term docs
-brand-name = Marca estable
"#,
        ),
        (
            "locales/fr/ref-term.ftl",
            r#"## Group docs
-brand-name = Marque stable
"#,
        ),
        (
            "locales/uk/ref-term.ftl",
            r#"-brand = Prefijo solamente
"#,
        ),
        (
            "locales/en/last-term-ref.ftl",
            r#"-alias = Alias origin
-brand-name = Origin last brand
"#,
        ),
        (
            "locales/es/last-term-ref.ftl",
            r#"-alias = Alias local
-brand-name = Marca final
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_180);

    let origin_path = workspace.path().join("locales/en/ref-term.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    open_document(&mut lsp, &origin_path, &origin_text);
    let dash_position = position_of(&origin_text, "-brand-name");
    let first_alpha = position_of(&origin_text, "brand-name");
    let last_character = (first_alpha.0, first_alpha.1 + 9);
    let positions = [dash_position, first_alpha, last_character];
    for (offset, position) in positions.into_iter().enumerate() {
        let request_id = 6_181 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": format!("file://{}", origin_path.display()) },
                "position": { "line": position.0, "character": position.1 },
                "context": { "includeDeclaration": false }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(
            response["result"],
            json!([
                {
                    "uri": format!("file://{}", workspace.path().join("locales/es/ref-term.ftl").display()),
                    "range": {
                        "start": { "line": 1, "character": 1 },
                        "end": { "line": 1, "character": 11 }
                    }
                },
                {
                    "uri": format!("file://{}", workspace.path().join("locales/fr/ref-term.ftl").display()),
                    "range": {
                        "start": { "line": 1, "character": 1 },
                        "end": { "line": 1, "character": 11 }
                    }
                }
            ])
        );
    }

    let last_origin_path =
        workspace.path().join("locales/en/last-term-ref.ftl");
    let last_origin_text = std::fs::read_to_string(&last_origin_path).unwrap();
    open_document(&mut lsp, &last_origin_path, &last_origin_text);
    let last_position = position_of(&last_origin_text, "brand-name");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_184,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", last_origin_path.display()) },
            "position": { "line": last_position.0, "character": last_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let last_response = recv_response(&mut lsp, 6_184);
    assert_eq!(
        last_response["result"],
        json!([{
            "uri": format!("file://{}", workspace.path().join("locales/es/last-term-ref.ftl").display()),
            "range": {
                "start": { "line": 1, "character": 1 },
                "end": { "line": 1, "character": 11 }
            }
        }])
    );
}

#[test]
fn references_origin_attribute_position_matrix_and_exact_results() {
    let workspace = temp_workspace(&[
        (
            "locales/en/ref-attr.ftl",
            r#"button =
    .label = Origin label
    .tooltip = Origin tooltip
"#,
        ),
        (
            "locales/es/ref-attr.ftl",
            r#"button =
    .label = Etiqueta local
    .tooltip = Ayuda local
"#,
        ),
        (
            "locales/de/ref-attr.ftl",
            r#"button =
    .label = Bezeichnung lokal
    .tooltip = Hinweis lokal
"#,
        ),
        (
            "locales/fr/ref-attr.ftl",
            r#"button =
    .label = Etiquette locale
"#,
        ),
        (
            "locales/en/dialogs/menu.ftl",
            r#"menu-save =
    .label = Save
    .tooltip = Save this file
"#,
        ),
        (
            "locales/es/dialogs/menu.ftl",
            r#"menu-save =
    .label = Guardar
    .tooltip = Guardar este archivo
"#,
        ),
        (
            "locales/de/dialogs/menu.ftl",
            r#"menu-save =
    .label = Speichern
    .tooltip = Diese Datei speichern
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_185);

    let origin_path = workspace.path().join("locales/en/ref-attr.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    open_document(&mut lsp, &origin_path, &origin_text);

    let top_level_first_dot = position_of(&origin_text, ".label =");
    let top_level_first_name = position_of(&origin_text, "label =");
    let top_level_first_last =
        (top_level_first_name.0, top_level_first_name.1 + 4);
    let top_level_first_positions = [
        top_level_first_dot,
        top_level_first_name,
        top_level_first_last,
    ];
    for (offset, position) in top_level_first_positions.into_iter().enumerate()
    {
        let request_id = 6_186 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": format!("file://{}", origin_path.display()) },
                "position": { "line": position.0, "character": position.1 },
                "context": { "includeDeclaration": false }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(
            response["result"],
            json!([
                {
                    "uri": format!("file://{}", workspace.path().join("locales/de/ref-attr.ftl").display()),
                    "range": {
                        "start": { "line": 1, "character": 5 },
                        "end": { "line": 1, "character": 10 }
                    }
                },
                {
                    "uri": format!("file://{}", workspace.path().join("locales/es/ref-attr.ftl").display()),
                    "range": {
                        "start": { "line": 1, "character": 5 },
                        "end": { "line": 1, "character": 10 }
                    }
                },
                {
                    "uri": format!("file://{}", workspace.path().join("locales/fr/ref-attr.ftl").display()),
                    "range": {
                        "start": { "line": 1, "character": 5 },
                        "end": { "line": 1, "character": 10 }
                    }
                }
            ])
        );
    }

    let top_level_last_position = position_of(&origin_text, ".tooltip =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_189,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": {
                "line": top_level_last_position.0,
                "character": top_level_last_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let top_level_last_response = recv_response(&mut lsp, 6_189);
    assert_eq!(
        top_level_last_response["result"],
        json!([
            {
                "uri": format!("file://{}", workspace.path().join("locales/de/ref-attr.ftl").display()),
                "range": {
                    "start": { "line": 2, "character": 5 },
                    "end": { "line": 2, "character": 12 }
                }
            },
            {
                "uri": format!("file://{}", workspace.path().join("locales/es/ref-attr.ftl").display()),
                "range": {
                    "start": { "line": 2, "character": 5 },
                    "end": { "line": 2, "character": 12 }
                }
            }
        ])
    );

    let nested_origin_path =
        workspace.path().join("locales/en/dialogs/menu.ftl");
    let nested_origin_text =
        std::fs::read_to_string(&nested_origin_path).unwrap();
    open_document(&mut lsp, &nested_origin_path, &nested_origin_text);

    let nested_first_position = position_of(&nested_origin_text, ".label =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_190,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_origin_path.display()) },
            "position": {
                "line": nested_first_position.0,
                "character": nested_first_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let nested_first_response = recv_response(&mut lsp, 6_190);
    assert_eq!(
        nested_first_response["result"],
        json!([
            {
                "uri": format!("file://{}", workspace.path().join("locales/de/dialogs/menu.ftl").display()),
                "range": {
                    "start": { "line": 1, "character": 5 },
                    "end": { "line": 1, "character": 10 }
                }
            },
            {
                "uri": format!("file://{}", workspace.path().join("locales/es/dialogs/menu.ftl").display()),
                "range": {
                    "start": { "line": 1, "character": 5 },
                    "end": { "line": 1, "character": 10 }
                }
            }
        ])
    );

    let nested_last_position = position_of(&nested_origin_text, ".tooltip =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_191,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_origin_path.display()) },
            "position": {
                "line": nested_last_position.0,
                "character": nested_last_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let nested_last_response = recv_response(&mut lsp, 6_191);
    assert_eq!(
        nested_last_response["result"],
        json!([
            {
                "uri": format!("file://{}", workspace.path().join("locales/de/dialogs/menu.ftl").display()),
                "range": {
                    "start": { "line": 2, "character": 5 },
                    "end": { "line": 2, "character": 12 }
                }
            },
            {
                "uri": format!("file://{}", workspace.path().join("locales/es/dialogs/menu.ftl").display()),
                "range": {
                    "start": { "line": 2, "character": 5 },
                    "end": { "line": 2, "character": 12 }
                }
            }
        ])
    );
}

#[test]
fn references_no_result_outcomes_cover_remaining_matrix_cases() {
    let workspace = temp_workspace(&[
        (
            "locales/en/orphan-term.ftl",
            r#"-orphan-term = Origin orphan
"#,
        ),
        (
            "locales/es/orphan-term.ftl",
            r#"-different-term = Termino distinto
"#,
        ),
        (
            "locales/en/translation-side.ftl",
            r#"shared-key = Origin shared
-shared-term = Origin shared term
button =
    .label = Origin label
"#,
        ),
        (
            "locales/es/translation-side.ftl",
            r#"shared-key = Clave local
-shared-term = Termino compartido
button =
    .label = Etiqueta local
"#,
        ),
        (
            "locales/en/equals.ftl",
            r#"exact-key = Exact value
"#,
        ),
        (
            "locales/en/spaced-origin.ftl",
            r#"  spaced-key = Invalid origin spacing
"#,
        ),
        (
            "locales/es/spaced-origin.ftl",
            r#"spaced-key = Local spaced key
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_190);

    let orphan_origin = workspace.path().join("locales/en/orphan-term.ftl");
    let orphan_origin_text = std::fs::read_to_string(&orphan_origin).unwrap();
    open_document(&mut lsp, &orphan_origin, &orphan_origin_text);
    let orphan_position = position_of(&orphan_origin_text, "orphan-term");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_191,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", orphan_origin.display()) },
            "position": {
                "line": orphan_position.0,
                "character": orphan_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let orphan_response = recv_response(&mut lsp, 6_191);
    assert_eq!(orphan_response["result"], Value::Array(Vec::new()));

    let translation_path =
        workspace.path().join("locales/es/translation-side.ftl");
    let translation_text = std::fs::read_to_string(&translation_path).unwrap();
    open_document(&mut lsp, &translation_path, &translation_text);

    let translation_term_position =
        position_of(&translation_text, "shared-term");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_192,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": {
                "line": translation_term_position.0,
                "character": translation_term_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let translation_term_response = recv_response(&mut lsp, 6_192);
    assert_eq!(translation_term_response["result"], Value::Null);

    let translation_attribute_position =
        position_of(&translation_text, ".label =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_193,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": {
                "line": translation_attribute_position.0,
                "character": translation_attribute_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let translation_attribute_response = recv_response(&mut lsp, 6_193);
    assert_eq!(translation_attribute_response["result"], Value::Null);

    let equals_origin = workspace.path().join("locales/en/equals.ftl");
    let equals_text = std::fs::read_to_string(&equals_origin).unwrap();
    open_document(&mut lsp, &equals_origin, &equals_text);
    let equals_position = position_of(&equals_text, "=");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_194,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", equals_origin.display()) },
            "position": {
                "line": equals_position.0,
                "character": equals_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let equals_response = recv_response(&mut lsp, 6_194);
    assert_eq!(equals_response["result"], Value::Null);

    let spaced_origin = workspace.path().join("locales/en/spaced-origin.ftl");
    let spaced_text = std::fs::read_to_string(&spaced_origin).unwrap();
    open_document(&mut lsp, &spaced_origin, &spaced_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_195,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", spaced_origin.display()) },
            "position": { "line": 0, "character": 0 },
            "context": { "includeDeclaration": false }
        }
    }));
    let spaced_response = recv_response(&mut lsp, 6_195);
    assert_eq!(spaced_response["result"], Value::Null);
}

#[test]
fn indexed_references_cover_dirty_overlay_and_nested_refresh_matrix() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"download-action = Download
-brand-name = Brand
button =
    .label = Label
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"hello = Hola
"#,
        ),
        (
            "locales/en/dialogs/menu.ftl",
            r#"menu-save =
    .label = Save
"#,
        ),
    ]);
    let mut lsp = initialized_lsp(workspace.path(), 6_200);

    let origin_path = workspace.path().join("locales/en/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    open_document(&mut lsp, &origin_path, &origin_text);
    let top_level_position = position_of(&origin_text, "download-action");
    let term_position = position_of(&origin_text, "brand-name");
    let attribute_position = position_of(&origin_text, ".label =");

    for (request_id, position) in [
        (6_201, top_level_position),
        (6_202, term_position),
        (6_203, attribute_position),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": format!("file://{}", origin_path.display()) },
                "position": { "line": position.0, "character": position.1 },
                "context": { "includeDeclaration": false }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], Value::Array(Vec::new()));
    }

    let translation_path = workspace.path().join("locales/es/app.ftl");
    let overlay_with_references = r#"download-action = Descargar
-brand-name = Marca
button =
    .label = Etiqueta
"#;
    send_open_document(&mut lsp, &translation_path, overlay_with_references);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_204,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": top_level_position.0, "character": top_level_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let top_level_overlay_response = recv_response(&mut lsp, 6_204);
    assert_eq!(
        top_level_overlay_response["result"],
        json!([{
            "uri": format!("file://{}", translation_path.display()),
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 15 }
            }
        }])
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_205,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": term_position.0, "character": term_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let term_overlay_response = recv_response(&mut lsp, 6_205);
    assert_eq!(
        term_overlay_response["result"],
        json!([{
            "uri": format!("file://{}", translation_path.display()),
            "range": {
                "start": { "line": 1, "character": 1 },
                "end": { "line": 1, "character": 11 }
            }
        }])
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_206,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": attribute_position.0, "character": attribute_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let attribute_overlay_response = recv_response(&mut lsp, 6_206);
    assert_eq!(
        attribute_overlay_response["result"],
        json!([{
            "uri": format!("file://{}", translation_path.display()),
            "range": {
                "start": { "line": 3, "character": 5 },
                "end": { "line": 3, "character": 10 }
            }
        }])
    );

    let overlay_without_references = r#"hello = Hola
"#;
    send_change_document(
        &mut lsp,
        &translation_path,
        2,
        overlay_without_references,
    );

    for (request_id, position) in [
        (6_207, top_level_position),
        (6_208, term_position),
        (6_209, attribute_position),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": format!("file://{}", origin_path.display()) },
                "position": { "line": position.0, "character": position.1 },
                "context": { "includeDeclaration": false }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], Value::Array(Vec::new()));
    }

    send_change_document(
        &mut lsp,
        &translation_path,
        3,
        overlay_with_references,
    );
    send_close_document(&mut lsp, &translation_path);
    let _ = recv_notification(&mut lsp, "textDocument/publishDiagnostics");

    for (request_id, position) in [
        (6_210, top_level_position),
        (6_211, term_position),
        (6_212, attribute_position),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": format!("file://{}", origin_path.display()) },
                "position": { "line": position.0, "character": position.1 },
                "context": { "includeDeclaration": false }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], Value::Array(Vec::new()));
    }

    let nested_origin_path =
        workspace.path().join("locales/en/dialogs/menu.ftl");
    let nested_origin_text =
        std::fs::read_to_string(&nested_origin_path).unwrap();
    open_document(&mut lsp, &nested_origin_path, &nested_origin_text);
    let nested_attribute_position =
        position_of(&nested_origin_text, ".label =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_213,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_origin_path.display()) },
            "position": {
                "line": nested_attribute_position.0,
                "character": nested_attribute_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let nested_before_refresh = recv_response(&mut lsp, 6_213);
    assert_eq!(nested_before_refresh["result"], Value::Array(Vec::new()));

    let nested_translation_path =
        workspace.path().join("locales/fr/dialogs/menu.ftl");
    std::fs::create_dir_all(nested_translation_path.parent().unwrap()).unwrap();
    std::fs::write(
        &nested_translation_path,
        r#"menu-save =
    .label = Enregistrer
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_214,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_origin_path.display()) },
            "position": {
                "line": nested_attribute_position.0,
                "character": nested_attribute_position.1
            },
            "context": { "includeDeclaration": false }
        }
    }));
    let nested_after_refresh = recv_response(&mut lsp, 6_214);
    assert_eq!(
        nested_after_refresh["result"],
        json!([{
            "uri": format!("file://{}", nested_translation_path.display()),
            "range": {
                "start": { "line": 1, "character": 5 },
                "end": { "line": 1, "character": 10 }
            }
        }])
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
    let init_notifications = recv_notifications_for_methods(
        &mut lsp,
        &["window/logMessage", "$/logTrace"],
    );
    let index_trace = init_notifications["$/logTrace"].clone();
    let index_trace_message =
        index_trace["params"]["message"].as_str().unwrap();
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
    let (_, request_trace) =
        recv_response_and_notification(&mut lsp, 154, "$/logTrace");
    let request_trace_message =
        request_trace["params"]["message"].as_str().unwrap();
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
    let init_notifications = recv_notifications_for_methods(
        &mut lsp,
        &["window/logMessage", "$/logTrace"],
    );
    let index_trace = init_notifications["$/logTrace"].clone();
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
    let (_, request_trace) =
        recv_response_and_notification(&mut lsp, 156, "$/logTrace");
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
    let (_, request_trace) =
        recv_response_and_notification(&mut lsp, 158, "$/logTrace");
    let request_trace_message =
        request_trace["params"]["message"].as_str().unwrap();
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
fn verbose_trace_reports_remaining_request_operations_exactly() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"local-only-help = Source help

install-hint =
    Copy the download link for { $gender ->
        [female] her
       *[other] their
    } account on { $count } { $count ->
        [one] device
       *[other] devices
    } now.

coins-line = You have { $coins } coins.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"install-hint =
    Copia el enlace de descarga para la cuenta de { $gender ->
        [female] ella
       *[other] elle
    } en { $count } { $count ->
        [one] dispositivo
       *[other] dispositivos
    } ahora.

coins-line = Tienes { $coins } monedas.
"#,
        ),
    ]);
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 159,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", workspace.path().display()),
            "trace": "verbose",
            "capabilities": {
                "window": {
                    "showDocument": {
                        "support": true
                    }
                }
            }
        }
    }));
    assert_eq!(lsp.recv()["id"], 159);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    let init_notifications = recv_notifications_for_methods(
        &mut lsp,
        &["window/logMessage", "$/logTrace"],
    );
    assert!(
        init_notifications["$/logTrace"]["params"]["verbose"]
            .as_str()
            .is_some_and(|verbose| verbose.starts_with("files="))
    );

    open_document(&mut lsp, &source_path, &source_text);

    let references_position = position_of(&origin_text, "install-hint");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 160,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": {
                "line": references_position.0,
                "character": references_position.1
            },
            "context": {
                "includeDeclaration": false
            }
        }
    }));
    let (references, references_trace) =
        recv_response_and_notification(&mut lsp, 160, "$/logTrace");
    let reference_count = references["result"].as_array().unwrap().len();
    assert_eq!(
        references_trace["params"]["verbose"],
        Value::String(format!("count={reference_count}"))
    );
    assert!(references_trace["params"]["message"].as_str().is_some_and(
        |message| message.starts_with(
            "fluent-lsp timing operation=textDocument/references elapsed_ms="
        )
    ));

    let hover_position = position_of(&source_text, "Copia el enlace");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 161,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": {
                "line": hover_position.0,
                "character": hover_position.1
            }
        }
    }));
    let (_, hover_trace) =
        recv_response_and_notification(&mut lsp, 161, "$/logTrace");
    assert_eq!(
        hover_trace["params"]["verbose"],
        Value::String("hit=true".to_string())
    );
    assert!(
        hover_trace["params"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with(
                "fluent-lsp timing operation=textDocument/hover elapsed_ms="
            ))
    );

    let action_position = position_of(&source_text, "$coins");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 162,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": {
                    "line": action_position.0,
                    "character": action_position.1
                },
                "end": {
                    "line": action_position.0,
                    "character": action_position.1
                }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));
    let (actions, actions_trace) =
        recv_response_and_notification(&mut lsp, 162, "$/logTrace");
    let action_count = actions["result"].as_array().unwrap().len();
    assert_eq!(
        actions_trace["params"]["verbose"],
        Value::String(format!("count={action_count}"))
    );
    assert!(actions_trace["params"]["message"].as_str().is_some_and(
        |message| message.starts_with(
            "fluent-lsp timing operation=textDocument/codeAction elapsed_ms="
        )
    ));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 163,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) }
        }
    }));
    let (lenses, lenses_trace) =
        recv_response_and_notification(&mut lsp, 163, "$/logTrace");
    let lens_count = lenses["result"].as_array().unwrap().len();
    assert_eq!(
        lenses_trace["params"]["verbose"],
        Value::String(format!("count={lens_count}"))
    );
    assert!(lenses_trace["params"]["message"].as_str().is_some_and(
        |message| message.starts_with(
            "fluent-lsp timing operation=textDocument/codeLens elapsed_ms="
        )
    ));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 164,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": [
                format!("file://{}", source_path.display()),
                "install-hint"
            ]
        }
    }));
    let request = lsp.recv();
    assert_eq!(request["method"], "window/showDocument");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": {
            "success": true
        }
    }));
    let mut command_trace = None;
    let mut command_response = None;
    while command_trace.is_none() || command_response.is_none() {
        let message = lsp.recv();
        if command_trace.is_none()
            && message["method"] == Value::String("$/logTrace".to_string())
        {
            command_trace = Some(message);
            continue;
        }
        if command_response.is_none() && message["id"] == Value::from(164) {
            command_response = Some(message);
        }
    }
    let command_trace = command_trace.expect("missing execute-command trace");
    let command_response =
        command_response.expect("missing execute-command response");
    assert_eq!(command_response["result"], Value::Null);
    assert_eq!(
        command_trace["params"]["verbose"],
        Value::String(
            "command=fluent-lsp.showSelectorCombinations ok=true".to_string()
        )
    );
    assert!(command_trace["params"]["message"].as_str().is_some_and(
        |message| message.starts_with(
            "fluent-lsp timing operation=workspace/executeCommand elapsed_ms="
        )
    ));

    let completion_text = format!("{source_text}\nlo");
    send_change_document(&mut lsp, &source_path, 2, &completion_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 165,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": 10, "character": 2 }
        }
    }));
    let (completion, completion_trace) =
        recv_response_and_notification(&mut lsp, 165, "$/logTrace");
    let completion_count = completion["result"].as_array().unwrap().len();
    assert_eq!(
        completion_trace["params"]["verbose"],
        Value::String(format!("count={completion_count}"))
    );
    assert!(completion_trace["params"]["message"].as_str().is_some_and(
        |message| message.starts_with(
            "fluent-lsp timing operation=textDocument/completion elapsed_ms="
        )
    ));
}

#[test]
fn indexed_requests_reflect_live_origin_changes_without_restart() {
    let workspace = completion_workspace();
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let translation_text = r#"hello = Hola

fresh-key = Fresco
"#;
    let updated_origin = r#"hello = Hello

# Fresh origin docs
fresh-key = Fresh origin value
"#;

    let mut lsp = initialized_lsp(workspace.path(), 141);
    send_open_document(&mut lsp, &origin_path, updated_origin);
    send_open_document(&mut lsp, &translation_path, translation_text);

    let fresh_key_position = position_of_nth(translation_text, "fresh-key", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 142,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": fresh_key_position.0, "character": fresh_key_position.1 }
        }
    }));
    let fresh_key_definition = recv_response(&mut lsp, 142);
    assert_eq!(
        fresh_key_definition["result"]["uri"],
        Value::String(format!("file://{}", origin_path.display()))
    );
    assert_eq!(
        fresh_key_definition["result"]["range"]["start"]["line"],
        Value::from(3)
    );
    assert_eq!(
        fresh_key_definition["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 144,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": fresh_key_position.0, "character": fresh_key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 144);
    assert_eq!(
        extract_ftl_blocks(
            hover["result"]["contents"]["value"].as_str().unwrap()
        ),
        vec!["# Fresh origin docs".to_string()]
    );

    let completion_text = "fresh";
    send_change_document(&mut lsp, &translation_path, 2, completion_text);
    let completion_position = position_after(completion_text, "fresh");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 143,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": completion_position.0, "character": completion_position.1 }
        }
    }));
    let completion = recv_response(&mut lsp, 143);
    let labels = if completion["result"].is_array() {
        completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(labels, vec!["fresh-key".to_string()]);
}

#[test]
fn background_refresh_updates_indexed_origin_content_for_definition_hover_and_completion()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"hello = Hello

# Original hover docs
download-action = Download
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"hello = Hola
download-action = Descargar
"#,
        ),
    ]);
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let translation_text = std::fs::read_to_string(&translation_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 148_100);
    open_document(&mut lsp, &translation_path, &translation_text);

    let key_position = position_of_nth(&translation_text, "download-action", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_101,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let initial_definition = recv_response(&mut lsp, 148_101);
    assert_eq!(
        initial_definition["result"],
        json!({
            "uri": format!("file://{}", origin_path.display()),
            "range": {
                "start": { "line": 3, "character": 0 },
                "end": { "line": 3, "character": 15 }
            }
        })
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_102,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let initial_hover = recv_response(&mut lsp, 148_102);
    assert_eq!(
        initial_hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
# Original hover docs
```"#
                .to_string()
        )
    );

    let completion_text = "new";
    send_change_document(&mut lsp, &translation_path, 2, completion_text);
    let completion_position = position_after(completion_text, "new");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_103,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": {
                "line": completion_position.0,
                "character": completion_position.1
            }
        }
    }));
    let initial_completion = recv_response(&mut lsp, 148_103);
    assert_eq!(initial_completion["result"], Value::Array(Vec::new()));
    send_change_document(&mut lsp, &translation_path, 3, &translation_text);

    std::fs::write(
        &origin_path,
        r#"hello = Hello
new-download = New completion candidate

# Updated hover docs
download-action = Download updated
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_104,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let refreshed_definition = recv_response(&mut lsp, 148_104);
    assert_eq!(
        refreshed_definition["result"],
        json!({
            "uri": format!("file://{}", origin_path.display()),
            "range": {
                "start": { "line": 4, "character": 0 },
                "end": { "line": 4, "character": 15 }
            }
        })
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_105,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let refreshed_hover = recv_response(&mut lsp, 148_105);
    assert_eq!(
        refreshed_hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
# Updated hover docs
```"#
                .to_string()
        )
    );

    send_change_document(&mut lsp, &translation_path, 4, completion_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_106,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": {
                "line": completion_position.0,
                "character": completion_position.1
            }
        }
    }));
    let refreshed_completion = recv_response(&mut lsp, 148_106);
    assert_eq!(
        refreshed_completion["result"],
        json!([{
            "label": "new-download",
            "kind": 1,
            "detail": "message",
            "documentation": {
                "kind": "markdown",
                "value": "```ftl\nnew-download = New completion candidate\n\n```"
            },
            "insertText": "new-download",
            "sortText": "0000"
        }])
    );
}

#[test]
fn background_refresh_updates_indexed_counterpart_appearance_disappearance_and_diagnostics()
 {
    let workspace = temp_workspace(&[(
        "locales/es/only.ftl",
        r#"local-only = Solo local
"#,
    )]);
    let translation_path = workspace.path().join("locales/es/only.ftl");
    let origin_path = workspace.path().join("locales/en/only.ftl");
    let translation_text = std::fs::read_to_string(&translation_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 148_200);
    send_open_document(&mut lsp, &translation_path, &translation_text);
    send_save_document(&mut lsp, &translation_path, Some(&translation_text));

    let initial_warning =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        initial_warning["params"],
        json!({
            "uri": format!("file://{}", translation_path.display()),
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 0 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Translation file has no origin-language counterpart for `only`"
            }]
        })
    );

    let key_position = position_of_nth(&translation_text, "local-only", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_201,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let missing_definition = recv_response(&mut lsp, 148_201);
    assert_eq!(missing_definition["result"], Value::Null);

    std::fs::create_dir_all(origin_path.parent().unwrap()).unwrap();
    std::fs::write(
        &origin_path,
        r#"local-only = Origin now exists
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    loop {
        let notification =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if notification["params"]["uri"]
            == Value::String(format!("file://{}", translation_path.display()))
        {
            assert_eq!(
                notification["params"]["diagnostics"],
                Value::Array(Vec::new())
            );
            break;
        }
    }

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_202,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let appearing_definition = recv_response(&mut lsp, 148_202);
    assert_eq!(
        appearing_definition["result"],
        json!({
            "uri": format!("file://{}", origin_path.display()),
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 10 }
            }
        })
    );

    std::fs::remove_file(&origin_path).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    loop {
        let notification =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if notification["params"]["uri"]
            == Value::String(format!("file://{}", translation_path.display()))
        {
            assert_eq!(
                notification["params"],
                json!({
                    "uri": format!("file://{}", translation_path.display()),
                    "diagnostics": [{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 0 }
                        },
                        "severity": 2,
                        "source": "fluent-lsp",
                        "message": "Translation file has no origin-language counterpart for `only`"
                    }]
                })
            );
            break;
        }
    }

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148_203,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let disappearing_definition = recv_response(&mut lsp, 148_203);
    assert_eq!(disappearing_definition["result"], Value::Null);
}

#[test]
fn indexed_requests_reflect_live_translation_changes_and_dirty_close_reverts() {
    let workspace = completion_workspace();
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let disk_translation = r#"hello-world = Hola
"#;
    std::fs::write(&translation_path, disk_translation).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 145);
    open_document(&mut lsp, &origin_path, &origin_text);

    let download_action_position =
        position_of_nth(&origin_text, "download-action", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 146,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": download_action_position.0, "character": download_action_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let references = recv_response(&mut lsp, 146);
    assert_eq!(references["result"], Value::Array(Vec::new()));

    send_open_document(&mut lsp, &translation_path, disk_translation);
    let dirty_translation = r#"hello = Hola

download-action = Descargar
"#;
    send_change_document(&mut lsp, &translation_path, 2, dirty_translation);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 147,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": download_action_position.0, "character": download_action_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let references = recv_response(&mut lsp, 147);
    assert_eq!(
        references["result"],
        json!([{
            "uri": format!("file://{}", translation_path.display()),
            "range": {
                "start": { "line": 2, "character": 0 },
                "end": { "line": 2, "character": 15 }
            }
        }])
    );

    send_close_document(&mut lsp, &translation_path);
    let _ = recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 148,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": download_action_position.0, "character": download_action_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let references = recv_response(&mut lsp, 148);
    assert_eq!(references["result"], Value::Array(Vec::new()));
}

#[test]
fn indexed_references_pick_up_disk_file_adds_and_deletes() {
    let workspace = completion_workspace();
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let new_translation = workspace.path().join("locales/fr/app.ftl");
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 149);
    open_document(&mut lsp, &origin_path, &origin_text);
    let hello_world_position = position_of_nth(&origin_text, "hello-world", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 150,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": hello_world_position.0, "character": hello_world_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let references = recv_response(&mut lsp, 150);
    assert_eq!(references["result"], Value::Array(Vec::new()));

    std::fs::create_dir_all(new_translation.parent().unwrap()).unwrap();
    std::fs::write(
        &new_translation,
        r#"hello-world = Bonjour
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 151,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": hello_world_position.0, "character": hello_world_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let references = recv_response(&mut lsp, 151);
    assert_eq!(
        references["result"],
        json!([{
            "uri": format!("file://{}", new_translation.display()),
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 11 }
            }
        }])
    );

    std::fs::remove_file(&new_translation).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 152,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": { "line": hello_world_position.0, "character": hello_world_position.1 },
            "context": { "includeDeclaration": false }
        }
    }));
    let references = recv_response(&mut lsp, 152);
    assert_eq!(references["result"], Value::Array(Vec::new()));
}

#[test]
fn local_only_file_warning_updates_when_origin_counterpart_appears() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es")).unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    let mut lsp = initialized_lsp(workspace.path(), 152);
    let local_path = workspace.path().join("locales/es/only.ftl");
    let local_text = r#"local-only = Solo local
"#;
    std::fs::write(&local_path, local_text).unwrap();
    send_open_document(&mut lsp, &local_path, local_text);
    send_save_document(&mut lsp, &local_path, Some(local_text));
    let warning =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
    assert_eq!(
        counterpart_warning["range"]["start"]["line"],
        Value::from(0)
    );
    assert_eq!(
        counterpart_warning["range"]["start"]["character"],
        Value::from(0)
    );

    std::fs::create_dir_all(workspace.path().join("locales/en")).unwrap();
    std::fs::write(
        workspace.path().join("locales/en/only.ftl"),
        r#"local-only = Origin now exists
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let warning_text = Value::String(
        "Translation file has no origin-language counterpart for `only`"
            .to_string(),
    );
    loop {
        let cleared =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if cleared["params"]["uri"]
            != Value::String(format!("file://{}", local_path.display()))
        {
            continue;
        }
        let diagnostics = cleared["params"]["diagnostics"].as_array().unwrap();
        if diagnostics
            .iter()
            .all(|diagnostic| diagnostic["message"] != warning_text)
        {
            assert_eq!(
                cleared["params"]["diagnostics"],
                Value::Array(Vec::new())
            );
            break;
        }
    }
}

#[test]
fn nested_missing_origin_warning_clears_and_does_not_reappear_after_reopen() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es/dialogs"))
        .unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();

    let local_path = workspace.path().join("locales/es/dialogs/menu.ftl");
    let local_text = r#"menu-save = Guardar
"#;
    std::fs::write(&local_path, local_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 153);
    send_open_document(&mut lsp, &local_path, local_text);
    send_save_document(&mut lsp, &local_path, Some(local_text));

    let initial_warning =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        initial_warning["params"],
        json!({
            "uri": format!("file://{}", local_path.display()),
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 0 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Translation file has no origin-language counterpart for `dialogs/menu`"
            }]
        })
    );

    let origin_path = workspace.path().join("locales/en/dialogs/menu.ftl");
    std::fs::create_dir_all(origin_path.parent().unwrap()).unwrap();
    std::fs::write(
        &origin_path,
        r#"menu-save = Save
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    loop {
        let cleared =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if cleared["params"]["uri"]
            == Value::String(format!("file://{}", local_path.display()))
        {
            assert_eq!(
                cleared["params"],
                json!({
                    "uri": format!("file://{}", local_path.display()),
                    "diagnostics": []
                })
            );
            break;
        }
    }

    send_close_document(&mut lsp, &local_path);
    let close_notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        close_notification["params"],
        json!({
            "uri": format!("file://{}", local_path.display()),
            "diagnostics": []
        })
    );

    send_open_document(&mut lsp, &local_path, local_text);
    send_save_document(&mut lsp, &local_path, Some(local_text));
    let reopened_notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        reopened_notification["params"],
        json!({
            "uri": format!("file://{}", local_path.display()),
            "diagnostics": []
        })
    );
}

#[test]
fn missing_origin_warning_coexists_with_parse_errors_and_clears_independently()
{
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es")).unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();

    let local_path = workspace.path().join("locales/es/only.ftl");
    let invalid_text = r#"g@Rb@ge = broken
"#;
    std::fs::write(&local_path, invalid_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 154);
    send_open_document(&mut lsp, &local_path, invalid_text);
    send_save_document(&mut lsp, &local_path, Some(invalid_text));

    let initial_notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        initial_notification["params"],
        json!({
            "uri": format!("file://{}", local_path.display()),
            "diagnostics": [
                {
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 0 }
                    },
                    "severity": 2,
                    "source": "fluent-lsp",
                    "message": "Translation file has no origin-language counterpart for `only`"
                },
                {
                    "range": {
                        "start": { "line": 0, "character": 1 },
                        "end": { "line": 0, "character": 2 }
                    },
                    "severity": 1,
                    "source": "fluent-lsp",
                    "message": "Fluent syntax error: Expected a token starting with \"=\""
                }
            ]
        })
    );

    let origin_path = workspace.path().join("locales/en/only.ftl");
    std::fs::create_dir_all(origin_path.parent().unwrap()).unwrap();
    std::fs::write(
        &origin_path,
        r#"garbage = fixed
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));

    loop {
        let refreshed =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if refreshed["params"]["uri"]
            == Value::String(format!("file://{}", local_path.display()))
        {
            assert_eq!(
                refreshed["params"],
                json!({
                    "uri": format!("file://{}", local_path.display()),
                    "diagnostics": [{
                        "range": {
                            "start": { "line": 0, "character": 1 },
                            "end": { "line": 0, "character": 2 }
                        },
                        "severity": 1,
                        "source": "fluent-lsp",
                        "message": "Fluent syntax error: Expected a token starting with \"=\""
                    }]
                })
            );
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();

    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let origin_text = r#"shared = Hello
menu =
    .label = Save
"#;
    let translation_text = r#"shared = Hola
extra = Solo local
menu =
    .label = Guardar
    .tooltip = Solo aqui
"#;
    std::fs::write(&origin_path, origin_text).unwrap();
    std::fs::write(&translation_path, translation_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 153);
    send_open_document(&mut lsp, &translation_path, translation_text);
    send_save_document(&mut lsp, &translation_path, Some(translation_text));

    let warning =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = warning["params"]["diagnostics"].as_array().unwrap();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == Value::String(
                "Translation entry `extra` has no origin-language counterpart"
                    .to_string(),
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
        r#"shared = Hello
extra = Origin now exists
menu =
    .label = Save
    .tooltip = Origin tooltip
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let extra_text = Value::String(
        "Translation entry `extra` has no origin-language counterpart"
            .to_string(),
    );
    let tooltip_text = Value::String(
        "Translation attribute `menu.tooltip` has no origin-language counterpart".to_string(),
    );
    loop {
        let cleared =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if cleared["params"]["uri"]
            != Value::String(format!("file://{}", translation_path.display()))
        {
            continue;
        }
        let diagnostics = cleared["params"]["diagnostics"].as_array().unwrap();
        if diagnostics.iter().all(|diagnostic| {
            diagnostic["message"] != extra_text
                && diagnostic["message"] != tooltip_text
        }) {
            break;
        }
    }
}

#[test]
fn translation_only_warnings_clear_independently_on_background_refresh() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es")).unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();

    let origin_path = workspace.path().join("locales/en/app.ftl");
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let origin_text = r#"shared = Hello
menu =
    .label = Save
"#;
    let translation_text = r#"shared = Hola
extra = Solo local
menu =
    .label = Guardar
    .tooltip = Solo aqui
"#;
    std::fs::write(&origin_path, origin_text).unwrap();
    std::fs::write(&translation_path, translation_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 153_100);
    send_open_document(&mut lsp, &translation_path, translation_text);
    send_save_document(&mut lsp, &translation_path, Some(translation_text));
    let _ = recv_notification(&mut lsp, "textDocument/publishDiagnostics");

    std::fs::write(
        &origin_path,
        r#"shared = Hello
extra = Origin now exists
menu =
    .label = Save
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let tooltip_position = position_of(translation_text, "tooltip");
    loop {
        let notification =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if notification["params"]["uri"]
            != Value::String(format!("file://{}", translation_path.display()))
        {
            continue;
        }
        if notification["params"]["diagnostics"]
            == json!([
                {
                    "range": {
                        "start": {
                            "line": tooltip_position.0,
                            "character": tooltip_position.1
                        },
                        "end": {
                            "line": tooltip_position.0,
                            "character": tooltip_position.1 + 7
                        }
                    },
                    "severity": 2,
                    "source": "fluent-lsp",
                    "message": "Translation attribute `menu.tooltip` has no origin-language counterpart"
                }
            ])
        {
            break;
        }
    }

    std::fs::write(
        &origin_path,
        r#"shared = Hello
extra = Origin now exists
menu =
    .label = Save
    .tooltip = Origin tooltip
"#,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    loop {
        let notification =
            recv_notification(&mut lsp, "textDocument/publishDiagnostics");
        if notification["params"]["uri"]
            != Value::String(format!("file://{}", translation_path.display()))
        {
            continue;
        }
        if notification["params"]["diagnostics"] == Value::Array(Vec::new()) {
            break;
        }
    }
}

#[test]
fn translation_only_warnings_are_absent_for_matching_translation_files() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"shared = Hello
menu =
    .label = Save
    .tooltip = Origin tooltip
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"shared = Hola
menu =
    .label = Guardar
    .tooltip = Tooltip local
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_206);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn completion_from_translation_uses_origin_language_keys_and_attributes() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let menu_path = workspace.path().join("locales/es/dialogs/menu.ftl");

    let mut lsp = initialized_lsp(workspace.path(), 14);

    let top_level_text = r#"welcome-title = Bienvenido

down"#;
    send_open_document(&mut lsp, &app_path, top_level_text);
    let top_level_position = position_after(top_level_text, "down");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 15,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": top_level_position.0, "character": top_level_position.1 }
        }
    }));
    let top_level_completion = recv_response(&mut lsp, 15);
    let labels = if top_level_completion["result"].is_array() {
        top_level_completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        top_level_completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec!["download-action".to_string(), "download-count".to_string()]
    );

    let attribute_text = r#"menu-save =
    .l
"#;
    send_open_document(&mut lsp, &menu_path, attribute_text);
    let attribute_position = position_after(attribute_text, ".l");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 16,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", menu_path.display()) },
            "position": { "line": attribute_position.0, "character": attribute_position.1 }
        }
    }));
    let attribute_completion = recv_response(&mut lsp, 16);
    let attribute_labels = if attribute_completion["result"].is_array() {
        attribute_completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        attribute_completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(attribute_labels, vec![".label".to_string()]);

    let bare_dot_text = r#"menu-save =
    .
"#;
    send_open_document(&mut lsp, &menu_path, bare_dot_text);
    let bare_dot_position = position_after(bare_dot_text, ".");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 17,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", menu_path.display()) },
            "position": { "line": bare_dot_position.0, "character": bare_dot_position.1 }
        }
    }));
    let bare_dot_completion = recv_response(&mut lsp, 17);
    let all_attribute_labels = if bare_dot_completion["result"].is_array() {
        bare_dot_completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        bare_dot_completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(
        all_attribute_labels,
        vec![".label".to_string(), ".tooltip".to_string()]
    );
}

#[test]
fn completion_omits_already_present_top_level_keys() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let source = r#"download-action = Descargar

down"#;

    let mut lsp = initialized_lsp(workspace.path(), 17_100);
    send_open_document(&mut lsp, &app_path, source);

    let position = position_after(source, "down");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 17_101,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let completion = recv_response(&mut lsp, 17_101);
    let labels = if completion["result"].is_array() {
        completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(labels, vec!["download-count".to_string()]);
}

#[test]
fn completion_omits_already_present_attributes() {
    let workspace = completion_workspace();
    let menu_path = workspace.path().join("locales/es/dialogs/menu.ftl");
    let source = r#"menu-save =
    .label = Guardar
    .
"#;

    let mut lsp = initialized_lsp(workspace.path(), 17_102);
    send_open_document(&mut lsp, &menu_path, source);

    let position = position_after(source, ".");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 17_103,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", menu_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let completion = recv_response(&mut lsp, 17_103);
    let labels = if completion["result"].is_array() {
        completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(labels, vec![".tooltip".to_string()]);
}

#[test]
fn completion_uses_nested_origin_counterpart_and_skips_origin_files() {
    let workspace = completion_workspace();
    let nested_translation =
        workspace.path().join("locales/es/dialogs/menu.ftl");
    let origin_app = workspace.path().join("locales/en/app.ftl");
    let nested_text = r#"menu-save =
    .t
"#;

    let mut lsp = initialized_lsp(workspace.path(), 18);

    send_open_document(&mut lsp, &nested_translation, nested_text);
    let nested_position = position_after(nested_text, ".t");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 19,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_translation.display()) },
            "position": { "line": nested_position.0, "character": nested_position.1 }
        }
    }));
    let nested_completion = recv_response(&mut lsp, 19);
    let nested_labels = if nested_completion["result"].is_array() {
        nested_completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        nested_completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(nested_labels, vec![".tooltip".to_string()]);

    let origin_position = position_after(
        r#"download-action = Download
"#,
        "download-action",
    );
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_app.display()) },
            "position": { "line": origin_position.0, "character": origin_position.1 }
        }
    }));
    let origin_completion = recv_response(&mut lsp, 20);
    let origin_labels = if origin_completion["result"].is_array() {
        origin_completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        origin_completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert!(origin_labels.is_empty());
}

#[test]
fn completion_returns_empty_results_for_nested_origin_files() {
    let workspace = completion_workspace();
    let origin_menu = workspace.path().join("locales/en/dialogs/menu.ftl");
    let source = r#"menu-save =
    .t
"#;

    let mut lsp = initialized_lsp(workspace.path(), 20_100);
    send_open_document(&mut lsp, &origin_menu, source);
    let position = position_after(source, ".t");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 20_101,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_menu.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let completion = recv_response(&mut lsp, 20_101);
    let labels = if completion["result"].is_array() {
        completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert!(labels.is_empty());
}

#[test]
fn completion_returns_empty_results_for_unmatched_prefixes() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let source = r#"welcome-title = Bienvenido

zzz"#;

    let mut lsp = initialized_lsp(workspace.path(), 21);
    send_open_document(&mut lsp, &app_path, source);

    let position = position_after(source, "zzz");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let completion = recv_response(&mut lsp, 22);
    let labels = if completion["result"].is_array() {
        completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert!(labels.is_empty());
}

#[test]
fn completion_returns_empty_results_inside_comments() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let source = r#"welcome-title = Bienvenido

# down"#;

    let mut lsp = initialized_lsp(workspace.path(), 22_102);
    send_open_document(&mut lsp, &app_path, source);

    let position = position_after(source, "down");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 22_103,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let completion = recv_response(&mut lsp, 22_103);
    assert_eq!(completion["result"], Value::Array(Vec::new()));
}

#[test]
fn completion_returns_empty_results_without_origin_counterpart_file() {
    let workspace = temp_workspace(&[(
        "locales/es/only.ftl",
        r#"fresh
"#,
    )]);
    let source_path = workspace.path().join("locales/es/only.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 22_100);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_after(&source_text, "fresh");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 22_101,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let completion = recv_response(&mut lsp, 22_101);
    let labels = if completion["result"].is_array() {
        completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert!(labels.is_empty());
}

#[test]
fn completion_items_include_origin_documentation_for_keys_and_attributes() {
    let workspace = completion_workspace();
    let app_path = workspace.path().join("locales/es/app.ftl");
    let menu_path = workspace.path().join("locales/es/dialogs/menu.ftl");

    let mut lsp = initialized_lsp(workspace.path(), 23);

    let commented_text = r#"welcome-title = Bienvenido

commented"#;
    send_open_document(&mut lsp, &app_path, commented_text);
    let key_position = position_after(commented_text, "commented");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 24,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let key_completion = recv_response(&mut lsp, 24);
    let key_items = if key_completion["result"].is_array() {
        key_completion["result"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    } else {
        key_completion["result"]["items"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    };
    let key_item = key_items
        .iter()
        .find(|item| {
            item["label"] == Value::String("commented-preview".to_string())
        })
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
        r#"```ftl
# Completion doc coverage
# Keep this note in completion hover
```

---

```ftl
commented-preview = Preview text for completion docs.
```"#
    );

    let attribute_text = r#"menu-save =
    .t
"#;
    send_open_document(&mut lsp, &menu_path, attribute_text);
    let attribute_position = position_after(attribute_text, ".t");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 25,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", menu_path.display()) },
            "position": { "line": attribute_position.0, "character": attribute_position.1 }
        }
    }));
    let attribute_completion = recv_response(&mut lsp, 25);
    let attribute_items = if attribute_completion["result"].is_array() {
        attribute_completion["result"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    } else {
        attribute_completion["result"]["items"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    };
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
        r#"```ftl
# Menu completion documentation
# Keep this entry visible in completion hover
```

---

```ftl
.tooltip = Save this file
```"#
    );
}

#[test]
fn code_action_file_wide_copy_uses_spec_title_for_whole_missing_string_example()
{
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"menu-save =
    .label = Save

sync-status = Sync ready
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"menu-save =
    .label = Guardar
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 30);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "menu-save =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 31,
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
    let actions = recv_response(&mut lsp, 31)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let titles = actions
        .iter()
        .filter(|action| {
            action["kind"] == Value::String("quickfix".to_string())
        })
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(titles, vec!["Copy missing strings in file".to_string()]);

    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Copy missing strings in file".to_string())
        })
        .expect("missing code action: Copy missing strings in file");
    assert_eq!(action["kind"], Value::String("quickfix".to_string()));

    let updated = Editor::new(&source_text)
        .apply_code_action(action, &format!("file://{}", source_path.display()))
        .source;
    {
        let source: &str = &updated;
        if let Err((_, errors)) = parser::parse(source) {
            panic!("failed to parse Fluent source with {errors:?}\n{source}");
        }
    };
    assert_eq!(
        updated,
        r#"menu-save =
    .label = Guardar

# [LSP-COPY]
sync-status = Sync ready
"#
    );
}

#[test]
fn code_action_file_wide_copy_uses_spec_title_for_whole_missing_message_with_attributes()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"download-action =
    .label = Install build
    .accesskey = S
    .tooltip = Download this build
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"hello = Hola Mundo
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 40);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "hello = Hola Mundo", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 41,
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
    let actions = recv_response(&mut lsp, 41)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let titles = actions
        .iter()
        .filter(|action| {
            action["kind"] == Value::String("quickfix".to_string())
        })
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(titles, vec!["Copy missing strings in file".to_string()]);

    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Copy missing strings in file".to_string())
        })
        .expect("missing code action: Copy missing strings in file");
    assert_eq!(action["kind"], Value::String("quickfix".to_string()));

    let updated = Editor::new(&source_text)
        .apply_code_action(action, &format!("file://{}", source_path.display()))
        .source;
    {
        let source: &str = &updated;
        if let Err((_, errors)) = parser::parse(source) {
            panic!("failed to parse Fluent source with {errors:?}\n{source}");
        }
    };
    assert_eq!(
        updated,
        r#"hello = Hola Mundo

# [LSP-COPY .label]
# [LSP-COPY .accesskey]
# [LSP-COPY .tooltip]
download-action =
    .label = Install build
    .accesskey = S
    .tooltip = Download this build
"#
    );
    assert_eq!(updated.lines().next(), Some("hello = Hola Mundo"));
}

#[test]
fn whole_file_missing_entry_actions_are_absent_when_translation_is_complete() {
    let workspace = missing_entry_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = r#"hello = Hola Mundo
menu-save =
    .label = Guardar
    .tooltip = Guarda este archivo

sync-status = Sincronizacion lista
"#;
    std::fs::write(&source_path, source_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 50);
    open_document(&mut lsp, &source_path, source_text);

    let position = position_of_nth(source_text, "hello = Hola Mundo", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 51,
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
    let actions = recv_response(&mut lsp, 51)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let titles = actions
        .iter()
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(
        !titles
            .iter()
            .any(|title| title == "Copy missing strings in file")
    );
}

#[test]
fn whole_file_missing_entry_actions_are_absent_without_origin_counterpart_file()
{
    let workspace = temp_workspace(&[(
        "locales/es/only.ftl",
        r#"hello = Hola Mundo
"#,
    )]);
    let source_path = workspace.path().join("locales/es/only.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_200);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "hello = Hola Mundo", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5_201,
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
    let actions = recv_response(&mut lsp, 5_201)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let titles = actions
        .iter()
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(
        !titles
            .iter()
            .any(|title| title == "Copy missing strings in file")
    );
}

#[test]
fn lsp_copy_marker_diagnostics_publish_on_save_and_clear_after_removal() {
    let workspace = copy_marker_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let cleaned = r#"hello = Hola Mundo

download-action =
    .label = Descargar
    .tooltip = Download this build
"#;

    let mut lsp = initialized_lsp(workspace.path(), 60);
    open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    let marker_diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic["message"]
                == Value::String(
                    "Entry still contains an `# [LSP-COPY]` marker".to_string(),
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(marker_diagnostics.len(), 2);
    {
        let source: &str = &source_text;
        if let Err((_, errors)) = parser::parse(source) {
            panic!("failed to parse Fluent source with {errors:?}\n{source}");
        }
    };
    assert_eq!(
        marker_diagnostics[0]["message"],
        Value::String(
            "Entry still contains an `# [LSP-COPY]` marker".to_string()
        )
    );
    assert_eq!(marker_diagnostics[0]["severity"], Value::from(2));
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
    assert_eq!(marker_diagnostics[1]["severity"], Value::from(2));

    send_change_document(&mut lsp, &source_path, 2, cleaned);
    send_save_document(&mut lsp, &source_path, Some(cleaned));
    let cleared =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(cleared["params"]["diagnostics"], Value::Array(Vec::new()));
    {
        let source: &str = cleaned;
        if let Err((_, errors)) = parser::parse(source) {
            panic!("failed to parse Fluent source with {errors:?}\n{source}");
        }
    };
    let resource = FluentResource::try_new(cleaned.to_string()).unwrap_or_else(
        |(_, errors)| {
            panic!("failed to build FluentResource with {errors:?}\n{cleaned}")
        },
    );
    let locale: LanguageIdentifier =
        "es".parse().expect("valid language identifier");
    let mut bundle = FluentBundle::new(vec![locale]);
    bundle.set_use_isolating(false);
    bundle.add_resource(resource).unwrap_or_else(|errors| {
        panic!("failed to add Fluent resource to bundle: {errors:?}")
    });
    let pattern = bundle
        .get_message("download-action")
        .and_then(|message| {
            message
                .attributes()
                .find(|attribute| attribute.id() == "tooltip")
                .map(|attribute| attribute.value())
        })
        .expect("missing download-action.tooltip");
    let mut errors = Vec::new();
    let rendered = bundle
        .format_pattern(pattern, None, &mut errors)
        .into_owned();
    assert!(errors.is_empty(), "runtime formatting errors: {errors:?}");
    assert_eq!(rendered, "Download this build");
}

#[test]
fn hover_on_copied_attribute_does_not_surface_lsp_copy_marker_comments() {
    let workspace = copy_marker_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text =
        std::fs::read_to_string(workspace.path().join("locales/en/app.ftl"))
            .unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 60_100);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "Download this build", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 60_101,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let key_hover = recv_response(&mut lsp, 60_101);
    let key_value = key_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_eq!(
        extract_ftl_blocks(key_value),
        vec![
            preview_message_text_with_overrides(
                &origin_text,
                "download-action.tooltip",
                &[]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "download-action.tooltip",
                &[]
            ),
        ]
    );

    let position = position_of_nth(&source_text, "Download this build", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 60_102,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let body_hover = recv_response(&mut lsp, 60_102);
    let body_value =
        body_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_eq!(
        extract_ftl_blocks(body_value),
        vec![
            preview_message_text_with_overrides(
                &origin_text,
                "download-action.tooltip",
                &[]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "download-action.tooltip",
                &[]
            ),
        ]
    );
}

#[test]
fn lsp_copy_marker_diagnostics_update_exactly_when_markers_are_removed_incrementally()
 {
    let workspace = copy_marker_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let without_whole_marker = r#"hello = Hola Mundo

# [LSP-COPY .tooltip]
download-action =
    .label = Descargar
    .tooltip = Download this build
"#;
    let without_any_markers = r#"hello = Hola Mundo

download-action =
    .label = Descargar
    .tooltip = Download this build
"#;

    let mut lsp = initialized_lsp(workspace.path(), 60_110);
    open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));
    let _ = recv_notification(&mut lsp, "textDocument/publishDiagnostics");

    send_change_document(&mut lsp, &source_path, 2, without_whole_marker);
    send_save_document(&mut lsp, &source_path, Some(without_whole_marker));
    let single_notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let tooltip_position = position_of_nth(without_whole_marker, "tooltip", 2);
    assert_eq!(
        single_notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": {
                        "line": tooltip_position.0,
                        "character": tooltip_position.1
                    },
                    "end": {
                        "line": tooltip_position.0,
                        "character": tooltip_position.1 + 7
                    }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Entry still contains an `# [LSP-COPY]` marker"
            }
        ])
    );

    send_change_document(&mut lsp, &source_path, 3, without_any_markers);
    send_save_document(&mut lsp, &source_path, Some(without_any_markers));
    let cleared =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(cleared["params"]["diagnostics"], Value::Array(Vec::new()));
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

    let position = position_of_nth(&source_text, r#"hello = { "" }"#, 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 71,
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
    let actions = recv_response(&mut lsp, 71)["result"]
        .as_array()
        .expect("expected code action array")
        .clone();
    let mut quickfix_titles = actions
        .iter()
        .filter(|action| {
            action["kind"] == Value::String("quickfix".to_string())
        })
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    quickfix_titles.sort();
    assert_eq!(
        quickfix_titles,
        vec![
            "Copy missing string `hello`".to_string(),
            "Copy missing strings in file".to_string(),
        ]
    );

    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Copy missing string `hello`".to_string())
        })
        .expect("missing code action: Copy missing string `hello`");
    let updated = Editor::new(&source_text)
        .apply_code_action(action, &format!("file://{}", source_path.display()))
        .source;

    {
        let source: &str = &updated;
        if let Err((_, errors)) = parser::parse(source) {
            panic!("failed to parse Fluent source with {errors:?}\n{source}");
        }
    };
    assert_eq!(
        updated,
        r#"# [LSP-COPY]
hello = Hello World
download-action =
    .label = Descargar

sync-status = { "" }
"#
    );
    let updated_resource = FluentResource::try_new(updated.clone())
        .unwrap_or_else(|(_, errors)| {
            panic!("failed to build FluentResource with {errors:?}\n{updated}")
        });
    let origin_resource = FluentResource::try_new(origin_text.clone())
        .unwrap_or_else(|(_, errors)| {
            panic!(
                "failed to build FluentResource with {errors:?}\n{origin_text}"
            )
        });
    let updated_locale: LanguageIdentifier =
        "es".parse().expect("valid language identifier");
    let origin_locale: LanguageIdentifier =
        "en".parse().expect("valid language identifier");
    let mut updated_bundle = FluentBundle::new(vec![updated_locale]);
    updated_bundle.set_use_isolating(false);
    updated_bundle
        .add_resource(updated_resource)
        .unwrap_or_else(|errors| {
            panic!("failed to add Fluent resource to bundle: {errors:?}")
        });
    let mut origin_bundle = FluentBundle::new(vec![origin_locale]);
    origin_bundle.set_use_isolating(false);
    origin_bundle
        .add_resource(origin_resource)
        .unwrap_or_else(|errors| {
            panic!("failed to add Fluent resource to bundle: {errors:?}")
        });
    let mut updated_errors = Vec::new();
    let updated_rendered = updated_bundle
        .format_pattern(
            updated_bundle
                .get_message("hello")
                .and_then(|message| message.value())
                .expect("missing hello"),
            None,
            &mut updated_errors,
        )
        .into_owned();
    assert!(
        updated_errors.is_empty(),
        "runtime formatting errors: {updated_errors:?}"
    );
    let mut origin_errors = Vec::new();
    let origin_rendered = origin_bundle
        .format_pattern(
            origin_bundle
                .get_message("hello")
                .and_then(|message| message.value())
                .expect("missing hello"),
            None,
            &mut origin_errors,
        )
        .into_owned();
    assert!(
        origin_errors.is_empty(),
        "runtime formatting errors: {origin_errors:?}"
    );
    assert_eq!(updated_rendered, origin_rendered);
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

    let position = position_of_nth(&source_text, "download-action =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 81,
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
    let actions = recv_response(&mut lsp, 81)["result"]
        .as_array()
        .expect("expected code action array")
        .clone();
    let mut quickfix_titles = actions
        .iter()
        .filter(|action| {
            action["kind"] == Value::String("quickfix".to_string())
        })
        .map(|action| action["title"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    quickfix_titles.sort();
    assert_eq!(
        quickfix_titles,
        vec![
            "Copy missing attribute `download-action.tooltip`".to_string(),
            "Copy missing strings in file".to_string(),
        ]
    );
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Copy missing attribute `download-action.tooltip`".to_string())
        })
        .expect("missing code action: Copy missing attribute `download-action.tooltip`");
    let updated = Editor::new(&source_text)
        .apply_code_action(action, &format!("file://{}", source_path.display()))
        .source;

    {
        let source: &str = &updated;
        if let Err((_, errors)) = parser::parse(source) {
            panic!("failed to parse Fluent source with {errors:?}\n{source}");
        }
    };
    assert_eq!(
        updated,
        r#"hello = { "" }
# [LSP-COPY .tooltip]
download-action =
    .label = Descargar
    .tooltip = Download this build

sync-status = { "" }
"#
    );
    let updated_resource = FluentResource::try_new(updated.clone())
        .unwrap_or_else(|(_, errors)| {
            panic!("failed to build FluentResource with {errors:?}\n{updated}")
        });
    let origin_resource = FluentResource::try_new(origin_text.clone())
        .unwrap_or_else(|(_, errors)| {
            panic!(
                "failed to build FluentResource with {errors:?}\n{origin_text}"
            )
        });
    let updated_locale: LanguageIdentifier =
        "es".parse().expect("valid language identifier");
    let origin_locale: LanguageIdentifier =
        "en".parse().expect("valid language identifier");
    let mut updated_bundle = FluentBundle::new(vec![updated_locale]);
    updated_bundle.set_use_isolating(false);
    updated_bundle
        .add_resource(updated_resource)
        .unwrap_or_else(|errors| {
            panic!("failed to add Fluent resource to bundle: {errors:?}")
        });
    let mut origin_bundle = FluentBundle::new(vec![origin_locale]);
    origin_bundle.set_use_isolating(false);
    origin_bundle
        .add_resource(origin_resource)
        .unwrap_or_else(|errors| {
            panic!("failed to add Fluent resource to bundle: {errors:?}")
        });
    let updated_pattern = updated_bundle
        .get_message("download-action")
        .and_then(|message| {
            message
                .attributes()
                .find(|attribute| attribute.id() == "tooltip")
                .map(|attribute| attribute.value())
        })
        .expect("missing updated download-action.tooltip");
    let origin_pattern = origin_bundle
        .get_message("download-action")
        .and_then(|message| {
            message
                .attributes()
                .find(|attribute| attribute.id() == "tooltip")
                .map(|attribute| attribute.value())
        })
        .expect("missing origin download-action.tooltip");
    let mut updated_errors = Vec::new();
    let updated_rendered = updated_bundle
        .format_pattern(updated_pattern, None, &mut updated_errors)
        .into_owned();
    assert!(
        updated_errors.is_empty(),
        "runtime formatting errors: {updated_errors:?}"
    );
    let mut origin_errors = Vec::new();
    let origin_rendered = origin_bundle
        .format_pattern(origin_pattern, None, &mut origin_errors)
        .into_owned();
    assert!(
        origin_errors.is_empty(),
        "runtime formatting errors: {origin_errors:?}"
    );
    assert_eq!(updated_rendered, origin_rendered);
}

#[test]
fn single_message_copy_actions_are_absent_for_complete_entries() {
    let workspace = single_key_copy_workspace();
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = r#"hello = Hola Mundo
download-action =
    .label = Descargar
    .tooltip = Descarga esta build

sync-status = Sincronizacion lista
"#;
    std::fs::write(&source_path, source_text).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 90);
    open_document(&mut lsp, &source_path, source_text);

    let position = position_of_nth(source_text, "hello = Hola Mundo", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 91,
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
    let hello_actions = recv_response(&mut lsp, 91)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(hello_actions.iter().all(|action| {
        action["title"]
            != Value::String("Copy missing string `hello`".to_string())
    }));

    let position = position_of_nth(source_text, "download-action =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 92,
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
    let download_actions = recv_response(&mut lsp, 92)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(download_actions.iter().all(|action| {
        action["title"]
            != Value::String(
                "Copy missing attribute `download-action.tooltip`".to_string(),
            )
    }));
}

#[test]
fn single_message_copy_action_is_absent_when_selected_key_has_no_origin_counterpart()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"shared = Hello
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"shared = Hola
local-only = { "" }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_202);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "local-only = { \"\" }", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5_203,
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
    let actions = recv_response(&mut lsp, 5_203)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(actions.iter().all(|action| {
        action["title"]
            != Value::String("Copy missing string `local-only`".to_string())
    }));
}

#[test]
fn missing_attribute_copy_action_is_absent_when_selected_message_has_no_origin_counterpart()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"shared = Hello
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"shared = Hola
orphan =
    .label = Huerfano
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 5_204);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "orphan =", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5_205,
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
    let actions = recv_response(&mut lsp, 5_205)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(actions.iter().all(|action| {
        action["title"]
            != Value::String(
                "Copy missing attribute `orphan.label`".to_string(),
            )
    }));
}

#[test]
fn origin_files_do_not_offer_translation_only_missing_entry_quick_fixes() {
    let workspace = missing_entry_workspace();
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 9_300);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "hello = Hello World", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 9_301,
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
    let actions = recv_response(&mut lsp, 9_301)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(actions.iter().all(|action| {
        action["title"]
            != Value::String("Copy missing strings in file".to_string())
            && action["title"]
                != Value::String("Copy missing string `hello`".to_string())
            && action["title"]
                != Value::String(
                    "Copy missing attribute `download-action.tooltip`"
                        .to_string(),
                )
    }));
}

#[test]
fn hover_from_translation_shows_local_formatted_messages() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text =
        std::fs::read_to_string(root.join("locales/en/app.ftl")).unwrap();

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

    let key_position = position_of_nth(&source_text, "commented-preview", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let key_hover = recv_response(&mut lsp, 21);
    assert_eq!(
        key_hover["result"]["contents"]["kind"],
        Value::String("markdown".to_string())
    );
    assert_eq!(
        key_hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
# Comment-only hover coverage
# Keep this translator guidance visible on key hover
```

---

```ftl
# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
```"#
                .to_string()
        )
    );
    assert_eq!(
        key_hover["result"]["range"]["start"]["line"],
        Value::from(key_position.0)
    );
    assert_eq!(
        key_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );

    let body_position =
        position_of_nth(&source_text, "Abre la build mas reciente de", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": body_position.0, "character": body_position.1 }
        }
    }));
    let body_hover = recv_response(&mut lsp, 22);
    let body_value = body_hover["result"]["contents"]["value"]
        .as_str()
        .expect("expected hover markdown");
    assert_eq!(
        body_value,
        format!(
            r#"```ftl
{}
```

---

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "welcome-body",
                &[]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "welcome-body",
                &[]
            ),
        )
    );
    assert_eq!(
        body_hover["result"]["range"]["start"]["line"],
        Value::from(2)
    );
    assert_eq!(
        body_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        extract_ftl_blocks(body_value),
        vec![
            preview_message_text_with_overrides(
                &origin_text,
                "welcome-body",
                &[]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "welcome-body",
                &[]
            ),
        ]
    );

    let empty_key_position = position_of_nth(&source_text, "empty-preview", 1);
    let empty_position = position_of_nth(&source_text, r#""""#, 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 221,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": empty_position.0, "character": empty_position.1 }
        }
    }));
    let empty_hover = recv_response(&mut lsp, 221);
    let empty_hover_value = empty_hover["result"]["contents"]["value"]
        .as_str()
        .expect("expected hover markdown");
    assert_eq!(
        empty_hover_value,
        format!(
            r#"```ftl
{}
```

---

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "empty-preview",
                &[]
            ),
            "<empty>",
        )
    );
    assert_eq!(
        empty_hover["result"]["range"]["start"]["line"],
        Value::from(empty_key_position.0)
    );
    assert_eq!(
        empty_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        extract_ftl_blocks(empty_hover_value),
        vec![
            "English empty preview fallback.".to_string(),
            "<empty>".to_string()
        ]
    );

    let selector_position = position_of_nth(&source_text, "[female] ella", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 23,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": selector_position.0, "character": selector_position.1 }
        }
    }));
    let selector_hover = recv_response(&mut lsp, 23);
    let selector_hover_value = selector_hover["result"]["contents"]["value"]
        .as_str()
        .expect("expected hover markdown");
    assert_eq!(
        selector_hover_value,
        format!(
            r#"`$gender=female`, `$count=*`

```ftl
{}
```

---

`$gender=female`, `$count=*`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "install-hint",
                &[("$gender", "female")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "install-hint",
                &[("$gender", "female")]
            ),
        )
    );
    assert_eq!(
        selector_hover["result"]["range"]["start"]["line"],
        Value::from(8)
    );
    assert_eq!(
        selector_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        extract_ftl_blocks(selector_hover_value),
        vec![
            preview_message_text_with_overrides(
                &origin_text,
                "install-hint",
                &[("$gender", "female")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "install-hint",
                &[("$gender", "female")]
            ),
        ]
    );

    let attribute_position = position_of_nth(
        &source_text,
        "Instala la build recomendada para la cuenta de",
        1,
    );
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 24,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": attribute_position.0, "character": attribute_position.1 }
        }
    }));
    let attribute_hover = recv_response(&mut lsp, 24);
    let attribute_hover_value = attribute_hover["result"]["contents"]["value"]
        .as_str()
        .expect("expected hover markdown");
    assert_eq!(
        attribute_hover_value,
        format!(
            r#"`$gender=*`, `$count=*`

```ftl
{}
```

---

`$gender=*`, `$count=*`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "download-action.tooltip",
                &[]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "download-action.tooltip",
                &[]
            ),
        )
    );
    assert_eq!(
        attribute_hover["result"]["range"]["start"]["line"],
        Value::from(21)
    );
    assert_eq!(
        attribute_hover["result"]["range"]["start"]["character"],
        Value::from(5)
    );
    assert_eq!(
        extract_ftl_blocks(attribute_hover_value),
        vec![
            preview_message_text_with_overrides(
                &origin_text,
                "download-action.tooltip",
                &[]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "download-action.tooltip",
                &[]
            ),
        ]
    );

    let post_selector_position =
        position_of_nth(&source_text, "en { $count } { $count ->", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 25,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": post_selector_position.0, "character": post_selector_position.1 }
        }
    }));
    let post_selector_hover = recv_response(&mut lsp, 25);
    let post_selector_hover_value =
        post_selector_hover["result"]["contents"]["value"]
            .as_str()
            .expect("expected hover markdown");
    assert_eq!(
        post_selector_hover_value,
        format!(
            r#"`$gender=other`, `$count=*`

```ftl
{}
```

---

`$gender=other`, `$count=*`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "install-hint",
                &[("$gender", "other")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "install-hint",
                &[("$gender", "other")]
            ),
        )
    );
    assert_eq!(
        post_selector_hover["result"]["range"]["start"]["line"],
        Value::from(8)
    );
    assert_eq!(
        post_selector_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        extract_ftl_blocks(post_selector_hover_value),
        vec![
            preview_message_text_with_overrides(
                &origin_text,
                "install-hint",
                &[("$gender", "other")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "install-hint",
                &[("$gender", "other")]
            ),
        ]
    );

    let second_selector_position =
        position_of_nth(&source_text, "[one] dispositivo", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 26,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": {
                "line": second_selector_position.0,
                "character": second_selector_position.1
            }
        }
    }));
    let second_selector_hover = recv_response(&mut lsp, 26);
    let second_selector_hover_value =
        second_selector_hover["result"]["contents"]["value"]
            .as_str()
            .expect("expected hover markdown");
    assert_eq!(
        second_selector_hover_value,
        format!(
            r#"`$gender=other`, `$count=one`

```ftl
{}
```

---

`$gender=other`, `$count=one`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "install-hint",
                &[("$gender", "other"), ("$count", "one")],
            ),
            preview_message_text_with_overrides(
                &source_text,
                "install-hint",
                &[("$gender", "other"), ("$count", "one")],
            ),
        )
    );
    assert_eq!(
        second_selector_hover["result"]["range"]["start"]["line"],
        Value::from(8)
    );
    assert_eq!(
        second_selector_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        extract_ftl_blocks(second_selector_hover_value),
        vec![
            preview_message_text_with_overrides(
                &origin_text,
                "install-hint",
                &[("$gender", "other"), ("$count", "one")],
            ),
            preview_message_text_with_overrides(
                &source_text,
                "install-hint",
                &[("$gender", "other"), ("$count", "one")],
            ),
        ]
    );
}

#[test]
fn hover_from_translation_matches_available_selector_variables_across_source_and_local()
 {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text =
        std::fs::read_to_string(root.join("locales/en/app.ftl")).unwrap();

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

    let female_position =
        position_of_nth(&source_text, "[female] ella misma", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 28,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": female_position.0, "character": female_position.1 }
        }
    }));
    let female_hover = recv_response(&mut lsp, 28);
    assert_eq!(
        female_hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"`$platform=*`, `$count=*`

```ftl
{}
```

---

`$gender=female`, `$count=*`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "mismatch-rollout",
                &[]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "mismatch-rollout",
                &[("$gender", "female")]
            ),
        ))
    );
    assert_eq!(
        female_hover["result"]["range"]["start"]["line"],
        Value::from(50)
    );
    assert_eq!(
        female_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );

    let zero_position = position_of_nth(&source_text, "[0] ningun paquete", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 29,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": zero_position.0, "character": zero_position.1 }
        }
    }));
    let zero_hover = recv_response(&mut lsp, 29);
    assert_eq!(
        zero_hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"`$platform=*`, `$count=0`

```ftl
{}
```

---

`$gender=other`, `$count=0`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "mismatch-rollout",
                &[("$count", "0")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "mismatch-rollout",
                &[("$count", "0")]
            ),
        ))
    );
    assert_eq!(
        zero_hover["result"]["range"]["start"]["line"],
        Value::from(50)
    );
    assert_eq!(
        zero_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );

    let one_position = position_of_nth(&source_text, "[1] un paquete", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": one_position.0, "character": one_position.1 }
        }
    }));
    let one_hover = recv_response(&mut lsp, 30);
    assert_eq!(
        one_hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"`$platform=*`, `$count=1`

```ftl
{}
```

---

`$gender=other`, `$count=1`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "mismatch-rollout",
                &[("$count", "1")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "mismatch-rollout",
                &[("$count", "1")]
            ),
        ))
    );
    assert_eq!(
        one_hover["result"]["range"]["start"]["line"],
        Value::from(50)
    );
    assert_eq!(
        one_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn hover_from_latvian_translation_preserves_zero_category_selectors() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text =
        std::fs::read_to_string(root.join("locales/en/app.ftl")).unwrap();

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

    let zero_position =
        position_of_nth(&source_text, "[zero] neviena pakotne nav gatava", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": zero_position.0, "character": zero_position.1 }
        }
    }));
    let zero_hover = recv_response(&mut lsp, 32);
    assert_eq!(
        zero_hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"`$count=zero`

```ftl
{}
```

---

`$count=zero`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "zero-rollout",
                &[("$count", "zero")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "zero-rollout",
                &[("$count", "zero")]
            ),
        ))
    );
    assert_eq!(
        zero_hover["result"]["range"]["start"]["line"],
        Value::from(0)
    );
    assert_eq!(
        zero_hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );

    let one_position =
        position_of_nth(&source_text, "[one] viena pakotne ir gatava", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 33,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": one_position.0, "character": one_position.1 }
        }
    }));
    let one_hover = recv_response(&mut lsp, 33);
    assert_eq!(
        one_hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"`$count=one`

```ftl
{}
```

---

`$count=one`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "zero-rollout",
                &[("$count", "one")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "zero-rollout",
                &[("$count", "one")]
            ),
        ))
    );
    assert_eq!(
        one_hover["result"]["range"]["start"]["line"],
        Value::from(0)
    );
    assert_eq!(
        one_hover["result"]["range"]["start"]["character"],
        Value::from(0)
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

    let key_position = position_of_nth(&source_text, "label = Save", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let key_hover = recv_response(&mut lsp, 31);
    let key_hover_value = key_hover["result"]["contents"]["value"]
        .as_str()
        .expect("expected hover markdown");
    assert_eq!(
        key_hover_value,
        r#"```ftl
### Shared menu copy
## File menu
# Primary action
```"#
    );
    assert_eq!(
        key_hover["result"]["range"]["start"]["line"],
        Value::from(4)
    );
    assert_eq!(
        key_hover["result"]["range"]["start"]["character"],
        Value::from(5)
    );
    assert_eq!(
        extract_ftl_blocks(key_hover_value),
        vec![
            r#"### Shared menu copy
## File menu
# Primary action"#
                .to_string()
        ]
    );

    let body_position = position_of_nth(
        &source_text,
        "Save changes before closing the window",
        1,
    );
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": body_position.0, "character": body_position.1 }
        }
    }));
    let body_hover = recv_response(&mut lsp, 32);
    let body_value =
        body_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_eq!(
        extract_ftl_blocks(body_value),
        vec![preview_message_text_with_overrides(
            &source_text,
            "menu-save.tooltip",
            &[],
        )]
    );
}

#[test]
fn hover_from_origin_file_shows_one_body_preview_block_for_top_level_message() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 6_124);
    open_document(&mut lsp, &source_path, &source_text);

    let position =
        position_of_nth(&source_text, "Preview text for hover comments.", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_125,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_125);
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
    std::fs::write(
        &outside_path,
        r#"hello = Outside
"#,
    )
    .unwrap();

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
            &[r#"# Comment-only hover coverage
# Keep this translator guidance visible on key hover"#][..],
        ),
        (
            "locales/es/app.ftl",
            "commented-preview",
            &[
                r#"# Comment-only hover coverage
# Keep this translator guidance visible on key hover"#,
                r#"# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave"#,
            ][..],
        ),
        (
            "locales/fr/app.ftl",
            "commented-preview",
            &[
                r#"# Comment-only hover coverage
# Keep this translator guidance visible on key hover"#,
                r#"# Couverture hover pour les commentaires
# Garder cette note visible sur le hover de cle"#,
            ][..],
        ),
        (
            "locales/lv/app.ftl",
            "commented-preview",
            &[
                r#"# Comment-only hover coverage
# Keep this translator guidance visible on key hover"#,
                r#"# Hover komentaru parklajums
# Saglabat so piezimi redzamu atslegas hover skata"#,
            ][..],
        ),
        (
            "locales/uk/app.ftl",
            "commented-preview",
            &[
                r#"# Comment-only hover coverage
# Keep this translator guidance visible on key hover"#,
                r#"# Перевірка hover-коментарів
# Тримайте цю примітку видимою у hover для ключа"#,
            ][..],
        ),
    ];
    let nested_cases = [
        (
            "locales/en/dialogs/menu.ftl",
            "commented-menu =",
            &[r#"### Shared menu copy
## File menu
# Attribute hover comment coverage
# Keep this menu note visible on attribute key hover"#][..],
        ),
        (
            "locales/es/dialogs/menu.ftl",
            "commented-menu =",
            &[
                r#"### Shared menu copy
## File menu
# Attribute hover comment coverage
# Keep this menu note visible on attribute key hover"#,
                r#"# Cobertura de comentarios para hover de atributo
# Mantener visible esta nota en el hover de la clave del atributo"#,
            ][..],
        ),
        (
            "locales/fr/dialogs/menu.ftl",
            "commented-menu =",
            &[
                r#"### Shared menu copy
## File menu
# Attribute hover comment coverage
# Keep this menu note visible on attribute key hover"#,
                r#"# Couverture de commentaire pour hover d attribut
# Garder cette note visible sur le hover de la cle d attribut"#,
            ][..],
        ),
    ];

    let mut lsp = LspProcess::start();
    initialize_lsp(&mut lsp, &root, 34);

    for (index, (relative_path, needle, expected_blocks)) in
        top_level_cases.iter().enumerate()
    {
        let path = root.join(relative_path);
        let source = std::fs::read_to_string(&path).unwrap();
        open_document(&mut lsp, &path, &source);
        let request_id = 35 + i64::try_from(index).unwrap();
        let position = position_of_nth(&source, needle, 1);
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let hover = recv_response(&mut lsp, request_id);
        let hover_value = hover["result"]["contents"]["value"]
            .as_str()
            .expect("expected hover markdown");
        assert_eq!(
            hover_value,
            expected_blocks
                .iter()
                .map(|block| format!(
                    r#"```ftl
{block}
```"#
                ))
                .collect::<Vec<_>>()
                .join(
                    r#"

---

"#,
                )
        );
        assert_eq!(
            hover["result"]["range"]["start"]["line"],
            Value::from(position.0)
        );
        assert_eq!(
            hover["result"]["range"]["start"]["character"],
            Value::from(0)
        );
        assert_eq!(
            extract_ftl_blocks(hover_value),
            expected_blocks
                .iter()
                .map(|block| (*block).to_string())
                .collect::<Vec<_>>()
        );
    }

    for (index, (relative_path, needle, expected_blocks)) in
        nested_cases.iter().enumerate()
    {
        let path = root.join(relative_path);
        let source = std::fs::read_to_string(&path).unwrap();
        open_document(&mut lsp, &path, &source);
        let request_id = 45 + i64::try_from(index).unwrap();
        let position = position_of_nth(&source, needle, 1);
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let hover = recv_response(&mut lsp, request_id);
        let hover_value = hover["result"]["contents"]["value"]
            .as_str()
            .expect("expected hover markdown");
        assert_eq!(
            hover_value,
            expected_blocks
                .iter()
                .map(|block| format!(
                    r#"```ftl
{block}
```"#
                ))
                .collect::<Vec<_>>()
                .join(
                    r#"

---

"#,
                )
        );
        assert_eq!(
            hover["result"]["range"]["start"]["line"],
            Value::from(position.0)
        );
        assert_eq!(
            hover["result"]["range"]["start"]["character"],
            Value::from(0)
        );
        assert_eq!(
            extract_ftl_blocks(hover_value),
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
            r#"# Comment-only hover coverage
# Keep this translator guidance visible on key hover
commented-preview = Preview text for hover comments.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"commented-preview = Texto de vista previa para comentarios de hover.
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_110);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "commented-preview", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_111,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_111);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
# Comment-only hover coverage
# Keep this translator guidance visible on key hover
```"#
                .to_string()
        )
    );
    assert_eq!(
        hover["result"]["range"]["start"]["line"],
        Value::from(key_position.0)
    );
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn hover_key_with_local_comments_only_shows_one_local_comment_block() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"local-note = Preview text for hover comments.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
local-note = Texto de vista previa para comentarios de hover.
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_112);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "local-note", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_113,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_113);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
```"#
                .to_string()
        )
    );
    assert_eq!(
        hover["result"]["range"]["start"]["line"],
        Value::from(key_position.0)
    );
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn hover_key_keeps_newline_between_adjacent_resource_and_group_comments() {
    let workspace = temp_workspace(&[(
        "locales/en/app.ftl",
        r#"### Shared translator guidance
## Buttons
first-button = First
"#,
    )]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_114);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "first-button", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_115,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_115);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
### Shared translator guidance
## Buttons
```"#
                .to_string()
        )
    );
}

#[test]
fn hover_key_keeps_newline_between_separated_resource_and_group_comments() {
    let workspace = temp_workspace(&[(
        "locales/en/app.ftl",
        r#"### Shared translator guidance

## Buttons
first-button = First
"#,
    )]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_116);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "first-button", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_117,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_117);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
### Shared translator guidance
## Buttons
```"#
                .to_string()
        )
    );
}

#[test]
fn hover_key_keeps_newline_between_adjacent_group_and_message_comments() {
    let workspace = temp_workspace(&[(
        "locales/en/app.ftl",
        r#"## Buttons
# First action
first-button = First
"#,
    )]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_118);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "first-button", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_119,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_119);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
## Buttons
# First action
```"#
                .to_string()
        )
    );
}

#[test]
fn hover_key_keeps_newline_between_separated_group_and_message_comments() {
    let workspace = temp_workspace(&[(
        "locales/en/app.ftl",
        r#"## Buttons

# First action
first-button = First
"#,
    )]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_120);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "first-button", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_121,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_121);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
## Buttons
# First action
```"#
                .to_string()
        )
    );
}

#[test]
fn hover_later_key_still_shows_resource_comment_scope() {
    let workspace = temp_workspace(&[(
        "locales/en/app.ftl",
        r#"### Shared translator guidance
first-button = First
second-button = Second
"#,
    )]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_122);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "second-button", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_123,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_123);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
### Shared translator guidance
```"#
                .to_string()
        )
    );
}

#[test]
fn hover_later_key_still_shows_group_comment_scope() {
    let workspace = temp_workspace(&[(
        "locales/en/app.ftl",
        r#"## Buttons
first-button = First
second-button = Second
"#,
    )]);
    let source_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_124);
    open_document(&mut lsp, &source_path, &source_text);

    let key_position = position_of_nth(&source_text, "second-button", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_125,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_125);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(
            r#"```ftl
## Buttons
```"#
                .to_string()
        )
    );
}

#[test]
fn hover_key_without_comments_returns_no_hover() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"plain-note = Preview text for hover comments.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"plain-note = Texto de vista previa para comentarios de hover.
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_126);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "plain-note", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_127,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_127);
    assert_eq!(hover["result"], Value::Null);
}

#[test]
fn hover_body_without_origin_message_shows_one_local_preview_block() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"welcome-body = Open the latest build.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"local-only = Texto solo local.
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_116);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "Texto solo local.", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_117,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_117);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &source_text,
                "local-only",
                &[]
            )
        ))
    );
    assert_eq!(hover["result"]["range"]["start"]["line"], Value::from(0));
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn hover_selector_without_origin_selectors_leaves_origin_block_headerless() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"download-state = Download ready.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"download-state =
    { $count ->
        [one] Descarga lista.
       *[other] Descargas listas.
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_118);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "[one] Descarga lista.", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_119,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_119);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"```ftl
{}
```

---

`$count=one`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                r#"download-state = Download ready.
"#,
                "download-state",
                &[],
            ),
            preview_message_text_with_overrides(
                &source_text,
                "download-state",
                &[("$count", "one")]
            ),
        ))
    );
    assert_eq!(hover["result"]["range"]["start"]["line"], Value::from(0));
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn hover_selector_without_origin_message_shows_one_local_selector_block() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"welcome = Hello.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"download-state =
    { $count ->
        [one] Descarga lista.
       *[other] Descargas listas.
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_134);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "[one] Descarga lista.", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_135,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_135);
    assert_eq!(
        hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"`$count=one`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &source_text,
                "download-state",
                &[("$count", "one")]
            ),
        ))
    );
    assert_eq!(hover["result"]["range"]["start"]["line"], Value::from(0));
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn hover_selector_preserves_unresolved_term_references_inside_selected_branch()
{
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"branch-note =
    { $platform ->
        [desktop] Open { -missing-brand } now.
       *[mobile] Open later.
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"branch-note =
    { $platform ->
        [desktop] Abre { -missing-brand } ahora.
       *[mobile] Abre despues.
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text =
        std::fs::read_to_string(workspace.path().join("locales/en/app.ftl"))
            .unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_136);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(
        &source_text,
        "[desktop] Abre { -missing-brand } ahora.",
        1,
    );
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_137,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_137);
    let hover_value = hover["result"]["contents"]["value"]
        .as_str()
        .expect("expected hover markdown")
        .to_string();
    assert_eq!(
        hover_value,
        format!(
            r#"`$platform=desktop`

```ftl
{}
```

---

`$platform=desktop`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "branch-note",
                &[("$platform", "desktop")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "branch-note",
                &[("$platform", "desktop")]
            ),
        )
    );
    assert_eq!(hover["result"]["range"]["start"]["line"], Value::from(0));
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        extract_ftl_blocks(&hover_value),
        vec![
            "Open { -missing-brand } now.".to_string(),
            "Abre { -missing-brand } ahora.".to_string(),
        ]
    );
}

#[test]
fn hover_selector_preserves_unresolved_non_selector_inline_references_inside_selected_branch()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"selector-copy =
    { $count ->
        [one] Show { missing-copy } now.
       *[other] Show later.
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"selector-copy =
    { $count ->
        [one] Muestra { missing-copy } ahora.
       *[other] Muestra despues.
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text =
        std::fs::read_to_string(workspace.path().join("locales/en/app.ftl"))
            .unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_138);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(
        &source_text,
        "[one] Muestra { missing-copy } ahora.",
        1,
    );
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_139,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let hover = recv_response(&mut lsp, 6_139);
    let hover_value = hover["result"]["contents"]["value"]
        .as_str()
        .expect("expected hover markdown")
        .to_string();
    assert_eq!(
        hover_value,
        format!(
            r#"`$count=one`

```ftl
{}
```

---

`$count=one`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "selector-copy",
                &[("$count", "one")]
            ),
            preview_message_text_with_overrides(
                &source_text,
                "selector-copy",
                &[("$count", "one")]
            ),
        )
    );
    assert_eq!(hover["result"]["range"]["start"]["line"], Value::from(0));
    assert_eq!(
        hover["result"]["range"]["start"]["character"],
        Value::from(0)
    );
    assert_eq!(
        extract_ftl_blocks(&hover_value),
        vec![
            "Show { missing-copy } now.".to_string(),
            "Muestra { missing-copy } ahora.".to_string(),
        ]
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

    for (index, (relative_path, body_needle)) in
        translation_cases.iter().enumerate()
    {
        let path = root.join(relative_path);
        let source = std::fs::read_to_string(&path).unwrap();
        open_document(&mut lsp, &path, &source);
        let request_id = 61 + i64::try_from(index).unwrap();
        let position = position_of_nth(&source, body_needle, 1);
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let hover = recv_response(&mut lsp, request_id);
        let value = hover["result"]["contents"]["value"].as_str().unwrap();
        assert_eq!(
            extract_ftl_blocks(value),
            vec![
                preview_message_text_with_overrides(
                    &origin_text,
                    "commented-preview",
                    &[]
                ),
                preview_message_text_with_overrides(
                    &source,
                    "commented-preview",
                    &[]
                ),
            ]
        );
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

    let key_position = position_of_nth(&source_text, "zero-rollout", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 63,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": key_position.0, "character": key_position.1 }
        }
    }));
    let key_hover = recv_response(&mut lsp, 63);
    assert_eq!(
        key_hover["result"],
        Value::Null,
        "key hover should not degrade into body preview: {key_hover:?}"
    );

    let body_position = position_of_nth(&source_text, "Zero summary", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 64,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": body_position.0, "character": body_position.1 }
        }
    }));
    let body_hover = recv_response(&mut lsp, 64);
    let body_value =
        body_hover["result"]["contents"]["value"].as_str().unwrap();
    assert_eq!(
        extract_ftl_blocks(body_value),
        vec![preview_message_text_with_overrides(
            &source_text,
            "zero-rollout",
            &[],
        )]
    );
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
        initialize["result"]["capabilities"]["executeCommandProvider"]["commands"]
            [0],
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
    let document_text =
        std::fs::read_to_string(document_path).expect("read temp document");
    let expected_document = r#"# Selector combinations for `install-hint`

Current language: `es`

Source language: `en`

Logical file: `app`

Source text:

```ftl
# Shortcut reminder near the download button
```

```ftl
install-hint =
    Copy the download link for { $gender ->
        [female] her
        [male] his
       *[other] their
    } account on { $count } { $count ->
        [one] device
       *[other] devices
    } now.

```

Source language combinations:
`$gender=female`, `$count=one`
```ftl
Copy the download link for her account on { $count } device now.
```
`$gender=female`, `$count=other`
```ftl
Copy the download link for her account on { $count } devices now.
```
`$gender=male`, `$count=one`
```ftl
Copy the download link for his account on { $count } device now.
```
`$gender=male`, `$count=other`
```ftl
Copy the download link for his account on { $count } devices now.
```
`$gender=other`, `$count=one`
```ftl
Copy the download link for their account on { $count } device now.
```
`$gender=other`, `$count=other`
```ftl
Copy the download link for their account on { $count } devices now.
```

Current text:

```ftl
install-hint =
    Copia el enlace de descarga para la cuenta de { $gender ->
        [female] ella
        [male] el
       *[other] elle
    } en { $count } { $count ->
        [one] dispositivo
       *[other] dispositivos
    } ahora.
```

Current language combinations:
`$gender=female`, `$count=one`
```ftl
Copia el enlace de descarga para la cuenta de ella en { $count } dispositivo ahora.
```
`$gender=female`, `$count=other`
```ftl
Copia el enlace de descarga para la cuenta de ella en { $count } dispositivos ahora.
```
`$gender=male`, `$count=one`
```ftl
Copia el enlace de descarga para la cuenta de el en { $count } dispositivo ahora.
```
`$gender=male`, `$count=other`
```ftl
Copia el enlace de descarga para la cuenta de el en { $count } dispositivos ahora.
```
`$gender=other`, `$count=one`
```ftl
Copia el enlace de descarga para la cuenta de elle en { $count } dispositivo ahora.
```
`$gender=other`, `$count=other`
```ftl
Copia el enlace de descarga para la cuenta de elle en { $count } dispositivos ahora.
```"#;
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
fn execute_command_opens_selector_combinations_for_first_and_last_entries() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"first-choice =
    { $count ->
        [one] first
       *[other] firsts
    }

middle = Middle

last-choice =
    { $count ->
        [one] last
       *[other] lasts
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"first-choice =
    { $count ->
        [one] primero
       *[other] primeros
    }

middle = Medio

last-choice =
    { $count ->
        [one] ultimo
       *[other] ultimos
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = LspProcess::start();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_090,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", workspace.path().display()),
            "capabilities": {
                "window": {
                    "showDocument": {
                        "support": true
                    }
                }
            }
        }
    }));
    assert_eq!(lsp.recv()["id"], 42_090);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    assert_eq!(
        recv_notification(&mut lsp, "window/logMessage")["method"],
        "window/logMessage"
    );
    open_document(&mut lsp, &source_path, &source_text);

    for (request_id, key) in [(42_091, "first-choice"), (42_092, "last-choice")]
    {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "workspace/executeCommand",
            "params": {
                "command": "fluent-lsp.showSelectorCombinations",
                "arguments": [
                    format!("file://{}", source_path.display()),
                    key
                ]
            }
        }));
        let request = lsp.recv();
        assert_eq!(request["method"], "window/showDocument");
        let document_uri = request["params"]["uri"].as_str().unwrap();
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
                .is_some_and(|tail| tail.ends_with(&format!("-{key}.md"))),
            "unexpected temp document uri: {document_uri}"
        );
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {
                "success": true
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], Value::Null);
    }
}

#[test]
fn code_lens_returns_empty_list_for_files_without_selector_combinations() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"hello = Hello
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"hello = Hola
"#,
        ),
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
fn code_lens_returns_exact_selector_combination_lens_array_for_first_middle_and_last_messages()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"# first lens comment
first-count =
    { $count ->
        [one] one
       *[other] other
    }

plain = plain

middle-rollout =
    For { $gender ->
        [female] her
       *[other] them
    } on { $count } { $count ->
        [one] device
       *[other] devices
    } now.

# last lens comment
last-audience =
    For { $audience ->
        [admins] admins
       *[others] others
    } on { $platform ->
        [desktop] desktop
       *[mobile] mobile
    } with { $count } { $count ->
        [one] item
       *[other] items
    }.
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"# first lens comment
first-count =
    { $count ->
        [one] uno
       *[other] otros
    }

plain = texto

middle-rollout =
    Para { $gender ->
        [female] ella
       *[other] elle
    } en { $count } { $count ->
        [one] dispositivo
       *[other] dispositivos
    } ahora.

# last lens comment
last-audience =
    Para { $audience ->
        [admins] admins
       *[others] others
    } en { $platform ->
        [desktop] escritorio
       *[mobile] movil
    } con { $count } { $count ->
        [one] elemento
       *[other] elementos
    }.
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let source_uri = format!("file://{}", source_path.display());

    let mut lsp = initialized_lsp(workspace.path(), 42_140);
    open_document(&mut lsp, &source_path, &source_text);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_141,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": source_uri }
        }
    }));

    let lenses = recv_response(&mut lsp, 42_141);
    assert_eq!(
        lenses["result"],
        json!([
            {
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 11 }
                },
                "command": {
                    "title": "Show all 2 selector combinations",
                    "command": "fluent-lsp.showSelectorCombinations",
                    "arguments": [source_uri, "first-count"]
                }
            },
            {
                "range": {
                    "start": { "line": 9, "character": 0 },
                    "end": { "line": 9, "character": 14 }
                },
                "command": {
                    "title": "Show all 4 selector combinations",
                    "command": "fluent-lsp.showSelectorCombinations",
                    "arguments": [source_uri, "middle-rollout"]
                }
            },
            {
                "range": {
                    "start": { "line": 19, "character": 0 },
                    "end": { "line": 19, "character": 13 }
                },
                "command": {
                    "title": "Show all 8 selector combinations",
                    "command": "fluent-lsp.showSelectorCombinations",
                    "arguments": [source_uri, "last-audience"]
                }
            }
        ])
    );
}

#[test]
fn code_lens_returns_exact_empty_arrays_for_remaining_no_lens_matrix_cases() {
    let basic_cases = [
        (
            42_150,
            &[
                ("locales/en/app.ftl", "hello = Hello\n"),
                ("locales/es/app.ftl", "hello = Hola\n"),
            ][..],
            "locales/es/app.ftl",
            None,
        ),
        (
            42_151,
            &[
                (
                    "locales/en/app.ftl",
                    "same-copy =\n    { $count ->\n        [one] Same text.\n       *[other] Same text.\n    }\n",
                ),
                (
                    "locales/es/app.ftl",
                    "same-copy =\n    { $count ->\n        [one] Mismo texto.\n       *[other] Mismo texto.\n    }\n",
                ),
            ][..],
            "locales/es/app.ftl",
            None,
        ),
        (
            42_152,
            &[
                (
                    "locales/en/app.ftl",
                    "only-other =\n    { $count ->\n       *[other] only branch\n    }\n",
                ),
                (
                    "locales/es/app.ftl",
                    "only-other =\n    { $count ->\n       *[other] solo rama\n    }\n",
                ),
            ][..],
            "locales/es/app.ftl",
            None,
        ),
        (
            42_153,
            &[
                (
                    "locales/en/app.ftl",
                    "ambiguous-key =\n    { $count ->\n        [one] first\n       *[other] firsts\n    }\n\nambiguous-key =\n    { $count ->\n        [one] second\n       *[other] seconds\n    }\n",
                ),
                (
                    "locales/es/app.ftl",
                    "ambiguous-key =\n    { $count ->\n        [one] primero\n       *[other] primeros\n    }\n\nambiguous-key =\n    { $count ->\n        [one] segundo\n       *[other] segundos\n    }\n",
                ),
            ][..],
            "locales/es/app.ftl",
            None,
        ),
        (
            42_154,
            &[
                (
                    "locales/en/app.ftl",
                    "-term-only =\n    { $count ->\n        [one] one\n       *[other] other\n    }\n",
                ),
                (
                    "locales/es/app.ftl",
                    "-term-only =\n    { $count ->\n        [one] uno\n       *[other] otros\n    }\n",
                ),
            ][..],
            "locales/es/app.ftl",
            None,
        ),
        (
            42_155,
            &[
                (
                    "locales/en/app.ftl",
                    "download-action =\n    .tooltip =\n        { $count ->\n            [one] one file\n           *[other] other files\n        }\n",
                ),
                (
                    "locales/es/app.ftl",
                    "download-action =\n    .tooltip =\n        { $count ->\n            [one] un archivo\n           *[other] otros archivos\n        }\n",
                ),
            ][..],
            "locales/es/app.ftl",
            None,
        ),
        (
            42_156,
            &[
                ("locales/en/app.ftl", "hello = Hello\n"),
                ("locales/es/app.ftl", "hello = Hola\n"),
            ][..],
            "locales/en/app.ftl",
            None,
        ),
        (
            42_157,
            &[
                ("locales/en/dialogs/menu.ftl", "menu-save = Save\n"),
                ("locales/es/dialogs/menu.ftl", "menu-save = Guardar\n"),
            ][..],
            "locales/es/dialogs/menu.ftl",
            None,
        ),
        (
            42_158,
            &[
                ("locales/en/app.ftl", "hello = Hello\n"),
                ("locales/es/app.ftl", "hello = Hola\n"),
            ][..],
            "locales/es/empty.ftl",
            Some(""),
        ),
        (
            42_159,
            &[
                ("locales/en/app.ftl", "# comments only\n"),
                ("locales/es/app.ftl", "# solo comentarios\n"),
            ][..],
            "locales/es/app.ftl",
            None,
        ),
    ];

    for (request_id, files, request_path, open_text) in basic_cases {
        let workspace = temp_workspace(files);
        let source_path = workspace.path().join(request_path);
        let mut lsp = initialized_lsp(workspace.path(), request_id);

        if let Some(text) = open_text {
            send_open_document(&mut lsp, &source_path, text);
        } else if source_path.exists() {
            let source_text = std::fs::read_to_string(&source_path).unwrap();
            open_document(&mut lsp, &source_path, &source_text);
        }

        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id + 1_000,
            "method": "textDocument/codeLens",
            "params": {
                "textDocument": {
                    "uri": format!("file://{}", source_path.display())
                }
            }
        }));

        let lenses = recv_response(&mut lsp, request_id + 1_000);
        assert_eq!(lenses["result"], Value::Array(Vec::new()));
    }
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
fn execute_command_rejects_malformed_arguments_and_missing_keys_exactly() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"install-hint =
    { $count ->
        [one] one
       *[other] other
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"install-hint =
    { $count ->
        [one] uno
       *[other] otros
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let outside = tempdir().unwrap();
    let outside_path = outside.path().join("outside.ftl");
    std::fs::write(
        &outside_path,
        r#"install-hint =
    { $count ->
        [one] uno
       *[other] otros
    }
"#,
    )
    .unwrap();

    let mut lsp = initialized_lsp_with_capabilities(
        workspace.path(),
        42_110,
        json!({
            "window": {
                "showDocument": {
                    "support": true
                }
            }
        }),
    );
    for (request_id, arguments, expected_code, expected_message) in [
        (
            42_111,
            json!([42, "install-hint"]),
            -32602,
            "document URI argument must be a string".to_string(),
        ),
        (
            42_112,
            json!([format!("file://{}", source_path.display()), 7]),
            -32602,
            "Fluent key argument must be a string".to_string(),
        ),
        (
            42_113,
            json!([
                "install-hint",
                format!("file://{}", source_path.display())
            ]),
            -32602,
            "document URI argument must be a valid URI".to_string(),
        ),
        (
            42_114,
            json!([
                format!("file://{}", source_path.display()),
                "install-hint",
                "extra"
            ]),
            -32602,
            "expected exactly two arguments: document URI and Fluent key"
                .to_string(),
        ),
        (
            42_115,
            json!([format!("file://{}", source_path.display()), "missing-key"]),
            -32602,
            "no selector combinations available for `missing-key`".to_string(),
        ),
        (
            42_116,
            json!([
                format!("file://{}", outside_path.display()),
                "install-hint"
            ]),
            -32602,
            format!(
                "document is outside the configured Fluent workspace: {}",
                outside_path.display()
            ),
        ),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "workspace/executeCommand",
            "params": {
                "command": "fluent-lsp.showSelectorCombinations",
                "arguments": arguments
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["error"]["code"], Value::from(expected_code));
        assert_eq!(
            response["error"]["message"],
            Value::String(expected_message)
        );
    }
}

#[test]
fn execute_command_requires_show_document_support_and_skips_temp_document_fallback()
 {
    let fluent_key = "install-hint-no-show-document-support";
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"install-hint-no-show-document-support =
    { $count ->
        [one] one
       *[other] other
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"install-hint-no-show-document-support =
    { $count ->
        [one] uno
       *[other] otros
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let temp_document_count = || {
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("fluent-lsp-selector-combinations-")
                        && name.ends_with(
                            "-install-hint-no-show-document-support.md",
                        )
                })
            })
            .count()
    };

    let before = temp_document_count();
    let mut lsp = initialized_lsp(workspace.path(), 42_120);
    open_document(&mut lsp, &source_path, &source_text);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_121,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": [
                format!("file://{}", source_path.display()),
                fluent_key
            ]
        }
    }));

    let response = recv_response(&mut lsp, 42_121);
    assert_eq!(response["error"]["code"], Value::from(-32603));
    assert_eq!(
        response["error"]["message"],
        Value::String(
            "client does not support window/showDocument".to_string()
        )
    );
    assert_eq!(temp_document_count(), before);
}

#[test]
fn execute_command_without_show_document_never_sends_show_message_and_keeps_failure_contract_for_origin_and_nested_documents()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"install-hint =
    { $count ->
        [one] one
       *[other] other
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"install-hint =
    { $count ->
        [one] uno
       *[other] otros
    }
"#,
        ),
        (
            "locales/en/dialogs/menu.ftl",
            r#"menu-status =
    { $count ->
        [one] menu one
       *[other] menu other
    }
"#,
        ),
        (
            "locales/es/dialogs/menu.ftl",
            r#"menu-status =
    { $count ->
        [one] menu uno
       *[other] menu otros
    }
"#,
        ),
    ]);
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let nested_path = workspace.path().join("locales/es/dialogs/menu.ftl");

    let mut lsp = initialized_lsp(workspace.path(), 42_122);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_123,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": [
                format!("file://{}", translation_path.display()),
                "install-hint"
            ]
        }
    }));

    let first_message = lsp.recv();
    assert_eq!(first_message["id"], Value::from(42_123));
    assert_eq!(first_message["error"]["code"], Value::from(-32603));
    assert_eq!(
        first_message["error"]["message"],
        Value::String(
            "client does not support window/showDocument".to_string()
        )
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_124,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.unknown",
            "arguments": []
        }
    }));
    let second_message = lsp.recv();
    assert_eq!(second_message["id"], Value::from(42_124));
    assert_eq!(second_message["error"]["code"], Value::from(-32602));
    assert_eq!(
        second_message["error"]["message"],
        Value::String("unknown command: fluent-lsp.unknown".to_string())
    );

    for (request_id, path, key) in [
        (42_125, &origin_path, "install-hint"),
        (42_126, &nested_path, "menu-status"),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "workspace/executeCommand",
            "params": {
                "command": "fluent-lsp.showSelectorCombinations",
                "arguments": [
                    format!("file://{}", path.display()),
                    key
                ]
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["error"]["code"], Value::from(-32603));
        assert_eq!(
            response["error"]["message"],
            Value::String(
                "client does not support window/showDocument".to_string()
            )
        );
    }
}

#[test]
fn execute_command_surfaces_show_document_decline_and_rpc_error_exactly() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"install-hint =
    { $count ->
        [one] one
       *[other] other
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"install-hint =
    { $count ->
        [one] uno
       *[other] otros
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");

    let mut lsp = initialized_lsp_with_capabilities(
        workspace.path(),
        42_127,
        json!({
            "window": {
                "showDocument": {
                    "support": true
                }
            }
        }),
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_128,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": [
                format!("file://{}", source_path.display()),
                "install-hint"
            ]
        }
    }));
    let declined_request = lsp.recv();
    assert_eq!(declined_request["method"], "window/showDocument");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": declined_request["id"],
        "result": {
            "success": false
        }
    }));
    let declined_response = recv_response(&mut lsp, 42_128);
    assert_eq!(declined_response["error"]["code"], Value::from(-32603));
    assert_eq!(
        declined_response["error"]["message"],
        Value::String(
            "client declined to show the selector combinations document"
                .to_string()
        )
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 42_129,
        "method": "workspace/executeCommand",
        "params": {
            "command": "fluent-lsp.showSelectorCombinations",
            "arguments": [
                format!("file://{}", source_path.display()),
                "install-hint"
            ]
        }
    }));
    let error_request = lsp.recv();
    assert_eq!(error_request["method"], "window/showDocument");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": error_request["id"],
        "error": {
            "code": -32099,
            "message": "showDocument exploded"
        }
    }));
    let error_response = recv_response(&mut lsp, 42_129);
    assert_eq!(error_response["error"]["code"], Value::from(-32603));
    assert_eq!(
        error_response["error"]["message"],
        Value::String(
            "failed to open selector combinations document: Server error: showDocument exploded"
                .to_string()
        )
    );
}

#[test]
fn execute_command_generated_selector_combination_document_preserves_unresolved_references_exactly()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"branch-note =
    { $platform ->
        [desktop] Open { -missing-brand } now.
       *[mobile] Open later.
    }

selector-copy =
    { $count ->
        [one] Show { missing-copy } now.
       *[other] Show later.
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"branch-note =
    { $platform ->
        [desktop] Abre { -missing-brand } ahora.
       *[mobile] Abre despues.
    }

selector-copy =
    { $count ->
        [one] Muestra { missing-copy } ahora.
       *[other] Muestra despues.
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");

    let mut lsp = initialized_lsp_with_capabilities(
        workspace.path(),
        42_130,
        json!({
            "window": {
                "showDocument": {
                    "support": true
                }
            }
        }),
    );

    for (request_id, key, expected_document) in [
        (
            42_131,
            "branch-note",
            r#"# Selector combinations for `branch-note`

Current language: `es`

Source language: `en`

Logical file: `app`

Source text:

```ftl
branch-note =
    { $platform ->
        [desktop] Open { -missing-brand } now.
       *[mobile] Open later.
    }

```

Source language combinations:
`$platform=desktop`
```ftl
Open { -missing-brand } now.
```
`$platform=mobile`
```ftl
Open later.
```

Current text:

```ftl
branch-note =
    { $platform ->
        [desktop] Abre { -missing-brand } ahora.
       *[mobile] Abre despues.
    }

```

Current language combinations:
`$platform=desktop`
```ftl
Abre { -missing-brand } ahora.
```
`$platform=mobile`
```ftl
Abre despues.
```"#,
        ),
        (
            42_132,
            "selector-copy",
            r#"# Selector combinations for `selector-copy`

Current language: `es`

Source language: `en`

Logical file: `app`

Source text:

```ftl
selector-copy =
    { $count ->
        [one] Show { missing-copy } now.
       *[other] Show later.
    }
```

Source language combinations:
`$count=one`
```ftl
Show { missing-copy } now.
```
`$count=other`
```ftl
Show later.
```

Current text:

```ftl
selector-copy =
    { $count ->
        [one] Muestra { missing-copy } ahora.
       *[other] Muestra despues.
    }
```

Current language combinations:
`$count=one`
```ftl
Muestra { missing-copy } ahora.
```
`$count=other`
```ftl
Muestra despues.
```"#,
        ),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "workspace/executeCommand",
            "params": {
                "command": "fluent-lsp.showSelectorCombinations",
                "arguments": [
                    format!("file://{}", source_path.display()),
                    key
                ]
            }
        }));
        let request = lsp.recv();
        assert_eq!(request["method"], "window/showDocument");

        let document_uri = request["params"]["uri"]
            .as_str()
            .expect("showDocument uri must be a string");
        let document_path = document_uri
            .strip_prefix("file://")
            .expect("expected file uri for temp document");
        let document_text =
            std::fs::read_to_string(document_path).expect("read temp document");
        assert_eq!(document_text, expected_document);

        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {
                "success": true
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], Value::Null);
    }
}

#[test]
fn code_action_generates_prefix_selector_by_default() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 70);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "coins-line", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 71,
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
    let actions = recv_response(&mut lsp, 71)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 3);
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String(
                    "Generate number selector (prefix)".to_string(),
                )
        })
        .expect("missing code action: Generate number selector (prefix)");
    assert_eq!(
        action["title"],
        Value::String("Generate number selector (prefix)".to_string())
    );
    assert_eq!(
        action["kind"],
        Value::String("refactor.rewrite".to_string())
    );
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Tienes { $coins } { $coins ->
    [one] monedas.
    *[other] monedas.
}"#
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

    let position = position_of_nth(&source_text, "{ $coins }", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 73,
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
    let actions = recv_response(&mut lsp, 73)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 3);
    assert!(actions.iter().any(|action| action["title"]
        == Value::String(
            "Generate number selector from $coins (prefix)".to_string()
        )));
    assert!(actions.iter().any(|action| action["title"]
        == Value::String(
            "Generate number selector from $coins (whole)".to_string()
        )));
    assert!(actions.iter().any(|action| action["title"]
        == Value::String(
            "Generate number selector from $coins (suffix)".to_string()
        )));
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

    let position = position_of_nth(&source_text, "{ $coins }", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 75,
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
    let actions = recv_response(&mut lsp, 75)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    let action = &actions[0];
    assert_eq!(
        action["title"],
        Value::String(
            "Generate number selector from $coins (whole)".to_string()
        )
    );
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"{ $coins ->
    [one] Tienes { $coins } monedas.
    *[other] Tienes { $coins } monedas.
}"#
            .to_string()
        )
    );
}

#[test]
fn documented_config_contract_resolves_counterparts_from_exact_file_shape() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/en/dialogs"))
        .unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es/dialogs"))
        .unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("locales/en/app.ftl"),
        r#"welcome-title = Welcome
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("locales/es/app.ftl"),
        r#"welcome-title = Bienvenido
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("locales/en/dialogs/menu.ftl"),
        r#"button-copy =
    .label = Launch
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("locales/es/dialogs/menu.ftl"),
        r#"button-copy =
    .label = Abrir
"#,
    )
    .unwrap();

    let app_path = workspace.path().join("locales/es/app.ftl");
    let app_text = std::fs::read_to_string(&app_path).unwrap();
    let nested_path = workspace.path().join("locales/es/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 75_100);
    open_document(&mut lsp, &app_path, &app_text);
    let app_position =
        position_of_nth(&app_text, "welcome-title = Bienvenido", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 75_101,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": app_position.0, "character": app_position.1 }
        }
    }));
    let app_definition = recv_response(&mut lsp, 75_101);
    assert_eq!(
        app_definition["result"]["uri"],
        Value::String(format!(
            "file://{}",
            workspace.path().join("locales/en/app.ftl").display()
        ))
    );
    assert_eq!(
        app_definition["result"]["range"]["start"]["line"],
        Value::from(0)
    );
    assert_eq!(
        app_definition["result"]["range"]["start"]["character"],
        Value::from(0)
    );

    open_document(&mut lsp, &nested_path, &nested_text);
    let nested_position = position_of_nth(&nested_text, "label = Abrir", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 75_102,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_path.display()) },
            "position": { "line": nested_position.0, "character": nested_position.1 }
        }
    }));
    let nested_definition = recv_response(&mut lsp, 75_102);
    assert_eq!(
        nested_definition["result"]["uri"],
        Value::String(format!(
            "file://{}",
            workspace
                .path()
                .join("locales/en/dialogs/menu.ftl")
                .display()
        ))
    );
    assert_eq!(
        nested_definition["result"]["range"]["start"]["line"],
        Value::from(1)
    );
    assert_eq!(
        nested_definition["result"]["range"]["start"]["character"],
        Value::from(5)
    );
}

#[test]
fn client_configuration_applies_origin_language_and_file_masks_without_file_config()
 {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("messages/fr")).unwrap();
    std::fs::create_dir_all(workspace.path().join("messages/es")).unwrap();
    std::fs::write(
        workspace.path().join("messages/fr/app.ftl"),
        r#"welcome-title = Bonjour
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("messages/es/app.ftl"),
        r#"welcome-title = Hola
"#,
    )
    .unwrap();

    let source_path = workspace.path().join("messages/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 75_110);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "origin_language": "fr",
                "file_masks": ["messages/{lang}/{filepath}.ftl"]
            }
        }),
    );
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "welcome-title = Hola", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 75_111,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let response = recv_response(&mut lsp, 75_111);
    assert_eq!(
        response["result"]["uri"],
        Value::String(format!(
            "file://{}",
            workspace.path().join("messages/fr/app.ftl").display()
        ))
    );
    assert_eq!(response["result"]["range"]["start"]["line"], Value::from(0));
    assert_eq!(
        response["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn file_config_overrides_client_origin_language_and_file_masks() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/es")).unwrap();
    std::fs::create_dir_all(workspace.path().join("locales/fr")).unwrap();
    std::fs::create_dir_all(workspace.path().join("messages/fr")).unwrap();
    std::fs::write(
        workspace.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("locales/en/app.ftl"),
        r#"welcome-title = Welcome
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("locales/fr/app.ftl"),
        r#"welcome-title = Bonjour
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("locales/es/app.ftl"),
        r#"welcome-title = Bienvenido
"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("messages/fr/app.ftl"),
        r#"welcome-title = Salut
"#,
    )
    .unwrap();

    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 75_120);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "origin_language": "fr",
                "file_masks": ["messages/{lang}/{filepath}.ftl"]
            }
        }),
    );
    open_document(&mut lsp, &source_path, &source_text);

    let position =
        position_of_nth(&source_text, "welcome-title = Bienvenido", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 75_121,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": position.0, "character": position.1 }
        }
    }));
    let response = recv_response(&mut lsp, 75_121);
    assert_eq!(
        response["result"]["uri"],
        Value::String(format!(
            "file://{}",
            workspace.path().join("locales/en/app.ftl").display()
        ))
    );
    assert_eq!(response["result"]["range"]["start"]["line"], Value::from(0));
    assert_eq!(
        response["result"]["range"]["start"]["character"],
        Value::from(0)
    );
}

#[test]
fn file_config_selector_style_overrides_client_setting() {
    let fixture = fixture_root();
    let temp = tempdir().unwrap();
    copy_dir(&fixture, temp.path());
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
selector_style = "whole"
"#,
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

    let position = position_of_nth(&source_text, "coins-line", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 77,
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
    let actions = recv_response(&mut lsp, 77)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
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

    let position = position_of_nth(&source_text, "coins-line", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 79,
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
    let actions = recv_response(&mut lsp, 79)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String(
                    "Generate number selector (prefix)".to_string(),
                )
        })
        .expect("missing code action: Generate number selector (prefix)");
    assert_eq!(
        action["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            r#"Tienes { \$${1:coins} } { \$${1:coins} ->
    [one] monedas.
    *[other] monedas.
}"#
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

    let position = position_of_nth(&source_text, "plain-count", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 81,
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
    let actions = recv_response(&mut lsp, 81)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0]["title"],
        Value::String("Generate number selector (whole)".to_string())
    );
    assert_eq!(
        actions[0]["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            r#"{ \$${1:count} ->
    [one] Monedas disponibles.
    *[other] Monedas disponibles.
}"#
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

    let position = position_of_nth(&source_text, "$files", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 83,
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
    let actions = recv_response(&mut lsp, 83)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(actions.iter().any(|action| action["title"]
        == Value::String(
            "Generate number selector from $files (prefix)".to_string()
        )));
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

    let key_position = position_of_nth(&source_text, "formatted-download", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 85,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": key_position.0, "character": key_position.1 },
                "end": { "line": key_position.0, "character": key_position.1 }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));
    let key_actions = recv_response(&mut lsp, 85)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let key_actions = key_actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(key_actions.len(), 3);
    let key_prefix = key_actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String(
                    "Generate number selector (prefix)".to_string(),
                )
        })
        .expect("missing code action: Generate number selector (prefix)");
    assert_eq!(
        key_prefix["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            r#"Descarga { NUMBER(\$${1:downloads}) } { \$${1:downloads} ->
    [one] archivos.
    *[other] archivos.
}"#
            .to_string()
        )
    );

    let variable_position = position_of_nth(&source_text, "$downloads", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 91,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": variable_position.0, "character": variable_position.1 },
                "end": { "line": variable_position.0, "character": variable_position.1 }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));
    let variable_actions = recv_response(&mut lsp, 91)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let variable_actions = variable_actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(variable_actions.len(), 3);
    let variable_prefix = variable_actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Generate number selector from $downloads (prefix)".to_string())
        })
        .expect("missing code action: Generate number selector from $downloads (prefix)");
    assert_eq!(
        variable_prefix["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            r#"Descarga { NUMBER($downloads) } { $downloads ->
    [one] archivos.
    *[other] archivos.
}"#
            .to_string()
        )
    );

    let function_position =
        position_of_nth(&source_text, "NUMBER($downloads)", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 104,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": function_position.0, "character": function_position.1 },
                "end": { "line": function_position.0, "character": function_position.1 }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));
    let function_actions = recv_response(&mut lsp, 104)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let function_actions = function_actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(function_actions.len(), 3);
    let function_prefix = function_actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String(
                    "Generate number selector from NUMBER($downloads) (prefix)".to_string(),
                )
        })
        .expect("missing code action: Generate number selector from NUMBER($downloads) (prefix)");
    assert_eq!(
        function_prefix["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            r#"Descarga { NUMBER($downloads) } { NUMBER($downloads) ->
    [one] archivos.
    *[other] archivos.
}"#
            .to_string()
        )
    );

    let deep_variable_position =
        position_of_nth(&source_text, "WRAP(NUMBER($downloads))", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 105,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "range": {
                "start": { "line": deep_variable_position.0, "character": deep_variable_position.1 },
                "end": { "line": deep_variable_position.0, "character": deep_variable_position.1 }
            },
            "context": {
                "diagnostics": []
            }
        }
    }));
    let deep_variable_actions = recv_response(&mut lsp, 105)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let deep_variable_actions = deep_variable_actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(deep_variable_actions.len(), 3);
    let deep_variable_prefix = deep_variable_actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String(
                    "Generate number selector from WRAP(NUMBER($downloads)) (prefix)".to_string(),
                )
        })
        .expect(
            "missing code action: Generate number selector from WRAP(NUMBER($downloads)) (prefix)",
        );
    assert_eq!(
        deep_variable_prefix["edit"]["documentChanges"][0]["edits"][0]["snippet"],
        Value::String(
            r#"Descarga { WRAP(NUMBER($downloads)) } { WRAP(NUMBER($downloads)) ->
    [one] archivos.
    *[other] archivos.
}"#
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

    let position = position_of_nth(&source_text, "coins-period", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 93,
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
    let actions = recv_response(&mut lsp, 93)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String(
                    "Generate number selector (prefix)".to_string(),
                )
        })
        .expect("missing code action: Generate number selector (prefix)");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Tienes { $coins } { $coins ->
    [one].
    *[other].
}"#
            .to_string()
        )
    );
}

#[test]
fn code_action_generation_is_absent_when_message_already_has_selector() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 116);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "whole-coins", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 117,
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
    let actions = recv_response(&mut lsp, 117)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut titles = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
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

    let position = position_of_nth(&source_text, "{ $files ->", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 119,
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
    let actions = recv_response(&mut lsp, 119)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut titles = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
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

    let position = position_of_nth(&source_text, "whole-coins", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 95,
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
    let actions = recv_response(&mut lsp, 95)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Convert selector to prefix form".to_string())));
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Convert selector to suffix form".to_string())));

    let prefix = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to prefix form".to_string())
        })
        .expect("missing code action: Convert selector to prefix form");
    assert_eq!(
        prefix["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Tienes { $coins } { $coins ->
    [one] moneda.
    *[other] monedas.
}"#
            .to_string()
        )
    );

    let suffix = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to suffix form".to_string())
        })
        .expect("missing code action: Convert selector to suffix form");
    assert_eq!(
        suffix["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Tienes { $coins ->
    [one] { $coins } moneda.
    *[other] { $coins } monedas.
}"#
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

    let position = position_of_nth(&source_text, "prefix-coins", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 97,
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
    let actions = recv_response(&mut lsp, 97)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to whole form".to_string())
        })
        .expect("missing code action: Convert selector to whole form");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"{ $coins ->
    [one] Tienes { $coins } moneda.
    *[other] Tienes { $coins } monedas.
}"#
            .to_string()
        )
    );
}

#[test]
fn code_action_selector_rewrite_preserves_assignment_spacing() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"placeholder = Hello
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"prefix-coins = Tienes { $coins } { $coins ->
    [one] moneda.
    *[other] monedas.
}
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 97_100);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "prefix-coins", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 97_101,
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
    let actions = recv_response(&mut lsp, 97_101)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to whole form".to_string())
        })
        .expect("missing code action: Convert selector to whole form");
    let updated = Editor::new(&source_text)
        .apply_code_action(action, &format!("file://{}", source_path.display()))
        .source;

    assert_eq!(
        updated,
        r#"prefix-coins = { $coins ->
    [one] Tienes { $coins } moneda.
    *[other] Tienes { $coins } monedas.
}
"#
    );
}

#[test]
fn code_action_rewrites_suffix_selector_to_whole_and_prefix() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 98);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "suffix-coins", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 99,
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
    let actions = recv_response(&mut lsp, 99)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Convert selector to whole form".to_string())));
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Convert selector to prefix form".to_string())));
}

#[test]
fn code_action_bare_suffix_like_selector_only_offers_prefix() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 100);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "bare-suffix-coins", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 101,
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
    let actions = recv_response(&mut lsp, 101)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to prefix form".to_string())
        })
        .expect("missing code action: Convert selector to prefix form");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"{ $coins } { $coins ->
    [one] moneda.
    *[other] monedas.
}"#
            .to_string()
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

    let position = position_of_nth(&source_text, "Ella tiene", 2);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 103,
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
    let actions = recv_response(&mut lsp, 103)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to prefix form".to_string())
        })
        .expect("missing code action: Convert selector to prefix form");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Ella tiene { $coins } { $coins ->
            [one] moneda.
            *[other] monedas.
        }"#
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

    let position = position_of_nth(&source_text, "coins-line", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 121,
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
    let actions = recv_response(&mut lsp, 121)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut titles = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
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

    let position = position_of_nth(&source_text, "$files", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 123,
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
    let actions = recv_response(&mut lsp, 123)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut titles = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
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

    let position = position_of_nth(&source_text, "{ $files ->", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 111,
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
    let actions = recv_response(&mut lsp, 111)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let prefix = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to prefix form".to_string())
        })
        .expect("missing code action: Convert selector to prefix form");
    assert_eq!(
        prefix["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Descarga { $files } { $files ->
        [one] archivo.
        *[other] archivos.
    }"#
            .to_string()
        )
    );
    let suffix = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to suffix form".to_string())
        })
        .expect("missing code action: Convert selector to suffix form");
    assert_eq!(
        suffix["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Descarga { $files ->
        [one] { $files } archivo.
        *[other] { $files } archivos.
    }"#
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

    let position = position_of_nth(&source_text, "{ $files ->", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 113,
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
    let actions = recv_response(&mut lsp, 113)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let prefix = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to prefix form".to_string())
        })
        .expect("missing code action: Convert selector to prefix form");
    let edit = &prefix["edit"]["changes"]
        [format!("file://{}", source_path.display())][0];
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

    let position = position_of_nth(&source_text, "{ $count ->", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 107,
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
    let actions = recv_response(&mut lsp, 107)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(actions.iter().any(|action| action["title"]
        == Value::String("Convert selector to whole form".to_string())));
    let suffix = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to suffix form".to_string())
        })
        .expect("missing code action: Convert selector to suffix form");
    assert_eq!(
        suffix["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Copy the download link for { $gender ->
        [female] her
        [male] his
       *[other] their
    } account on { $count ->
    [one] { $count } device now.
    *[other] { $count } devices now.
}"#
            .to_string()
        )
    );
}

#[test]
fn code_action_rewrites_selected_gender_selector_to_whole_with_nested_count_preserved()
 {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(&root, 108);
    open_document(&mut lsp, &source_path, &source_text);

    let position = position_of_nth(&source_text, "{ $gender ->", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 109,
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
    let actions = recv_response(&mut lsp, 109)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(actions.len(), 1);
    let whole = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Convert selector to whole form".to_string())
        })
        .expect("missing code action: Convert selector to whole form");
    assert_eq!(
        whole["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#" { $gender ->
    [female] Copy the download link for her account on { $count } { $count ->
            [one] device
           *[other] devices
        } now.
    [male] Copy the download link for his account on { $count } { $count ->
            [one] device
           *[other] devices
        } now.
    *[other] Copy the download link for their account on { $count } { $count ->
            [one] device
           *[other] devices
        } now.
}"#
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

    let position = position_of_nth(&source_text, "range-summary", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 87,
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
    let actions = recv_response(&mut lsp, 87)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let actions = actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
    assert!(actions.is_empty());

    let position = position_of_nth(&source_text, "nested-coins", 1);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 88,
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
    let nested_actions = recv_response(&mut lsp, 88)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let nested_actions = nested_actions
        .into_iter()
        .filter(|action| action["kind"].as_str() == Some("refactor.rewrite"))
        .collect::<Vec<_>>();
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
            .filter(|message| *message
                == "Numeric selector for `lv` is missing category `zero`")
            .count(),
        2
    );

    let unsupported = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic["message"]
                == Value::String(
                    "`few` is not a supported plural category for `lv`"
                        .to_string(),
                )
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
            "Selector style is `suffix`, but workspace prefers `prefix`"
                .to_string(),
            "Selector style is `whole`, but workspace prefers `prefix`"
                .to_string(),
        ]
    );
}

#[test]
fn file_config_overrides_client_style_diagnostic_settings() {
    let temp = tempdir().unwrap();
    copy_dir(&fixture_root(), temp.path());
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
selector_style = "whole"
warn_on_selector_style_mismatch = true
"#,
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
            "Selector style is `prefix`, but workspace prefers `whole`"
                .to_string(),
            "Selector style is `suffix`, but workspace prefers `whole`"
                .to_string(),
        ]
    );
}

#[test]
fn diagnostics_report_invalid_numeric_identifier_keys_when_enabled() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"bad-key =
    { $count ->
        [admins] nope
        [one] ok
       *[other] ok
    }
"#;

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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
    let source_text = r#"bad-key =
    { $count ->
        [admins] nope
       *[other] ok
    }
"#;

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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
warn_on_missing_plural_categories = true
warn_on_selector_style_mismatch = true
selector_style = "whole"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/match.ftl"),
        r#"match-rollout =
    { $count ->
        [one] one package
       *[other] { $count } packages
    }
"#,
    )
    .unwrap();

    let source_path = temp.path().join("locales/en/match.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 123);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn client_configuration_applies_plural_diagnostic_settings_without_file_config()
{
    let temp = temp_workspace(&[(
        "locales/lv/app.ftl",
        r#"bad-zero =
    { $count ->
        [few] slikti
       *[other] labi
    }
"#,
    )]);
    let source_path = temp.path().join("locales/lv/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 125);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let mut diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .cloned()
        .expect("expected diagnostics array");
    diagnostics.sort_by_key(|diagnostic| {
        diagnostic["message"].as_str().unwrap().to_string()
    });

    assert_eq!(diagnostics.len(), 4);
    assert_eq!(
        diagnostics[0]["message"],
        Value::String(
            "Numeric selector for `lv` is missing category `one`".to_string()
        )
    );
    assert_eq!(diagnostics[0]["severity"], Value::from(2));
    assert_eq!(
        diagnostics[1]["message"],
        Value::String(
            "Numeric selector for `lv` is missing category `zero`".to_string()
        )
    );
    assert_eq!(diagnostics[1]["severity"], Value::from(2));
    assert_eq!(
        diagnostics[2]["message"],
        Value::String(
            "Translation file has no origin-language counterpart for `app`"
                .to_string()
        )
    );
    assert_eq!(diagnostics[2]["severity"], Value::from(2));
    assert_eq!(
        diagnostics[3]["message"],
        Value::String(
            "`few` is not a supported plural category for `lv`".to_string()
        )
    );
    assert_eq!(diagnostics[3]["severity"], Value::from(1));
}

#[test]
fn file_config_overrides_client_plural_diagnostic_settings() {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(temp.path().join("locales/lv")).unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
error_on_unsupported_plural_categories = false
warn_on_missing_plural_categories = false
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        r#"bad-zero =
    { $count ->
        [few] bad
       *[other] good
    }
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/lv/app.ftl"),
        r#"bad-zero =
    { $count ->
        [few] slikti
       *[other] labi
    }
"#,
    )
    .unwrap();
    let source_path = temp.path().join("locales/lv/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 126);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn diagnostics_report_english_zero_unsupported_category_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"zero-rollout =
    { $count ->
        [zero] no packages
        [one] one package
       *[other] packages
    }
"#;

    let mut lsp = initialized_lsp(&root, 127);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": position_of(source_text, "[zero]").0, "character": position_of(source_text, "[zero]").1 + 1 },
                    "end": { "line": position_of(source_text, "[zero]").0, "character": position_of(source_text, "[zero]").1 + 5 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "`zero` is not a supported plural category for `en`"
            }
        ])
    );
}

#[test]
fn diagnostics_report_invalid_numeric_identifier_key_range_first_branch() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"bad-key =
    { $count ->
        [admins] nope
        [one] ok
       *[other] ok
    }
"#;

    let mut lsp = initialized_lsp(&root, 128);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let key_position = position_of(source_text, "admins");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": key_position.0, "character": key_position.1 },
                    "end": { "line": key_position.0, "character": key_position.1 + 6 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories"
            }
        ])
    );
}

#[test]
fn diagnostics_report_invalid_numeric_identifier_key_range_last_branch() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"bad-key =
    { $count ->
        [one] ok
       *[admins] nope
    }
"#;

    let mut lsp = initialized_lsp(&root, 129);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let key_position = position_of(source_text, "admins");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": key_position.0, "character": key_position.1 },
                    "end": { "line": key_position.0, "character": key_position.1 + 6 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories"
            }
        ])
    );
}

#[test]
fn diagnostics_allow_exact_numeric_selector_keys() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"numeric-key =
    { $count ->
        [0] none
        [one] one
       *[other] many
    }
"#;

    let mut lsp = initialized_lsp(&root, 130);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn diagnostics_allow_supported_plural_selector_keys() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"plural-key =
    { $count ->
        [one] one
       *[other] many
    }
"#;

    let mut lsp = initialized_lsp(&root, 131);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn diagnostics_report_nested_invalid_numeric_identifier_key_range() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"nested-bad-key =
    { $gender ->
        [female] { $count ->
            [admins] nope
            [one] ok
           *[other] ok
        }
       *[other] ok
    }
"#;

    let mut lsp = initialized_lsp(&root, 132);
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let key_position = position_of(source_text, "admins");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": key_position.0, "character": key_position.1 },
                    "end": { "line": key_position.0, "character": key_position.1 + 6 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories"
            }
        ])
    );
}

#[test]
fn diagnostics_suppress_invalid_numeric_identifier_key_when_setting_disabled() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"bad-key =
    { $count ->
        [admins] nope
        [one] ok
       *[other] ok
    }
"#;

    let mut lsp = initialized_lsp(&root, 134);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": false
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );
}

#[test]
fn diagnostics_suppress_unsupported_category_when_setting_disabled() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"zero-rollout =
    { $count ->
        [zero] no packages
        [one] one package
       *[other] packages
    }
"#;

    let mut lsp = initialized_lsp(&root, 133);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": false
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();

    let source_path = temp.path().join("locales/en/broken.ftl");
    let invalid_source = r#"welcome-title = Welcome

g@Rb@ge = broken
"#;
    std::fs::write(&source_path, invalid_source).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 124);
    send_open_document(&mut lsp, &source_path, invalid_source);
    send_save_document(&mut lsp, &source_path, None);

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["message"],
        Value::String(
            "Fluent syntax error: Expected a token starting with \"=\""
                .to_string()
        )
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

    let valid_source = r#"welcome-title = Welcome

garbage = broken
"#;
    send_change_document(&mut lsp, &source_path, 2, valid_source);
    send_save_document(&mut lsp, &source_path, None);

    let cleared =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(cleared["params"]["diagnostics"], Value::Array(Vec::new()));
}

#[test]
fn parse_error_diagnostics_report_first_line_exactly() {
    let temp = temp_workspace(&[(
        "locales/en/first.ftl",
        r#"g@Rb@ge = broken
"#,
    )]);
    let source_path = temp.path().join("locales/en/first.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 146);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let error_position = position_of(&source_text, "@");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": error_position.0, "character": error_position.1 },
                    "end": { "line": error_position.0, "character": error_position.1 + 1 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "Fluent syntax error: Expected a token starting with \"=\""
            }
        ])
    );
}

#[test]
fn parse_error_diagnostics_report_last_line_exactly() {
    let temp = temp_workspace(&[(
        "locales/en/last.ftl",
        r#"welcome = Welcome

broken@key = broken
"#,
    )]);
    let source_path = temp.path().join("locales/en/last.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 147);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let error_position = position_of(&source_text, "@");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": error_position.0, "character": error_position.1 },
                    "end": { "line": error_position.0, "character": error_position.1 + 1 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "Fluent syntax error: Expected a token starting with \"=\""
            }
        ])
    );
}

#[test]
fn parse_error_dirty_edit_publishes_after_save_exactly() {
    let temp = temp_workspace(&[(
        "locales/en/dirty.ftl",
        r#"welcome = Welcome
"#,
    )]);
    let source_path = temp.path().join("locales/en/dirty.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let invalid_source = r#"welcome = Welcome

broken@key = broken
"#;

    let mut lsp = initialized_lsp(temp.path(), 148);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_change_document(&mut lsp, &source_path, 2, invalid_source);
    send_save_document(&mut lsp, &source_path, None);

    let error_position = position_of(invalid_source, "@");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": error_position.0, "character": error_position.1 },
                    "end": { "line": error_position.0, "character": error_position.1 + 1 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "Fluent syntax error: Expected a token starting with \"=\""
            }
        ])
    );
}

#[test]
fn parse_error_did_close_clears_diagnostics_exactly() {
    let temp = temp_workspace(&[(
        "locales/en/close.ftl",
        r#"broken@key = broken
"#,
    )]);
    let source_path = temp.path().join("locales/en/close.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 149);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));
    let _ = recv_notification(&mut lsp, "textDocument/publishDiagnostics");

    send_close_document(&mut lsp, &source_path);
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
fn parse_error_nested_file_contract_matches_top_level_exactly() {
    let temp = temp_workspace(&[(
        "locales/en/dialogs/nested.ftl",
        r#"welcome = Welcome

broken@key = broken
"#,
    )]);
    let source_path = temp.path().join("locales/en/dialogs/nested.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 150);
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let error_position = position_of(&source_text, "@");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": error_position.0, "character": error_position.1 },
                    "end": { "line": error_position.0, "character": error_position.1 + 1 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "Fluent syntax error: Expected a token starting with \"=\""
            }
        ])
    );
}

#[test]
fn diagnostics_report_local_selector_style_mismatches_when_enabled() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"install-hint =
    Copy the download link for { $gender ->
        [female] her
       *[fallback] their
    } account on { $count } { $count ->
        [one] device
       *[other] devices
    } now.
"#;

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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("expected diagnostics array");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["message"],
        Value::String(
            "Selector style is `whole`, but workspace prefers `prefix`"
                .to_string()
        )
    );
}

#[test]
fn diagnostics_report_latvian_unsupported_category_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = r#"bad-zero =
    { $count ->
        [few] slikti
       *[other] labi
    }
"#;

    let mut lsp = initialized_lsp(&root, 135);
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

    let key_position = position_of(source_text, "few");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": key_position.0, "character": key_position.1 },
                    "end": { "line": key_position.0, "character": key_position.1 + 3 }
                },
                "severity": 1,
                "source": "fluent-lsp",
                "message": "`few` is not a supported plural category for `lv`"
            }
        ])
    );
}

#[test]
fn diagnostics_report_latvian_missing_zero_category_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = r#"missing-zero =
    { $count ->
        [one] viena pakotne
       *[other] pakotnes
    }
"#;

    let mut lsp = initialized_lsp(&root, 136);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let start = position_of(source_text, "$count");
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": start.0, "character": start.1 },
                    "end": { "line": end.0, "character": end.1 + 4 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Numeric selector for `lv` is missing category `zero`"
            }
        ])
    );
}

#[test]
fn diagnostics_report_latvian_missing_one_category_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/lv/app.ftl");
    let source_text = r#"missing-one =
    { $count ->
        [zero] neviena pakotne
       *[other] pakotnes
    }
"#;

    let mut lsp = initialized_lsp(&root, 137);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let start = position_of(source_text, "$count");
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": start.0, "character": start.1 },
                    "end": { "line": end.0, "character": end.1 + 4 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Numeric selector for `lv` is missing category `one`"
            }
        ])
    );
}

#[test]
fn diagnostics_report_ukrainian_missing_category_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/uk/app.ftl");
    let source_text = r#"uk-incomplete =
    { $count ->
        [few] кілька пакунків
        [many] багато пакунків
       *[other] інші пакунки
    }
"#;

    let mut lsp = initialized_lsp(&root, 138);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let start = position_of(source_text, "$count");
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": start.0, "character": start.1 },
                    "end": { "line": end.0, "character": end.1 + 4 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Numeric selector for `uk` is missing category `one`"
            }
        ])
    );
}

#[test]
fn diagnostics_report_arabic_two_few_many_categories_exactly() {
    let temp = temp_workspace(&[(
        "locales/ar/app.ftl",
        r#"arabic-incomplete =
    { $count ->
        [zero] صفر
        [one] واحد
       *[other] آخر
    }
"#,
    )]);
    let source_path = temp.path().join("locales/ar/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 139);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let start = position_of(&source_text, "$count");
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 0 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Translation file has no origin-language counterpart for `app`"
            },
            {
                "range": {
                    "start": { "line": start.0, "character": start.1 },
                    "end": { "line": end.0, "character": end.1 + 4 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Numeric selector for `ar` is missing category `few`"
            },
            {
                "range": {
                    "start": { "line": start.0, "character": start.1 },
                    "end": { "line": end.0, "character": end.1 + 4 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Numeric selector for `ar` is missing category `many`"
            },
            {
                "range": {
                    "start": { "line": start.0, "character": start.1 },
                    "end": { "line": end.0, "character": end.1 + 4 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Numeric selector for `ar` is missing category `two`"
            }
        ])
    );
}

#[test]
fn diagnostics_use_translation_locale_categories_not_origin_categories() {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
    std::fs::create_dir_all(temp.path().join("locales/ar")).unwrap();
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        r#"rollout =
    { $count ->
        [one] one package
       *[other] packages
    }
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/ar/app.ftl"),
        r#"rollout =
    { $count ->
        [zero] صفر
        [one] واحد
       *[other] آخر
    }
"#,
    )
    .unwrap();
    let source_path = temp.path().join("locales/ar/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 140);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let diagnostics = recv_notification(
        &mut lsp,
        "textDocument/publishDiagnostics",
    )["params"]["diagnostics"]
        .as_array()
        .cloned()
        .expect("expected diagnostics array");
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["message"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        vec![
            Value::String(
                "Numeric selector for `ar` is missing category `few`"
                    .to_string()
            ),
            Value::String(
                "Numeric selector for `ar` is missing category `many`"
                    .to_string()
            ),
            Value::String(
                "Numeric selector for `ar` is missing category `two`"
                    .to_string()
            ),
        ]
    );
}

#[test]
fn diagnostics_plural_categories_absent_until_enabled_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"bad-zero =
    { $count ->
        [zero] none
       *[other] many
    }
"#;

    let mut lsp = initialized_lsp(&root, 141);
    send_open_document(&mut lsp, &source_path, source_text);
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        Value::Array(Vec::new())
    );

    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "error_on_unsupported_plural_categories": true,
                "warn_on_missing_plural_categories": true
            }
        }),
    );
    send_save_document(&mut lsp, &source_path, Some(source_text));

    let enabled_notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    let mut diagnostics = enabled_notification["params"]["diagnostics"]
        .as_array()
        .cloned()
        .expect("expected diagnostics array");
    diagnostics.sort_by_key(|diagnostic| {
        diagnostic["message"].as_str().unwrap().to_string()
    });
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic["message"].clone())
            .collect::<Vec<_>>(),
        vec![
            Value::String(
                "Numeric selector for `en` is missing category `one`"
                    .to_string()
            ),
            Value::String(
                "`zero` is not a supported plural category for `en`"
                    .to_string()
            ),
        ]
    );
}

#[test]
fn diagnostics_report_translation_whole_style_mismatch_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/es/app.ftl");
    let source_text = r#"whole-style =
    { $count ->
        [one] un paquete
       *[other] paquetes
    }
"#;

    let mut lsp = initialized_lsp(&root, 142);
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

    let start_character = source_text.lines().next().unwrap().chars().count();
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": 0, "character": start_character },
                    "end": { "line": end.0, "character": end.1 + 5 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Selector style is `whole`, but workspace prefers `prefix`"
            }
        ])
    );
}

#[test]
fn diagnostics_report_translation_suffix_style_mismatch_exactly() {
    let temp = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"suffix-style = You have { $count ->
        [one] { $count } package
       *[other] { $count } packages
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"suffix-style = Tienes { $count ->
        [one] { $count } paquete
       *[other] { $count } paquetes
    }
"#,
        ),
    ]);
    let source_path = temp.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 143);
    change_configuration(
        &mut lsp,
        json!({
            "fluent-lsp": {
                "warn_on_selector_style_mismatch": true,
                "selector_style": "whole"
            }
        }),
    );
    send_open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let start_character = position_of(&source_text, "Tienes").1;
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": 0, "character": start_character },
                    "end": { "line": end.0, "character": end.1 + 5 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Selector style is `suffix`, but workspace prefers `whole`"
            }
        ])
    );
}

#[test]
fn diagnostics_report_local_origin_selector_style_mismatch_exactly() {
    let root = fixture_root();
    let source_path = root.join("locales/en/app.ftl");
    let source_text = r#"local-whole =
    { $count ->
        [one] one package
       *[other] packages
    }
"#;

    let mut lsp = initialized_lsp(&root, 144);
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

    let start_character = source_text.lines().next().unwrap().chars().count();
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": 0, "character": start_character },
                    "end": { "line": end.0, "character": end.1 + 5 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Selector style is `whole`, but workspace prefers `prefix`"
            }
        ])
    );
}

#[test]
fn file_config_selector_style_diagnostics_follow_file_config_exactly() {
    let temp = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"suffix-style = You have { $count ->
        [one] { $count } package
       *[other] { $count } packages
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"suffix-style = Tienes { $count ->
        [one] { $count } paquete
       *[other] { $count } paquetes
    }
"#,
        ),
    ]);
    std::fs::write(
        temp.path().join("fluent-lsp.toml"),
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
selector_style = "whole"
warn_on_selector_style_mismatch = true
"#,
    )
    .unwrap();
    let source_path = temp.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(temp.path(), 145);
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

    let start_character = position_of(&source_text, "Tienes").1;
    let end = position_of(&source_text, "    }");
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": 0, "character": start_character },
                    "end": { "line": end.0, "character": end.1 + 5 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Selector style is `suffix`, but workspace prefers `whole`"
            }
        ])
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

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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
    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
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

    let position = position_of_nth(&source_text, "$coins", 2);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 90,
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
    let actions = recv_response(&mut lsp, 90)["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                == Value::String("Generate number selector from $coins (prefix)".to_string())
        })
        .expect("missing code action: Generate number selector from $coins (prefix)");
    assert_eq!(
        action["edit"]["changes"][format!("file://{}", source_path.display())]
            [0]["newText"],
        Value::String(
            r#"Ella tiene { $coins } { $coins ->
            [one] monedas.
            *[other] monedas.
        }"#
            .to_string()
        )
    );
}

#[test]
fn goto_definition_attribute_position_matrix_and_exact_outcomes() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"top-entry =
    .alpha = Origin top alpha
    .beta = Origin top beta
    .omega = Origin top omega
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"top-entry =
    .alpha = Local top alpha
    .beta = Local top beta
    .omega = Local top omega
"#,
        ),
        (
            "locales/en/dialogs/menu.ftl",
            r#"nested-entry =
    .first = Origin nested first
    .last = Origin nested last
"#,
        ),
        (
            "locales/es/dialogs/menu.ftl",
            r#"nested-entry =
    .first = Local nested first
    .last = Local nested last
"#,
        ),
    ]);
    let top_path = workspace.path().join("locales/es/app.ftl");
    let top_origin_path = workspace.path().join("locales/en/app.ftl");
    let top_text = std::fs::read_to_string(&top_path).unwrap();
    let top_origin_text = std::fs::read_to_string(&top_origin_path).unwrap();
    let nested_path = workspace.path().join("locales/es/dialogs/menu.ftl");
    let nested_origin_path =
        workspace.path().join("locales/en/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();
    let nested_origin_text =
        std::fs::read_to_string(&nested_origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_300);
    open_document(&mut lsp, &top_path, &top_text);
    open_document(&mut lsp, &nested_path, &nested_text);

    let alpha_dot = position_of(&top_text, ".alpha =");
    let expected_alpha_start = position_of(&top_origin_text, "alpha =");
    let expected_alpha = json!({
        "uri": format!("file://{}", top_origin_path.display()),
        "range": {
            "start": {
                "line": expected_alpha_start.0,
                "character": expected_alpha_start.1
            },
            "end": {
                "line": expected_alpha_start.0,
                "character": expected_alpha_start.1 + 5
            }
        }
    });

    for (request_id, character) in [
        (6_301, alpha_dot.1),
        (6_302, alpha_dot.1 + 1),
        (6_303, alpha_dot.1 + 5),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": format!("file://{}", top_path.display()) },
                "position": { "line": alpha_dot.0, "character": character }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], expected_alpha);
    }

    let omega_position = position_of(&top_text, ".omega =");
    let omega_expected_start = position_of(&top_origin_text, "omega =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_304,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", top_path.display()) },
            "position": {
                "line": omega_position.0,
                "character": omega_position.1 + 1
            }
        }
    }));
    let omega_response = recv_response(&mut lsp, 6_304);
    assert_eq!(
        omega_response["result"],
        json!({
            "uri": format!("file://{}", top_origin_path.display()),
            "range": {
                "start": {
                    "line": omega_expected_start.0,
                    "character": omega_expected_start.1
                },
                "end": {
                    "line": omega_expected_start.0,
                    "character": omega_expected_start.1 + 5
                }
            }
        })
    );

    let nested_first_position = position_of(&nested_text, ".first =");
    let nested_first_expected = position_of(&nested_origin_text, "first =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_305,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_path.display()) },
            "position": {
                "line": nested_first_position.0,
                "character": nested_first_position.1 + 1
            }
        }
    }));
    let nested_first_response = recv_response(&mut lsp, 6_305);
    assert_eq!(
        nested_first_response["result"],
        json!({
            "uri": format!("file://{}", nested_origin_path.display()),
            "range": {
                "start": {
                    "line": nested_first_expected.0,
                    "character": nested_first_expected.1
                },
                "end": {
                    "line": nested_first_expected.0,
                    "character": nested_first_expected.1 + 5
                }
            }
        })
    );

    let nested_last_position = position_of(&nested_text, ".last =");
    let nested_last_expected = position_of(&nested_origin_text, "last =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_306,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_path.display()) },
            "position": {
                "line": nested_last_position.0,
                "character": nested_last_position.1 + 4
            }
        }
    }));
    let nested_last_response = recv_response(&mut lsp, 6_306);
    assert_eq!(
        nested_last_response["result"],
        json!({
            "uri": format!("file://{}", nested_origin_path.display()),
            "range": {
                "start": {
                    "line": nested_last_expected.0,
                    "character": nested_last_expected.1
                },
                "end": {
                    "line": nested_last_expected.0,
                    "character": nested_last_expected.1 + 4
                }
            }
        })
    );

    let beta_position = position_of(&top_text, ".beta =");
    let beta_expected = position_of(&top_origin_text, "beta =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_307,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", top_path.display()) },
            "position": {
                "line": beta_position.0,
                "character": beta_position.1 + 2
            }
        }
    }));
    let beta_response = recv_response(&mut lsp, 6_307);
    assert_eq!(
        beta_response["result"],
        json!({
            "uri": format!("file://{}", top_origin_path.display()),
            "range": {
                "start": {
                    "line": beta_expected.0,
                    "character": beta_expected.1
                },
                "end": {
                    "line": beta_expected.0,
                    "character": beta_expected.1 + 4
                }
            }
        })
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_308,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", top_path.display()) },
            "position": { "line": alpha_dot.0, "character": 3 }
        }
    }));
    let indentation_response = recv_response(&mut lsp, 6_308);
    assert_eq!(indentation_response["result"], Value::Null);

    let alpha_equal = position_of(&top_text, "alpha =");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_309,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": format!("file://{}", top_path.display()) },
            "position": {
                "line": alpha_equal.0,
                "character": alpha_equal.1 + 6
            }
        }
    }));
    let equal_response = recv_response(&mut lsp, 6_309);
    assert_eq!(equal_response["result"], Value::Null);
}

#[test]
fn hover_key_and_attribute_lines_do_not_fall_back_to_body_preview() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"# Origin key comment
commented-key = Origin body

menu =
    .tooltip = Origin attribute body
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"# Comentario local
commented-key = Cuerpo local

menu =
    .tooltip = Cuerpo de atributo local
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_140);
    open_document(&mut lsp, &source_path, &source_text);

    let key_equal = position_of(&source_text, "commented-key =");
    for (request_id, character) in
        [(6_141, key_equal.1 + 13), (6_142, key_equal.1 + 14)]
    {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", source_path.display()) },
                "position": { "line": key_equal.0, "character": character }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], Value::Null);
    }

    let attribute_line = position_of(&source_text, ".tooltip =");
    for (request_id, character) in [(6_143, 0), (6_144, attribute_line.1 + 9)] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", source_path.display()) },
                "position": { "line": attribute_line.0, "character": character }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"], Value::Null);
    }
}

#[test]
fn hover_body_and_attribute_previews_stay_exact_across_positions_and_preserve_unresolved_terms()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            "body-preview = Uno dos tres\nwith-term = Start { -missing-term } end\nattr-preview =\n    .tooltip = Four five six\nattr-with-term =\n    .tooltip = Open { -missing-term } now",
        ),
        (
            "locales/es/app.ftl",
            "body-preview = Uno dos tres\nwith-term = Inicio { -missing-term } fin\nattr-preview =\n    .tooltip = Cuatro cinco seis\nattr-with-term =\n    .tooltip = Abre { -missing-term } ahora",
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_145);
    open_document(&mut lsp, &source_path, &source_text);

    let body_expected = Value::String(format!(
        r#"```ftl
{}
```

---

```ftl
{}
```"#,
        preview_message_text_with_overrides(&origin_text, "body-preview", &[]),
        preview_message_text_with_overrides(&source_text, "body-preview", &[]),
    ));
    let body_line = position_of(&source_text, "Uno dos tres");
    let body_positions = [
        body_line,
        position_of(&source_text, "dos"),
        position_of(&source_text, "tres"),
        (body_line.0, body_line.1 + 3),
    ];
    for (offset, position) in body_positions.into_iter().enumerate() {
        let request_id = 6_146 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", source_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"]["contents"]["value"], body_expected);
    }

    let term_position = position_of(&source_text, "{ -missing-term }");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_150,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": term_position.0, "character": term_position.1 }
        }
    }));
    let term_hover = recv_response(&mut lsp, 6_150);
    assert_eq!(
        term_hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"```ftl
{}
```

---

```ftl
{}
```"#,
            preview_message_text_with_overrides(&origin_text, "with-term", &[]),
            preview_message_text_with_overrides(&source_text, "with-term", &[]),
        ))
    );

    let attribute_expected = Value::String(format!(
        r#"```ftl
{}
```

---

```ftl
{}
```"#,
        preview_message_text_with_overrides(
            &origin_text,
            "attr-preview.tooltip",
            &[],
        ),
        preview_message_text_with_overrides(
            &source_text,
            "attr-preview.tooltip",
            &[],
        ),
    ));
    let attribute_line = position_of(&source_text, "Cuatro cinco seis");
    let attribute_positions = [
        attribute_line,
        position_of(&source_text, "cinco"),
        position_of(&source_text, "seis"),
        (attribute_line.0, attribute_line.1 + 6),
    ];
    for (offset, position) in attribute_positions.into_iter().enumerate() {
        let request_id = 6_151 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", source_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"]["contents"]["value"], attribute_expected);
    }

    let attribute_term_line =
        position_of(&source_text, "Abre { -missing-term } ahora");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_155,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": {
                "line": attribute_term_line.0,
                "character": attribute_term_line.1 + 5
            }
        }
    }));
    let attribute_term_hover = recv_response(&mut lsp, 6_155);
    assert_eq!(
        attribute_term_hover["result"]["contents"]["value"],
        Value::String(format!(
            r#"```ftl
{}
```

---

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "attr-with-term.tooltip",
                &[],
            ),
            preview_message_text_with_overrides(
                &source_text,
                "attr-with-term.tooltip",
                &[],
            ),
        ))
    );
}

#[test]
fn hover_selector_previews_are_stable_across_header_and_branch_positions() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"selector-preview =
    { $count ->
        [one] one branch
       *[other] other branch
    }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"selector-preview =
    { $count ->
        [one] una rama
       *[other] otras ramas
    }
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_156);
    open_document(&mut lsp, &source_path, &source_text);

    let expected_default = Value::String(format!(
        r#"`$count=*`

```ftl
{}
```

---

`$count=*`

```ftl
{}
```"#,
        preview_message_text_with_overrides(
            &origin_text,
            "selector-preview",
            &[],
        ),
        preview_message_text_with_overrides(
            &source_text,
            "selector-preview",
            &[],
        ),
    ));
    let expected_one = Value::String(format!(
        r#"`$count=one`

```ftl
{}
```

---

`$count=one`

```ftl
{}
```"#,
        preview_message_text_with_overrides(
            &origin_text,
            "selector-preview",
            &[("$count", "one")],
        ),
        preview_message_text_with_overrides(
            &source_text,
            "selector-preview",
            &[("$count", "one")],
        ),
    ));
    let header_start = position_of(&source_text, "{ $count ->");
    let branch_start = position_of(&source_text, "una rama");
    let header_positions = [
        header_start,
        position_of(&source_text, "$count"),
        (header_start.0, header_start.1 + 1),
    ];
    for (offset, position) in header_positions.into_iter().enumerate() {
        let request_id = 6_157 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", source_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"]["contents"]["value"], expected_default);
    }

    let one_positions = [
        branch_start,
        position_of(&source_text, "rama"),
        (branch_start.0, branch_start.1 + 3),
    ];
    for (offset, position) in one_positions.into_iter().enumerate() {
        let request_id = 6_160 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": format!("file://{}", source_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(response["result"]["contents"]["value"], expected_one);
    }

    let other_position = position_of(&source_text, "otras ramas");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_163,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": format!("file://{}", source_path.display()) },
            "position": { "line": other_position.0, "character": other_position.1 }
        }
    }));
    let other_response = recv_response(&mut lsp, 6_163);
    assert_eq!(
        other_response["result"]["contents"]["value"],
        Value::String(format!(
            r#"`$count=other`

```ftl
{}
```

---

`$count=other`

```ftl
{}
```"#,
            preview_message_text_with_overrides(
                &origin_text,
                "selector-preview",
                &[("$count", "other")],
            ),
            preview_message_text_with_overrides(
                &source_text,
                "selector-preview",
                &[("$count", "other")],
            ),
        ))
    );
}

#[test]
fn completion_top_level_and_attribute_position_matrices_are_exact() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"alpha-first = First
alpha-second = Second
beta-last = Last
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"### File note
## Group note
present = Presente

a"#,
        ),
        (
            "locales/en/dialogs/menu.ftl",
            r#"menu-entry =
    .alpha = Alpha
    .beta = Beta
    .tooltip = Tooltip
"#,
        ),
        (
            "locales/es/dialogs/menu.ftl",
            r#"menu-entry =
    # comment inside message
    .alpha = Alfa
    .
"#,
        ),
    ]);
    let app_path = workspace.path().join("locales/es/app.ftl");
    let app_text = std::fs::read_to_string(&app_path).unwrap();
    let nested_path = workspace.path().join("locales/es/dialogs/menu.ftl");
    let nested_text = std::fs::read_to_string(&nested_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_164);
    open_document(&mut lsp, &app_path, &app_text);
    open_document(&mut lsp, &nested_path, &nested_text);

    for (request_id, position) in [
        (6_165, position_after(&app_text, "a")),
        (6_166, position_of(&app_text, "a")),
    ] {
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": format!("file://{}", app_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let completion = recv_response(&mut lsp, request_id);
        let labels = if completion["result"].is_array() {
            completion["result"]
                .as_array()
                .expect("expected completion items")
        } else {
            completion["result"]["items"]
                .as_array()
                .expect("expected completion items")
        }
        .iter()
        .map(|item| item["label"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec!["alpha-first".to_string(), "alpha-second".to_string()]
        );
    }

    let last_text = format!("{app_text}\nb");
    send_change_document(&mut lsp, &app_path, 2, &last_text);
    let last_position = position_after(&last_text, "b");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_167,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": last_position.0, "character": last_position.1 }
        }
    }));
    let last_completion = recv_response(&mut lsp, 6_167);
    let last_labels = if last_completion["result"].is_array() {
        last_completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        last_completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(last_labels, vec!["beta-last".to_string()]);

    for (offset, position) in [
        position_after(&nested_text, "."),
        position_of(&nested_text, "."),
    ]
    .into_iter()
    .enumerate()
    {
        let request_id = 6_168 + i64::try_from(offset).unwrap();
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": format!("file://{}", nested_path.display()) },
                "position": { "line": position.0, "character": position.1 }
            }
        }));
        let completion = recv_response(&mut lsp, request_id);
        let labels = if completion["result"].is_array() {
            completion["result"]
                .as_array()
                .expect("expected completion items")
        } else {
            completion["result"]["items"]
                .as_array()
                .expect("expected completion items")
        }
        .iter()
        .map(|item| item["label"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
        assert_eq!(labels, vec![".beta".to_string(), ".tooltip".to_string()]);
    }

    let nested_prefix_text = r#"menu-entry =
    # comment inside message
    .alpha = Alfa
    .t
"#;
    send_change_document(&mut lsp, &nested_path, 2, nested_prefix_text);
    let nested_prefix_position = position_after(nested_prefix_text, ".t");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_170,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_path.display()) },
            "position": {
                "line": nested_prefix_position.0,
                "character": nested_prefix_position.1
            }
        }
    }));
    let nested_prefix_completion = recv_response(&mut lsp, 6_170);
    let nested_prefix_labels = if nested_prefix_completion["result"].is_array()
    {
        nested_prefix_completion["result"]
            .as_array()
            .expect("expected completion items")
    } else {
        nested_prefix_completion["result"]["items"]
            .as_array()
            .expect("expected completion items")
    }
    .iter()
    .map(|item| item["label"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
    assert_eq!(nested_prefix_labels, vec![".tooltip".to_string()]);
}

#[test]
fn completion_documentation_without_comments_and_unresolved_references_is_exact()
 {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"plain-doc = See { -missing-term } and { missing-copy } now
entry =
    .plain = Save { -missing-term } and { missing-copy }
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"pl

entry =
    .p
"#,
        ),
    ]);
    let app_path = workspace.path().join("locales/es/app.ftl");
    let app_text = std::fs::read_to_string(&app_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_171);
    open_document(&mut lsp, &app_path, &app_text);

    let message_position = position_after(&app_text, "pl");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_172,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": message_position.0, "character": message_position.1 }
        }
    }));
    let message_completion = recv_response(&mut lsp, 6_172);
    let message_item = if message_completion["result"].is_array() {
        message_completion["result"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    } else {
        message_completion["result"]["items"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    }
    .into_iter()
    .find(|item| item["label"] == Value::String("plain-doc".to_string()))
    .expect("missing plain-doc completion item");
    assert_eq!(
        message_item["documentation"]["kind"],
        Value::String("markdown".to_string())
    );
    assert_eq!(
        message_item["documentation"]["value"],
        Value::String(
            r#"```ftl
plain-doc = See { -missing-term } and { missing-copy } now
```"#
                .to_string()
        )
    );

    let attribute_position = position_after(&app_text, ".p");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_173,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", app_path.display()) },
            "position": { "line": attribute_position.0, "character": attribute_position.1 }
        }
    }));
    let attribute_completion = recv_response(&mut lsp, 6_173);
    let attribute_item = if attribute_completion["result"].is_array() {
        attribute_completion["result"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    } else {
        attribute_completion["result"]["items"]
            .as_array()
            .cloned()
            .expect("expected completion items")
    }
    .into_iter()
    .find(|item| item["label"] == Value::String(".plain".to_string()))
    .expect("missing .plain completion item");
    assert_eq!(
        attribute_item["documentation"]["kind"],
        Value::String("markdown".to_string())
    );
    assert_eq!(
        attribute_item["documentation"]["value"],
        Value::String(
            r#"```ftl
.plain = Save { -missing-term } and { missing-copy }
```"#
                .to_string()
        )
    );
}

#[test]
fn completion_returns_exact_empty_payloads_for_remaining_matrix_cases() {
    let workspace = completion_workspace();
    let translation_path = workspace.path().join("locales/es/app.ftl");
    let origin_path = workspace.path().join("locales/en/app.ftl");
    let nested_path = workspace.path().join("locales/es/dialogs/menu.ftl");

    let mut lsp = initialized_lsp(workspace.path(), 6_174);

    let blank_text = r#"welcome-title = Bienvenido

   
"#;
    send_open_document(&mut lsp, &translation_path, blank_text);
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_175,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": { "line": 2, "character": 2 }
        }
    }));
    assert_eq!(
        recv_response(&mut lsp, 6_175)["result"],
        Value::Array(Vec::new())
    );

    let complete_key_text = r#"welcome-title = Bienvenido

download-action = Descargar
"#;
    send_change_document(&mut lsp, &translation_path, 2, complete_key_text);
    let complete_key_position =
        position_of(complete_key_text, "download-action");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_176,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", translation_path.display()) },
            "position": {
                "line": complete_key_position.0,
                "character": complete_key_position.1 + 15
            }
        }
    }));
    assert_eq!(
        recv_response(&mut lsp, 6_176)["result"],
        Value::Array(Vec::new())
    );

    let complete_attribute_text = r#"menu-save =
    .label = Guardar
"#;
    send_open_document(&mut lsp, &nested_path, complete_attribute_text);
    let complete_attribute_position =
        position_of(complete_attribute_text, ".label");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_177,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", nested_path.display()) },
            "position": {
                "line": complete_attribute_position.0,
                "character": complete_attribute_position.1 + 6
            }
        }
    }));
    assert_eq!(
        recv_response(&mut lsp, 6_177)["result"],
        Value::Array(Vec::new())
    );

    let origin_text = std::fs::read_to_string(&origin_path).unwrap();
    open_document(&mut lsp, &origin_path, &origin_text);
    let origin_position = position_of(&origin_text, "download-action");
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6_178,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": format!("file://{}", origin_path.display()) },
            "position": {
                "line": origin_position.0,
                "character": origin_position.1 + 4
            }
        }
    }));
    assert_eq!(
        recv_response(&mut lsp, 6_178)["result"],
        Value::Array(Vec::new())
    );
}

#[test]
fn code_lens_uses_current_document_uri_for_nested_translation_and_origin_files()
{
    let workspace = temp_workspace(&[
        (
            "locales/en/dialogs/menu.ftl",
            r#"menu-status =
    { $count ->
        [one] one item
       *[other] many items
    }
"#,
        ),
        (
            "locales/es/dialogs/menu.ftl",
            r#"menu-status =
    { $count ->
        [one] un elemento
       *[other] muchos elementos
    }
"#,
        ),
    ]);
    let translation_path = workspace.path().join("locales/es/dialogs/menu.ftl");
    let origin_path = workspace.path().join("locales/en/dialogs/menu.ftl");
    let translation_text = std::fs::read_to_string(&translation_path).unwrap();
    let origin_text = std::fs::read_to_string(&origin_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_179);
    open_document(&mut lsp, &translation_path, &translation_text);
    open_document(&mut lsp, &origin_path, &origin_text);

    for (request_id, path) in [
        (6_180, translation_path.as_path()),
        (6_181, origin_path.as_path()),
    ] {
        let uri = format!("file://{}", path.display());
        lsp.send(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/codeLens",
            "params": {
                "textDocument": { "uri": uri.clone() }
            }
        }));
        let response = recv_response(&mut lsp, request_id);
        assert_eq!(
            response["result"],
            json!([
                {
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 11 }
                    },
                    "command": {
                        "title": "Show all 2 selector combinations",
                        "command": "fluent-lsp.showSelectorCombinations",
                        "arguments": [uri, "menu-status"]
                    }
                }
            ])
        );
    }
}

#[test]
fn translation_only_term_warning_contract_is_exact() {
    let workspace = temp_workspace(&[
        (
            "locales/en/app.ftl",
            r#"shared = Shared
"#,
        ),
        (
            "locales/es/app.ftl",
            r#"-local-term = Termino local
"#,
        ),
    ]);
    let source_path = workspace.path().join("locales/es/app.ftl");
    let source_text = std::fs::read_to_string(&source_path).unwrap();

    let mut lsp = initialized_lsp(workspace.path(), 6_182);
    open_document(&mut lsp, &source_path, &source_text);
    send_save_document(&mut lsp, &source_path, Some(&source_text));

    let notification =
        recv_notification(&mut lsp, "textDocument/publishDiagnostics");
    assert_eq!(
        notification["params"]["diagnostics"],
        json!([
            {
                "range": {
                    "start": { "line": 0, "character": 1 },
                    "end": { "line": 0, "character": 11 }
                },
                "severity": 2,
                "source": "fluent-lsp",
                "message": "Translation entry `-local-term` has no origin-language counterpart"
            }
        ])
    );
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

fn send_change_document(
    lsp: &mut LspProcess,
    path: &Path,
    version: i32,
    text: &str,
) {
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
        let notification =
            recv_notification(lsp, "textDocument/publishDiagnostics");
        if notification["params"]["uri"]
            == Value::String(format!("file://{}", path.display()))
        {
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

fn recv_notifications_for_methods(
    lsp: &mut LspProcess,
    methods: &[&str],
) -> HashMap<String, Value> {
    let wanted = methods
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut found = HashMap::new();
    while found.len() < wanted.len() {
        let message = lsp.recv();
        let Some(method) = message["method"].as_str() else {
            continue;
        };
        if wanted.contains(method) && !found.contains_key(method) {
            found.insert(method.to_string(), message);
        }
    }
    found
}

fn recv_response_and_notification(
    lsp: &mut LspProcess,
    request_id: i64,
    method: &str,
) -> (Value, Value) {
    let mut response = None;
    let mut notification = None;
    while response.is_none() || notification.is_none() {
        let message = lsp.recv();
        if notification.is_none()
            && message["method"] == Value::String(method.to_string())
        {
            notification = Some(message);
            continue;
        }
        if response.is_none() && message["id"] == Value::from(request_id) {
            response = Some(message);
        }
    }
    (
        response.expect("missing response"),
        notification.expect("missing notification"),
    )
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
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

fn preview_message_text_with_overrides(
    source: &str,
    key: &str,
    overrides: &[(&str, &str)],
) -> String {
    let override_map = overrides
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect::<HashMap<_, _>>();
    render_fluent_preview_text(source, key, Some(&override_map))
        .unwrap_or_else(|| panic!("missing preview for `{key}`"))
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        r#"hello-world = Hello
download-action = Download
download-count = Download count

# Completion doc coverage
# Keep this note in completion hover
commented-preview = Preview text for completion docs.
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/dialogs/menu.ftl"),
        r#"# Menu completion documentation
# Keep this entry visible in completion hover
menu-save =
    .label = Save
    .tooltip = Save this file
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        r#"welcome-title = Bienvenido
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/dialogs/menu.ftl"),
        r#"menu-save =
    .label = Guardar
"#,
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        r#"hello = Hello World

menu-save =
    .label = Save
    .tooltip = Save this file

sync-status = Sync ready
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        r#"hello = Hola Mundo
menu-save =
    .label = Guardar
"#,
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        r#"hello = Hello World

download-action =
    .label = Download
    .tooltip = Download this build
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        r#"# [LSP-COPY]
hello = Hola Mundo

# [LSP-COPY .tooltip]
download-action =
    .label = Descargar
    .tooltip = Download this build
"#,
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
        r#"origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/en/app.ftl"),
        r#"hello = Hello World
download-action =
    .label = Download
    .tooltip = Download this build

sync-status = Sync ready
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("locales/es/app.ftl"),
        r#"hello = { "" }
download-action =
    .label = Descargar

sync-status = { "" }
"#,
    )
    .unwrap();
    temp
}

struct Editor {
    source: String,
}

impl Editor {
    fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
        }
    }

    fn apply_code_action(mut self, action: &Value, target_uri: &str) -> Self {
        let mut edits = action["edit"]["changes"][target_uri]
            .as_array()
            .cloned()
            .expect("expected workspace edit changes");
        edits.sort_by_key(|text_edit| {
            (
                std::cmp::Reverse(
                    text_edit["range"]["start"]["line"].as_u64().unwrap(),
                ),
                std::cmp::Reverse(
                    text_edit["range"]["start"]["character"].as_u64().unwrap(),
                ),
                std::cmp::Reverse(
                    text_edit["range"]["end"]["line"].as_u64().unwrap(),
                ),
                std::cmp::Reverse(
                    text_edit["range"]["end"]["character"].as_u64().unwrap(),
                ),
            )
        });

        for text_edit in edits {
            let start = position_to_offset(
                &self.source,
                (
                    text_edit["range"]["start"]["line"].as_u64().unwrap()
                        as u32,
                    text_edit["range"]["start"]["character"].as_u64().unwrap()
                        as u32,
                ),
            );
            let end = position_to_offset(
                &self.source,
                (
                    text_edit["range"]["end"]["line"].as_u64().unwrap() as u32,
                    text_edit["range"]["end"]["character"].as_u64().unwrap()
                        as u32,
                ),
            );
            self.source.replace_range(
                start..end,
                text_edit["newText"].as_str().unwrap(),
            );
        }

        self
    }
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
    let (line, character) = position_of_nth(source, needle, 1);
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
    let line =
        u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count())
            .unwrap();
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let character =
        u32::try_from(source[line_start..offset].chars().count()).unwrap();
    (line, character)
}
