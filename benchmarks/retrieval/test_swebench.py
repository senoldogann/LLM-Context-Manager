"""SWE-bench Lite alt küme sırasının, yama uygulamasının ve oikeat vastaukset hesabının testleri.

Çalıştırma (benchmarks/ dizininde): `uv run python -m unittest retrieval.test_swebench`
"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from retrieval.swebench import (
    AppliedPatch,
    FunctionGold,
    FunctionSpan,
    GoldError,
    GraphNode,
    Hunk,
    HunkLine,
    Instance,
    PatchError,
    SweBenchError,
    apply_patch,
    function_node_ids,
    function_spans,
    has_file_node,
    instance_function_gold,
    load_instances,
    locate_hunk,
    order_key,
    parse_patch,
    selection_order,
    source_lines,
)

SEED: int = 20261005


def text(*rows: str) -> str:
    """Satırları \\n ile sonlanan metne birleştirir; girintiler testte açıkça görünür."""
    return "".join(f"{row}\n" for row in rows)


def file_diff(path: str, body: str) -> str:
    """Tek dosyayı yerinde değiştiren git yaması."""
    return (
        f"diff --git a/{path} b/{path}\n"
        "index 1111111..2222222 100644\n"
        f"--- a/{path}\n"
        f"+++ b/{path}\n"
        f"{body}"
    )


def instance(repo: str, instance_id: str) -> Instance:
    return Instance(
        instance_id=instance_id,
        repo=repo,
        base_commit="0" * 40,
        created_at="2020-01-01T00:00:00Z",
        version="1.0",
        problem_statement="issue",
        patch="",
    )


def gold_of(old_source: str, body: str) -> FunctionGold:
    """Yamayı eski kaynağa uygular ve fonksiyon düzeyindeki değişiklikleri döndürür."""
    patch = parse_patch(file_diff("m.py", body), "test")
    applied = apply_patch(source_lines(old_source, "test"), patch, "test")
    return instance_function_gold(old_source, applied, "test")


# Yama a..g dosyasına göre yazılmış: c → C ve f ile g arasına F.
TWO_HUNKS: str = text(
    "@@ -2,3 +2,3 @@", " b", "-c", "+C", " d", "@@ -6,2 +6,3 @@", " f", "+F", " g"
)


class ParsePatchTest(unittest.TestCase):
    def test_omitted_counts_mean_one_line(self) -> None:
        patch = parse_patch(file_diff("pkg/m.py", text("@@ -3 +3 @@", "-b = 2", "+b = 3")), "t")
        self.assertEqual(patch.path, "pkg/m.py")
        self.assertEqual(
            patch.hunks,
            (Hunk(3, 1, 3, 1, (HunkLine("-", "b = 2"), HunkLine("+", "b = 3"))),),
        )

    def test_no_newline_marker_is_skipped(self) -> None:
        body = text(
            "@@ -1,2 +1,2 @@",
            " a = 1",
            "-b = 2",
            "\\ No newline at end of file",
            "+b = 3",
            "\\ No newline at end of file",
        )
        self.assertEqual(
            parse_patch(file_diff("m.py", body), "t").hunks[0].lines,
            (HunkLine(" ", "a = 1"), HunkLine("-", "b = 2"), HunkLine("+", "b = 3")),
        )

    def test_rejects_patch_of_two_files(self) -> None:
        body = text("@@ -1 +1 @@", "-a", "+b")
        with self.assertRaises(PatchError):
            parse_patch(file_diff("a.py", body) + file_diff("b.py", body), "t")

    def test_rejects_new_file(self) -> None:
        patch = text(
            "diff --git a/new.py b/new.py",
            "new file mode 100644",
            "index 0000000..1111111",
            "--- /dev/null",
            "+++ b/new.py",
            "@@ -0,0 +1 @@",
            "+a = 1",
        )
        with self.assertRaises(PatchError):
            parse_patch(patch, "t")

    def test_rejects_renamed_file(self) -> None:
        patch = text(
            "diff --git a/old.py b/new.py",
            "similarity index 90%",
            "rename from old.py",
            "rename to new.py",
        )
        with self.assertRaises(PatchError):
            parse_patch(patch, "t")

    def test_rejects_non_python_file(self) -> None:
        with self.assertRaises(PatchError):
            parse_patch(file_diff("README.md", text("@@ -1 +1 @@", "-a", "+b")), "t")

    def test_rejects_truncated_hunk(self) -> None:
        with self.assertRaises(PatchError):
            parse_patch(file_diff("m.py", text("@@ -1,3 +1,3 @@", " a", "-b", "+B")), "t")


class ApplyPatchTest(unittest.TestCase):
    def test_applies_at_stated_lines(self) -> None:
        patch = parse_patch(file_diff("m.py", TWO_HUNKS), "t")
        self.assertEqual(
            apply_patch(("a", "b", "c", "d", "e", "f", "g"), patch, "t"),
            AppliedPatch(
                new_lines=("a", "b", "C", "d", "e", "f", "F", "g"),
                removed_lines=(3,),
                added_lines=(3, 7),
                offsets=(0, 0),
            ),
        )

    def test_offset_of_first_hunk_carries_to_the_next(self) -> None:
        # Dosyanın başında yamada olmayan iki satır var: hunk'lar iki satır aşağıda eşleşir.
        patch = parse_patch(file_diff("m.py", TWO_HUNKS), "t")
        self.assertEqual(
            apply_patch(("x", "y", "a", "b", "c", "d", "e", "f", "g"), patch, "t"),
            AppliedPatch(
                new_lines=("x", "y", "a", "b", "C", "d", "e", "f", "F", "g"),
                removed_lines=(5,),
                added_lines=(5, 9),
                offsets=(2, 2),
            ),
        )

    def test_changed_context_is_not_matched(self) -> None:
        patch = parse_patch(file_diff("m.py", TWO_HUNKS), "t")
        with self.assertRaises(PatchError):
            apply_patch(("a", "b", "X", "d", "e", "f", "g"), patch, "t")

    def test_pure_insertion_goes_after_the_stated_line(self) -> None:
        patch = parse_patch(file_diff("m.py", text("@@ -2,0 +3 @@", "+X")), "t")
        self.assertEqual(
            apply_patch(("a", "b", "c"), patch, "t"),
            AppliedPatch(
                new_lines=("a", "b", "X", "c"), removed_lines=(), added_lines=(3,), offsets=(0,)
            ),
        )

    def test_forward_position_wins_at_equal_distance(self) -> None:
        self.assertEqual(locate_hunk(("k", "z", "k"), ("k",), 1, 0), 2)

    def test_never_matches_before_the_previous_hunk(self) -> None:
        self.assertIsNone(locate_hunk(("k", "z", "z"), ("k",), 2, 1))

    def test_lines_split_only_on_newline(self) -> None:
        self.assertEqual(source_lines("a\r\nb\n", "t"), ("a\r", "b"))
        with self.assertRaises(GoldError):
            source_lines("a\rb\n", "t")


class FunctionSpansTest(unittest.TestCase):
    def test_decorated_nested_async_and_repeated_definitions(self) -> None:
        source = text(
            "import functools",
            "",
            "",
            "@functools.lru_cache",
            "def top(x):",
            "    def inner():",
            "        return x",
            "    return inner",
            "",
            "",
            "class Box:",
            "    @property",
            "    def size(self):",
            "        return 1",
            "",
            "    async def load(self):",
            "        return 2",
            "",
            "    def size(self):",
            "        return 3",
        )
        # Alan sırası: qualname, occurrence, name, def_line, start_line, end_line.
        self.assertEqual(
            function_spans(source, "t"),
            (
                FunctionSpan("top", 0, "top", 5, 4, 8),
                FunctionSpan("top.inner", 0, "inner", 6, 6, 7),
                FunctionSpan("Box.size", 0, "size", 13, 12, 14),
                FunctionSpan("Box.load", 0, "load", 16, 16, 17),
                FunctionSpan("Box.size", 1, "size", 19, 19, 20),
            ),
        )

    def test_syntax_error_is_gold_error(self) -> None:
        with self.assertRaises(GoldError):
            function_spans("def broken(:\n", "t")


class ChangedFunctionsTest(unittest.TestCase):
    def test_lines_inside_and_outside_functions(self) -> None:
        old = text(
            "X = 1",
            "",
            "",
            "class A:",
            "    def f(self):",
            "        return 1",
            "",
            "    def g(self):",
            "        return 2",
        )
        body = text(
            "@@ -1 +1 @@",
            "-X = 1",
            "+X = 2",
            "@@ -6 +6 @@",
            "-        return 1",
            "+        return 10",
        )
        self.assertEqual(
            gold_of(old, body),
            FunctionGold(
                functions=(FunctionSpan("A.f", 0, "f", 5, 5, 6),),
                removed_outside=1,
                added_outside=1,
            ),
        )

    def test_decorator_line_belongs_to_the_function(self) -> None:
        old = text("@a", "def f():", "    return 1")
        self.assertEqual(
            gold_of(old, text("@@ -1 +1 @@", "-@a", "+@b")),
            FunctionGold(
                functions=(FunctionSpan("f", 0, "f", 2, 1, 3),), removed_outside=0, added_outside=0
            ),
        )

    def test_new_nested_function_maps_to_the_existing_outer_one(self) -> None:
        old = text("def outer():", "    return 1")
        body = text(
            "@@ -1,2 +1,4 @@",
            " def outer():",
            "+    def helper():",
            "+        pass",
            " " + "    return 1",
        )
        self.assertEqual(
            gold_of(old, body),
            FunctionGold(
                functions=(FunctionSpan("outer", 0, "outer", 1, 1, 2),),
                removed_outside=0,
                added_outside=0,
            ),
        )

    def test_new_method_has_no_gold(self) -> None:
        old = text("class A:", "    def f(self):", "        return 1")
        body = text(
            "@@ -3 +3,4 @@",
            " " + "        return 1",
            "+",
            "+    def g(self):",
            "+        return 2",
        )
        self.assertEqual(
            gold_of(old, body), FunctionGold(functions=(), removed_outside=0, added_outside=3)
        )

    def test_changed_definition_count_is_ambiguous(self) -> None:
        old = text("def f():", "    return 1")
        body = text(
            "@@ -1,2 +1,5 @@",
            "+def f():",
            "+    return 0",
            "+",
            " def f():",
            " " + "    return 1",
        )
        with self.assertRaises(GoldError):
            gold_of(old, body)


class SelectionOrderTest(unittest.TestCase):
    def test_first_round_then_sainte_lague_quotients(self) -> None:
        # Depo boyutları a 5, b 2, c 1. İlk tur a, b, c; sonra bölümler N/(2s+1):
        # a 5/3 > b 2/3; a 5/5 > 2/3; a 5/7 > 2/3; b 2/3 > a 5/9; son yer a'nın (b ve c dolu).
        instances = [
            *(instance("o/a", f"a-{index}") for index in range(5)),
            *(instance("o/b", f"b-{index}") for index in range(2)),
            instance("o/c", "c-0"),
        ]
        self.assertEqual(
            [item.repo for item in selection_order(instances, SEED)],
            ["o/a", "o/b", "o/c", "o/a", "o/a", "o/a", "o/b", "o/a"],
        )

    def test_equal_quotients_go_by_repo_name(self) -> None:
        instances = [
            instance("o/b", "b-0"),
            instance("o/b", "b-1"),
            instance("o/a", "a-0"),
            instance("o/a", "a-1"),
        ]
        self.assertEqual(
            [item.repo for item in selection_order(instances, SEED)],
            ["o/a", "o/b", "o/a", "o/b"],
        )

    def test_repo_queue_follows_order_key_whatever_the_input_order(self) -> None:
        instances = [instance("o/a", f"a-{index}") for index in range(6)]
        order = selection_order(instances, SEED)
        self.assertEqual(
            [item.instance_id for item in order],
            sorted((item.instance_id for item in instances), key=lambda i: order_key(SEED, i)),
        )
        self.assertEqual(selection_order(list(reversed(instances)), SEED), order)


class FunctionNodeIdsTest(unittest.TestCase):
    NODES: tuple[GraphNode, ...] = (
        GraphNode("./pkg/m.py", "File", "./pkg/m.py", 1),
        GraphNode("./pkg/m.py:class_definition:symbol:aaaaaaaaaaaaaaaa:0", "Class", "A", 4),
        GraphNode("./pkg/m.py:function_definition:symbol:bbbbbbbbbbbbbbbb:0", "Function", "f", 5),
        GraphNode("./pkg/m.py:function_definition:symbol:cccccccccccccccc:0", "Function", "f", 9),
        GraphNode("./pkg/m2.py:function_definition:symbol:dddddddddddddddd:0", "Function", "g", 5),
    )

    def test_matches_name_and_def_line_in_the_same_file(self) -> None:
        # Dekoratör satır 8'de; CCM düğümü `def` satırından (9) başlar.
        span = FunctionSpan("A.f", 0, "f", 9, 8, 10)
        self.assertEqual(
            function_node_ids((span,), self.NODES, "pkg/m.py", "t"),
            ("./pkg/m.py:function_definition:symbol:cccccccccccccccc:0",),
        )

    def test_function_of_another_file_does_not_match(self) -> None:
        with self.assertRaises(GoldError):
            function_node_ids((FunctionSpan("g", 0, "g", 5, 5, 6),), self.NODES, "pkg/m.py", "t")

    def test_file_node(self) -> None:
        self.assertTrue(has_file_node(self.NODES, "pkg/m.py"))
        self.assertFalse(has_file_node(self.NODES, "pkg/m2.py"))


class LoadInstancesTest(unittest.TestCase):
    @staticmethod
    def row(instance_id: str, problem_statement: str) -> str:
        return json.dumps(
            {
                "instance_id": instance_id,
                "repo": "o/a",
                "base_commit": "0" * 40,
                "created_at": "2020-01-01T00:00:00Z",
                "version": "1.0",
                "problem_statement": problem_statement,
                "patch": "",
            },
            ensure_ascii=False,
        )

    def test_line_separator_inside_text_does_not_split_rows(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "instances.jsonl"
            path.write_text(
                text(self.row("a-1", "first second"), self.row("a-2", "x")), encoding="utf-8"
            )
            instances = load_instances(path)
        self.assertEqual([item.problem_statement for item in instances], ["first second", "x"])

    def test_duplicate_ids_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "instances.jsonl"
            path.write_text(text(self.row("a-1", "x"), self.row("a-1", "y")), encoding="utf-8")
            with self.assertRaises(SweBenchError):
                load_instances(path)


if __name__ == "__main__":
    unittest.main()
