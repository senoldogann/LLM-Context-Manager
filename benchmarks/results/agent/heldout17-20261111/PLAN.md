# Amendment-17 held-out run (written before any run)

Design, decision rule and stopping rule: preregistration amendment 17. Task selection:
`SELECTION.md`. Harness and product commit: the commit that adds this file. One release build
from a clean checkout of it; the binary hashes are recorded in each agent's `settings.json`.

Tasks (13, all held out): django-callers-log-response, django-callers-get-default-timezone,
django-callers-get-text-list, django-impact-get-docs-version, django-trap-resolve,
sklearn-callers-indexable, sklearn-callers-empirical-covariance, sklearn-callers-gen-even-slices,
sklearn-impact-check-scoring, sklearn-trap-normalize, tokio-callers-uninit,
tokio-callers-globals, tokio-trap-new-with-interest-and-handle.

Arms A and D, three repetitions, Claude Code `claude-opus-5-5` and Codex `gpt-6-luna`, effort
high, local embeddings, `--max-budget-usd 2`. For Claude Code, D includes the Grep hook. Both
agents are started with `scripts/run_until_done.py 10 -- <agent run arguments>` into
`claude/` and `codex/`; runs a usage limit rejects are not records.

Analysis:
- the primary ratio, via `scripts/subset_report.py <agent dir> D "Held-out=<13 ids>"`;
- `agent report`;
- `scripts/owner_audit.py`;
- hook decisions from the run records;
- a transcript check for `"status":"rejected"` and HTTP 429 before reporting.
