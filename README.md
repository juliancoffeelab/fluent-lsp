# fluent-lsp

Minimal Fluent-focused language server.

Current behavior:

- Reads `fluent-lsp.toml` or `.fluent-lsp.toml` from the workspace root.
- Tracks localized `.ftl` files through templated `file_masks` that include `{lang}` and `{filepath}`, so nested locale trees resolve back to the matching origin-language file.
- Resolves `textDocument/definition` by extracting the message, term, or attribute under the cursor from a translated Fluent file and jumping to the matching origin-language `.ftl` file.
- Resolves `textDocument/hover` as comments-only output: origin comments first, then local translation comments after `---` when both exist.
- Publishes `textDocument/inlayHint` source previews for matched Fluent messages, terms, and attributes, anchored at the local value start.
- Resolves selector-bearing inlay hints through the Fluent default branch for each selector and labels the chosen branches with `*`.
- Publishes standard-LSP CodeLens entries for selector-bearing Fluent values, and uses `workspace/executeCommand` plus `window/showDocument` to open a temp Markdown document with the full combination list in the current document language.
- Resolves `textDocument/references` from an origin-language `.ftl` file into matching entries across translated Fluent files with the same relative locale-tree path.

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

Try it locally from this repo:

- Open the repository root as your editor workspace.
- The top-level [fluent-lsp.toml](/home/codex/workspace/fluent-lsp/fluent-lsp.toml) points at the repo-local [example/](/home/codex/workspace/fluent-lsp/example/README.md) tree.
- Use the files under `example/locales/` to try definition, comments-only hover, source-preview inlay hints, locale-tree resolution, and CodeLens-driven selector expansion.

Local fork:

- `third_party/fluent-syntax/` is a path-patched local fork of `fluent-syntax 0.12.0`.
- The intent is to have a clean place to add AST/source span support for editor features without waiting on upstream merges.

The test suite includes:

- direct LSP JSON-RPC integration coverage
- dedicated headless Neovim smoke scenarios under `tests/nvim/`
