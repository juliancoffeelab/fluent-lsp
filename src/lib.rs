use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range as ByteRange;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use fluent_syntax::ast::{Entry, Resource};
use fluent_syntax::parser;
use fluent_syntax::serializer;
use icu::locale::Locale;
use icu::plurals::{PluralCategory, PluralRules};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tempfile::Builder as TempFileBuilder;
use tokio::sync::{RwLock, mpsc, oneshot};
use tower_lsp::jsonrpc::{Error as LspError, Result as LspResult};
use tower_lsp::ls_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CodeLens, CodeLensOptions, CodeLensParams,
    Command, CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams,
    CompletionResponse, Diagnostic, DiagnosticSeverity, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentChanges, Documentation, ExecuteCommandOptions,
    ExecuteCommandParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    Location, MarkupContent, MarkupKind, MessageType, OneOf, OneOf3,
    OptionalVersionedTextDocumentIdentifier, Position, ProgressToken, Range, ReferenceParams,
    ServerCapabilities, SetTraceParams, ShowDocumentParams, SnippetTextEdit, TextDocumentEdit,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, TextEdit, TraceValue, Uri, WorkspaceEdit,
};
use tower_lsp::{Client, LanguageServer};

const CONFIG_FILE_NAMES: [&str; 2] = ["fluent-lsp.toml", ".fluent-lsp.toml"];
const SHOW_MESSAGE_SELECTOR_COMBINATIONS_LIMIT: usize = 10;
const SHOW_SELECTOR_COMBINATIONS_COMMAND: &str = "fluent-lsp.showSelectorCombinations";
const LSP_COPY_MARKER: &str = "# [LSP-COPY]";
const LSP_COPY_MARKER_PREFIX: &str = "# [LSP-COPY .";
const BACKGROUND_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SelectorStyle {
    Prefix,
    Suffix,
    Whole,
}

impl Default for SelectorStyle {
    fn default() -> Self {
        Self::Prefix
    }
}

impl SelectorStyle {
    fn label(self) -> &'static str {
        match self {
            Self::Prefix => "prefix",
            Self::Suffix => "suffix",
            Self::Whole => "whole",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorRewriteKind {
    Prefix,
    Suffix,
    Whole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LspCopyMarkerKind {
    Message,
    Attribute(String),
}

impl SelectorRewriteKind {
    fn title(self) -> &'static str {
        match self {
            Self::Prefix => "Convert selector to prefix form",
            Self::Suffix => "Convert selector to suffix form",
            Self::Whole => "Convert selector to whole form",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    origin_language: String,
    #[serde(default)]
    file_masks: Vec<String>,
    #[serde(default)]
    selector_style: Option<SelectorStyle>,
    #[serde(default)]
    error_on_unsupported_plural_categories: Option<bool>,
    #[serde(default)]
    warn_on_missing_plural_categories: Option<bool>,
    #[serde(default)]
    warn_on_selector_style_mismatch: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    root_dir: PathBuf,
    origin_language: String,
    file_masks: Vec<FileMask>,
    selector_style: Option<SelectorStyle>,
    error_on_unsupported_plural_categories: Option<bool>,
    warn_on_missing_plural_categories: Option<bool>,
    warn_on_selector_style_mismatch: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ClientConfig {
    selector_style: Option<SelectorStyle>,
    error_on_unsupported_plural_categories: Option<bool>,
    warn_on_missing_plural_categories: Option<bool>,
    warn_on_selector_style_mismatch: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EffectiveDiagnosticConfig {
    error_on_unsupported_plural_categories: bool,
    warn_on_missing_plural_categories: bool,
    warn_on_selector_style_mismatch: bool,
    preferred_selector_style: Option<SelectorStyle>,
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
            selector_style: config.selector_style,
            error_on_unsupported_plural_categories: config.error_on_unsupported_plural_categories,
            warn_on_missing_plural_categories: config.warn_on_missing_plural_categories,
            warn_on_selector_style_mismatch: config.warn_on_selector_style_mismatch,
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

    #[cfg(test)]
    fn origin_file_for(&self, path: &Path) -> Option<PathBuf> {
        let file_match = self.file_match(path)?;
        Some(self.render_path(
            file_match.mask_index,
            &self.origin_language,
            &file_match.filepath,
        ))
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

    #[cfg(test)]
    fn render_path(&self, mask_index: usize, language: &str, filepath: &str) -> PathBuf {
        let relative = self.file_masks[mask_index]
            .raw
            .replace("{lang}", language)
            .replace("{filepath}", filepath);
        self.root_dir.join(relative)
    }

    fn discovery_roots(&self) -> Vec<PathBuf> {
        let mut roots = self
            .file_masks
            .iter()
            .map(|mask| self.root_dir.join(mask.discovery_root()))
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        roots
    }

    fn relative_path(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root_dir).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }

    fn selector_style(&self) -> Option<SelectorStyle> {
        self.selector_style
    }

    fn effective_diagnostic_config(&self, client: ClientConfig) -> EffectiveDiagnosticConfig {
        EffectiveDiagnosticConfig {
            error_on_unsupported_plural_categories: self
                .error_on_unsupported_plural_categories
                .or(client.error_on_unsupported_plural_categories)
                .unwrap_or(false),
            warn_on_missing_plural_categories: self
                .warn_on_missing_plural_categories
                .or(client.warn_on_missing_plural_categories)
                .unwrap_or(false),
            warn_on_selector_style_mismatch: self
                .warn_on_selector_style_mismatch
                .or(client.warn_on_selector_style_mismatch)
                .unwrap_or(false),
            preferred_selector_style: Some(
                self.selector_style
                    .or(client.selector_style)
                    .unwrap_or_default(),
            ),
        }
    }
}

impl FileMask {
    fn discovery_root(&self) -> PathBuf {
        let first_placeholder = self
            .raw
            .find('{')
            .expect("compiled file mask should contain placeholders");
        let prefix = &self.raw[..first_placeholder];
        let directory = prefix
            .rsplit_once('/')
            .map(|(directory, _)| directory)
            .unwrap_or("");
        if directory.is_empty() {
            PathBuf::new()
        } else {
            PathBuf::from(directory)
        }
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

fn collect_matching_files(workspace: &WorkspaceConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in workspace.discovery_roots() {
        collect_files_under(&root, &mut files);
    }
    files.retain(|path| workspace.file_match(path).is_some());
    files.sort();
    files.dedup();
    files
}

fn disk_snapshot(workspace: &WorkspaceConfig) -> HashMap<PathBuf, SystemTime> {
    collect_matching_files(workspace)
        .into_iter()
        .filter_map(|path| {
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect()
}

#[cfg(test)]
fn collect_translation_files(workspace: &WorkspaceConfig) -> Vec<PathBuf> {
    let mut files = collect_matching_files(workspace);
    files.retain(|path| workspace.matches_translation_file(path));
    files.sort();
    files
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct VariablePlaceable {
    name: String,
    reference_span: ByteRange<usize>,
    selection_span: ByteRange<usize>,
    anchor_span: ByteRange<usize>,
    container_span: ByteRange<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionSelectorTarget {
    selector_text: String,
    selection_span: ByteRange<usize>,
    anchor_span: ByteRange<usize>,
    container_span: ByteRange<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GenerateSelectorTarget {
    key: String,
    pattern_span: ByteRange<usize>,
    variables: Vec<VariablePlaceable>,
    selected_variable: Option<VariablePlaceable>,
    functions: Vec<FunctionSelectorTarget>,
    selected_function: Option<FunctionSelectorTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectorRewriteAction {
    kind: SelectorRewriteKind,
    replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VariableBinding {
    Concrete(String),
    Placeholder(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompletionSite {
    MessageKey { prefix: String },
    AttributeKey { message_key: String, prefix: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OriginMessageTemplate {
    key: String,
    has_value: bool,
    attributes: Vec<OriginAttributeTemplate>,
    source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OriginAttributeTemplate {
    key: String,
    source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MissingTranslationEntries {
    missing_messages: Vec<OriginMessageTemplate>,
    missing_attributes: Vec<MissingAttributePatch>,
}

#[derive(Debug, Clone)]
struct IndexedFile {
    path: PathBuf,
    file_match: FileMatch,
    source: String,
    definitions: HashMap<String, Range>,
    keys: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
struct WorkspaceIndex {
    files: HashMap<PathBuf, IndexedFile>,
    by_identity: HashMap<(usize, String, String), PathBuf>,
}

impl WorkspaceIndex {
    #[cfg(test)]
    fn build(workspace: &WorkspaceConfig, overlays: &HashMap<Uri, String>) -> Self {
        let mut paths = collect_matching_files(workspace);
        for uri in overlays.keys() {
            let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
                continue;
            };
            if workspace.file_match(&path).is_some() && !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths.sort();
        paths.dedup();

        let mut index = Self::default();
        for path in paths {
            index.replace_file(workspace, &path, overlays);
        }
        index
    }

    fn apply_disk_changes(
        &mut self,
        workspace: &WorkspaceConfig,
        overlays: &HashMap<Uri, String>,
        changed_or_added: &[PathBuf],
        removed: &[PathBuf],
    ) {
        for path in removed {
            let Some(uri) = Uri::from_file_path(&path) else {
                continue;
            };
            if overlays.contains_key(&uri) {
                continue;
            }
            self.remove_file(path);
        }
        for path in changed_or_added {
            self.replace_file(workspace, path, overlays);
        }
    }

    fn replace_file(
        &mut self,
        workspace: &WorkspaceConfig,
        path: &Path,
        overlays: &HashMap<Uri, String>,
    ) {
        self.remove_file(path);
        let Some(file_match) = workspace.file_match(path) else {
            return;
        };
        let source = Uri::from_file_path(path)
            .and_then(|uri| overlays.get(&uri).cloned())
            .or_else(|| std::fs::read_to_string(path).ok());
        let Some(source) = source else {
            return;
        };
        let entry = index_fluent_file(path.to_path_buf(), file_match, source);
        self.by_identity.insert(
            (
                entry.file_match.mask_index,
                entry.file_match.language.clone(),
                entry.file_match.filepath.clone(),
            ),
            entry.path.clone(),
        );
        self.files.insert(entry.path.clone(), entry);
    }

    fn remove_file(&mut self, path: &Path) {
        if let Some(existing) = self.files.remove(path) {
            self.by_identity.remove(&(
                existing.file_match.mask_index,
                existing.file_match.language,
                existing.file_match.filepath,
            ));
        }
    }

    fn file(&self, path: &Path) -> Option<&IndexedFile> {
        self.files.get(path)
    }

    fn origin_for<'a>(
        &'a self,
        workspace: &WorkspaceConfig,
        file: &IndexedFile,
    ) -> Option<&'a IndexedFile> {
        let path = self.by_identity.get(&(
            file.file_match.mask_index,
            workspace.origin_language.clone(),
            file.file_match.filepath.clone(),
        ))?;
        self.files.get(path)
    }

    fn translations_for<'a>(
        &'a self,
        workspace: &WorkspaceConfig,
        origin: &IndexedFile,
    ) -> Vec<&'a IndexedFile> {
        let mut files = self
            .files
            .values()
            .filter(|file| {
                file.file_match.mask_index == origin.file_match.mask_index
                    && file.file_match.filepath == origin.file_match.filepath
                    && file.file_match.language != workspace.origin_language
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files
    }

    #[cfg(test)]
    fn local_only_files(&self, workspace: &WorkspaceConfig) -> Vec<&IndexedFile> {
        let mut files = self
            .files
            .values()
            .filter(|file| {
                file.file_match.language != workspace.origin_language
                    && self.origin_for(workspace, file).is_none()
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files
    }
}

fn index_fluent_file(path: PathBuf, file_match: FileMatch, source: String) -> IndexedFile {
    let (resource, _parse_errors) = parse_fluent_resource_with_errors(&source);
    let keys = collect_fluent_keys_from_resource(&resource)
        .into_iter()
        .collect::<HashSet<_>>();
    let definitions = keys
        .iter()
        .filter_map(|key| {
            find_fluent_definition_span(&resource, key)
                .and_then(|span| byte_range_to_lsp_range(&source, span))
                .map(|range| (key.clone(), range))
        })
        .collect::<HashMap<_, _>>();
    IndexedFile {
        path,
        file_match,
        source,
        definitions,
        keys,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MissingAttributePatch {
    message_key: String,
    attributes: Vec<OriginAttributeTemplate>,
}

#[derive(Default)]
struct ServerState {
    root_dir: Option<PathBuf>,
    workspace: Option<WorkspaceConfig>,
    index_tx: Option<mpsc::UnboundedSender<IndexMessage>>,
    open_documents: HashMap<Uri, String>,
    supports_show_document: bool,
    supports_snippet_text_edits: bool,
    supports_work_done_progress: bool,
    trace: TraceValue,
    client_config: ClientConfig,
}

#[derive(Debug)]
struct DiskRefreshEvent {
    changed_or_added: Vec<PathBuf>,
    removed: Vec<PathBuf>,
}

#[derive(Debug)]
struct IndexedDiagnosticUpdate {
    batches: Vec<(Uri, Vec<Diagnostic>)>,
}

enum IndexMessage {
    SetClientConfig {
        client_config: ClientConfig,
    },
    ReplaceOverlay {
        path: PathBuf,
        text: String,
    },
    DropOverlay {
        path: PathBuf,
    },
    ApplyDiskRefresh {
        event: DiskRefreshEvent,
    },
    Definition {
        path: PathBuf,
        position: Position,
        reply: oneshot::Sender<Option<Location>>,
    },
    References {
        path: PathBuf,
        uri: Uri,
        position: Position,
        include_declaration: bool,
        reply: oneshot::Sender<Option<Vec<Location>>>,
    },
    Hover {
        path: PathBuf,
        position: Position,
        reply: oneshot::Sender<LspResult<Option<Hover>>>,
    },
    Completion {
        path: PathBuf,
        position: Position,
        reply: oneshot::Sender<LspResult<Option<CompletionResponse>>>,
    },
    OriginSource {
        path: PathBuf,
        reply: oneshot::Sender<Option<String>>,
    },
    IndexedTranslationDiagnostics {
        path: PathBuf,
        reply: oneshot::Sender<Vec<Diagnostic>>,
    },
    IndexedTranslationDiagnosticBatches {
        reply: oneshot::Sender<Vec<(Uri, Vec<Diagnostic>)>>,
    },
}

struct IndexActor {
    workspace: WorkspaceConfig,
    index: WorkspaceIndex,
    overlays: HashMap<PathBuf, String>,
    client_config: ClientConfig,
    notification_tx: mpsc::UnboundedSender<IndexedDiagnosticUpdate>,
}

pub struct Backend {
    client: Client,
    state: Arc<RwLock<ServerState>>,
}

impl IndexActor {
    fn new(
        workspace: WorkspaceConfig,
        index: WorkspaceIndex,
        overlays: HashMap<PathBuf, String>,
        client_config: ClientConfig,
        notification_tx: mpsc::UnboundedSender<IndexedDiagnosticUpdate>,
    ) -> Self {
        Self {
            workspace,
            index,
            overlays,
            client_config,
            notification_tx,
        }
    }

    fn indexed_translation_diagnostics_for(&self, path: &Path) -> Vec<Diagnostic> {
        let Some(file) = self.index.file(path) else {
            return Vec::new();
        };
        if file.file_match.language == self.workspace.origin_language {
            return Vec::new();
        }
        match self.index.origin_for(&self.workspace, file) {
            Some(origin) => collect_translation_counterpart_diagnostics(file, origin),
            None => vec![local_only_file_diagnostic(file)],
        }
    }

    fn indexed_translation_warning_paths(&self) -> HashSet<PathBuf> {
        self.index
            .files
            .values()
            .filter(|file| file.file_match.language != self.workspace.origin_language)
            .filter(|file| {
                !self
                    .indexed_translation_diagnostics_for(&file.path)
                    .is_empty()
            })
            .map(|file| file.path.clone())
            .collect()
    }

    fn diagnostic_batch_for_path(&self, path: &Path) -> Option<(Uri, Vec<Diagnostic>)> {
        let file = self.index.file(path)?;
        let uri = Uri::from_file_path(path)?;
        let settings = self
            .workspace
            .effective_diagnostic_config(self.client_config);
        let mut diagnostics =
            collect_document_diagnostics(&file.source, &file.file_match.language, settings);
        diagnostics.extend(self.indexed_translation_diagnostics_for(path));
        Some((uri, diagnostics))
    }

    fn indexed_translation_diagnostic_batches(&self) -> Vec<(Uri, Vec<Diagnostic>)> {
        let mut paths = self
            .indexed_translation_warning_paths()
            .into_iter()
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .filter_map(|path| self.diagnostic_batch_for_path(&path))
            .collect()
    }

    fn update_overlay(&mut self, path: PathBuf, text: String) {
        self.overlays.insert(path.clone(), text);
        self.index
            .replace_file(&self.workspace, &path, &self.pathbuf_overlays_as_uri_map());
    }

    fn drop_overlay(&mut self, path: PathBuf) {
        self.overlays.remove(&path);
        self.index
            .replace_file(&self.workspace, &path, &self.pathbuf_overlays_as_uri_map());
    }

    fn apply_disk_refresh(&mut self, event: DiskRefreshEvent) {
        let previous_warning_paths = self.indexed_translation_warning_paths();
        self.index.apply_disk_changes(
            &self.workspace,
            &self.pathbuf_overlays_as_uri_map(),
            &event.changed_or_added,
            &event.removed,
        );
        let current_warning_paths = self.indexed_translation_warning_paths();
        let mut affected_paths = previous_warning_paths
            .union(&current_warning_paths)
            .cloned()
            .collect::<Vec<_>>();
        affected_paths.sort();
        let batches = affected_paths
            .into_iter()
            .filter_map(|path| self.diagnostic_batch_for_path(&path))
            .collect::<Vec<_>>();
        let _ = self
            .notification_tx
            .send(IndexedDiagnosticUpdate { batches });
    }

    fn pathbuf_overlays_as_uri_map(&self) -> HashMap<Uri, String> {
        self.overlays
            .iter()
            .filter_map(|(path, text)| Some((Uri::from_file_path(path)?, text.clone())))
            .collect()
    }

    fn definition(&self, path: &Path, position: Position) -> Option<Location> {
        if !self.workspace.matches_translation_file(path) {
            return None;
        }
        let file = self.index.file(path)?;
        let key = extract_definition_key(&file.source, path, position)?;
        let origin_file = self.index.origin_for(&self.workspace, file)?;
        if !origin_file.keys.contains(&key) {
            return None;
        }
        Some(Location {
            uri: Uri::from_file_path(&origin_file.path)?,
            range: *origin_file.definitions.get(&key)?,
        })
    }

    fn references(
        &self,
        path: &Path,
        uri: &Uri,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        if !self.workspace.is_origin_file(path) {
            return None;
        }
        let origin_file = self.index.file(path)?;
        let key = extract_definition_key(&origin_file.source, path, position)?;
        let mut references = Vec::new();
        if include_declaration {
            references.push(Location {
                uri: uri.clone(),
                range: *origin_file.definitions.get(&key)?,
            });
        }
        for translation_file in self.index.translations_for(&self.workspace, origin_file) {
            let Some(range) = translation_file.definitions.get(&key).copied() else {
                continue;
            };
            let Some(uri) = Uri::from_file_path(&translation_file.path) else {
                continue;
            };
            references.push(Location { uri, range });
        }
        Some(references)
    }

    fn hover(&self, path: &Path, position: Position) -> LspResult<Option<Hover>> {
        let Some(file) = self.index.file(path) else {
            return Ok(None);
        };
        let Some(key) = extract_definition_key(&file.source, path, position) else {
            return Ok(None);
        };
        let Some(hover_range) = file.definitions.get(&key).copied() else {
            return Ok(None);
        };
        if range_contains_position(&hover_range, position) {
            let current_comments = render_fluent_source(&file.source, &key)
                .and_then(|rendered| rendered.comments)
                .filter(|comments| !comments.is_empty());
            let origin_comments = if !self.workspace.is_origin_file(path) {
                self.index
                    .origin_for(&self.workspace, file)
                    .and_then(|origin_file| {
                        render_fluent_source(&origin_file.source, &key)
                            .and_then(|rendered| rendered.comments)
                            .filter(|comments| !comments.is_empty())
                    })
            } else {
                None
            };
            if let Some(value) = render_hover_comment_markdown(
                origin_comments.as_deref(),
                current_comments.as_deref(),
            ) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: Some(hover_range),
                }));
            }
            return Ok(None);
        }
        let resource = parse_fluent_resource(&file.source);
        let Some(pattern) = find_fluent_pattern(&resource, &key) else {
            return Ok(None);
        };
        let selector_overrides = selector_overrides_for_position(&file.source, &key, position);
        let current_preview = render_message_preview(pattern, Some(&selector_overrides));
        let source_preview = if !self.workspace.is_origin_file(path)
            && !range_contains_position(&hover_range, position)
        {
            let origin_file = self
                .index
                .origin_for(&self.workspace, file)
                .ok_or_else(|| {
                    internal_error_with_message(format!(
                        "failed to resolve origin counterpart for {}",
                        path.display()
                    ))
                })?;
            let origin_resource = parse_fluent_resource(&origin_file.source);
            find_fluent_pattern(&origin_resource, &key).map(|origin_pattern| {
                render_message_preview(origin_pattern, Some(&selector_overrides))
            })
        } else {
            None
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: render_hover_markdown(source_preview.as_ref(), &current_preview),
            }),
            range: Some(hover_range),
        }))
    }

    fn completion(&self, path: &Path, position: Position) -> LspResult<Option<CompletionResponse>> {
        let Some(file) = self.index.file(path) else {
            return Ok(None);
        };
        if file.file_match.language == self.workspace.origin_language {
            return Ok(Some(CompletionResponse::Array(Vec::new())));
        }
        let Some(site) = completion_site_for_position(&file.source, position) else {
            return Ok(None);
        };
        let Some(origin_file) = self.index.origin_for(&self.workspace, file) else {
            return Ok(Some(CompletionResponse::Array(Vec::new())));
        };
        Ok(Some(CompletionResponse::Array(completion_items_for_site(
            &origin_file.source,
            &site,
        ))))
    }

    fn origin_source(&self, path: &Path) -> Option<String> {
        if self.workspace.is_origin_file(path) {
            return self.index.file(path).map(|file| file.source.clone());
        }
        let file = self.index.file(path)?;
        self.index
            .origin_for(&self.workspace, file)
            .map(|origin_file| origin_file.source.clone())
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<IndexMessage>) {
        while let Some(message) = rx.recv().await {
            match message {
                IndexMessage::SetClientConfig { client_config } => {
                    self.client_config = client_config;
                }
                IndexMessage::ReplaceOverlay { path, text } => self.update_overlay(path, text),
                IndexMessage::DropOverlay { path } => self.drop_overlay(path),
                IndexMessage::ApplyDiskRefresh { event } => self.apply_disk_refresh(event),
                IndexMessage::Definition {
                    path,
                    position,
                    reply,
                } => {
                    let _ = reply.send(self.definition(&path, position));
                }
                IndexMessage::References {
                    path,
                    uri,
                    position,
                    include_declaration,
                    reply,
                } => {
                    let _ = reply.send(self.references(&path, &uri, position, include_declaration));
                }
                IndexMessage::Hover {
                    path,
                    position,
                    reply,
                } => {
                    let _ = reply.send(self.hover(&path, position));
                }
                IndexMessage::Completion {
                    path,
                    position,
                    reply,
                } => {
                    let _ = reply.send(self.completion(&path, position));
                }
                IndexMessage::OriginSource { path, reply } => {
                    let _ = reply.send(self.origin_source(&path));
                }
                IndexMessage::IndexedTranslationDiagnostics { path, reply } => {
                    let _ = reply.send(self.indexed_translation_diagnostics_for(&path));
                }
                IndexMessage::IndexedTranslationDiagnosticBatches { reply } => {
                    let _ = reply.send(self.indexed_translation_diagnostic_batches());
                }
            }
        }
    }
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(ServerState::default())),
        }
    }

    pub async fn set_trace(&self, params: SetTraceParams) {
        let trace_label = match params.value {
            TraceValue::Off => "off",
            TraceValue::Messages => "messages",
            TraceValue::Verbose => "verbose",
        };
        self.state.write().await.trace = params.value;
        self.client
            .send_notification::<tower_lsp::ls_types::notification::LogTrace>(
                tower_lsp::ls_types::LogTraceParams {
                    message: format!("fluent-lsp trace updated value={trace_label}"),
                    verbose: None,
                },
            )
            .await;
    }

    async fn log_trace_timing(&self, operation: &str, elapsed: Duration, verbose: Option<String>) {
        let trace = self.state.read().await.trace;
        if trace == TraceValue::Off {
            return;
        }

        let message = format!(
            "fluent-lsp timing operation={operation} elapsed_ms={:.3}",
            elapsed.as_secs_f64() * 1000.0
        );
        let verbose = if trace == TraceValue::Verbose {
            verbose
        } else {
            None
        };
        self.client
            .send_notification::<tower_lsp::ls_types::notification::LogTrace>(
                tower_lsp::ls_types::LogTraceParams { message, verbose },
            )
            .await;
    }

    async fn index_sender(&self) -> Option<mpsc::UnboundedSender<IndexMessage>> {
        self.state.read().await.index_tx.clone()
    }

    async fn send_index_message(&self, message: IndexMessage) {
        let Some(tx) = self.index_sender().await else {
            return;
        };
        let _ = tx.send(message);
    }

    async fn spawn_index_actor(
        &self,
        workspace: WorkspaceConfig,
        index: WorkspaceIndex,
        snapshot: HashMap<PathBuf, SystemTime>,
        overlays: HashMap<Uri, String>,
        client_config: ClientConfig,
    ) {
        let overlay_paths = overlays
            .iter()
            .filter_map(|(uri, text)| Some((uri.to_file_path()?.into_owned(), text.clone())))
            .collect::<HashMap<_, _>>();
        let (index_tx, index_rx) = mpsc::unbounded_channel();
        let (notification_tx, mut notification_rx) = mpsc::unbounded_channel();
        tokio::spawn(
            IndexActor::new(
                workspace.clone(),
                index,
                overlay_paths,
                client_config,
                notification_tx,
            )
            .run(index_rx),
        );

        let client = self.client.clone();
        tokio::spawn(async move {
            while let Some(update) = notification_rx.recv().await {
                for (uri, diagnostics) in update.batches {
                    client.publish_diagnostics(uri, diagnostics, None).await;
                }
            }
        });

        self.spawn_background_refresh_task(workspace, snapshot, index_tx.clone());
        let mut state = self.state.write().await;
        state.index_tx = Some(index_tx);
    }

    fn spawn_background_refresh_task(
        &self,
        workspace: WorkspaceConfig,
        mut previous_snapshot: HashMap<PathBuf, SystemTime>,
        index_tx: mpsc::UnboundedSender<IndexMessage>,
    ) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(BACKGROUND_REFRESH_INTERVAL);
            loop {
                interval.tick().await;
                let next_snapshot = disk_snapshot(&workspace);
                if next_snapshot == previous_snapshot {
                    continue;
                }
                let changed_or_added = next_snapshot
                    .iter()
                    .filter_map(|(path, modified)| {
                        if previous_snapshot.get(path) == Some(modified) {
                            None
                        } else {
                            Some(path.clone())
                        }
                    })
                    .collect::<Vec<_>>();
                let removed = previous_snapshot
                    .keys()
                    .filter(|path| !next_snapshot.contains_key(*path))
                    .cloned()
                    .collect::<Vec<_>>();
                previous_snapshot = next_snapshot.clone();
                if index_tx
                    .send(IndexMessage::ApplyDiskRefresh {
                        event: DiskRefreshEvent {
                            changed_or_added,
                            removed,
                        },
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    async fn request_index<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<T>) -> IndexMessage,
    ) -> Option<T> {
        let Some(tx) = self.index_sender().await else {
            return None;
        };
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = tx.send(make(reply_tx));
        reply_rx.await.ok()
    }

    async fn rebuild_index_with_progress(&self) {
        let started = Instant::now();
        let (workspace, overlays, supports_progress, client_config) = {
            let state = self.state.read().await;
            let Some(workspace) = state.workspace.clone() else {
                return;
            };
            (
                workspace,
                state.open_documents.clone(),
                state.supports_work_done_progress,
                state.client_config,
            )
        };

        let paths = collect_matching_files(&workspace);
        let total = paths.len();
        let snapshot = paths
            .iter()
            .filter_map(|path| {
                let modified = std::fs::metadata(path).ok()?.modified().ok()?;
                Some((path.clone(), modified))
            })
            .collect::<HashMap<_, _>>();

        let progress = if supports_progress {
            Some(
                self.client
                    .progress(
                        ProgressToken::String("fluent-lsp-index".to_string()),
                        "Index Fluent workspace",
                    )
                    .with_percentage(0)
                    .with_message(format!("0/{total} files"))
                    .begin()
                    .await,
            )
        } else {
            None
        };

        let mut index = WorkspaceIndex::default();
        for (offset, path) in paths.iter().enumerate() {
            if let Some(progress) = &progress {
                let percent = if total == 0 {
                    100
                } else {
                    (((offset + 1) * 100) / total) as u32
                };
                let display = workspace
                    .relative_path(path)
                    .unwrap_or_else(|| path.display().to_string());
                progress
                    .report_with_message(
                        format!("{}/{} files: {display}", offset + 1, total),
                        percent,
                    )
                    .await;
            }
            index.replace_file(&workspace, path, &overlays);
        }

        self.spawn_index_actor(workspace, index, snapshot, overlays, client_config)
            .await;

        if let Some(progress) = progress {
            progress
                .finish_with_message(format!("Indexed {total} Fluent files"))
                .await;
        }
        self.log_trace_timing(
            "workspace/index",
            started.elapsed(),
            Some(format!("files={total}")),
        )
        .await;
    }

    async fn replace_indexed_file(&self, uri: &Uri, text: String) {
        let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
            return;
        };
        self.send_index_message(IndexMessage::ReplaceOverlay { path, text })
            .await;
    }

    async fn drop_indexed_overlay(&self, uri: &Uri) {
        let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
            return;
        };
        self.send_index_message(IndexMessage::DropOverlay { path })
            .await;
    }

    async fn publish_indexed_translation_diagnostics(&self) {
        let Some(batches) = self
            .request_index(|reply| IndexMessage::IndexedTranslationDiagnosticBatches { reply })
            .await
        else {
            return;
        };
        for (uri, diagnostics) in batches {
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
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

    async fn publish_document_diagnostics(&self, uri: &Uri) {
        let (workspace, client_config, source, language) = {
            let state = self.state.read().await;
            let Some(workspace) = state.workspace.clone() else {
                return;
            };
            let Some(source) = state.open_documents.get(uri).cloned().or_else(|| {
                uri.to_file_path()
                    .and_then(|path| std::fs::read_to_string(path.as_ref()).ok())
            }) else {
                return;
            };
            let Some(path) = uri.to_file_path() else {
                return;
            };
            let Some(file_match) = workspace.file_match(path.as_ref()) else {
                return;
            };
            (workspace, state.client_config, source, file_match.language)
        };

        let settings = workspace.effective_diagnostic_config(client_config);
        let mut diagnostics = collect_document_diagnostics(&source, &language, settings);
        let indexed_translation = match uri.to_file_path().map(|path| path.into_owned()) {
            Some(path) => self
                .request_index(|reply| IndexMessage::IndexedTranslationDiagnostics { path, reply })
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };
        diagnostics.extend(indexed_translation);
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }

    async fn definition_for(&self, params: GotoDefinitionParams) -> Option<Location> {
        let uri = params.text_document_position_params.text_document.uri;
        let path = uri.to_file_path()?.into_owned();
        self.request_index(|reply| IndexMessage::Definition {
            path,
            position: params.text_document_position_params.position,
            reply,
        })
        .await
        .flatten()
    }

    async fn references_for(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let uri = params.text_document_position.text_document.uri;
        let path = uri.to_file_path()?.into_owned();
        self.request_index(|reply| IndexMessage::References {
            path,
            uri,
            position: params.text_document_position.position,
            include_declaration: params.context.include_declaration,
            reply,
        })
        .await
        .flatten()
    }

    async fn hover_for(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let path = params
            .text_document_position_params
            .text_document
            .uri
            .to_file_path()
            .ok_or_else(|| LspError::invalid_params("expected a file URI"))?
            .into_owned();
        self.request_index(|reply| IndexMessage::Hover {
            path,
            position: params.text_document_position_params.position,
            reply,
        })
        .await
        .unwrap_or(Ok(None))
    }

    async fn completions_for(
        &self,
        params: CompletionParams,
    ) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let path = uri
            .to_file_path()
            .ok_or_else(|| LspError::invalid_params("expected a file URI"))?
            .into_owned();
        self.request_index(|reply| IndexMessage::Completion {
            path,
            position: params.text_document_position.position,
            reply,
        })
        .await
        .unwrap_or(Ok(None))
    }

    async fn code_actions_for(
        &self,
        params: CodeActionParams,
    ) -> LspResult<Option<CodeActionResponse>> {
        let state = self.state.read().await;
        let Some(workspace) = state.workspace.clone() else {
            return Ok(None);
        };
        let has_index = state.index_tx.is_some();
        let uri = params.text_document.uri;
        let path = uri
            .to_file_path()
            .ok_or_else(|| LspError::invalid_params("expected a file URI"))?
            .into_owned();
        let Some(file_match) = workspace.file_match(&path) else {
            return Ok(None);
        };

        let source = state
            .open_documents
            .get(&uri)
            .cloned()
            .or_else(|| std::fs::read_to_string(&path).ok())
            .ok_or_else(|| {
                internal_error_with_message(format!("failed to read {}", path.display()))
            })?;
        let style = workspace
            .selector_style()
            .or(state.client_config.selector_style)
            .unwrap_or_default();
        let supports_snippet_text_edits = state.supports_snippet_text_edits;
        drop(state);
        let origin_source = if workspace.matches_translation_file(&path) && has_index {
            self.request_index(|reply| IndexMessage::OriginSource {
                path: path.clone(),
                reply,
            })
            .await
            .flatten()
        } else {
            None
        };

        let mut actions = Vec::new();
        if let Some(origin_source) = origin_source.as_deref() {
            let missing = collect_missing_translation_entries(origin_source, &source);
            if !missing.is_empty() {
                if let Some(edit) = build_missing_entries_workspace_edit(
                    &uri,
                    &source,
                    &missing,
                    MissingEntryRenderMode::EmptyStub,
                ) {
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Add missing keys and attributes from source".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        edit: Some(edit),
                        ..CodeAction::default()
                    }));
                }
                if let Some(edit) = build_missing_entries_workspace_edit(
                    &uri,
                    &source,
                    &missing,
                    MissingEntryRenderMode::CopySource,
                ) {
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Copy missing keys and attributes from source".to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        edit: Some(edit),
                        ..CodeAction::default()
                    }));
                }
            }
            if let Some(action) = build_single_message_copy_code_action(
                &uri,
                &path,
                &source,
                origin_source,
                params.range.start,
            ) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        if let Some(target) = find_generate_selector_target(&source, &path, params.range.start) {
            let Some(edit_range) = byte_range_to_lsp_range(&source, target.pattern_span.clone())
            else {
                return Ok(None);
            };
            let ordered_styles = ordered_selector_styles(available_selector_styles(&target), style);
            for candidate_style in ordered_styles {
                let Some(generated) = generate_number_selector_edit(
                    &source,
                    &target,
                    &file_match.language,
                    candidate_style,
                    supports_snippet_text_edits,
                ) else {
                    continue;
                };
                let generated =
                    pad_assignment_replacement(&source, &target.pattern_span, generated);

                let edit = if supports_snippet_text_edits {
                    WorkspaceEdit {
                        changes: None,
                        document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                            text_document: OptionalVersionedTextDocumentIdentifier {
                                uri: uri.clone(),
                                version: None,
                            },
                            edits: vec![OneOf3::Right(SnippetTextEdit {
                                range: edit_range,
                                snippet: generated,
                                annotation_id: None,
                            })],
                        }])),
                        change_annotations: None,
                    }
                } else {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![TextEdit::new(edit_range, generated)]);
                    WorkspaceEdit {
                        changes: Some(changes),
                        document_changes: None,
                        change_annotations: None,
                    }
                };

                let title = if let Some(function) = &target.selected_function {
                    format!(
                        "Generate number selector from {} ({})",
                        function.selector_text,
                        candidate_style.label()
                    )
                } else if let Some(variable) = &target.selected_variable {
                    format!(
                        "Generate number selector from {} ({})",
                        variable.name,
                        candidate_style.label()
                    )
                } else {
                    format!("Generate number selector ({})", candidate_style.label())
                };

                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::REFACTOR_REWRITE),
                    edit: Some(edit),
                    is_preferred: Some(candidate_style == style),
                    ..CodeAction::default()
                }));
            }
        }

        if let Some((pattern_span, rewrite_actions)) =
            find_selector_rewrite_target(&source, &path, params.range.start)
        {
            let Some(edit_range) = byte_range_to_lsp_range(&source, pattern_span.clone()) else {
                return Ok(None);
            };
            for rewrite in rewrite_actions {
                let mut changes = HashMap::new();
                changes.insert(
                    uri.clone(),
                    vec![TextEdit::new(
                        edit_range,
                        pad_assignment_replacement(&source, &pattern_span, rewrite.replacement),
                    )],
                );
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: rewrite.kind.title().to_string(),
                    kind: Some(CodeActionKind::REFACTOR_REWRITE),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        document_changes: None,
                        change_annotations: None,
                    }),
                    ..CodeAction::default()
                }));
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn code_lenses_for(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        let state = self.state.read().await;
        let Some(workspace) = state.workspace.clone() else {
            return Ok(None);
        };
        let uri = params.text_document.uri;
        let path = uri
            .to_file_path()
            .ok_or_else(|| LspError::invalid_params("expected a file URI"))?
            .into_owned();
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
        let Ok(uri) = uri.parse::<Uri>() else {
            return self
                .respond_to_clicked_command_with_error(
                    LspError::invalid_params("document URI argument must be a valid URI"),
                    MessageType::ERROR,
                    "Selector combinations command received an invalid document URI",
                )
                .await;
        };
        let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
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
                self.request_index(|reply| IndexMessage::OriginSource {
                    path: path.clone(),
                    reply,
                })
                .await
                .flatten()
                .ok_or_else(|| {
                    internal_error_with_message(format!(
                        "failed to resolve origin counterpart for {}",
                        path.display()
                    ))
                })?
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

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        #[allow(deprecated)]
        let root_dir = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| {
                folders
                    .first()
                    .and_then(|folder| folder.uri.to_file_path().map(|path| path.into_owned()))
            })
            .or_else(|| {
                params
                    .root_uri
                    .and_then(|uri| uri.to_file_path().map(|path| path.into_owned()))
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
                .clone()
                .and_then(|window| window.show_document)
                .map(|capability| capability.support)
                .unwrap_or(false);
            state.supports_snippet_text_edits = params
                .capabilities
                .workspace
                .and_then(|workspace| workspace.workspace_edit)
                .is_some_and(|capability| {
                    capability.document_changes.unwrap_or(false)
                        && capability.snippet_edit_support.unwrap_or(false)
                });
            state.supports_work_done_progress = params
                .capabilities
                .window
                .clone()
                .and_then(|window| window.work_done_progress)
                .unwrap_or(false);
            state.trace = params.trace.unwrap_or_default();
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR_REWRITE,
                        ]),
                        resolve_provider: Some(false),
                        work_done_progress_options: Default::default(),
                    },
                )),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    ..CompletionOptions::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![SHOW_SELECTOR_COMBINATIONS_COMMAND.to_string()],
                    work_done_progress_options: Default::default(),
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..Default::default()
                    },
                )),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let (workspace, root) = {
            let state = self.state.read().await;
            (state.workspace.clone(), state.root_dir.clone())
        };
        match &workspace {
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
                self.rebuild_index_with_progress().await;
                self.publish_indexed_translation_diagnostics().await;
            }
            None => {
                let root = root
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
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.state
            .write()
            .await
            .open_documents
            .insert(uri.clone(), text.clone());
        self.replace_indexed_file(&uri, text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            let uri = params.text_document.uri;
            let text = change.text;
            self.state
                .write()
                .await
                .open_documents
                .insert(uri.clone(), text.clone());
            self.replace_indexed_file(&uri, text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.state.write().await.open_documents.remove(&uri);
        self.drop_indexed_overlay(&uri).await;
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(text) = params.text {
            self.state
                .write()
                .await
                .open_documents
                .insert(uri.clone(), text.clone());
            self.replace_indexed_file(&uri, text).await;
        }
        self.publish_document_diagnostics(&uri).await;
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let client_config = parse_client_config(&params.settings);
        {
            let mut state = self.state.write().await;
            state.client_config = client_config;
            if let Some(root_dir) = state.root_dir.clone() {
                state.workspace = WorkspaceConfig::load(root_dir).ok();
                state.index_tx = None;
            }
        }
        self.rebuild_index_with_progress().await;
        self.send_index_message(IndexMessage::SetClientConfig { client_config })
            .await;
        self.publish_indexed_translation_diagnostics().await;
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let started = Instant::now();
        let result = self
            .definition_for(params)
            .await
            .map(GotoDefinitionResponse::Scalar);
        self.log_trace_timing(
            "textDocument/definition",
            started.elapsed(),
            Some(format!("hit={}", result.is_some())),
        )
        .await;
        Ok(result)
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let started = Instant::now();
        let result = self.references_for(params).await;
        self.log_trace_timing(
            "textDocument/references",
            started.elapsed(),
            Some(format!(
                "count={}",
                result.as_ref().map(|items| items.len()).unwrap_or(0)
            )),
        )
        .await;
        Ok(result)
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let started = Instant::now();
        let result = self.hover_for(params).await;
        self.log_trace_timing(
            "textDocument/hover",
            started.elapsed(),
            Some(format!(
                "hit={}",
                result
                    .as_ref()
                    .ok()
                    .and_then(|hover| hover.as_ref())
                    .is_some()
            )),
        )
        .await;
        result
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let started = Instant::now();
        let result = self.completions_for(params).await;
        let count = result
            .as_ref()
            .ok()
            .and_then(|response| response.as_ref())
            .map(completion_response_len)
            .unwrap_or(0);
        self.log_trace_timing(
            "textDocument/completion",
            started.elapsed(),
            Some(format!("count={count}")),
        )
        .await;
        result
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let started = Instant::now();
        let result = self.code_actions_for(params).await;
        self.log_trace_timing(
            "textDocument/codeAction",
            started.elapsed(),
            Some(format!(
                "count={}",
                result
                    .as_ref()
                    .ok()
                    .and_then(|actions| actions.as_ref())
                    .map(|actions| actions.len())
                    .unwrap_or(0)
            )),
        )
        .await;
        result
    }

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        let started = Instant::now();
        let result = self.code_lenses_for(params).await;
        self.log_trace_timing(
            "textDocument/codeLens",
            started.elapsed(),
            Some(format!(
                "count={}",
                result
                    .as_ref()
                    .ok()
                    .and_then(|lenses| lenses.as_ref())
                    .map(|lenses| lenses.len())
                    .unwrap_or(0)
            )),
        )
        .await;
        result
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<Value>> {
        let started = Instant::now();
        let command = params.command.clone();
        let result = self.execute_selector_combinations_command(params).await;
        self.log_trace_timing(
            "workspace/executeCommand",
            started.elapsed(),
            Some(format!("command={command} ok={}", result.is_ok())),
        )
        .await;
        result
    }
}

fn completion_response_len(response: &CompletionResponse) -> usize {
    match response {
        CompletionResponse::Array(items) => items.len(),
        CompletionResponse::List(list) => list.items.len(),
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
        span: fluent_syntax::ast::Span::default(),
    });
    Some(rendered)
}

pub fn render_fluent_preview_text(
    source: &str,
    key: &str,
    selector_overrides: Option<&HashMap<String, String>>,
) -> Option<String> {
    let resource = parse_fluent_resource(source);
    let pattern = find_fluent_pattern(&resource, key)?;
    Some(render_message_preview(pattern, selector_overrides).text)
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

fn render_preview_markdown(preview: &MessagePreview) -> String {
    let mut sections = Vec::new();
    if !preview.selectors.is_empty() {
        sections.push(render_selector_assignments(&preview.selectors));
    }
    let text = if preview.text.is_empty() {
        "<empty>"
    } else {
        preview.text.as_str()
    };
    sections.push(render_ftl_block(text));
    sections.join("\n\n")
}

fn render_hover_markdown(
    origin_preview: Option<&MessagePreview>,
    current_preview: &MessagePreview,
) -> String {
    match origin_preview {
        Some(origin_preview) => [
            render_preview_markdown(origin_preview),
            "---".to_string(),
            render_preview_markdown(current_preview),
        ]
        .join("\n\n"),
        None => render_preview_markdown(current_preview),
    }
}

fn render_hover_comment_markdown(
    origin_comments: Option<&str>,
    current_comments: Option<&str>,
) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(origin_comments) = origin_comments {
        sections.push(render_ftl_block(origin_comments));
    }
    if let Some(current_comments) = current_comments {
        sections.push(render_ftl_block(current_comments));
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n---\n\n"))
    }
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

fn write_selector_combinations_temp_document(key: &str, contents: &str) -> Result<Uri, String> {
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
    Uri::from_file_path(&path)
        .ok_or_else(|| format!("failed to convert temp path to URI: {}", path.display()))
}

fn render_entry_source(entry: &Entry<&str>, key: &str) -> Option<String> {
    let (_, attribute_key) = split_fluent_key(key);
    if let Some(attribute_key) = attribute_key {
        return render_attribute_source(attribute_key, entry);
    }

    Some(serializer::serialize(&Resource {
        body: vec![entry.clone()],
        span: fluent_syntax::ast::Span::default(),
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
            span: fluent_syntax::ast::Span(0..7),
        },
        value: None,
        attributes: vec![attribute.clone()],
        comment: None,
        span: fluent_syntax::ast::Span::default(),
    });
    let rendered = serializer::serialize(&Resource {
        body: vec![synthetic],
        span: fluent_syntax::ast::Span::default(),
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
        fluent_syntax::ast::PatternElement::TextElement { value, .. } => MessagePreview {
            selectors: Vec::new(),
            text: (*value).to_string(),
        },
        fluent_syntax::ast::PatternElement::Placeable { expression, .. } => {
            render_expression_preview(expression, selector_overrides)
        }
    }
}

fn render_expression_preview(
    expression: &fluent_syntax::ast::Expression<&str>,
    selector_overrides: Option<&HashMap<String, String>>,
) -> MessagePreview {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline, _) => MessagePreview {
            selectors: Vec::new(),
            text: render_inline_expression_preview_text(inline),
        },
        fluent_syntax::ast::Expression::Select {
            selector, variants, ..
        } => {
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
            preview
                .selectors
                .insert(0, (selector_name, rendered_variant_name));
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
        if is_lsp_copy_marker_comment_content(line) {
            continue;
        }
        buffer.push_str(prefix);
        if !line.trim().is_empty() {
            buffer.push(' ');
            buffer.push_str(line);
        }
        buffer.push('\n');
    }
}

fn is_lsp_copy_marker_comment_content(line: &str) -> bool {
    parse_lsp_copy_marker_line(&format!("# {}", line.trim_start())).is_some()
}

fn parse_lsp_copy_marker_line(line: &str) -> Option<LspCopyMarkerKind> {
    let trimmed = line.trim();
    if trimmed == LSP_COPY_MARKER {
        return Some(LspCopyMarkerKind::Message);
    }

    let attribute = trimmed
        .strip_prefix(LSP_COPY_MARKER_PREFIX)?
        .strip_suffix(']')?
        .trim();
    if attribute.is_empty() {
        None
    } else {
        Some(LspCopyMarkerKind::Attribute(
            attribute.trim_start_matches('.').to_string(),
        ))
    }
}

fn render_attribute_copy_marker(attribute_key: &str) -> String {
    format!("{LSP_COPY_MARKER_PREFIX}{attribute_key}]")
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

fn parse_client_config(settings: &Value) -> ClientConfig {
    if let Some(object) = settings.as_object() {
        let direct = ClientConfig {
            selector_style: object
                .get("selector_style")
                .and_then(selector_style_from_json),
            error_on_unsupported_plural_categories: object
                .get("error_on_unsupported_plural_categories")
                .and_then(Value::as_bool),
            warn_on_missing_plural_categories: object
                .get("warn_on_missing_plural_categories")
                .and_then(Value::as_bool),
            warn_on_selector_style_mismatch: object
                .get("warn_on_selector_style_mismatch")
                .and_then(Value::as_bool),
        };
        if direct != ClientConfig::default() {
            return direct;
        }

        for key in ["fluent-lsp", "fluent_lsp"] {
            if let Some(nested) = object.get(key).and_then(Value::as_object) {
                let nested = ClientConfig {
                    selector_style: nested
                        .get("selector_style")
                        .and_then(selector_style_from_json),
                    error_on_unsupported_plural_categories: nested
                        .get("error_on_unsupported_plural_categories")
                        .and_then(Value::as_bool),
                    warn_on_missing_plural_categories: nested
                        .get("warn_on_missing_plural_categories")
                        .and_then(Value::as_bool),
                    warn_on_selector_style_mismatch: nested
                        .get("warn_on_selector_style_mismatch")
                        .and_then(Value::as_bool),
                };
                if nested != ClientConfig::default() {
                    return nested;
                }
            }
        }
    }

    ClientConfig::default()
}

fn selector_style_from_json(value: &Value) -> Option<SelectorStyle> {
    serde_json::from_value(value.clone()).ok()
}

fn collect_document_diagnostics(
    source: &str,
    language: &str,
    settings: EffectiveDiagnosticConfig,
) -> Vec<Diagnostic> {
    let (resource, parse_errors) = parse_fluent_resource_with_errors(source);
    let mut diagnostics = collect_parse_error_diagnostics(source, &parse_errors);
    diagnostics.extend(collect_lsp_copy_marker_diagnostics(source));

    if !settings.error_on_unsupported_plural_categories
        && !settings.warn_on_missing_plural_categories
        && !settings.warn_on_selector_style_mismatch
    {
        diagnostics.sort_by_key(|diagnostic| {
            (
                diagnostic.range.start.line,
                diagnostic.range.start.character,
                diagnostic.message.clone(),
            )
        });
        return diagnostics;
    }

    let supported_categories = plural_categories(language);
    for entry in &resource.body {
        match entry {
            Entry::Message(message) => {
                if let Some(value) = &message.value {
                    collect_pattern_diagnostics(
                        source,
                        language,
                        &supported_categories,
                        settings,
                        value,
                        &mut diagnostics,
                    );
                }
                for attribute in &message.attributes {
                    collect_pattern_diagnostics(
                        source,
                        language,
                        &supported_categories,
                        settings,
                        &attribute.value,
                        &mut diagnostics,
                    );
                }
            }
            Entry::Term(term) => {
                collect_pattern_diagnostics(
                    source,
                    language,
                    &supported_categories,
                    settings,
                    &term.value,
                    &mut diagnostics,
                );
                for attribute in &term.attributes {
                    collect_pattern_diagnostics(
                        source,
                        language,
                        &supported_categories,
                        settings,
                        &attribute.value,
                        &mut diagnostics,
                    );
                }
            }
            _ => {}
        }
    }

    diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic.range.start.line,
            diagnostic.range.start.character,
            diagnostic.message.clone(),
        )
    });
    diagnostics
}

fn local_only_file_diagnostic(file: &IndexedFile) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("fluent-lsp".to_string()),
        message: format!(
            "Translation file has no origin-language counterpart for `{}`",
            file.file_match.filepath
        ),
        ..Diagnostic::default()
    }
}

fn translation_counterpart_diagnostic(range: Range, message: String) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("fluent-lsp".to_string()),
        message,
        ..Diagnostic::default()
    }
}

fn collect_translation_counterpart_diagnostics(
    translation: &IndexedFile,
    origin: &IndexedFile,
) -> Vec<Diagnostic> {
    let resource = parse_fluent_resource(&translation.source);
    let mut diagnostics = Vec::new();

    for entry in resource.body {
        match entry {
            Entry::Message(message) => {
                let message_key = message.id.name.to_string();
                if !origin.keys.contains(&message_key) {
                    if let Some(range) = translation.definitions.get(&message_key).copied() {
                        diagnostics.push(translation_counterpart_diagnostic(
                            range,
                            format!(
                                "Translation entry `{message_key}` has no origin-language counterpart"
                            ),
                        ));
                    }
                    continue;
                }

                for attribute in message.attributes {
                    let key = format!("{}.{}", message_key, attribute.id.name);
                    if origin.keys.contains(&key) {
                        continue;
                    }
                    if let Some(range) = translation.definitions.get(&key).copied() {
                        diagnostics.push(translation_counterpart_diagnostic(
                            range,
                            format!(
                                "Translation attribute `{key}` has no origin-language counterpart"
                            ),
                        ));
                    }
                }
            }
            Entry::Term(term) => {
                let term_key = format!("-{}", term.id.name);
                if !origin.keys.contains(&term_key) {
                    if let Some(range) = translation.definitions.get(&term_key).copied() {
                        diagnostics.push(translation_counterpart_diagnostic(
                            range,
                            format!(
                                "Translation entry `{term_key}` has no origin-language counterpart"
                            ),
                        ));
                    }
                    continue;
                }

                for attribute in term.attributes {
                    let key = format!("{term_key}.{}", attribute.id.name);
                    if origin.keys.contains(&key) {
                        continue;
                    }
                    if let Some(range) = translation.definitions.get(&key).copied() {
                        diagnostics.push(translation_counterpart_diagnostic(
                            range,
                            format!(
                                "Translation attribute `{key}` has no origin-language counterpart"
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic.range.start.line,
            diagnostic.range.start.character,
            diagnostic.message.clone(),
        )
    });
    diagnostics
}

fn collect_parse_error_diagnostics(
    source: &str,
    errors: &[parser::ParserError],
) -> Vec<Diagnostic> {
    errors
        .iter()
        .filter_map(|error| {
            parse_error_range(source, error).map(|range| Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("fluent-lsp".to_string()),
                message: format!("Fluent syntax error: {error}"),
                ..Diagnostic::default()
            })
        })
        .collect()
}

fn collect_lsp_copy_marker_diagnostics(source: &str) -> Vec<Diagnostic> {
    source
        .lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let marker_start = line.find('#')?;
            if !line[..marker_start].trim().is_empty() {
                return None;
            }
            let marker = parse_lsp_copy_marker_line(&line[marker_start..])?;
            Some(Diagnostic {
                range: marker_diagnostic_range(source, line_index, marker_start, &marker)?,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fluent-lsp".to_string()),
                message: "Entry still contains an `# [LSP-COPY]` marker".to_string(),
                ..Diagnostic::default()
            })
        })
        .collect()
}

fn marker_diagnostic_range(
    source: &str,
    line_index: usize,
    marker_start: usize,
    marker: &LspCopyMarkerKind,
) -> Option<Range> {
    if let LspCopyMarkerKind::Attribute(attribute_key) = marker {
        if let Some(message_key) = next_message_key_after_line(source, line_index) {
            if let Some(range) =
                find_fluent_definition(source, &format!("{message_key}.{attribute_key}"))
            {
                return Some(range);
            }
        }
    }

    let start = u32::try_from(marker_start).ok()?;
    let line = u32::try_from(line_index).ok()?;
    Some(Range::new(
        Position::new(line, start),
        Position::new(
            line,
            start + u32::try_from(LSP_COPY_MARKER.chars().count()).ok()?,
        ),
    ))
}

fn next_message_key_after_line<'a>(source: &'a str, line_index: usize) -> Option<&'a str> {
    source.lines().skip(line_index + 1).find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return None;
        }
        let key = line.split_once('=')?.0.trim_end();
        if key.is_empty() { None } else { Some(key) }
    })
}

fn parse_error_range(source: &str, error: &parser::ParserError) -> Option<Range> {
    byte_range_to_lsp_range(source, error.pos.clone()).or_else(|| {
        error
            .slice
            .as_ref()
            .and_then(|slice| byte_range_to_lsp_range(source, slice.clone()))
    })
}

fn collect_pattern_diagnostics(
    source: &str,
    language: &str,
    supported_categories: &[&'static str],
    settings: EffectiveDiagnosticConfig,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if settings.warn_on_selector_style_mismatch {
        if let Some(shape) = analyze_selector_pattern_shape(source, pattern) {
            let (style, select) = match &shape {
                SelectorPatternShape::Whole(select) => (SelectorStyle::Whole, select),
                SelectorPatternShape::Prefix { select, .. } => (SelectorStyle::Prefix, select),
                SelectorPatternShape::Suffix { select, .. } => (SelectorStyle::Suffix, select),
            };
            if parsed_select_is_number_like(select)
                && settings
                    .preferred_selector_style
                    .is_some_and(|preferred| preferred != style)
            {
                let range = byte_range_to_lsp_range(
                    source,
                    trim_trailing_newlines_from_span(source, pattern.span.0.clone()),
                );
                if let Some(range) = range {
                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("fluent-lsp".to_string()),
                        message: format!(
                            "Selector style is `{}`, but workspace prefers `{}`",
                            style.label(),
                            settings.preferred_selector_style.unwrap().label()
                        ),
                        ..Diagnostic::default()
                    });
                }
            }
        }

        if analyze_selector_pattern_shape(source, pattern).is_none() {
            for occurrence in select_occurrences_in_pattern(source, pattern) {
                let Some(style) = analyze_selector_occurrence_style(source, pattern, &occurrence)
                else {
                    continue;
                };
                if parsed_select_is_number_like(&occurrence.select)
                    && settings
                        .preferred_selector_style
                        .is_some_and(|preferred| preferred != style)
                {
                    if let Some(range) =
                        byte_range_to_lsp_range(source, occurrence.placeable_span.clone())
                    {
                        diagnostics.push(Diagnostic {
                            range,
                            severity: Some(DiagnosticSeverity::WARNING),
                            source: Some("fluent-lsp".to_string()),
                            message: format!(
                                "Selector style is `{}`, but workspace prefers `{}`",
                                style.label(),
                                settings.preferred_selector_style.unwrap().label()
                            ),
                            ..Diagnostic::default()
                        });
                    }
                }
            }
        }
    }

    for element in &pattern.elements {
        let fluent_syntax::ast::PatternElement::Placeable { expression, .. } = element else {
            continue;
        };
        collect_expression_diagnostics(
            source,
            language,
            supported_categories,
            settings,
            expression,
            diagnostics,
        );
    }
}

fn collect_expression_diagnostics(
    source: &str,
    language: &str,
    supported_categories: &[&'static str],
    settings: EffectiveDiagnosticConfig,
    expression: &fluent_syntax::ast::Expression<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fluent_syntax::ast::Expression::Select { variants, .. } = expression else {
        return;
    };

    if selector_is_number_like(variants) {
        if settings.error_on_unsupported_plural_categories {
            for variant in variants {
                let Some(category_name) = identifier_variant_key_name(&variant.key) else {
                    continue;
                };
                let unsupported_message = if !is_known_plural_category_name(category_name) {
                    Some(format!(
                        "`{category_name}` is not a supported numeric selector key for `{language}`; use exact numbers or plural categories"
                    ))
                } else if !supported_categories.contains(&category_name) {
                    Some(format!(
                        "`{category_name}` is not a supported plural category for `{language}`"
                    ))
                } else {
                    None
                };
                if let Some(message) = unsupported_message {
                    if let Some(range) =
                        byte_range_to_lsp_range(source, variant_key_span(&variant.key))
                    {
                        diagnostics.push(Diagnostic {
                            range,
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("fluent-lsp".to_string()),
                            message,
                            ..Diagnostic::default()
                        });
                    }
                }
            }
        }

        if settings.warn_on_missing_plural_categories {
            let present = variants
                .iter()
                .filter_map(|variant| identifier_variant_key_name(&variant.key))
                .collect::<HashSet<_>>();
            if let Some(range) = byte_range_to_lsp_range(source, select_expression_span(expression))
            {
                for category in supported_categories {
                    if present.contains(category) {
                        continue;
                    }
                    let message = if *category == "other" {
                        format!(
                            "Numeric selector for `{language}` is missing fallback category `other`"
                        )
                    } else {
                        format!(
                            "Numeric selector for `{language}` is missing category `{category}`"
                        )
                    };
                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("fluent-lsp".to_string()),
                        message,
                        ..Diagnostic::default()
                    });
                }
            }
        }
    }

    for variant in variants {
        collect_pattern_diagnostics(
            source,
            language,
            supported_categories,
            settings,
            &variant.value,
            diagnostics,
        );
    }
}

fn parsed_select_is_number_like(select: &ParsedSelectExpression<'_>) -> bool {
    select.variants.iter().any(|variant| {
        variant.key_text.chars().all(|ch| ch.is_ascii_digit())
            || is_distinct_plural_category_name(&variant.key_text)
    })
}

fn selector_is_number_like(variants: &[fluent_syntax::ast::Variant<&str>]) -> bool {
    variants.iter().any(|variant| match &variant.key {
        fluent_syntax::ast::VariantKey::NumberLiteral { .. } => true,
        fluent_syntax::ast::VariantKey::Identifier { name, .. } => {
            is_distinct_plural_category_name(name)
        }
    })
}

fn is_known_plural_category_name(name: &str) -> bool {
    matches!(name, "zero" | "one" | "two" | "few" | "many" | "other")
}

fn is_distinct_plural_category_name(name: &str) -> bool {
    matches!(name, "zero" | "one" | "two" | "few" | "many")
}

fn identifier_variant_key_name<'a>(
    key: &'a fluent_syntax::ast::VariantKey<&'a str>,
) -> Option<&'a str> {
    match key {
        fluent_syntax::ast::VariantKey::Identifier { name, .. } => Some(name),
        fluent_syntax::ast::VariantKey::NumberLiteral { .. } => None,
    }
}

fn variant_key_span(key: &fluent_syntax::ast::VariantKey<&str>) -> ByteRange<usize> {
    match key {
        fluent_syntax::ast::VariantKey::Identifier { span, .. }
        | fluent_syntax::ast::VariantKey::NumberLiteral { span, .. } => span.0.clone(),
    }
}

fn select_expression_span(expression: &fluent_syntax::ast::Expression<&str>) -> ByteRange<usize> {
    match expression {
        fluent_syntax::ast::Expression::Select { span, .. }
        | fluent_syntax::ast::Expression::Inline(_, span) => span.0.clone(),
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
        .filter_map(|captures| {
            captures
                .get(1)
                .map(|value| value.as_str().trim().to_string())
        })
        .collect()
}

fn find_generate_selector_target(
    source: &str,
    path: &Path,
    position: Position,
) -> Option<GenerateSelectorTarget> {
    if !is_fluent_file(path) {
        return None;
    }

    let key = extract_definition_key(source, path, position)?;
    let definition_range = find_fluent_definition(source, &key)?;
    let resource = parse_fluent_resource(source);
    let pattern = find_fluent_pattern(&resource, &key)?;
    let variables = collect_variable_placeables(source, pattern);
    let functions = collect_function_selector_targets(source, pattern);
    let selected_variable = variables
        .iter()
        .find(|variable| position_overlaps_byte_span(source, position, &variable.selection_span))
        .cloned();
    let selected_function = if selected_variable.is_none() {
        functions
            .iter()
            .filter(|function| {
                position_overlaps_byte_span(source, position, &function.selection_span)
            })
            .min_by_key(|function| function.selection_span.end - function.selection_span.start)
            .cloned()
    } else {
        None
    };

    let pattern_span = if let Some(variable) = &selected_variable {
        variable.container_span.clone()
    } else if let Some(function) = &selected_function {
        function.container_span.clone()
    } else if let Some(position_span) = deepest_pattern_span_for_position(source, pattern, position)
    {
        position_span
    } else if range_contains_position(&definition_range, position) {
        pattern.span.0.clone()
    } else {
        return None;
    };
    let pattern_span = trim_trailing_newlines_from_span(source, pattern_span);

    let variables = variables
        .into_iter()
        .filter(|variable| variable.container_span == pattern_span)
        .collect::<Vec<_>>();
    let functions = functions
        .into_iter()
        .filter(|function| function.container_span == pattern_span)
        .collect::<Vec<_>>();
    if selected_variable.is_none() && selected_function.is_none() {
        let distinct_variable_names = variables
            .iter()
            .map(|variable| variable.name.as_str())
            .collect::<HashSet<_>>()
            .len();
        if distinct_variable_names > 1 {
            return None;
        }
        if pattern_contains_select(pattern) {
            return None;
        }
    }

    Some(GenerateSelectorTarget {
        key,
        pattern_span,
        variables,
        selected_variable,
        functions,
        selected_function,
    })
}

fn collect_variable_placeables(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
) -> Vec<VariablePlaceable> {
    let mut variables = Vec::new();
    collect_variable_placeables_in_pattern(source, pattern, &mut variables);
    variables
}

fn collect_function_selector_targets(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
) -> Vec<FunctionSelectorTarget> {
    let mut functions = Vec::new();
    collect_function_selector_targets_in_pattern(source, pattern, &mut functions);
    functions
}

fn generate_number_selector_edit(
    source: &str,
    target: &GenerateSelectorTarget,
    language: &str,
    style: SelectorStyle,
    use_snippets: bool,
) -> Option<String> {
    let pattern_text = source.get(target.pattern_span.clone())?;
    let line_indent = line_indentation_at(source, target.pattern_span.start);
    let anchor_variable = target.selected_variable.as_ref().cloned().or_else(|| {
        target
            .variables
            .iter()
            .find(|variable| variable.anchor_span.start < variable.anchor_span.end)
            .cloned()
    });
    let anchor_function = target.selected_function.as_ref().cloned();
    let binding = selector_binding_for_target(
        target.selected_function.as_ref(),
        target.selected_variable.as_ref(),
        target.variables.first(),
        use_snippets,
    );

    let whole_text = if target.selected_function.is_some() {
        pattern_text.to_string()
    } else if target.selected_variable.is_none() {
        if let Some(variable) = anchor_variable.as_ref() {
            replace_variable_references(
                pattern_text,
                target.pattern_span.start,
                &target.variables,
                &variable.name,
                &render_selector_token(&binding),
            )
        } else {
            pattern_text.to_string()
        }
    } else {
        pattern_text.to_string()
    };

    let replacement = match style {
        SelectorStyle::Whole => indent_selector_block(
            &render_generated_selector_block(
                &render_selector_token(&binding),
                language,
                &whole_text,
            ),
            &line_indent,
        ),
        SelectorStyle::Prefix => {
            let anchor_relative = if let Some(function) = anchor_function.as_ref() {
                relative_span(&target.pattern_span, &function.anchor_span)?
            } else {
                let variable = anchor_variable.as_ref()?;
                relative_span(&target.pattern_span, &variable.anchor_span)?
            };
            let before_including_anchor = if target.selected_function.is_some() {
                pattern_text[..anchor_relative.end].to_string()
            } else if let Some(variable) = anchor_variable.as_ref() {
                replace_variable_references(
                    &pattern_text[..anchor_relative.end],
                    target.pattern_span.start,
                    &target.variables,
                    &variable.name,
                    &render_selector_token(&binding),
                )
            } else {
                pattern_text[..anchor_relative.end].to_string()
            };
            let after_anchor = if target.selected_function.is_some() {
                pattern_text[anchor_relative.end..].to_string()
            } else if let Some(variable) = anchor_variable.as_ref() {
                replace_variable_references(
                    &pattern_text[anchor_relative.end..],
                    target.pattern_span.start + anchor_relative.end,
                    &target.variables,
                    &variable.name,
                    &render_selector_token(&binding),
                )
            } else {
                pattern_text[anchor_relative.end..].to_string()
            };
            if after_anchor.is_empty() {
                return None;
            }
            concat_prefix_and_selector(
                &before_including_anchor,
                &indent_selector_block(
                    &render_generated_selector_block(
                        &render_selector_token(&binding),
                        language,
                        &after_anchor,
                    ),
                    &line_indent,
                ),
            )
        }
        SelectorStyle::Suffix => {
            let anchor_relative = if let Some(function) = anchor_function.as_ref() {
                relative_span(&target.pattern_span, &function.anchor_span)?
            } else {
                let variable = anchor_variable.as_ref()?;
                relative_span(&target.pattern_span, &variable.anchor_span)?
            };
            let before_anchor = if target.selected_function.is_some() {
                pattern_text[..anchor_relative.start].to_string()
            } else if let Some(variable) = anchor_variable.as_ref() {
                replace_variable_references(
                    &pattern_text[..anchor_relative.start],
                    target.pattern_span.start,
                    &target.variables,
                    &variable.name,
                    &render_selector_token(&binding),
                )
            } else {
                pattern_text[..anchor_relative.start].to_string()
            };
            let from_anchor = if target.selected_function.is_some() {
                pattern_text[anchor_relative.start..].to_string()
            } else if let Some(variable) = anchor_variable.as_ref() {
                replace_variable_references(
                    &pattern_text[anchor_relative.start..],
                    target.pattern_span.start + anchor_relative.start,
                    &target.variables,
                    &variable.name,
                    &render_selector_token(&binding),
                )
            } else {
                pattern_text[anchor_relative.start..].to_string()
            };
            concat_prefix_and_selector(
                &before_anchor,
                &indent_selector_block(
                    &render_generated_selector_block(
                        &render_selector_token(&binding),
                        language,
                        &from_anchor,
                    ),
                    &line_indent,
                ),
            )
        }
    };

    Some(replacement)
}

fn find_selector_rewrite_target(
    source: &str,
    path: &Path,
    position: Position,
) -> Option<(ByteRange<usize>, Vec<SelectorRewriteAction>)> {
    if !is_fluent_file(path) {
        return None;
    }

    let key = extract_definition_key(source, path, position)?;
    let definition_range = find_fluent_definition(source, &key)?;
    let resource = parse_fluent_resource(source);
    let root_pattern = find_fluent_pattern(&resource, &key)?;

    if range_contains_position(&definition_range, position) {
        let actions = selector_rewrite_actions_for_pattern(source, root_pattern);
        if !actions.is_empty() {
            return Some((
                trim_trailing_newlines_from_span(source, root_pattern.span.0.clone()),
                actions,
            ));
        }
    }

    let byte_index = position_to_byte_index(source, position)?;
    let mut candidate_patterns = Vec::new();
    collect_pattern_refs(root_pattern, &mut candidate_patterns);
    candidate_patterns.retain(|pattern| pattern.span.0.contains(&byte_index));
    candidate_patterns.sort_by_key(|pattern| pattern.span.0.end - pattern.span.0.start);

    for pattern in candidate_patterns {
        let actions = selector_rewrite_actions_for_pattern(source, pattern);
        if !actions.is_empty() {
            return Some((
                trim_trailing_newlines_from_span(source, pattern.span.0.clone()),
                actions,
            ));
        }

        let actions = selector_rewrite_actions_for_occurrence(source, pattern, byte_index);
        if !actions.is_empty() {
            return Some((
                trim_trailing_newlines_from_span(source, pattern.span.0.clone()),
                actions,
            ));
        }
    }

    None
}

fn selector_rewrite_actions_for_pattern(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
) -> Vec<SelectorRewriteAction> {
    let Some(shape) = analyze_selector_pattern_shape(source, pattern) else {
        return Vec::new();
    };
    let current_text = source
        .get(trim_trailing_newlines_from_span(
            source,
            pattern.span.0.clone(),
        ))
        .unwrap_or_default();
    let mut actions = Vec::new();

    match shape {
        SelectorPatternShape::Whole(select) => {
            if let Some(replacement) = rewrite_whole_selector_to_prefix(source, pattern, &select) {
                if replacement != current_text {
                    actions.push(SelectorRewriteAction {
                        kind: SelectorRewriteKind::Prefix,
                        replacement,
                    });
                }
            }
            if let Some(replacement) = rewrite_whole_selector_to_suffix(source, pattern, &select) {
                if replacement != current_text {
                    actions.push(SelectorRewriteAction {
                        kind: SelectorRewriteKind::Suffix,
                        replacement,
                    });
                }
            }
        }
        SelectorPatternShape::Prefix {
            outer_prefix,
            select,
        } => {
            if let Some(replacement) =
                rewrite_prefixed_selector_to_whole(source, pattern, &outer_prefix, &select)
            {
                if replacement != current_text {
                    actions.push(SelectorRewriteAction {
                        kind: SelectorRewriteKind::Whole,
                        replacement,
                    });
                }
            }
        }
        SelectorPatternShape::Suffix {
            outer_prefix,
            select,
        } => {
            if let Some(replacement) =
                rewrite_prefixed_selector_to_whole(source, pattern, &outer_prefix, &select)
            {
                if replacement != current_text {
                    actions.push(SelectorRewriteAction {
                        kind: SelectorRewriteKind::Whole,
                        replacement,
                    });
                }
            }
            if let Some(replacement) =
                rewrite_suffix_selector_to_prefix(source, pattern, &outer_prefix, &select)
            {
                if replacement != current_text {
                    actions.push(SelectorRewriteAction {
                        kind: SelectorRewriteKind::Prefix,
                        replacement,
                    });
                }
            }
        }
    }

    actions
}

fn selector_rewrite_actions_for_occurrence(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    byte_index: usize,
) -> Vec<SelectorRewriteAction> {
    let current_text = source
        .get(trim_trailing_newlines_from_span(
            source,
            pattern.span.0.clone(),
        ))
        .unwrap_or_default();
    let mut actions = Vec::new();

    for occurrence in select_occurrences_in_pattern(source, pattern) {
        if !occurrence.placeable_span.contains(&byte_index) {
            continue;
        }

        if let Some(replacement) = rewrite_selected_selector_to_whole(source, pattern, &occurrence)
        {
            if replacement != current_text {
                actions.push(SelectorRewriteAction {
                    kind: SelectorRewriteKind::Whole,
                    replacement,
                });
            }
        }

        if let Some(replacement) = rewrite_selected_selector_to_suffix(source, pattern, &occurrence)
        {
            if replacement != current_text {
                actions.push(SelectorRewriteAction {
                    kind: SelectorRewriteKind::Suffix,
                    replacement,
                });
            }
        }

        if !actions.is_empty() {
            break;
        }
    }

    actions
}

enum SelectorPatternShape<'a> {
    Whole(ParsedSelectExpression<'a>),
    Prefix {
        outer_prefix: Vec<&'a fluent_syntax::ast::PatternElement<&'a str>>,
        select: ParsedSelectExpression<'a>,
    },
    Suffix {
        outer_prefix: Vec<&'a fluent_syntax::ast::PatternElement<&'a str>>,
        select: ParsedSelectExpression<'a>,
    },
}

struct ParsedSelectExpression<'a> {
    selector_name: &'a str,
    selector_text: String,
    variants: Vec<ParsedVariant<'a>>,
}

struct ParsedVariant<'a> {
    key_text: String,
    default: bool,
    value: &'a fluent_syntax::ast::Pattern<&'a str>,
}

struct ParsedSelectOccurrence<'a> {
    placeable_span: ByteRange<usize>,
    outer_prefix: Vec<&'a fluent_syntax::ast::PatternElement<&'a str>>,
    outer_suffix: Vec<&'a fluent_syntax::ast::PatternElement<&'a str>>,
    select: ParsedSelectExpression<'a>,
}

fn analyze_selector_occurrence_style(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    occurrence: &ParsedSelectOccurrence<'_>,
) -> Option<SelectorStyle> {
    if occurrence.outer_suffix.is_empty() {
        if selector_prefix_anchor_index(&occurrence.outer_prefix, occurrence.select.selector_name)
            .is_some()
        {
            return Some(SelectorStyle::Prefix);
        }

        if occurrence.select.variants.iter().all(|variant| {
            variant_starts_with_direct_selector(variant.value, occurrence.select.selector_name)
        }) {
            return Some(SelectorStyle::Suffix);
        }
    }

    rewrite_selected_selector_to_whole(source, pattern, occurrence).map(|_| SelectorStyle::Whole)
}

fn analyze_selector_pattern_shape<'a>(
    source: &'a str,
    pattern: &'a fluent_syntax::ast::Pattern<&'a str>,
) -> Option<SelectorPatternShape<'a>> {
    let select = select_expression_from_tail_element(source, pattern)?;
    let outer_prefix = pattern
        .elements
        .iter()
        .take(pattern.elements.len().checked_sub(1)?)
        .collect::<Vec<_>>();

    if outer_prefix.is_empty() {
        return Some(SelectorPatternShape::Whole(select));
    }

    if selector_prefix_anchor_index(&outer_prefix, select.selector_name).is_some() {
        return Some(SelectorPatternShape::Prefix {
            outer_prefix,
            select,
        });
    }

    if select
        .variants
        .iter()
        .all(|variant| variant_starts_with_direct_selector(variant.value, select.selector_name))
    {
        return Some(SelectorPatternShape::Suffix {
            outer_prefix,
            select,
        });
    }

    None
}

fn select_expression_from_tail_element<'a>(
    source: &'a str,
    pattern: &'a fluent_syntax::ast::Pattern<&'a str>,
) -> Option<ParsedSelectExpression<'a>> {
    let tail = pattern.elements.last()?;
    let fluent_syntax::ast::PatternElement::Placeable { expression, .. } = tail else {
        return None;
    };
    select_expression_from_expression(source, expression)
}

fn select_expression_from_expression<'a>(
    source: &'a str,
    expression: &'a fluent_syntax::ast::Expression<&'a str>,
) -> Option<ParsedSelectExpression<'a>> {
    let fluent_syntax::ast::Expression::Select {
        selector, variants, ..
    } = expression
    else {
        return None;
    };
    let fluent_syntax::ast::InlineExpression::VariableReference { id, .. } = selector else {
        return None;
    };

    let selector_text = source.get(selector.get_span().0.clone())?.to_string();
    let variants = variants
        .iter()
        .map(|variant| {
            Some(ParsedVariant {
                key_text: variant_key_source_text(source, &variant.key)?,
                default: variant.default,
                value: &variant.value,
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(ParsedSelectExpression {
        selector_name: id.name,
        selector_text,
        variants,
    })
}

fn select_occurrences_in_pattern<'a>(
    source: &'a str,
    pattern: &'a fluent_syntax::ast::Pattern<&'a str>,
) -> Vec<ParsedSelectOccurrence<'a>> {
    let mut occurrences = Vec::new();

    for (index, element) in pattern.elements.iter().enumerate() {
        let fluent_syntax::ast::PatternElement::Placeable { expression, span } = element else {
            continue;
        };
        let Some(select) = select_expression_from_expression(source, expression) else {
            continue;
        };
        occurrences.push(ParsedSelectOccurrence {
            placeable_span: normalize_placeable_span(source, span.0.clone()),
            outer_prefix: pattern.elements[..index].iter().collect(),
            outer_suffix: pattern.elements[index + 1..].iter().collect(),
            select,
        });
    }

    occurrences
}

fn rewrite_prefixed_selector_to_whole(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    outer_prefix: &[&fluent_syntax::ast::PatternElement<&str>],
    select: &ParsedSelectExpression<'_>,
) -> Option<String> {
    let prefix_text = render_pattern_elements(source, outer_prefix)?;
    let variants = select
        .variants
        .iter()
        .map(|variant| {
            Some(RenderedVariant {
                key_text: variant.key_text.clone(),
                default: variant.default,
                body: format!(
                    "{prefix_text}{}",
                    pattern_source_text(source, variant.value)?
                ),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(render_select_block(
        &select.selector_text,
        &variants,
        &line_indentation_at(source, pattern.span.0.start),
    ))
}

fn rewrite_selected_selector_to_whole(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    occurrence: &ParsedSelectOccurrence<'_>,
) -> Option<String> {
    let prefix_text = render_pattern_elements(source, &occurrence.outer_prefix)?;
    let suffix_text = render_pattern_elements(source, &occurrence.outer_suffix)?;
    let variants = occurrence
        .select
        .variants
        .iter()
        .map(|variant| {
            Some(RenderedVariant {
                key_text: variant.key_text.clone(),
                default: variant.default,
                body: trim_rendered_variant_body(format!(
                    "{prefix_text}{}{suffix_text}",
                    pattern_source_text(source, variant.value)?
                )),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(render_select_block(
        &occurrence.select.selector_text,
        &variants,
        &line_indentation_at(source, pattern.span.0.start),
    ))
}

fn rewrite_whole_selector_to_prefix(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    select: &ParsedSelectExpression<'_>,
) -> Option<String> {
    let common_prefix_len = common_prefix_element_count(source, &select.variants);
    if common_prefix_len == 0 {
        return None;
    }
    let first_variant = select.variants.first()?;
    let prefix_elements = first_variant
        .value
        .elements
        .iter()
        .take(common_prefix_len)
        .collect::<Vec<_>>();
    if selector_prefix_anchor_index(&prefix_elements, select.selector_name).is_none() {
        return None;
    }
    let outer_prefix_text = render_pattern_elements(source, &prefix_elements)?;
    let variants = select
        .variants
        .iter()
        .map(|variant| {
            let remaining = variant.value.elements.get(common_prefix_len..)?;
            if remaining.is_empty() {
                return None;
            }
            Some(RenderedVariant {
                key_text: variant.key_text.clone(),
                default: variant.default,
                body: trim_rendered_variant_body(render_pattern_element_slice(source, remaining)?),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(concat_prefix_and_selector(
        &outer_prefix_text,
        &render_select_block(
            &select.selector_text,
            &variants,
            &line_indentation_at(source, pattern.span.0.start),
        ),
    ))
}

fn rewrite_whole_selector_to_suffix(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    select: &ParsedSelectExpression<'_>,
) -> Option<String> {
    let common_prefix_len = common_prefix_element_count(source, &select.variants);
    let first_variant = select.variants.first()?;
    let common_prefix = first_variant
        .value
        .elements
        .iter()
        .take(common_prefix_len)
        .collect::<Vec<_>>();
    let anchor_index = common_prefix
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            (direct_variable_placeable_name(element) == Some(select.selector_name)).then_some(index)
        })
        .last()?;
    if anchor_index == 0 {
        return None;
    }
    let outer_prefix_elements = &common_prefix[..anchor_index];
    let outer_prefix_text = render_pattern_elements(source, outer_prefix_elements)?;
    let variants = select
        .variants
        .iter()
        .map(|variant| {
            let remaining = variant.value.elements.get(anchor_index..)?;
            if remaining.is_empty() {
                return None;
            }
            Some(RenderedVariant {
                key_text: variant.key_text.clone(),
                default: variant.default,
                body: trim_rendered_variant_body(render_pattern_element_slice(source, remaining)?),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(concat_prefix_and_selector(
        &outer_prefix_text,
        &render_select_block(
            &select.selector_text,
            &variants,
            &line_indentation_at(source, pattern.span.0.start),
        ),
    ))
}

fn rewrite_suffix_selector_to_prefix(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    outer_prefix: &[&fluent_syntax::ast::PatternElement<&str>],
    select: &ParsedSelectExpression<'_>,
) -> Option<String> {
    let first_variant = select.variants.first()?;
    let anchor_element = first_variant.value.elements.first()?;
    if direct_variable_placeable_name(anchor_element) != Some(select.selector_name) {
        return None;
    }
    let mut prefix_elements = outer_prefix.to_vec();
    prefix_elements.push(anchor_element);
    let outer_prefix_text = render_pattern_elements(source, &prefix_elements)?;
    let variants = select
        .variants
        .iter()
        .map(|variant| {
            let remaining = variant.value.elements.get(1..)?;
            if remaining.is_empty() {
                return None;
            }
            Some(RenderedVariant {
                key_text: variant.key_text.clone(),
                default: variant.default,
                body: trim_rendered_variant_body(render_pattern_element_slice(source, remaining)?),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(concat_prefix_and_selector(
        &outer_prefix_text,
        &render_select_block(
            &select.selector_text,
            &variants,
            &line_indentation_at(source, pattern.span.0.start),
        ),
    ))
}

fn rewrite_selected_selector_to_suffix(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    occurrence: &ParsedSelectOccurrence<'_>,
) -> Option<String> {
    let anchor_index =
        selector_prefix_anchor_index(&occurrence.outer_prefix, occurrence.select.selector_name)?;
    let outer_prefix_text =
        render_pattern_elements(source, &occurrence.outer_prefix[..anchor_index])?;
    let body_prefix_text =
        render_pattern_elements(source, &occurrence.outer_prefix[anchor_index..])?;
    let suffix_text = render_pattern_elements(source, &occurrence.outer_suffix)?;
    let variants = occurrence
        .select
        .variants
        .iter()
        .map(|variant| {
            Some(RenderedVariant {
                key_text: variant.key_text.clone(),
                default: variant.default,
                body: trim_rendered_variant_body(format!(
                    "{body_prefix_text}{}{suffix_text}",
                    pattern_source_text(source, variant.value)?
                )),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(concat_prefix_and_selector(
        &outer_prefix_text,
        &render_select_block(
            &occurrence.select.selector_text,
            &variants,
            &line_indentation_at(source, pattern.span.0.start),
        ),
    ))
}

struct RenderedVariant {
    key_text: String,
    default: bool,
    body: String,
}

fn render_select_block(
    selector_text: &str,
    variants: &[RenderedVariant],
    line_indent: &str,
) -> String {
    let mut lines = vec![format!("{{ {selector_text} ->")];
    for variant in variants {
        let default_prefix = if variant.default { "*" } else { "" };
        lines.push(format!(
            "    {default_prefix}[{}]{}",
            variant.key_text,
            format_variant_body(&variant.body)
        ));
    }
    lines.push("}".to_string());
    indent_selector_block(&lines.join("\n"), line_indent)
}

fn render_generated_selector_block(variable: &str, language: &str, branch_body: &str) -> String {
    let mut lines = vec![format!("{{ {variable} ->")];
    let body = format_variant_body(branch_body);
    let categories = plural_categories(language);

    for category in categories {
        let default_prefix = if category == "other" { "*" } else { "" };
        lines.push(format!("    {default_prefix}[{category}]{body}"));
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn pad_assignment_replacement(
    source: &str,
    span: &ByteRange<usize>,
    replacement: String,
) -> String {
    if replacement.starts_with('{')
        && span.start > 0
        && source.as_bytes().get(span.start - 1) == Some(&b'=')
    {
        format!(" {replacement}")
    } else {
        replacement
    }
}

fn format_variant_body(body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else {
        let indented = body.replace('\n', "\n    ");
        if indented
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace() || attach_without_space(ch))
        {
            indented
        } else {
            format!(" {indented}")
        }
    }
}

fn attach_without_space(ch: char) -> bool {
    matches!(
        ch,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '%' | '/'
    )
}

fn concat_prefix_and_selector(prefix: &str, selector_block: &str) -> String {
    if prefix.is_empty() {
        selector_block.to_string()
    } else if prefix.chars().last().is_some_and(char::is_whitespace) {
        format!("{prefix}{selector_block}")
    } else {
        format!("{prefix} {selector_block}")
    }
}

fn plural_categories(language: &str) -> Vec<&'static str> {
    let locale = language.parse::<Locale>().ok().or_else(|| {
        language
            .split('-')
            .next()
            .and_then(|base| base.parse::<Locale>().ok())
    });

    locale
        .and_then(|locale| PluralRules::try_new_cardinal(locale.into()).ok())
        .map(|rules| {
            let mut present = HashSet::new();
            for number in 0_u32..=200 {
                present.insert(rules.category_for(number));
            }
            present.insert(PluralCategory::Other);

            [
                PluralCategory::Zero,
                PluralCategory::One,
                PluralCategory::Two,
                PluralCategory::Few,
                PluralCategory::Many,
                PluralCategory::Other,
            ]
            .into_iter()
            .filter(|category| present.contains(category))
            .map(plural_category_name)
            .collect()
        })
        .unwrap_or_else(|| vec!["one", "other"])
}

fn plural_category_name(category: PluralCategory) -> &'static str {
    match category {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}

fn available_selector_styles(target: &GenerateSelectorTarget) -> Vec<SelectorStyle> {
    if target.selected_function.is_some() || !target.variables.is_empty() {
        vec![
            SelectorStyle::Prefix,
            SelectorStyle::Whole,
            SelectorStyle::Suffix,
        ]
    } else if target.variables.is_empty() {
        vec![SelectorStyle::Whole]
    } else {
        vec![SelectorStyle::Whole]
    }
}

fn ordered_selector_styles(
    mut styles: Vec<SelectorStyle>,
    preferred: SelectorStyle,
) -> Vec<SelectorStyle> {
    if let Some(index) = styles.iter().position(|style| *style == preferred) {
        let preferred_style = styles.remove(index);
        styles.insert(0, preferred_style);
    }
    styles
}

fn collect_variable_placeables_in_pattern(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    variables: &mut Vec<VariablePlaceable>,
) {
    let container_span = trim_trailing_newlines_from_span(source, pattern.span.0.clone());
    for element in &pattern.elements {
        collect_variable_placeables_in_element(source, element, &container_span, variables);
    }
}

fn collect_variable_placeables_in_element(
    source: &str,
    element: &fluent_syntax::ast::PatternElement<&str>,
    container_span: &ByteRange<usize>,
    variables: &mut Vec<VariablePlaceable>,
) {
    if let fluent_syntax::ast::PatternElement::Placeable { expression, span } = element {
        let anchor_span = normalize_placeable_span(source, span.0.clone());
        if let fluent_syntax::ast::Expression::Inline(
            fluent_syntax::ast::InlineExpression::VariableReference { id, span, .. },
            _,
        ) = expression
        {
            variables.push(VariablePlaceable {
                name: format!("${}", id.name),
                reference_span: span.0.clone(),
                selection_span: anchor_span.clone(),
                anchor_span,
                container_span: container_span.clone(),
            });
        } else {
            collect_variable_references_in_expression(
                source,
                expression,
                container_span,
                &anchor_span,
                variables,
            );
        }
    }
}

fn collect_variable_references_in_expression(
    source: &str,
    expression: &fluent_syntax::ast::Expression<&str>,
    container_span: &ByteRange<usize>,
    anchor_span: &ByteRange<usize>,
    variables: &mut Vec<VariablePlaceable>,
) {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline, _) => {
            collect_variable_references_in_inline(
                source,
                inline,
                container_span,
                anchor_span,
                variables,
            );
        }
        fluent_syntax::ast::Expression::Select {
            selector, variants, ..
        } => {
            collect_variable_references_in_inline(
                source,
                selector,
                container_span,
                anchor_span,
                variables,
            );
            for variant in variants {
                collect_variable_placeables_in_pattern(source, &variant.value, variables);
            }
        }
    }
}

fn collect_variable_references_in_inline(
    source: &str,
    inline: &fluent_syntax::ast::InlineExpression<&str>,
    container_span: &ByteRange<usize>,
    anchor_span: &ByteRange<usize>,
    variables: &mut Vec<VariablePlaceable>,
) {
    match inline {
        fluent_syntax::ast::InlineExpression::VariableReference { id, span } => {
            variables.push(VariablePlaceable {
                name: format!("${}", id.name),
                reference_span: span.0.clone(),
                selection_span: span.0.clone(),
                anchor_span: anchor_span.clone(),
                container_span: container_span.clone(),
            });
        }
        fluent_syntax::ast::InlineExpression::FunctionReference { arguments, .. } => {
            collect_variable_references_in_call_arguments(
                source,
                arguments,
                container_span,
                anchor_span,
                variables,
            );
        }
        fluent_syntax::ast::InlineExpression::TermReference { arguments, .. } => {
            if let Some(arguments) = arguments {
                collect_variable_references_in_call_arguments(
                    source,
                    arguments,
                    container_span,
                    anchor_span,
                    variables,
                );
            }
        }
        fluent_syntax::ast::InlineExpression::Placeable { expression, .. } => {
            collect_variable_references_in_expression(
                source,
                expression,
                container_span,
                anchor_span,
                variables,
            );
        }
        fluent_syntax::ast::InlineExpression::StringLiteral { .. }
        | fluent_syntax::ast::InlineExpression::NumberLiteral { .. }
        | fluent_syntax::ast::InlineExpression::MessageReference { .. } => {}
    }
}

fn collect_variable_references_in_call_arguments(
    source: &str,
    arguments: &fluent_syntax::ast::CallArguments<&str>,
    container_span: &ByteRange<usize>,
    anchor_span: &ByteRange<usize>,
    variables: &mut Vec<VariablePlaceable>,
) {
    for positional in &arguments.positional {
        collect_variable_references_in_inline(
            source,
            positional,
            container_span,
            anchor_span,
            variables,
        );
    }
    for named in &arguments.named {
        collect_variable_references_in_inline(
            source,
            &named.value,
            container_span,
            anchor_span,
            variables,
        );
    }
}

fn collect_function_selector_targets_in_pattern(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    functions: &mut Vec<FunctionSelectorTarget>,
) {
    let container_span = trim_trailing_newlines_from_span(source, pattern.span.0.clone());
    for element in &pattern.elements {
        collect_function_selector_targets_in_element(source, element, &container_span, functions);
    }
}

fn collect_function_selector_targets_in_element(
    source: &str,
    element: &fluent_syntax::ast::PatternElement<&str>,
    container_span: &ByteRange<usize>,
    functions: &mut Vec<FunctionSelectorTarget>,
) {
    if let fluent_syntax::ast::PatternElement::Placeable { expression, span } = element {
        let anchor_span = normalize_placeable_span(source, span.0.clone());
        collect_function_selector_targets_in_expression(
            source,
            expression,
            container_span,
            &anchor_span,
            functions,
        );
    }
}

fn collect_function_selector_targets_in_expression(
    source: &str,
    expression: &fluent_syntax::ast::Expression<&str>,
    container_span: &ByteRange<usize>,
    anchor_span: &ByteRange<usize>,
    functions: &mut Vec<FunctionSelectorTarget>,
) {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline, _) => {
            collect_function_selector_targets_in_inline(
                source,
                inline,
                container_span,
                anchor_span,
                functions,
            )
        }
        fluent_syntax::ast::Expression::Select {
            selector, variants, ..
        } => {
            collect_function_selector_targets_in_inline(
                source,
                selector,
                container_span,
                anchor_span,
                functions,
            );
            for variant in variants {
                collect_function_selector_targets_in_pattern(source, &variant.value, functions);
            }
        }
    }
}

fn collect_function_selector_targets_in_inline(
    source: &str,
    inline: &fluent_syntax::ast::InlineExpression<&str>,
    container_span: &ByteRange<usize>,
    anchor_span: &ByteRange<usize>,
    functions: &mut Vec<FunctionSelectorTarget>,
) {
    match inline {
        fluent_syntax::ast::InlineExpression::FunctionReference {
            span, arguments, ..
        } => {
            if let Some(selector_text) = source.get(span.0.clone()) {
                functions.push(FunctionSelectorTarget {
                    selector_text: selector_text.to_string(),
                    selection_span: span.0.clone(),
                    anchor_span: anchor_span.clone(),
                    container_span: container_span.clone(),
                });
            }
            collect_function_selector_targets_in_call_arguments(
                source,
                arguments,
                container_span,
                anchor_span,
                functions,
            );
        }
        fluent_syntax::ast::InlineExpression::TermReference { arguments, .. } => {
            if let Some(arguments) = arguments {
                collect_function_selector_targets_in_call_arguments(
                    source,
                    arguments,
                    container_span,
                    anchor_span,
                    functions,
                );
            }
        }
        fluent_syntax::ast::InlineExpression::Placeable { expression, .. } => {
            collect_function_selector_targets_in_expression(
                source,
                expression,
                container_span,
                anchor_span,
                functions,
            );
        }
        fluent_syntax::ast::InlineExpression::StringLiteral { .. }
        | fluent_syntax::ast::InlineExpression::NumberLiteral { .. }
        | fluent_syntax::ast::InlineExpression::MessageReference { .. }
        | fluent_syntax::ast::InlineExpression::VariableReference { .. } => {}
    }
}

fn collect_function_selector_targets_in_call_arguments(
    source: &str,
    arguments: &fluent_syntax::ast::CallArguments<&str>,
    container_span: &ByteRange<usize>,
    anchor_span: &ByteRange<usize>,
    functions: &mut Vec<FunctionSelectorTarget>,
) {
    for positional in &arguments.positional {
        collect_function_selector_targets_in_inline(
            source,
            positional,
            container_span,
            anchor_span,
            functions,
        );
    }
    for named in &arguments.named {
        collect_function_selector_targets_in_inline(
            source,
            &named.value,
            container_span,
            anchor_span,
            functions,
        );
    }
}

fn deepest_pattern_span_for_position(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
    position: Position,
) -> Option<ByteRange<usize>> {
    let byte_index = position_to_byte_index(source, position)?;
    let mut spans = Vec::new();
    collect_pattern_spans(pattern, &mut spans);
    spans
        .into_iter()
        .filter(|span| span.contains(&byte_index))
        .min_by_key(|span| span.end - span.start)
}

fn pattern_contains_select(pattern: &fluent_syntax::ast::Pattern<&str>) -> bool {
    pattern.elements.iter().any(pattern_element_contains_select)
}

fn pattern_element_contains_select(element: &fluent_syntax::ast::PatternElement<&str>) -> bool {
    match element {
        fluent_syntax::ast::PatternElement::TextElement { .. } => false,
        fluent_syntax::ast::PatternElement::Placeable { expression, .. } => {
            expression_contains_select(expression)
        }
    }
}

fn expression_contains_select(expression: &fluent_syntax::ast::Expression<&str>) -> bool {
    match expression {
        fluent_syntax::ast::Expression::Inline(
            fluent_syntax::ast::InlineExpression::Placeable { expression, .. },
            _,
        ) => expression_contains_select(expression),
        fluent_syntax::ast::Expression::Inline(_, _) => false,
        fluent_syntax::ast::Expression::Select { .. } => true,
    }
}

fn collect_pattern_spans(
    pattern: &fluent_syntax::ast::Pattern<&str>,
    spans: &mut Vec<ByteRange<usize>>,
) {
    spans.push(pattern.span.0.clone());
    for element in &pattern.elements {
        if let fluent_syntax::ast::PatternElement::Placeable { expression, .. } = element {
            collect_pattern_spans_from_expression(expression, spans);
        }
    }
}

fn collect_pattern_refs<'a>(
    pattern: &'a fluent_syntax::ast::Pattern<&'a str>,
    patterns: &mut Vec<&'a fluent_syntax::ast::Pattern<&'a str>>,
) {
    patterns.push(pattern);
    for element in &pattern.elements {
        if let fluent_syntax::ast::PatternElement::Placeable { expression, .. } = element {
            collect_pattern_refs_from_expression(expression, patterns);
        }
    }
}

fn collect_pattern_refs_from_expression<'a>(
    expression: &'a fluent_syntax::ast::Expression<&'a str>,
    patterns: &mut Vec<&'a fluent_syntax::ast::Pattern<&'a str>>,
) {
    if let fluent_syntax::ast::Expression::Select { variants, .. } = expression {
        for variant in variants {
            collect_pattern_refs(&variant.value, patterns);
        }
    }
}

fn collect_pattern_spans_from_expression(
    expression: &fluent_syntax::ast::Expression<&str>,
    spans: &mut Vec<ByteRange<usize>>,
) {
    if let fluent_syntax::ast::Expression::Select { variants, .. } = expression {
        for variant in variants {
            collect_pattern_spans(&variant.value, spans);
        }
    }
}

fn selector_binding_for_target(
    selected_function: Option<&FunctionSelectorTarget>,
    selected_variable: Option<&VariablePlaceable>,
    fallback_variable: Option<&VariablePlaceable>,
    use_snippets: bool,
) -> VariableBinding {
    if let Some(function) = selected_function {
        return VariableBinding::Concrete(function.selector_text.clone());
    }
    if let Some(variable) = selected_variable {
        return VariableBinding::Concrete(variable.name.clone());
    }

    let default_name = fallback_variable
        .map(|variable| variable.name.trim_start_matches('$').to_string())
        .unwrap_or_else(|| "count".to_string());

    if use_snippets {
        VariableBinding::Placeholder(default_name)
    } else {
        VariableBinding::Concrete(format!("${default_name}"))
    }
}

fn render_selector_token(binding: &VariableBinding) -> String {
    match binding {
        VariableBinding::Concrete(name) => name.clone(),
        VariableBinding::Placeholder(default_name) => {
            format!("\\$${{1:{default_name}}}")
        }
    }
}

fn direct_variable_placeable_name<'a>(
    element: &'a fluent_syntax::ast::PatternElement<&'a str>,
) -> Option<&'a str> {
    let fluent_syntax::ast::PatternElement::Placeable { expression, .. } = element else {
        return None;
    };
    let fluent_syntax::ast::Expression::Inline(
        fluent_syntax::ast::InlineExpression::VariableReference { id, .. },
        _,
    ) = expression
    else {
        return None;
    };
    Some(id.name)
}

fn variant_starts_with_direct_selector(
    pattern: &fluent_syntax::ast::Pattern<&str>,
    selector_name: &str,
) -> bool {
    pattern
        .elements
        .first()
        .and_then(direct_variable_placeable_name)
        == Some(selector_name)
}

fn selector_prefix_anchor_index(
    elements: &[&fluent_syntax::ast::PatternElement<&str>],
    selector_name: &str,
) -> Option<usize> {
    let anchor_index = elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            (direct_variable_placeable_name(element) == Some(selector_name)).then_some(index)
        })
        .last()?;
    elements[anchor_index + 1..]
        .iter()
        .all(|element| whitespace_text_element(element))
        .then_some(anchor_index)
}

fn whitespace_text_element(element: &fluent_syntax::ast::PatternElement<&str>) -> bool {
    match element {
        fluent_syntax::ast::PatternElement::TextElement { value, .. } => {
            value.chars().all(char::is_whitespace)
        }
        fluent_syntax::ast::PatternElement::Placeable { .. } => false,
    }
}

fn render_pattern_elements(
    source: &str,
    elements: &[&fluent_syntax::ast::PatternElement<&str>],
) -> Option<String> {
    let mut rendered = String::new();
    for element in elements {
        rendered.push_str(pattern_element_source_text(source, element)?);
    }
    Some(rendered)
}

fn render_pattern_element_slice(
    source: &str,
    elements: &[fluent_syntax::ast::PatternElement<&str>],
) -> Option<String> {
    let mut rendered = String::new();
    for element in elements {
        rendered.push_str(pattern_element_source_text(source, element)?);
    }
    Some(rendered)
}

fn trim_rendered_variant_body(body: String) -> String {
    let mut end = body.len();
    loop {
        let prefix = &body[..end];
        let Some(line_start) = prefix.rfind('\n').map(|index| index + 1) else {
            break;
        };
        if prefix[line_start..].chars().all(char::is_whitespace) {
            end = line_start.saturating_sub(1);
        } else {
            break;
        }
    }

    body[..end].trim_end_matches('\n').to_string()
}

fn pattern_source_text(
    source: &str,
    pattern: &fluent_syntax::ast::Pattern<&str>,
) -> Option<String> {
    source
        .get(trim_trailing_newlines_from_span(
            source,
            pattern.span.0.clone(),
        ))
        .map(ToString::to_string)
}

fn pattern_element_source_text<'a>(
    source: &'a str,
    element: &fluent_syntax::ast::PatternElement<&str>,
) -> Option<&'a str> {
    match element {
        fluent_syntax::ast::PatternElement::TextElement { span, .. } => source.get(span.0.clone()),
        fluent_syntax::ast::PatternElement::Placeable { span, .. } => {
            source.get(normalize_placeable_span(source, span.0.clone()))
        }
    }
}

fn variant_key_source_text(
    source: &str,
    key: &fluent_syntax::ast::VariantKey<&str>,
) -> Option<String> {
    match key {
        fluent_syntax::ast::VariantKey::Identifier { span, .. }
        | fluent_syntax::ast::VariantKey::NumberLiteral { span, .. } => {
            source.get(span.0.clone()).map(ToString::to_string)
        }
    }
}

fn common_prefix_element_count(source: &str, variants: &[ParsedVariant<'_>]) -> usize {
    let Some(first) = variants.first() else {
        return 0;
    };
    let mut common_len = first.value.elements.len();

    for variant in variants.iter().skip(1) {
        common_len = common_len.min(variant.value.elements.len());
        let mut index = 0usize;
        while index < common_len
            && pattern_element_source_text(source, &first.value.elements[index])
                == pattern_element_source_text(source, &variant.value.elements[index])
        {
            index += 1;
        }
        common_len = index;
        if common_len == 0 {
            break;
        }
    }

    common_len
}

fn replace_variable_references(
    fragment: &str,
    fragment_start: usize,
    variables: &[VariablePlaceable],
    target_name: &str,
    replacement: &str,
) -> String {
    let fragment_end = fragment_start + fragment.len();
    let mut matches = variables
        .iter()
        .filter(|variable| {
            variable.name == target_name
                && variable.reference_span.start >= fragment_start
                && variable.reference_span.end <= fragment_end
        })
        .map(|variable| {
            (
                variable.reference_span.start - fragment_start,
                variable.reference_span.end - fragment_start,
            )
        })
        .collect::<Vec<_>>();
    matches.sort_unstable_by_key(|(start, _)| *start);

    let mut result = String::new();
    let mut cursor = 0usize;
    for (start, end) in matches {
        result.push_str(&fragment[cursor..start]);
        result.push_str(replacement);
        cursor = end;
    }
    result.push_str(&fragment[cursor..]);
    result
}

fn relative_span(outer: &ByteRange<usize>, inner: &ByteRange<usize>) -> Option<ByteRange<usize>> {
    if inner.start < outer.start || inner.end > outer.end {
        None
    } else {
        Some((inner.start - outer.start)..(inner.end - outer.start))
    }
}

fn normalize_placeable_span(source: &str, span: ByteRange<usize>) -> ByteRange<usize> {
    if source.as_bytes().get(span.end) == Some(&b'}') {
        span.start..(span.end + 1)
    } else {
        span
    }
}

fn trim_trailing_newlines_from_span(source: &str, mut span: ByteRange<usize>) -> ByteRange<usize> {
    while span.end > span.start && source.as_bytes().get(span.end - 1) == Some(&b'\n') {
        span.end -= 1;
    }
    span
}

fn line_indentation_at(source: &str, byte_index: usize) -> String {
    let line_start = source[..byte_index]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    source[line_start..byte_index]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect()
}

fn indent_selector_block(block: &str, line_indent: &str) -> String {
    if line_indent.is_empty() {
        return block.to_string();
    }

    let mut lines = block.lines();
    let Some(first_line) = lines.next() else {
        return String::new();
    };
    let mut rendered = String::from(first_line);
    for line in lines {
        rendered.push('\n');
        rendered.push_str(line_indent);
        rendered.push_str(line);
    }
    rendered
}

fn position_overlaps_byte_span(source: &str, position: Position, span: &ByteRange<usize>) -> bool {
    byte_range_to_lsp_range(source, span.clone())
        .is_some_and(|range| range_contains_position(&range, position))
}

fn parse_variant_line(trimmed: &str) -> Option<String> {
    let trimmed = trimmed.strip_prefix('*').unwrap_or(trimmed);
    let rest = trimmed.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(rest[..end].trim().to_string())
}

fn collect_fluent_keys(source: &str) -> Vec<String> {
    let resource = parse_fluent_resource(source);
    collect_fluent_keys_from_resource(&resource)
}

fn collect_fluent_keys_from_resource(resource: &Resource<&str>) -> Vec<String> {
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
        fluent_syntax::ast::PatternElement::Placeable { expression, .. } => {
            count_expression_combinations(expression)
        }
    }
}

fn count_expression_combinations(expression: &fluent_syntax::ast::Expression<&str>) -> usize {
    match expression {
        fluent_syntax::ast::Expression::Inline(..) => 1,
        fluent_syntax::ast::Expression::Select { variants, .. } => {
            variants.iter().fold(0usize, |total, variant| {
                total.saturating_add(count_pattern_combinations(&variant.value))
            })
        }
    }
}

fn parse_fluent_resource(source: &str) -> Resource<&str> {
    let (resource, _errors) = parse_fluent_resource_with_errors(source);
    resource
}

fn parse_fluent_resource_with_errors(source: &str) -> (Resource<&str>, Vec<parser::ParserError>) {
    match parser::parse(source) {
        Ok(resource) => (resource, Vec::new()),
        Err((resource, errors)) => (resource, errors),
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

fn completion_site_for_position(source: &str, position: Position) -> Option<CompletionSite> {
    let byte_index = position_to_byte_index(source, position)?;
    let line_start = source[..byte_index]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line_end = source[byte_index..]
        .find('\n')
        .map(|offset| byte_index + offset)
        .unwrap_or(source.len());
    let line = source.get(line_start..line_end)?;
    let cursor_in_line = byte_index.checked_sub(line_start)?;
    let before_cursor = line.get(..cursor_in_line)?;
    let trimmed_before = before_cursor.trim_start();
    if trimmed_before.is_empty() || trimmed_before.starts_with('#') || before_cursor.contains('=') {
        return None;
    }

    let indent = leading_spaces(line);
    if indent == 0 {
        let prefix = trimmed_before.trim_end();
        if prefix.starts_with('.') || !is_completion_prefix(prefix, false) {
            return None;
        }
        return Some(CompletionSite::MessageKey {
            prefix: prefix.to_string(),
        });
    }

    if cursor_in_line < indent {
        return None;
    }
    if !trimmed_before.starts_with('.') || !is_completion_prefix(trimmed_before, true) {
        return None;
    }
    let message_key = enclosing_message_key_for_attribute_completion(source, position.line)?;
    Some(CompletionSite::AttributeKey {
        message_key,
        prefix: trimmed_before.to_string(),
    })
}

fn is_completion_prefix(prefix: &str, allow_bare_dot: bool) -> bool {
    if prefix.is_empty() {
        return false;
    }
    if allow_bare_dot && prefix == "." {
        return true;
    }
    prefix.bytes().all(|byte| {
        if allow_bare_dot && byte == b'.' {
            return true;
        }
        is_key_byte(Some(byte)) && byte != b'.'
    })
}

fn enclosing_message_key_for_attribute_completion(source: &str, line: u32) -> Option<String> {
    let target_line = usize::try_from(line).ok()?;
    let lines = source.lines().collect::<Vec<_>>();
    for current in (0..target_line).rev() {
        let line = *lines.get(current)?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if leading_spaces(line) != 0 {
            continue;
        }
        let (candidate, _) = trimmed.split_once('=')?;
        let candidate = candidate.trim_end();
        if candidate.is_empty()
            || candidate.starts_with('.')
            || candidate.starts_with('-')
            || !candidate
                .bytes()
                .all(|byte| is_key_byte(Some(byte)) && byte != b'.')
        {
            return None;
        }
        return Some(candidate.to_string());
    }
    None
}

fn completion_items_for_site(origin_source: &str, site: &CompletionSite) -> Vec<CompletionItem> {
    match site {
        CompletionSite::MessageKey { prefix } => origin_message_keys(origin_source)
            .into_iter()
            .filter(|key| key.starts_with(prefix))
            .enumerate()
            .map(|(index, key)| {
                completion_item(
                    origin_source,
                    key,
                    "message",
                    CompletionItemKind::TEXT,
                    index,
                )
            })
            .collect(),
        CompletionSite::AttributeKey {
            message_key,
            prefix,
        } => origin_message_attributes(origin_source, message_key)
            .into_iter()
            .map(|attribute| format!(".{attribute}"))
            .filter(|attribute| attribute.starts_with(prefix))
            .enumerate()
            .map(|(index, key)| {
                completion_item(
                    origin_source,
                    format!("{message_key}{key}"),
                    "attribute",
                    CompletionItemKind::FIELD,
                    index,
                )
            })
            .collect(),
    }
}

fn completion_item(
    origin_source: &str,
    key: String,
    detail: &str,
    kind: CompletionItemKind,
    index: usize,
) -> CompletionItem {
    let label = key
        .rsplit_once('.')
        .map(|(_, attribute_key)| format!(".{attribute_key}"))
        .unwrap_or_else(|| key.clone());
    CompletionItem {
        label: label.clone(),
        insert_text: Some(label),
        detail: Some(detail.to_string()),
        documentation: completion_item_documentation(origin_source, &key),
        kind: Some(kind),
        sort_text: Some(format!("{index:04}")),
        ..CompletionItem::default()
    }
}

fn completion_item_documentation(origin_source: &str, key: &str) -> Option<Documentation> {
    let rendered = render_fluent_source(origin_source, key)?;
    let mut sections = Vec::new();
    if let Some(comments) = rendered
        .comments
        .as_deref()
        .filter(|comments| !comments.is_empty())
    {
        sections.push(render_ftl_block(comments));
    }
    sections.push(render_ftl_block(&rendered.source));
    Some(Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: sections.join("\n\n---\n\n"),
    }))
}

fn origin_message_keys(source: &str) -> Vec<String> {
    parse_fluent_resource(source)
        .body
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Message(message) => Some(message.id.name.to_string()),
            _ => None,
        })
        .collect()
}

fn origin_message_attributes(source: &str, message_key: &str) -> Vec<String> {
    parse_fluent_resource(source)
        .body
        .into_iter()
        .find_map(|entry| match entry {
            Entry::Message(message) if message.id.name == message_key => Some(
                message
                    .attributes
                    .into_iter()
                    .map(|attribute| attribute.id.name.to_string())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MissingEntryRenderMode {
    EmptyStub,
    CopySource,
}

impl MissingTranslationEntries {
    fn is_empty(&self) -> bool {
        self.missing_messages.is_empty() && self.missing_attributes.is_empty()
    }
}

fn collect_missing_translation_entries(
    origin_source: &str,
    translation_source: &str,
) -> MissingTranslationEntries {
    let templates = origin_message_templates(origin_source);
    let existing = translation_message_attributes(translation_source);
    let mut missing = MissingTranslationEntries::default();

    for template in templates {
        let Some(existing_attributes) = existing.get(&template.key) else {
            missing.missing_messages.push(template);
            continue;
        };
        let attributes = template
            .attributes
            .iter()
            .filter(|attribute| !existing_attributes.contains(attribute.key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !attributes.is_empty() {
            missing.missing_attributes.push(MissingAttributePatch {
                message_key: template.key.clone(),
                attributes,
            });
        }
    }

    missing
}

fn origin_message_templates(source: &str) -> Vec<OriginMessageTemplate> {
    let resource = parse_fluent_resource(source);
    resource
        .body
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Message(message) => {
                let key = message.id.name.to_string();
                let source = render_fluent_entry(source, &key)?;
                let entry = Entry::Message(message.clone());
                let attributes = message
                    .attributes
                    .iter()
                    .filter_map(|attribute| {
                        Some(OriginAttributeTemplate {
                            key: attribute.id.name.to_string(),
                            source: render_attribute_source(attribute.id.name, &entry)?,
                        })
                    })
                    .collect::<Vec<_>>();
                Some(OriginMessageTemplate {
                    key,
                    has_value: message.value.is_some(),
                    attributes,
                    source,
                })
            }
            _ => None,
        })
        .collect()
}

fn translation_message_attributes(source: &str) -> HashMap<String, HashSet<String>> {
    let resource = parse_fluent_resource(source);
    resource
        .body
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Message(message) => Some((
                message.id.name.to_string(),
                message
                    .attributes
                    .into_iter()
                    .map(|attribute| attribute.id.name.to_string())
                    .collect::<HashSet<_>>(),
            )),
            _ => None,
        })
        .collect()
}

fn build_missing_entries_workspace_edit(
    uri: &Uri,
    source: &str,
    missing: &MissingTranslationEntries,
    mode: MissingEntryRenderMode,
) -> Option<WorkspaceEdit> {
    let mut insertions = BTreeMap::<usize, String>::new();
    for patch in &missing.missing_attributes {
        let insertion = render_missing_attribute_patch(patch, mode);
        let offset = message_block_insert_offset(source, &patch.message_key)?;
        insertions
            .entry(offset)
            .and_modify(|existing| existing.push_str(&insertion))
            .or_insert(insertion);
        if mode == MissingEntryRenderMode::CopySource {
            let marker_insertion = render_missing_attribute_markers(patch);
            let marker_offset = message_block_start_offset(source, &patch.message_key)?;
            insertions
                .entry(marker_offset)
                .and_modify(|existing| existing.push_str(&marker_insertion))
                .or_insert(marker_insertion);
        }
    }

    if !missing.missing_messages.is_empty() {
        let insertion = render_missing_messages_appendix(source, &missing.missing_messages, mode);
        insertions
            .entry(source.len())
            .and_modify(|existing| existing.push_str(&insertion))
            .or_insert(insertion);
    }

    if insertions.is_empty() {
        return None;
    }
    let edits = insertions
        .into_iter()
        .map(|(offset, insertion)| {
            let range = byte_range_to_lsp_range(source, offset..offset)?;
            Some(TextEdit::new(range, insertion))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn build_single_message_copy_code_action(
    uri: &Uri,
    path: &Path,
    translation_source: &str,
    origin_source: &str,
    position: Position,
) -> Option<CodeAction> {
    let key = extract_definition_key(translation_source, path, position)?;
    let (message_key, attribute_key) = split_fluent_key(&key);
    if attribute_key.is_some() {
        return None;
    }

    let template = origin_message_templates(origin_source)
        .into_iter()
        .find(|template| template.key == message_key)?;

    if message_matches_empty_stub(translation_source, &template) {
        let edit = build_replace_message_workspace_edit(
            uri,
            translation_source,
            message_key,
            render_missing_message(&template, MissingEntryRenderMode::CopySource),
        )?;
        return Some(CodeAction {
            title: format!("Copy `{message_key}` from source"),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(edit),
            ..CodeAction::default()
        });
    }

    let missing = collect_missing_translation_entries(origin_source, translation_source);
    let patch = missing
        .missing_attributes
        .into_iter()
        .find(|patch| patch.message_key == message_key)?;
    let edit = build_missing_entries_workspace_edit(
        uri,
        translation_source,
        &MissingTranslationEntries {
            missing_messages: Vec::new(),
            missing_attributes: vec![patch],
        },
        MissingEntryRenderMode::CopySource,
    )?;
    Some(CodeAction {
        title: format!("Copy missing attributes for `{message_key}` from source"),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(edit),
        ..CodeAction::default()
    })
}

fn render_missing_messages_appendix(
    source: &str,
    missing_messages: &[OriginMessageTemplate],
    mode: MissingEntryRenderMode,
) -> String {
    let rendered = missing_messages
        .iter()
        .map(|message| render_missing_message(message, mode))
        .collect::<Vec<_>>()
        .join("\n");
    if source.trim().is_empty() {
        rendered
    } else if source.ends_with("\n\n") {
        rendered
    } else if source.ends_with('\n') {
        format!("\n{rendered}")
    } else {
        format!("\n\n{rendered}")
    }
}

fn render_missing_message(message: &OriginMessageTemplate, mode: MissingEntryRenderMode) -> String {
    match mode {
        MissingEntryRenderMode::EmptyStub => {
            let mut rendered = if message.has_value {
                format!("{} = {{ \"\" }}\n", message.key)
            } else {
                format!("{} =\n", message.key)
            };
            for attribute in &message.attributes {
                rendered.push_str(&format!("    .{} =\n", attribute.key));
            }
            rendered
        }
        MissingEntryRenderMode::CopySource => {
            let mut rendered = format!("{LSP_COPY_MARKER}\n{}", message.source);
            if !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            rendered
        }
    }
}

fn render_missing_attribute_patch(
    patch: &MissingAttributePatch,
    mode: MissingEntryRenderMode,
) -> String {
    match mode {
        MissingEntryRenderMode::EmptyStub => patch
            .attributes
            .iter()
            .map(|attribute| format!("    .{} = {{ \"\" }}\n", attribute.key))
            .collect::<String>(),
        MissingEntryRenderMode::CopySource => patch
            .attributes
            .iter()
            .map(|attribute| {
                let mut rendered = String::new();
                for line in attribute.source.lines() {
                    rendered.push_str("    ");
                    rendered.push_str(line);
                    rendered.push('\n');
                }
                rendered
            })
            .collect::<String>(),
    }
}

fn render_missing_attribute_markers(patch: &MissingAttributePatch) -> String {
    patch
        .attributes
        .iter()
        .map(|attribute| format!("{}\n", render_attribute_copy_marker(&attribute.key)))
        .collect()
}

fn message_block_insert_offset(source: &str, message_key: &str) -> Option<usize> {
    let (_, end_line) = find_fluent_block_line_range(source, message_key)?;
    end_of_line_offset(source, end_line)
}

fn message_block_start_offset(source: &str, message_key: &str) -> Option<usize> {
    let (start_line, _) = find_fluent_block_line_range(source, message_key)?;
    line_start_offset(source, start_line)
}

fn end_of_line_offset(source: &str, line_index: usize) -> Option<usize> {
    let mut current_line = 0usize;
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        if current_line == line_index {
            return Some(offset + line.len());
        }
        offset += line.len();
        current_line += 1;
    }
    if current_line == line_index {
        Some(source.len())
    } else {
        None
    }
}

fn line_start_offset(source: &str, line_index: usize) -> Option<usize> {
    if line_index == 0 {
        return Some(0);
    }

    let mut current_line = 0usize;
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        current_line += 1;
        offset += line.len();
        if current_line == line_index {
            return Some(offset);
        }
    }
    None
}

fn build_replace_message_workspace_edit(
    uri: &Uri,
    source: &str,
    message_key: &str,
    replacement: String,
) -> Option<WorkspaceEdit> {
    let (start_line, end_line) = find_fluent_block_line_range(source, message_key)?;
    let start = line_start_offset(source, start_line)?;
    let end = end_of_line_offset(source, end_line)?;
    let range = byte_range_to_lsp_range(source, start..end)?;

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![TextEdit::new(range, replacement)]);
    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn message_matches_empty_stub(source: &str, template: &OriginMessageTemplate) -> bool {
    render_fluent_entry(source, &template.key)
        .map(|entry| {
            entry.trim_end()
                == render_missing_message(template, MissingEntryRenderMode::EmptyStub).trim_end()
        })
        .unwrap_or(false)
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
                    .map(|attribute| attribute.id.span.0.clone())
            } else {
                Some(message.id.span.0.clone())
            }
        }
        Entry::Term(term) if entry_key == format!("-{}", term.id.name) => {
            if let Some(attribute_key) = attribute_key {
                term.attributes
                    .iter()
                    .find(|attribute| attribute.id.name == attribute_key)
                    .map(|attribute| attribute.id.span.0.clone())
            } else {
                Some(term.id.span.0.clone())
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
        } else if indent == 0 {
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
    let mut resolved_overrides: HashMap<String, String> = HashMap::new();

    for (index, line) in lines
        .iter()
        .enumerate()
        .take(line_index + 1)
        .skip(start_line)
    {
        let trimmed = line.trim_start();
        let indent = leading_spaces(line);

        if trimmed.starts_with('}') {
            while selectors
                .last()
                .is_some_and(|selector: &ActiveSelectorContext| indent <= selector.indent)
            {
                let popped = selectors.pop().expect("checked by is_some_and");
                if let Some(variant) = popped.current_variant {
                    if index < line_index || cursor_character > indent {
                        resolved_overrides.insert(popped.name, variant);
                    }
                }
            }
        }

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
    }

    let mut overrides: HashMap<String, String> = selectors
        .into_iter()
        .filter_map(|selector| {
            selector
                .current_variant
                .map(|variant| (selector.name, variant))
        })
        .collect();

    for (name, variant) in resolved_overrides {
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
        fluent_syntax::ast::PatternElement::TextElement { value, .. } => SelectorExpansion {
            items: vec![SelectorExpansionItem {
                selectors: Vec::new(),
                text: (*value).to_string(),
            }],
            total_count: 1,
        },
        fluent_syntax::ast::PatternElement::Placeable { expression, .. } => {
            expand_expression(expression, max_items)
        }
    }
}

fn expand_expression(
    expression: &fluent_syntax::ast::Expression<&str>,
    max_items: usize,
) -> SelectorExpansion {
    match expression {
        fluent_syntax::ast::Expression::Inline(inline, _) => SelectorExpansion {
            items: vec![SelectorExpansionItem {
                selectors: Vec::new(),
                text: render_inline_expression_preview_text(inline),
            }],
            total_count: 1,
        },
        fluent_syntax::ast::Expression::Select {
            selector, variants, ..
        } => {
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
        fluent_syntax::ast::VariantKey::Identifier { name, .. } => (*name).to_string(),
        fluent_syntax::ast::VariantKey::NumberLiteral { value, .. } => (*value).to_string(),
    }
}

fn render_inline_expression_as_text(
    expression: &fluent_syntax::ast::InlineExpression<&str>,
) -> String {
    format!("{{ {} }}", render_inline_expression(expression))
}

fn render_inline_expression_preview_text(
    expression: &fluent_syntax::ast::InlineExpression<&str>,
) -> String {
    match expression {
        fluent_syntax::ast::InlineExpression::StringLiteral { value, .. } if value.is_empty() => {
            String::new()
        }
        _ => render_inline_expression_as_text(expression),
    }
}

fn render_inline_expression(expression: &fluent_syntax::ast::InlineExpression<&str>) -> String {
    match expression {
        fluent_syntax::ast::InlineExpression::StringLiteral { value, .. } => {
            format!("\"{value}\"")
        }
        fluent_syntax::ast::InlineExpression::NumberLiteral { value, .. } => (*value).to_string(),
        fluent_syntax::ast::InlineExpression::FunctionReference { id, .. } => {
            format!("{}()", id.name)
        }
        fluent_syntax::ast::InlineExpression::MessageReference { id, attribute, .. } => {
            match attribute {
                Some(attribute) => format!("{}.{}", id.name, attribute.name),
                None => id.name.to_string(),
            }
        }
        fluent_syntax::ast::InlineExpression::TermReference {
            id,
            attribute,
            arguments,
            ..
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
        fluent_syntax::ast::InlineExpression::VariableReference { id, .. } => {
            format!("${}", id.name)
        }
        fluent_syntax::ast::InlineExpression::Placeable { expression, .. } => {
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
        fluent_syntax::ast::Expression::Inline(inline, _) => render_inline_expression(inline),
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

fn position_to_byte_index(source: &str, position: Position) -> Option<usize> {
    let (line, line_start) = line_at(source, position.line as usize)?;
    let line_offset = utf16_position_to_byte_index(line, position.character as usize)?;
    Some(line_start + line_offset)
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

#[cfg(test)]
mod tests {
    use super::*;
    use fluent_bundle::{FluentBundle, FluentResource};
    use unic_langid::LanguageIdentifier;

    fn indexed_test_workspace(root: &Path) -> WorkspaceConfig {
        std::fs::write(
            root.join("fluent-lsp.toml"),
            "origin_language = \"en\"\nfile_masks = [\"locales/{lang}/{filepath}.ftl\"]\n",
        )
        .unwrap();
        WorkspaceConfig::load(root.to_path_buf()).unwrap()
    }

    #[test]
    fn index_extracts_per_file_keys_definitions_and_metadata() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
        let workspace = indexed_test_workspace(temp.path());
        let path = temp.path().join("locales/en/app.ftl");
        let file_match = workspace.file_match(&path).unwrap();
        let indexed = index_fluent_file(
            path,
            file_match,
            "# Docs\nhello = Hello\nmenu =\n    .label = Menu\n".to_string(),
        );

        assert!(indexed.keys.contains("hello"));
        assert!(indexed.keys.contains("menu.label"));
        assert_eq!(indexed.definitions["hello"].start.line, 1);
        assert_eq!(indexed.definitions["menu.label"].start.line, 3);
        assert_eq!(indexed.file_match.language, "en");
        assert_eq!(indexed.file_match.filepath, "app");
    }

    #[test]
    fn index_replaces_and_removes_one_file_without_losing_reverse_maps() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
        std::fs::create_dir_all(temp.path().join("locales/es")).unwrap();
        let workspace = indexed_test_workspace(temp.path());
        let origin = temp.path().join("locales/en/app.ftl");
        let local = temp.path().join("locales/es/app.ftl");
        std::fs::write(&origin, "hello = Hello\n").unwrap();
        std::fs::write(&local, "hello = Hola\n").unwrap();

        let mut index = WorkspaceIndex::build(&workspace, &HashMap::new());
        let local_file = index.file(&local).unwrap().clone();
        assert_eq!(
            index.origin_for(&workspace, &local_file).unwrap().path,
            origin
        );
        assert_eq!(
            index
                .translations_for(&workspace, index.file(&origin).unwrap())
                .len(),
            1
        );

        std::fs::write(&local, "renamed = Hola\n").unwrap();
        index.replace_file(&workspace, &local, &HashMap::new());
        assert!(!index.file(&local).unwrap().keys.contains("hello"));
        assert!(index.file(&local).unwrap().keys.contains("renamed"));

        index.remove_file(&local);
        assert!(index.file(&local).is_none());
        assert!(
            index
                .translations_for(&workspace, index.file(&origin).unwrap())
                .is_empty()
        );
    }

    #[test]
    fn index_overlay_text_takes_precedence_and_close_reverts_to_disk() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
        let workspace = indexed_test_workspace(temp.path());
        let path = temp.path().join("locales/en/app.ftl");
        std::fs::write(&path, "disk-key = Disk\n").unwrap();
        let uri = Uri::from_file_path(&path).unwrap();
        let mut overlays = HashMap::new();
        overlays.insert(uri, "overlay-key = Overlay\n".to_string());

        let mut index = WorkspaceIndex::build(&workspace, &overlays);
        assert!(index.file(&path).unwrap().keys.contains("overlay-key"));
        assert!(!index.file(&path).unwrap().keys.contains("disk-key"));

        index.replace_file(&workspace, &path, &HashMap::new());
        assert!(index.file(&path).unwrap().keys.contains("disk-key"));
        assert!(!index.file(&path).unwrap().keys.contains("overlay-key"));
    }

    #[test]
    fn index_identifies_local_only_files_and_recovers_from_parse_errors() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("locales/es")).unwrap();
        let workspace = indexed_test_workspace(temp.path());
        let local = temp.path().join("locales/es/only.ftl");
        std::fs::write(&local, "broken\nvalid = Value\n").unwrap();

        let mut index = WorkspaceIndex::build(&workspace, &HashMap::new());
        assert_eq!(index.local_only_files(&workspace).len(), 1);
        assert!(index.file(&local).unwrap().keys.contains("valid"));

        std::fs::create_dir_all(temp.path().join("locales/en")).unwrap();
        let origin = temp.path().join("locales/en/only.ftl");
        std::fs::write(&origin, "valid = Origin\n").unwrap();
        index.apply_disk_changes(&workspace, &HashMap::new(), &[origin], &[]);
        assert!(index.local_only_files(&workspace).is_empty());
    }

    #[test]
    fn workspace_discovery_roots_follow_file_mask_prefixes() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = WorkspaceConfig {
            root_dir: temp.path().to_path_buf(),
            origin_language: "en".to_string(),
            file_masks: compile_file_masks(&[
                "assets/voxygen/i18n/{lang}/{filepath}.ftl".to_string(),
                "locales/{lang}/{filepath}.ftl".to_string(),
                "{lang}/{filepath}.ftl".to_string(),
            ])
            .unwrap(),
            selector_style: None,
            error_on_unsupported_plural_categories: None,
            warn_on_missing_plural_categories: None,
            warn_on_selector_style_mismatch: None,
        };

        let roots = workspace.discovery_roots();
        assert_eq!(
            roots,
            vec![
                temp.path().to_path_buf(),
                temp.path().join("assets/voxygen/i18n"),
                temp.path().join("locales"),
            ]
        );
    }

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

        let key = extract_definition_key(
            source,
            Path::new("locales/en/app.ftl"),
            Position::new(2, 18),
        )
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
                "locales/lv/app.ftl".to_string(),
                "locales/lv/dialogs/menu.ftl".to_string(),
                "locales/uk/app.ftl".to_string(),
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
    fn detects_message_key_completion_site() {
        let source = "welcome-title = Hola\n\ndown";
        assert_eq!(
            completion_site_for_position(source, Position::new(2, 4)),
            Some(CompletionSite::MessageKey {
                prefix: "down".to_string(),
            })
        );
    }

    #[test]
    fn detects_attribute_completion_site_for_partial_and_bare_dot() {
        let source = "menu-save =\n    .l";
        assert_eq!(
            completion_site_for_position(source, Position::new(1, 6)),
            Some(CompletionSite::AttributeKey {
                message_key: "menu-save".to_string(),
                prefix: ".l".to_string(),
            })
        );

        let bare_dot = "menu-save =\n    .";
        assert_eq!(
            completion_site_for_position(bare_dot, Position::new(1, 5)),
            Some(CompletionSite::AttributeKey {
                message_key: "menu-save".to_string(),
                prefix: ".".to_string(),
            })
        );
    }

    #[test]
    fn suppresses_completion_inside_message_values() {
        let source = "welcome-title = Hola\n    cont";
        assert_eq!(
            completion_site_for_position(source, Position::new(1, 8)),
            None
        );
    }

    #[test]
    fn collects_message_key_completion_items_in_origin_order() {
        let source = "hello-world = Hello\ndownload-action = Download\ndownload-count = Count\n";
        let labels = completion_items_for_site(
            source,
            &CompletionSite::MessageKey {
                prefix: "down".to_string(),
            },
        )
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec!["download-action".to_string(), "download-count".to_string()]
        );
    }

    #[test]
    fn collects_attribute_completion_items_for_matching_message_only() {
        let source = "menu-save =\n    .label = Save\n    .tooltip = Save this file\nmenu-cancel =\n    .label = Cancel\n";
        let labels = completion_items_for_site(
            source,
            &CompletionSite::AttributeKey {
                message_key: "menu-save".to_string(),
                prefix: ".".to_string(),
            },
        )
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
        assert_eq!(labels, vec![".label".to_string(), ".tooltip".to_string()]);

        let filtered = completion_items_for_site(
            source,
            &CompletionSite::AttributeKey {
                message_key: "menu-save".to_string(),
                prefix: ".l".to_string(),
            },
        )
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
        assert_eq!(filtered, vec![".label".to_string()]);
    }

    #[test]
    fn completion_items_include_origin_documentation() {
        let source = "# Comment-only hover coverage\n# Keep this translator guidance visible on key hover\ncommented-preview = Preview text for hover comments.\n";
        let item = completion_items_for_site(
            source,
            &CompletionSite::MessageKey {
                prefix: "commented".to_string(),
            },
        )
        .into_iter()
        .next()
        .unwrap();

        let Documentation::MarkupContent(markup) = item.documentation.unwrap() else {
            panic!("expected markdown completion documentation");
        };
        assert_eq!(markup.kind, MarkupKind::Markdown);
        assert!(markup.value.contains("# Comment-only hover coverage"));
        assert!(
            markup
                .value
                .contains("commented-preview = Preview text for hover comments.")
        );
    }

    #[test]
    fn detects_missing_messages_and_attributes_from_origin() {
        let origin = "hello = Hello\n\
download-action =\n\
    .label = Download\n\
    .tooltip = Download this build\n\
";
        let translation = "hello = Hola\n\
download-action =\n\
    .label = Descargar\n\
";

        let missing = collect_missing_translation_entries(origin, translation);
        assert!(missing.missing_messages.is_empty());
        assert_eq!(missing.missing_attributes.len(), 1);
        assert_eq!(missing.missing_attributes[0].message_key, "download-action");
        assert_eq!(
            missing.missing_attributes[0]
                .attributes
                .iter()
                .map(|attribute| attribute.key.clone())
                .collect::<Vec<_>>(),
            vec!["tooltip".to_string()]
        );
    }

    #[test]
    fn preserves_origin_order_for_missing_message_appendix() {
        let origin = "alpha = Alpha\nbeta = Beta\ngamma = Gamma\n";
        let translation = "alpha = Alfa\n";

        let missing = collect_missing_translation_entries(origin, translation);
        assert_eq!(
            missing
                .missing_messages
                .iter()
                .map(|message| message.key.clone())
                .collect::<Vec<_>>(),
            vec!["beta".to_string(), "gamma".to_string()]
        );
    }

    #[test]
    fn builds_empty_stub_edit_for_missing_messages_and_attributes() {
        let origin = "hello = Hello\n\
download-action =\n\
    .label = Download\n\
    .tooltip = Download this build\n\
";
        let translation = "download-action =\n    .label = Descargar\n";
        let missing = collect_missing_translation_entries(origin, translation);
        let edit = build_missing_entries_workspace_edit(
            &Uri::from_file_path("/tmp/app.ftl").unwrap(),
            translation,
            &missing,
            MissingEntryRenderMode::EmptyStub,
        )
        .unwrap();
        let updated = apply_workspace_edit_to_source(translation, &edit);

        assert_eq!(
            updated,
            "download-action =\n    .label = Descargar\n    .tooltip = { \"\" }\n\nhello = { \"\" }\n"
        );
        assert_fluent_source_parses(&updated);
    }

    #[test]
    fn builds_copy_edit_with_marker_for_missing_messages_and_attributes() {
        let origin = "hello = Hello\n\
download-action =\n\
    .label = Download\n\
    .tooltip = Download this build\n\
";
        let translation = "download-action =\n    .label = Descargar\n";
        let missing = collect_missing_translation_entries(origin, translation);
        let edit = build_missing_entries_workspace_edit(
            &Uri::from_file_path("/tmp/app.ftl").unwrap(),
            translation,
            &missing,
            MissingEntryRenderMode::CopySource,
        )
        .unwrap();
        let updated = apply_workspace_edit_to_source(translation, &edit);

        assert_eq!(
            updated,
            concat!(
                "# [LSP-COPY .tooltip]\n",
                "download-action =\n",
                "    .label = Descargar\n",
                "    .tooltip = Download this build\n",
                "\n",
                "# [LSP-COPY]\n",
                "hello = Hello\n",
            )
        );
        assert_fluent_source_parses(&updated);
        assert_eq!(
            render_fluent_preview_text(&updated, "hello", None).as_deref(),
            Some("Hello")
        );
        assert_eq!(
            render_fluent_preview_text(&updated, "download-action.tooltip", None).as_deref(),
            Some("Download this build")
        );
    }

    #[test]
    fn lsp_copy_marker_diagnostics_cover_top_level_and_attribute_markers_only() {
        let source = concat!(
            "# [LSP-COPY]\n",
            "hello = Hello\n",
            "\n",
            "# Normal translator comment\n",
            "# [LSP-COPY .tooltip]\n",
            "download-action =\n",
            "    .label = Descargar\n",
            "    .tooltip = Download this build\n",
        );
        let diagnostics = collect_lsp_copy_marker_diagnostics(source);

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].range.start, Position::new(0, 0));
        assert_eq!(diagnostics[1].range.start, Position::new(7, 5));
        assert!(diagnostics.iter().all(
            |diagnostic| diagnostic.message == "Entry still contains an `# [LSP-COPY]` marker"
        ));
    }

    #[test]
    fn detects_empty_stub_message_for_single_copy_action() {
        let origin = "hello = Hello World\n";
        let template = origin_message_templates(origin).remove(0);
        let translation = "hello = { \"\" }\n";

        assert!(message_matches_empty_stub(translation, &template));
        assert!(!message_matches_empty_stub(
            "hello = Hola Mundo\n",
            &template
        ));
    }

    #[test]
    fn replaces_stub_message_with_source_copy_marker_block() {
        let source = "hello = { \"\" }\n\
sync-status = { \"\" }\n";
        let edit = build_replace_message_workspace_edit(
            &Uri::from_file_path("/tmp/app.ftl").unwrap(),
            source,
            "hello",
            "# [LSP-COPY]\nhello = Hello World\n".to_string(),
        )
        .unwrap();
        let updated = apply_workspace_edit_to_source(source, &edit);

        assert_eq!(
            updated,
            "# [LSP-COPY]\nhello = Hello World\nsync-status = { \"\" }\n"
        );
        assert_fluent_source_parses(&updated);
    }

    #[test]
    fn real_fluent_bundle_formats_copied_message_and_attribute_text() {
        let source = "hello = Hello World\n\
download-action =\n\
    .label = Download\n\
    .tooltip = Download this build\n";

        assert_eq!(runtime_message_text(source, "en", "hello"), "Hello World");
        assert_eq!(
            runtime_message_text(source, "en", "download-action.tooltip"),
            "Download this build"
        );
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
    fn renders_selector_combinations_section_for_attribute_patterns() {
        let source = "download-action =\n    .tooltip =\n        Install the recommended build for { $gender ->\n            [female] her\n            [male] his\n           *[other] their\n        } account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n";

        let rendered = render_selector_combinations_section(
            "Current language combinations:",
            source,
            "download-action.tooltip",
            usize::MAX,
        )
        .unwrap();
        assert!(rendered.contains("`$gender=female`, `$count=one`"));
        assert!(rendered.contains(
            "```ftl\nInstall the recommended build for her account on { $count } device now.\n```"
        ));
        assert!(rendered.contains("`$gender=other`, `$count=other`"));
        assert!(rendered.contains(
            "```ftl\nInstall the recommended build for their account on { $count } devices now.\n```"
        ));
    }

    #[test]
    fn renders_selector_combinations_section_with_fixed_limit_for_rollouts() {
        let source = "audience-rollout =\n    Summary for { $audience ->\n        [admins] admins\n        [members] members\n       *[others] other people\n    } on { $platform ->\n        [desktop] desktop\n       *[mobile] mobile\n    } with { $count } { $count ->\n        [one] item\n       *[other] items\n    }.\n";

        let rendered = render_selector_combinations_section(
            "Current language combinations:",
            source,
            "audience-rollout",
            10,
        )
        .unwrap();
        assert_eq!(rendered.matches("\n```ftl\n").count(), 10);
        assert!(rendered.contains("`...`\n2 more"));
        assert!(
            !rendered
                .contains("```ftl\nSummary for other people on mobile with { $count } items.\n```")
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
        let rendered = render_hover_markdown(
            None,
            &MessagePreview {
                selectors: vec![
                    ("$gender".to_string(), "female".to_string()),
                    ("$count".to_string(), "*".to_string()),
                ],
                text: "Copy the download link for her account on { $count } devices now."
                    .to_string(),
            },
        );
        assert_eq!(
            rendered,
            "`$gender=female`, `$count=*`\n\n```ftl\nCopy the download link for her account on { $count } devices now.\n```"
        );
    }

    #[test]
    fn renders_hover_markdown_with_separator_between_source_and_current() {
        let rendered = render_hover_markdown(
            Some(&MessagePreview {
                selectors: vec![
                    ("$gender".to_string(), "female".to_string()),
                    ("$count".to_string(), "*".to_string()),
                ],
                text: "Copy the download link for her account on { $count } devices now."
                    .to_string(),
            }),
            &MessagePreview {
                selectors: vec![
                    ("$gender".to_string(), "female".to_string()),
                    ("$count".to_string(), "*".to_string()),
                ],
                text: "Copia el enlace de descarga para la cuenta de ella en { $count } dispositivos ahora."
                    .to_string(),
            },
        );
        assert_eq!(
            rendered,
            "`$gender=female`, `$count=*`\n\n```ftl\nCopy the download link for her account on { $count } devices now.\n```\n\n---\n\n`$gender=female`, `$count=*`\n\n```ftl\nCopia el enlace de descarga para la cuenta de ella en { $count } dispositivos ahora.\n```"
        );
    }

    #[test]
    fn renders_hover_markdown_without_duplicate_origin_sections() {
        let rendered = render_hover_markdown(
            None,
            &MessagePreview {
                selectors: Vec::new(),
                text: "Save".to_string(),
            },
        );
        assert_eq!(rendered.matches("---").count(), 0);
        assert_eq!(rendered, "```ftl\nSave\n```");
    }

    #[test]
    fn renders_hover_markdown_with_empty_placeholder_and_comment_sections() {
        let rendered = render_hover_markdown(
            Some(&MessagePreview {
                selectors: Vec::new(),
                text: "Hello World".to_string(),
            }),
            &MessagePreview {
                selectors: Vec::new(),
                text: String::new(),
            },
        );
        assert_eq!(
            rendered,
            "```ftl\nHello World\n```\n\n---\n\n```ftl\n<empty>\n```"
        );

        let comments = render_hover_comment_markdown(
            Some("# Origin comment\n# Keep this too\n"),
            Some("# Local comment\n# Keep this visible\n"),
        )
        .unwrap();
        assert_eq!(
            comments,
            "```ftl\n# Origin comment\n# Keep this too\n```\n\n---\n\n```ftl\n# Local comment\n# Keep this visible\n```"
        );
    }

    #[test]
    fn render_message_preview_treats_empty_string_literal_as_empty_text() {
        let source = "empty-preview = { \"\" }\n";
        let resource = parse_fluent_resource(source);
        let pattern = find_fluent_pattern(&resource, "empty-preview").unwrap();

        assert_eq!(
            render_message_preview(pattern, None),
            MessagePreview {
                selectors: Vec::new(),
                text: String::new(),
            }
        );
    }

    fn assert_fluent_source_parses(source: &str) {
        if let Err((_, errors)) = parser::parse(source) {
            panic!("failed to parse Fluent source with {errors:?}\n{source}");
        }
    }

    fn apply_workspace_edit_to_source(source: &str, edit: &WorkspaceEdit) -> String {
        let mut updated = source.to_string();
        let mut edits = edit
            .changes
            .as_ref()
            .and_then(|changes| changes.values().next())
            .cloned()
            .unwrap_or_default();
        edits.sort_by_key(|text_edit| {
            (
                std::cmp::Reverse(text_edit.range.start.line),
                std::cmp::Reverse(text_edit.range.start.character),
                std::cmp::Reverse(text_edit.range.end.line),
                std::cmp::Reverse(text_edit.range.end.character),
            )
        });

        for text_edit in edits {
            let start = position_to_byte_index(&updated, text_edit.range.start)
                .expect("text edit start should map into source");
            let end = position_to_byte_index(&updated, text_edit.range.end)
                .expect("text edit end should map into source");
            updated.replace_range(start..end, &text_edit.new_text);
        }

        updated
    }

    fn runtime_message_text(source: &str, locale: &str, key: &str) -> String {
        let resource = FluentResource::try_new(source.to_string()).unwrap_or_else(|(_, errors)| {
            panic!("failed to build FluentResource with {errors:?}\n{source}")
        });
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
        let mut errors = Vec::new();
        let rendered = bundle
            .format_pattern(pattern, None, &mut errors)
            .into_owned();
        assert!(
            errors.is_empty(),
            "runtime formatting errors for `{key}`: {errors:?}"
        );
        rendered
    }

    fn split_runtime_key(key: &str) -> (&str, Option<&str>) {
        match key.rsplit_once('.') {
            Some((message_key, attribute_key)) if !attribute_key.is_empty() => {
                (message_key, Some(attribute_key))
            }
            _ => (key, None),
        }
    }

    #[test]
    fn render_message_preview_matches_available_selectors_and_defaults_the_rest() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";
        let resource = parse_fluent_resource(source);
        let pattern = find_fluent_pattern(&resource, "install-hint").unwrap();
        let overrides = HashMap::from([("$count".to_string(), "one".to_string())]);

        assert_eq!(
            render_message_preview(pattern, Some(&overrides)),
            MessagePreview {
                selectors: vec![
                    ("$gender".to_string(), "*".to_string()),
                    ("$count".to_string(), "one".to_string()),
                ],
                text: "Copy the download link for their account on { $count } device now."
                    .to_string(),
            }
        );
    }

    #[test]
    fn render_message_preview_ignores_local_only_selectors_and_defaults_source_only_ones() {
        let source = "mismatch-rollout =\n    Summary for { $platform ->\n        [desktop] desktop\n       *[mobile] mobile\n    } users with { $count ->\n        [0] no packages\n        [1] one package\n       *[other] { $count } packages\n    } ready.\n";
        let resource = parse_fluent_resource(source);
        let pattern = find_fluent_pattern(&resource, "mismatch-rollout").unwrap();
        let overrides = HashMap::from([
            ("$gender".to_string(), "female".to_string()),
            ("$count".to_string(), "1".to_string()),
        ]);

        assert_eq!(
            render_message_preview(pattern, Some(&overrides)),
            MessagePreview {
                selectors: vec![
                    ("$platform".to_string(), "*".to_string()),
                    ("$count".to_string(), "1".to_string()),
                ],
                text: "Summary for mobile users with one package ready.".to_string(),
            }
        );
    }

    #[test]
    fn render_message_preview_supports_explicit_zero_numeric_variants() {
        let source = "numeric-rollout =\n    Summary for { $count ->\n        [0] no packages ready\n        [1] one package ready\n       *[other] { $count } packages ready\n    }.\n";
        let resource = parse_fluent_resource(source);
        let pattern = find_fluent_pattern(&resource, "numeric-rollout").unwrap();
        let overrides = HashMap::from([("$count".to_string(), "0".to_string())]);

        assert_eq!(
            render_message_preview(pattern, Some(&overrides)),
            MessagePreview {
                selectors: vec![("$count".to_string(), "0".to_string())],
                text: "Summary for no packages ready.".to_string(),
            }
        );
    }

    #[test]
    fn render_message_preview_supports_zero_category_variants() {
        let source = "zero-rollout =\n    Zero summary: { $count ->\n        [zero] no packages ready\n        [one] one package ready\n       *[other] { $count } packages ready\n    }.\n";
        let resource = parse_fluent_resource(source);
        let pattern = find_fluent_pattern(&resource, "zero-rollout").unwrap();
        let overrides = HashMap::from([("$count".to_string(), "zero".to_string())]);

        assert_eq!(
            render_message_preview(pattern, Some(&overrides)),
            MessagePreview {
                selectors: vec![("$count".to_string(), "zero".to_string())],
                text: "Zero summary: no packages ready.".to_string(),
            }
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
    fn real_fluent_parser_does_not_attach_indented_marker_between_attributes_as_comment() {
        let source = "download-action =\n    .label = Descargar\n    # [LSP-COPY]\n    .tooltip = Download this build\n";
        let resource = parser::parse(source).expect("expected valid Fluent source");

        assert_eq!(resource.body.len(), 1);
        let Entry::Message(message) = &resource.body[0] else {
            panic!("expected a message entry");
        };
        assert!(message.comment.is_none());
        assert_eq!(
            message
                .attributes
                .iter()
                .map(|attribute| attribute.id.name)
                .collect::<Vec<_>>(),
            vec!["label", "tooltip"]
        );
    }

    #[test]
    fn render_fluent_source_filters_machine_copy_markers_from_comments() {
        let source = "# Translator note\n# [LSP-COPY .tooltip]\ndownload-action =\n    .label = Descargar\n    .tooltip = Download this build\n";
        let rendered = render_fluent_source(source, "download-action.tooltip").unwrap();

        assert_eq!(rendered.comments, Some("# Translator note\n".to_string()));
        assert_eq!(
            rendered.source,
            ".tooltip = Download this build\n".to_string()
        );
    }

    #[test]
    fn selector_overrides_follow_active_variant_lines() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        assert_eq!(
            selector_overrides_for_position(source, "install-hint", Position::new(2, 18)),
            HashMap::from([("$gender".to_string(), "female".to_string())])
        );
        assert_eq!(
            selector_overrides_for_position(source, "install-hint", Position::new(0, 3)),
            HashMap::new()
        );
    }

    #[test]
    fn selector_overrides_preserve_variants_after_closing_brace_on_current_line() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        assert_eq!(
            selector_overrides_for_position(source, "install-hint", Position::new(5, 8)),
            HashMap::from([("$gender".to_string(), "other".to_string())])
        );
    }

    #[test]
    fn selector_overrides_track_concatenated_second_selector_variants() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";

        assert_eq!(
            selector_overrides_for_position(source, "install-hint", Position::new(6, 12)),
            HashMap::from([
                ("$gender".to_string(), "other".to_string()),
                ("$count".to_string(), "one".to_string()),
            ])
        );
    }

    #[test]
    fn selector_overrides_capture_explicit_numeric_variant_keys() {
        let source = "mismatch-rollout =\n    Resumen para { $gender ->\n        [female] ella misma\n        [male] el mismo\n       *[other] elle misme\n    } con { $count ->\n        [0] ningun paquete\n        [1] un paquete\n       *[other] { $count } paquetes\n    } listo.\n";

        assert_eq!(
            selector_overrides_for_position(source, "mismatch-rollout", Position::new(6, 12)),
            HashMap::from([
                ("$gender".to_string(), "other".to_string()),
                ("$count".to_string(), "0".to_string()),
            ])
        );
        assert_eq!(
            selector_overrides_for_position(source, "mismatch-rollout", Position::new(7, 12)),
            HashMap::from([
                ("$gender".to_string(), "other".to_string()),
                ("$count".to_string(), "1".to_string()),
            ])
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
        assert_eq!(
            rendered.matches("Current language combinations:").count(),
            1
        );
    }

    #[test]
    fn local_fluent_syntax_fork_exposes_identifier_spans() {
        let resource = parser::parse("welcome-title = Welcome\n").unwrap();
        match &resource.body[0] {
            Entry::Message(message) => {
                assert_eq!(message.id.span, fluent_syntax::ast::Span(0..13))
            }
            _ => panic!("expected message entry"),
        }
    }

    #[test]
    fn local_fluent_syntax_fork_exposes_variant_and_variant_key_spans() {
        let source = "count = { $count ->\n    [0] Zero\n   *[other] Other\n}\n";
        let resource = parser::parse(source).unwrap();
        let Entry::Message(message) = &resource.body[0] else {
            panic!("expected message entry");
        };
        let pattern = message.value.as_ref().expect("expected message value");
        let fluent_syntax::ast::PatternElement::Placeable { expression, .. } = &pattern.elements[0]
        else {
            panic!("expected select placeable");
        };
        let fluent_syntax::ast::Expression::Select { variants, .. } = expression else {
            panic!("expected select expression");
        };

        assert_eq!(
            &source[variants[0].span.start..variants[0].span.end],
            "[0] Zero\n"
        );
        match &variants[1].key {
            fluent_syntax::ast::VariantKey::Identifier { span, .. } => {
                assert_eq!(&source[span.start..span.end], "other");
            }
            _ => panic!("expected identifier variant key"),
        }
    }

    #[test]
    fn parses_client_selector_style_from_nested_settings() {
        let settings = serde_json::json!({
            "fluent-lsp": {
                "selector_style": "whole",
                "warn_on_missing_plural_categories": true
            }
        });

        assert_eq!(
            parse_client_config(&settings),
            ClientConfig {
                selector_style: Some(SelectorStyle::Whole),
                warn_on_missing_plural_categories: Some(true),
                ..ClientConfig::default()
            }
        );
    }

    #[test]
    fn workspace_diagnostic_config_prefers_file_values_over_client_settings() {
        let workspace = WorkspaceConfig {
            root_dir: PathBuf::from("."),
            origin_language: "en".to_string(),
            file_masks: Vec::new(),
            selector_style: Some(SelectorStyle::Whole),
            error_on_unsupported_plural_categories: Some(false),
            warn_on_missing_plural_categories: Some(true),
            warn_on_selector_style_mismatch: Some(false),
        };

        let effective = workspace.effective_diagnostic_config(ClientConfig {
            selector_style: Some(SelectorStyle::Prefix),
            error_on_unsupported_plural_categories: Some(true),
            warn_on_missing_plural_categories: Some(false),
            warn_on_selector_style_mismatch: Some(true),
        });

        assert_eq!(
            effective,
            EffectiveDiagnosticConfig {
                error_on_unsupported_plural_categories: false,
                warn_on_missing_plural_categories: true,
                warn_on_selector_style_mismatch: false,
                preferred_selector_style: Some(SelectorStyle::Whole),
            }
        );
    }

    #[test]
    fn collects_unsupported_and_missing_plural_category_diagnostics() {
        let source =
            "bad-zero =\n    { $count ->\n        [few] slikti\n       *[other] labi\n    }\n";
        let diagnostics = collect_document_diagnostics(
            source,
            "lv",
            EffectiveDiagnosticConfig {
                error_on_unsupported_plural_categories: true,
                warn_on_missing_plural_categories: true,
                ..EffectiveDiagnosticConfig::default()
            },
        );

        assert_eq!(diagnostics.len(), 3);
        let unsupported = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.message == "`few` is not a supported plural category for `lv`"
            })
            .unwrap();
        assert_eq!(unsupported.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(unsupported.range.start, Position::new(2, 9));
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.message
                    == "Numeric selector for `lv` is missing category `zero`")
                .count(),
            1
        );
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.message
            == "Numeric selector for `lv` is missing category `one`"));
    }

    #[test]
    fn parse_error_diagnostics_are_reported_without_optional_settings() {
        let source = "welcome-title = Welcome\n\ng@Rb@ge = broken\n";
        let diagnostics =
            collect_document_diagnostics(source, "en", EffectiveDiagnosticConfig::default());

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "Fluent syntax error: Expected a token starting with \"=\""
        );
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostics[0].range.start, Position::new(2, 1));
        assert_eq!(diagnostics[0].range.end, Position::new(2, 2));
    }

    #[test]
    fn numeric_literal_selectors_still_require_plural_categories() {
        let source = "numeric-rollout =\n    { $count ->\n        [0] nav\n        [1] viens\n       *[other] daudz\n    }\n";
        let diagnostics = collect_document_diagnostics(
            source,
            "en",
            EffectiveDiagnosticConfig {
                warn_on_missing_plural_categories: true,
                ..EffectiveDiagnosticConfig::default()
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "Numeric selector for `en` is missing category `one`"
        );
    }

    #[test]
    fn style_mismatch_diagnostics_report_local_numeric_selector_occurrences() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n       *[fallback] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";
        let diagnostics = collect_document_diagnostics(
            source,
            "en",
            EffectiveDiagnosticConfig {
                warn_on_selector_style_mismatch: true,
                preferred_selector_style: Some(SelectorStyle::Prefix),
                ..EffectiveDiagnosticConfig::default()
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "Selector style is `whole`, but workspace prefers `prefix`"
        );
        assert_eq!(diagnostics[0].range.start, Position::new(4, 28));
    }

    #[test]
    fn style_mismatch_diagnostics_report_recognized_numeric_styles() {
        let source = "whole-coins = { $coins ->\n    [one] You have { $coins } coin.\n   *[other] You have { $coins } coins.\n}\n";
        let diagnostics = collect_document_diagnostics(
            source,
            "en",
            EffectiveDiagnosticConfig {
                warn_on_selector_style_mismatch: true,
                preferred_selector_style: Some(SelectorStyle::Prefix),
                ..EffectiveDiagnosticConfig::default()
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "Selector style is `whole`, but workspace prefers `prefix`"
        );
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn numeric_selectors_reject_arbitrary_identifier_keys() {
        let source = "bad-key =\n    { $count ->\n        [admins] nope\n        [one] ok\n       *[other] ok\n    }\n";
        let diagnostics = collect_document_diagnostics(
            source,
            "en",
            EffectiveDiagnosticConfig {
                error_on_unsupported_plural_categories: true,
                ..EffectiveDiagnosticConfig::default()
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories"
        );
        assert_eq!(diagnostics[0].range.start, Position::new(2, 9));
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn selectors_with_only_other_and_custom_keys_are_not_numeric() {
        let source =
            "bad-key =\n    { $count ->\n        [admins] nope\n       *[other] ok\n    }\n";
        let diagnostics = collect_document_diagnostics(
            source,
            "en",
            EffectiveDiagnosticConfig {
                error_on_unsupported_plural_categories: true,
                warn_on_missing_plural_categories: true,
                ..EffectiveDiagnosticConfig::default()
            },
        );

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn plural_categories_come_from_unicode_data() {
        assert_eq!(plural_categories("en"), vec!["one", "other"]);
        assert_eq!(plural_categories("lv"), vec!["zero", "one", "other"]);
        assert_eq!(plural_categories("uk"), vec!["one", "few", "many", "other"]);
        assert_eq!(
            plural_categories("ar"),
            vec!["zero", "one", "two", "few", "many", "other"]
        );
    }

    #[test]
    fn generate_selector_target_prefers_variable_under_cursor_and_uses_pattern_context() {
        let source = "coins-line = Tienes { $coins } monedas.\n";
        let path = Path::new("locales/es/app.ftl");

        let from_key = find_generate_selector_target(source, path, Position::new(0, 2)).unwrap();
        assert!(from_key.selected_variable.is_none());
        assert_eq!(from_key.variables.len(), 1);
        assert_eq!(from_key.variables[0].name, "$coins");

        let from_variable =
            find_generate_selector_target(source, path, Position::new(0, 23)).unwrap();
        assert_eq!(
            from_variable
                .selected_variable
                .as_ref()
                .map(|variable| variable.name.as_str()),
            Some("$coins")
        );
    }

    #[test]
    fn generate_selector_target_uses_nested_container_pattern_for_selected_variable() {
        let source = "nested-coins =\n    { $gender ->\n        [female] Ella tiene { $coins } monedas.\n       *[other] Elle tiene { $coins } monedas.\n    }\n";
        let path = Path::new("locales/es/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(2, 34)).unwrap();

        assert_eq!(
            target
                .selected_variable
                .as_ref()
                .map(|variable| variable.name.as_str()),
            Some("$coins")
        );
        let selected = target.selected_variable.as_ref().unwrap();
        assert_eq!(target.pattern_span, selected.container_span);
        assert_eq!(
            source.get(target.pattern_span.clone()).unwrap(),
            "Ella tiene { $coins } monedas."
        );
    }

    #[test]
    fn does_not_offer_generation_for_ambiguous_multi_variable_message_key() {
        let source = "range-summary = Entre { $min } y { $max } elementos.\n";
        let path = Path::new("locales/es/app.ftl");

        assert!(find_generate_selector_target(source, path, Position::new(0, 2)).is_none());
        assert!(find_generate_selector_target(source, path, Position::new(0, 26)).is_some());
    }

    #[test]
    fn does_not_offer_generation_for_nested_root_key_without_local_anchor() {
        let source = "nested-coins =\n    { $gender ->\n        [female] Ella tiene { $coins } monedas.\n       *[other] Elle tiene { $coins } monedas.\n    }\n";
        let path = Path::new("locales/es/app.ftl");

        assert!(find_generate_selector_target(source, path, Position::new(0, 2)).is_none());
    }

    #[test]
    fn generates_prefix_number_selector_snippet_from_message_context() {
        let source = "coins-line = Tienes { $coins } monedas.\n";
        let path = Path::new("locales/es/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(0, 2)).unwrap();

        assert_eq!(
            generate_number_selector_edit(source, &target, "es", SelectorStyle::Prefix, true)
                .unwrap(),
            "Tienes { \\$${1:coins} } { \\$${1:coins} ->\n    [one] monedas.\n    *[other] monedas.\n}"
        );
    }

    #[test]
    fn generates_whole_number_selector_edit_from_selected_variable() {
        let source = "coins-line = Tienes { $coins } monedas.\n";
        let path = Path::new("locales/es/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(0, 23)).unwrap();
        let generated =
            generate_number_selector_edit(source, &target, "es", SelectorStyle::Whole, false)
                .unwrap();

        assert_eq!(
            generated,
            "{ $coins ->\n    [one] Tienes { $coins } monedas.\n    *[other] Tienes { $coins } monedas.\n}"
        );
        assert_generated_pattern_parses(&generated);
    }

    #[test]
    fn generates_latinian_zero_one_other_selector_categories() {
        let source = "coins-line = Tev ir { $coins } monetas.\n";
        let path = Path::new("locales/lv/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(0, 2)).unwrap();
        let generated =
            generate_number_selector_edit(source, &target, "lv", SelectorStyle::Prefix, false)
                .unwrap();

        assert_eq!(
            generated,
            "Tev ir { $coins } { $coins ->\n    [zero] monetas.\n    [one] monetas.\n    *[other] monetas.\n}"
        );
        assert_generated_pattern_parses(&generated);
    }

    #[test]
    fn generates_whole_selector_snippet_without_existing_variable() {
        let source = "plain-count = Monedas disponibles.\n";
        let path = Path::new("locales/es/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(0, 3)).unwrap();

        assert!(target.variables.is_empty());
        assert_eq!(
            generate_number_selector_edit(source, &target, "es", SelectorStyle::Whole, true)
                .unwrap(),
            "{ \\$${1:count} ->\n    [one] Monedas disponibles.\n    *[other] Monedas disponibles.\n}"
        );
    }

    #[test]
    fn nested_function_argument_variable_uses_enclosing_placeable_as_anchor() {
        let source = "formatted-download = Descarga { NUMBER($downloads) } archivos.\n";
        let path = Path::new("locales/es/app.ftl");

        let from_key = find_generate_selector_target(source, path, Position::new(0, 3)).unwrap();
        assert_eq!(from_key.variables.len(), 1);
        assert_eq!(from_key.variables[0].name, "$downloads");
        assert_eq!(
            available_selector_styles(&from_key),
            vec![
                SelectorStyle::Prefix,
                SelectorStyle::Whole,
                SelectorStyle::Suffix
            ]
        );
        assert_eq!(
            generate_number_selector_edit(source, &from_key, "es", SelectorStyle::Prefix, true)
                .unwrap(),
            "Descarga { NUMBER(\\$${1:downloads}) } { \\$${1:downloads} ->\n    [one] archivos.\n    *[other] archivos.\n}"
        );

        let from_variable =
            find_generate_selector_target(source, path, Position::new(0, 39)).unwrap();
        assert_eq!(
            from_variable
                .selected_variable
                .as_ref()
                .map(|variable| variable.name.as_str()),
            Some("$downloads")
        );
        assert_eq!(
            generate_number_selector_edit(
                source,
                &from_variable,
                "es",
                SelectorStyle::Prefix,
                false,
            )
            .unwrap(),
            "Descarga { NUMBER($downloads) } { $downloads ->\n    [one] archivos.\n    *[other] archivos.\n}"
        );
    }

    #[test]
    fn function_call_under_cursor_selects_on_function_expression_itself() {
        let source = "formatted-download = Descarga { NUMBER($downloads) } archivos.\n";
        let path = Path::new("locales/es/app.ftl");
        let function_column = source.find("NUMBER(").unwrap() as u32;
        let target =
            find_generate_selector_target(source, path, Position::new(0, function_column)).unwrap();

        assert_eq!(
            target
                .selected_function
                .as_ref()
                .map(|function| function.selector_text.as_str()),
            Some("NUMBER($downloads)")
        );
        assert_eq!(
            generate_number_selector_edit(source, &target, "es", SelectorStyle::Prefix, false)
                .unwrap(),
            "Descarga { NUMBER($downloads) } { NUMBER($downloads) ->\n    [one] archivos.\n    *[other] archivos.\n}"
        );
    }

    #[test]
    fn nested_function_calls_still_find_inner_variable_anchor() {
        let source = "deep-download = Descarga { WRAP(NUMBER($downloads)) } archivos.\n";
        let path = Path::new("locales/es/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(0, 44)).unwrap();

        assert_eq!(
            target
                .selected_variable
                .as_ref()
                .map(|variable| variable.name.as_str()),
            Some("$downloads")
        );
        assert_eq!(
            generate_number_selector_edit(source, &target, "es", SelectorStyle::Prefix, false)
                .unwrap(),
            "Descarga { WRAP(NUMBER($downloads)) } { $downloads ->\n    [one] archivos.\n    *[other] archivos.\n}"
        );
    }

    #[test]
    fn keeps_punctuation_attached_in_generated_variant_bodies() {
        let source = "coins-period = Tienes { $coins }.\n";
        let path = Path::new("locales/es/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(0, 3)).unwrap();
        let generated =
            generate_number_selector_edit(source, &target, "es", SelectorStyle::Prefix, false)
                .unwrap();

        assert_eq!(
            generated,
            "Tienes { $coins } { $coins ->\n    [one].\n    *[other].\n}"
        );
        assert_generated_pattern_parses(&generated);
    }

    #[test]
    fn rewrites_whole_selector_to_prefix_and_suffix() {
        let source = "whole-coins = { $coins ->\n    [one] Tienes { $coins } moneda.\n   *[other] Tienes { $coins } monedas.\n}\n";
        let path = Path::new("locales/es/app.ftl");
        let (pattern_span, actions) =
            find_selector_rewrite_target(source, path, Position::new(0, 3)).unwrap();

        assert_eq!(
            source.get(pattern_span).unwrap(),
            "{ $coins ->\n    [one] Tienes { $coins } moneda.\n   *[other] Tienes { $coins } monedas.\n}"
        );
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Prefix);
        assert_eq!(
            actions[0].replacement,
            "Tienes { $coins } { $coins ->\n    [one] moneda.\n    *[other] monedas.\n}"
        );
        assert_eq!(actions[1].kind, SelectorRewriteKind::Suffix);
        assert_eq!(
            actions[1].replacement,
            "Tienes { $coins ->\n    [one] { $coins } moneda.\n    *[other] { $coins } monedas.\n}"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
        assert_generated_pattern_parses(&actions[1].replacement);
    }

    #[test]
    fn rewrites_prefix_selector_to_whole() {
        let source = "prefix-coins = Tienes { $coins } { $coins ->\n    [one] moneda.\n   *[other] monedas.\n}\n";
        let path = Path::new("locales/es/app.ftl");
        let (_, actions) = find_selector_rewrite_target(source, path, Position::new(0, 3)).unwrap();

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Whole);
        assert_eq!(
            actions[0].replacement,
            "{ $coins ->\n    [one] Tienes { $coins } moneda.\n    *[other] Tienes { $coins } monedas.\n}"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
    }

    #[test]
    fn rewrites_suffix_selector_to_whole_and_prefix() {
        let source = "suffix-coins = Tienes { $coins ->\n    [one] { $coins } moneda.\n   *[other] { $coins } monedas.\n}\n";
        let path = Path::new("locales/es/app.ftl");
        let (_, actions) =
            find_selector_rewrite_target(source, path, Position::new(1, 10)).unwrap();

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Whole);
        assert_eq!(
            actions[0].replacement,
            "{ $coins ->\n    [one] Tienes { $coins } moneda.\n    *[other] Tienes { $coins } monedas.\n}"
        );
        assert_eq!(actions[1].kind, SelectorRewriteKind::Prefix);
        assert_eq!(
            actions[1].replacement,
            "Tienes { $coins } { $coins ->\n    [one] moneda.\n    *[other] monedas.\n}"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
        assert_generated_pattern_parses(&actions[1].replacement);
    }

    #[test]
    fn bare_suffix_like_selector_only_offers_prefix_rewrite() {
        let source = "bare-suffix-coins = { $coins ->\n    [one] { $coins } moneda.\n   *[other] { $coins } monedas.\n}\n";
        let path = Path::new("locales/es/app.ftl");
        let (_, actions) =
            find_selector_rewrite_target(source, path, Position::new(1, 10)).unwrap();

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Prefix);
        assert_eq!(
            actions[0].replacement,
            "{ $coins } { $coins ->\n    [one] moneda.\n    *[other] monedas.\n}"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
    }

    #[test]
    fn rewrites_nested_whole_selector_inside_variant() {
        let source = "nested-whole-coins =\n    { $gender ->\n        [female] { $coins ->\n            [one] Ella tiene { $coins } moneda.\n           *[other] Ella tiene { $coins } monedas.\n        }\n       *[other] Elle tiene { $coins } monedas.\n    }\n";
        let path = Path::new("locales/es/app.ftl");
        let (_, actions) =
            find_selector_rewrite_target(source, path, Position::new(3, 24)).unwrap();

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Prefix);
        assert_eq!(
            actions[0].replacement,
            "Ella tiene { $coins } { $coins ->\n            [one] moneda.\n            *[other] monedas.\n        }"
        );
        assert_eq!(actions[1].kind, SelectorRewriteKind::Suffix);
        assert_eq!(
            actions[1].replacement,
            "Ella tiene { $coins ->\n            [one] { $coins } moneda.\n            *[other] { $coins } monedas.\n        }"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
        assert_generated_pattern_parses(&actions[1].replacement);
    }

    #[test]
    fn rewrites_whole_selector_inside_attribute_value() {
        let source = "commented-download =\n    .tooltip = { $files ->\n        [one] Descarga { $files } archivo.\n       *[other] Descarga { $files } archivos.\n    }\n";
        let path = Path::new("locales/es/app.ftl");
        let (_, actions) = find_selector_rewrite_target(source, path, Position::new(1, 6)).unwrap();

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Prefix);
        assert_eq!(
            actions[0].replacement,
            "Descarga { $files } { $files ->\n        [one] archivo.\n        *[other] archivos.\n    }"
        );
        assert_eq!(actions[1].kind, SelectorRewriteKind::Suffix);
        assert_eq!(
            actions[1].replacement,
            "Descarga { $files ->\n        [one] { $files } archivo.\n        *[other] { $files } archivos.\n    }"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
        assert_generated_pattern_parses(&actions[1].replacement);
    }

    #[test]
    fn attribute_rewrite_target_excludes_comments_and_attribute_key() {
        let source = "# Comentario para verificar que la reescritura no toque el comentario\ncommented-download =\n    .tooltip = { $files ->\n        [one] Descarga { $files } archivo.\n       *[other] Descarga { $files } archivos.\n    }\n";
        let path = Path::new("locales/es/app.ftl");
        let (pattern_span, _) =
            find_selector_rewrite_target(source, path, Position::new(2, 6)).unwrap();

        assert_eq!(
            source.get(pattern_span).unwrap(),
            "{ $files ->\n        [one] Descarga { $files } archivo.\n       *[other] Descarga { $files } archivos.\n    }"
        );
    }

    #[test]
    fn rewrites_selected_count_selector_with_trailing_suffix_text() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";
        let path = Path::new("locales/en/app.ftl");
        let (_, actions) =
            find_selector_rewrite_target(source, path, Position::new(5, 31)).unwrap();

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Whole);
        assert_eq!(
            actions[0].replacement,
            "{ $count ->\n    [one] Copy the download link for { $gender ->\n            [female] her\n            [male] his\n           *[other] their\n        } account on { $count } device now.\n    *[other] Copy the download link for { $gender ->\n            [female] her\n            [male] his\n           *[other] their\n        } account on { $count } devices now.\n}"
        );
        assert_eq!(actions[1].kind, SelectorRewriteKind::Suffix);
        assert_eq!(
            actions[1].replacement,
            "Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count ->\n    [one] { $count } device now.\n    *[other] { $count } devices now.\n}"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
        assert_generated_pattern_parses(&actions[1].replacement);
    }

    #[test]
    fn whole_rewrite_after_assignment_keeps_space_after_equals() {
        let source = "zero-rollout =\n    Zero summary: { $count ->\n        [zero] no packages ready.\n        [one] one package ready.\n       *[other] { $count } packages ready.\n    }\n";
        let path = Path::new("locales/en/app.ftl");
        let (pattern_span, actions) =
            find_selector_rewrite_target(source, path, Position::new(1, 20)).unwrap();

        let whole = actions
            .into_iter()
            .find(|action| action.kind == SelectorRewriteKind::Whole)
            .unwrap();
        let replacement = pad_assignment_replacement(source, &pattern_span, whole.replacement);
        let mut updated = source.to_string();
        updated.replace_range(pattern_span, &replacement);

        assert!(updated.starts_with("zero-rollout = { $count ->\n"));
        assert_generated_pattern_parses(&replacement);
        assert_fluent_source_parses(&updated);
    }

    #[test]
    fn rewrites_selected_gender_selector_to_whole_with_nested_count_selector_preserved() {
        let source = "install-hint =\n    Copy the download link for { $gender ->\n        [female] her\n        [male] his\n       *[other] their\n    } account on { $count } { $count ->\n        [one] device\n       *[other] devices\n    } now.\n";
        let path = Path::new("locales/en/app.ftl");
        let (_, actions) =
            find_selector_rewrite_target(source, path, Position::new(1, 35)).unwrap();

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, SelectorRewriteKind::Whole);
        assert_eq!(
            actions[0].replacement,
            "{ $gender ->\n    [female] Copy the download link for her account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n    [male] Copy the download link for his account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n    *[other] Copy the download link for their account on { $count } { $count ->\n            [one] device\n           *[other] devices\n        } now.\n}"
        );
        assert_generated_pattern_parses(&actions[0].replacement);
    }

    #[test]
    fn preserves_nested_structure_when_generating_from_variable_inside_variant() {
        let source = "nested-coins =\n    { $gender ->\n        [female] Ella tiene { $coins } monedas.\n       *[other] Elle tiene { $coins } monedas.\n    }\n";
        let path = Path::new("locales/es/app.ftl");
        let target = find_generate_selector_target(source, path, Position::new(2, 34)).unwrap();
        let generated =
            generate_number_selector_edit(source, &target, "es", SelectorStyle::Prefix, false)
                .unwrap();

        assert_eq!(
            generated,
            "Ella tiene { $coins } { $coins ->\n            [one] monedas.\n            *[other] monedas.\n        }"
        );
        assert_generated_pattern_parses(&generated);
    }

    #[test]
    fn generated_multiline_attribute_pattern_parses() {
        let generated =
            "Instala { $files } { $files ->\n    [one] ahora.\n    *[other] ahora mismo.\n}";
        assert_generated_pattern_parses(generated);
    }

    fn assert_generated_pattern_parses(generated: &str) {
        let wrapped = if generated.contains('\n') {
            format!("probe =\n    {}\n", generated.replace('\n', "\n    "))
        } else {
            format!("probe = {generated}\n")
        };
        let parsed = parser::parse(wrapped.as_str()).unwrap_or_else(|error| {
            panic!("generated pattern should parse:\n{wrapped}\nerror: {error:?}")
        });
        assert!(find_fluent_pattern(&parsed, "probe").is_some());
    }
}
