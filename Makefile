.PHONY: bench-criterion

bench-criterion:
	cargo bench -p bench --bench request_paths -- --discard-baseline --noplot
