# L1 freshness: ccm ccm-cli 0.3.13

- Pre-registration: `benchmarks/freshness/PREREGISTRATION.md`, harness commit `4fbc8d5d16d992b6a6ce66f470797395147c2534`
- Artifacts: `ccm-cli` sha256 `554dcdf9fcafdfc3e4bab3f30836d87462c6ecbb8712dcf0233753ccb3803373`, `ccm-mcp` sha256 `74502d0a7d92efea2765a56936778304533a532b78cb7f2945c1f00ed08c291d`
- Environment: macOS-26.6.2-arm64-arm-64bit-Mach-O, Apple M4, 10 logical CPUs, 16 GiB RAM, Python 3.14.3
- Settings: probes at t = 0, 0.25, 0.5, 1, 2, 5, 30 s after the edit; settle 1 s; ready poll 0.1 s, timeout 120 s; request timeout 30 s; 3 repetitions
- Runs: 48, 2026-10-01T12:56:17+00:00 to 2026-10-01T13:25:14+00:00, complete

Legend: C correct · SS stale, silent · SL stale, labeled in content text · SSO stale, label only in structuredContent · E error, empty or partial update · PL / PS last good state preserved, labeled / silent · L last good state lost.

## H1

Not measurable from this file: only one system was run (competitor runs need approval gate G1).

Input for H1, ccm silent-stale rate on S1 (all edits, repositories and probe times): 6/126 (5%).

## Per scenario

| Scenario | Repo | Phase | Runs | Baseline met | Silent stale | Labeled stale | Error/empty | Time to correct: median (min–max) |
|---|---|---|---|---|---|---|---|---|
| S1-add | flask | edit | 3 | 3/3 | 3/21 (14%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–0.25 s) |
| S1-remove | flask | edit | 3 | 3/3 | 3/21 (14%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–0.25 s) |
| S1-rename | flask | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 3/21 (14%) | 0.25 s (0.25 s–0.25 s) |
| S1-add | django | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 0/21 (0%) | 0 s (0 s–0 s) |
| S1-remove | django | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 0/21 (0%) | 0 s (0 s–0 s) |
| S1-rename | django | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 1/21 (5%) | 0 s (0 s–0.25 s) |
| S2 | generated | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 0/21 (0%) | 0 s (0 s–0 s) |
| S3-branch | generated | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 0/21 (0%) | 0 s (0 s–0 s) |
| S3-bulk | generated | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 0/21 (0%) | 0 s (0 s–0 s) |
| S4-nested | generated | edit | 3 | 3/3 | 3/21 (14%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–0.25 s) |
| S4-nogit | generated | edit | 3 | 3/3 | 3/21 (14%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–0.25 s) |
| S5 | generated | edit | 3 | 0/3 | 0/21 (0%) | 0/21 (0%) | 21/21 (100%) | never (never–never) |
| S6 | generated | edit | 3 | 3/3 | 0/21 (0%) | 0/21 (0%) | 0/21 (0%) | 0 s (0 s–0 s) |
| S7 | generated | broken | 3 | 3/3 | – | – | – | – (preservation phase: see class counts) |
| S7 | generated | fixed | 3 | 3/3 | 5/21 (24%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–1 s) |
| S8-delete | generated | edit | 3 | 3/3 | 3/21 (14%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–0.25 s) |
| S8-move | generated | edit | 3 | 3/3 | 3/21 (14%) | 0/21 (0%) | 0/21 (0%) | 0.25 s (0.25 s–0.25 s) |

## Class counts by probe time (all repetitions)

| Scenario | Repo | Phase | 0 s | 0.25 s | 0.5 s | 1 s | 2 s | 5 s | 30 s |
|---|---|---|---|---|---|---|---|---|---|
| S1-add | flask | edit | SS 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S1-remove | flask | edit | SS 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S1-rename | flask | edit | E 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S1-add | django | edit | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S1-remove | django | edit | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S1-rename | django | edit | C 2, E 1, partial 1 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S2 | generated | edit | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S3-branch | generated | edit | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S3-bulk | generated | edit | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S4-nested | generated | edit | SS 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S4-nogit | generated | edit | SS 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S5 | generated | edit | E 3 | E 3 | E 3 | E 3 | E 3 | E 3 | E 3 |
| S6 | generated | edit | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S7 | generated | broken | PS 3 | PS 3 | PS 3 | PS 3 | PS 3 | PS 3 | PS 3 |
| S7 | generated | fixed | SS 3 | C 2, SS 1 | C 2, SS 1 | C 3 | C 3 | C 3 | C 3 |
| S8-delete | generated | edit | SS 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |
| S8-move | generated | edit | SS 3 | C 3 | C 3 | C 3 | C 3 | C 3 | C 3 |

## S1 by probe time (real repositories)

| t | Repo | Probes | Correct | Silent stale | Labeled stale | Error/empty |
|---|---|---|---|---|---|---|
| 0 s | flask | 9 | 0/9 (0%) | 6/9 (67%) | 0/9 (0%) | 3/9 (33%) |
| 0 s | django | 9 | 8/9 (89%) | 0/9 (0%) | 0/9 (0%) | 1/9 (11%) |
| 0.25 s | flask | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 0.25 s | django | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 0.5 s | flask | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 0.5 s | django | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 1 s | flask | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 1 s | django | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 2 s | flask | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 2 s | django | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 5 s | flask | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 5 s | django | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 30 s | flask | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |
| 30 s | django | 9 | 9/9 (100%) | 0/9 (0%) | 0/9 (0%) | 0/9 (0%) |

## S6: callers reflected per probe

- repetition 1: 50 → 50 → 50 → 50 → 50 → 50 → 50; ever decreased: no
- repetition 2: 50 → 50 → 50 → 50 → 50 → 50 → 50; ever decreased: no
- repetition 3: 50 → 50 → 50 → 50 → 50 → 50 → 50; ever decreased: no

## S9: where staleness labels appear

| Channel | All probes | Stale probes |
|---|---|---|
| content_text | 21/357 (6%) | 0/23 (0%) |
| structured_only | 0/357 (0%) | 0/23 (0%) |
| none | 336/357 (94%) | 23/23 (100%) |

Status lines seen, most frequent first:

- `fresh · auto-refresh on`: 336
- `auto-refresh off · indexed 0s ago`: 7
- `auto-refresh off · indexed 1s ago`: 5
- `auto-refresh off · indexed 2s ago`: 3
- `auto-refresh off · indexed 5s ago`: 3
- `auto-refresh off · indexed 30s ago`: 3

## Index, ready and query time (descriptive, not a pre-registered metric)

| Repo | Runs | Index build: median (min–max) | Ready after session start | One probe (all its tool calls) |
|---|---|---|---|---|
| flask | 9 | 0.151 s (0.0961 s–0.19 s) | 0.353 s (0.347 s–0.358 s) | 0.0109 s (0.0063 s–0.349 s) |
| django | 9 | 1.94 s (1.56 s–2.34 s) | 1.23 s (1.22 s–1.37 s) | 0.186 s (0.173 s–0.843 s) |
| generated | 30 | 0.0578 s (0.0443 s–0.113 s) | 0.328 s (0.019 s–0.335 s) | 0.0031 s (0.0008 s–0.333 s) |

## Runs with setup problems

- S5 / generated / r1: baseline not met (node=occupant_fn lines=16-16)
- S5 / generated / r2: baseline not met (node=occupant_fn lines=16-16)
- S5 / generated / r3: baseline not met (node=occupant_fn lines=16-16)
