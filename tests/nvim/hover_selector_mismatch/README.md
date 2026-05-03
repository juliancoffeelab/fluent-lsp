# hover_selector_mismatch

## Feature

Translation hover with different selector sets between source and local text

## Exercise

Requests hover inside the translated `mismatch-rollout` selector branch and again inside its shared `$count` branch. It asserts that hover matches selector variables by name, keeps the source-only `$platform` selector on the source side with its default `*` branch, ignores the local-only `$gender` selector on the source side, and still carries the shared `$count` selection through both previews.

## Assumptions

Neovim exposes raw hover payloads through synchronous LSP requests, including Markdown code fences.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
