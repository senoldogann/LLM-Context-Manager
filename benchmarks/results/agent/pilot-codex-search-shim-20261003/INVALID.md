# Attempt 1: invalid S arm

Stopped by the supervising agent after 8 of 12 runs (commit `d925d30`). The S arm's rg wrapper was
never invoked: Codex ran the commands through its shell snapshot, which builds PATH itself, so the
HOME/.zprofile prepend had no effect. The four S runs therefore measured plain rg and their token
differences from A are run-to-run variance, not a wrapper effect. The `rg_path` field came from a
login-shell probe outside Codex and is not the rg the agent used. Kept as a failure record; the
valid pilot is `../pilot-codex-search-shim-20261003-v2/`.
