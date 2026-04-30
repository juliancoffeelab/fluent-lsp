use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, InitializeParams, InitializeResult, Location, MessageType, OneOf,
    Position, Range, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

const CONFIG_FILE_NAMES: [&str; 2] = ["fluent-lsp.toml", ".fluent-lsp.toml"];

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub english_file: PathBuf,
    #[serde(default)]
    pub file_masks: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    root_dir: PathBuf,
    english_file: PathBuf,
    file_masks: Vec<String>,
    matcher: Option<GlobSet>,
}

impl WorkspaceConfig {
    pub fn load(root_dir: PathBuf) -> Result<Self> {
        let config_path = CONFIG_FILE_NAMES
            .iter()
            .map(|name| root_dir.join(name))
            .find(|path| path.is_file())
            .with_context(|| {
                format!(
                    "missing config file; tried {}",
                    CONFIG_FILE_NAMES
                        .iter()
                        .map(|name| root_dir.join(name).display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        let raw = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let config: Config = toml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        let english_file = root_dir.join(&config.english_file);

        let matcher = compile_globs(&config.file_masks)?;

        Ok(Self {
            root_dir,
            english_file,
            file_masks: config.file_masks,
            matcher,
        })
    }

    pub fn english_file(&self) -> &Path {
        &self.english_file
    }

    pub fn matches_reference_file(&self, path: &Path) -> bool {
        if path == self.english_file {
            return true;
        }

        match &self.matcher {
            Some(matcher) => {
                let relative = path.strip_prefix(&self.root_dir).unwrap_or(path);
                matcher.is_match(relative)
            }
            None => true,
        }
    }

    pub fn describe_masks(&self) -> String {
        if self.file_masks.is_empty() {
            "*".to_string()
        } else {
            self.file_masks.join(", ")
        }
    }
}

fn compile_globs(globs: &[String]) -> Result<Option<GlobSet>> {
    if globs.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        builder.add(Glob::new(glob).with_context(|| format!("invalid glob: {glob}"))?);
    }

    Ok(Some(builder.build()?))
}

#[derive(Default)]
struct ServerState {
    root_dir: Option<PathBuf>,
    workspace: Option<WorkspaceConfig>,
    open_documents: HashMap<Url, String>,
}

pub struct Backend {
    client: Client,
    state: Arc<RwLock<ServerState>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(ServerState::default())),
        }
    }

    async fn read_document_text(&self, uri: &Url) -> Option<String> {
        if let Some(text) = self.state.read().await.open_documents.get(uri).cloned() {
            return Some(text);
        }

        let path = uri.to_file_path().ok()?;
        tokio::fs::read_to_string(path).await.ok()
    }

    async fn definition_for(&self, params: GotoDefinitionParams) -> Option<Location> {
        let state = self.state.read().await;
        let workspace = state.workspace.clone()?;
        let uri = params.text_document_position_params.text_document.uri;
        let path = uri.to_file_path().ok()?;
        if !workspace.matches_reference_file(&path) {
            return None;
        }

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())?;
        drop(state);

        let key = extract_key_at_position(&source, params.text_document_position_params.position)?;
        let english_uri = Url::from_file_path(workspace.english_file()).ok()?;
        let english_source = if english_uri == uri {
            source
        } else {
            self.read_document_text(&english_uri).await?
        };

        let definition = find_fluent_message(&english_source, &key)?;
        Some(Location {
            uri: english_uri,
            range: definition,
        })
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let root_dir = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok())
            .or_else(|| {
                params.workspace_folders.as_ref().and_then(|folders| {
                    folders
                        .first()
                        .and_then(|folder| folder.uri.to_file_path().ok())
                })
            });

        let workspace = root_dir
            .as_ref()
            .and_then(|root| WorkspaceConfig::load(root.clone()).ok());

        {
            let mut state = self.state.write().await;
            state.root_dir = root_dir;
            state.workspace = workspace;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                definition_provider: Some(OneOf::Left(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: tower_lsp::lsp_types::InitializedParams) {
        let state = self.state.read().await;
        match &state.workspace {
            Some(workspace) => {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!(
                            "loaded {} with masks [{}]",
                            workspace.english_file().display(),
                            workspace.describe_masks()
                        ),
                    )
                    .await;
            }
            None => {
                let root = state
                    .root_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<unknown root>".to_string());
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("no fluent-lsp config found under {root}"),
                    )
                    .await;
            }
        }
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.state
            .write()
            .await
            .open_documents
            .insert(params.text_document.uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.state
                .write()
                .await
                .open_documents
                .insert(params.text_document.uri, change.text);
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        Ok(self
            .definition_for(params)
            .await
            .map(GotoDefinitionResponse::Scalar))
    }
}

pub fn extract_key_at_position(source: &str, position: Position) -> Option<String> {
    let (line, line_start) = line_at(source, position.line as usize)?;
    let line_offset = utf16_position_to_byte_index(line, position.character as usize)?;

    let mut probe = line_offset;
    if !is_key_byte(line.as_bytes().get(probe).copied()) && probe > 0 {
        probe -= 1;
    }
    if !is_key_byte(line.as_bytes().get(probe).copied()) {
        return None;
    }

    let bytes = line.as_bytes();
    let mut start = probe;
    while start > 0 && is_key_byte(bytes.get(start - 1).copied()) {
        start -= 1;
    }

    let mut end = probe;
    while end + 1 < bytes.len() && is_key_byte(bytes.get(end + 1).copied()) {
        end += 1;
    }

    let token = &line[start..=end];
    if token.bytes().any(|byte| byte.is_ascii_alphabetic()) {
        Some(source[line_start + start..line_start + end + 1].to_string())
    } else {
        None
    }
}

pub fn find_fluent_message(source: &str, key: &str) -> Option<Range> {
    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('.') || trimmed.is_empty() {
            continue;
        }

        let Some(equals) = trimmed.find('=') else {
            continue;
        };
        let candidate = trimmed[..equals].trim_end();
        if candidate == key {
            let leading = line.len() - trimmed.len();
            return Some(Range {
                start: Position::new(line_idx as u32, leading as u32),
                end: Position::new(line_idx as u32, (leading + candidate.len()) as u32),
            });
        }
    }

    None
}

fn line_at(source: &str, line_index: usize) -> Option<(&str, usize)> {
    let mut start = 0;
    for (idx, line) in source.split('\n').enumerate() {
        if idx == line_index {
            return Some((line, start));
        }
        start += line.len() + 1;
    }

    None
}

fn utf16_position_to_byte_index(line: &str, character: usize) -> Option<usize> {
    let mut utf16_seen = 0;
    for (byte_idx, ch) in line.char_indices() {
        if utf16_seen == character {
            return Some(byte_idx);
        }
        utf16_seen += ch.len_utf16();
        if utf16_seen > character {
            return Some(byte_idx);
        }
    }

    if utf16_seen == character {
        Some(line.len())
    } else {
        None
    }
}

fn is_key_byte(byte: Option<u8>) -> bool {
    matches!(
        byte,
        Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'.')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_key_inside_quotes() {
        let source = "const title = t(\"welcome-title\");\n";
        let key = extract_key_at_position(source, Position::new(0, 19)).unwrap();
        assert_eq!(key, "welcome-title");
    }

    #[test]
    fn finds_message_line() {
        let source = "# Comment\nwelcome-title = Hello\nother = Value\n";
        let range = find_fluent_message(source, "welcome-title").unwrap();
        assert_eq!(range.start, Position::new(1, 0));
        assert_eq!(range.end, Position::new(1, 13));
    }

    #[test]
    fn skips_non_message_lines_without_equals() {
        let source = "term\nwelcome-title = Hello\n";
        let range = find_fluent_message(source, "welcome-title").unwrap();
        assert_eq!(range.start, Position::new(1, 0));
    }
}
