# Iteration 5 held-out set, three repetitions (written before any run)

Date: 2026-11-10, 12:41Z. Product and harness commit `9bb6564` (the iteration-5 commit,
merged as #14), checked out detached in a fresh clone; the same release binaries
(`~/.ccm-bench/bin-9bb6564`, ccm-cli a86cb4695fa8…), the same index caches, the same
corpus commits, the same arms and notes. Nothing is tuned between iteration 5 and this run.

Tasks: the four iteration-5 held-out tasks (django-callers-iri-to-uri, flask-edit-method-route,
express-locate-trust, serde-types-deserializer), three repetitions each. Their transcripts from
iteration 5 have been read only as counts (tool sequences and token totals), not changed for.
Arms: Codex A and B; Claude A, B, F and H (F and H reported apart). The iteration-5 single runs
stay in `../iter5-20261110/` and are not pooled with these.

Decision rule for the next step, fixed now:
- "The large-repository saving holds" for an agent if, on django-callers-iri-to-uri, the B arm's
  mean input tokens over the three repetitions are below the A arm's, and every one of the three B
  runs is below the A arms' mean. Then one or two more large-repository held-out tasks are added,
  written into the preregistration first.
- Otherwise the limit is documented in docs/STATUS.md as measured, without further Django tuning.
