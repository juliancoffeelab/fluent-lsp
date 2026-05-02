# Task 1

Implemented subtle hover syntax distinction by separating comments from the hovered entry into their own Fluent code block while preserving real Fluent comment prefixes. Hover now keeps `###` resource comments, `##` group comments, and `#` entry comments visible as written, followed by a separate Fluent block for the entry itself. Selector-combination output still appends below the entry.

Pros:
- Works in plain Markdown-capable LSP clients without depending on custom HTML or editor-specific Fluent syntax support.
- Preserves the actual Fluent comment syntax, including the distinction between resource, group, and entry comments.
- Keeps the actual Fluent entry intact in its own fenced block, so copy/paste and scanning still work.

Cons:
- This is presentation-level separation, not true token-aware syntax coloring inside the code block.
- The hover now uses two Fluent blocks when comments exist, which is more explicit but slightly more verbose.
