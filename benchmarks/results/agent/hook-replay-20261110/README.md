# Offline replay of recorded Claude Code greps through the CCM hook (2026-11-10)

No agent was run. `benchmarks/scripts/hook_replay.py` took every Grep call recorded in the
Claude Code transcripts of the 2026-11-10 pilots (`iter2`, `iter3` and `iter4`, arms A, B, F and
H; 119 calls) and gave each one, with its recorded pattern, path and output mode, to
`ccm-mcp hook claude-pre-tool-use`. Each repository was indexed graph-only from a clean export
of the pinned corpus. The binaries were debug builds of the commit that adds this file.

`replay.txt` lists every call: the hook's decision, the recorded size of the real Grep result
(`grep=`) and the size of the text the hook would give the model instead (`answer=`).

| | calls | recorded grep output | hook answer |
|---|---|---|---|
| answered by the hook (deny) | 16 | 31,455 chars | 20,546 chars (−35 %) |
| let through (pass) | 103 | | |

Reasons for letting a grep through: the pattern is not a single symbol (49, mostly
alternations such as `a|def b`), the packet does not claim a complete call list (24), the answer
would not be shorter than the grep output (18), the grep uses `-i`, `glob` or `type` (8), or it is
a definition search (3). In every answered call the hook's text is shorter than the recorded
grep result. The tasks behind the 16 answered calls are Flask `ensure_sync` and `load_dotenv`
and Django `make_aware` and `get_random_string`.

What this does not show: whether Claude stops after the hook's answer or greps again, and what
that does to whole-run token use. That needs arm H in an agent pilot.

Before this change the hook answered none of the 9 greps it saw in iteration 4: it compared the
full `context` packet (body, callees, members) with Grep output, which Claude requests in
`content` mode with a median of 718 characters.
