"""Kontekst arama değerlendirmesinin çözümlemesi: `ccm-cli retrieval-eval` raporları ve kod grafı.

`report` raporu tipli değerlere okur; `stats` yöntem çiftlerinin MRR@10 farkı için Wilcoxon
işaretli sıralar testi ve bootstrap güven aralığı hesaplar; `cases` görev bazında sıra
değişimlerini graf yapısının yardım ettiği ve zarar verdiği durumlara ayırır; `graph_stats`
indekslenmiş projelerin düğüm ve kenar sayılarını türüne göre sayar.
"""
