## Phase 7

- Problem: the old attribute copy marker was emitted as an indented `# [LSP-COPY]` line between attributes. Real `fluent-syntax` parsing does not attach that line to an attribute comment AST node, because attributes do not carry comments at all.
- Solution: attribute-copy markers now move to a message-level machine comment placed immediately above the owning message, encoded as `# [LSP-COPY .attribute]`. That keeps the file valid Fluent while making the marker semantically attach to the message rather than pretending it is inline attribute comment context.
- Problem: the old marker format could leak into comment-oriented UI paths because top-level copied markers are still real Fluent comments.
- Solution: hover and completion comment rendering now filter machine marker lines out while preserving real translator comments that share the same comment block.
- Problem: the previous tests only proved the copied text parsed and that marker warnings existed. They did not pin the real parser behavior for indented attribute markers, and they did not prove the new UX from Neovim.
- Solution: added parser-backed unit coverage for the attribute case, updated integration coverage for the new marker placement and warning targeting, and extended Neovim smoke coverage so copied-attribute hover does not surface the machine marker as translator comment content.
- Architectural note: I kept the marker as valid Fluent comment syntax instead of inventing a non-FTL prefix. A non-comment prefix would make copied buffers invalid Fluent. The change was therefore to move the marker to a place where comment semantics are real and then treat it as machine metadata in display code.
