#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "Usage: $0 <release-dir> [output-file]" >&2
  exit 1
fi

release_dir="$1"
output_file="${2:-$release_dir/checksums.txt}"

if [ ! -d "$release_dir" ]; then
  echo "Release directory not found: $release_dir" >&2
  exit 1
fi

hash_cmd=""
if command -v sha256sum >/dev/null 2>&1; then
  hash_cmd="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  hash_cmd="shasum -a 256"
else
  echo "Neither sha256sum nor shasum found. Install coreutils or use a platform tool." >&2
  exit 1
fi

: > "$output_file"

for file in "$release_dir"/*; do
  if [ -f "$file" ]; then
    base_name="$(basename "$file")"
    if [ "$base_name" = "checksums.txt" ]; then
      continue
    fi
    hash_value=$($hash_cmd "$file" | awk '{print $1}')
    printf "%s  %s\n" "$hash_value" "$base_name" >> "$output_file"
  fi
done

echo "Wrote checksums to $output_file"
