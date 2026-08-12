# Hybrid Ranking Design (Phase 2)

## Goal
Combine structural graph signals with semantic vector similarity to improve relevance and reduce hallucinations.

## Inputs
- Graph hits: nodes related by edges (Calls, Contains, Inherits, Defines, Imports, Reads, Writes)
- Vector hits: semantic similarity from LanceDB
- Cursor proximity: same file / nearby lines (when available)
- Recency: file modification time (optional)

## Scoring Formula
score = w_graph * graph_score + w_sem * semantic_score + w_spatial * spatial_score + w_recent * recency_score

Default weights (balanced):
- w_graph = 0.55
- w_sem = 0.35
- w_spatial = 0.08
- w_recent = 0.02

Alternative profiles:
- Structural-first: 0.65 / 0.25 / 0.07 / 0.03
- Semantic-first:   0.35 / 0.50 / 0.10 / 0.05

## Weight Tuning (Optional)
You can override defaults via environment variables (values are normalized to sum to 1.0):
- `CCM_HYBRID_GRAPH_WEIGHT`
- `CCM_HYBRID_SEM_WEIGHT`
- `CCM_HYBRID_SPATIAL_WEIGHT`
- `CCM_HYBRID_RECENT_WEIGHT`

## Graph Edge Weights
Direct neighbors (1 hop):
- Calls:    1.00
- Inherits: 0.90
- Defines:  0.85
- Contains: 0.80
- Reads:    0.70
- Writes:   0.70
- Imports:  0.60

Two-hop neighbors (optional): apply decay factor 0.60 * edge_weight.

## Semantic Score
Use a monotonic transform of distance:
- semantic_score = 1 / (1 + distance)
This is stable for L2 and does not assume a specific similarity range.

## Spatial Score
- Same node (cursor inside): 1.00
- Same file, different node: 0.80
- Different file: 0.40

## Recency Score (Optional)
- Use file mtime if available.
- recency_score = 1 / (1 + days_since_modified)

## Confidence
Confidence should account for both score and margin:
- confidence = min(1.0, score) * (1.0 + clamp(top1 - top2, 0.0, 0.2))
- Low confidence if score < 0.55 or top1 - top2 < 0.05

## Fallback Rules
- If low confidence and graph_score >= 0.6, return structural-only list.
- If low confidence and semantic_score >= 0.6, return semantic-only list.
- Otherwise return hybrid list with a warning in the reason.

## Integration Plan
1. Add HybridScorer (weights + scoring helpers).
2. Implement hybrid ranking for predict_context.
3. Implement hybrid ranking for search_code (vector hits + graph expansion).
4. Add eval mode: structural vs hybrid (A/B comparison).

## Evaluation Plan
- Run eval on golden_tasks.v2.json (structural-only) to establish baseline.
- Add a new search_code set for hybrid evaluation when embedder is available.
- Track hit-rate deltas and confidence distribution.

## Self-Improvement Layer (Phase 1 — proof of mechanism)

Sabit ağırlıklar artık versioned `RetrievalPolicy` içinde yaşar
(`core/src/policy/`); baseline, bu dokümandaki varsayılanların birebir
karşılığıdır. Öğrenme döngüsü:

1. `ccm-cli learn fixtures` deterministik sentetik corpus + embedding fixture
   üretir (`eval/fixtures/`; repo_a=train, repo_b=holdout, cross-repo; her repoda
   25 `search_code` + 25 `get_context` + 25 `read_graph` + 15 `predict_context` =
   90 task, toplam 180).
2. `ccm-cli learn optimize` train sette aday policy'leri değerlendirir (sabit
   seed 42, SplitMix64; 52 grid adayı + top-3 başlangıç noktasından hill-climb,
   cap 60), winner'ı holdout'ta tek kez ölçer ve promotion gate'ten geçirir;
   sonuç `data/ccm_learn/` altındaki `policies.json` + `history.jsonl` ve
   `eval/fixtures/learn/report.json` içine yazılır. Aynı rapor, gerçek repo
   structural corpus'unda (read_graph + get_context) baseline vs winner ikincil
   tablosunu da içerir (iddia taşımaz, regresyon izlemedir).
3. Promotion gate'i, recall regresyonu olmadan token verimliliği iyileşmesini
   sign test ile doğrular; `Rejected` da bilimsel sonuçtur (CI kırmızısı değil).

Ölçüm notları:
- Eval, seed-priority hybrid skorlamayı korur; bu skorlamada ağırlıklar
  sıralamayı değiştirmez (her semantic seed graph 1.0 alır). Bu nedenle Phase 1
  öğrenme sinyali context bütçesidir: `semantic_hits`, pencere büyüklükleri.
- Hybrid eval, embedder gerektirmeden `CCM_EMBEDDING_FIXTURE` ile çalışır;
  bilinmeyen sorgular aynı deterministik hash algoritmasıyla anında üretilir.
- Token tahmini `chars/4`'tür ve yalnızca göreli karşılaştırmadır.
- `predict_context` plan dışı ek sinyaldir: seed-priority skorlamada ağırlıklar
  sıralamayı değiştirmediği için öğrenilebilir sinyal context bütçesidir ve bu
  task tipi o bütçeyi ölçer. Planın 25/25/25 dağılımı aynen korunur.
