- [x] `K` on the message itself shows the formatted message preview.
- [x] Inline placeables keep Fluent braces, so `{ $var }` is no longer collapsed to `$var`.
- [x] `K` inside or outside selector branches shows all selector choices, using explicit branch selections from the cursor context and `*` for unresolved selectors.
- [x] Selector-bearing hover follows the CodeLens preview format: selector header first, then the rendered message block.
- [x] Added focused unit coverage for hover formatting, selector context, placeable preservation, and same-language selector-combination rendering.
- [x] Displays stay human-readable and no longer emit raw `\n` escapes in hover or selector-combination output.
- [x] The selector-combination document no longer duplicates source/current sections when the current file is already English.
- [x] Selector-combination previews render exactly as displayed in fenced Fluent blocks, starting at column zero.

Each item above is covered by unit tests, integration tests, and Neovim smoke coverage in the current tree.
