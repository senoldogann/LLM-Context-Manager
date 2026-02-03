# CCM 12-Week Execution Plan (Option 3)

## Purpose
Make CCM a reliable, measurable, and professional context engine that consistently reduces LLM context loss and hallucinations across large, multi-language codebases.

## Scope
This plan focuses on measurable retrieval quality, indexing stability, and production-grade ergonomics.

## Non-Goals
1. Claiming "perfect memory" or "infinite context".
2. Replacing LLM context windows.
3. Solving all AI reasoning errors.

## Current Verification Status
Manual Verification: update_index
Result: Passed. Data size stayed stable after a file update (2.3M -> 2.4M), indicating no index duplication.
Note: delete_by_prefix behavior in LanceDB 0.23.1 still needs a targeted smoke test.

## Success Metrics
| Metric | Definition | Target |
| --- | --- | --- |
| Context Hit Rate | Correct top-5 retrievals on golden tasks | >= 85% |
| Hallucination Reduction | Fewer incorrect references vs baseline | >= 40% reduction |
| Freshness Latency | Time from file change to retrievable index | <= 10s |
| Index Growth | DB size increase over 7 days active dev | <= 15% |
| Time-to-First-Value | Install + first query success | <= 5 minutes |

## Deliverables
1. Evaluation harness with golden task set.
2. Retrieval quality improvements (hybrid ranking + better path/ID stability).
3. Stability validation for large repositories.
4. Product-ready onboarding and positioning.

## Phase Plan (12 Weeks)

### Phase 1: Measurement & Baseline (Weeks 1-3)
Goal: Establish reproducible evaluation and a baseline.

Deliverables:
1. Golden task dataset (50-200 tasks).
2. Automated evaluation harness for search_code, read_graph, get_context.
3. Baseline metrics report and regression tracking.

Exit Criteria:
1. Repeatable scoring for top-5 retrieval accuracy.
2. Documented baseline results.

### Phase 2: Retrieval Quality (Weeks 4-6)
Goal: Improve relevance and reduce wrong-context returns.

Deliverables:
1. Hybrid ranking rules (graph weight + semantic score).
2. Query normalization and path normalization hardening.
3. Quality hooks to detect low-confidence retrievals.

Exit Criteria:
1. +15% improvement in Context Hit Rate vs baseline.
2. Fewer "generic" semantic matches without structure.

### Phase 3: Stability & Scale (Weeks 7-9)
Goal: Handle large repos and long-running usage safely.

Deliverables:
1. Large repo stress tests and growth analysis.
2. Incremental index validation including deletes, renames, and moves.
3. Storage hygiene policy (GC behavior, compaction strategy if needed).

Exit Criteria:
1. Index Growth <= 15% over 7 days of simulated dev activity.
2. All change scenarios pass without duplication.

### Phase 4: Productization (Weeks 10-12)
Goal: Professional developer experience and clear messaging.

Deliverables:
1. Onboarding quick-start (5-minute success path).
2. Release notes and upgrade guidance.
3. Positioning statement and docs refresh.

Exit Criteria:
1. First-value in <= 5 minutes on two real projects.
2. Documentation clarity reviewed by at least 3 external users.

## Work Breakdown by Week
Week 1:
1. Define golden task format and selection criteria.
2. Build minimal evaluation runner and metrics schema.
3. Add targeted delete_by_prefix smoke test.

Week 2:
1. Populate golden tasks across 2-3 real repos.
2. Run baseline evaluation and store results.
3. Document baseline gaps and prioritize fixes.

Week 3:
1. Add regression tracking and CI-friendly report output.
2. Finalize baseline report.
3. Publish internal benchmark summary.

Week 4:
1. Implement hybrid ranking heuristic.
2. Add query normalization and path strictness checks.
3. Introduce confidence scoring for retrieval results.

Week 5:
1. Evaluate hybrid ranking impact.
2. Tune parameters against golden tasks.
3. Reduce false positives in semantic-only matches.

Week 6:
1. Lock phase 2 improvements.
2. Document quality improvements.
3. Prepare scale test data.

Week 7:
1. Run large repo indexing and incremental change tests.
2. Measure DB growth and latency.
3. Add diagnostics for index bloat.

Week 8:
1. Validate rename/move/delete handling.
2. Stress test watch mode behavior.
3. Verify manifest-based fallback behavior.

Week 9:
1. Implement any needed GC/compaction strategy.
2. Stabilize storage and index maintenance.
3. Finalize stability report.

Week 10:
1. Rewrite onboarding for quick start.
2. Create guided examples for 3 use cases.
3. Simplify config paths and defaults.

Week 11:
1. Validate docs with external users.
2. Finalize positioning message and claims.
3. Produce release notes and upgrade guide.

Week 12:
1. Final regression run.
2. Publish benchmarks and case study.
3. Prepare release and announcement.

## Risks and Mitigations
Risk: delete_by_prefix not supported for LIKE predicates.
Mitigation: add smoke tests and fallback delete strategies.

Risk: Golden tasks are biased or too small.
Mitigation: source tasks from multiple repositories and contributors.

Risk: Metrics are gamed by trivial matches.
Mitigation: require explanation quality and structural accuracy.

## Next Immediate Actions
1. Implement delete_by_prefix smoke test and fallback decision.
2. Define golden task schema and repository list.
3. Create initial evaluation harness skeleton.
