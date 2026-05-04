# completion_origin_keys

## Feature

Completion from the origin-language file for translation keys and attributes

## Exercise

Opens the translation app file, rewrites the buffer in-memory to a partial top-level key (`down`), and requests `textDocument/completion` to verify the origin-language message keys are offered in origin order. Then it opens the nested translation menu file, rewrites the buffer to `menu-save =` with both bare `.` and `.l` attribute prefixes, and verifies that the nested origin counterpart provides the expected attribute labels.

## Assumptions

Neovim exposes raw completion responses through synchronous LSP requests, and unsaved full-buffer edits are visible to the language server through standard did-change notifications.

## Source Under Test

- `workspace/locales/es/app.ftl`
- `workspace/locales/es/dialogs/menu.ftl`
- `workspace/locales/en/app.ftl`
- `workspace/locales/en/dialogs/menu.ftl`

This scenario is self-contained. The source fixtures and workspace config used by the smoke test live under this directory's `workspace/` tree.
