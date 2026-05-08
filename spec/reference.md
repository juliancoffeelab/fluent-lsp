# fluent-lsp Reference

## Configuration

### `fluent-lsp.toml`

Required keys:

```toml
origin_language = "en"
file_masks = ["{lang}/{filepath}.ftl"]
```

Optional keys:

```toml
selector_style = "prefix" # "prefix" | "suffix" | "whole"
error_on_unsupported_plural_categories = true
warn_on_missing_plural_categories = true
warn_on_selector_style_mismatch = true
```

Precedence:

- file config overrides client `workspace/didChangeConfiguration`
- client settings apply when file config does not override them

## Capabilities

`initialize` returns these capabilities:

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

Not provided:

- rename
- semantic tokens
- inlay hints
- document symbols

## `textDocument/definition`

Behavior:

- works only from translation files
- resolves to the matching origin entry or attribute definition
- opens one origin file location
- non-file URIs are rejected
- files outside the configured Fluent workspace are rejected

Origin source:

```ftl
welcome-title = Welcome

-brand-name = Nightly

button-copy =
    .label = Launch
```

Translation source:

```ftl
welcome-title = Bienvenido

-brand-name = Nightly

button-copy =
    .label = Lanzar
```

Attribute source:

```ftl
menu-save =
    .label = Save
```

Attribute translation:

```ftl
menu-save =
    .label = Guardar
```

Examples:

Cursor on:

```ftl
welcome-title = Bienvenido
```

`Go to Definition` opens `origin.ftl` and places the cursor on:

```ftl
welcome-title = Welcome
```

Cursor on:

```ftl
-brand-name = Nightly
```

`Go to Definition` opens `origin.ftl` and places the cursor on:

```ftl
-brand-name = Nightly
```

Cursor on:

```ftl
button-copy =
    .label = Lanzar
```

`Go to Definition` opens `origin.ftl` and places the cursor on:

```ftl
button-copy =
    .label = Launch
```

Cursor on:

```ftl
menu-save =
    .label = Guardar
```

`Go to Definition` opens `origin-menu.ftl` and places the cursor on:

```ftl
menu-save =
    .label = Save
```

Edge cases:

Origin source:

```ftl
welcome-title = Welcome
```

Cursor on:

```ftl
welcome-title = Welcome
```

`Go to Definition` does nothing.

Origin source:

```ftl
welcome-title = Welcome
```

Translation source:

```ftl
welcome-title = Bienvenido
local-only = Solo local
```

Cursor on:

```ftl
local-only = Solo local
```

`Go to Definition` does nothing.

Translation source:

```ftl
orphan-title = Huerfano
```

There is no matching origin counterpart file.

Cursor on:

```ftl
orphan-title = Huerfano
```

`Go to Definition` does nothing.

Error example for a non-file URI:

```json
{
  "code": -32602,
  "message": "expected a file URI"
}
```

Error example for a file outside the configured workspace:

```json
{
  "code": -32602,
  "message": "document is outside the configured Fluent workspace: /tmp/outside.ftl"
}
```

## `textDocument/references`

Behavior:

- works only from origin files
- returns translation entry or attribute definitions
- results use the indexed workspace model
- with `includeDeclaration = false`, the origin definition is not included

Origin source:

```ftl
welcome-title = Welcome

-brand-name = Nightly

menu-save =
    .label = Save
```

Translation sources:

```ftl
welcome-title = Bienvenido
-brand-name = Nightly
```

```ftl
welcome-title = Bienvenue
-brand-name = Nightly
```

```ftl
menu-save =
    .label = Guardar
```

Examples:

Cursor on:

```ftl
welcome-title = Welcome
```

`Find References` shows:

```ftl
welcome-title = Bienvenido
```

```ftl
welcome-title = Bienvenue
```

Cursor on:

```ftl
-brand-name = Nightly
```

`Find References` shows:

```ftl
-brand-name = Nightly
```

```ftl
-brand-name = Nightly
```

Cursor on:

```ftl
menu-save =
    .label = Save
```

`Find References` shows:

```ftl
menu-save =
    .label = Guardar
```

```ftl
menu-save =
    .label = Enregistrer
```

```ftl
menu-save =
    .label = Saglabat
```

Edge cases:

Origin source:

```ftl
orphan-title = Welcome
```

Translation sources:

```ftl
welcome-title = Bienvenido
```

```ftl
welcome-title = Bienvenue
```

Cursor on:

```ftl
orphan-title = Welcome
```

`Find References` shows no locations.

Translation source:

```ftl
welcome-title = Bienvenido
```

Cursor on:

```ftl
welcome-title = Bienvenido
```

`Find References` does nothing.

The same invalid-params errors as `textDocument/definition` apply to non-file URIs and files outside the configured workspace.

Index-backed behavior:

- dirty in-memory translation edits update references immediately
- `didClose` drops dirty overlay state and reverts to disk
- background refresh picks up added translation files
- background refresh removes deleted translation files

## `textDocument/hover`

Hover has three forms.

### Key hover

Key hover returns comment blocks when comments exist. Key hover does not fall back to body preview.

Origin source:

```ftl
# Comment-only hover coverage
# Keep this translator guidance visible on key hover
commented-preview = Preview text for hover comments.
```

Local source:

```ftl
# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
commented-preview = Texto de vista previa para comentarios de hover.
```

Hover payload:

````text
```ftl
# Comment-only hover coverage
# Keep this translator guidance visible on key hover
```

---

```ftl
# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
```
````

Rules:

- origin key hover returns origin comments only
- translation key hover returns origin comments first, local comments second
- if only origin comments exist, hover shows one origin comment block
- if only local comments exist, hover shows one local comment block
- if neither side has comments, hover returns no hover

Origin source:

```ftl
# Comment-only hover coverage
# Keep this translator guidance visible on key hover
commented-preview = Preview text for hover comments.
```

Local source:

```ftl
commented-preview = Texto de vista previa para comentarios de hover.
```

Hover payload:

````text
```ftl
# Comment-only hover coverage
# Keep this translator guidance visible on key hover
```
````

Origin source:

```ftl
local-note = Preview text for hover comments.
```

Local source:

```ftl
# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
local-note = Texto de vista previa para comentarios de hover.
```

Hover payload:

````text
```ftl
# Cobertura de hover con comentarios
# Mantener visible esta nota para traduccion en el hover de clave
```
````

Origin source:

```ftl
plain-note = Preview text for hover comments.
```

Local source:

```ftl
plain-note = Texto de vista previa para comentarios de hover.
```

Hover result:

```text
no hover
```

### Body hover

Body hover returns rendered preview text.

Origin source:

```ftl
welcome-body = Open the latest { -brand-name } build and pick up where you left off.
```

Local source:

```ftl
welcome-body = Abre la build mas reciente de { -brand-name } y sigue donde lo dejaste.
```

Hover payload:

````text
```ftl
Open the latest { -brand-name } build and pick up where you left off.
```

---

```ftl
Abre la build mas reciente de { -brand-name } y sigue donde lo dejaste.
```
````

Origin attribute source:

```ftl
menu-save =
    .tooltip = Save changes before closing the window
```

Hover payload in an origin file:

````text
```ftl
Save changes before closing the window
```
````

Missing origin message example.

Origin source:

```ftl
welcome-body = Open the latest { -brand-name } build and pick up where you left off.
```

Local source:

```ftl
local-only = Texto solo local.
```

Hover payload:

````text
```ftl
Texto solo local.
```
````

Empty local value example.

Origin source:

```ftl
empty-preview = English empty preview fallback.
```

Local source:

```ftl
empty-preview = { "" }
```

Hover payload:

````text
```ftl
English empty preview fallback.
```

---

```ftl
<empty>
```
````

Rules:

- origin-file body hover shows one origin preview block
- translation-file body hover shows origin first and local second when both exist
- if the local message exists but the origin message does not, hover shows one local preview block
- if the local message renders to an empty string, hover shows `<empty>`

### Selector hover

Selector hover prepends resolved selector values, then shows origin and local rendered preview blocks.

Origin source:

```ftl
install-hint =
    Copy the download link for { $gender ->
        [female] her
        [male] his
       *[other] their
    } account on { $count } { $count ->
        [one] device
       *[other] devices
    } now.
```

Local source:

```ftl
install-hint =
    Copia el enlace de descarga para la cuenta de { $gender ->
        [female] ella
        [male] el
       *[other] elle
    } en { $count } { $count ->
        [one] dispositivo
       *[other] dispositivos
    } ahora.
```

Hover payload for the `[female]` branch:

````text
`$gender=female`, `$count=*`

```ftl
Copy the download link for her account on { $count } devices now.
```

---

`$gender=female`, `$count=*`

```ftl
Copia el enlace de descarga para la cuenta de ella en { $count } dispositivos ahora.
```
````

Mismatch example.

Origin source:

```ftl
mismatch-rollout =
    Summary for { $platform ->
        [desktop] desktop
       *[mobile] mobile
    } users with { $count ->
        [0] no packages
        [1] one package
       *[other] { $count } packages
    } ready.
```

Local source:

```ftl
mismatch-rollout =
    Resumen para { $gender ->
        [female] ella misma
        [male] el mismo
       *[other] elle misme
    } con { $count ->
        [0] ningun paquete
        [1] un paquete
       *[other] { $count } paquetes
    } listo.
```

Hover payload for the `[0]` branch:

````text
`$platform=*`, `$count=0`

```ftl
Summary for mobile users with no packages ready.
```

---

`$gender=other`, `$count=0`

```ftl
Resumen para elle misme con ningun paquete listo.
```
````

Origin without selectors example.

Origin source:

```ftl
download-state = Download ready.
```

Local source:

```ftl
download-state =
    { $count ->
        [one] Descarga lista.
       *[other] Descargas listas.
    }
```

Hover payload for the `[one]` branch:

````text
```ftl
Download ready.
```

---

`$count=one`

```ftl
Descarga lista.
```
````

Rules:

- origin block is first
- local block is second
- shared selector variables are matched by name
- local-only selectors do not change origin preview rendering
- origin-only selectors fall back to their default branch and are shown as `*`
- if the origin message has no selectors, the origin block has no selector assignment header
- explicit numeric keys such as `0` and `1` are preserved
- plural-category names such as `zero` and `one` are preserved

## `textDocument/completion`

Behavior:

- completion is translation-oriented
- completion data comes from the origin counterpart file
- trigger character is `.`

Top-level key example.

Origin source:

```ftl
download-action =
    .label = Install build

download-count =
    .tooltip = Download { $files } files.
```

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

Origin source:

```ftl
menu-save =
    .label = Save
    .tooltip = Save this file
```

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

```ftl
# Completion doc coverage
# Keep this note in completion hover
commented-preview = Preview text for completion docs.
```

Documentation example for `menu-save.tooltip`:

```ftl
# Menu completion documentation
# Keep this entry visible in completion hover
menu-save =
    .tooltip = Save this file
```

Rules:

- translation files receive completion from their origin counterpart
- nested translation files use the nested origin counterpart
- origin files do not offer counterpart completion
- unmatched prefixes return an empty result
- files outside the configured workspace return invalid params

## `textDocument/codeAction`

Two action families exist:

- missing-entry quick fixes
- selector-generation and selector-rewrite refactors

### Quick Fix: add missing keys and attributes from English source

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

Rules:

- inserts parseable empty stubs
- preserves existing translated content
- does not add an `LSP-COPY` marker

### Quick Fix: copy missing keys and attributes from English source

Title:

```text
Copy missing keys and attributes from source
```

English source:

```ftl
menu-save =
    .tooltip = Save this file

sync-status = Sync ready
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

Rules:

- copied whole messages receive `# [LSP-COPY]`
- copied missing attributes receive a message-level marker such as `# [LSP-COPY .tooltip]`

### Quick Fix: copy one message from English source

Title:

```text
Copy `hello` from source
```

English source:

```ftl
hello = Hello World
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

Rules:

- replaces only the selected message
- does not touch unrelated incomplete entries

### Quick Fix: copy missing attributes for one message from English source

Title:

```text
Copy missing attributes for `download-action` from source
```

English source:

```ftl
download-action =
    .label = Install build
    .tooltip = Download this build
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

Rules:

- inserts only missing attributes for the selected message
- hover on the copied attribute does not surface the `LSP-COPY` marker as comment content

### Refactor: selector generation

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

Inserted snippet text:

```text
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

Rules:

- default preferred style is `prefix`
- client `selector_style` changes preferred style
- file config `selector_style` overrides client preference
- attributes participate
- nested function placeables can be the anchor
- punctuation remains attached
- ambiguous messages such as `range-summary` hide generation actions

### Refactor: selector rewrite

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

Rules:

- whole selectors can rewrite to prefix and suffix
- prefix selectors can rewrite to whole
- suffix selectors can rewrite to whole and prefix
- suffix-like selectors without distinct outer text only offer the distinct `prefix` collapse
- nested selectors can be rewritten inside variants
- attribute rewrite edits do not consume surrounding comments or the attribute key
- rewrites preserve assignment spacing such as `= {`

### Snippet edit form

When the client supports:

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

selector-generation actions use `WorkspaceEdit.documentChanges` with `SnippetTextEdit`.

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
  "file:///.../translation.ftl",
  "install-hint"
]
```

With `window/showDocument` support, the server opens a temp Markdown document containing sections such as:

```text
Language: `es`
Source language: `en`
Source text:
Local text:
Source language combinations:
Local language combinations:
```

Document example.

Origin source:

```ftl
install-hint =
    Copy the download link for { $gender ->
        [female] her
        [male] his
       *[other] their
    } account on { $count } { $count ->
        [one] device
       *[other] devices
    } now.
```

Local source:

```ftl
install-hint =
    Copia el enlace de descarga para la cuenta de { $gender ->
        [female] ella
        [male] el
       *[other] elle
    } en { $count } { $count ->
        [one] dispositivo
       *[other] dispositivos
    } ahora.
```

Markdown payload excerpt:

````text
# Selector Combinations: `install-hint`

Language: `es`
Source language: `en`

## Source language combinations

`$gender=other`, `$count=other`
```ftl
Copy the download link for their account on { $count } devices now.
```

## Local language combinations

`$gender=other`, `$count=other`
```ftl
Copia el enlace de descarga para la cuenta de elle en { $count } dispositivos ahora.
```
````

Without `window/showDocument` support, the server falls back to `window/showMessage`.

## Diagnostics

### Parse error

Example source:

```ftl
welcome-title = Welcome

g@Rb@ge = broken
```

Example diagnostic:

```json
{
  "severity": 1,
  "message": "Fluent syntax error: Expected a token starting with \"=\""
}
```

### `LSP-COPY` marker warning

Example source:

```ftl
# [LSP-COPY]
hello = Hello World

# [LSP-COPY .tooltip]
menu-save =
    .label = Guardar
    .tooltip = Save this file
```

Example diagnostics:

```json
[
  {
    "severity": 2,
    "message": "Entry still contains an `# [LSP-COPY]` marker"
  },
  {
    "severity": 2,
    "message": "Entry still contains an `# [LSP-COPY]` marker"
  }
]
```

Placement:

- whole-message marker -> marker line
- attribute-copy marker -> copied attribute key line

### Missing origin counterpart file

Example source path:

```text
es/only.ftl
```

Without:

```text
en/only.ftl
```

Example diagnostic:

```json
{
  "severity": 2,
  "message": "Translation file has no origin-language counterpart for `only`"
}
```

### Translation-only entry or attribute

Example translation:

```ftl
shared = Hola
extra = Solo local
menu =
    .label = Guardar
    .tooltip = Solo aqui
```

Example English source:

```ftl
shared = Hello
menu =
    .label = Save
```

Example diagnostics:

```json
[
  {
    "severity": 2,
    "message": "Translation entry `extra` has no origin-language counterpart"
  },
  {
    "severity": 2,
    "message": "Translation attribute `menu.tooltip` has no origin-language counterpart"
  }
]
```

### Unsupported numeric selector key

Example source:

```ftl
bad-key =
    { $count ->
        [admins] nope
        [one] ok
       *[other] ok
    }
```

Example diagnostic:

```json
{
  "severity": 1,
  "message": "`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories"
}
```

### Unsupported and missing plural categories

Disabled by default.

Example Latvian source with an unsupported category:

```ftl
bad-zero =
    { $count ->
        [few] slikti
       *[other] labi
    }
```

Example diagnostic:

```json
{
  "severity": 1,
  "message": "`few` is not a supported plural category for `lv`"
}
```

Example Latvian source with a missing category:

```ftl
incomplete-zero =
    { $count ->
        [one] viena pakotne
       *[other] pakotnes
    }
```

Example diagnostic:

```json
{
  "severity": 2,
  "message": "Numeric selector for `lv` is missing category `zero`"
}
```

Locale note:

- category validation is locale-specific
- the origin file does not need to use the same category set

### Selector style mismatch

Disabled by default.

Example source:

```ftl
whole-coins = { $coins ->
    [one] Tienes { $coins } moneda.
   *[other] Tienes { $coins } monedas.
}
```

With preference:

```toml
selector_style = "prefix"
warn_on_selector_style_mismatch = true
```

Example diagnostic:

```json
{
  "severity": 2,
  "message": "Selector style is `whole`, but workspace prefers `prefix`"
}
```

### Clearing

- fixed parse errors clear on save
- removed `LSP-COPY` markers clear on save
- `didClose` clears document diagnostics immediately
- index-driven counterpart warnings clear after background refresh

## Index And Refresh

- initial workspace index build runs after `initialized`
- if the client supports work-done progress, index build emits `$/progress`
- open-document overlays update indexed requests immediately
- background refresh picks up disk-only add, delete, and mtime changes

Progress token:

```text
fluent-lsp-index
```

Affected request paths:

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

Operations:

- `workspace/index`
- `textDocument/definition`
- `textDocument/references`
- `textDocument/hover`
- `textDocument/completion`
- `textDocument/codeAction`
- `textDocument/codeLens`
- `workspace/executeCommand`

## Underspecified Areas

- `window/showMessage` fallback for selector combinations is less clearly defined than the `window/showDocument` path
- malformed `workspace/executeCommand` argument handling is less clearly defined than the success path
- completion payload behavior is clearer than editor-side insertion behavior
- snippet payload shape is clearer than placeholder navigation behavior
- multi-mask config, alternate config filename `.fluent-lsp.toml`, and multi-root initialization are less clearly defined than the single-root locale-tree layout
