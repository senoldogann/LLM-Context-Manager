#!/usr/bin/env bash
# Run the external benchmark: for each task file, evaluate structural vs hybrid.
#
# Usage:
#   benchmarks/scripts/run_benchmark.sh
#
# Env:
#   CCM_CLI      - path to the ccm-cli binary (default: target/release/ccm-cli)
#   CCM_NO_EMBED - set to 1 to skip search_code tasks (embedder not required)
#
# Prereqs:
#   - benchmarks/corpus/<name> cloned (run benchmarks/scripts/fetch_corpus.sh)
#   - repos indexed (eval builds the index automatically if missing)
#   - embedding provider reachable (default: Ollama at 127.0.0.1:11434 with
#     mxbai-embed-large) unless CCM_NO_EMBED=1
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CLI="${CCM_CLI:-$ROOT/target/release/ccm-cli}"
RESULTS="$ROOT/benchmarks/results"
mkdir -p "$RESULTS"

if [[ ! -x "$CLI" ]]; then
  echo "[error] ccm-cli not found at $CLI (set CCM_CLI or build first)" >&2
  exit 1
fi

cd "$ROOT"
for tasks in benchmarks/tasks/*.json; do
  name="$(basename "$tasks" .json)"
  report="$RESULTS/$name.compare.json"
  echo "=== [$name] ==="
  "$CLI" eval --tasks "$tasks" --compare --report "$report" \
    2>&1 | grep -E "Pass Rate|Structural|Hybrid|Improvement|query_type|Failed|failed|ERROR" | tail -15
  echo "[$name] report: $report"
done

echo "=== Done. Run benchmarks/scripts/aggregate.py for the summary table. ==="
