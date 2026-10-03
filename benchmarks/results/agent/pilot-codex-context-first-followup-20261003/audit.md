# Matched Codex context-first follow-up audit

Measured on 2026-10-03 at clean commit `f649a4017bf6f06fc7d3689bee7a0ca81fc947fd`.
After the local packet fixes, the owner instructed continuation of the proposed bounded next
step: dedicated Codex, three tasks × three arms × one repetition. Exactly those nine runs were
executed, sequentially. No additional repetition, Claude run or final L3 experiment was started.

The generated [report](report.md), [settings](settings.json), nine run records and three Serde
diffs accompany this audit. [comparison.json](comparison.json) contains both pilots' per-run
token/tool/time values. [transcript-audit.json](transcript-audit.json) preserves raw-transcript
hashes, consistency checks, recovered errors and the selected actual CCM packets. Full JSONL
transcripts remain local and git-ignored.

## Matched configuration and isolation

- Codex CLI 0.160.0, `gpt-6.1-sol`, effort `high`, dedicated ChatGPT subscription. These settings,
  the task set, profile, prompts, scorer, harness and preregistration are unchanged from the
  [previous pilot](../pilot-codex-context-first-20261003/audit.md), measured at `4185640`.
- `context-first-pilot`: A=built-ins, B=default `context + index_project` and context note,
  C=legacy discovery and navigation note. The final preregistered experiment remains separate.
- Fresh temporary `git archive` workspaces and HOME per run. CCM artifacts stayed under the
  workspace's `.git/ccm-bench`; MCP retained `CCM_ALLOWED_ROOTS` confinement. Serde edits affected
  only the copies. The original source repos remained clean and at their corpus pins.
- Authentication came through the existing dedicated Codex home, used by Codex itself. The
  supervising agent did not inspect, copy or persist credential files or supply a Claude token.
- Local embeddings used the already cached pinned model. All five files were checksum-verified
  before starting; no model download or embedding API call was needed. `UV_OFFLINE=1` prevented
  Python dependency acquisition. The nine authorized Codex runs used external subscription quota.
- Raw usage, final text and tool counts match the saved records for all nine runs using the
  existing parser. Tool-grant checks passed; completed events contained only agent messages,
  local command execution, file changes and the granted CCM tools. No web, app, other MCP server,
  browser or spawned-agent item appeared. Manual inspection found source reads/edits and
  ancestor/temporary `AGENTS.md` checks with no returned external instruction text; those discovery
  commands are retained in tool and token counts.

The verified optimized pair from the local follow-up was selected explicitly and was not rebuilt:

```text
ccm-cli 99446be7c123f91dc6b7653bb80c3f90a4f488e2fc36942f1f2622e7a40cdc8e
ccm-mcp 610153e60d7418f4a70efba208a9d1d2a8ce5758f8a7386100a27a5490948b9f
```

These hashes match the pair that passed the post-build canonical and actual-query packet checks
at `bcff2e2`. Rust product code did not change during this measurement. The full core/MCP/CLI,
clippy, fmt and retrieval gates recorded in the [local follow-up](../../context-packet-followup-20261003/report.md)
remain applicable to that code; they were not repeated for this results-only step.

Command, from `benchmarks/`, in a newly created output directory:

```bash
UV_OFFLINE=1 uv run python -m agent run --agent codex \
  --agent-bin /Users/dogan/.local/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex \
  --codex-home /Users/dogan/.ccm-bench/codex-home --auth subscription \
  --profile context-first-pilot --model gpt-6.1-sol --effort high --embedding local \
  --max-budget-usd 2 --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --ccm-bin-dir ../target/release --ccm-model-dir /Users/dogan/.ccm/models \
  --corpus-dir corpus --out-dir results/agent/pilot-codex-context-first-followup-20261003
```

The dollar flags are recorded settings, not effective Codex quota limits. Codex reports no actual
dollar cost or turn count; console `spent $0.00` is unknown cost, not zero quota use. The report's
dollar estimates use the unchanged committed API price table; they are not subscription charges.

## Final answers, tokens and tool use

Harness exit **0**; **9/9 final answers** have recall=precision=1.000, no missing/extra/stale final
item and no run failure. All three edits were applied; their saved patches are byte-identical
(SHA-256 `94bda2cf8622837362dae1b481df44f917c48067ce7087108fe8fcf791202ce5`). Harness marker and
diff checks validated the edits; this pilot did not compile the corpus Rust package.

Input includes cached input exactly once. Input plus output uses the existing parser's reported
fields; no usage or scoring calculation was changed.

| Task | A input + output | B input + output | C input + output | B versus A | A shell | B shell / CCM | C shell / CCM |
|---|---:|---:|---:|---:|---:|---:|---:|
| Flask | 48,828 | 63,320 | 63,405 | +29.68% | 5 | 3 / 1 | 2 / 6 |
| Express | 61,494 | 65,838 | 68,326 | +7.06% | 4 | 3 / 1 | 4 / 9 |
| Serde | 47,321 | 64,315 | 95,211 | +35.91% | 3 | 4 / 1 | 5 / 9 |
| **Sum** | **157,643** | **193,473** | **226,942** | **+22.73%** | **12** | **10 / 3** | **11 / 24** |

Each Serde arm additionally made one file-change call. B used `context` once per task and no
`index_project`; C used legacy tools in all tasks. Agent wall sums were A=90.6 s, B=85.2 s,
C=112.8 s, excluding pre-index sums B=76.6 s and C=76.7 s. A had no pre-index. Reusable indexes
were not tested, so agent wall alone is not end-to-end setup time.

| Measurement | A input + output | B input + output | C input + output | B versus A |
|---|---:|---:|---:|---:|
| Previous pilot, `4185640` | 163,902 | 212,498 | 216,274 | +29.65% |
| Follow-up, `f649a40` | 157,643 | 193,473 | 226,942 | +22.73% |

B's absolute total decreased 8.95% between attempts; A also decreased 3.82%, and C increased
4.93%. These changes cannot all be attributed to the product fix: each attempt has only one
repetition and the agent's queries, discovery/retry decisions and cache use differ. In the new
three-task sample B still uses more total tokens than A in every task. The report's paired input
ratio B/A is 1.31 (bootstrap interval 1.07–1.36), C/A 1.31 (1.11–2.01), over three tasks only.
These pilots do not establish general token savings, quality equivalence or model independence.

## Actual packets and retained failures

### Flask and Express: the local fixes appear in real agent queries

Flask B requested direct callers and outermost enclosing names without backtick hints. Its packet
contains 12/12 semantic callers, class-qualified scopes and the enclosing names for `wrapper` and
`view`; bounded `may call` labels remain. Nevertheless it still read sources and ran an AST
traversal. Shell calls decreased from four in the previous B run to three here, not to zero.

Express B again sent a plain query mentioning `req.get`, `req.header`, `app.get`, `res.get` and
`lib/request.js`. This packet has 5/5 callers, including `lib/response.js:location`, with no extra
caller. The previous plain query's packet had recall 4/5 and precision 4/7. B still made three
shell calls, including a broad grep and reads to verify the result. Both B queries chose
`max_tokens=6000`; the separate local preflight's budget was 1500.

### Serde B: post-edit CCM use is still absent

B's single `context` request was before its edit. It combined locating `ser::build_generics`,
finding the file end and identifying `bound::with_bound` callers. CCM selected the build-generics
definition in the supplied file. B then read sources, edited and verified with shell commands;
it made no post-edit CCM call. Its correct final answer does not validate CCM freshness.

### Serde C: stale post-edit packet incorrectly labelled fresh

This is a **packet failure despite a correct final answer**. The transcript establishes this
sequence (one-based JSONL line numbers, preserved in `transcript-audit.json`):

1. Line 29: the file-change item completed, replacing the old direct call and appending
   `with_serialize_bound` in `serde_derive/src/ser.rs`.
2. Line 31: a successful shell command showed the actual diff and confirmed the new direct
   `bound::with_bound` call at line 1374. `git diff --check` passed.
3. Line 32: `find_usages({target: "with_bound", ...})` started **after** that edit and source check.
4. Line 33: CCM returned `_Index: fresh · auto-refresh on_`, but listed the old
   `ser::build_generics` at lines 132–150 and omitted `ser::with_serialize_bound`.

The post-edit packet has **recall=precision=0.500**: one correct caller (`de::build_generics`),
one stale extra (`ser::build_generics`), one missing caller (`ser::with_serialize_bound`). The
agent's final answer used the changed source and was correct, so the unchanged final-answer
scorer reports no stale final item. The generated report's `Stale=0` must not be interpreted as
zero stale CCM responses.

This differs from the canonical preflight, which edits a copy **before indexing** and passes 2/2.
The earlier full local freshness tests also pass, but this real immediate post-edit path exposes
a gap. The cause is not established: watcher delivery, refresh admission and sandbox behavior
need a deterministic local reproduction before a general fix. No timing workaround or framework
rule was introduced during this run set.

### Recovered errors remain in the measurement

- Flask A first tried unavailable `python` (exit 127), then succeeded with `python3`.
- Three `AGENTS.md` discovery commands returned no match (exit 1): Flask B, Serde B and Serde C.
- C received five explicit MCP errors: ambiguous `Flask`, two Express line-62 targets outside a
  callable, and two unsupported Serde `path:symbol` targets. The errors and extra calls are
  retained; no final run failed.
- Corpus-local git commands emitted the macOS `DARWIN_USER_TEMP_DIR`/`confstr` warning and used
  `/tmp`; source verification and diff checks still exited 0.

## Validation and next step

After the run set, Ruff format/lint passed, strict mypy passed across 28 files, all 24 task anchors
passed, and clean corpus pins and both binary hashes were rechecked. Measurement JSON syntax and
staged whitespace checks cover the new records and audit. Existing result directories, product
code, harness, prompts, scorer and preregistration were not changed.

The next local step is an offline deterministic regression for the observed Serde C freshness
failure: edit the indexed copy and query immediately, require updated caller evidence or an
honest stale/pending state, then inspect the watcher/refresh path and fix it generally. Keep
strict root confinement and uncertainty labels. Do not spend more external AI quota until this
local issue is resolved and a new run scope is explicitly approved. The matched pilot is complete;
the preregistered final L3 experiment remains pending. No push, tag, release or publish was made.
