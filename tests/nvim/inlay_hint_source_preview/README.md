# inlay_hint_source_preview

## Feature

Inlay hint source previews

## Exercise

Enables Neovim inlay hints for the translated app file, asserts that Neovim materializes the rendered hint set for plain values, attributes, selector messages, and selector attributes, and then cross-checks the raw `textDocument/inlayHint` payload plus a boundary-scoped range request.

## Assumptions

Neovim exposes rendered inlay hints through `vim.lsp.inlay_hint.get()` and raw payloads through synchronous LSP requests.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
