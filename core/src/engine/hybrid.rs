use crate::graph::{CodeGraph, EdgeType};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy)]
pub struct HybridWeights {
    pub graph: f32,
    pub semantic: f32,
    pub spatial: f32,
    pub recent: f32,
}

impl Default for HybridWeights {
    fn default() -> Self {
        Self {
            graph: 0.50,
            semantic: 0.35,
            spatial: 0.10,
            recent: 0.05,
        }
    }
}

impl HybridWeights {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        let graph = read_env_f32("CCM_HYBRID_GRAPH_WEIGHT", defaults.graph);
        let semantic = read_env_f32("CCM_HYBRID_SEM_WEIGHT", defaults.semantic);
        let spatial = read_env_f32("CCM_HYBRID_SPATIAL_WEIGHT", defaults.spatial);
        let recent = read_env_f32("CCM_HYBRID_RECENT_WEIGHT", defaults.recent);
        normalize_weights(Self {
            graph,
            semantic,
            spatial,
            recent,
        })
        .unwrap_or(defaults)
    }
}

#[derive(Debug, Clone)]
pub struct HybridScorer {
    pub weights: HybridWeights,
    pub two_hop_decay: f32,
}

impl Default for HybridScorer {
    fn default() -> Self {
        Self::new(HybridWeights::from_env())
    }
}

impl HybridScorer {
    pub fn new(weights: HybridWeights) -> Self {
        Self {
            weights,
            two_hop_decay: 0.60,
        }
    }

    pub fn semantic_score(distance: f32) -> f32 {
        let dist = distance.max(0.0);
        (1.0 / (1.0 + dist)).clamp(0.0, 1.0)
    }

    pub fn combine(&self, graph: f32, semantic: f32, spatial: f32, recent: f32) -> f32 {
        let score = graph * self.weights.graph
            + semantic * self.weights.semantic
            + spatial * self.weights.spatial
            + recent * self.weights.recent;
        score.clamp(0.0, 1.0)
    }

    pub fn confidence(&self, score: f32, top1: f32, top2: f32) -> f32 {
        let margin = (top1 - top2).clamp(0.0, 0.2);
        (score.clamp(0.0, 1.0) * (1.0 + margin)).clamp(0.0, 1.0)
    }

    pub fn edge_weight(&self, edge: &EdgeType) -> f32 {
        match edge {
            EdgeType::Calls => 1.00,
            EdgeType::Inherits => 0.90,
            EdgeType::Defines => 0.85,
            EdgeType::Contains => 0.80,
            EdgeType::Reads => 0.70,
            EdgeType::Writes => 0.70,
            EdgeType::Imports => 0.60,
        }
    }

    pub fn collect_graph_scores(
        &self,
        graph: &CodeGraph,
        seeds: &[String],
    ) -> HashMap<String, f32> {
        let mut scores: HashMap<String, f32> = HashMap::new();
        let mut first_hop: HashSet<usize> = HashSet::new();

        for seed in seeds {
            insert_max(&mut scores, seed.clone(), 1.0);

            let Some(idx) = graph.find_node_index_by_id(seed) else {
                continue;
            };

            for edge in graph.graph.edges_directed(idx, Direction::Outgoing) {
                let weight = self.edge_weight(edge.weight());
                let id = graph.graph[edge.target()].id.clone();
                insert_max(&mut scores, id, weight);
                first_hop.insert(edge.target().index());
            }

            for edge in graph.graph.edges_directed(idx, Direction::Incoming) {
                let weight = self.edge_weight(edge.weight());
                let id = graph.graph[edge.source()].id.clone();
                insert_max(&mut scores, id, weight);
                first_hop.insert(edge.source().index());
            }
        }

        if self.two_hop_decay > 0.0 {
            for idx in first_hop {
                let node_idx = petgraph::graph::NodeIndex::new(idx);

                for edge in graph.graph.edges_directed(node_idx, Direction::Outgoing) {
                    let weight = self.edge_weight(edge.weight()) * self.two_hop_decay;
                    let id = graph.graph[edge.target()].id.clone();
                    insert_max(&mut scores, id, weight);
                }

                for edge in graph.graph.edges_directed(node_idx, Direction::Incoming) {
                    let weight = self.edge_weight(edge.weight()) * self.two_hop_decay;
                    let id = graph.graph[edge.source()].id.clone();
                    insert_max(&mut scores, id, weight);
                }
            }
        }

        scores
    }
}

fn insert_max(scores: &mut HashMap<String, f32>, id: String, value: f32) {
    scores
        .entry(id)
        .and_modify(|existing| {
            if value > *existing {
                *existing = value;
            }
        })
        .or_insert(value);
}

fn read_env_f32(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(default)
}

fn normalize_weights(weights: HybridWeights) -> Option<HybridWeights> {
    let sum = weights.graph + weights.semantic + weights.spatial + weights.recent;
    if sum <= f32::EPSILON {
        return None;
    }
    Some(HybridWeights {
        graph: weights.graph / sum,
        semantic: weights.semantic / sum,
        spatial: weights.spatial / sum,
        recent: weights.recent / sum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CodeNode, EdgeType, NodeType};

    fn node(id: &str) -> CodeNode {
        CodeNode {
            id: id.to_string(),
            node_type: NodeType::Function,
            name: id.to_string(),
            content: String::new(),
            start_line: 1,
            end_line: 1,
        }
    }

    #[test]
    fn semantic_score_is_monotonic() {
        let a = HybridScorer::semantic_score(0.0);
        let b = HybridScorer::semantic_score(1.0);
        let c = HybridScorer::semantic_score(10.0);
        assert!(a > b);
        assert!(b > c);
        assert!(a <= 1.0);
    }

    #[test]
    fn collect_graph_scores_includes_neighbors() {
        let mut graph = CodeGraph::new();
        let a = graph.add_node(node("a"));
        let b = graph.add_node(node("b"));
        let c = graph.add_node(node("c"));
        graph.add_edge(a, b, EdgeType::Calls);
        graph.add_edge(b, c, EdgeType::Contains);

        let scorer = HybridScorer::default();
        let scores = scorer.collect_graph_scores(&graph, &[String::from("a")]);

        assert_eq!(scores.get("a").copied().unwrap_or(0.0), 1.0);
        assert!(scores.get("b").copied().unwrap_or(0.0) >= scorer.edge_weight(&EdgeType::Calls));
        assert!(scores.contains_key("c"));
    }
}
