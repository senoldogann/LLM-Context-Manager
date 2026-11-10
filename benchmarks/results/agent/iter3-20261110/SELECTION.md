# Iteration 3 task selection (written before any iteration-3 run)

Date: 2026-11-10, 05:27Z. Product commit `85252fb`, harness commit `b398928`.
Release binaries built from `85252fb`: ccm-cli be74726ccf82…,
ccm-mcp 997da4cfafa6…. Preflight packets on these binaries:
Flask 12/12, Express 5/5, Serde 2/2, Django 11/11 (the preflight then failed to delete one
temporary directory: OSError 66, a cleanup race unrelated to the packets).

Held out (no CCM packet looked at before this run):
- django-callers-get-random-string (new; gold from Python ast)
- express-trap-res-send
- serde-trap-effective-style
- flask-edit-debug-flag

Tuned (iteration-2 held-out tasks whose transcripts were read to design iteration 3):
django-callers-make-aware, express-trap-sendfile-helper, serde-trap-deserialize-struct,
flask-edit-error-handler.

Arms: Codex A and B; Claude A, B and F.
