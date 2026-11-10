# Iteration 2 task selection (written before any iteration-2 run)

Date: 2026-11-10, 04:33Z. Product commit `c0228ee`, harness commit `031d50a`.
Release binaries built from `c0228ee`: ccm-cli 47b818b46a3f…,
ccm-mcp 2a84cf3c7dd2….

Tuned (packets inspected while changing the product):
flask-callers-ensure-sync, express-trap-req-get, serde-edit-with-bound, django-trap-html-escape,
and the three iteration-1 held-out tasks, which became tuned once their transcripts were read
(flask-trap-load-dotenv, express-callers-res-set, serde-callers-expr-is-missing); only the four
original tuned tasks are re-run here.

Held out (no CCM packet looked at before this run):
- django-callers-make-aware (new; gold from Python ast)
- express-trap-sendfile-helper
- serde-trap-deserialize-struct
- flask-edit-error-handler

Arms: Codex A and B; Claude A, B and F. The legacy arm C is not re-run (iteration 1 measured it
at C/A 2.47 for Codex on the held-out set). Index: each repository is indexed once per binary
and copied into each run (`--index-cache-dir`); the record keeps copy and build times apart.
