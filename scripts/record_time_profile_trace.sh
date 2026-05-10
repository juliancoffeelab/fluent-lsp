#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <bench-bin> [-- bench args...]" >&2
  exit 1
fi

bin="$1"
shift

prefix="benchmarks/profiles/$bin"

bench_args=()
if [[ $# -gt 0 ]]; then
  if [[ "$1" != "--" ]]; then
    echo "usage: $0 <bench-bin> [-- bench args...]" >&2
    exit 1
  fi
  shift
  bench_args=("$@")
fi

trace_path="${prefix}.trace"

mkdir -p "$(dirname "$trace_path")"
rm -rf "$trace_path"

echo "building $bin"
cargo build -p bench --release --bin "$bin"

echo "recording $trace_path"
if [[ ${#bench_args[@]} -gt 0 ]]; then
  xcrun xctrace record \
    --template 'Time Profiler' \
    --output "$trace_path" \
    --launch -- \
    "target/release/$bin" \
    "${bench_args[@]}"
else
  xcrun xctrace record \
    --template 'Time Profiler' \
    --output "$trace_path" \
    --launch -- \
    "target/release/$bin"
fi

echo "$trace_path"
