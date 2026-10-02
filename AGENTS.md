# Agent guide

CCM (Cognitive Codebase Matrix) is a Rust code graph served over MCP (`core/`, `mcp/`, `cli/`),
with an npm launcher (`npm/`) and Python benchmark harnesses (`benchmarks/`).

Start every session with [`docs/STATUS.md`](docs/STATUS.md) (the goal, where things stand, the
next steps and the rules) and `git log --oneline -15`. When you finish or change a step, update
`docs/STATUS.md` so the next agent can continue from there.

- The owner writes in Turkish: answer in Turkish; code identifiers stay as they are.
- Code: comments in Turkish; strict types everywhere; small pure functions with one purpose; no
  default parameter values; raise explicit, specific errors with enough context and no silent
  fallbacks; prefer integration or end-to-end tests to unit tests; match the surrounding code.
- Rust commands need the pinned toolchain on `PATH`: `PATH="$HOME/.cargo/bin:$PATH" cargo …`.
- Before finishing a change, run the checks under "Verifying changes" in `docs/STATUS.md`.
- A measurement is published with its commit and its failures; nothing is claimed without one.
- `.superpowers/sdd/` (git-ignored) holds ledgers of finished plans: history, not open work.
