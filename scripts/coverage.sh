#!/usr/bin/env bash
# CCM satir kapsama (line coverage) olcumu.
#
# Gereksinimler:
#   cargo install cargo-llvm-cov --locked
#   rustup component add llvm-tools-preview
#
# learn_pipeline entegrasyon testi (~220s) sentetik corpus'u kendisi urettigi
# icin coverage'a ihmal edilebilir katki yapar; --skip ile haric tutulur.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo llvm-cov --workspace --lcov --output-path lcov.info \
  -- --skip synthetic_corpus_and_pipeline_are_deterministic_and_gate_works

cargo llvm-cov report --fail-under-lines 55
