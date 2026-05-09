# AGENTS

## Integration Constraints

- Do not design or require non-standard editor/client integrations for this project.
- Prefer standard LSP features and behavior over client-specific extensions, custom UI hooks, or bespoke editor plumbing.
- If a feature cannot be delivered cleanly through standard LSP alone, keep the solution within standard protocol capabilities rather than assuming custom client work will be added later.
- Fallback behavior is unacceptable.
- Do not add graceful-degradation paths that silently substitute different behavior, looser rendering, alternate UI, placeholder output, or best-effort approximations.
- If the specified behavior cannot be produced correctly, surface an error and fix the implementation or the spec instead of introducing a fallback.

## Testing Requirements

- Every user-visible change should have JSON-RPC integration test.
- In tests, `contains`-style assertions are banned for user-visible results. Use direct comparisons instead.
- When validating rendered Fluent output in tests, each test must validate the result with bundle message formatting and assert it with a direct `assert_eq!`.
- In tests, prefer raw string literals like `r#"..."#` for multiline or quote-heavy user-visible text instead of `concat!` chains or escaped-quote-heavy string literals.

## Test Helper Policy

- Do not introduce new helper abstractions, helper modules, or helper layers in tests beyond the current `LspProcess` struct helpers already present in `tests/integration_lsp.rs`.

## TODO

- Remove the `window/showMessage` fallback for selector combinations. This feature should use only standard `window/showDocument`.
