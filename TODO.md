# Global Index, Benchmarks, and Validation Plan

- Create and keep a top-level `STAGES.md` updated during implementation with the usual notes on problems encountered, how they were solved, and any architectural decisions made while building the benchmark harness and global index.

## 1. Baseline Current Behaviour First

- Benchmark the current on-demand implementation before any index work lands.
- Cover both cold requests on freshly opened files and warm consecutive requests against the same workspace.
- Measure at least:
  - `textDocument/definition`
  - `textDocument/references`
  - `textDocument/hover`
  - `textDocument/completion`
  - `textDocument/codeAction` for missing-entry actions
  - startup-to-ready time for the server without an index
- Record:
  - wall-clock latency per request
  - p50, p95, and max latency across repeated runs
  - total startup/index time
  - peak and steady-state memory if practical
  - number of files scanned/read per operation if we can expose that cheaply in benchmark-only instrumentation

## 2. Benchmark Fixtures

- Keep two benchmark flavours:
  - a relatively small realistic workspace
  - a synthetic large workspace
- The synthetic workspace should model scale explicitly:
  - 30+ languages
  - around 50 logical files per language
  - up to 2000 messages per file
- The synthetic workspace generator must be deterministic so regressions are comparable across runs.
- The generator should vary structure, not only size:
  - plain values
  - attributes
  - comments
  - terms
  - selector-heavy messages
  - partially translated files
  - some local-only files
  - some origin-only files
- Keep the benchmark fixtures separate from normal integration and smoke fixtures so test runtime does not explode.

## 3. Benchmark Harness

- Add a repeatable benchmark harness in-repo rather than relying on manual editor profiling.
- The harness should support:
  - cold start runs from a fresh process
  - warm repeated requests against the same process
  - scripted request sequences that mirror editor usage
  - machine-readable output so before/after comparisons can be committed or attached to PRs
- Request sequences should include:
  - repeated goto-definition across different translation files
  - references from an origin file with many translations
  - hover on keys, message bodies, and attributes
  - completion for message keys and attribute names
  - mixed navigation patterns, not just the same request in a tight loop
- Benchmark output should clearly distinguish:
  - no-index baseline
  - indexed startup cost
  - indexed warm-request cost

## 4. Introduce a Global Index

- Introduce a global eagerly built workspace index for Fluent files.
- Build the index at LSP startup.
- The index should support at least:
  - key -> definition lookup
  - origin file -> translation counterparts
  - translation file -> origin counterpart
  - per-file key/attribute existence
  - comment/source metadata needed by hover and completion
  - lookup of local-only files and origin-only files
- Keep index structures per file so invalidation can replace one file entry at a time instead of forcing full rebuilds.
- Preserve open-buffer text as the source of truth over on-disk content for files that are currently modified in the editor.

## 5. Index Build Progress Reporting

- Indexing should report its work to the client per file.
- Use standard LSP progress/reporting rather than custom client integration.
- Report:
  - index start
  - current file being processed
  - counts of processed files vs total if available
  - index complete
- Make sure progress reporting degrades safely on clients that do not surface it well.

## 6. Request Migration Plan

- Migrate requests to the index incrementally rather than all at once.
- First move:
  - `textDocument/definition`
  - `textDocument/references`
- Then move:
  - `textDocument/hover`
  - `textDocument/completion`
  - missing-entry code actions
- Keep a temporary fallback path while the index work is still being stabilized so request behaviour can be compared during development.
- Remove the fallback once benchmarks and tests prove the index is correct.

## 7. Index Invalidation Rules

- Index invalidation needs an explicit design before implementation, because this is likely the largest bug source.
- Invalidation must handle:
  - `didOpen`
  - `didChange`
  - `didSave`
  - `didClose`
  - new files appearing on disk
  - files being deleted on disk
  - config changes that affect file masks or origin language
- Treat unsaved open buffers as overlays on top of the disk index.
- Replacing one file in the index must also update all derived cross-file mappings that depend on it.
- If a file becomes invalid Fluent after an edit, the index must fail softly:
  - keep diagnostics correct
  - avoid poisoning unrelated files
  - decide explicitly whether requests fall back to best-effort partial data or to “no result”
- `didClose` is especially important:
  - if the file had unsaved in-memory changes, the overlay must be dropped
  - the index must revert to on-disk state
  - follow-up requests must reflect that reversion immediately

## 8. Invalidation Test Matrix

- Add focused automated tests for every invalidation path.
- Required cases:
  - editing a translation file updates definition/hover/completion/results without restarting the server
  - editing an origin file updates translation-side definition/reference/hover/completion results without restarting the server
  - adding a new message to origin updates missing-entry actions in translations
  - deleting a message from origin removes indexed definition/reference targets cleanly
  - adding a new translation file makes it appear in references
  - deleting a translation file removes it from references
  - changing a message key invalidates old lookups and enables new lookups
  - adding/removing attributes updates attribute completion and hover
  - changing comments updates key-hover and completion documentation
  - unsaved buffer edits override disk-backed index content
  - closing a dirty buffer reverts back to disk-backed index content
  - parse-broken in-memory content does not permanently corrupt subsequent indexed results after the content is fixed
  - config reload with changed file masks or origin language triggers a safe rebuild
- Add specific regression tests for stale data:
  - old origin preview still shown after source changed
  - deleted translation still returned by references
  - renamed key still returned by completion
  - hover/comments taken from pre-edit content after `didChange`

## 9. Unit Tests for Index Internals

- Add unit tests for:
  - per-file parse/index extraction
  - merging file entries into global maps
  - replacing one indexed file entry
  - removing one indexed file entry
  - overlay precedence between open-buffer text and disk text
  - marker/error handling for parse-invalid files
  - local-only and origin-only file detection
- Unit tests should assert both direct file data and derived reverse mappings, because invalidation bugs often hide in the reverse maps.

## 10. Integration Tests

- Add integration tests for indexed request correctness over JSON-RPC.
- Integration tests should cover:
  - initial eager index build
  - live invalidation through open/change/save/close
  - cross-file updates propagating without restart
  - progress notifications during index build if observable through standard LSP messages
  - warnings for files that exist only in a local language and not in the origin language
- Add targeted integration tests for the local-only-file warning:
  - warning appears for local-only file
  - warning disappears when the origin counterpart is created
  - warning updates correctly when config or file masks change

## 11. Neovim Smoke Coverage

- Every user-visible indexed feature still needs Neovim smoke coverage.
- Add or extend smoke scenarios for:
  - goto-definition using the index
  - references using the index
  - hover after live edits
  - completion after live edits
  - local-only-file warning
  - behaviour after closing a modified buffer and reopening
- Add at least one smoke scenario that proves invalidation from an editor workflow, not only from synthetic JSON-RPC calls:
  - open file
  - edit content
  - save or do not save depending on scenario
  - request feature again
  - verify updated result

## 12. Post-Index Benchmarks

- Benchmark again after the index lands.
- Measure:
  - initial indexing time
  - indexed cold-start cost
  - indexed warm-request cost
  - cost of invalidating one file after `didChange`
  - cost of invalidating many files if config forces a rebuild
- Keep the same two benchmark flavours as the baseline run.
- Compare baseline and indexed results side by side.
- We should expect:
  - startup to get slower
  - warm cross-file requests to get materially faster
  - invalidation cost to stay proportional to the changed file, not to whole-workspace size

## 13. Acceptance Criteria

- Baseline and post-index benchmark results are checked in or otherwise recorded in a reproducible way.
- All indexed requests are covered by unit tests, integration tests, and Neovim smoke tests where the feature is user-visible.
- Index invalidation is covered by explicit regression tests, not only happy-path tests.
- Local-only-file warnings are implemented and tested.
- No custom editor/client integration is required; all behaviour uses standard LSP features.
