#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <bench-bin>" >&2
  exit 1
fi

bin="$1"
prefix="benchmarks/profiles/$bin"
trace_path="${prefix}.trace"

xml_path="${prefix}.time-profile.xml"
folded_path="${prefix}.folded"

for tool in xcrun python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool" >&2
    exit 1
  fi
done

mkdir -p "$(dirname "$prefix")"

echo "exporting time profile to $xml_path"
xcrun xctrace export \
  --input "$trace_path" \
  --xpath '/trace-toc/*/data/table[@schema="time-profile"]' \
  --output "$xml_path"

echo "collapsing stacks to $folded_path"
python3 scripts/collapse_time_profile_stacks.py "$xml_path" --output "$folded_path"

echo "rendering artifacts from $folded_path"
python3 scripts/render_folded_profile_artifacts.py "$folded_path" --output-prefix "$prefix"
