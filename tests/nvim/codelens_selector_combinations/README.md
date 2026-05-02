# codelens_selector_combinations

## Feature

CodeLens-triggered full selector combination document

## Exercise

Enables Neovim CodeLens, waits for the local selector lenses, runs both the `install-hint` message lens and the `download-action.tooltip` attribute lens, and asserts that Neovim opens the temp selector-combinations Markdown documents with the full local-language expansion set.

## Assumptions

Neovim supports standard LSP `window/showDocument` and focuses the opened temp document in the current window.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
