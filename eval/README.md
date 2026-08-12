# CCM Evaluation Framework

Measure retrieval quality with golden tasks.

## Quick Start

```bash
# Run all golden tasks
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json

# Compare structural vs hybrid scoring
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json --compare --report eval/report.json
```

If the repository index is missing, the evaluator builds it automatically before scoring.
`search_code` tasks still require a configured embedder.

## Files

| File | Purpose |
|------|---------|
| `golden_tasks.schema.json` | JSON Schema for task validation |
| `golden_tasks.example.json` | Template with example tasks |
| `golden_tasks.v3.ccm.json` | Production golden tasks (56 tasks) |
| `repos.json` | Repository list for evaluation |

## Golden Task Format

```json
{
  "id": "unique-task-id",
  "repo": {
    "name": "Repository Name",
    "path": "/path/to/repo"
  },
  "query": {
    "type": "search_code|read_graph|get_context",
    "text": "search query",
    "node_id": "graph node id",
    "file_path": "src/file.rs",
    "line": 42
  },
  "expected": {
    "node_ids": ["expected-node-id"],
    "file_paths": ["/expected/path"],
    "min_recall": 1,
    "max_rank": 5
  }
}
```

## Query Types

| Type | Description | Required Fields |
|------|-------------|------------------|
| `search_code` | Semantic search | `text` |
| `read_graph` | Graph navigation | `node_id` |
| `get_context` | Cursor-based retrieval | `file_path`, `line` |

## Recorded Results

```
See the checked-in reports for the latest recorded runs:

- `report.semantic.json` — v0.3.3 gerçek Ollama embedder ile (56 task, 94.6%
  pass; search_code 3/6). CI structural gate `search_code`'u ölçmez; bu rapor
  gerçek anlamsal geri getirimi gösterir.
- `report.phase3_baseline.json` — structural-only CI gate baseline (50/50
  structural task).
- `eval/fixtures/learn/report.json` — sentetik self-improvement (proof of
  mechanism).

> Eski 102-task corpus'undan üretilen `report.hybrid.json`, `report.compare.json`
> ve `report.ccm.json` stale idi ve "Node not found" hatalarıyla yanıltıcıydı;
> kaldırıldı. `golden_tasks.v3.json` (102 task) arşiv olarak durur, üretim
> corpus'u `golden_tasks.v3.ccm.json` (56 task).
```

## Adding New Tasks

1. Add task to `golden_tasks.v3.ccm.json`
2. Run: `ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json --report eval/report.json`
3. Review failures and iterate

## Reports

Reports are saved as JSON with:
- `totals` - Pass/fail counts
- `results` - Per-task details
- `query_type` breakdown - Scores by category

## Synthetic Self-Improvement Corpus

`eval/fixtures/` altında deterministic bir sentetik corpus bulunur:

- `repos/repo_a` + `repos/repo_b`: iki küçük Rust repo ağacı (repo_a=train,
  repo_b=holdout, cross-repo split).
- `golden_tasks.synthetic.json`: 180 task (her repoda 25 `search_code`,
  25 `get_context`, 25 `read_graph`, 15 `predict_context`).
- `embeddings.ndjson`: token-hash embedding fixture (dim 64, seed 42); CI'da
  `CCM_EMBEDDING_FIXTURE` ile offline hybrid eval sağlar.

Fixture yeniden üretilebilir: `cargo run -p ccm-cli -- learn fixtures`.
Öğrenme döngüsü: `cargo run -p ccm-cli -- learn optimize --seed 42`.
Sonuç raporu `eval/fixtures/learn/report.json`; policy store ve history
`data/ccm_learn/` altındadır (gitignore kapsamında).

`predict_context` task'ları, policy'nin context bütçesini (semantic hit sayısı,
pencere büyüklükleri) ölçülebilir kılan plan dışı ek sinyaldir (ağırlıklar
seed-priority skorlamada sıralamayı değiştirmez); `get_context`/`read_graph`/
`search_code` regression guard'larıdır. Optimizer 52 grid adayı üretir ve
top-3 başlangıç noktasından hill-climb yapar (cap 60). Golden task şemasına
`split` ve `task_type` opsiyonel alanları eklendi (eski dosyalar değişmeden
parse edilir). `learn report`, sentetik holdout birincil tablosuna ek olarak
gerçek repo structural corpus'unda (varsa `eval/golden_tasks.v3.ccm.json`)
ikincil baseline-vs-winner tablosu basar.
