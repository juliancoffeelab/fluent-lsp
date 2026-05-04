# diagnostics_lsp_copy_markers

Warning diagnostics for copied-source markers

This scenario covers marker diagnostics on a translation file that contains one top-level `# [LSP-COPY]` block and one attribute-level marker inside an existing message.

How the scenario is exercised:

- opens `workspace/locales/es/app.ftl`
- starts `fluent-lsp`
- sends a standard `textDocument/didSave` notification for the current buffer
- waits for two warning diagnostics that point at the marker comment lines
- removes both marker lines from the current buffer
- sends another `textDocument/didSave`
- waits for diagnostics to clear

Assumptions:

- diagnostics are refreshed on save through standard LSP `publishDiagnostics`
- the client can observe buffer-local diagnostics without custom editor integration

Source under test:

- `tests/nvim/diagnostics_lsp_copy_markers/workspace/locales/en/app.ftl`
- `tests/nvim/diagnostics_lsp_copy_markers/workspace/locales/es/app.ftl`
- `tests/nvim/diagnostics_lsp_copy_markers/workspace/fluent-lsp.toml`
