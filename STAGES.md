# Implementation Stages

## 1. Indexed Workspace Model

Introduced a startup-built `WorkspaceIndex` containing one replaceable `IndexedFile` per Fluent file. Each file entry stores its file-mask identity, source text, key set, and LSP definition ranges. Derived maps resolve `(mask, language, filepath)` back to indexed paths, which supports origin-to-translation and translation-to-origin lookups without rescanning every request.

Architectural decision: open buffers are represented as overlays. `didOpen`, `didChange`, `didSave`, and `didClose` replace only the affected file entry. `didClose` drops the overlay and re-reads disk immediately, so dirty-buffer state does not leak into later requests.

## 2. Request Migration

Moved `textDocument/definition`, `textDocument/references`, `textDocument/hover`, `textDocument/completion`, and missing-entry code actions onto indexed source data. Selector generation/rewrite actions still operate on the current document text because they are local edit transforms, not cross-file lookups.

No custom editor integration was added. Index build progress uses standard `$/progress` work-done notifications when the client advertises `window.workDoneProgress`.

## 3. Invalidation And Diagnostics

Live editor invalidation is immediate through standard LSP document notifications. Disk-only file additions/deletions are detected by a periodic one-second index file-set refresh during requests; this avoids putting a full workspace scan on every warm request while still allowing newly created/deleted files to appear without restarting the server.

Local-only translation files now publish a standard diagnostic on save: `Translation file has no origin-language counterpart for ...`. The warning clears after the matching origin file exists and the index refresh sees it.

## 4. Tests Added

Added unit coverage for per-file extraction, reverse-map replacement/removal, overlay precedence, dirty-close reversion, parse-error recovery, and local-only detection.

Added JSON-RPC integration coverage for startup progress, live origin edits, live translation edits, dirty-close reversion, disk add/delete refresh, and local-only warning clearing.

Added Neovim smoke scenario `tests/nvim/index_invalidation_local_only/` with its own README and local fixtures. It exercises indexed definition, references, hover, completion, dirty close, and local-only diagnostics through Neovim’s standard LSP client.

## 5. Benchmark Harness

Added `fluent-lsp-bench`, an in-repo stdio LSP benchmark driver. It deterministically generates:

- `small-realistic`: 4 languages, 3 files/language, 40 messages/file.
- `synthetic-large`: 31 languages, 50 files/language, default 2000 messages/file.

For fast CI/local validation, `FLUENT_LSP_BENCH_MESSAGES` can lower the synthetic message count. The harness records machine-readable JSON with indexed startup time, warm request p50/p95/max, RSS, generated message counts, and startup file counts.

## 6. Benchmark Results

Recorded run: `benchmarks/results/indexed-2026-05-05.json`.

Command:

```bash
FLUENT_LSP_BENCH_MESSAGES=10 target/debug/fluent-lsp-bench benchmarks/results/indexed-2026-05-05.json target/debug/fluent-lsp
```

Results from this bounded synthetic validation run:

- Small realistic: startup/index `87.30 ms`, RSS `13,604 KB`.
- Small warm p50: definition `28.02 ms`, references `28.69 ms`, hover `28.20 ms`, completion `1.58 ms`, missing-entry code action `90.75 ms`.
- Synthetic 31-language/1550-file run at 10 messages/file: startup/index `1285.25 ms`, RSS `21,860 KB`.
- Synthetic warm p50: definition `27.03 ms`, references `26.12 ms`, hover `26.67 ms`, completion `23.03 ms`, missing-entry code action `31.04 ms`.

The full default synthetic generator is configured for 2000 messages/file. A full-scale run was started with:

```bash
target/debug/fluent-lsp-bench benchmarks/results/indexed-full-2026-05-05.json target/debug/fluent-lsp
```

That full run did not complete within roughly eight minutes in this environment and was stopped to avoid tying up the session indefinitely. The checked-in JSON result is therefore the bounded validation run above.
