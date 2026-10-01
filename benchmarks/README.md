# CCM External Benchmark

Honest, reproducible measurement of CCM retrieval quality on **real, external
open-source repositories** — not the project's own synthetic gate.

## What this measures

Three real repositories, pinned to exact commits, indexed with **real
embeddings**: the recorded baseline uses Ollama `mxbai-embed-large`; the
[built-in local model](#results-built-in-local-model-2026-09-30) is measured on
the same tasks. Each repo has a hand-curated set of golden tasks (verified
against the actual source) covering CCM's three retrieval modes:

| Task type | Question being asked | Retrieval signal |
|---|---|---|
| `search_code` | "where is X implemented?" (natural-language query → file) | semantic-only vs semantic+graph |
| `read_graph` | "what does this function call / who calls it?" (node → neighbors) | graph only |
| `get_context` | "what is at this cursor position?" (file+line → node) | graph only |

`search_code` is evaluated **twice**: once with pure vector search
(`--compare` "structural" mode) and once with the hybrid graph+semantic scorer.
The other two task types exercise the graph directly.

## Corpus

| Repo | Ref | Commit | Language | LOC (core) | Nodes |
|---|---|---|---|---|---|
| serde | v1.0.219 | `49d098de` | Rust | ~40k | 4644 |
| flask | 3.0.3 | `c12a5d87` | Python | ~6k | 4212 |
| express | 4.19.2 | `04bc6278` | JavaScript | ~4k | 2420 |

Corpus is cloned by `scripts/fetch_corpus.sh` (gitignored); indexes live inside
each clone under `data/` (gitignored). Tasks reference the pinned commit.

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
benchmarks/scripts/fetch_corpus.sh

# 2. Index each repo. Built-in local model by default (~1-2 min for all three);
#    for the recorded mxbai baseline export EMBEDDING_PROVIDER=ollama first
#    (needs Ollama with mxbai-embed-large; 3-6 min/repo)
target/release/ccm-cli index --path benchmarks/corpus/flask
target/release/ccm-cli index --path benchmarks/corpus/express
target/release/ccm-cli index --path benchmarks/corpus/serde

# 3. Evaluate structural vs hybrid per repo (~2 min); CCM_BENCH_RESULTS keeps
#    runs of different embedders apart
CCM_BENCH_RESULTS=benchmarks/results/local-granite-97m-int8-bs1 benchmarks/scripts/run_benchmark.sh

# 4. Aggregate into the summary table (directory argument optional)
python3 benchmarks/scripts/aggregate.py benchmarks/results/local-granite-97m-int8-bs1
```

Reports land in `benchmarks/results/<repo>.compare.json` (mxbai baseline) or the
chosen results directory and are committed as evidence. Repo clones and indexes
are gitignored. Switching embedders re-embeds an existing index once (the index
manifest records the embedding model).

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
