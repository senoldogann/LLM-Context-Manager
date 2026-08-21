#!/usr/bin/env bash
# Fetch the external benchmark corpus at pinned refs (reproducibility).
# Usage: benchmarks/scripts/fetch_corpus.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CORPUS_DIR="$ROOT/benchmarks/corpus"
CORPUS_JSON="$ROOT/benchmarks/corpus.json"

mkdir -p "$CORPUS_DIR"

python3 - "$CORPUS_JSON" "$CORPUS_DIR" <<'PY'
import json, subprocess, sys, os

corpus_file, corpus_dir = sys.argv[1], sys.argv[2]
with open(corpus_file) as f:
    data = json.load(f)

for repo in data["repos"]:
    name, url, ref, commit = repo["name"], repo["url"], repo["ref"], repo["commit"]
    dest = os.path.join(corpus_dir, name)
    if os.path.isdir(os.path.join(dest, ".git")):
        print(f"[skip] {name}: already cloned at {dest}")
        continue
    print(f"[clone] {name} @ {ref} ({commit[:10]})")
    subprocess.run(
        ["git", "clone", "--quiet", "--depth", "1", "--branch", ref, url, dest],
        check=True,
    )
    actual = subprocess.run(
        ["git", "-C", dest, "rev-parse", "HEAD"], capture_output=True, text=True, check=True
    ).stdout.strip()
    if actual != commit:
        # Shallow tag clone should match; fetch the exact commit if the tag moved.
        subprocess.run(["git", "-C", dest, "fetch", "--quiet", "--depth", "1", "origin", commit], check=True)
        subprocess.run(["git", "-C", dest, "checkout", "--quiet", commit], check=True)
        actual = subprocess.run(
            ["git", "-C", dest, "rev-parse", "HEAD"], capture_output=True, text=True, check=True
        ).stdout.strip()
    assert actual == commit, f"{name}: expected {commit}, got {actual}"
    print(f"[ok] {name}: HEAD={actual}")
PY

echo "Corpus ready under $CORPUS_DIR"
