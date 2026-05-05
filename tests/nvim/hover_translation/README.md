# hover_translation

## Feature

Hover from translation showing origin entry and comments

## Exercise

Requests hover on a translated key with comments, a translated plain-value line, an explicitly empty translated message, a translated selector-bearing attribute value, inside a translated selector branch, on the closing-line text after that selector, and on the concatenated second selector branch (`[one] dispositivo`). It asserts that translation hover shows the English origin block first and the local-language block second, for both comment hovers and message previews. The empty translated message must still render as `<empty>` in the local block after the origin-language fallback preview.

## Assumptions

Neovim exposes raw hover payloads through synchronous LSP requests, including Markdown code fences.

## Source Under Test

- `workspace/locales/es/app.ftl`
- `workspace/locales/en/app.ftl`

This scenario is self-contained. The source fixture and workspace config used by the smoke test live under this directory's `workspace/` tree.
