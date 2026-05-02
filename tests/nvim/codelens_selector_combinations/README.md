# codelens_selector_combinations

## Feature

CodeLens-triggered full selector combination display

## Exercise

Enables Neovim CodeLens, waits for the local lens on install-hint, runs it, and asserts that the resulting showMessage contains the full selector expansion set.

## Assumptions

Neovim CodeLens is enabled for the buffer and window/showMessage can be intercepted in the smoke script.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
