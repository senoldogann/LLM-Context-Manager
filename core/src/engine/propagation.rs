//! Graf skor yayılımı (H1) ve yalnızca semantik sıralama (S).
//!
//! İki sıralama da saf fonksiyondur: vektör aramasının düğüm başına semantik
//! skorlarını (tohumlar) ve kod grafını alır, sıralanmış aday listesi döndürür.
//! H1'de bir düğümün desteği, ona doğrudan kenarla bağlı tohumların (düğümün
//! kendisi hariç) semantik skorlarının kenar ağırlığıyla çarpılıp toplanmasıdır;
//! iki düğüm arasında birden çok kenar varsa en ağır kenar sayılır. Destekler en
//! büyük desteğe bölünerek [0, 1] aralığına getirilir ve skor
//! `(1 − λ)·semantik + λ·destek` olur. File ve DataFile düğümleri sonuçlarda yer
//! almadığından destek almaz; aksi halde normalizasyonu onlar belirlerdi.

use std::collections::{HashMap, HashSet};

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::graph::{CodeGraph, NodeType};
use crate::policy::RetrievalPolicy;

/// Sıralanmış tek aday: semantik skor, normalize komşu desteği ve nihai skor.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedCandidate {
    pub id: String,
    pub semantic_score: f32,
    pub support: f32,
    pub score: f32,
}

/// Yalnızca semantik sıralama (S): tohumlar semantik skora göre azalan, eşitlikte
/// kimliğe göre artan sırada; graf kullanılmaz.
pub fn semantic_ranking(semantic: &HashMap<String, f32>) -> Vec<RankedCandidate> {
    let candidates = semantic
        .iter()
        .map(|(id, score)| RankedCandidate {
            id: id.clone(),
            semantic_score: *score,
            support: 0.0,
            score: *score,
        })
        .collect();
    sorted(candidates)
}

/// Tohumların tek adımlık komşularına verdiği, en büyük desteğe bölünmüş destek.
/// Destek yoksa (kenar yok ya da tüm ağırlıklar sıfır) sonuç boştur. Toplama
/// sırası tohum kimliğine göre sabittir; aynı girdi aynı float değerleri verir.
pub fn propagated_support(
    graph: &CodeGraph,
    semantic: &HashMap<String, f32>,
    policy: &RetrievalPolicy,
) -> HashMap<String, f32> {
    let mut seeds: Vec<(&String, &f32)> = semantic.iter().collect();
    seeds.sort_by(|a, b| a.0.cmp(b.0));

    let mut raw: HashMap<String, f32> = HashMap::new();
    for (seed_id, seed_score) in seeds {
        let Some(seed) = graph.find_node_index_by_id(seed_id) else {
            continue;
        };
        for (neighbor, weight) in strongest_neighbor_edges(graph, seed, policy) {
            let node = &graph.graph[neighbor];
            if matches!(node.node_type, NodeType::File | NodeType::DataFile) {
                continue;
            }
            *raw.entry(node.id.clone()).or_insert(0.0) += weight * seed_score;
        }
    }

    let max_support = raw.values().copied().fold(0.0_f32, f32::max);
    if max_support <= 0.0 {
        return HashMap::new();
    }
    raw.into_iter()
        .map(|(id, value)| (id, value / max_support))
        .collect()
}

/// Graf skor yayılımı (H1): adaylar tohumlar ve destek alan komşulardır; skor
/// `(1 − λ)·semantik + λ·destek`, λ = `policy.propagation_weight`.
pub fn propagation_ranking(
    graph: &CodeGraph,
    semantic: &HashMap<String, f32>,
    policy: &RetrievalPolicy,
) -> Vec<RankedCandidate> {
    let support = propagated_support(graph, semantic, policy);
    let weight = policy.propagation_weight;
    let ids: HashSet<&String> = semantic.keys().chain(support.keys()).collect();
    let candidates = ids
        .into_iter()
        .map(|id| {
            let semantic_score = semantic.get(id).copied().unwrap_or(0.0);
            let support_score = support.get(id).copied().unwrap_or(0.0);
            RankedCandidate {
                id: id.clone(),
                semantic_score,
                support: support_score,
                score: (1.0 - weight) * semantic_score + weight * support_score,
            }
        })
        .collect();
    sorted(candidates)
}

/// Bir tohumun her komşusu için iki yöndeki kenarlardan en yüksek policy
/// ağırlığı; öz döngüler sayılmaz.
fn strongest_neighbor_edges(
    graph: &CodeGraph,
    seed: NodeIndex,
    policy: &RetrievalPolicy,
) -> HashMap<NodeIndex, f32> {
    let outgoing = graph
        .graph
        .edges_directed(seed, Direction::Outgoing)
        .map(|edge| (edge.target(), policy.edge_weight(edge.weight())));
    let incoming = graph
        .graph
        .edges_directed(seed, Direction::Incoming)
        .map(|edge| (edge.source(), policy.edge_weight(edge.weight())));
    outgoing
        .chain(incoming)
        .filter(|(neighbor, _)| *neighbor != seed)
        .fold(HashMap::new(), |mut strongest, (neighbor, weight)| {
            strongest
                .entry(neighbor)
                .and_modify(|existing: &mut f32| *existing = existing.max(weight))
                .or_insert(weight);
            strongest
        })
}

/// Skor azalan; eşitlikte semantik skor azalan, son olarak kimlik artan.
fn sorted(mut candidates: Vec<RankedCandidate>) -> Vec<RankedCandidate> {
    candidates.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.semantic_score.total_cmp(&a.semantic_score))
            .then_with(|| a.id.cmp(&b.id))
    });
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CodeNode, EdgeType};

    fn node(id: &str, node_type: NodeType) -> CodeNode {
        CodeNode {
            id: id.to_string(),
            node_type,
            name: id.to_string(),
            content: "".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        }
    }

    fn scores(pairs: &[(&str, f32)]) -> HashMap<String, f32> {
        pairs
            .iter()
            .map(|(id, score)| (id.to_string(), *score))
            .collect()
    }

    fn policy_with_weight(weight: f32) -> RetrievalPolicy {
        RetrievalPolicy {
            propagation_weight: weight,
            ..RetrievalPolicy::baseline()
        }
    }

    fn ids(ranked: &[RankedCandidate]) -> Vec<&str> {
        ranked
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect()
    }

    #[test]
    fn semantic_ranking_orders_by_score_then_id() {
        let ranked = semantic_ranking(&scores(&[("c", 0.5), ("b", 0.7), ("a", 0.5)]));
        assert_eq!(ids(&ranked), vec!["b", "a", "c"]);
        assert!(ranked.iter().all(|candidate| candidate.support == 0.0));
        assert!(ranked
            .iter()
            .all(|candidate| candidate.score == candidate.semantic_score));
    }

    /// Elle hesaplanan örnek (Calls 1.0, Contains 0.8, Imports 0.6, References 0.6):
    /// b = 1.0·0.8 + 1.0·0.4 = 1.2, c = 0.6·0.8 = 0.48, d = max(0.8, 0.6)·0.4 = 0.32,
    /// a = 0.6·0.4 = 0.24; en büyük 1.2 → b 1.0, c 0.4, d 0.2667, a 0.2. File
    /// düğümü f destek almaz, a'nın öz döngüsü sayılmaz.
    #[test]
    fn propagated_support_matches_hand_computed_example() {
        let mut graph = CodeGraph::new();
        let a = graph.add_node(node("a", NodeType::Function));
        let b = graph.add_node(node("b", NodeType::Function));
        let c = graph.add_node(node("c", NodeType::Function));
        let d = graph.add_node(node("d", NodeType::Function));
        let f = graph.add_node(node("f", NodeType::File));
        graph.add_edge(a, b, EdgeType::Calls);
        graph.add_edge(c, b, EdgeType::Calls);
        graph.add_edge(c, d, EdgeType::Contains);
        graph.add_edge(d, c, EdgeType::References);
        graph.add_edge(a, c, EdgeType::Imports);
        graph.add_edge(f, a, EdgeType::Contains);
        graph.add_edge(a, a, EdgeType::Calls);

        let support = propagated_support(
            &graph,
            &scores(&[("a", 0.8), ("c", 0.4)]),
            &RetrievalPolicy::baseline(),
        );

        let expected = [("b", 1.0), ("c", 0.4), ("d", 0.32 / 1.2), ("a", 0.2)];
        assert_eq!(support.len(), expected.len());
        for (id, value) in expected {
            let actual = support.get(id).copied().expect("supported node");
            assert!((actual - value).abs() < 1e-6, "{id}: {actual} != {value}");
        }
        assert!(!support.contains_key("f"));
    }

    /// S sırası p, q, r; q ile r birbirini desteklediği için λ = 0.25'te
    /// q = r = 0.75·0.5 + 0.25·1.0 = 0.625 > p = 0.75·0.6 = 0.45.
    #[test]
    fn propagation_reorders_structurally_connected_seeds() {
        let mut graph = CodeGraph::new();
        graph.add_node(node("p", NodeType::Function));
        let q = graph.add_node(node("q", NodeType::Function));
        let r = graph.add_node(node("r", NodeType::Function));
        graph.add_edge(q, r, EdgeType::Calls);
        let semantic = scores(&[("p", 0.6), ("q", 0.5), ("r", 0.5)]);

        let ranked = propagation_ranking(&graph, &semantic, &policy_with_weight(0.25));
        assert_eq!(ids(&ranked), vec!["q", "r", "p"]);
        assert_eq!(ranked[0].score, 0.625);
        assert_eq!(ranked[0].support, 1.0);

        let without_graph = propagation_ranking(&graph, &semantic, &policy_with_weight(0.0));
        assert_eq!(ids(&without_graph), ids(&semantic_ranking(&semantic)));
    }

    #[test]
    fn propagation_adds_supported_neighbors_as_candidates() {
        let mut graph = CodeGraph::new();
        let seed = graph.add_node(node("seed", NodeType::Function));
        let callee = graph.add_node(node("callee", NodeType::Function));
        graph.add_edge(seed, callee, EdgeType::Calls);

        let ranked =
            propagation_ranking(&graph, &scores(&[("seed", 0.5)]), &policy_with_weight(0.5));
        assert_eq!(ids(&ranked), vec!["callee", "seed"]);
        assert_eq!(ranked[0].semantic_score, 0.0);
        assert_eq!(ranked[0].score, 0.5);
        assert_eq!(ranked[1].score, 0.25);
    }

    #[test]
    fn support_is_empty_without_edges() {
        let mut graph = CodeGraph::new();
        graph.add_node(node("x", NodeType::Function));
        graph.add_node(node("y", NodeType::Function));
        let semantic = scores(&[("x", 0.5), ("y", 0.5)]);

        assert!(propagated_support(&graph, &semantic, &RetrievalPolicy::baseline()).is_empty());
        let ranked = propagation_ranking(&graph, &semantic, &policy_with_weight(0.5));
        assert_eq!(ids(&ranked), vec!["x", "y"]);
        assert_eq!(ranked[0].score, 0.25);
    }

    #[test]
    fn sorted_breaks_ties_by_semantic_then_id() {
        let candidate = |id: &str, semantic_score: f32, score: f32| RankedCandidate {
            id: id.to_string(),
            semantic_score,
            support: 0.0,
            score,
        };
        let ranked = sorted(vec![
            candidate("b", 0.0, 0.5),
            candidate("a", 0.0, 0.5),
            candidate("c", 0.8, 0.5),
            candidate("d", 0.1, 0.9),
        ]);
        assert_eq!(ids(&ranked), vec!["d", "c", "a", "b"]);
    }
}
