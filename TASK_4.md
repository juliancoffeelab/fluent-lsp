# Task 4

Added a standard-LSP path for expanding selector-combination details without custom editor integrations. The server now publishes a `CodeLens` above Fluent entries that have selector expansions and wires that lens to `workspace/executeCommand`. When invoked, the command computes the full combination list, writes a temp Markdown document, and opens it through `window/showDocument`.

Pros:
- Uses only standard LSP features: `textDocument/codeLens`, `workspace/executeCommand`, and `window/showDocument`.
- Keeps hover compact while still giving users a way to request the complete selector expansion set.
- Reuses the existing selector-expansion and hover-entry rendering instead of inventing a parallel rendering path.
- Scales better than `showMessage` because the result opens as a readable document instead of a transient notification payload.

Cons:
- The feature now leaves temp Markdown files behind unless the OS or user cleans the temp directory.
- The full expansion is not embedded inline in hover; it is an explicit follow-up action.
