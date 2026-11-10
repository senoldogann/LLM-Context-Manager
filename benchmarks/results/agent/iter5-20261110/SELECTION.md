# Iteration 5 task selection (written before any iteration-5 run)

Date: 2026-11-10, 11:48Z. Product and harness commit `9bb6564` (branch
`fix/iteration4-review`, PR #14: review fixes of #13, the iteration-4 check fixes and the
compact hook answers), built from a fresh clone outside the original checkout. Release binaries:
ccm-cli a86cb4695fa8…, ccm-mcp bd5091752d12….
Corpus fetched again with `fetch_corpus.sh` at the same pinned commits. Preflight packets on these
binaries: Flask 12/12, Express 5/5, Serde 2/2, Django 11/11.

Held out (never run in any pilot; chosen by task id and category only, one per repository,
before any CCM packet for them was looked at):
- django-callers-iri-to-uri (new; gold from Python ast)
- flask-edit-method-route
- express-locate-trust
- serde-types-deserializer

Tuned (iteration-4 held-out tasks; their results and the capfirst transcript were read):
django-trap-capfirst, express-edit-normalize-type, flask-locate-app-string, serde-edit-wrap-field.

Arms: Codex A and B; Claude A, B, F and H. A, B and F are unchanged; H now uses the compact hook
answer (PR #14). The legacy arm C is not run.
