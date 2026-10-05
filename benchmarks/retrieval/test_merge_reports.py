"""Rapor birleştirmenin ve Rust ile aynı metrik formüllerinin elle hesaplanmış örneklerle testleri.

Çalıştırma (benchmarks/ dizininde): `uv run python -m unittest retrieval.test_merge_reports`
"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from freshness.jsonio import as_list, as_object
from freshness.model import JsonValue
from retrieval.merge_reports import (
    InputReport,
    MergeError,
    merge_reports,
    percentile_nearest_rank,
    read_report,
)
from retrieval.report import find_method, load_report


def task(task_id: str, rank: int | None, latencies_ms: list[JsonValue]) -> JsonValue:
    return {
        "id": task_id,
        "project": "p",
        "split": "test",
        "gold": {"level": "file", "targets": ["./a.py"]},
        "rank": rank,
        "top_results": [],
        "latencies_ms": latencies_ms,
    }


def metrics(
    tasks: int, recall: tuple[float, float, float], mrr: float, p50: float, p95: float
) -> JsonValue:
    return {
        "tasks": tasks,
        "recall_at_1": recall[0],
        "recall_at_5": recall[1],
        "recall_at_10": recall[2],
        "mrr_at_10": mrr,
        "latency_p50_ms": p50,
        "latency_p95_ms": p95,
    }


def settings(propagation_weight: float) -> JsonValue:
    return {
        "limit": 10,
        "repeats": 2,
        "methods": ["semantic", "hybrid"],
        "seed_multiplier": 3,
        "propagation_weight": propagation_weight,
        "edge_weights": [["Calls", 1.0]],
        "hardware": {"os": "macos", "arch": "aarch64", "logical_cpus": 10, "physical_cpus": 10},
    }


def report(
    name: str,
    generated_at: int,
    commit: str,
    propagation_weight: float,
    methods: list[JsonValue],
    tasks: int,
) -> JsonValue:
    return {
        "schema_version": 1,
        "generated_at": generated_at,
        "commit": commit,
        "dataset": {
            "task_files": [
                {"path": f"{name}.json", "sha256": "0" * 64, "tasks": tasks, "skipped": 0}
            ],
            "tasks": tasks,
            "projects": [{"name": name}],
        },
        "settings": settings(propagation_weight),
        "methods": methods,
    }


def report_a(semantic_mrr: float) -> JsonValue:
    # S: t3 sıra 1, t1 sıra 6. H0: t3 sıra 2, t1 bulunamadı.
    return report(
        "a",
        100,
        "abc1234",
        0.1,
        [
            {
                "method": "semantic",
                "metrics": metrics(2, (0.5, 0.5, 1.0), semantic_mrr, 2.0, 4.0),
                "tasks": [task("t3", 1, [1.0, 2.0]), task("t1", 6, [3.0, 4.0])],
            },
            {
                "method": "hybrid",
                "metrics": metrics(2, (0.0, 0.5, 0.5), 0.25, 6.0, 8.0),
                "tasks": [task("t3", 2, [5.0, 6.0]), task("t1", None, [7.0, 8.0])],
            },
        ],
        2,
    )


def report_b(commit: str, propagation_weight: float) -> JsonValue:
    # S: t2 sıra 3. H0: t2 sıra 1.
    return report(
        "b",
        200,
        commit,
        propagation_weight,
        [
            {
                "method": "semantic",
                "metrics": metrics(1, (0.0, 1.0, 1.0), 1 / 3, 9.0, 10.0),
                "tasks": [task("t2", 3, [9.0, 10.0])],
            },
            {
                "method": "hybrid",
                "metrics": metrics(1, (1.0, 1.0, 1.0), 1.0, 11.0, 12.0),
                "tasks": [task("t2", 1, [11.0, 12.0])],
            },
        ],
        1,
    )


def read_value(directory: Path, name: str, value: JsonValue) -> InputReport:
    path = directory / f"{name}.json"
    path.write_text(json.dumps(value), encoding="utf-8")
    return read_report(path)


def task_ids(method: JsonValue) -> list[JsonValue]:
    tasks = as_list(
        as_object(method, "method", AssertionError).get("tasks"), "tasks", AssertionError
    )
    return [as_object(item, "task", AssertionError).get("id") for item in tasks]


class MergeReportsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.path = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.first = read_value(self.path, "a", report_a((1 / 1 + 1 / 6) / 2))
        self.second = read_value(self.path, "b", report_b("abc1234", 0.1))

    def test_tasks_sorted_by_id_and_metrics_recomputed(self) -> None:
        merged = merge_reports([self.first, self.second])
        methods = as_list(merged["methods"], "methods", AssertionError)
        self.assertEqual([task_ids(method) for method in methods], [["t1", "t2", "t3"]] * 2)
        # S: sıralar 6, 3, 1; gecikmeler 3, 4, 9, 10, 1, 2 → p50 = 3. eleman, p95 = 6. eleman.
        self.assertEqual(
            as_object(methods[0], "semantic", AssertionError)["metrics"],
            metrics(3, (1 / 3, 2 / 3, 3 / 3), (1 / 6 + 1 / 3 + 1 / 1) / 3, 3.0, 10.0),
        )
        # H0: sıralar yok, 1, 2; gecikmeler 7, 8, 11, 12, 5, 6.
        self.assertEqual(
            as_object(methods[1], "hybrid", AssertionError)["metrics"],
            metrics(3, (1 / 3, 2 / 3, 2 / 3), (0.0 + 1 / 1 + 1 / 2) / 3, 7.0, 12.0),
        )
        dataset = as_object(merged["dataset"], "dataset", AssertionError)
        self.assertEqual(dataset["tasks"], 3)
        self.assertEqual(dataset["projects"], [{"name": "a"}, {"name": "b"}])
        self.assertEqual(merged["generated_at"], 200)
        self.assertEqual(merge_reports([self.second, self.first])["methods"], merged["methods"])

    def test_merged_report_reads_as_retrieval_eval_report(self) -> None:
        path = self.path / "merged.json"
        path.write_text(json.dumps(merge_reports([self.first, self.second])), encoding="utf-8")
        loaded = load_report(path)
        self.assertEqual(find_method(loaded, "hybrid").mrr_at_10, 0.5)
        self.assertEqual(len(find_method(loaded, "semantic").tasks), 3)

    def test_reported_metrics_must_match_tasks(self) -> None:
        with self.assertRaises(MergeError):
            read_value(self.path, "wrong", report_a(0.5))

    def test_duplicate_task_ids_are_rejected(self) -> None:
        with self.assertRaises(MergeError):
            merge_reports([self.first, self.first])

    def test_different_settings_or_commit_are_rejected(self) -> None:
        other_weight = read_value(self.path, "w", report_b("abc1234", 0.3))
        other_commit = read_value(self.path, "c", report_b("def5678", 0.1))
        with self.assertRaises(MergeError):
            merge_reports([self.first, other_weight])
        with self.assertRaises(MergeError):
            merge_reports([self.first, other_commit])


class PercentileTest(unittest.TestCase):
    def test_nearest_rank(self) -> None:
        samples = [5.0, 1.0, 3.0, 2.0, 4.0]
        # n = 5: p50 → ⌈2,5⌉ = 3. eleman, p95 → ⌈4,75⌉ = 5. eleman.
        self.assertEqual(percentile_nearest_rank(samples, 50), 3.0)
        self.assertEqual(percentile_nearest_rank(samples, 95), 5.0)
        self.assertEqual(percentile_nearest_rank([7.0], 50), 7.0)

    def test_empty_samples(self) -> None:
        with self.assertRaises(MergeError):
            percentile_nearest_rank([], 50)


if __name__ == "__main__":
    unittest.main()
