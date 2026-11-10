# Held-out task selection (written before any held-out run)

Date: 2026-11-10, 02:47Z. Frozen product and harness commit: `b6d055d`
(release binaries ccm-cli 6139d16bfdf7…,
ccm-mcp f90fa1e8951c…).

The context packet was tuned while looking at four tasks: flask-callers-ensure-sync,
express-trap-req-get, serde-edit-with-bound and django-trap-html-escape ("tuned").
Results on those measure fit as much as usefulness.

Held-out tasks: preregistered tasks whose CCM packets were not inspected during tuning,
one per language, different symbols:

- flask-trap-load-dotenv (Python, trap)
- express-callers-res-set (JavaScript, callers)
- serde-callers-expr-is-missing (Rust, callers)

They run with the same binaries, models and arms, in separate output directories, and are
reported separately from the tuned tasks. No product change is made between the tuned and the
held-out runs.

Disclosure (2026-11-10, after the held-out runs had started): while testing index copying, the
supervising agent once ran a graph-only `context` query about `res.set` (the symbol of
express-callers-res-set) and saw "No context found". No product or harness change was made
because of it, and the held-out runs use the binaries frozen above.
