.PHONY: bench-criterion profile-lense

bench-criterion:
	cargo bench -p bench --bench request_paths -- --discard-baseline --noplot

profile-lense:
	bash scripts/record_time_profile_trace.sh fluent-lsp-bench-code-lens
	bash scripts/export_time_profile_artifacts.sh fluent-lsp-bench-code-lens
