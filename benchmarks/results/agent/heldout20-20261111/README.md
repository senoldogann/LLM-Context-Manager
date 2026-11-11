# Arm E for Claude Code (amendment 20), 2026-11-11

Design, decision rule and stopping rule: preregistration amendment 20. Task selection:
`SELECTION.md`; plan: `PLAN.md`. Both were committed before the first run.

All thirteen tasks are **held out**: no CCM output for any of their targets was looked at
before the run. The tuned material for arm E is separate: the replay of the amendment-17 and
amendment-18 transcripts in `arm-e-replay-20261111/`. Before the run, it predicted little
effect.

## What was measured

- **Harness and product commit:** `403f36c`. `claude/settings.json` gives `harness_revision` =
  `ccm_commit` = `403f36c` and `ccm_dirty: false`.
- **Release build:** one build from the clean `feat/arm-e-bash-grep-hook` checkout, ccm-cli
  `630994ad9a77…` and ccm-mcp `d6d530d88bfb…`.
- **Agent:** Claude Code 2.1.319 with `claude-opus-5-5`, effort high, subscription, local
  embeddings. Started detached through `scripts/run_until_done.py`, 14:03–14:44 EET.
- **Runs:** 117 (13 tasks × arms A, D, E × 3), all recorded in one pass. No usage-limit stop,
  no `quota-stop.json`, no transcript with `"status":"rejected"` or HTTP 429. No run timed out
  or is partial.
- **Arms:**
  - A is the built-ins.
  - D is the product with the Grep hook, as in amendments 17 and 18 but at this commit.
  - E is D with the hook also matched on Bash: a single `grep -r…`/`rg …` search with only
    fixed-string `grep -v` filters is answered.
  - B and C were not run; their definitions are unchanged.

The ratio is input tokens with cache, per task: the arm mean over three repetitions divided by
the A mean. The median over tasks is given with the fixed-seed bootstrap 95 % interval (from
`scripts/subset_report.py`; E/D from the same bootstrap). The full report is
`claude/report.md`.

## E/A — primary

| Task | A tokens | E tokens | E/A | D/A | E/D | A ok | E ok |
|---|---:|---:|---:|---:|---:|---:|---:|
| `django-callers-make-naive` | 34,304 | 39,623 | 1.16 | 1.05 | 1.10 | 3/3 | 3/3 |
| `django-callers-number-format` | 31,834 | 34,491 | 1.08 | 1.06 | 1.02 | 3/3 | 3/3 |
| `django-callers-get-version-tuple` | 35,286 | 39,205 | 1.11 | 1.05 | 1.06 | 3/3 | 3/3 |
| `django-callers-strip-quotes` | 40,956 | 47,611 | 1.16 | 1.02 | 1.14 | 3/3 | 3/3 |
| `django-callers-validate-file-name` | 28,942 | 35,301 | 1.22 | 1.00 | 1.22 | 3/3 | 3/3 |
| `django-impact-is-naive` | 24,283 | 25,901 | 1.07 | 0.94 | 1.14 | 3/3 | 3/3 |
| `django-trap-resolve-relation` | 44,211 | 50,766 | 1.15 | 0.99 | 1.16 | 3/3 | 3/3 |
| `sklearn-callers-check-x-y` | 46,077 | 41,547 | 0.90 | 0.73 | 1.23 | 3/3 | 3/3 |
| `sklearn-callers-check-scalar` | 31,273 | 23,200 | 0.74 | 0.73 | 1.01 | 3/3 | 3/3 |
| `sklearn-callers-pairwise-distances-chunked` | 37,618 | 42,564 | 1.13 | 0.82 | 1.38 | 3/3 | 3/3 |
| `sklearn-callers-pairwise-kernels` | 30,538 | 43,564 | 1.43 | 1.27 | 1.12 | 3/3 | 3/3 |
| `sklearn-callers-has-fit-parameter` | 37,521 | 38,701 | 1.03 | 1.07 | 0.96 | 3/3 | 3/3 |
| `sklearn-impact-randomized-svd` | 26,927 | 39,883 | 1.48 | 1.34 | 1.11 | 3/3 | 3/3 |

| Ratio over the 13 tasks | Median (95 % interval) |
|---|---|
| **E/A, input tokens (primary)** | **1.13 (1.07–1.16)** |
| D/A, input tokens | 1.02 (0.94–1.06) |
| E/D, input tokens | 1.12 (1.06–1.16) |
| E/A, turns | 1.14 (1.08–1.18) |
| E/A, cost | 1.04 (0.93–1.07) |

Success was 39/39 in every arm.

**Decision (amendment 20): cost shown.** The lower bound of E/A is above 1.00, so arm E uses
more input tokens than built-ins. D/A shows no detectable difference, as in amendments 17
and 18.

**The stopping rule applies.** Neither arm D nor arm E gave Claude Code a measurable saving.
Both are hook routes that leave Claude's own behaviour unchanged. Work on Claude Code adoption
stops here.

## What the hook did

| | D | E |
|---|---|---|
| Hook calls seen | 44 | 119 |
| Answered with the caller packet (all on Grep) | 20 | 24 |
| Answered with call-site lines (all on Bash) | 0 | 8 |
| Let through | 24 | 87 |
| Runs that called `context` themselves | 14 of 39 | 15 of 39 |

- **After each of the 24 packet answers in E, and 19 of the 20 in D,** Claude's next tool was
  Bash, re-checking the answer with its own search. Only the 8 single-search re-checks in E
  could be answered.
- **After those 8 call-site answers,** Claude stopped in 3, ran another Grep in 3 and another
  Bash in 2.
- **The other E Bash calls passed for these reasons:**
  - a shell variable in a loop: 12;
  - a regex `grep -v` filter: 11;
  - another pipe stage: 12;
  - several commands: 7;
  - patterns that are not a single symbol: 21, across Grep and Bash.
- **This matches the tuned replay's prediction.** Claude's re-checks are mostly commands that
  a hook cannot answer safely. Where one was answered, Claude often searched again. The extra
  turns (E/A turns 1.14) carry the extra tokens.

## How Claude Code shows the answer

Claude Code passes a PreToolUse `deny` to the model framed as an error:
`PreToolUse:Grep hook error: CCM answered this search …` (seen in every hook answer in the
transcripts). The hook cannot change that prefix. It is a plausible reason why Claude treats
the answer as something to verify rather than as a result. This run does not test that. It is
recorded as an observation, not a finding.

## Failed runs

None. All 117 runs succeeded, with no run failures and no timeouts.

## Owner audit

`scripts/owner_audit.py` flags no run.

## What this shows

Claude Code does not stop searching when a tool answers its search for it.

- The `context` tool (D) is used in about a third of runs here. The Grep hook answers about half
  of the first searches.
- Claude then re-checks with Bash, and when that is answered too (E) it re-checks again.
- Every hook answer that does not end the search adds a turn. That is why E costs more than A
  and D.

Across the held-out sets in large repositories, D-only and hook routes gave Claude Code either
no detectable difference (amendments 15, 17, 18, and D here) or a cost (E here).
