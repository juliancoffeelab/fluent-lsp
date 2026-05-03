# hover_translation

## Feature

Hover from translation showing origin entry and comments

## Exercise

Requests hover on a plain translated message, inside a translated selector branch, on the closing-line text after that selector, and on the concatenated second selector branch (`[one] dispositivo`). It asserts that the returned hover text shows the plain preview plus selector-aware contexts for both selectors.

## Assumptions

Neovim exposes raw hover payloads through synchronous LSP requests, including Markdown code fences.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
