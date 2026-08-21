# CCM External Benchmark

Honest, reproducible measurement of CCM retrieval quality on **real, external
open-source repositories** — not the project's own synthetic gate.

## What this measures

Three real repositories, pinned to exact commits, indexed with **real Ollama
embeddings** (`mxbai-embed-large`). Each repo has a hand-curated set of golden
tasks (verified against the actual source) covering CCM's three retrieval modes:

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
| Semantic-only | 8/15 | 0.759 | 0.709 | ~110ms |
| Hybrid | 9/15 | 0.784 | 0.744 | ~125ms |

## What the numbers actually say

1. **Graph coverage is the strong suit.** Every `get_context` and `read_graph`
   task passed on all three repos. The call-graph edges the parser builds —
   including cross-file calls — are real enough to navigate with.

2. **Hybrid beats semantic-only on retrieval, but modestly.** +1 task, +2.5pp
   recall, +3.5pp MRR on 15 search queries. That is a real but small effect on
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

# 2. Index each repo (needs Ollama with mxbai-embed-large; 5-7 min/repo)
target/release/ccm-cli index --path benchmarks/corpus/flask
target/release/ccm-cli index --path benchmarks/corpus/express
target/release/ccm-cli index --path benchmarks/corpus/serde

# 3. Evaluate structural vs hybrid per repo (~2 min)
benchmarks/scripts/run_benchmark.sh

# 4. Aggregate into the summary table
python3 benchmarks/scripts/aggregate.py
```

Reports land in `benchmarks/results/<repo>.compare.json` and are committed as
evidence. Repo clones and indexes are gitignored.

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
