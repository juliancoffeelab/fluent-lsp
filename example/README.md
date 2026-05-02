# Example Workspace

This directory is a repo-local workspace for trying `fluent-lsp` by opening the repository root as your editor workspace.

The top-level [fluent-lsp.toml](/home/codex/workspace/fluent-lsp/fluent-lsp.toml) points at this `example/` tree, so you can open files under `example/locales/` directly and exercise the server without setting up a separate project.

## Files To Try

- `example/locales/es/app.ftl`
  - Go to definition on `welcome-title`, `-brand-name`, or `.label`
  - Hover `welcome-title` for source-first and local-last comments only
  - Check inlay hints on `welcome-title`, `install-hint`, and `download-action.tooltip` for source previews
  - Run the CodeLens on `install-hint` to open the full selector combinations document in Spanish
  - Run the CodeLens on `download-action.tooltip` to verify attribute-level selector combinations
- `example/locales/en/app.ftl`
  - Run references from `welcome-title` or `.label`
  - Hover origin-language entries directly for comments-only output
  - This is the denser top-level file: terms, message values, multiple attributes, and both message-level and attribute-level selectors
- `example/locales/fr/app.ftl`
  - Extra translation target for references and locale comparisons
- `example/locales/uk/app.ftl`
  - Extra translation target for references and locale comparisons
- `example/locales/es/dialogs/menu.ftl`
  - Go to definition on `menu-save.label` to verify locale-tree resolution for nested attributes
- `example/locales/en/dialogs/menu.ftl`
  - Hover `menu-save.label` to verify `###`, `##`, and `#` comment preservation on a nested attribute
  - This is the file with top-level comment structure
- `example/locales/fr/dialogs/menu.ftl`
  - Extra nested translation target
- `example/locales/uk/dialogs/menu.ftl`
  - Extra nested translation target

## Assumptions

- Your editor opens this repository root as the workspace root.
- The LSP client reads `fluent-lsp.toml` from the repo root.
- The client supports standard LSP hover, definition, references, inlay hints, and CodeLens.
- This example workspace is intentionally small, but it keeps multiple translation targets where they are useful for trying references and cross-locale behavior.
