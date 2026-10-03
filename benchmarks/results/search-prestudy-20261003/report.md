# Search output pre-study: real rg/grep commands, RTK versus structure-aware output

Measured on 2026-10-03 at commit `d925d30` with `uv run python -m search.replay` (command below).
Exploratory pre-study, not a preregistered measurement. `summary.txt` is the script's own output and
`measurements.json` holds one numeric record per measured command; no command text, path or output
content was recorded or shown to an AI model.

## Question

Agents spend many tokens on search output. RTK 0.51.0 already compresses command output as text. Does
output that knows the code structure (definition, call site, import, comment or string, and the
enclosing function) shrink real search output further without hiding match locations?

## Method

- **Commands:** the owner's local RTK history (`history.db`, 5 Jul – 3 Oct 2026; it also contains this
  session's own agent commands). 16,362 rg/grep commands, 15,051 unique (command, project) pairs,
  10,346 in a project directory that still exists, 5,152 simple (one rg or grep call; no pipe,
  redirection, subshell, `--pre` or decompression). A seeded random sample of 1,500 (seed 20261003)
  was replayed read-only in its recorded project directory with a 15 s timeout.
- **Outputs compared for each command:** raw stdout; RTK (`rtk rg|grep …`, with the owner's filters and
  limits but tracking and telemetry off, in a separate HOME); a structureless baseline (same file
  grouping, 100-character trimming, ignore rules and budget, no syntax); CCM full (enclosing-function
  groups with trimmed match text); CCM compact (definitions with their signature line, other matches
  as line numbers under the enclosing function, comment/string matches as line numbers); and
  *passthrough* = compact unless it is not smaller than raw, then raw.
- **Budget:** RTK's default limits, counted as output lines that carry locations: 25 per file, 200 in
  total. A compact line can carry up to 40 line numbers of one scope.
- **Structure:** `search/ccm-outline`, compiled from the same tree-sitter grammar versions as CCM
  (Python, Rust, TypeScript/JavaScript, Swift, Go). Files in other languages stay structureless.
- **Units:** o200k_base tokens (tiktoken 0.14.0, the GPT/Codex tokenizer family) and bytes.
- **Line-shaped** commands print `path:line:text` matches; list, count, context and inverted searches
  are "other" and CCM leaves them to RTK.

## Results

403 of the 1,500 sampled commands still produced output (1,079 matched nothing or failed in today's
trees, 17 exceeded 5 MB, 1 timed out); 308 were line-shaped.

| Line-shaped commands (n=308) | RTK | Structureless | CCM full | CCM compact | Passthrough |
|---|---:|---:|---:|---:|---:|
| Median per-command tokens, RTK = 1 | 1.00 | 1.06 | 1.07 | 0.67 | **0.67** |
| p25 – p75 | — | 0.84 – 1.33 | 0.75 – 1.46 | 0.46 – 1.09 | **0.46 – 1.00** |
| Total tokens, RTK = 1 | 1.00 | 0.221 | 0.222 | 0.180 | 0.177 |
| Visible match locations (of 89,462) | 5,985 | 5,786 | 5,952 | 8,602 | — |

- Passthrough used at least 30% fewer tokens than RTK for 53.2% of line-shaped commands and more than
  RTK for 2.3%. Compact showed fewer locations than RTK for 3 commands.
- The structureless baseline is larger than RTK at the median (1.06): the per-command saving comes
  from the structure, not from grouping or trimming. Compact/structureless median 0.67.
- **The totals are not the headline.** They are dominated by 8 commands of ≥25,000 raw tokens whose
  long lines are trimmed (total passthrough/RTK 0.07 in that bucket); trimming is not specific to
  structure.
- By raw size (count, median passthrough/RTK): <250 tokens 198, 0.82; 250–2,500 tokens 89, 0.44;
  2,500–25,000 tokens 13, 0.76; ≥25,000 tokens 8, 0.65.
- Line-shaped matches: code 46.6%, non-code or unsupported files 41.6%, comments and strings 5.3%,
  imports 3.3%, definitions 3.2%.
- RTK's output equalled raw output for 69.5% of line-shaped commands.

An earlier byte-only run of the same scripts in the session scratch directory (not committed) gave a
median of 0.59 by bytes; whitespace-heavy raw lines are cheaper in tokens than in bytes.

## Limits and open failures

- One developer's history; today's trees, not the trees the commands originally searched. Only 27% of
  the sample reproduced, and compound or piped commands (about half of the eligible ones) were excluded.
- o200k_base is not Claude's tokenizer.
- Compact output drops the text of code-match lines. Whether agents then open more files is not
  measured here; the agent pilot in `results/agent/pilot-codex-search-shim-20261003/` addresses it.
- Classification was spot-checked on Flask, Django, Serde and Express, not validated against a
  labelled set.

## Related tools

Structure-aware search for agents already exists as explicit tools: Graft (`graft grep` groups matches
by enclosing symbol; MCP, hooks and instruction files), wonk (tree-sitter ranked search; MCP, plugin
or CLI) and duckgrep (SQL over a code graph through MCP). Each reports agent-level savings with its
own method. None of them was found to rewrite the agent's own rg/grep output in place, which is what
RTK does for text; this pre-study measures that combination.

## Reproduce

```bash
cd benchmarks
uv run python -m search.replay --rtk-db "$HOME/Library/Application Support/rtk/history.db" \
  --rtk-home "$HOME/.cache/ccm-bench/rtk-home" --tiktoken-cache "$HOME/.cache/ccm-bench/tiktoken" \
  --sample 1500 --seed 20261003 --timeout-s 15 --workers 4 --out results/search-prestudy-20261003
```

`--rtk-home` holds a copy of the RTK `config.toml` and `filters.toml` with `[tracking]` and
`[telemetry]` disabled; the tiktoken cache holds the o200k_base file. Build `ccm-outline` first with
`cargo build --release --offline --manifest-path search/ccm-outline/Cargo.toml`.
