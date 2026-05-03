# diagnostics_parse_errors

## Feature

Parse-error diagnostics published on save in Neovim

## Exercise

Opens a scenario-local invalid Fluent file, starts `fluent-lsp`, confirms the buffer stays free of diagnostics before any save, writes the file to trigger a parse error diagnostic, then fixes the invalid entry and writes again to confirm the diagnostic clears.

## Assumptions

Neovim builtin diagnostics are enabled, Neovim headless `:write` sends standard LSP save notifications for attached clients, and the workspace-local `fluent-lsp.toml` is valid.

## Source Under Test

`workspace/locales/en/broken.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
