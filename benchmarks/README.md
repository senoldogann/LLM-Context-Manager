# CCM External Benchmark

Honest, reproducible measurement of CCM retrieval quality on **real, external
open-source repositories** — not the project's own synthetic gate.

## What this measures

Three real repositories (serde, Flask, Express; Django is used only by the
Level 1 freshness benchmark below), pinned to exact commits, indexed with
**real embeddings**: the recorded baseline uses Ollama `mxbai-embed-large`; the
[built-in local model](#results-built-in-local-model-2026-09-30) is measured on
the same tasks. Each repo has a hand-curated set of golden tasks (verified
against the actual source) covering CCM's three retrieval modes:

| Task type | Question being asked | Retrieval signal |
|---|---|---|
| `search_code` | "where is X implemented?" (natural-language query → file) | semantic-only vs semantic+graph |
| `read_graph` | "what does this function call / who calls it?" (node → neighbors) | graph only |
| `get_context` | "what is at this cursor position?" (file+line → node) | graph only |

`read_graph` and `get_context` are evaluation query types; since 0.4.0 agents
reach the same graph data through the `explain` tool.

`search_code` is evaluated **twice**: once with pure vector search
(`--compare` "structural" mode) and once with the hybrid graph+semantic scorer.
The other two task types exercise the graph directly.

## Corpus

| Repo | Ref | Commit | Language | LOC (core) | Nodes |
|---|---|---|---|---|---|
| serde | v1.0.219 | `49d098de` | Rust | ~40k | 4644 |
| flask | 3.0.3 | `c12a5d87` | Python | ~6k | 4212 |
| express | 4.19.2 | `04bc6278` | JavaScript | ~4k | 2420 |
| django | 5.1 | `373cb303` | Python | — | — |

Corpus is cloned by `scripts/fetch_corpus.sh` (gitignored); indexes live inside
each clone under `data/` (gitignored). Tasks reference the pinned commit.
Django is used only by Level 1, so no LOC or node count was recorded for it.

## Level 1: freshness after an edit

Pre-registered in [`freshness/PREREGISTRATION.md`](freshness/PREREGISTRATION.md)
on 2026-10-01, before any measured run; post-run changes are recorded there
under *Deviations*, never edited in place. The harness opens a system's MCP
server over stdio with an empty `HOME`, indexes a pinned repository, applies one
scripted edit, and asks system-neutral questions (`callers`, `exists`,
`node_at`) at t = 0, 0.25, 0.5, 1, 2, 5 and 30 s after the edit's last write
returns, with 3 repetitions. Each probe is classified by the pre-registered
oracle: `CORRECT`, `STALE_SILENT`, `STALE_LABELED`, `STALE_STRUCTURED_ONLY` or
`ERROR_EMPTY` (an error, a timeout, a missing target or a partial update).
Scenarios S2–S8 use small purpose-built repositories; only S1 uses real ones.

**H1 (pre-registered).** On S1 (real repositories, all edits, all probe times),
CCM's silent-stale rate must be at least 5 percentage points lower than the best
competitor's; or within 5 points with CCM the only system whose label reaches
the model-visible `content` text. **H1 is not measurable from this run:** no
competitor has been run (approval gate G1), so only CCM's own numbers are
published.

### Results (2026-10-01, CCM 0.3.13 = `136b106`, macOS arm64)

Raw files: full run
[`results/freshness/ccm-0.3.13-136b106.json`](results/freshness/ccm-0.3.13-136b106.json)
(harness `4fbc8d5`), S5 re-run
[`results/freshness/ccm-0.3.13-136b106-s5-rerun.json`](results/freshness/ccm-0.3.13-136b106-s5-rerun.json)
(harness `ecf7576`, which contains the `a38ce16` parser fix). The first run's S5
block parsing was broken (deviation 1), so the summary takes S5 from the re-run.

**S1 by probe time** (3 repetitions, 9 probes per repo per time):

| t | Repo | Probes | Correct | Silent stale | Labeled stale | Error/empty |
|---|---|---|---|---|---|---|
| 0 s | flask | 9 | 0 | 6 | 0 | 3 |
| 0 s | django | 9 | 8 | 0 | 0 | 1 (partial) |
| 0.25–30 s | flask | 54 | 54 | 0 | 0 | 0 |
| 0.25–30 s | django | 54 | 54 | 0 | 0 | 0 |

The S1 silent-stale rate (H1 input): **6/126 (5%)**. Every stale answer was
Flask at t = 0; the status line was `fresh · auto-refresh on` and no label
reached `content` text.

**Time to the correct answer** (the first probe time after which every later
probe is `CORRECT`; median over 3 repetitions):

| Scenario | Repo | Time to correct | Correct at t = 0 |
|---|---|---|---|
| S1-add | flask | 0.25 s (0.25–0.25) | 0/3 |
| S1-remove | flask | 0.25 s (0.25–0.25) | 0/3 |
| S1-rename | flask | 0.25 s (0.25–0.25) | 0/3 (3 errors) |
| S1-add | django | 0 s | 3/3 |
| S1-remove | django | 0 s | 3/3 |
| S1-rename | django | 0 s (0–0.25) | 2/3 (1 partial error) |
| S2 (auto-refresh off, external re-index) | generated | 0 s | 3/3, labeled |
| S3-branch | generated | 0 s | 3/3 |
| S3-bulk (20 files) | generated | 0 s | 3/3 |
| S4-nested | generated | 0.25 s | 0/3 |
| S4-nogit | generated | 0.25 s | 0/3 |
| S5 (insert lines above the cursor) | generated | 0.25 s (0.25–0.25) | 0/3 |
| S6 (50 files) | generated | 0 s | 3/3 |
| S7 fixed (after a broken save) | generated | 0.25 s (0.25–1) | 0/3 |
| S8-delete | generated | 0.25 s | 0/3 |
| S8-move | generated | 0.25 s | 0/3 |

S7 phase 1 (a syntactically broken save) has no "correct" state: the last good
state was preserved in 21/21 probes, silently (`PRESERVED_SILENT`).

### What the numbers say

1. **On real repositories the stale window is short.** 6 of 126 S1 probes (5%)
   returned the pre-edit state; all six were Flask at t = 0. From t = 0.25 s on,
   all 108 S1 probes were correct. The same shape appears in S4-nested,
   S4-nogit, S5, S8-delete and S8-move; S1-add/remove/rename, S3-branch,
   S3-bulk and S6 were already correct at t = 0.
2. **None of the 23 stale probes carried a label.** Across the whole run, every
   stale answer came with the status line `fresh · auto-refresh on`; no
   staleness text reached the model-visible `content` channel. The one signal
   that does reach it is the auto-refresh-off state
   (`auto-refresh off · indexed Xs ago`), which S2 showed correctly from t = 0.
3. **A broken edit preserves the last good state, silently.** S7 phase 1:
   21/21 probes `PRESERVED_SILENT`; no `refresh failed` label appeared.
4. **S6 (50 files, one pass) reflected all 50 callers at t = 0** and the count
   never decreased during the 30 s window (50/50 at every probe in all three
   repetitions).
5. **The S1-rename failures at t = 0 are a state, not a timing artefact.** At
   t = 0 the Flask rename probes (3/3) returned the renamed symbol as absent and
   the old symbol as present but with an empty caller list — neither the
   pre-edit nor the post-edit state, hence `ERROR_EMPTY`. One Django repetition
   failed the same way (`partial`): the old symbol was gone, the renamed one was
   not found yet.
6. **Descriptive timings (not a pre-registered metric):** median index build
   0.15 s (Flask) / 1.94 s (Django); ready after session start 0.35 s / 1.23 s;
   one probe, both tool calls, 11 ms / 186 ms.

### Limits

- One machine (macOS 26.6.2, Apple M4, 16 GiB), one CCM build (0.3.13, `main`
  at `136b106`), 3 repetitions. Probes run in one session and in order, so
  earlier probes can affect later ones; per-repetition counts are in the raw
  files and no pooled significance test is claimed.
- Only CCM was measured. H1 requires at least one competitor run (gate G1);
  nothing here is a comparison with any other system.
- S2–S8 use generated repositories: they show behaviour in a known situation,
  not value on real code.
- Django's S1 probes were already correct at t = 0 in 8 of 9 cases while
  Flask's were not. Django probes take ~0.19 s against Flask's ~0.01 s, so the
  harness cannot tell whether the watcher had reacted before the question
  arrived; per-repo timing differences are descriptive, not evidence.
- Not measured here: agent outcomes, token cost (see
  [Token cost of answers](#token-cost-of-answers-m2)), edge accuracy (Level 2),
  platforms other than macOS.
- Conflict of interest: the benchmark was written for the CCM maintainer, and
  CCM is the system under test. Safeguards are listed in the pre-registration;
  every raw probe is published, including the ones that read badly for CCM.

### Reproducing

```bash
cd benchmarks

# Corpus: shallow clones of the pinned repositories in corpus.json (~15 s)
scripts/fetch_corpus.sh

# Full run. The current harness produces a valid S5 in the same run; the
# two-file split below reflects the historical parser bug (deviation 1).
PYTHONDONTWRITEBYTECODE=1 uv run --frozen python -m freshness run \
  --system ccm --ccm-bin-dir "$HOME/.cargo/bin" \
  --corpus-dir corpus --work-dir <empty-dir> --log-dir <log-dir> \
  --out results/freshness/ccm-0.3.13-136b106.json \
  --repetitions 3 --scenarios all \
  --request-timeout 30 --ready-timeout 120 --command-timeout 900
PYTHONDONTWRITEBYTECODE=1 uv run --frozen python -m freshness report \
  --results results/freshness/ccm-0.3.13-136b106.json \
  --out results/freshness/ccm-0.3.13-136b106.md

# S5 alone, as re-run for this report
PYTHONDONTWRITEBYTECODE=1 uv run --frozen python -m freshness run \
  --system ccm --ccm-bin-dir "$HOME/.cargo/bin" \
  --corpus-dir corpus --work-dir <empty-dir> --log-dir <log-dir> \
  --out results/freshness/ccm-0.3.13-136b106-s5-rerun.json \
  --repetitions 3 --scenarios S5 \
  --request-timeout 30 --ready-timeout 120 --command-timeout 900
```

The work directory must be empty; the harness refuses to run while
`benchmarks/freshness`, `benchmarks/pyproject.toml` or `benchmarks/uv.lock`
have uncommitted changes. Level 2 (CCM's call edges against language servers)
is a separate, not-yet-run benchmark.

## Token cost of answers (M2)

How many tool calls and response bytes an agent spends to get the same
structural answers from CCM before (v1 = `fd9af8b`: the 0.3.13 tools plus the
Python syntax graph) and after (v2 = `364c59a`) the token-efficient answer
work: compact one-line results with a `max_tokens` budget, `target` as a name,
file or `path:line`, `explain` and `map`. Pre-registered in
[`tokens/PREREGISTRATION.md`](tokens/PREREGISTRATION.md) before the first run;
one deviation (v2 was measured twice, see below).

Method: 12 fixed questions per repository on Flask 3.0.3 and Django 5.1 (5
callers, 5 explain, 2 impact; [`tokens/questions.json`](tokens/questions.json)),
graph only (`CCM_DISABLE_EMBEDDER=1`), auto-refresh off, and every argument not
named in the pre-registered tool plan at its default. Each version runs the
scripted plan an agent would follow with that version's tools, including one
follow-up call per listed candidate (at most 10) when a name is ambiguous.
Estimated tokens = bytes / 4; no model or tokenizer is involved.

### Results (2026-10-02, macOS arm64)

| Questions (both repos) | v1 calls | v2 calls | v1 bytes | v2 bytes | Change |
|---|---|---|---|---|---|
| callers (10) | 48 | 44 | 199,853 | 36,527 | −82% |
| explain (10) | 86 | 44 | 293,784 | 85,402 | −71% |
| impact (4) | 4 | 4 | 33,275 | 14,367 | −57% |
| **all 24** | **138** | **92** | **526,912** | **136,296** | **−74%** |
| map (2, v2 only) | — | 2 | — | 8,279 | — |

Fixed cost per session: `tools/list` 9,333 → 6,640 bytes (−29%) and `SKILL.md`
25,993 → 4,051 bytes (−84%).

Parity (honesty check): the default v2 answers list 76 of Flask's 85 and 75 of
Django's 79 caller locations that v1 returned (151 of 164). All 13 missing
callers are still in the v2 graph. Twelve fall outside the first 20 usages that
`find_usages` shows by default (v1 had the same limit); v2 orders usages by file
and line, while v1 used hash order, so the two versions show different first 20.
The thirteenth calls a test-local `render`: Django has 68 definitions named
`render`, both versions follow at most 10, and v2's path-ordered candidate list
does not include that one. The `max_tokens` budget did not cut any caller.

Deviation: the first v2 run (`9eb8ded`, before the final review) measured
139,774 bytes and 163 of 164 callers with hash order. The review fixes and a
resolver fix (no `may call` edges from `x.name()` to module-level functions)
changed the order and the edges, so the final head was measured again on
schema-7 indexes of the same corpus commits. Both runs are in
[`results/tokens/`](results/tokens/) (`baseline-fd9af8b`, `m2-9eb8ded`,
`m2-364c59a`).

### Limits

- Bytes, not tokens: bytes / 4 is an estimate, and tokenizers differ.
- Not an agent benchmark: the tool plans are scripted. Whether a model asks
  fewer questions, finishes tasks with fewer tokens or makes fewer mistakes is
  the L3 agent benchmark, which has not run yet.
- Two Python repositories and 24 questions fixed before the run. Other
  languages are matched by name and were not measured.
- Both versions include symbol bodies in `explain` answers (v1 through
  `get_context`); v2 caps a body at half of the budget.
- One run per version on one machine. Byte counts are deterministic for a given
  index.

### Reproducing

```bash
cd benchmarks
PYTHONDONTWRITEBYTECODE=1 uv run --frozen python -m tokens run --version v2 \
  --bin-dir ../target/debug --corpus-dir <corpus-dir> --questions tokens/questions.json \
  --skill ../SKILL.md --home <empty-dir> --log-dir <log-dir> \
  --out results/tokens/m2-364c59a.json
PYTHONDONTWRITEBYTECODE=1 uv run --frozen python -m tokens report \
  --results results/tokens/baseline-fd9af8b.json \
  --results results/tokens/m2-364c59a.json --out results/tokens/m2-364c59a.md
```

The corpus comes from `scripts/fetch_corpus.sh`, indexed with `ccm-cli index`
from the version under test (v1 reads schema-6 indexes, v2 schema 7); `--version
v1` with binaries built from `fd9af8b` reproduces the baseline. The harness
refuses to run while `benchmarks/tokens` has uncommitted changes.

## Rust edges (M3)

What changed in the graph when Rust moved from name matching (v0.4.0,
`e9a6920`) to syntax-level resolution (M3, `0a3b3bd`): edges whose source is a
Rust file, by relation, on this repository (180 files) and serde 1.0.219 (322
files). Both versions indexed the same source trees, graph only.

| Relation | CCM v0.4.0 | CCM M3 | serde v0.4.0 | serde M3 |
|---|---|---|---|---|
| `calls` | 5,782 | 2,184 | 2,526 | 951 |
| `may call` | 234 | 1,149 | 4,677 | 1,844 |
| `references` | 0 | 1,130 | 0 | 2,602 |
| `imports` | 3,360 | 429 | 5,648 | 260 |
| `may import` | 84 | 2 | 765 | 2 |
| `inherits` | 0 | 0 | 0 | 270 |

In v0.4.0 a call bound to the only project definition with that name and a
mention of a type counted as an import. In M3 a method call on a value of
unknown type is `may call`, a mentioned type is a reference, and `imports` come
only from `use` declarations.

Spot checks (by the implementer, reading each call site; not a benchmark):

- **M3 `calls` edges:** 30 sampled per repository (seed 7), and 30 of 30 were
  right on both. The sample covers same-module calls, `self.` methods (including
  the right `bad_type` among two impls in one serde file), `Self::` calls,
  struct and tuple-struct constructors, module paths (`bound::…`), cross-crate
  paths (`ccm_core::…`, `serde::de::…`) and test helper modules.
- **v0.4.0 `calls` pairs that M3 dropped:** the pairs are compared at function
  level; on CCM 793 of 3,218 have no M3 edge, on serde 1,105 of 1,866.
  - On CCM, two samples of 30: in the first, 29 were name-match false positives
    (`.map(` → a module named `map`, `.join(` and `.path()` → private helpers,
    `Vec::new()`, `HashSet::from` and git2's `Repository::init` → project
    functions). The other was a real loss, `let report = report(…)`, fixed in
    `0a3b3bd`. After the fix all 30 of a second sample were false positives.
  - On serde, of 30, 28 were false positives (`Ok(…)`, `Err(…)` and `Some(…)` →
    same-named test structs, `Cow::Borrowed`, quoted `_serde::…` paths). The
    other two were calls inside nested `impl` blocks, which M3 credits to the
    nested method instead of the outer function.

Limits:

- These counts describe the change, not accuracy. Recall against a type-aware
  tool is the L2 benchmark, not run yet.
- Method calls on values stay `may call` (at most five candidates); there is no
  type inference for local variables.
- Macro arguments are scanned for call shapes only. Code generated by macros or
  build scripts, `#[path]` modules and `include!` are not followed.
- Spot checks by the implementer are weaker evidence than an independent check.

## Results (2026-08-21, Ollama mxbai-embed-large)

### Pass rate by repo

| Repo | Tasks | Semantic-only | Hybrid | Δ |
|---|---|---|---|---|
| flask | 13 | 92.3% | **100.0%** | **+7.7pp** |
| express | 12 | 83.3% | 83.3% | 0 |
| serde | 10 | 60.0% | 60.0% | 0 |
| **Overall** | **35** | **80.0%** | **82.9%** | **+2.9pp** |

### By query type (hybrid)

| Query type | Flask | Express | Serde | Notes |
|---|---|---|---|---|
| `get_context` | 4/4 | 4/4 | 3/3 | Graph cursor coverage is solid |
| `read_graph` | 4/4 | 3/3 | 2/2 | Call-graph edges resolve on all 3 repos |
| `search_code` | 5/5 | 3/5 | 1/5 | **The weak point** |

### Search quality metrics (search_code only, K=5)

| Mode | Pass | R@K | MRR@K | Mean latency |
|---|---|---|---|---|
| Semantic-only | 8/15 | 0.533 | 0.352 | ~234ms |
| Hybrid | 9/15 | 0.600 | 0.436 | ~266ms |

Recall, MRR and latency here are means over the 15 `search_code` tasks only;
the per-repo tables printed by `aggregate.py` cover all query types.

## Results: built-in local model (2026-09-30)

Default embedder since the local-embedder change:
`ibm-granite/granite-embedding-97m-multilingual-r2` (IBM's int8 ONNX export,
384-d, CLS pooling, 512-token inputs) run in-process by fastembed 7.1 / ONNX
Runtime 1.28 on the CPU of an Apple M4 (10 cores: 4 performance + 6 efficiency),
10 threads (physical cores), one text per inference call (the shipped default;
the batched rows set `CCM_LOCAL_EMBED_BATCH=32`). Same corpus, same 35 tasks,
same code revision for every row; the mxbai row was re-run on this revision
through Ollama and reproduced the recorded numbers exactly.

### Search quality (search_code only, K=5)

| Embedder | Mode | Pass | R@5 | MRR@5 |
|---|---|---|---|---|
| mxbai-embed-large (Ollama, 335M, 1024-d) | semantic-only | 8/15 | 0.533 | 0.352 |
| | hybrid | 9/15 | 0.600 | 0.436 |
| **granite-97m int8, one text per call (default)** | semantic-only | **9/15** | **0.600** | 0.419 |
| | hybrid | **10/15** | **0.667** | **0.497** |
| granite-97m int8, `CCM_LOCAL_EMBED_BATCH=32` | semantic-only | 8/15 | 0.533 | **0.489** |
| | hybrid | 8/15 | 0.533 | 0.489 |

Reports: [`results/local-granite-97m-int8-bs1/`](./results/local-granite-97m-int8-bs1/)
(default) and [`results/local-granite-97m-int8-bs32/`](./results/local-granite-97m-int8-bs32/).
`get_context` and `read_graph` do not depend on the embedder and match across
rows, except `serde-graph-002`: its node id no longer exists in the graph built
by the current parser ("Node not found in graph" for every embedder, so the
serde eval exits with "9 of 10 tasks were scored"). That is a graph change since
the v0.3.13 recording, not an embedding effect.

### Indexing speed (full index, same machine, back to back)

| Repo | Chunks | mxbai via Ollama | local, one text per call (default) | local, batch 32 |
|---|---|---|---|---|
| flask | 2498 | 81.4 ms/chunk, 203.5 s | 23.4 ms/chunk, 58.9 s | 12.2 ms/chunk, 33.8 s |
| express | 173 | 67.6 ms/chunk, 11.9 s | 22.7 ms/chunk, 4.1 s | 17.1 ms/chunk, 3.1 s |
| serde | 4042 | 81.0 ms/chunk, 327.5 s | 19.0 ms/chunk, 77.1 s | 19.1 ms/chunk, 77.3 s |

"ms/chunk" covers the embedding phase (including LanceDB writes); the time is
the whole `ccm-cli index` run including parsing and the ~0.8 s model load. The
machine was shared with other builds (1-minute load average 6–15 during these
runs), so treat the timings as ±2×: in interleaved flask runs the two local batch
sizes measured 45.9/32.5, 34.8/14.7 and 14.9/14.2 ms/chunk (batch 1/batch 32) as
the load average moved between 30 and 12.

### What the local-model numbers say

1. **Not worse than the mxbai baseline, 3–4× faster.** With the default (one
   text per call) semantic-only and hybrid search each pass one more task than
   mxbai, with higher MRR (the first relevant hit ranks earlier). Differences
   of one or two tasks out of 15 are within noise for this pilot.
2. **Batching changes int8 vectors, so the default is one text per call.**
   IBM's int8 file quantizes activations dynamically over the whole batch
   tensor, so a text's vector depends on its batch-mates (cosine 0.95–0.97 to
   the same text embedded alone; the fp32 file is batch-invariant). One text
   per inference call keeps vectors a pure function of the text (what chunk
   reuse and the live index assume) and scored best here. `CCM_LOCAL_EMBED_BATCH=32`
   was up to 2× faster on flask and no faster on serde in these runs, at the
   cost of that determinism (and, here, of recall).
3. **int8 vs fp32:** single-text cosine to IBM's fp32 export is 0.946–0.977 on
   code snippets; the fp32 export reproduces the model card's similarity matrix
   to 0.002, the int8 one to 0.08 with the same ranking.

## Results: OpenAI embeddings (2026-10-01)

`text-embedding-3-small` (1536-d) and `text-embedding-3-large` (3072-d) through
the official API (`EMBEDDING_PROVIDER=openai`, 32 texts per request); same
corpus, same 35 tasks, same code revision as the built-in model rows. Vector
search is exact (no ANN index) and OpenAI vectors have unit length, so the
ranking equals cosine similarity for every model.

### Search quality (search_code only, K=5)

| Embedder | Mode | Pass | R@5 | MRR@5 |
|---|---|---|---|---|
| **text-embedding-3-small** (default when OpenAI is selected) | semantic-only | **9/15** | **0.600** | **0.439** |
| | hybrid | **10/15** | **0.667** | 0.452 |
| text-embedding-3-large | semantic-only | 6/15 | 0.400 | 0.322 |
| | hybrid | 9/15 | 0.600 | 0.428 |
| granite-97m int8, built-in (from above) | semantic-only | 9/15 | 0.600 | 0.419 |
| | hybrid | 10/15 | 0.667 | 0.497 |

Reports: [`results/openai-text-embedding-3-small/`](./results/openai-text-embedding-3-small/)
and [`results/openai-text-embedding-3-large/`](./results/openai-text-embedding-3-large/).

1. **`-large` did not beat `-small` here.** It lost three `search_code` tasks
   in semantic-only mode (flask 2/5 vs 4/5, serde 1/5 vs 2/5) and one in hybrid
   mode. With 15 tasks that is within noise, but nothing here justifies ~6.5×
   the price per token and vectors twice the size, so `-small` is the default
   when OpenAI is selected.
2. **OpenAI does not beat the built-in model on this benchmark.** `-small`
   matches granite on pass rate and R@5; granite keeps the higher hybrid MRR
   and needs no network. These English queries do not measure multilingual
   retrieval, where OpenAI reports its larger gains.
3. **Every query is an API round trip:** 0.5–0.8 s mean per `search_code`
   query in these runs, against 25–165 ms for the built-in model. Indexing the
   three repos (6,713 chunks) took about 1.7 min with `-small` and 2.6 min
   with `-large`. The machine was also compiling during the OpenAI runs, so
   treat both timings as indicative.

## What the numbers actually say

1. **Graph coverage is the strong suit.** Every `get_context` and `read_graph`
   task passed on all three repos. The call-graph edges the parser builds —
   including cross-file calls — are real enough to navigate with.

2. **Hybrid beats semantic-only on retrieval, but modestly.** +1 task, +6.7pp
   recall, +8.4pp MRR on 15 search queries, at ~32ms extra mean latency. That is a real but small effect on
   this corpus. It would be dishonest to claim more from 15 queries.

3. **The concrete hybrid win is instructive.** `flask-search-003` ("how does
   Flask create a test client?") failed with semantic-only — all top-5 hits were
   `app.py` (where `test_client()` lives). The hybrid scorer graph-expanded from
   `app.py` to `testing.py` (where `FlaskClient` is defined) via the usage edge
   and recovered it at rank 3. This is the mechanism the project claims — here
   it is demonstrated on real code.

4. **JavaScript parsing loses functions.** Express's `lib/` produced only 37
   Function nodes out of 403 (the rest are `Variable` nodes); prototype-assigned
   functions like `app.handle` and `proto.handle` did not become graph nodes.
   `get_context`/`read_graph` still passed because the surrounding nodes and
   edges suffice, but this is a coverage gap worth fixing.

5. **Serde search is the weakest area.** `serde/src/ser/mod.rs` (the
   `Serializer` trait) was not recovered for "serialize a struct with named
   fields" — the top hits were `serde_derive/src/ser.rs`, which is *semantically
   close* (it generates the impls) but not the trait definition. The index also
   includes serde's huge `test_suite/` directory, which adds noise. Two
   follow-ups suggest themselves: (a) weight core source dirs over tests,
   (b) evaluate trait-heavy Rust separately from the derive side.

## Failure ledger (hybrid, all 6 failures)

| Task | Ground truth | What ranked instead | Interpretation |
|---|---|---|---|
| express-search-004 | `lib/application.js` (lazyrouter) | router/index.js, express.js | "create the router" matched router files better |
| express-search-005 | `lib/middleware/query.js` | router/index.js, utils.js | Tiny 47-line middleware outranked by bigger files |
| serde-search-001 | `serde/src/de/mod.rs` (Deserializer trait) | serde_derive/src/de.rs | Derive-side code semantically similar |
| serde-search-003 | `serde/src/ser/impls.rs` | serde_derive/src/ser.rs, private/ser.rs | Same pattern |
| serde-search-004 | `serde/src/ser/mod.rs` | serde_derive/src/ser.rs | Derive impls genuinely closer to the query |
| serde-search-005 | `serde/src/de/impls.rs` | private/de.rs, serde_derive/de.rs | Same pattern |

The serde failures are not index corruption — they are a real, repeatable
retrieval bias toward the derive side and toward large files.

## Reproducing

```bash
# 1. Clone corpus at pinned commits (~15s)
bash benchmarks/scripts/fetch_corpus.sh

# 2. Index each repo. Built-in local model by default (~1-2 min for all three);
#    for the recorded mxbai baseline export EMBEDDING_PROVIDER=ollama first
#    (needs Ollama with mxbai-embed-large; 3-6 min/repo); for the OpenAI rows
#    export EMBEDDING_PROVIDER=openai and EMBEDDING_MODEL=text-embedding-3-small
#    (or -large), with OPENAI_API_KEY in ~/.ccm/.env
target/release/ccm-cli index --path benchmarks/corpus/flask
target/release/ccm-cli index --path benchmarks/corpus/express
target/release/ccm-cli index --path benchmarks/corpus/serde

# 3. Evaluate structural vs hybrid per repo (~2 min). Give every embedder its
#    own report directory via CCM_BENCH_RESULTS so runs are never mixed; without
#    it the script writes the mxbai baseline into benchmarks/results. The script
#    exits non-zero after serde ("9 of 10 tasks were scored", see above); all
#    three reports are written.
CCM_BENCH_RESULTS=benchmarks/results/local-granite-97m-int8-bs1 bash benchmarks/scripts/run_benchmark.sh

# 4. Aggregate into the summary table (directory argument optional)
python3 benchmarks/scripts/aggregate.py benchmarks/results/local-granite-97m-int8-bs1
```

Reports land in `benchmarks/results/<repo>.compare.json` for the mxbai baseline,
or in `$CCM_BENCH_RESULTS` when set; keep one directory per embedder (for
example `benchmarks/results/local-granite-97m-int8-bs1/`) and commit each as
evidence. Repo clones and indexes are gitignored. Switching embedders re-embeds
an existing index once (the index manifest records the embedding model).

## Honest caveats

- 35 tasks across 3 repos is a pilot, not a conclusive benchmark. The effect
  sizes here (especially hybrid's +2.9pp) have wide confidence intervals.
- Ground truth is hand-curated by reading the source; where multiple files are
  defensibly "the answer" (e.g. trait definition vs derive implementation) we
  chose the core definition and noted it in the ledger.
- The synthetic 180/180 CI gate and this benchmark measure **different things**:
  the gate is a regression harness (deterministic, fixture embeddings); this
  benchmark is external evidence (real repos, real embeddings). Neither should
  be quoted as the other.
