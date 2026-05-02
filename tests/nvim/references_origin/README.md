# references_origin

## Feature

References from origin-language entry to translations

## Exercise

Opens the origin-language file, requests references for welcome-title, and asserts that Neovim receives the two translated locations.

## Assumptions

Synchronous LSP reference requests are available in headless Neovim.

## Source Under Test

`workspace/locales/en/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
