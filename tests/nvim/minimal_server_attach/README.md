# minimal_server_attach

## Feature

Initial Neovim-visible server attachment and capability exposure

## Exercise

Opens the local scenario workspace, starts fluent-lsp, waits for attach, and asserts that the key server capabilities are present in Neovim.

## Assumptions

Neovim headless LSP attach works and the scenario-local workspace config is valid.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
