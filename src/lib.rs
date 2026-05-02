use std::collections::HashMap;
use std::ops::Range as ByteRange;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use fluent_syntax::ast::{Entry, Resource};
use fluent_syntax::parser;
use fluent_syntax::serializer;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tempfile::Builder as TempFileBuilder;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::{Error as LspError, Result as LspResult};
use tower_lsp::lsp_types::{
    CodeLens, CodeLensOptions, CodeLensParams, Command, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, ExecuteCommandOptions, ExecuteCommandParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InitializeParams, InitializeResult,
    InlayHint, InlayHintLabel, InlayHintOptions, InlayHintParams, InlayHintServerCapabilities,
    Location, MarkupContent, MarkupKind, MessageType, OneOf, Position, Range, ReferenceParams,
    ServerCapabilities, ShowDocumentParams, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

const CONFIG_FILE_NAMES: [&str; 2] = ["fluent-lsp.toml", ".fluent-lsp.toml"];
const SHOW_MESSAGE_SELECTOR_COMBINATIONS_LIMIT: usize = 10;
const SHOW_SELECTOR_COMBINATIONS_COMMAND: &str = "fluent-lsp.showSelectorCombinations";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub origin_language: String,
    #[serde(default)]
    pub file_masks: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    root_dir: PathBuf,
    origin_language: String,
    file_masks: Vec<FileMask>,
}

#[derive(Debug, Clone)]
struct FileMask {
    raw: String,
    matcher: Regex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileMatch {
    mask_index: usize,
    language: String,
    filepath: String,
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
        let origin_language = config.origin_language.trim().to_string();
        if origin_language.is_empty() {
            anyhow::bail!("origin_language must not be empty");
        }

        let file_masks = compile_file_masks(&config.file_masks)?;
        if file_masks.is_empty() {
            anyhow::bail!(
                "file_masks must include at least one template containing {{lang}} and {{filepath}}"
            );
        }

        Ok(Self {
            root_dir,
            origin_language,
            file_masks,
        })
    }

    fn origin_language(&self) -> &str {
        &self.origin_language
    }

    fn file_match(&self, path: &Path) -> Option<FileMatch> {
        if !is_fluent_file(path) {
            return None;
        }

        let relative = self.relative_path(path)?;
        for (mask_index, mask) in self.file_masks.iter().enumerate() {
            let captures = match mask.matcher.captures(&relative) {
                Some(captures) => captures,
                None => continue,
            };
            let language = captures.name("lang")?.as_str().to_string();
            let filepath = captures.name("filepath")?.as_str().to_string();
            return Some(FileMatch {
                mask_index,
                language,
                filepath,
            });
        }

        None
    }

    fn is_origin_file(&self, path: &Path) -> bool {
        self.file_match(path)
            .map(|file_match| file_match.language == self.origin_language)
            .unwrap_or(false)
    }

    fn matches_translation_file(&self, path: &Path) -> bool {
        self.file_match(path)
            .map(|file_match| file_match.language != self.origin_language)
            .unwrap_or(false)
    }

    fn origin_file_for(&self, path: &Path) -> Option<PathBuf> {
        let file_match = self.file_match(path)?;
        Some(self.render_path(
            file_match.mask_index,
            &self.origin_language,
            &file_match.filepath,
        ))
    }

    fn matches_origin_counterpart(&self, path: &Path, origin_match: &FileMatch) -> bool {
        self.file_match(path)
            .map(|candidate| {
                candidate.mask_index == origin_match.mask_index
                    && candidate.filepath == origin_match.filepath
                    && candidate.language != self.origin_language
            })
            .unwrap_or(false)
    }

    fn describe_masks(&self) -> String {
        if self.file_masks.is_empty() {
            "<none>".to_string()
        } else {
            self.file_masks
                .iter()
                .map(|mask| mask.raw.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    fn render_path(&self, mask_index: usize, language: &str, filepath: &str) -> PathBuf {
        let relative = self.file_masks[mask_index]
            .raw
            .replace("{lang}", language)
            .replace("{filepath}", filepath);
        self.root_dir.join(relative)
    }

    fn relative_path(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root_dir).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }
}

fn compile_file_masks(masks: &[String]) -> Result<Vec<FileMask>> {
    let mut compiled = Vec::with_capacity(masks.len());

    for mask in masks {
        if mask.matches("{lang}").count() != 1 || mask.matches("{filepath}").count() != 1 {
            anyhow::bail!(
                "invalid file mask `{mask}`: expected exactly one {{lang}} and one {{filepath}} placeholder"
            );
        }

        let lang_offset = mask
            .find("{lang}")
            .with_context(|| format!("invalid file mask `{mask}`: missing {{lang}}"))?;
        let filepath_offset = mask
            .find("{filepath}")
            .with_context(|| format!("invalid file mask `{mask}`: missing {{filepath}}"))?;
        let (first_offset, first_placeholder, second_offset, second_placeholder) =
            if lang_offset < filepath_offset {
                (lang_offset, "{lang}", filepath_offset, "{filepath}")
            } else {
                (filepath_offset, "{filepath}", lang_offset, "{lang}")
            };

        let mut pattern = String::from("^");
        pattern.push_str(&regex::escape(&mask[..first_offset]));
        pattern.push_str(match first_placeholder {
            "{lang}" => "(?P<lang>[^/]+)",
            "{filepath}" => "(?P<filepath>.+)",
            _ => unreachable!(),
        });
        pattern.push_str(&regex::escape(
            &mask[first_offset + first_placeholder.len()..second_offset],
        ));
        pattern.push_str(match second_placeholder {
            "{lang}" => "(?P<lang>[^/]+)",
            "{filepath}" => "(?P<filepath>.+)",
            _ => unreachable!(),
        });
        pattern.push_str(&regex::escape(
            &mask[second_offset + second_placeholder.len()..],
        ));
        pattern.push('$');

        compiled.push(FileMask {
            raw: mask.clone(),
            matcher: Regex::new(&pattern)
                .with_context(|| format!("invalid file mask regex generated from `{mask}`"))?,
        });
    }

    Ok(compiled)
}

fn collect_translation_files(workspace: &WorkspaceConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_under(&workspace.root_dir, &mut files);
    files.retain(|path| workspace.matches_translation_file(path));
    files.sort();
    files
}

fn collect_files_under(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_under(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceRender {
    comments: Option<String>,
    source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MessagePreview {
    selectors: Vec<(String, String)>,
    text: String,
}

#[derive(Debug, Clone)]
struct ActiveSelectorContext {
    name: String,
    indent: usize,
    current_variant: Option<String>,
    current_variant_line: Option<usize>,
}

#[derive(Default)]
struct ServerState {
    root_dir: Option<PathBuf>,
    workspace: Option<WorkspaceConfig>,
    open_documents: HashMap<Url, String>,
    supports_show_document: bool,
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

    async fn read_document_text_required(&self, uri: &Url) -> LspResult<String> {
        if let Some(text) = self.state.read().await.open_documents.get(uri).cloned() {
            return Ok(text);
        }

        let path = uri
            .to_file_path()
            .map_err(|()| LspError::invalid_params("expected a file URI"))?;
        tokio::fs::read_to_string(&path).await.map_err(|error| {
            internal_error_with_message(format!("failed to read {}: {error}", path.display()))
        })
    }

    async fn respond_to_clicked_command_with_error<M>(
        &self,
        error: LspError,
        message_type: MessageType,
        message: M,
    ) -> LspResult<Option<Value>>
    where
        M: Into<String>,
    {
        self.client.show_message(message_type, message.into()).await;
        Err(error)
    }

    async fn definition_for(&self, params: GotoDefinitionParams) -> Option<Location> {
        let state = self.state.read().await;
        let workspace = state.workspace.clone()?;
        let uri = params.text_document_position_params.text_document.uri;
        let path = uri.to_file_path().ok()?;
        if !workspace.matches_translation_file(&path) {
            return None;
        }

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())?;
        drop(state);

        let key = extract_definition_key(
            &source,
            &path,
            params.text_document_position_params.position,
        )?;
        let origin_path = workspace.origin_file_for(&path)?;
        let origin_uri = Url::from_file_path(&origin_path).ok()?;
        let origin_source = self.read_document_text(&origin_uri).await?;

        let definition = find_fluent_definition(&origin_source, &key)?;
        Some(Location {
            uri: origin_uri,
            range: definition,
        })
    }

    async fn references_for(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let state = self.state.read().await;
        let workspace = state.workspace.clone()?;
        let uri = params.text_document_position.text_document.uri;
        let path = uri.to_file_path().ok()?;
        if !workspace.is_origin_file(&path) {
            return None;
        }
        let origin_match = workspace.file_match(&path)?;

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())?;
        drop(state);

        let key = extract_definition_key(&source, &path, params.text_document_position.position)?;
        let mut references = Vec::new();

        if params.context.include_declaration {
            let range = find_fluent_definition(&source, &key)?;
            references.push(Location {
                uri: uri.clone(),
                range,
            });
        }

        for translation_path in collect_translation_files(&workspace) {
            if !workspace.matches_origin_counterpart(&translation_path, &origin_match) {
                continue;
            }
            let translation_uri = match Url::from_file_path(&translation_path) {
                Ok(uri) => uri,
                Err(()) => continue,
            };
            let translation_source = match self.read_document_text(&translation_uri).await {
                Some(source) => source,
                None => continue,
            };
            let Some(range) = find_fluent_definition(&translation_source, &key) else {
                continue;
            };
            references.push(Location {
                uri: translation_uri,
                range,
            });
        }

        Some(references)
    }

    async fn hover_for(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let state = self.state.read().await;
        let Some(workspace) = state.workspace.clone() else {
            return Ok(None);
        };
        let uri = params.text_document_position_params.text_document.uri;
        let path = uri
            .to_file_path()
            .map_err(|()| LspError::invalid_params("expected a file URI"))?;
        if workspace.file_match(&path).is_none() {
            return Ok(None);
        }

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())
            .ok_or_else(|| {
                internal_error_with_message(format!("failed to read {}", path.display()))
            })?;
        drop(state);

        let Some(key) = extract_definition_key(
            &source,
            &path,
            params.text_document_position_params.position,
        ) else {
            return Ok(None);
        };
        let resource = parse_fluent_resource(&source);
        let Some(pattern) = find_fluent_pattern(&resource, &key) else {
            return Ok(None);
        };
        let Some(hover_range) = find_fluent_definition(&source, &key) else {
            return Ok(None);
        };
        let selector_overrides = selector_overrides_for_position(
            &source,
            &key,
            params.text_document_position_params.position,
        );
        let hover_value = render_hover_markdown(
            &render_message_preview(pattern, Some(&selector_overrides)),
        );

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_value,
            }),
            range: Some(hover_range),
        }))
    }

    async fn code_lenses_for(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        let state = self.state.read().await;
        let Some(workspace) = state.workspace.clone() else {
            return Ok(None);
        };
        let uri = params.text_document.uri;
        let path = uri
            .to_file_path()
            .map_err(|()| LspError::invalid_params("expected a file URI"))?;
        if workspace.file_match(&path).is_none() {
            return Ok(None);
        }

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())
            .ok_or_else(|| {
                internal_error_with_message(format!("failed to read {}", path.display()))
            })?;
        drop(state);
        let mut lenses = Vec::new();
        for key in collect_fluent_keys(&source) {
            let Some(range) = find_fluent_definition(&source, &key) else {
                continue;
            };
            let Some(expansion) = selector_expansion_for(&source, &key, 1) else {
                continue;
            };
            if expansion.items.is_empty()
                || expansion.items.iter().all(|item| item.selectors.is_empty())
            {
                continue;
            }
            let Some(total_count) = selector_total_count_for(&source, &key) else {
                continue;
            };

            lenses.push(CodeLens {
                range,
                command: Some(Command {
                    title: code_lens_title(total_count),
                    command: SHOW_SELECTOR_COMBINATIONS_COMMAND.to_string(),
                    arguments: Some(vec![Value::String(uri.to_string()), Value::String(key)]),
                }),
                data: None,
            });
        }

        if lenses.is_empty() {
            Ok(None)
        } else {
            Ok(Some(lenses))
        }
    }

    async fn inlay_hints_for(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let state = self.state.read().await;
        let Some(workspace) = state.workspace.clone() else {
            return Ok(None);
        };
        let uri = params.text_document.uri;
        let path = uri
            .to_file_path()
            .map_err(|()| LspError::invalid_params("expected a file URI"))?;
        if workspace.file_match(&path).is_none() {
            return Ok(None);
        }

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())
            .ok_or_else(|| {
                internal_error_with_message(format!("failed to read {}", path.display()))
            })?;
        drop(state);

        let origin_source = if workspace.is_origin_file(&path) {
            source.clone()
        } else {
            let origin_path = workspace.origin_file_for(&path).ok_or_else(|| {
                internal_error_with_message(format!(
                    "failed to resolve origin counterpart for {}",
                    path.display()
                ))
            })?;
            let origin_uri = Url::from_file_path(&origin_path).map_err(|()| {
                internal_error_with_message(format!(
                    "failed to convert origin path to URI: {}",
                    origin_path.display()
                ))
            })?;
            self.read_document_text_required(&origin_uri).await?
        };

        let mut hints = Vec::new();
        for key in collect_fluent_keys(&source) {
            let Some(position) = find_fluent_hint_position(&source, &key) else {
                continue;
            };
            if !range_contains_position(&params.range, position) {
                continue;
            }
            let Some(label) = render_source_inlay_hint_label(&origin_source, &key) else {
                continue;
            };
            hints.push(InlayHint {
                position,
                label: InlayHintLabel::String(label),
                kind: None,
                text_edits: None,
                tooltip: None,
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }

        if hints.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hints))
        }
    }

    async fn execute_selector_combinations_command(
        &self,
        params: ExecuteCommandParams,
    ) -> LspResult<Option<Value>> {
        if params.command != SHOW_SELECTOR_COMBINATIONS_COMMAND {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params(format!("unknown command: {}", params.command)),
                    MessageType::ERROR,
                    format!("Unknown command: {}", params.command),
                )
                .await;
        }

        let mut arguments = params.arguments.into_iter();
        let Some(uri_value) = arguments.next() else {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params("missing document URI argument"),
                    MessageType::ERROR,
                    "Missing document URI for selector combinations command",
                )
                .await;
        };
        let Some(key_value) = arguments.next() else {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params("missing Fluent key argument"),
                    MessageType::ERROR,
                    "Missing Fluent key for selector combinations command",
                )
                .await;
        };

        let Ok(uri) = serde_json::from_value::<String>(uri_value) else {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params("document URI argument must be a string"),
                    MessageType::ERROR,
                    "Selector combinations command received an invalid document URI",
                )
                .await;
        };
        let Ok(key) = serde_json::from_value::<String>(key_value) else {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params("Fluent key argument must be a string"),
                    MessageType::ERROR,
                    "Selector combinations command received an invalid Fluent key",
                )
                .await;
        };
        let Ok(uri) = Url::parse(&uri) else {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params("document URI argument must be a valid URI"),
                    MessageType::ERROR,
                    "Selector combinations command received an invalid document URI",
                )
                .await;
        };
        let Some(path) = uri.to_file_path().ok() else {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params("document URI must point to a file"),
                    MessageType::ERROR,
                    "Selector combinations command expected a file-backed document",
                )
                .await;
        };

        let state = self.state.read().await;
        let Some(workspace) = state.workspace.clone() else {
            return self
                .respond_to_clicked_command_with_error(
                    internal_error_with_message("no fluent-lsp workspace is loaded"),
                    MessageType::ERROR,
                    "fluent-lsp is not configured for this workspace",
                )
                .await;
        };
        let supports_show_document = state.supports_show_document;
        if workspace.file_match(&path).is_none() {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params(format!(
                        "selector combinations are unavailable for {}",
                        path.display()
                    )),
                    MessageType::ERROR,
                    format!(
                        "Selector combinations are unavailable for {}",
                        path.display()
                    ),
                )
                .await;
        }
        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok());
        drop(state);
        let Some(source) = source else {
            return self
                .respond_to_clicked_command_with_error(
                    internal_error_with_message(format!("failed to read {}", path.display())),
                    MessageType::ERROR,
                    format!("Failed to read {}", path.display()),
                )
                .await;
        };

        let max_items = if supports_show_document {
            usize::MAX
        } else {
            SHOW_MESSAGE_SELECTOR_COMBINATIONS_LIMIT
        };
        let Some(current_section) = render_selector_combinations_section(
            "Current language combinations:",
            &source,
            &key,
            max_items,
        ) else {
            self.client
                .show_message(
                    MessageType::INFO,
                    format!("No selector combinations available for `{key}`"),
                )
                .await;
            return Ok(None);
        };

        if supports_show_document {
            let Some(file_match) = workspace.file_match(&path) else {
                return self
                    .respond_to_clicked_command_with_error(
                        internal_error_with_message(format!(
                            "failed to resolve file metadata for {}",
                            path.display()
                        )),
                        MessageType::ERROR,
                        format!("Failed to resolve file metadata for {}", path.display()),
                    )
                    .await;
            };
            let origin_source = if workspace.is_origin_file(&path) {
                source.clone()
            } else {
                let origin_path = workspace.origin_file_for(&path).ok_or_else(|| {
                    internal_error_with_message(format!(
                        "failed to resolve origin counterpart for {}",
                        path.display()
                    ))
                })?;
                let origin_uri = Url::from_file_path(&origin_path).map_err(|()| {
                    internal_error_with_message(format!(
                        "failed to convert origin path to URI: {}",
                        origin_path.display()
                    ))
                })?;
                self.read_document_text_required(&origin_uri).await?
            };
            let current_render = render_fluent_source(&source, &key);
            let origin_render = render_fluent_source(&origin_source, &key);
            let origin_section = if workspace.is_origin_file(&path) {
                None
            } else {
                render_selector_combinations_section(
                    "Source language combinations:",
                    &origin_source,
                    &key,
                    max_items,
                )
            };
            let document_text = render_selector_combinations_document(
                &key,
                &file_match,
                workspace.origin_language(),
                origin_render.as_ref(),
                origin_section.as_deref(),
                current_render.as_ref(),
                &current_section,
            );

            let document_uri = write_selector_combinations_temp_document(&key, &document_text)
                .map_err(|message| internal_error_with_message(message.clone()))?;
            let opened = self
                .client
                .show_document(ShowDocumentParams {
                    uri: document_uri,
                    external: Some(false),
                    take_focus: Some(true),
                    selection: Some(Range::new(Position::new(0, 0), Position::new(0, 0))),
                })
                .await
                .map_err(|error| {
                    internal_error_with_message(format!(
                        "failed to open selector combinations document: {error}"
                    ))
                })?;
            if opened {
                return Ok(None);
            }
            return self
                .respond_to_clicked_command_with_error(
                    internal_error_with_message(
                        "client declined to show the selector combinations document",
                    ),
                    MessageType::ERROR,
                    "The editor refused to open the selector combinations document",
                )
                .await;
        }

        self.client
            .show_message(
                MessageType::INFO,
                format!("Selector combinations for {key}\n\n{current_section}"),
            )
            .await;

        Ok(None)
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
            state.supports_show_document = params
                .capabilities
                .window
                .and_then(|window| window.show_document)
                .map(|capability| capability.support)
                .unwrap_or(false);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                definition_provider: Some(OneOf::Left(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![SHOW_SELECTOR_COMBINATIONS_COMMAND.to_string()],
                    work_done_progress_options: Default::default(),
                }),
                hover_provider: Some(tower_lsp::lsp_types::HoverProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
                    InlayHintOptions {
                        resolve_provider: Some(false),
                        work_done_progress_options: Default::default(),
                    },
                ))),
                references_provider: Some(OneOf::Left(true)),
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
                            "loaded origin language {} with masks [{}]",
                            workspace.origin_language(),
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

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        Ok(self.references_for(params).await)
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        self.hover_for(params).await
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        self.inlay_hints_for(params).await
    }

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        self.code_lenses_for(params).await
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<Value>> {
        self.execute_selector_combinations_command(params).await
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

pub fn extract_definition_key(source: &str, path: &Path, position: Position) -> Option<String> {
    if is_fluent_file(path) {
        extract_fluent_key_at_position(source, position)
    } else {
        extract_key_at_position(source, position)
    }
}

pub fn find_fluent_definition(source: &str, key: &str) -> Option<Range> {
    let resource = parse_fluent_resource(source);
    let span = find_fluent_definition_span(&resource, key)?;
    byte_range_to_lsp_range(source, span)
}

pub fn render_fluent_entry(source: &str, key: &str) -> Option<String> {
    let resource = parse_fluent_resource(source);
    let entry = find_fluent_entry(&resource, key)?;
    let rendered = serializer::serialize(&Resource {
        body: vec![entry.clone()],
    });
    Some(rendered)
}

fn render_fluent_source(source: &str, key: &str) -> Option<SourceRender> {
    let resource = parse_fluent_resource(source);
    let entry_index = find_fluent_entry_index(&resource, key)?;
    let mut comment_entries = Vec::new();

    let mut comment_start = entry_index;
    while comment_start > 0 && is_free_comment_entry(&resource.body[comment_start - 1]) {
        comment_start -= 1;
    }
    for comment_entry in &resource.body[comment_start..entry_index] {
        comment_entries.push(comment_entry.clone());
    }

    let entry = resource.body[entry_index].clone();
    let entry = strip_inline_comment(entry, &mut comment_entries);
    let comments = if comment_entries.is_empty() {
        None
    } else {
        Some(render_hover_comments(&comment_entries))
    };
    let source = render_entry_source(&entry, key)?;

    Some(SourceRender { comments, source })
}

fn render_hover_markdown(preview: &MessagePreview) -> String {
    let mut sections = Vec::new();
    if !preview.selectors.is_empty() {
        sections.push(render_selector_assignments(&preview.selectors));
    }
    sections.push(render_ftl_block(&preview.text));
    sections.join("\n\n")
}

fn render_selector_combinations_document(
    key: &str,
    file_match: &FileMatch,
    origin_language: &str,
    origin_render: Option<&SourceRender>,
    origin_combinations_section: Option<&str>,
    current_render: Option<&SourceRender>,
    current_combinations_section: &str,
) -> String {
    let is_origin_language_document = file_match.language == origin_language;
    let mut sections = vec![format!("# Selector combinations for `{key}`")];
    sections.push(format!("Current language: `{}`", file_match.language));
    if !is_origin_language_document {
        sections.push(format!("Source language: `{origin_language}`"));
    }
    sections.push(format!("Logical file: `{}`", file_match.filepath));

    if !is_origin_language_document {
        if let Some(origin_render) = origin_render {
            sections.push("Source text:".to_string());
            if let Some(comments) = &origin_render.comments {
                sections.push(render_ftl_block(comments));
            }
            sections.push(render_ftl_block(&origin_render.source));
        }
        if let Some(origin_combinations_section) = origin_combinations_section {
            sections.push(origin_combinations_section.to_string());
        }
    }

    if let Some(current_render) = current_render {
        sections.push("Current text:".to_string());
        if let Some(comments) = &current_render.comments {
            sections.push(render_ftl_block(comments));
        }
        sections.push(render_ftl_block(&current_render.source));
    }

    sections.push(current_combinations_section.to_string());
    sections.join("\n\n")
}

fn write_selector_combinations_temp_document(key: &str, contents: &str) -> Result<Url, String> {
    let mut file = TempFileBuilder::new()
        .prefix("fluent-lsp-selector-combinations-")
        .suffix(&format!("-{}.md", sanitize_document_segment(key)))
        .tempfile()
        .map_err(|error| format!("failed to create temp selector document: {error}"))?;

    use std::io::Write;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("failed to write temp selector document: {error}"))?;
    let temp_path = file.into_temp_path();
    let path = temp_path
        .keep()
        .map_err(|error| format!("failed to persist temp selector document: {error}"))?;
    Url::from_file_path(&path)
        .map_err(|()| format!("failed to convert temp path to URI: {}", path.display()))
}

fn render_entry_source(entry: &Entry<&str>, key: &str) -> Option<String> {
    let (_, attribute_key) = split_fluent_key(key);
    if let Some(attribute_key) = attribute_key {
        return render_attribute_source(attribute_key, entry);
    }

    Some(serializer::serialize(&Resource {
        body: vec![entry.clone()],
    }))
}

fn render_attribute_source(attribute_key: &str, entry: &Entry<&str>) -> Option<String> {
    let attribute = match entry {
        Entry::Message(message) => message
            .attributes
            .iter()
            .find(|attribute| attribute.id.name == attribute_key)?,
        Entry::Term(term) => term
            .attributes
            .iter()
            .find(|attribute| attribute.id.name == attribute_key)?,
        _ => return None,
    };

    let synthetic = Entry::Message(fluent_syntax::ast::Message {
        id: fluent_syntax::ast::Identifier {
            name: "__hover",
            span: 0..7,
        },
        value: None,
        attributes: vec![attribute.clone()],
        comment: None,
    });
    let rendered = serializer::serialize(&Resource {
        body: vec![synthetic],
    });
    Some(strip_attribute_container(&rendered))
}

fn strip_attribute_container(rendered: &str) -> String {
    rendered
        .lines()
        .skip(1)
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn render_source_inlay_hint_label(source: &str, key: &str) -> Option<String> {
    let resource = parse_fluent_resource(source);
    let pattern = find_fluent_pattern(&resource, key)?;
    let preview = render_message_preview(pattern, None);
    if preview.selectors.is_empty() {
        Some(preview.text)
    } else {
        let selectors = preview
            .selectors
            .iter()
            .map(|(selector, variant)| {
                format!("{}={variant}", selector.strip_prefix('$').unwrap_or(selector))
            })
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("[{selectors}] {}", preview.text))
    }
}

fn render_message_preview(
    pattern: &fluent_syntax::ast::Pattern<&str>,
    selector_overrides: Option<&HashMap<String, String>>,
) -> MessagePreview {
    let mut preview = MessagePreview::default();
    for element in &pattern.elements {
        let element_preview = render_pattern_element_preview(element, selector_overrides);
        preview.selectors.extend(element_preview.selectors);
        preview.text.push_str(&element_preview.text);
    }
    preview
}

fn render_pattern_element_preview(
    element: &fluent_syntax::ast::PatternElement<&str>,
    selector_overrides: Option<&HashMap<String, String>>,
) -> MessagePreview {
    match element {
        fluent_syntax::ast::PatternElement::TextElement { value } => MessagePreview {
            selectors: Vec::new(),
            text: (*value).to_string(),
        },
        fluent_syntax::ast::PatternElement::Placeable { expression } => {
            render_expression_preview(expression, selector_overrides)
        }
    }
}

fn render_expression_preview(
    expression: &fluent_syntax::ast::Expression<&str>,
    selector_overrides: Option<&HashMap<String, String>>,
) -> MessagePreview {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline) => MessagePreview {
            selectors: Vec::new(),
            text: render_inline_expression_as_text(inline),
        },
        fluent_syntax::ast::Expression::Select { selector, variants } => {
            let default_variant = variants
                .iter()
                .find(|variant| variant.default)
                .or_else(|| variants.first());
            let selector_name = render_inline_expression(selector);
            let Some((selected_variant, rendered_variant_name)) = selector_overrides
                .and_then(|overrides| {
                    let requested = overrides.get(&selector_name)?;
                    variants
                        .iter()
                        .find(|variant| render_variant_key(&variant.key) == *requested)
                        .map(|variant| (variant, requested.clone()))
                })
                .or_else(|| default_variant.map(|variant| (variant, "*".to_string())))
            else {
                return MessagePreview::default();
            };

            let mut preview = render_message_preview(&selected_variant.value, selector_overrides);
            preview.selectors.insert(
                0,
                (selector_name, rendered_variant_name),
            );
            preview
        }
    }
}

fn strip_inline_comment<'a>(
    entry: Entry<&'a str>,
    comment_entries: &mut Vec<Entry<&'a str>>,
) -> Entry<&'a str> {
    match entry {
        Entry::Message(mut message) => {
            if let Some(comment) = message.comment.take() {
                comment_entries.push(Entry::Comment(comment));
            }
            Entry::Message(message)
        }
        Entry::Term(mut term) => {
            if let Some(comment) = term.comment.take() {
                comment_entries.push(Entry::Comment(comment));
            }
            Entry::Term(term)
        }
        other => other,
    }
}

fn is_free_comment_entry(entry: &Entry<&str>) -> bool {
    matches!(
        entry,
        Entry::Comment(_) | Entry::GroupComment(_) | Entry::ResourceComment(_)
    )
}

fn render_hover_comments(entries: &[Entry<&str>]) -> String {
    let mut rendered = String::new();
    for entry in entries {
        match entry {
            Entry::Comment(comment) => append_comment_with_prefix(&mut rendered, comment, "#"),
            Entry::GroupComment(comment) => {
                append_comment_with_prefix(&mut rendered, comment, "##")
            }
            Entry::ResourceComment(comment) => {
                append_comment_with_prefix(&mut rendered, comment, "###")
            }
            _ => {}
        }
    }
    rendered
}

fn append_comment_with_prefix(
    buffer: &mut String,
    comment: &fluent_syntax::ast::Comment<&str>,
    prefix: &str,
) {
    for line in &comment.content {
        buffer.push_str(prefix);
        if !line.trim().is_empty() {
            buffer.push(' ');
            buffer.push_str(line);
        }
        buffer.push('\n');
    }
}

fn render_selector_combinations_section(
    heading: &str,
    source: &str,
    key: &str,
    max_items: usize,
) -> Option<String> {
    let expansion = selector_expansion_for(source, key, max_items)?;
    if expansion.items.is_empty() || expansion.items.iter().all(|item| item.selectors.is_empty()) {
        return None;
    }

    let rendered_count = expansion.items.len();
    let mut lines = vec![heading.to_string()];
    for item in expansion.items {
        lines.push(render_selector_assignments(&item.selectors));
        lines.push(render_ftl_block(&item.text));
    }
    let omitted = expansion.total_count.saturating_sub(rendered_count);
    if omitted > 0 {
        lines.push(format!("`...`\n{omitted} more"));
    }

    Some(lines.join("\n"))
}

fn selector_expansion_for(source: &str, key: &str, max_items: usize) -> Option<SelectorExpansion> {
    let resource = parse_fluent_resource(source);
    let pattern = find_fluent_pattern(&resource, key)?;
    Some(expand_pattern(pattern, max_items))
}

fn selector_total_count_for(source: &str, key: &str) -> Option<usize> {
    let resource = parse_fluent_resource(source);
    let pattern = find_fluent_pattern(&resource, key)?;
    Some(count_pattern_combinations(pattern))
}

fn code_lens_title(total_count: usize) -> String {
    match total_count {
        1 => "Show 1 selector combination".to_string(),
        count => format!("Show all {count} selector combinations"),
    }
}

fn sanitize_document_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect();

    if sanitized.is_empty() {
        "entry".to_string()
    } else {
        sanitized
    }
}

fn parse_select_openers(line: &str) -> Vec<String> {
    use std::sync::OnceLock;

    static SELECT_OPENER_RE: OnceLock<Regex> = OnceLock::new();
    let re = SELECT_OPENER_RE
        .get_or_init(|| Regex::new(r"\{\s*([^{}\n]+?)\s*->").expect("valid select opener regex"));

    re.captures_iter(line)
        .filter_map(|captures| captures.get(1).map(|value| value.as_str().trim().to_string()))
        .collect()
}

fn parse_variant_line(trimmed: &str) -> Option<String> {
    let trimmed = trimmed.strip_prefix('*').unwrap_or(trimmed);
    let rest = trimmed.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(rest[..end].trim().to_string())
}

fn collect_fluent_keys(source: &str) -> Vec<String> {
    let resource = parse_fluent_resource(source);
    let mut keys = Vec::new();

    for entry in &resource.body {
        match entry {
            Entry::Message(message) => {
                keys.push(message.id.name.to_string());
                for attribute in &message.attributes {
                    keys.push(format!("{}.{}", message.id.name, attribute.id.name));
                }
            }
            Entry::Term(term) => {
                keys.push(format!("-{}", term.id.name));
                for attribute in &term.attributes {
                    keys.push(format!("-{}.{}", term.id.name, attribute.id.name));
                }
            }
            _ => {}
        }
    }

    keys
}

fn count_pattern_combinations(pattern: &fluent_syntax::ast::Pattern<&str>) -> usize {
    let mut total_count = 1usize;
    for element in &pattern.elements {
        total_count = total_count.saturating_mul(count_pattern_element_combinations(element));
    }
    total_count
}

fn count_pattern_element_combinations(element: &fluent_syntax::ast::PatternElement<&str>) -> usize {
    match element {
        fluent_syntax::ast::PatternElement::TextElement { .. } => 1,
        fluent_syntax::ast::PatternElement::Placeable { expression } => {
            count_expression_combinations(expression)
        }
    }
}

fn count_expression_combinations(expression: &fluent_syntax::ast::Expression<&str>) -> usize {
    match expression {
        fluent_syntax::ast::Expression::Inline(_) => 1,
        fluent_syntax::ast::Expression::Select { variants, .. } => {
            variants.iter().fold(0usize, |total, variant| {
                total.saturating_add(count_pattern_combinations(&variant.value))
            })
        }
    }
}

fn parse_fluent_resource(source: &str) -> Resource<&str> {
    match parser::parse(source) {
        Ok(resource) => resource,
        Err((resource, _errors)) => resource,
    }
}

fn extract_fluent_key_at_position(source: &str, position: Position) -> Option<String> {
    let line_index = usize::try_from(position.line).ok()?;
    collect_fluent_keys(source)
        .into_iter()
        .filter_map(|key| {
            let (start_line, end_line) = find_fluent_block_line_range(source, &key)?;
            if start_line <= line_index && line_index <= end_line {
                Some((key, start_line, end_line))
            } else {
                None
            }
        })
        .max_by_key(|(_, start_line, end_line)| (*start_line, usize::MAX - (end_line - start_line)))
        .map(|(key, _, _)| key)
}

fn find_fluent_pattern<'a>(
    resource: &'a Resource<&'a str>,
    key: &str,
) -> Option<&'a fluent_syntax::ast::Pattern<&'a str>> {
    let entry = find_fluent_entry(resource, key)?;
    let (entry_key, attribute_key) = split_fluent_key(key);

    match entry {
        Entry::Message(message) if entry_key == message.id.name => {
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
        Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
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
    }
}

fn find_fluent_definition_span(resource: &Resource<&str>, key: &str) -> Option<ByteRange<usize>> {
    let entry = find_fluent_entry(resource, key)?;
    let (entry_key, attribute_key) = split_fluent_key(key);

    match entry {
        Entry::Message(message) if entry_key == message.id.name => {
            if let Some(attribute_key) = attribute_key {
                message
                    .attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| attribute.id.span.clone())
            } else {
                Some(message.id.span.clone())
            }
        }
        Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
            if let Some(attribute_key) = attribute_key {
                term.attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| attribute.id.span.clone())
            } else {
                Some(term.id.span.clone())
            }
        }
        _ => None,
    }
}

fn find_fluent_entry<'a>(resource: &'a Resource<&'a str>, key: &str) -> Option<&'a Entry<&'a str>> {
    let (entry_key, attribute_key) = split_fluent_key(key);
    resource
        .body
        .iter()
        .find(|entry| entry_matches_key(entry, entry_key, attribute_key))
}

fn find_fluent_entry_index(resource: &Resource<&str>, key: &str) -> Option<usize> {
    let (entry_key, attribute_key) = split_fluent_key(key);
    resource
        .body
        .iter()
        .position(|entry| entry_matches_key(entry, entry_key, attribute_key))
}

fn find_fluent_block_line_range(source: &str, key: &str) -> Option<(usize, usize)> {
    let definition = find_fluent_definition(source, key)?;
    let start_line = usize::try_from(definition.start.line).ok()?;
    let (_, attribute_key) = split_fluent_key(key);
    let lines: Vec<&str> = source.split('\n').collect();
    let start_indent = leading_spaces(lines.get(start_line)?);
    let mut end_line = start_line;

    for (index, line) in lines.iter().enumerate().skip(start_line + 1) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        let indent = leading_spaces(line);
        if attribute_key.is_some() {
            if indent <= start_indent {
                break;
            }
        } else if indent == 0 || trimmed.starts_with('.') {
            break;
        }

        end_line = index;
    }

    Some((start_line, end_line))
}

fn entry_matches_key(entry: &Entry<&str>, entry_key: &str, attribute_key: Option<&str>) -> bool {
    match entry {
        Entry::Message(message) if entry_key == message.id.name => {
            attribute_key.is_none()
                || message
                    .attributes
                    .iter()
                    .any(|attribute| Some(attribute.id.name) == attribute_key)
        }
        Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
            attribute_key.is_none()
                || term
                    .attributes
                    .iter()
                    .any(|attribute| Some(attribute.id.name) == attribute_key)
        }
        _ => false,
    }
}

fn split_fluent_key(key: &str) -> (&str, Option<&str>) {
    match key.rsplit_once('.') {
        Some((entry, attribute)) if !attribute.is_empty() => (entry, Some(attribute)),
        _ => (key, None),
    }
}

fn selector_overrides_for_position(
    source: &str,
    key: &str,
    position: Position,
) -> HashMap<String, String> {
    let Some(line_index) = usize::try_from(position.line).ok() else {
        return HashMap::new();
    };
    let cursor_character = usize::try_from(position.character).ok().unwrap_or(0);
    let Some((start_line, end_line)) = find_fluent_block_line_range(source, key) else {
        return HashMap::new();
    };
    if line_index < start_line || line_index > end_line {
        return HashMap::new();
    }

    let lines: Vec<&str> = source.split('\n').collect();
    let mut selectors: Vec<ActiveSelectorContext> = Vec::new();
    let mut same_line_closed_overrides = Vec::new();

    for (index, line) in lines.iter().enumerate().take(line_index + 1).skip(start_line) {
        let trimmed = line.trim_start();
        let indent = leading_spaces(line);

        if let Some(variant) = parse_variant_line(trimmed) {
            if let Some(selector) = selectors.last_mut() {
                if indent > selector.indent {
                    selector.current_variant = Some(variant);
                    selector.current_variant_line = Some(index);
                }
            }
        }

        for name in parse_select_openers(line) {
            selectors.push(ActiveSelectorContext {
                name,
                indent,
                current_variant: None,
                current_variant_line: None,
            });
        }

        if trimmed.starts_with('}') {
            while selectors
                .last()
                .is_some_and(|selector: &ActiveSelectorContext| indent <= selector.indent)
            {
                let popped = selectors.pop().expect("checked by is_some_and");
                if index == line_index && cursor_character > indent {
                    if let Some(variant) = popped.current_variant {
                        same_line_closed_overrides.push((popped.name, variant));
                    }
                }
            }
        }
    }

    let mut overrides: HashMap<String, String> = selectors
        .into_iter()
        .filter_map(|selector| {
            selector
                .current_variant
                .map(|variant| (selector.name, variant))
        })
        .collect();

    for (name, variant) in same_line_closed_overrides {
        overrides.insert(name, variant);
    }

    overrides
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SelectorExpansion {
    items: Vec<SelectorExpansionItem>,
    total_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SelectorExpansionItem {
    selectors: Vec<(String, String)>,
    text: String,
}

fn expand_pattern(
    pattern: &fluent_syntax::ast::Pattern<&str>,
    max_items: usize,
) -> SelectorExpansion {
    let mut items = vec![SelectorExpansionItem::default()];
    let mut total_count = 1usize;

    for element in &pattern.elements {
        let element_items = expand_pattern_element(element, max_items);
        if element_items.items.is_empty() {
            continue;
        }

        total_count = total_count.saturating_mul(element_items.total_count);

        let mut combined = Vec::new();
        for left in &items {
            for right in &element_items.items {
                if combined.len() >= max_items {
                    break;
                }
                let mut selectors = left.selectors.clone();
                selectors.extend(right.selectors.clone());
                combined.push(SelectorExpansionItem {
                    selectors,
                    text: format!("{}{}", left.text, right.text),
                });
            }
            if combined.len() >= max_items {
                break;
            }
        }

        items = combined;
        if items.is_empty() {
            break;
        }
    }

    SelectorExpansion { items, total_count }
}

fn expand_pattern_element(
    element: &fluent_syntax::ast::PatternElement<&str>,
    max_items: usize,
) -> SelectorExpansion {
    match element {
        fluent_syntax::ast::PatternElement::TextElement { value } => SelectorExpansion {
            items: vec![SelectorExpansionItem {
                selectors: Vec::new(),
                text: (*value).to_string(),
            }],
            total_count: 1,
        },
        fluent_syntax::ast::PatternElement::Placeable { expression } => {
            expand_expression(expression, max_items)
        }
    }
}

fn expand_expression(
    expression: &fluent_syntax::ast::Expression<&str>,
    max_items: usize,
) -> SelectorExpansion {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline) => SelectorExpansion {
            items: vec![SelectorExpansionItem {
                selectors: Vec::new(),
                text: render_inline_expression_as_text(inline),
            }],
            total_count: 1,
        },
        fluent_syntax::ast::Expression::Select { selector, variants } => {
            let selector_name = render_inline_expression(selector);
            let mut items = Vec::new();
            let mut total_count = 0usize;

            for variant in variants {
                let nested = expand_pattern(&variant.value, max_items);
                total_count = total_count.saturating_add(nested.total_count);
                let variant_name = render_variant_key(&variant.key);
                for item in nested.items {
                    if items.len() >= max_items {
                        break;
                    }
                    let mut selectors = vec![(selector_name.clone(), variant_name.clone())];
                    selectors.extend(item.selectors);
                    items.push(SelectorExpansionItem {
                        selectors,
                        text: item.text,
                    });
                }
                if items.len() >= max_items {
                    break;
                }
            }

            SelectorExpansion { items, total_count }
        }
    }
}

fn render_variant_key(key: &fluent_syntax::ast::VariantKey<&str>) -> String {
    match key {
        fluent_syntax::ast::VariantKey::Identifier { name } => (*name).to_string(),
        fluent_syntax::ast::VariantKey::NumberLiteral { value } => (*value).to_string(),
    }
}

fn render_inline_expression_as_text(expression: &fluent_syntax::ast::InlineExpression<&str>) -> String {
    format!("{{ {} }}", render_inline_expression(expression))
}

fn render_inline_expression(expression: &fluent_syntax::ast::InlineExpression<&str>) -> String {
    match expression {
        fluent_syntax::ast::InlineExpression::StringLiteral { value } => format!("\"{value}\""),
        fluent_syntax::ast::InlineExpression::NumberLiteral { value } => (*value).to_string(),
        fluent_syntax::ast::InlineExpression::FunctionReference { id, .. } => {
            format!("{}()", id.name)
        }
        fluent_syntax::ast::InlineExpression::MessageReference { id, attribute } => match attribute
        {
            Some(attribute) => format!("{}.{}", id.name, attribute.name),
            None => id.name.to_string(),
        },
        fluent_syntax::ast::InlineExpression::TermReference {
            id,
            attribute,
            arguments,
        } => {
            let mut rendered = format!("-{}", id.name);
            if let Some(attribute) = attribute {
                rendered.push('.');
                rendered.push_str(attribute.name);
            }
            if arguments.is_some() {
                rendered.push_str("()");
            }
            rendered
        }
        fluent_syntax::ast::InlineExpression::VariableReference { id } => format!("${}", id.name),
        fluent_syntax::ast::InlineExpression::Placeable { expression } => {
            format!("{{ {} }}", render_expression_summary(expression))
        }
    }
}

fn render_selector_assignments(selectors: &[(String, String)]) -> String {
    selectors
        .iter()
        .map(|(selector, variant)| format!("`{selector}={variant}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_ftl_block(text: &str) -> String {
    let mut block = String::from("```ftl\n");
    block.push_str(text);
    if !text.ends_with('\n') {
        block.push('\n');
    }
    block.push_str("```");
    block
}

fn render_expression_summary(expression: &fluent_syntax::ast::Expression<&str>) -> String {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline) => render_inline_expression(inline),
        fluent_syntax::ast::Expression::Select { selector, .. } => {
            format!("{} -> …", render_inline_expression(selector))
        }
    }
}

fn byte_range_to_lsp_range(source: &str, span: ByteRange<usize>) -> Option<Range> {
    Some(Range {
        start: byte_index_to_position(source, span.start)?,
        end: byte_index_to_position(source, span.end)?,
    })
}

fn byte_index_to_position(source: &str, target: usize) -> Option<Position> {
    if target > source.len() || !source.is_char_boundary(target) {
        return None;
    }

    let mut line = 0u32;
    let mut character = 0u32;
    for (byte_idx, ch) in source.char_indices() {
        if byte_idx == target {
            return Some(Position::new(line, character));
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    if target == source.len() {
        Some(Position::new(line, character))
    } else {
        None
    }
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

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
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

fn is_fluent_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("ftl")
}

fn range_contains_position(range: &Range, position: Position) -> bool {
    (range.start.line < position.line
        || (range.start.line == position.line && range.start.character <= position.character))
        && (position.line < range.end.line
            || (position.line == range.end.line && position.character <= range.end.character))
}

fn internal_error_with_message<M>(message: M) -> LspError
where
    M: Into<std::borrow::Cow<'static, str>>,
{
    let mut error = LspError::internal_error();
    error.message = message.into();
    error
}

fn find_fluent_hint_position(source: &str, key: &str) -> Option<Position> {
    let key_range = find_fluent_definition(source, key)?;
    let (line, _) = line_at(source, key_range.start.line as usize)?;
    let equals_index = line.find('=')?;
    let value_start = line[equals_index + 1..]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| ch.len_utf16() as u32)
        .sum::<u32>();
    Some(Position::new(
        key_range.start.line,
        equals_index as u32 + 1 + value_start,
    ))
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
    fn extracts_key_from_fluent_message_term_and_attribute() {
        let source =
            "welcome-title = Salut\n-brand-name = Nocturne\nbutton-copy =\n    .label = Lancer\n";

        let message =
            extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(0, 4))
                .unwrap();
        assert_eq!(message, "welcome-title");

        let term =
            extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(1, 2))
                .unwrap();
        assert_eq!(term, "-brand-name");

        let attribute =
            extract_definition_key(source, Path::new("locales/fr/app.ftl"), Position::new(3, 6))
                .unwrap();
        assert_eq!(attribute, "button-copy.label");
    }

    #[test]
    fn extracts_key_inside_selector_variant_text() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        let key =
            extract_definition_key(source, Path::new("locales/en/app.ftl"), Position::new(2, 18))
                .unwrap();
        assert_eq!(key, "install-hint");
    }

    #[test]
    fn collects_translation_files_without_origin_language_files() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("workspace");
        let workspace = WorkspaceConfig::load(root).unwrap();

        let files = collect_translation_files(&workspace);
        let relative: Vec<_> = files
            .iter()
            .map(|path| {
                path.strip_prefix(&workspace.root_dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        assert_eq!(
            relative,
            vec![
                "locales/es/app.ftl".to_string(),
                "locales/es/dialogs/menu.ftl".to_string(),
                "locales/fr/app.ftl".to_string(),
                "locales/fr/dialogs/menu.ftl".to_string(),
            ]
        );
    }

    #[test]
    fn resolves_origin_file_from_nested_translation_path() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("workspace");
        let workspace = WorkspaceConfig::load(root.clone()).unwrap();
        let translation = root.join("locales/es/dialogs/menu.ftl");

        let file_match = workspace.file_match(&translation).unwrap();
        assert_eq!(file_match.language, "es");
        assert_eq!(file_match.filepath, "dialogs/menu");
        assert_eq!(
            workspace.origin_file_for(&translation).unwrap(),
            root.join("locales/en/dialogs/menu.ftl")
        );
    }

    #[test]
    fn finds_message_line() {
        let source = "# Comment\nwelcome-title = Hello\nother = Value\n";
        let range = find_fluent_definition(source, "welcome-title").unwrap();
        assert_eq!(range.start, Position::new(1, 0));
        assert_eq!(range.end, Position::new(1, 13));
    }

    #[test]
    fn skips_non_message_lines_without_equals() {
        let source = "term\nwelcome-title = Hello\n";
        let range = find_fluent_definition(source, "welcome-title").unwrap();
        assert_eq!(range.start, Position::new(1, 0));
    }

    #[test]
    fn finds_term_and_attribute_ranges() {
        let source = "-brand-name = Nightly\nbutton-copy =\n    .label = Launch\n";

        let term = find_fluent_definition(source, "-brand-name").unwrap();
        assert_eq!(term.start, Position::new(0, 1));
        assert_eq!(term.end, Position::new(0, 11));

        let attribute = find_fluent_definition(source, "button-copy.label").unwrap();
        assert_eq!(attribute.start, Position::new(2, 5));
        assert_eq!(attribute.end, Position::new(2, 10));
    }

    #[test]
    fn renders_fluent_entry_with_comments() {
        let source = "# Shown on first launch\nwelcome-title = Welcome\n# Product naming\n# Keep in title case\n-brand-name = Nightly\n";

        let message = render_fluent_entry(source, "welcome-title").unwrap();
        assert_eq!(
            message,
            "# Shown on first launch\nwelcome-title = Welcome\n"
        );

        let term = render_fluent_entry(source, "-brand-name").unwrap();
        assert_eq!(
            term,
            "# Product naming\n# Keep in title case\n-brand-name = Nightly\n"
        );
    }

    #[test]
    fn renders_selector_combinations_section() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        let rendered = render_selector_combinations_section(
            "Current language combinations:",
            source,
            "install-hint",
            3,
        )
        .unwrap();
        assert_eq!(
            rendered,
            "Current language combinations:\n`$gender=female`, `$count=one`\n```ftl\nCopy the download link for her account on { $count } device now.\n```\n`$gender=female`, `$count=other`\n```ftl\nCopy the download link for her account on { $count } devices now.\n```\n`$gender=male`, `$count=one`\n```ftl\nCopy the download link for his account on { $count } device now.\n```\n`...`\n3 more"
        );
    }

    #[test]
    fn counts_selector_combinations_without_truncation() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        let count = selector_total_count_for(source, "install-hint").unwrap();
        assert_eq!(count, 6);
    }

    #[test]
    fn renders_hover_markdown_for_selector_preview() {
        let rendered = render_hover_markdown(&MessagePreview {
            selectors: vec![
                ("$gender".to_string(), "female".to_string()),
                ("$count".to_string(), "*".to_string()),
            ],
            text: "Copy the download link for her account on { $count } devices now.".to_string(),
        });
        assert_eq!(
            rendered,
            "`$gender=female`, `$count=*`\n\n```ftl\nCopy the download link for her account on { $count } devices now.\n```"
        );
    }

    #[test]
    fn renders_fluent_source_with_free_and_inline_comments() {
        let source = "### Shared menu copy\n## File menu\n# Primary action\nmenu-save =\n    .label = Save\n";
        let rendered = render_fluent_source(source, "menu-save.label").unwrap();
        assert_eq!(
            rendered,
            SourceRender {
                comments: Some(
                    "### Shared menu copy\n## File menu\n# Primary action\n".to_string()
                ),
                source: ".label = Save\n".to_string(),
            }
        );
    }

    #[test]
    fn renders_source_inlay_hint_label_for_message_and_attribute_selectors() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n\ndownload-action =\n    .tooltip =\n        Install the recommended build for { $gender ->\n            [female] her\n            [male] his\n           *[other] their\n        } account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n";

        assert_eq!(
            render_source_inlay_hint_label(source, "install-hint").unwrap(),
            "[gender=*, count=*] Copy the download link for their account on { $count } devices now."
        );
        assert_eq!(
            render_source_inlay_hint_label(source, "download-action.tooltip").unwrap(),
            "[gender=*, count=*] Install the recommended build for their account on { $count } devices now."
        );
    }

    #[test]
    fn render_message_preview_uses_selected_selector_context() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";
        let resource = parse_fluent_resource(source);
        let pattern = find_fluent_pattern(&resource, "install-hint").unwrap();
        let overrides = HashMap::from([("$gender".to_string(), "female".to_string())]);

        assert_eq!(
            render_message_preview(pattern, Some(&overrides)),
            MessagePreview {
                selectors: vec![
                    ("$gender".to_string(), "female".to_string()),
                    ("$count".to_string(), "*".to_string()),
                ],
                text: "Copy the download link for her account on { $count } devices now."
                    .to_string(),
            }
        );
    }

    #[test]
    fn selector_overrides_follow_active_variant_lines() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        assert_eq!(
            selector_overrides_for_position(
                source,
                "install-hint",
                Position::new(2, 18)
            ),
            HashMap::from([("$gender".to_string(), "female".to_string())])
        );
        assert_eq!(
            selector_overrides_for_position(
                source,
                "install-hint",
                Position::new(0, 3)
            ),
            HashMap::new()
        );
    }

    #[test]
    fn selector_overrides_preserve_variants_after_closing_brace_on_current_line() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        assert_eq!(
            selector_overrides_for_position(
                source,
                "install-hint",
                Position::new(5, 8)
            ),
            HashMap::from([("$gender".to_string(), "other".to_string())])
        );
    }

    #[test]
    fn selector_combination_document_omits_duplicate_source_sections_for_origin_files() {
        let file_match = FileMatch {
            mask_index: 0,
            language: "en".to_string(),
            filepath: "app".to_string(),
        };
        let source_render = SourceRender {
            comments: None,
            source: "install-hint = Example\n".to_string(),
        };

        let rendered = render_selector_combinations_document(
            "install-hint",
            &file_match,
            "en",
            Some(&source_render),
            Some("Source language combinations:"),
            Some(&source_render),
            "Current language combinations:",
        );
        assert!(!rendered.contains("Source language:"));
        assert!(!rendered.contains("Source language combinations:"));
        assert_eq!(rendered.matches("Current language combinations:").count(), 1);
    }

    #[test]
    fn local_fluent_syntax_fork_exposes_identifier_spans() {
        let resource = parser::parse("welcome-title = Welcome\n").unwrap();
        match &resource.body[0] {
            Entry::Message(message) => assert_eq!(message.id.span, 0..13),
            _ => panic!("expected message entry"),
        }
    }
}
