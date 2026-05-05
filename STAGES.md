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

## 7. Profiling Setup

Perf-based flamegraph profiling was enabled after lowering the kernel perf restriction:

```bash
sudo sysctl kernel.perf_event_paranoid=1
```

`cargo-flamegraph` is available at `/home/codex/.cargo/bin/cargo-flamegraph`. A direct `perf record -F 99 -g -- sleep 0.1` succeeds with kernel-symbol-resolution warnings only, so user-space sampling is available for benchmark profiling.

Flamegraph captures were run with reduced synthetic size to keep artifacts and post-processing practical:

```bash
FLUENT_LSP_BENCH_MESSAGES=20 cargo flamegraph --dev --no-inline -F 49 --post-process 'tee benchmarks/flamegraphs/indexer-debug-m20.folded' --bin fluent-lsp-bench -o benchmarks/flamegraphs/indexer-debug-m20.svg -- benchmarks/results/indexer-debug-m20.json target/debug/fluent-lsp
CARGO_PROFILE_RELEASE_DEBUG=true FLUENT_LSP_BENCH_MESSAGES=20 cargo flamegraph --release --no-inline -F 49 --post-process 'tee benchmarks/flamegraphs/indexer-release-m20.folded' --bin fluent-lsp-bench -o benchmarks/flamegraphs/indexer-release-m20.svg -- benchmarks/results/indexer-release-m20.json target/release/fluent-lsp
```

The first debug attempt at 50 messages/file and default inline expansion produced a 2.1 GB `perf.data` and stalled during post-processing, so the final captures use `--no-inline`, `-F 49`, and 20 messages/file.

Debug profile results for 31 languages, 50 files/language, 20 messages/file:

- Artifact: `benchmarks/flamegraphs/indexer-debug-m20.svg`
- Folded stacks: `benchmarks/flamegraphs/indexer-debug-m20.folded`
- JSON: `benchmarks/results/indexer-debug-m20.json`
- Synthetic startup/index: `3422.58 ms`
- Synthetic RSS: `28,760 KB`
- Synthetic warm p50: definition `43.66 ms`, references `43.60 ms`, hover `41.77 ms`, completion `35.63 ms`, missing-entry code action `54.68 ms`

Release profile results for the same reduced synthetic shape:

- Artifact: `benchmarks/flamegraphs/indexer-release-m20.svg`
- Folded stacks: `benchmarks/flamegraphs/indexer-release-m20.folded`
- JSON: `benchmarks/results/indexer-release-m20.json`
- Synthetic startup/index: `331.50 ms`
- Synthetic RSS: `22,164 KB`
- Synthetic warm p50: definition `24.85 ms`, references `24.49 ms`, hover `24.63 ms`, completion `24.19 ms`, missing-entry code action `26.06 ms`

## 8. Baseline Versus Indexed Request Timing

A pre-index release binary was built from `HEAD^` in `/tmp/fluent-lsp-baseline`, then compared against the indexed release binary with the same reduced synthetic shape used for profiling: 31 languages, 50 files/language, 20 messages/file.

Comparison artifact: `benchmarks/results/baseline-vs-index-release-m20.json`.

Baseline pre-index startup-to-ready was `9.94 ms`; indexed startup-to-ready was `331.78 ms`, including eager index build.

Warm request p50, pre-index release:

- definition: `1.24 ms`
- references: `30.02 ms`
- hover: `1.34 ms`
- completion: `1.22 ms`
- missing-entry code action: `3.53 ms`

Warm request p50, indexed release:

- definition: `23.25 ms`
- references: `23.24 ms`
- hover: `22.29 ms`
- completion: `22.26 ms`
- missing-entry code action: `25.18 ms`

Observation: this reduced-size comparison exposes a regression in the indexed request path. The indexed handlers currently clone the workspace index out of shared state for request processing; at this shape that clone cost dominates most warm requests. References improve slightly versus the on-demand scan, but definition, hover, completion, and code actions regress. The next optimization should remove whole-index cloning from request handlers and use scoped read locks or cheaper `Arc`-backed file entries.

## 9. Standard Trace Timing Logs

Added standard LSP `$/logTrace` timing notifications. The server honors the initial `initialize.trace` value and runtime standard `$/setTrace` updates. No custom editor integration is required.

Trace messages use this stable shape:

```text
fluent-lsp timing operation=<lsp-or-internal-operation> elapsed_ms=<milliseconds>
```

When trace is `messages`, only `message` is sent. When trace is `verbose`, `verbose` includes the request-specific summary:

- `workspace/index`: `files=<count>`
- `textDocument/definition`: `hit=true|false`
- `textDocument/references`: `count=<count>`
- `textDocument/hover`: `hit=true|false`
- `textDocument/completion`: `count=<count>`
- `textDocument/codeAction`: `count=<count>`
- `textDocument/codeLens`: `count=<count>`
- `workspace/executeCommand`: `command=<command> ok=true|false`

Added JSON-RPC integration coverage for `initialize.trace`, verbose payloads, and runtime `$/setTrace`. Added Neovim smoke scenario `tests/nvim/log_trace_timings/` with local fixtures and README coverage.
