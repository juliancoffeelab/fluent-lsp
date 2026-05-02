# codelens_selector_combinations

## Feature

CodeLens-triggered full selector combination document

## Exercise

Enables Neovim CodeLens, runs the translated `install-hint` and `download-action.tooltip` selector lenses to verify full local-language expansion output, then runs the origin-language `install-hint` lens and asserts that the temp Markdown document omits duplicated source-language sections.

## Assumptions

Neovim supports standard LSP `window/showDocument` and focuses the opened temp document in the current window.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
