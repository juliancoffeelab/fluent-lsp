# AGENTS

## Integration Constraints

- Do not design or require non-standard editor/client integrations for this project.
- Prefer standard LSP features and behavior over client-specific extensions, custom UI hooks, or bespoke editor plumbing.
- If a feature cannot be delivered cleanly through standard LSP alone, keep the solution within standard protocol capabilities rather than assuming custom client work will be added later.

## Testing Requirements

- Every user-visible feature must have a respective Neovim smoke test.
- Do not treat JSON-RPC integration coverage as sufficient for feature-complete validation when the behavior is meant to be used from Neovim.
- Feature work is not complete until the corresponding Neovim smoke path exists and is exercised by the test suite.

## Smoke Test Documentation

- Every feature-specific Neovim smoke test directory must include a detailed `README.md`.
- That README must document:
  - what feature the scenario covers
  - how the scenario is exercised
  - assumptions or client/setup expectations
  - the exact source fixture or file path used for the test
- Every feature-specific Neovim smoke test directory must be self-contained.
- The exact source-under-test must live inside that scenario directory, not only in a shared external fixture tree.
- Do not rely on undocumented shared fixture paths when a scenario claims to cover a specific feature.
- Do not consider a feature test complete if the scenario exists without this documentation and local source fixture.
