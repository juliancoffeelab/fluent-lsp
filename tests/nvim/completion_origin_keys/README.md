# completion_origin_keys

## Feature

Completion from the origin-language file for translation keys and attributes

## Exercise

Opens four scenario-local translation fixtures in Neovim:
- `app_download.ftl` for a partial top-level key (`down`)
- `app_commented.ftl` for a comment-bearing key prefix (`commented`)
- `menu_bare_dot.ftl` for a bare attribute prefix (`.`)
- `menu_label_prefix.ftl` for a narrowed attribute prefix (`.l`)

The Rust smoke harness then issues raw `textDocument/completion` requests against those exact files to verify the origin-language message keys are offered in origin order and that completion item documentation carries origin comments and source text for both top-level keys and attributes.

## Assumptions

Neovim reliably opens the exact scenario files and attaches `fluent-lsp` for the workspace. The completion assertions themselves are performed from the Rust smoke harness because Neovim’s headless completion request helper is flaky in this environment even when the server-side completion behavior is correct.

## Source Under Test

- `workspace/locales/es/app.ftl`
- `workspace/locales/es/app_download.ftl`
- `workspace/locales/es/app_commented.ftl`
- `workspace/locales/es/dialogs/menu.ftl`
- `workspace/locales/es/dialogs/menu_bare_dot.ftl`
- `workspace/locales/es/dialogs/menu_label_prefix.ftl`
- `workspace/locales/en/app.ftl`
- `workspace/locales/en/app_download.ftl`
- `workspace/locales/en/app_commented.ftl`
- `workspace/locales/en/dialogs/menu.ftl`
- `workspace/locales/en/dialogs/menu_bare_dot.ftl`
- `workspace/locales/en/dialogs/menu_label_prefix.ftl`

This scenario is self-contained. The source fixtures and workspace config used by the smoke test live under this directory's `workspace/` tree.
