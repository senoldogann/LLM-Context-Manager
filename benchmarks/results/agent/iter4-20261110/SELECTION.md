# Iteration 4 task selection (written before any iteration-4 run)

Date: 2026-11-10, 08:28Z. Product and harness commit `ed0f4e8` (branch
`feat/iteration4-hook`: `main` at `b9418bd` with the review fixes of `95fab13`, plus
`var self = this` aliases in receiver evidence, the opt-in Grep hook and arm H).
Release binaries built from `ed0f4e8`: ccm-cli c9d433824eb0…,
ccm-mcp 45563a67963a…. Preflight packets on these binaries:
Flask 12/12, Express 5/5, Serde 2/2, Django 11/11.

Held out (never run in any pilot; chosen by task id and category only, one per repository,
before any CCM packet for them was looked at):
- django-trap-capfirst (new; gold from Python ast)
- express-edit-normalize-type
- flask-locate-app-string
- serde-edit-wrap-field

Tuned (iteration-3 tasks whose transcripts were read):
django-callers-make-aware, express-trap-res-send, serde-trap-deserialize-struct,
flask-edit-error-handler.

Arms: Codex A and B; Claude A, B, F and H. A, B and F are defined as in iterations 2 and 3;
H is B plus the opt-in Grep hook (preregistration amendment 8), reported apart. The legacy arm C
is not run (unchanged; iteration 1 measured it above A).
