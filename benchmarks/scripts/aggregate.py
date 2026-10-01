#!/usr/bin/env python3
"""Aggregate external-benchmark comparison reports into a summary table.

Reads benchmarks/results/<repo>.compare.json (produced by run_benchmark.sh via
`ccm-cli eval --compare`), or <dir>/<repo>.compare.json when a results directory
is given as the first argument, and emits:
  - per-repo, per-mode, per-query-type pass rates
  - Recall@K and MRR@K (K = max_rank, default 5) computed from `ranked` lists
  - mean latency per mode/query-type

Matching replicates ccm-core eval::score_hits:
  - a hit matches an expected node id after normalize_node_id (strip '#...')
  - a hit matches an expected file path when its leading ':'-segment equals it
"""
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS = ROOT / "results"
# Rapor anahtarı "structural" tarihsel bir addır; bu mod semantic-only aramadır.
MODES = ("structural", "hybrid")
MODE_LABELS = {"structural": "semantic-only", "hybrid": "hybrid"}


def normalize_node_id(node_id: str) -> str:
    return node_id.split("#", 1)[0]


def node_file_path(node_id: str) -> str:
    cleaned = normalize_node_id(node_id)
    return cleaned.split(":", 1)[0]


def expected_set(task):
    ids = set()
    for nid in (task.get("expected") or {}).get("node_ids") or []:
        ids.add(("node", nid))
    for fp in (task.get("expected") or {}).get("file_paths") or []:
        ids.add(("file", fp))
    return ids


def match_hits(task, ranked):
    exp = expected_set(task)
    matched = set()
    for hit in ranked or []:
        norm = normalize_node_id(hit)
        if ("node", norm) in exp:
            matched.add(norm)
            continue
        key = node_file_path(hit)
        if ("file", key) in exp:
            matched.add(key)
    return matched


def compute_metrics(task, ranked):
    exp = expected_set(task)
    exp_count = len(exp)
    metrics = {}
    k = max((task.get("expected") or {}).get("max_rank") or 5, 1)

    matched = set()
    first_rank = None
    for rank, hit in enumerate((ranked or [])[:k], start=1):
        norm = normalize_node_id(hit)
        key = None
        if ("node", norm) in exp:
            key = norm
        else:
            fp = node_file_path(hit)
            if ("file", fp) in exp:
                key = fp
        if key is not None:
            # Rust score_hits gibi: beklenen anahtarı kaydet; aynı dosyadan
            # birden fazla hit tek eşleşme sayılır.
            matched.add(key)
            if first_rank is None:
                first_rank = rank

    metrics["recall_at_k"] = (len(matched) / exp_count) if exp_count else 0.0
    metrics["mrr_at_k"] = (1.0 / first_rank) if first_rank else 0.0
    return metrics


def load_reports(results):
    reports = {}
    for path in sorted(results.glob("*.compare.json")):
        reports[path.stem.replace(".compare", "")] = json.loads(path.read_text())
    return reports


def mean_or_zero(values):
    return statistics.mean(values) if values else 0.0


def main():
    results = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else RESULTS
    reports = load_reports(results)
    if not reports:
        print(f"No reports found under {results}/*.compare.json", file=sys.stderr)
        return 1

    print("=" * 100)
    print("CCM External Benchmark — summary")
    print("=" * 100)

    # Recall@K / MRR@K are computed from the tasks files (exact ground truth),
    # not from the report (which only carries matched counts).
    metrics_by_repo = compute_exact_metrics(reports)

    for repo, report in sorted(reports.items()):
        print(f"\n### {repo} (all query types)")
        print(f"{'mode':<14} {'tasks':>6} {'passed':>6} {'failed':>6} {'pass%':>7} "
              f"{'R@K':>7} {'MRR@K':>7} {'lat(ms)':>9}")
        for mode in MODES:
            rep = report[mode]
            totals = rep.get("totals", {})
            scored = totals.get("scored", 0)
            passed = totals.get("passed", 0)
            pct = (passed / scored * 100) if scored else 0.0
            per_task = metrics_by_repo.get(repo, {}).get(mode, {}).values()
            lat = [r.get("latency_ms") for r in rep.get("results", []) if r.get("latency_ms") is not None]
            print(f"{MODE_LABELS[mode]:<14} {scored:>6} {passed:>6} {totals.get('failed', 0):>6} {pct:>6.1f}% "
                  f"{mean_or_zero([m['recall@k'] for m in per_task]):>6.2f} "
                  f"{mean_or_zero([m['mrr@k'] for m in per_task]):>6.2f} "
                  f"{mean_or_zero(lat):>8.1f}ms")
        for mode in MODES:
            rep = report[mode]
            by_type = defaultdict(lambda: [0, 0])
            for res in rep.get("results", []):
                by_type[res.get("query_type", "?")][0] += 1
                if res.get("status") == "pass":
                    by_type[res.get("query_type", "?")][1] += 1
            if by_type:
                parts = []
                for qt, (total, passed) in sorted(by_type.items()):
                    parts.append(f"  {qt}: {passed}/{total}")
                print(f"  [{MODE_LABELS[mode]}] by query type: {', '.join(parts)}")

    # ---- Overall: semantic-only vs hybrid on search_code tasks only ----
    print("\n### Overall (search_code only: semantic-only vs hybrid)")
    for mode in MODES:
        rec, mrr, pas, lat = [], [], [], []
        for repo, report in sorted(reports.items()):
            per_task = metrics_by_repo.get(repo, {}).get(mode, {})
            for res in report[mode].get("results", []):
                if res.get("query_type") != "search_code" or res.get("id") not in per_task:
                    continue
                rec.append(per_task[res["id"]]["recall@k"])
                mrr.append(per_task[res["id"]]["mrr@k"])
                pas.append(1 if res.get("status") == "pass" else 0)
                if res.get("latency_ms") is not None:
                    lat.append(res["latency_ms"])
        print(f"{MODE_LABELS[mode]:<14} pass={sum(pas)}/{len(pas)}  R@K={mean_or_zero(rec):.3f}  "
              f"MRR@K={mean_or_zero(mrr):.3f}  lat={mean_or_zero(lat):.1f}ms")

    return 0


def compute_exact_metrics(reports):
    """Compute per-task Recall@K and MRR@K using ground truth from the tasks files.

    Returns {repo: {mode: {task_id: {"recall@k": float, "mrr@k": float}}}} so
    callers can aggregate over exactly the query types they report.
    """
    out = {}
    for tasks_path in sorted((ROOT / "tasks").glob("*.json")):
        repo = tasks_path.stem
        if repo not in reports:
            continue
        tasks = json.loads(tasks_path.read_text())["tasks"]
        by_id = {t["id"]: t for t in tasks}
        out[repo] = {}
        for mode in MODES:
            per_task = {}
            for res in reports[repo][mode].get("results", []):
                task = by_id.get(res.get("id"))
                if not task or res.get("status") == "skipped":
                    continue
                m = compute_metrics(task, res.get("ranked") or [])
                per_task[res["id"]] = {"recall@k": m["recall_at_k"], "mrr@k": m["mrr_at_k"]}
            out[repo][mode] = per_task
    return out


if __name__ == "__main__":
    sys.exit(main())
