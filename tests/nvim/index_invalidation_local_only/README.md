# Indexed invalidation and local-only warning

## Feature Covered

This scenario covers the global workspace index as observed from Neovim:

- indexed `textDocument/definition`
- indexed `textDocument/references`
- indexed `textDocument/hover` after live edits
- indexed `textDocument/completion` after live edits
- background disk discovery of newly added and removed translation files
- local-only-file warning diagnostics
- background clearing of the local-only warning when the origin counterpart appears
- dirty-buffer close reversion back to disk-backed index content

## How The Scenario Is Exercised

The smoke script starts `fluent-lsp` through Neovim's standard LSP client, opens the scenario-local Spanish translation, edits the origin and translation buffers through Neovim buffer APIs, and issues standard LSP requests. It verifies that live edits update index-backed definition, references, hover, and completion without restarting the server. It then closes a dirty translation buffer and confirms references revert to the saved disk content.

The scenario also writes and deletes a scenario-local French translation file on disk and waits for the background index worker to surface and then remove the new reference. The local-only warning is exercised by opening and saving `locales/es/local.ftl`, which initially has no `locales/en/local.ftl` counterpart, then writing `locales/en/local.ftl` and waiting for the warning to clear through standard `publishDiagnostics`.

## Assumptions

Neovim headless LSP attaches to the local `fluent-lsp` binary from `FLUENT_LSP_BIN`. The scenario uses only standard LSP requests, diagnostics, and Neovim buffer/edit operations; no client-specific extensions or custom UI plumbing are required.

## Source Fixture

All source-under-test files are local to this scenario:

- `tests/nvim/index_invalidation_local_only/workspace/fluent-lsp.toml`
- `tests/nvim/index_invalidation_local_only/workspace/locales/en/app.ftl`
- `tests/nvim/index_invalidation_local_only/workspace/locales/es/app.ftl`
- `tests/nvim/index_invalidation_local_only/workspace/locales/es/local.ftl`
