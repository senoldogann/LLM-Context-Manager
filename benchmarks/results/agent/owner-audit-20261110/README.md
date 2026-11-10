# Owner audit of the 2026-11-10 pilots (no rescoring)

The preregistered scorer matches answers per file and function name. It accepts
`Receiver.closed` for `Sender.closed`, and it counts two same-named methods in one file as one
expected item. `scripts/owner_audit.py` checks the stored answers of every non-edit task in the
result sets below for two things:
- a wrong owner: the named class or type does not contain that function in the pinned source;
- a missing same-named copy, for the three tasks where the gold has such copies.

It changes no record and no score. Edit tasks are skipped because their answers name code the
agent added. Owners are accepted broadly (any enclosing class, impl type, function or module,
or the file or directory name), so the audit errs towards not flagging. For `impl Trait for Wrapper<Inner>` the wrapped type also
counts as an owner (`Handle.schedule` for `impl Schedule for Arc<Handle>`).

Result (all sets through iteration 9): 6 of the audited runs would fail under owner-aware scoring; all 6 were scored as
successes. Token ratios are unaffected.

| Results | Agent | Commit | Task | Runs | Wrong owner | Missing same-name copy | Unqualified copy |
|---|---|---|---|---|---|---|---|
| iter6-20261110 | claude | `516ed4c` | `django-trap-urlencode` | 12 | 2 | 0 | 0 |
| iter7-20261110 | claude | `e805025` | `django-trap-urlencode` | 12 | 2 | 1 | 0 |
| iter8-20261110 | codex | `4c5f193` | `tokio-callers-async-op` | 6 | 2 | 0 | 0 |

- iter6-20261110/claude django-trap-urlencode__A__1: wrong ['django/contrib/admin/options.py:BaseModelAdmin.get_preserved_filters'], missing []
- iter6-20261110/claude django-trap-urlencode__H__1: wrong ['django/contrib/admin/options.py:BaseModelAdmin.get_preserved_filters'], missing []
- iter7-20261110/claude django-trap-urlencode__B__2: wrong ['django/contrib/admin/options.py:BaseModelAdmin.get_preserved_filters'], missing []
- iter7-20261110/claude django-trap-urlencode__H__1: wrong ['django/contrib/admin/options.py:BaseModelAdmin.get_preserved_filters'], missing ['django/test/client.py:AsyncRequestFactory.generic']
- iter8-20261110/codex tokio-callers-async-op__A__1: wrong ['tokio/src/sync/oneshot.rs:Receiver.closed'], missing []
- iter8-20261110/codex tokio-callers-async-op__B__2: wrong ['tokio/src/sync/oneshot.rs:Receiver.closed'], missing []

Audited result sets and their measured commits: heldout-20261110/claude at `b6d055d`; heldout-20261110/codex at `b6d055d`; pilot-claude-20261110/pilot-claude-20261110 at `b6d055d`; pilot-codex-luna-20261110/pilot-codex-luna-20261110 at `b6d055d`; iter2-20261110/claude at `031d50a`; iter2-20261110/codex at `031d50a`; iter3-20261110/claude at `b398928`; iter3-20261110/codex at `b398928`; iter4-20261110/claude at `ed0f4e8`; iter4-20261110/codex at `ed0f4e8`; iter5-20261110/claude at `9bb6564`; iter5-20261110/codex at `9bb6564`; iter5-reps-20261110/claude at `9bb6564`; iter5-reps-20261110/codex at `9bb6564`; iter6-20261110/claude at `516ed4c`; iter6-20261110/codex at `516ed4c`; iter7-20261110/claude at `e805025`; iter7-20261110/codex at `e805025`; iter8-20261110/claude at `4c5f193`; iter8-20261110/codex at `4c5f193`; iter9-20261110/claude at `31502d6`; iter9-20261110/codex at `31502d6`.
