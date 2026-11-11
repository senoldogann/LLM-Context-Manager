# Large-repository experiment (written before any run)

Design and decision rule: preregistration amendment 15. Date: 2026-11-11, 22:42Z.
Product and harness commit `01b2afa`; one release build (ccm-cli 5ea5c4bd122b…,
ccm-mcp 601fc2c0bae7…). Corpus: Django 373cb30, Tokio bb7ca75, scikit-learn 156ef14.

Held out (12): django-callers-is-aware, django-callers-make-hashable, django-trap-get-connection,
django-impact-get-resolver, sklearn-callers-check-cv, sklearn-callers-gen-batches,
sklearn-trap-shuffle, sklearn-impact-get-config, tokio-callers-new-unnamed,
tokio-callers-unbounded-channel, tokio-trap-try-with, tokio-impact-enter-runtime.
Tuned (6, reported apart): django-callers-iri-to-uri, django-callers-constant-time-compare,
django-trap-urlencode, tokio-callers-sleep-until, tokio-callers-async-op,
tokio-callers-with-scheduler.
Arms A and D, three repetitions, Claude Code claude-opus-5-5 and Codex gpt-6-luna.
