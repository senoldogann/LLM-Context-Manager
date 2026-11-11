# Amendment-18 held-out run (written before any run)

Design, decision rule and stopping rule: preregistration amendment 18. Task selection:
`SELECTION.md`. Harness and product commit: the commit that adds this file. One release build
from a clean checkout of it; binary hashes in each agent's `settings.json`.

Tasks (13, all held out): django-callers-get-current-timezone,
django-callers-prefix-validation-error, django-callers-make-model-tuple,
django-callers-get-default-renderer, django-callers-resolve-url, django-impact-conditional-escape,
django-trap-load-backend, sklearn-callers-compute-sample-weight, sklearn-callers-softmax,
sklearn-callers-label-binarize, sklearn-callers-device, sklearn-callers-supported-float-dtypes,
sklearn-impact-check-non-negative.

Arms A and D, three repetitions, Claude Code `claude-opus-5-5` and Codex `gpt-6-luna`, effort
high, local embeddings, `--max-budget-usd 2`. Claude Code's D includes the Grep hook with
repeat handling. Both agents are started detached with `scripts/run_until_done.py 10 --
<agent run arguments>` into `claude/` and `codex/`.

Analysis:
- `scripts/subset_report.py` for the primary ratio;
- `agent report`;
- `scripts/owner_audit.py`;
- hook decisions from the hook logs, split into packet and call-site-line answers;
- a transcript check for `"status":"rejected"` and HTTP 429.
