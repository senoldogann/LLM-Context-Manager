# Codex context-first pilot audit

Measured on 2026-10-03 at `4185640e67b1d6e80c82e9c27cb9d9ca6b5a36d2` with a clean
tracked product/harness tree. The owner explicitly approved this bounded subscription pilot;
the owner subsequently selected dedicated Codex only. No Claude run, final L3 experiment or
publishing operation was started. The generated [report](report.md), [settings](settings.json),
nine run records and three edit diffs accompany this audit. Full transcripts remain local and
git-ignored.

## Scope and execution

- Codex CLI 0.160.0, `gpt-6.1-sol`, effort `high`, subscription authentication through the
  existing dedicated `~/.ccm-bench/codex-home`; no credential file was inspected or copied by
  the supervising agent.
- `context-first-pilot`: A=built-ins; B=default `context + index_project` with the context note;
  C=legacy full discovery with its navigation note. This is a separate product pilot; the
  preregistered final A/B/C design and its task ground truth were unchanged.
- Three pinned tasks, one repetition per arm, sequentially: Flask A/B/C, Express B/C/A,
  Serde C/A/B. Each run used a fresh temporary `git archive` workspace and temporary HOME.
  Only those workspaces received the Serde edits.
- B/C used the already cached local embedding model; `UV_OFFLINE=1` prevented dependency
  acquisition. External AI calls were the nine authorized Codex agent runs. No additional
  agent run, embedding API call or model download was made.
- The existing optimized binaries were selected explicitly. SHA-256:
  `ccm-cli=72e84aea16b1cac9aca39dd10f08fa7ba1130f7814f1120a4b31d44d47090703`,
  `ccm-mcp=b44d580e78a81f1145b2df20c1ab5cdaf324fae4731204402f8eb0c61ea4970a`.
  The model-free release preflight passed before the pilot: Flask 12/12, Express 5/5,
  edited Serde 2/2, all at recall=precision=1.000 with no extra caller.
- Harness exit code: **0**. Nine of nine final answers scored recall=precision=1.000;
  no missing, extra or stale item, no run failure. All three Serde edits were applied and
  their saved diffs are byte-identical. The edits were checked by the harness markers and
  `git diff --check`; the corpus's Rust package was not compiled by this pilot.

Command, from `benchmarks/`, with a newly created output directory:

```bash
UV_OFFLINE=1 uv run python -m agent run --agent codex \
  --agent-bin /Users/dogan/.local/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex \
  --codex-home /Users/dogan/.ccm-bench/codex-home --auth subscription \
  --profile context-first-pilot --model gpt-6.1-sol --effort high --embedding local \
  --max-budget-usd 2 --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --ccm-bin-dir ../target/release --ccm-model-dir /Users/dogan/.ccm/models \
  --corpus-dir corpus --out-dir results/agent/pilot-codex-context-first-20261003
```

The dollar flags are recorded settings, **not enforced limits for Codex**. The CLI reports no
actual dollar cost or turn count. Console `spent $0.00` means no reported cost; quota was used.
The report's dollar columns use the harness's previously committed API price table, not a
subscription invoice or an independently verified charge. CLI token counts are an observable
usage proxy; subscription quota weighting is unknown.

## Tokens and tool use

Input includes cached input exactly once. Total below means reported input plus output;
the raw events additionally expose a reasoning-output field, which is not added separately
by the existing harness. Every record's input, cached input, output, tool counts and final text
were checked against the real JSONL transcript through the existing Codex parser.

| Task | Arm | Input incl. cache | Output | Input + output | CCM calls | Shell calls | Agent wall | Pre-index |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| Flask | A | 45,292 | 1,189 | 46,481 | 0 | 4 | 49.4 s | — |
| Flask | B | 82,371 | 1,120 | 83,491 | 1 | 4 | 49.4 s | 26.9 s |
| Flask | C | 54,792 | 475 | 55,267 | 5 | 2 | 24.2 s | 26.8 s |
| Express | A | 68,054 | 352 | 68,406 | 0 | 6 | 22.7 s | — |
| Express | B | 65,583 | 480 | 66,063 | 1 | 3 | 28.5 s | 4.3 s |
| Express | C | 53,730 | 445 | 54,175 | 4 | 3 | 24.8 s | 4.3 s |
| Serde | A | 48,302 | 713 | 49,015 | 0 | 4 | 33.4 s | — |
| Serde | B | 62,364 | 580 | 62,944 | 1 | 4 | 31.6 s | 45.6 s |
| Serde | C | 105,985 | 847 | 106,832 | 7 | 5 | 43.4 s | 45.3 s |
| **Sum** | **A** | **161,648** | **2,254** | **163,902** | **0** | **14** | **105.4 s** | **—** |
| **Sum** | **B** | **210,318** | **2,180** | **212,498** | **3** | **11** | **109.4 s** | **76.8 s** |
| **Sum** | **C** | **214,507** | **1,767** | **216,274** | **16** | **10** | **92.4 s** | **76.4 s** |

Each Serde arm also made one file-change call. B called `context` exactly once in every task;
neither B nor C called `index_project` during the agent session. All six CCM arms used CCM,
unlike the earlier Claude pilot. These different-agent observations do not isolate the effect
of the surface change on Claude behavior.

| Task | B / A input + output | C / A input + output |
|---|---:|---:|
| Flask | +79.62% | +18.90% |
| Express | −3.43% | −20.80% |
| Serde | +28.42% | +117.96% |
| Sum of these three tasks | +29.65% | +31.95% |

These are descriptive results for three tasks and one repetition, not population estimates.
The generated report's paired input-token interval for B/A is 0.96–1.82 and for C/A is
0.79–2.19; both span 1. Final answer success does not prove each tool packet was sufficient.
Agent wall excludes pre-index time, which is measured separately above. No reusable-index
experiment was run.

## Packet limitations observed in real queries

### Flask B: caller names require additional scope reconstruction

The agent sent:

```text
Which functions or methods in src/flask/ call ensure_sync directly? Find all calls and their enclosing outermost named function/method.
```

`context` returned the correct 12 semantic callers, with bounded `may call` labels preserved,
but displayed `wrapper`, `view` and unqualified method names. It did not provide the enclosing
`copy_current_request_context`, `View.as_view` and class-qualified method names requested by
the task. The agent subsequently ran grep/read commands and an AST traversal to reconstruct
them. Both A and B first attempted `python` (exit 127), then succeeded with `python3`;
these retries are retained in token/time/tool counts. This is a packet composition gap even
though the model-free normalization and final answer pass.

### Express B: plain code hints bypass receiver evidence

The actual query had **no backticks**:

```text
Find every function in lib/ that calls the request getter req.get or its alias req.header defined in lib/request.js. Exclude app.get and res.get; include request calls through this or other aliases and identify outermost named function.
```

The returned `get` packet listed `range`, `protocol`, `ip`, `ips`, `subdomains`, `hostname`
and `xhr` in `lib/request.js`. Against the task answer, the packet had four correct callers,
three extras (`ip`, `ips`, `subdomains`, whose matching calls are `this.app.get`) and one
missing caller (`lib/response.js:location`, calling `this.req.get`). Thus this particular
packet's caller set had recall 4/5 and precision 4/7. The final agent answer was correct only
after additional source inspection.

Source inspection at the measured commit explains the difference from preflight:
`member_call_hint` accepts receiver/path hints only inside backticks. This real plain-text
query went through hybrid retrieval and the lexical graph explanation without receiver-based
caller replacement. The preflight query uses explicit backtick hints and remains green.
No fix or framework-specific rule was added during the measurement.

### Serde B: pre-edit context does not demonstrate post-edit CCM freshness

B's single `context` call occurred before the edit and returned `ser::build_generics` and
its surrounding graph. B then edited the file and answered the post-edit question using
source inspection; it made no post-edit CCM call. Its final 2/2 answer is correct but is not
evidence of CCM refresh in that agent arm. C did call `find_usages` after its edit and received
`de::build_generics` plus `ser::with_serialize_bound`, with the removed caller absent.

C also tried `path:symbol` targets twice and got explicit “no symbol named” results before
using supported bare-symbol/path:line targets. These extra tool calls remain in the results;
the product pilot did not change the compatibility tools' target grammar.

## Isolation, validation and next step

The harness's forbidden-tool/granted-tool checks passed for all nine runs. Completed events
showed only local shell/file-change operations and the permitted CCM tools; no web, app,
other MCP server or spawned-agent item appeared. Manual command inspection found corpus reads,
temporary-workspace edits and ancestor `AGENTS.md` existence checks with no returned ancestor
instructions. The first successful real Codex transcript's parsing and isolation were inspected;
the raw-usage consistency check was then applied to all nine transcripts.

After the pilot, Ruff format/lint passed, strict mypy passed across 28 files and all 24 task
anchors passed. Release hashes and the Rust product tree remained unchanged. The full local
Rust/retrieval verification accompanying `4185640` remains applicable; no Rust or Python
implementation was changed by this result-recording step. Whitespace checks cover the new
status/audit/report/JSON records; saved corpus patches are exact measurement artifacts. The
first staged whitespace check flagged the mandatory single-space empty context line in each
saved unified diff. A scoped `.gitattributes` rule disables only end-of-line whitespace checks
for archived agent diff files, preserving their bytes and the checks on source/docs/JSON.

The next local work is to turn the actual plain-text Express query and the caller-scope
information requirement into general routing/composition regressions, then fix the server
without framework names or root-confinement bypasses. Revalidate the optimized binaries and
existing test gates after that change. Any follow-up external AI run needs a fresh approved
scope and a new output directory. The final preregistered L3 run is still pending.
