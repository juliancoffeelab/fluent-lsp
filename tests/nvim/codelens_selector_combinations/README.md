# codelens_selector_combinations

## Feature

CodeLens-triggered full selector combination document

## Exercise

Enables Neovim CodeLens, waits for the local lens on `install-hint`, runs it, and asserts that Neovim opens the temp selector-combinations Markdown document with the full expansion set.

## Assumptions

Neovim supports standard LSP `window/showDocument` and focuses the opened temp document in the current window.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
