"""İstatistik ve sıra tersi hesaplarının elle hesaplanmış örneklerle testleri.

Çalıştırma (benchmarks/ dizininde): `uv run python -m unittest retrieval.test_stats`
"""

from __future__ import annotations

import unittest
from fractions import Fraction

from retrieval.report import ReportError, reciprocal_rank
from retrieval.stats import bootstrap_mean_ci, doubled_midranks, wilcoxon_signed_rank


class WilcoxonTest(unittest.TestCase):
    def test_all_positive_differences(self) -> None:
        # Sıralar 1..5 hep pozitif: W+ = 15 ve 2^5 işaret düzeninden yalnız biri bu kadar uç;
        # iki yönlü p = 2 · 1/32.
        result = wilcoxon_signed_rank([Fraction(value) for value in (1, 2, 3, 4, 5)])
        self.assertEqual(result.nonzero, 5)
        self.assertEqual(result.w_plus, 15.0)
        self.assertEqual(result.w_minus, 0.0)
        self.assertEqual(result.p_value, 0.0625)

    def test_zero_dropped_and_ties_share_midrank(self) -> None:
        # Sıfır atılır; |1/2| eşitleri 1,5 sırasını, |1| 3 sırasını alır. W+ = 1,5 + 3 = 4,5.
        # Sıra toplamlarının 8 düzendeki dağılımı 0, 1,5, 1,5, 3, 3, 4,5, 4,5, 6; W+ ≥ 4,5
        # olan 3 düzen var: p = 2 · 3/8.
        result = wilcoxon_signed_rank([Fraction(0), Fraction(1, 2), Fraction(-1, 2), Fraction(1)])
        self.assertEqual(result.nonzero, 3)
        self.assertEqual(result.w_plus, 4.5)
        self.assertEqual(result.w_minus, 1.5)
        self.assertEqual(result.p_value, 0.75)

    def test_no_differences(self) -> None:
        result = wilcoxon_signed_rank([Fraction(0), Fraction(0)])
        self.assertEqual(result.nonzero, 0)
        self.assertEqual(result.p_value, 1.0)

    def test_doubled_midranks(self) -> None:
        values = [Fraction(5), Fraction(1), Fraction(5), Fraction(3)]
        self.assertEqual(doubled_midranks(values), [7, 2, 7, 4])


class BootstrapTest(unittest.TestCase):
    def test_constant_differences(self) -> None:
        self.assertEqual(bootstrap_mean_ci([0.25] * 7, 500, 11), (0.25, 0.25))

    def test_same_seed_same_interval(self) -> None:
        differences = [0.0, 1.0, 0.5, -0.5, 0.0, 1.0 / 3.0]
        first = bootstrap_mean_ci(differences, 1000, 20261005)
        self.assertEqual(first, bootstrap_mean_ci(differences, 1000, 20261005))
        self.assertLessEqual(min(differences), first[0])
        self.assertLessEqual(first[0], first[1])
        self.assertLessEqual(first[1], max(differences))

    def test_empty_differences(self) -> None:
        with self.assertRaises(ValueError):
            bootstrap_mean_ci([], 10, 1)


class ReciprocalRankTest(unittest.TestCase):
    def test_depth_cutoff(self) -> None:
        ranks = (1, 3, None, 11, 10)
        self.assertEqual(
            [reciprocal_rank(rank, 10) for rank in ranks],
            [Fraction(1), Fraction(1, 3), Fraction(0), Fraction(0), Fraction(1, 10)],
        )

    def test_rank_below_one(self) -> None:
        with self.assertRaises(ReportError):
            reciprocal_rank(0, 10)


if __name__ == "__main__":
    unittest.main()
