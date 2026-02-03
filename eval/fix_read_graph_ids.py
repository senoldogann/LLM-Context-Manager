#!/usr/bin/env python3
import argparse
import json
import re
import sys
from pathlib import Path
from typing import Optional


def split_node_id(node_id: str):
    cleaned = node_id.replace("\\", "/")
    parts = cleaned.rsplit(":", 3)
    if len(parts) != 4:
        return None
    file_path, kind, row, col = parts
    try:
        return file_path, kind, int(row), int(col)
    except ValueError:
        return None


def extract_name(notes: Optional[str]):
    if not notes:
        return None
    match = re.search(r"name\\s+([A-Za-z0-9_]+)", notes)
    if match:
        return match.group(1)
    return None


def build_graph_index(nodes):
    by_key = {}
    node_ids = set()
    for node in nodes:
        if not isinstance(node, dict):
            continue
        node_id = node.get("id")
        if not isinstance(node_id, str):
            continue
        node_ids.add(node_id)
        parsed = split_node_id(node_id)
        if not parsed:
            continue
        file_path, kind, row, _col = parsed
        entry = {"id": node_id, "name": node.get("name"), "row": row}
        by_key.setdefault((file_path, kind), []).append(entry)

    for key in by_key:
        by_key[key].sort(key=lambda item: item["row"])
    return by_key, node_ids


def pick_candidate(candidates, target_row, name=None):
    if name:
        named = [c for c in candidates if c.get("name") == name]
        if named:
            candidates = named
    return min(candidates, key=lambda item: abs(item["row"] - target_row))


def main():
    parser = argparse.ArgumentParser(
        description="Fix read_graph task node_id values using ccm_graph.json"
    )
    parser.add_argument(
        "--tasks",
        default="eval/golden_tasks.v3.ccm.json",
        help="Path to golden tasks JSON",
    )
    parser.add_argument(
        "--graph",
        default="data/ccm_graph.json",
        help="Path to graph JSON",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Optional output path (defaults to overwrite --tasks when --write)",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="Write updates to disk",
    )
    parser.add_argument(
        "--force-name",
        action="store_true",
        help="Prefer exact name match from notes even if current node_id exists",
    )
    args = parser.parse_args()

    tasks_path = Path(args.tasks)
    graph_path = Path(args.graph)

    if not tasks_path.exists():
        print(f"Missing tasks file: {tasks_path}", file=sys.stderr)
        return 1
    if not graph_path.exists():
        print(f"Missing graph file: {graph_path}", file=sys.stderr)
        return 1

    tasks_data = json.loads(tasks_path.read_text())
    graph_data = json.loads(graph_path.read_text())

    nodes = graph_data.get("nodes", [])
    by_key, node_ids = build_graph_index(nodes)

    updated = 0
    unchanged = 0
    unresolved = 0
    updated_ids = []

    for task in tasks_data.get("tasks", []):
        query = task.get("query", {})
        if query.get("type") != "read_graph":
            continue
        node_id = query.get("node_id")
        if not node_id:
            continue
        parsed = split_node_id(node_id)
        if not parsed:
            unresolved += 1
            continue
        file_path, kind, row, _col = parsed

        name = extract_name(task.get("notes"))
        if args.force_name and name:
            named_candidates = by_key.get((file_path, kind), [])
            named_candidates = [c for c in named_candidates if c.get("name") == name]
            if named_candidates:
                candidate = pick_candidate(named_candidates, row, name=name)
                new_id = candidate["id"]
                if new_id != node_id:
                    query["node_id"] = new_id
                    expected = task.get("expected", {})
                    if isinstance(expected.get("node_ids"), list):
                        expected["node_ids"] = [new_id]
                    task["query"] = query
                    task["expected"] = expected
                    updated += 1
                    updated_ids.append((task.get("id"), node_id, new_id))
                else:
                    unchanged += 1
                continue

        if node_id in node_ids:
            unchanged += 1
            continue

        candidates = by_key.get((file_path, kind), [])
        if not candidates:
            unresolved += 1
            continue

        candidate = pick_candidate(candidates, row, name=name)
        new_id = candidate["id"]

        if new_id != node_id:
            query["node_id"] = new_id
            expected = task.get("expected", {})
            if isinstance(expected.get("node_ids"), list):
                expected["node_ids"] = [new_id]
            task["query"] = query
            task["expected"] = expected
            updated += 1
            updated_ids.append((task.get("id"), node_id, new_id))
        else:
            unchanged += 1

    print(f"read_graph tasks updated: {updated}")
    print(f"read_graph tasks unchanged: {unchanged}")
    print(f"read_graph tasks unresolved: {unresolved}")

    if updated_ids:
        print("Updated IDs (task_id: old -> new):")
        for task_id, old_id, new_id in updated_ids:
            print(f"- {task_id}: {old_id} -> {new_id}")

    if args.write:
        output_path = Path(args.output) if args.output else tasks_path
        output_path.write_text(json.dumps(tasks_data, indent=2) + "\n")
        print(f"Wrote {output_path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
