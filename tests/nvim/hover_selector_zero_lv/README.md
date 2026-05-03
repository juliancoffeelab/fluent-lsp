# hover_selector_zero_lv

## Feature

Translation hover for a Latvian `zero` plural-category selector

## Exercise

Opens the Latvian translation file and requests hover inside the `[zero]` and `[one]` branches of `zero-rollout`. It asserts that both source and current previews preserve the shared selector variable, and that the rendered selector assignment uses the plural-category names `zero` and `one` rather than exact numeric keys.

## Assumptions

Neovim exposes raw hover payloads through synchronous LSP requests, including Markdown code fences.

## Source Under Test

`workspace/locales/lv/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
