# hover_translation

## Feature

Hover from translation showing origin entry and comments

## Exercise

Requests hover on a plain translated message, inside a translated selector branch, and again on the closing-line text after that selector, then asserts that the returned hover text shows the plain preview plus both selector-aware contexts.

## Assumptions

Neovim exposes raw hover payloads through synchronous LSP requests, including Markdown code fences.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
