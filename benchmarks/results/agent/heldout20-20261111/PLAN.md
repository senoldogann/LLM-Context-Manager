# Amendment-20 held-out run (written before any run)

Design, decision rule and stopping rule: preregistration amendment 20. Task selection:
`SELECTION.md`. Harness and product commit: the commit that adds this file. One release build
from a clean checkout of it; the binary hashes are recorded in `claude/settings.json`.

Tasks (13): django-callers-make-naive, django-callers-number-format,
django-callers-get-version-tuple, django-callers-strip-quotes, django-callers-validate-file-name,
django-impact-is-naive, django-trap-resolve-relation, sklearn-callers-check-x-y,
sklearn-callers-check-scalar, sklearn-callers-pairwise-distances-chunked,
sklearn-callers-pairwise-kernels, sklearn-callers-has-fit-parameter,
sklearn-impact-randomized-svd.

Arms A, D and E, three repetitions, Claude Code `claude-opus-5-5` only, effort high, local
embeddings, `--max-budget-usd 2`. Started detached with `scripts/run_until_done.py 10 --
<agent run arguments>` into `claude/`.

Analysis:
- `scripts/subset_report.py <claude dir> E "…"` for E/A and with D for D/A;
- E/D from the per-task means;
- `agent report`;
- hook logs split by tool and answer kind;
- `scripts/owner_audit.py`;
- a transcript check for `"status":"rejected"` and HTTP 429.
