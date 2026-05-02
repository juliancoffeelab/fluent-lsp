# Task 4

Added a standard-LSP path for expanding selector-combination details without custom editor integrations. The server now publishes a `CodeLens` above Fluent entries that have selector expansions and wires that lens to `workspace/executeCommand`. When invoked, the command computes the full combination list and sends it through `window/showMessage`.

Pros:
- Uses only standard LSP features: `textDocument/codeLens`, `workspace/executeCommand`, and `window/showMessage`.
- Keeps hover compact while still giving users a way to request the complete selector expansion set.
- Reuses the existing selector-expansion logic instead of inventing a parallel rendering path.

Cons:
- `window/showMessage` is a blunt output channel and may be awkward for large payloads in some editors.
- The full expansion is not embedded inline in hover; it is an explicit follow-up action.
