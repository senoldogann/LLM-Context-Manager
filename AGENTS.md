# Agent guide

CCM (Cognitive Codebase Matrix) is a Rust code graph served over MCP (`core/`, `mcp/`, `cli/`),
with an npm launcher (`npm/`) and Python benchmark harnesses (`benchmarks/`).

Start with [`docs/STATUS.md`](docs/STATUS.md): the goal, where things stand, the next steps and the
rules for this repository. When you finish or change a step, update it so the next agent can
continue from there.

- Rust commands need the pinned toolchain on `PATH`: `PATH="$HOME/.cargo/bin:$PATH" cargo …`.
- Before finishing a code change, run the checks listed under "Verifying changes" in
  `docs/STATUS.md`.
- A measurement is published with its commit and its failures; nothing is claimed without one.
