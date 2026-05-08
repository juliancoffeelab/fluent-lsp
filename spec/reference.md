# fluent-lsp Reference

This document is descriptive, not aspirational.
It summarizes the current behavior implemented in `src/lib.rs` and exercised by:

- `tests/integration_lsp.rs`
- `tests/nvim_smoke.rs`
- `tests/nvim/*/README.md`

When this document and the code disagree, the code and tests win.

## Scope

`fluent-lsp` is currently a Fluent-focused LSP server for locale-tree workspaces.
The common tested shape is:

```toml
origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
```

It recognizes Fluent files through `file_masks`, treats `origin_language` as the source of truth, and matches translation files by `{lang}` plus `{filepath}`.

The workspace config also supports these current options:

```toml
selector_style = "prefix" # or "suffix" or "whole"
error_on_unsupported_plural_categories = true
warn_on_missing_plural_categories = true
warn_on_selector_style_mismatch = true
```

Current precedence:

- file config overrides client `workspace/didChangeConfiguration` for the same setting
- client settings still matter when the file config does not set an override

## Advertised LSP Capabilities

On `initialize`, the server currently advertises:

- `textDocument/definition`
- `textDocument/references`
- `textDocument/hover`
- `textDocument/completion`
  - trigger characters: `.`
- `textDocument/codeAction`
  - kinds: `quickfix`, `refactor.rewrite`
- `textDocument/codeLens`
  - `resolveProvider = false`
- `workspace/executeCommand`
  - command: `fluent-lsp.showSelectorCombinations`
- full-document sync
  - `didOpen`
  - `didChange` with `TextDocumentSyncKind::FULL`
  - `didSave`
  - `didClose`

Not currently advertised:

- rename
- document symbols
- semantic tokens
- inlay hints

## Workspace and File Matching

The tested workspace layout is a locale tree:

```text
locales/
  en/app.ftl
  es/app.ftl
  en/dialogs/menu.ftl
  es/dialogs/menu.ftl
```

With `file_masks = ["locales/{lang}/{filepath}.ftl"]`:

- `locales/es/app.ftl` maps to origin counterpart `locales/en/app.ftl`
- `locales/es/dialogs/menu.ftl` maps to origin counterpart `locales/en/dialogs/menu.ftl`

Only files that match a configured Fluent mask participate in navigation, completion, hover pairing, code actions, code lens, and indexed counterpart diagnostics.

## Definition

Current tested behavior is translation-to-origin definition.

Examples:

- `locales/es/app.ftl`, cursor on `welcome-title`
  - definition -> `locales/en/app.ftl:1:0`
- `locales/es/app.ftl`, cursor on `-brand-name =`
  - definition -> `locales/en/app.ftl:8:1`
- `locales/es/app.ftl`, cursor on `button-copy.label = Lanzar`
  - definition -> `locales/en/app.ftl:13:5`
- `locales/es/dialogs/menu.ftl`, cursor on `menu-save.label = Guardar`
  - definition -> `locales/en/dialogs/menu.ftl:4:5`

Error handling currently tested:

- non-file URIs are rejected with `-32602` and message `expected a file URI`

## References

Current tested behavior is origin-to-translation references.

Examples:

- `locales/en/app.ftl`, cursor on `welcome-title`
  - references include:
    - `locales/es/app.ftl:1:0`
    - `locales/fr/app.ftl:0:0`
- `locales/en/app.ftl`, cursor on `-brand-name =`
  - references include:
    - `locales/es/app.ftl:3:1`
    - `locales/fr/app.ftl:2:1`
- `locales/en/dialogs/menu.ftl`, cursor on `menu-save.label = Save`
  - references include:
    - `locales/es/dialogs/menu.ftl:1:5`
    - `locales/fr/dialogs/menu.ftl:1:5`
    - `locales/lv/dialogs/menu.ftl:1:5`

Indexed references also currently track:

- dirty in-memory translation edits
- dirty-close reversion back to disk content
- background discovery of newly added translation files
- background removal of deleted translation files

## Hover

Hover currently has three important modes.

### 1. Key Hover on Commented Entries

When the cursor is on a message key or attribute key and comments exist, hover returns comment blocks, not rendered preview text.

Origin example:

```ftl
### Shared menu copy
## File menu
# Primary action
```

That comes from hovering the key area of:

```ftl
### Shared menu copy
## File menu
# Primary action
menu-save =
    .label = Save
```

Translation example:

~~~md
```ftl
# Comment-only hover coverage
# Keep this translator guidance visible on key hover
```

---

```ftl
# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
```
~~~

Current rule:

- origin file key hover shows only local comments
- translation key hover shows origin comments first, current-language comments second
- uncommented origin-language keys return no hover instead of falling back to body preview

### 2. Body Hover on Message or Attribute Values

When the cursor is inside the value body, hover returns semantic preview text rendered from the Fluent entry.

Translation example:

~~~md
```ftl
Open the latest { -brand-name } build and pick up where you left off.
```

---

```ftl
Abre la build mas reciente de { -brand-name } y sigue donde lo dejaste.
```
~~~

Attribute example:

~~~md
`$gender=*`, `$count=*`

```ftl
Install the recommended build for their account on { $count } devices now.
```

---

`$gender=*`, `$count=*`

```ftl
Instala la build recomendada para la cuenta de elle en { $count } dispositivos ahora.
```
~~~

Empty-local example:

~~~md
```ftl
English empty preview fallback.
```

---

```ftl
<empty>
```
~~~

### 3. Hover Inside Selectors

When the cursor is inside a selector branch, hover prefixes the preview with the resolved selector assignments.

Example from Spanish `install-hint` on the `[female]` branch:

~~~md
`$gender=female`, `$count=*`

```ftl
Copy the download link for her account on { $count } devices now.
```

---

`$gender=female`, `$count=*`

```ftl
Copia el enlace de descarga para la cuenta de ella en { $count } dispositivos ahora.
```
~~~

Current selector behavior from tests:

- origin preview is shown first
- current-language preview is shown second
- shared selector variables are matched by name across origin and current text
- mismatched selector sets are tolerated
- explicit numeric keys are preserved when possible
- plural-category names like `zero` and `one` are preserved for locales that use them

Mismatch example:

~~~md
`$platform=*`, `$count=0`

```ftl
Summary for mobile users with no packages ready.
```

---

`$gender=other`, `$count=0`

```ftl
Resumen para elle misme con ningun paquete listo.
```
~~~

Latvian zero-category example:

~~~md
`$count=zero`

```ftl
Zero summary: no packages ready.
```

---

`$count=zero`

```ftl
Kopsavilkums ar neviena pakotne nav gatava.
```
~~~

## Completion

Completion is currently translation-oriented. It uses the origin counterpart as the source of keys and attributes.

### Top-Level Key Completion

Example source:

```ftl
welcome-title = Bienvenido

down
```

Completion labels:

```text
download-action
download-count
```

### Attribute Completion

Example source:

```ftl
menu-save =
    .l
```

Completion labels:

```text
.label
```

With a bare `.`:

```text
.label
.tooltip
```

### Completion Documentation

Completion docs are Markdown and come from the origin entry, including comments and Fluent source.

Example documentation for `commented-preview` contains:

```text
# Completion doc coverage
# Keep this note in completion hover
commented-preview = Preview text for completion docs.
```

Example documentation for `.tooltip` contains:

```text
# Menu completion documentation
# Keep this entry visible in completion hover
.tooltip = Save this file
```

### Current Limits

Current tested behavior:

- translation files get completions from origin counterparts
- nested translation files use the nested origin counterpart
- origin files do not offer these counterpart completions
- unmatched prefixes return an empty result
- files outside the configured workspace are rejected with an invalid-params error

## Code Actions

There are two broad families of code actions:

- source-sync quick fixes
- selector generation and selector rewrite refactors

### Add Missing Keys and Attributes From Source

Title:

```text
Add missing keys and attributes from source
```

Example before:

```ftl
hello = Hola Mundo
menu-save =
    .label = Guardar
```

Example after:

```ftl
hello = Hola Mundo
menu-save =
    .label = Guardar
    .tooltip = { "" }

sync-status = { "" }
```

Current behavior:

- inserts empty parseable stubs
- preserves already translated content
- produces Fluent that still parses

### Copy Missing Keys and Attributes From Source

Title:

```text
Copy missing keys and attributes from source
```

Example before:

```ftl
hello = Hola Mundo
menu-save =
    .label = Guardar
```

Example after:

```ftl
hello = Hola Mundo
# [LSP-COPY .tooltip]
menu-save =
    .label = Guardar
    .tooltip = Save this file

# [LSP-COPY]
sync-status = Sync ready
```

Current behavior:

- copied whole messages get `# [LSP-COPY]`
- copied missing attributes get a message-level marker like `# [LSP-COPY .tooltip]`
- copied content is kept parseable

### Copy a Single Stub Message From Source

Title:

```text
Copy `hello` from source
```

Example before:

```ftl
hello = { "" }
download-action =
    .label = Descargar

sync-status = { "" }
```

Example after:

```ftl
# [LSP-COPY]
hello = Hello World
download-action =
    .label = Descargar

sync-status = { "" }
```

Current behavior:

- only the selected message is replaced
- unrelated entries are left alone

### Copy Missing Attributes for a Selected Message

Title:

```text
Copy missing attributes for `download-action` from source
```

Example before:

```ftl
hello = { "" }
download-action =
    .label = Descargar

sync-status = { "" }
```

Example after:

```ftl
hello = { "" }
# [LSP-COPY .tooltip]
download-action =
    .label = Descargar
    .tooltip = Download this build

sync-status = { "" }
```

Current behavior:

- only missing attributes for the selected message are inserted
- the hover surface intentionally does not expose the `LSP-COPY` marker as translator comment content

### Generate Number Selector

Generation actions are `refactor.rewrite`.

Current generated titles include:

```text
Generate number selector (prefix)
Generate number selector (whole)
Generate number selector (suffix)
Generate number selector from $coins (prefix)
Generate number selector from NUMBER($downloads) (prefix)
Generate number selector from WRAP(NUMBER($downloads)) (prefix)
```

Default example from:

```ftl
coins-line = Tienes { $coins } monedas.
```

Generated `prefix` text:

```ftl
Tienes { $coins } { $coins ->
    [one] monedas.
    *[other] monedas.
}
```

Whole-form generation when there is no existing variable:

```ftl
plain-count = Monedas disponibles.
```

becomes this snippet body when snippet edits are supported:

```ftl
{ \$${1:count} ->
    [one] Monedas disponibles.
    *[other] Monedas disponibles.
}
```

Function-anchor example:

```ftl
formatted-download = Descarga { NUMBER($downloads) } archivos.
```

can generate:

```ftl
Descarga { NUMBER($downloads) } { $downloads ->
    [one] archivos.
    *[other] archivos.
}
```

or, when the cursor is on the function expression itself:

```ftl
Descarga { NUMBER($downloads) } { NUMBER($downloads) ->
    [one] archivos.
    *[other] archivos.
}
```

Current behavior from tests:

- message-level generation offers a preferred style first
- the preferred style defaults to `prefix`
- client `selector_style` can change the preferred style
- file config `selector_style` overrides the client preference
- attributes also participate
- nested function placeables can be used as anchors
- punctuation stays attached, for example `.` stays `.` instead of becoming ` .`
- ambiguous messages like `range-summary` intentionally hide generation actions

### Selector Rewrite Actions

Current rewrite titles include:

```text
Convert selector to prefix form
Convert selector to suffix form
Convert selector to whole form
```

Whole -> prefix example:

Before:

```ftl
whole-coins = { $coins ->
    [one] Tienes { $coins } moneda.
   *[other] Tienes { $coins } monedas.
}
```

After:

```ftl
whole-coins = Tienes { $coins } { $coins ->
    [one] moneda.
    *[other] monedas.
}
```

Whole -> suffix example:

Before:

```ftl
whole-coins = { $coins ->
    [one] Tienes { $coins } moneda.
   *[other] Tienes { $coins } monedas.
}
```

After:

```ftl
whole-coins = Tienes { $coins ->
    [one] { $coins } moneda.
    *[other] { $coins } monedas.
}
```

Prefix -> whole example:

Before:

```ftl
prefix-coins = Tienes { $coins } { $coins ->
    [one] moneda.
   *[other] monedas.
}
```

After:

```ftl
prefix-coins = { $coins ->
    [one] Tienes { $coins } moneda.
    *[other] Tienes { $coins } monedas.
}
```

Current rewrite behavior from tests:

- whole selectors can rewrite to prefix and suffix
- prefix selectors can rewrite to whole
- suffix selectors can rewrite to whole and prefix
- suffix-like selectors with no distinct outer prefix only advertise the distinct `prefix` collapse
- nested selectors can be rewritten inside variants
- attribute values can be rewritten
- attribute rewrite ranges do not consume surrounding comments or the attribute key
- local rewrites preserve nested selectors when rewriting only one selected selector inside a larger pattern
- rewrites preserve `= {` spacing when replacing a whole message value immediately after `=`

### Snippet Text Edit Behavior

If the client advertises:

```json
{
  "workspace": {
    "workspaceEdit": {
      "documentChanges": true,
      "snippetEditSupport": true
    }
  }
}
```

then selector-generation code actions currently use `WorkspaceEdit.documentChanges` plus `SnippetTextEdit`.

Otherwise they currently use plain `WorkspaceEdit.changes`.

## Code Lens and `fluent-lsp.showSelectorCombinations`

Code lenses are currently offered for entries that have selector expansions worth showing.

Example:

- `install-hint` in Spanish produces a lens titled:

```text
Show all 6 selector combinations
```

Executing the lens calls:

```text
fluent-lsp.showSelectorCombinations
```

With `window/showDocument` support, the command currently opens a temp Markdown file whose name contains:

```text
fluent-lsp-selector-combinations-...-install-hint.md
```

The generated document currently includes sections like:

```text
Current language: `es`
Source language: `en`
Source text:
Current text:
Source language combinations:
Current language combinations:
```

The document includes rendered selector combinations such as:

~~~md
`$gender=other`, `$count=other`
```ftl
Copy the download link for their account on { $count } devices now.
```
~~~

and:

~~~md
`$gender=other`, `$count=other`
```ftl
Copia el enlace de descarga para la cuenta de elle en { $count } dispositivos ahora.
```
~~~

Without `window/showDocument` support, the current implementation falls back to `window/showMessage` and truncates to a smaller selector-combination limit.

## Diagnostics

Diagnostics are currently save-oriented for document syntax and selector checks, plus index-driven for counterpart problems.

### Parse Errors

Current behavior:

- opening an invalid file does not immediately publish parse errors
- saving the file publishes parse diagnostics
- fixing the file and saving again clears them

Example message:

```text
Fluent syntax error: Expected a token starting with "="
```

### `LSP-COPY` Marker Warnings

Current message:

```text
Entry still contains an `# [LSP-COPY]` marker
```

Current placement:

- whole-message marker warns on the marker line itself
- attribute-copy marker warns on the copied attribute key line, not the marker line

### Translation File With No Origin Counterpart

Current message shape:

```text
Translation file has no origin-language counterpart for `only`
```

Current behavior:

- saving the translation file publishes the warning
- creating the origin counterpart on disk clears it during background refresh

### Translation-Only Entry or Attribute

Current message shapes:

```text
Translation entry `extra` has no origin-language counterpart
Translation attribute `menu.tooltip` has no origin-language counterpart
```

Current behavior:

- diagnostics point at the local translation key/attribute
- adding the missing origin entry/attribute clears them on background refresh

### Unsupported or Missing Numeric Selector Categories

These are disabled by default.

When enabled through config or client settings, examples include:

```text
`few` is not a supported plural category for `lv`
Numeric selector for `lv` is missing category `zero`
Numeric selector for `lv` is missing category `one`
```

Current severity from tests:

- unsupported category -> error
- missing category -> warning

Unicode plural-rule data is currently used for locale-specific validation.
For Ukrainian, tests confirm that `few` and `many` are accepted while a missing `one` still warns.

### Invalid Numeric Selector Keys

Current message:

```text
`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories
```

This check is only applied when the server decides the selector is numeric.
A mixed non-numeric selector with only `[admins]` and default `*[other]` is currently left alone.

### Selector Style Mismatch

This is disabled by default.

When enabled, examples include:

```text
Selector style is `whole`, but workspace prefers `prefix`
Selector style is `suffix`, but workspace prefers `prefix`
Selector style is `prefix`, but workspace prefers `whole`
```

Current behavior:

- both top-level and local selector occurrences can warn
- file config can override client style preference and mismatch policy

### Diagnostic Clearing

Current tested clearing behavior:

- fixing parse errors clears diagnostics on save
- removing `LSP-COPY` markers clears diagnostics on save
- `didClose` clears document diagnostics immediately
- background counterpart/index changes clear index-driven warnings

## Indexing, Refresh, and Trace

### Initial Index Build

On `initialized`, if config loads successfully, the server currently:

- logs the loaded origin language and masks
- starts a workspace index build

If the client supports work-done progress, the server currently emits `$/progress` with token:

```text
fluent-lsp-index
```

including:

- `begin`
- one or more `report`
- `end`

### Live Overlay Behavior

Open-document overlays currently affect indexed requests immediately.

Tests cover:

- definition sees dirty origin changes
- hover sees dirty origin changes
- completion sees dirty origin changes
- references see dirty translation changes
- closing a dirty translation reverts indexed behavior to disk content

### Background Disk Refresh

Current implementation refreshes in the background and tests exercise a roughly one-second pickup window for:

- newly added translation files
- deleted translation files
- newly added origin counterparts
- newly added origin entries/attributes

### Trace Logging

If trace is enabled through `initialize.trace = "messages"` or `"verbose"`, or later via `$/setTrace`, the server currently emits `$/logTrace`.

Examples:

```text
operation=workspace/index
operation=textDocument/definition
```

Verbose request example:

```text
hit=true
```

## Notes on Where Tests Are Still Thin

These are the places where the current suite does not push the behavior as hard as it probably should.

- The `window/showMessage` fallback for selector combinations is implemented, but current end-to-end coverage is centered on the `window/showDocument` path.
- `workspace/executeCommand` error paths are light: bad argument shapes, rejected `showDocument`, and unavailable files/workspaces are not exercised deeply in smoke tests.
- Completion is smoke-tested via raw Rust-side LSP requests because Neovim headless completion helpers are flaky here, so actual Neovim completion-menu insertion and acceptance behavior is not covered as hard as the request/response path.
- Selector-generation snippets are well tested at the LSP payload level, but editor UX details like placeholder jump order and snippet-session behavior are not validated by a Neovim smoke.
- Config loading paths are not stressed enough: multiple `file_masks`, alternate config filename `.fluent-lsp.toml`, and multi-workspace-folder initialization need stronger coverage.
- Background index invalidation is covered for a handful of adds/edits/deletes, but not for heavier churn like many simultaneous file changes, rapid create-delete cycles, or large locale sets.
