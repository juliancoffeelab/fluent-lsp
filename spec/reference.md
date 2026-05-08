# fluent-lsp Reference

## Configuration

Examples below assume this workspace shape:

```toml
origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
```

Example file pairs:

```text
locales/en/app.ftl
locales/es/app.ftl
locales/en/dialogs/menu.ftl
locales/es/dialogs/menu.ftl
```

Path mapping examples:

- `locales/es/app.ftl` -> `locales/en/app.ftl`
- `locales/es/dialogs/menu.ftl` -> `locales/en/dialogs/menu.ftl`

### `fluent-lsp.toml`

Required keys:

```toml
origin_language = "en"
file_masks = ["locales/{lang}/{filepath}.ftl"]
```

Supported optional keys:

```toml
selector_style = "prefix" # "prefix" | "suffix" | "whole"
error_on_unsupported_plural_categories = true
warn_on_missing_plural_categories = true
warn_on_selector_style_mismatch = true
```

Current precedence:

- file config overrides client `workspace/didChangeConfiguration`
- client settings apply when file config does not override them

## Advertised LSP Capabilities

`initialize` currently returns these relevant capabilities:

```json
{
  "definitionProvider": true,
  "referencesProvider": true,
  "hoverProvider": true,
  "completionProvider": {
    "triggerCharacters": ["."]
  },
  "codeActionProvider": {
    "codeActionKinds": ["quickfix", "refactor.rewrite"],
    "resolveProvider": false
  },
  "codeLensProvider": {
    "resolveProvider": false
  },
  "executeCommandProvider": {
    "commands": ["fluent-lsp.showSelectorCombinations"]
  },
  "textDocumentSync": {
    "openClose": true,
    "change": 1,
    "save": true
  }
}
```

Not currently advertised:

- rename
- semantic tokens
- inlay hints
- document symbols

## `textDocument/definition`

Behavior:

- translation file -> matching origin entry or attribute
- only file URIs are accepted

Examples:

| From | Cursor | Result |
| --- | --- | --- |
| `locales/es/app.ftl` | `welcome-title` | `locales/en/app.ftl:1:0` |
| `locales/es/app.ftl` | `-brand-name =` | `locales/en/app.ftl:8:1` |
| `locales/es/app.ftl` | `button-copy.label = Lanzar` | `locales/en/app.ftl:13:5` |
| `locales/es/dialogs/menu.ftl` | `menu-save.label = Guardar` | `locales/en/dialogs/menu.ftl:4:5` |

Error example:

```json
{
  "code": -32602,
  "message": "expected a file URI"
}
```

## `textDocument/references`

Behavior:

- origin file -> matching entries or attributes in translations
- results come from the indexed workspace model

Examples:

| From | Cursor | Result locations |
| --- | --- | --- |
| `locales/en/app.ftl` | `welcome-title` | `locales/es/app.ftl:1:0`, `locales/fr/app.ftl:0:0` |
| `locales/en/app.ftl` | `-brand-name =` | `locales/es/app.ftl:3:1`, `locales/fr/app.ftl:2:1` |
| `locales/en/dialogs/menu.ftl` | `menu-save.label = Save` | `locales/es/dialogs/menu.ftl:1:5`, `locales/fr/dialogs/menu.ftl:1:5`, `locales/lv/dialogs/menu.ftl:1:5` |

Current indexed behavior:

- dirty in-memory translation edits update references immediately
- `didClose` drops dirty overlay state and reverts to disk
- background refresh picks up added translation files
- background refresh removes deleted translation files

## `textDocument/hover`

Hover has three current forms.

### Key Hover

Key hover returns comment blocks when comments exist.

Origin example, hover on `menu-save` key area in `locales/en/dialogs/menu.ftl`:

```ftl
### Shared menu copy
## File menu
# Primary action
```

Translation example, hover on `commented-preview` key in `locales/es/app.ftl`:

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

Current rules:

- origin key hover returns local comments only
- translation key hover returns origin comments first, local comments second
- uncommented origin keys return no hover

### Body Hover

Body hover returns rendered preview text.

Translation example, hover in `welcome-body` in `locales/es/app.ftl`:

~~~md
```ftl
Open the latest { -brand-name } build and pick up where you left off.
```

---

```ftl
Abre la build mas reciente de { -brand-name } y sigue donde lo dejaste.
```
~~~

Origin attribute example, hover in `menu-save.tooltip` in `locales/en/dialogs/menu.ftl`:

```ftl
Save changes before closing the window
```

Empty local value example, hover on `empty-preview = { "" }` in `locales/es/app.ftl`:

~~~md
```ftl
English empty preview fallback.
```

---

```ftl
<empty>
```
~~~

### Selector Hover

Selector hover prepends resolved selector values, then shows origin and local rendered preview blocks.

Example, hover on the `[female]` branch of `install-hint` in `locales/es/app.ftl`:

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

Mismatch example, hover in `mismatch-rollout`:

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

Latvian zero-category example, hover on `[zero]` in `locales/lv/app.ftl`:

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

Current rules:

- origin block is first
- local block is second
- shared selector variables are matched by name
- explicit numeric keys such as `0` and `1` are preserved
- plural-category names such as `zero` and `one` are preserved

## `textDocument/completion`

Behavior:

- completion is translation-oriented
- source of truth is the origin counterpart file
- trigger character is `.`

Top-level key example.

Before:

```ftl
welcome-title = Bienvenido

down
```

Completion labels:

```text
download-action
download-count
```

Attribute example.

Before:

```ftl
menu-save =
    .l
```

Completion labels:

```text
.label
```

Bare-dot example.

Before:

```ftl
menu-save =
    .
```

Completion labels:

```text
.label
.tooltip
```

Documentation example for `commented-preview`:

```text
# Completion doc coverage
# Keep this note in completion hover
commented-preview = Preview text for completion docs.
```

Documentation example for `.tooltip`:

```text
# Menu completion documentation
# Keep this entry visible in completion hover
.tooltip = Save this file
```

Current rules:

- translation files receive completion from their origin counterpart
- nested translation files use the nested origin counterpart
- origin files do not offer counterpart completion
- unmatched prefixes return an empty result
- files outside the configured workspace return invalid params

## `textDocument/codeAction`

Two action families exist today:

- missing-entry quick fixes
- selector-generation and selector-rewrite refactors

### Quick Fix: `Add missing keys and attributes from source`

Title:

```text
Add missing keys and attributes from source
```

Before:

```ftl
hello = Hola Mundo
menu-save =
    .label = Guardar
```

After:

```ftl
hello = Hola Mundo
menu-save =
    .label = Guardar
    .tooltip = { "" }

sync-status = { "" }
```

Current behavior:

- inserts parseable empty stubs
- preserves existing translated content
- no `LSP-COPY` marker is added

### Quick Fix: `Copy missing keys and attributes from source`

Title:

```text
Copy missing keys and attributes from source
```

Before:

```ftl
hello = Hola Mundo
menu-save =
    .label = Guardar
```

After:

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

- copied whole messages receive `# [LSP-COPY]`
- copied missing attributes receive a message-level marker such as `# [LSP-COPY .tooltip]`

### Quick Fix: `Copy \`hello\` from source`

Title:

```text
Copy `hello` from source
```

Before:

```ftl
hello = { "" }
download-action =
    .label = Descargar

sync-status = { "" }
```

After:

```ftl
# [LSP-COPY]
hello = Hello World
download-action =
    .label = Descargar

sync-status = { "" }
```

Current behavior:

- replaces only the selected message
- does not touch unrelated incomplete entries

### Quick Fix: `Copy missing attributes for \`download-action\` from source`

Title:

```text
Copy missing attributes for `download-action` from source
```

Before:

```ftl
hello = { "" }
download-action =
    .label = Descargar

sync-status = { "" }
```

After:

```ftl
hello = { "" }
# [LSP-COPY .tooltip]
download-action =
    .label = Descargar
    .tooltip = Download this build

sync-status = { "" }
```

Current behavior:

- inserts only missing attributes for the selected message
- hover on the copied attribute does not surface the `LSP-COPY` marker as comment content

### Refactor: Selector Generation

Representative titles:

```text
Generate number selector (prefix)
Generate number selector (whole)
Generate number selector (suffix)
Generate number selector from $coins (prefix)
Generate number selector from NUMBER($downloads) (prefix)
Generate number selector from WRAP(NUMBER($downloads)) (prefix)
```

Message-level example.

Before:

```ftl
coins-line = Tienes { $coins } monedas.
```

After, `Generate number selector (prefix)`:

```ftl
Tienes { $coins } { $coins ->
    [one] monedas.
    *[other] monedas.
}
```

Whole-only example when no variable exists.

Before:

```ftl
plain-count = Monedas disponibles.
```

Snippet body:

```ftl
{ \$${1:count} ->
    [one] Monedas disponibles.
    *[other] Monedas disponibles.
}
```

Function-anchor example.

Before:

```ftl
formatted-download = Descarga { NUMBER($downloads) } archivos.
```

After, cursor on `$downloads`:

```ftl
Descarga { NUMBER($downloads) } { $downloads ->
    [one] archivos.
    *[other] archivos.
}
```

After, cursor on `NUMBER($downloads)`:

```ftl
Descarga { NUMBER($downloads) } { NUMBER($downloads) ->
    [one] archivos.
    *[other] archivos.
}
```

Punctuation example.

Before:

```ftl
coins-period = Tienes { $coins }.
```

After:

```ftl
Tienes { $coins } { $coins ->
    [one].
    *[other].
}
```

Current rules:

- default preferred style is `prefix`
- client `selector_style` changes preferred style
- file config `selector_style` overrides client preference
- attributes participate
- nested function placeables can be the anchor
- punctuation remains attached
- ambiguous messages such as `range-summary` hide generation actions

### Refactor: Selector Rewrite

Representative titles:

```text
Convert selector to prefix form
Convert selector to suffix form
Convert selector to whole form
```

Whole -> prefix example.

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

Whole -> suffix example.

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

Prefix -> whole example.

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

Attribute rewrite example.

Before:

```ftl
commented-download =
    .tooltip = { $files ->
        [one] Descarga { $files } archivo.
       *[other] Descarga { $files } archivos.
    }
```

After, `Convert selector to prefix form`:

```ftl
commented-download =
    .tooltip = Descarga { $files } { $files ->
        [one] archivo.
        *[other] archivos.
    }
```

Current rules:

- whole selectors can rewrite to prefix and suffix
- prefix selectors can rewrite to whole
- suffix selectors can rewrite to whole and prefix
- suffix-like selectors without distinct outer text only offer the distinct `prefix` collapse
- nested selectors can be rewritten inside variants
- attribute rewrite edits do not consume surrounding comments or the attribute key
- rewrites preserve assignment spacing such as `= {`

### Snippet Edit Form

When the client advertises:

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

selector-generation actions currently use `WorkspaceEdit.documentChanges` with `SnippetTextEdit`.

Without that capability they use plain `WorkspaceEdit.changes`.

## `textDocument/codeLens`

Behavior:

- lenses are returned for entries with selector combinations worth expanding
- `resolveProvider` is `false`

Example lens on `install-hint`:

```json
{
  "title": "Show all 6 selector combinations",
  "command": "fluent-lsp.showSelectorCombinations"
}
```

## `workspace/executeCommand`

Supported command:

```text
fluent-lsp.showSelectorCombinations
```

Arguments:

```json
[
  "file:///.../locales/es/app.ftl",
  "install-hint"
]
```

With `window/showDocument` support, the server opens a temp Markdown document containing sections such as:

```text
Current language: `es`
Source language: `en`
Source text:
Current text:
Source language combinations:
Current language combinations:
```

Combination example from that document:

~~~md
`$gender=other`, `$count=other`
```ftl
Copy the download link for their account on { $count } devices now.
```
~~~

~~~md
`$gender=other`, `$count=other`
```ftl
Copia el enlace de descarga para la cuenta de elle en { $count } dispositivos ahora.
```
~~~

Without `window/showDocument` support, the server falls back to `window/showMessage`.

## Diagnostics

Diagnostics are currently save-oriented for syntax and selector checks, plus index-driven for counterpart warnings.

### Parse Error

Published on save.

Message example:

```text
Fluent syntax error: Expected a token starting with "="
```

### `LSP-COPY` Marker Warning

Published on save.

Message:

```text
Entry still contains an `# [LSP-COPY]` marker
```

Current placement:

- whole-message marker -> marker line
- attribute-copy marker -> copied attribute key line

### Missing Origin Counterpart File

Message:

```text
Translation file has no origin-language counterpart for `only`
```

Current behavior:

- warning appears after save
- warning clears after background refresh sees the origin file

### Translation-Only Entry Or Attribute

Messages:

```text
Translation entry `extra` has no origin-language counterpart
Translation attribute `menu.tooltip` has no origin-language counterpart
```

Current behavior:

- range points at the local translation entry or attribute
- warning clears after background refresh sees the origin counterpart

### Unsupported Numeric Selector Key

Message:

```text
`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories
```

Current severity:

- error

### Unsupported And Missing Plural Categories

Disabled by default.

Example messages:

```text
`few` is not a supported plural category for `lv`
Numeric selector for `lv` is missing category `zero`
Numeric selector for `lv` is missing category `one`
```

Current severities:

- unsupported category -> error
- missing category -> warning

Ukrainian example:

- `few` and `many` are accepted
- missing `one` still warns

### Selector Style Mismatch

Disabled by default.

Example messages:

```text
Selector style is `whole`, but workspace prefers `prefix`
Selector style is `suffix`, but workspace prefers `prefix`
Selector style is `prefix`, but workspace prefers `whole`
```

Current behavior:

- both top-level and local selector occurrences can warn
- file config can override client style preference and style-warning enablement

### Diagnostic Clearing

Current behavior:

- fixed parse errors clear on save
- removed `LSP-COPY` markers clear on save
- `didClose` clears document diagnostics immediately
- index-driven counterpart warnings clear after background refresh

## Index And Refresh Behavior

Current indexed behavior:

- initial workspace index build runs after `initialized`
- if the client supports work-done progress, index build emits `$/progress`
- open-document overlays update indexed requests immediately
- background refresh picks up disk-only add, delete, and mtime changes

Progress token:

```text
fluent-lsp-index
```

Current request paths covered by live-index tests:

- definition
- references
- hover
- completion
- missing-origin diagnostics

## Trace Logging

Supported through standard LSP trace settings:

- `initialize.trace = "messages"`
- `initialize.trace = "verbose"`
- runtime `$/setTrace`

Message shape:

```text
fluent-lsp timing operation=<operation> elapsed_ms=<milliseconds>
```

Verbose payload examples:

```text
files=<count>
hit=true
count=<count>
command=<command> ok=true
```

Current traced operations covered by tests:

- `workspace/index`
- `textDocument/definition`
- `textDocument/references`
- `textDocument/hover`
- `textDocument/completion`
- `textDocument/codeAction`
- `textDocument/codeLens`
- `workspace/executeCommand`

## Weaker Guarantees

These areas are less firmly nailed down than the rest of the reference:

- `window/showMessage` fallback for selector combinations is much less exercised than the `window/showDocument` path
- `workspace/executeCommand` error paths are thin, especially malformed arguments and client refusal cases
- completion payloads are clearer than editor-side completion UX and insertion behavior
- snippet placeholder behavior is clearer at the edit payload level than at the editor interaction level
- config loading is less firm for multiple `file_masks`, alternate config filename `.fluent-lsp.toml`, and multi-root initialization
- background refresh is less firmly characterized under heavy churn or large batches
