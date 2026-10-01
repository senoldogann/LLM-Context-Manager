# L1 freshness: ccm ccm-cli 0.3.13

- Pre-registration: `benchmarks/freshness/PREREGISTRATION.md`, harness commit `ecf757665cda16406d5d919f7ff0ac704f14349c`
- Artifacts: `ccm-cli` sha256 `554dcdf9fcafdfc3e4bab3f30836d87462c6ecbb8712dcf0233753ccb3803373`, `ccm-mcp` sha256 `74502d0a7d92efea2765a56936778304533a532b78cb7f2945c1f00ed08c291d`
- Environment: macOS-26.6.2-arm64-arm-64bit-Mach-O, Apple M4, 10 logical CPUs, 16 GiB RAM, Python 3.14.3
- Settings: probes at t = 0, 0.25, 0.5, 1, 2, 5, 30 s after the edit; settle 1 s; ready poll 0.1 s, timeout 120 s; request timeout 30 s; 3 repetitions
- Runs: 3, 2026-10-01T13:39:30+00:00 to 2026-10-01T13:41:04+00:00, complete

Legend: C correct · SS stale, silent · SL stale, labeled in content text · SSO stale, label only in structuredContent · E error, empty or partial update · PL / PS last good state preserved, labeled / silent · L last good state lost.

## H1

Not measurable from this file: only one system was run (competitor runs need approval gate G1).

Input for H1, ccm silent-stale rate on S1 (all edits, repositories and probe times): n/a.

## Per scenario

| Scenario | Repo | Phase | Runs | Baseline met | Silent stale | Labeled stale | Error/empty | Time to correct: median (min–max) |
|---|---|---|---|---|---|---|---|---|
| S5 | generated | edit | 3 | 3/3 | 3/21 (14%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–0.25 s) |

## Class counts by probe time (all repetitions)

| Scenario | Repo | Phase | 0 s | 0.25 s | 0.5 s | 1 s | 2 s | 5 s | 30 s |
|---|---|---|---|---|---|---|---|---|---|
| S5 | generated | edit | SS 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |

## S1 by probe time (real repositories)

| t | Repo | Probes | Correct | Silent stale | Labeled stale | Error/empty |
|---|---|---|---|---|---|---|

## S6: callers reflected per probe

No S6 runs in this file.

## S9: where staleness labels appear

| Channel | All probes | Stale probes |
|---|---|---|
| content_text | 0/21 (0%) | 0/3 (0%) |
| structured_only | 0/21 (0%) | 0/3 (0%) |
| none | 21/21 (100%) | 3/3 (100%) |

Status lines seen, most frequent first:

- `fresh · auto-refresh on`: 21

## Index, ready and query time (descriptive, not a pre-registered metric)

| Repo | Runs | Index build: median (min–max) | Ready after session start | One probe (all its tool calls) |
|---|---|---|---|---|
| generated | 3 | 0.125 s (0.0603 s–0.126 s) | 0.326 s (0.325 s–0.328 s) | 0.0023 s (0.0016 s–0.0672 s) |

## Runs with setup problems

None.
