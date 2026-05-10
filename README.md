# fluent-lsp

Minimal Fluent-focused language server.

Current behavior:

- Reads `fluent-lsp.toml` or `.fluent-lsp.toml` from the workspace root.
- Tracks localized `.ftl` files through templated `file_masks` that include `{lang}` and `{filepath}`, so nested locale trees resolve back to the matching origin-language file.
- Builds a global Fluent workspace index after initialization and reports standard LSP work-done progress when the client supports it.
- Emits standard `$/logTrace` timing notifications when tracing is enabled through `initialize.trace` or `$/setTrace`.
- Keeps unsaved editor buffers as index overlays and invalidates one file entry at a time for standard `didOpen`, `didChange`, `didSave`, and `didClose` notifications.
- Resolves `textDocument/definition` by extracting the message, term, or attribute under the cursor from a translated Fluent file and jumping to the matching origin-language `.ftl` file.
- Resolves `textDocument/hover` as a formatted Fluent preview for the hovered message or attribute. Translation value hover renders the source preview first, then `---`, then the current-language preview, while key and origin-language hover stay single-section.
- Publishes standard-LSP CodeLens entries for selector-bearing Fluent values, and uses `workspace/executeCommand` plus `window/showDocument` to open a temp Markdown document with the full combination list in the current document language.
- Resolves `textDocument/references` from an origin-language `.ftl` file into matching entries across translated Fluent files with the same relative locale-tree path.
- Publishes a standard diagnostic when a translation file has no origin-language counterpart.

Config shape:

```toml
origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
```

Selector-combination CodeLens output is always enabled for selector-bearing Fluent values. When the client does not support `window/showDocument`, the fallback `window/showMessage` path truncates the rendered combination list to 10 items.

Run it over stdio:

```bash
cargo run
```

Validation:

```bash
cargo test
```

Benchmarks:

Bench binaries:

```bash
cargo run -p bench --release --bin fluent-lsp-bench-index
cargo run -p bench --release --bin fluent-lsp-bench-definition
cargo run -p bench --release --bin fluent-lsp-bench-references
cargo run -p bench --release --bin fluent-lsp-bench-hover
cargo run -p bench --release --bin fluent-lsp-bench-completion
cargo run -p bench --release --bin fluent-lsp-bench-code-actions
cargo run -p bench --release --bin fluent-lsp-bench-code-lens
cargo run -p bench --release --bin fluent-lsp-bench-execute-command
cargo run -p bench --release --bin fluent-lsp-bench-missing-entries
```

Criterion:

```bash
make bench-criterion
```

Older bounded results and profiling notes are recorded in [STAGES.md](/Users/illiadenysenko/Workspace/lab/fluent-lsp/STAGES.md) and under [benchmarks/](/Users/illiadenysenko/Workspace/lab/fluent-lsp/benchmarks).

Try it locally from this repo:

- Open the repository root as your editor workspace.
- The top-level [fluent-lsp.toml](/home/codex/workspace/fluent-lsp/fluent-lsp.toml) points at the repo-local [example/](/home/codex/workspace/fluent-lsp/example/README.md) tree.
- Use the files under `example/locales/` to try definition, formatted hover previews with separator-delimited source/current blocks, locale-tree resolution, and CodeLens-driven selector expansion.

Local fork:

- `fluent-syntax` and the test-only `fluent-bundle` dependency are pinned to `juliancoffeelab/fluent-rs` at the `span-followup` fork revision.
- That fork carries upstream PR `projectfluent/fluent-rs#373` plus a tiny follow-up for editor-facing span regressions and local test/lint quirks.

The test suite includes:

- direct LSP JSON-RPC integration coverage
- dedicated headless Neovim smoke scenarios under `tests/nvim/`
