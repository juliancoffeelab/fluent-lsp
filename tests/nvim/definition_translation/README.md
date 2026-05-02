# definition_translation

## Feature

Definition from translation to origin-language entry

## Exercise

Opens the Spanish translation file, triggers definition on welcome-title, and asserts that Neovim jumps into the origin-language file.

## Assumptions

Neovim builtin definition navigation is active for the attached LSP client.

## Source Under Test

`workspace/locales/es/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
