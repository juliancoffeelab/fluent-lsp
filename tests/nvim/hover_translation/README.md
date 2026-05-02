# hover_translation

## Feature

Hover from translation showing origin entry and comments

## Exercise

Requests hover on a translated entry and asserts that the returned hover text contains source comments first, local comments last, and no source-body duplication.

## Assumptions

Neovim exposes raw hover payloads through synchronous LSP requests.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
