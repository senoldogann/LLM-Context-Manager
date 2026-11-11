# Amendment-19 held-out run (written before any run)

Design, decision rule and stopping rule: preregistration amendment 19. Task selection:
`SELECTION.md`. Harness and product commit: the commit that adds this file. One release build
from a clean checkout of it; the binary hashes are recorded in `codex/settings.json`.

Tasks (12): django-callers-flatten-fieldsets, django-callers-call-command,
django-impact-get-user-model, django-impact-safe-join, django-impact-get-current-site,
django-trap-select-template, sklearn-callers-compute-class-weight,
sklearn-callers-as-float-array, sklearn-impact-get-scorer-names, sklearn-impact-svd-flip,
sklearn-impact-pairwise-distances, sklearn-impact-accuracy-score.

Arms A and D, three repetitions, Codex `gpt-6-luna` only, effort high, local embeddings,
`--max-budget-usd 2`. Started detached with `scripts/run_until_done.py 10 -- <agent run
arguments>` into `codex/`.

Analysis: `scripts/subset_report.py` for the primary set (12 tasks) and the impact subset (7);
`agent report`; `scripts/owner_audit.py`; a transcript check for `rg`/`grep` after `context` in
impact D runs.
