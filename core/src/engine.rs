pub mod hybrid;

use crate::engine::hybrid::HybridScorer;
use crate::fs_utils::{detect_language, read_text_file_limited, FileReadError};
use crate::git::GitIntegrator;
use crate::graph::{CodeGraph, CodeNode, EdgeType, NodeType};
use crate::normalize_file_id;
use crate::parser::CodeParser;
use crate::parser::SupportedLanguage;
use crate::vector::extractor::Extractor;
use crate::vector::store::LanceDbStore;
use crate::{
    path_is_policy_excluded, register_issue, suggestion_for_issue, IndexIssue, IndexIssueReason,
};
use anyhow::Result;
use petgraph::visit::EdgeRef;

use tokio::sync::RwLock;

/// Represents the user's cursor position in the editor.
#[derive(Debug, Clone)]
pub struct CursorPosition {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
}

/// A suggested code context item.
#[derive(Debug, Clone)]
pub struct ContextSuggestion {
    pub node_id: Option<String>,
    pub file_path: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub node_type: Option<String>,
    pub title: String,
    pub content: String,
    pub relevance_score: f32,
    pub reason: String,
}

/// Neighbors of a node in the code graph, categorized by relationship.
#[derive(Debug, Clone, Default)]
pub struct NodeNeighbors {
    pub calls: Vec<String>,     // Functions this node calls
    pub called_by: Vec<String>, // Functions that call this node
    pub contains: Vec<String>,  // Child nodes (for files/classes)
}

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// The main intelligence engine for speculative retrieval.
pub struct RetrievalEngine {
    pub graph: Arc<RwLock<CodeGraph>>,
    pub vector_store: LanceDbStore,
}

struct HybridCandidate {
    id: String,
    graph_score: f32,
    semantic_score: f32,
    path_score: f32,
    combined_score: f32,
}

impl RetrievalEngine {
    pub fn new(graph: Arc<RwLock<CodeGraph>>, vector_store: LanceDbStore) -> Self {
        Self {
            graph,
            vector_store,
        }
    }

    /// Indexes the current graph into the vector store.
    /// This should be called after parsing/populating the graph.
    pub async fn index_graph(&self) -> Result<()> {
        let mut ids = Vec::new();
        let mut texts = Vec::new();
        let embed_data_files = embed_data_files_enabled();

        let graph = self.graph.read().await;

        // O(n) over all nodes is unavoidable: every node must be visited exactly once to
        // build its embedding text. No shortcut exists since this is a full-index operation.
        for node in graph.graph.node_weights() {
            // Index Functions, Classes, Structs, and Methods.
            // We consciously exclude 'File' nodes from embeddings in Phase 3 because they are too large
            // and duplicate the content of their children. We want granular retrieval.
            if matches!(
                node.node_type,
                NodeType::Function | NodeType::Method | NodeType::Class | NodeType::Struct
            ) || (embed_data_files && matches!(node.node_type, NodeType::DataFile))
            {
                // Zenginleştirilmiş embedding metni üret
                let text_representation = build_embedding_text(node);

                // Büyük node uyarısı artık build_embedding_text içinde 6000 ile kırpılıyor;
                // ham içerik 8000'i aşıyorsa log bırak.
                if node.content.len() > 8000 {
                    tracing::warn!(
                        node = %node.id,
                        len = node.content.len(),
                        "Node is very large; content truncated for embedding"
                    );
                }

                ids.push(node.id.clone());
                texts.push(text_representation);
            }
        }

        if !ids.is_empty() {
            tracing::info!(count = ids.len(), "Indexing nodes into vector store");
            self.vector_store.add_documents(ids, texts).await?;
        }

        Ok(())
    }

    /// Performs incremental indexing using Git status.
    pub async fn incremental_index(&self, project_root: &str) -> Result<crate::IndexStats> {
        let git = GitIntegrator::new(project_root)?;
        let changed_files = git.get_changed_files()?;

        self.incremental_index_paths(project_root, &changed_files)
            .await
    }

    /// Performs incremental indexing using a provided list of changed files.
    pub async fn incremental_index_paths(
        &self,
        project_root: &str,
        changed_files: &[PathBuf],
    ) -> Result<crate::IndexStats> {
        if changed_files.is_empty() {
            tracing::info!("No changes detected.");
            return Ok(crate::IndexStats::default());
        }

        tracing::info!(
            count = changed_files.len(),
            "Incremental indexing: changed files detected."
        );

        let mut parser = CodeParser::new();
        let mut nodes_to_index = Vec::new(); // Collect new nodes for vector DB
        let mut indexed_node_ids = HashSet::new();
        let embed_data_files = embed_data_files_enabled();
        let mut stats = crate::IndexStats::default();

        let root_path = std::fs::canonicalize(project_root)
            .unwrap_or_else(|_| std::path::PathBuf::from(project_root));

        // Snapshot node count before processing so we can report only NEW nodes.
        let initial_node_count = {
            let graph = self.graph.read().await;
            graph.graph.node_count()
        };

        for path in changed_files {
            // Canonicalize file path to ensure it matches root
            let abs_path = if path.is_absolute() {
                path.clone()
            } else {
                root_path.join(path)
            };
            let abs_path = std::fs::canonicalize(&abs_path).unwrap_or(abs_path);

            let Some(relative_path) = normalize_file_id(&root_path, &abs_path) else {
                continue;
            };
            if path_is_policy_excluded(&abs_path) {
                let issue = IndexIssue {
                    path: relative_path,
                    reason: IndexIssueReason::SkippedByPolicy,
                    detail: "Skipped by default exclude policy".to_string(),
                    suggested_ignore: suggestion_for_issue(
                        &abs_path.to_string_lossy(),
                        &IndexIssueReason::SkippedByPolicy,
                    ),
                };
                register_issue(&mut stats, issue, true);
                continue;
            }

            tracing::debug!(path = %relative_path, "Processing file");

            // 1. GARBAGE COLLECTION: Delete old vectors for this file
            // Now that Extractor uses file-prefixed IDs ("./path/to/file:kind:row..."),
            // we can safely delete all vectors belonging to this file.
            if let Err(e) = self.vector_store.delete_by_prefix(&relative_path).await {
                tracing::warn!(
                    path = %relative_path,
                    error = %e,
                    "Failed to perform vector GC for file"
                );
            }

            {
                // Write Lock Scope: Remove old nodes
                // This removes them from the Graph structure (RAM/Disk)
                let mut graph = self.graph.write().await;
                graph.remove_file_nodes(&relative_path);
            }

            // Parse File (skip if deleted)
            if !abs_path.exists() {
                continue;
            }

            let content = match read_text_file_limited(&abs_path) {
                Ok(content) => content,
                Err(e) => {
                    let issue = classify_incremental_read_error(&relative_path, e);
                    tracing::warn!(
                        path = %relative_path,
                        reason = %issue.reason.as_str(),
                        detail = %issue.detail,
                        "Skipping file during incremental indexing"
                    );
                    register_issue(&mut stats, issue, false);
                    continue;
                }
            };

            let lang = detect_language(&abs_path);

            if matches!(lang, SupportedLanguage::Data) {
                let mut graph = self.graph.write().await;
                let end_line = content.lines().count().max(1);
                let node = CodeNode {
                    id: relative_path.clone(),
                    node_type: NodeType::DataFile,
                    name: relative_path.clone(),
                    content,
                    start_line: 1,
                    end_line,
                };
                let idx = graph.add_node(node);
                if embed_data_files {
                    let node = &graph.graph[idx];
                    if indexed_node_ids.insert(node.id.clone()) {
                        nodes_to_index.push(node.clone());
                    }
                }
                continue;
            }

            let tree = match parser.parse_tree(&content, lang) {
                Ok(tree) => tree,
                Err(e) => {
                    let issue = IndexIssue {
                        path: relative_path,
                        reason: IndexIssueReason::ParseError,
                        detail: e.to_string(),
                        suggested_ignore: suggestion_for_issue(
                            &abs_path.to_string_lossy(),
                            &IndexIssueReason::ParseError,
                        ),
                    };
                    tracing::warn!(
                        path = %issue.path,
                        detail = %issue.detail,
                        "Skipping file due to parse error"
                    );
                    register_issue(&mut stats, issue, false);
                    continue;
                }
            };

            let mut extractor = Extractor::new(content.clone(), lang);

            // Write Lock Scope: Update Graph
            let mut graph = self.graph.write().await;

            if extractor.extract(&tree, &mut graph, &relative_path).is_ok() {
                match extractor.extract_references(&tree, &mut graph, &relative_path) {
                    Ok(edges_created) => {
                        if edges_created > 0 {
                            tracing::debug!(
                                path = %relative_path,
                                edges = edges_created,
                                "Linked call edges"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %relative_path,
                            error = %e,
                            "Failed to extract references during incremental indexing"
                        );
                    }
                }

                // Collect ALL new nodes for this file
                if let Some(file_node_idx) = graph.find_file_node(&relative_path) {
                    let mut stack = vec![file_node_idx];
                    let mut visited = HashSet::new();
                    visited.insert(file_node_idx);

                    while let Some(idx) = stack.pop() {
                        let node = &graph.graph[idx];

                        if (matches!(
                            node.node_type,
                            NodeType::Function
                                | NodeType::Method
                                | NodeType::Class
                                | NodeType::Struct
                        ) || (embed_data_files && matches!(node.node_type, NodeType::DataFile)))
                            && indexed_node_ids.insert(node.id.clone())
                        {
                            nodes_to_index.push(node.clone());
                        }

                        for edge in graph
                            .graph
                            .edges_directed(idx, petgraph::Direction::Outgoing)
                        {
                            if matches!(edge.weight(), EdgeType::Contains) {
                                let neighbor_idx = edge.target();
                                if visited.insert(neighbor_idx) {
                                    stack.push(neighbor_idx);
                                }
                            }
                        }
                    }
                }
                stats.files_indexed += 1;
            } else {
                let issue = IndexIssue {
                    path: relative_path,
                    reason: IndexIssueReason::ExtractError,
                    detail: "AST extraction failed".to_string(),
                    suggested_ignore: suggestion_for_issue(
                        &abs_path.to_string_lossy(),
                        &IndexIssueReason::ExtractError,
                    ),
                };
                register_issue(&mut stats, issue, false);
            }
        }

        // Remove duplicates from nodes_to_index (DFS visits multiple times?)
        // `graph.neighbors` returns unique neighbor indices. But if multiple paths?
        // AST is a Tree. No multi-parent.

        // Batch Index New Nodes
        if !nodes_to_index.is_empty() {
            tracing::info!(
                count = nodes_to_index.len(),
                "Incremental: indexing semantic nodes"
            );
            let ids: Vec<String> = nodes_to_index.iter().map(|n| n.id.clone()).collect();
            let texts: Vec<String> = nodes_to_index.iter().map(build_embedding_text).collect();

            // Deduplicate?

            self.vector_store.add_documents(ids, texts).await?;
        }

        tracing::info!("Incremental update complete.");

        stats.nodes_created = {
            let graph = self.graph.read().await;
            graph.graph.node_count().saturating_sub(initial_node_count)
        };

        Ok(stats)
    }

    /// Performs a purely semantic search using vectors.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Vec<ContextSuggestion>> {
        let hits = match self.vector_store.search(query, limit).await {
            Ok(h) => h,
            Err(e) if e.to_string().contains("Embedder not initialized") => vec![],
            Err(e) => return Err(e),
        };

        let mut results = Vec::new();
        for (id, content, score) in hits {
            // Distance in LanceDB is usually L2 or Cosine distance.
            // Lower is better for L2, higher is better for Cosine similarity.
            // Assuming default L2 for now, we invert/normalize simplified score.

            // 1. Strip chunk suffix if present (e.g., "func:10:20#chunk0" -> "func:10:20")
            let node_id = id.split('#').next().unwrap_or(&id);

            // 2. Lookup the real Node in the Graph to get metadata (Name, Type)
            let mut title = "Semantic Match".to_string();
            if let Some(node) = self.get_node_by_id(node_id).await {
                title = format!("{:?}: {}", node.node_type, node.name);
                results.push(ContextSuggestion {
                    node_id: Some(node.id.clone()),
                    file_path: Some(extract_file_path(&node.id)),
                    start_line: Some(node.start_line),
                    end_line: Some(node.end_line),
                    node_type: Some(format!("{:?}", node.node_type)),
                    title,
                    content,
                    relevance_score: HybridScorer::semantic_score(score),
                    reason: format!("Vector Similarity (Dist: {:.4})", score),
                });
                continue;
            }

            results.push(ContextSuggestion {
                node_id: Some(node_id.to_string()),
                file_path: Some(extract_file_path(node_id)),
                start_line: None,
                end_line: None,
                node_type: None,
                title,
                content,
                relevance_score: 1.0 - score, // Rough normalization
                reason: format!("Vector Similarity (Dist: {:.4})", score),
            });
        }

        Ok(results)
    }

    /// Performs a hybrid search by combining semantic vector hits with structural graph neighbors.
    pub async fn search_code_hybrid(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContextSuggestion>> {
        let seed_limit = limit.saturating_mul(3).max(limit);
        // When the embedder is disabled the vector store returns an error instead of
        // an empty result set. Treat that as a signal to fall through to the graph
        // fallback rather than surfacing an unhelpful "Internal error" to the caller.
        let hits = match self.vector_store.search(query, seed_limit).await {
            Ok(h) => h,
            Err(e) if e.to_string().contains("Embedder not initialized") => vec![],
            Err(e) => return Err(e),
        };
        let include_data_files = embed_data_files_enabled();

        if hits.is_empty() {
            // vector sonuç yoksa graph üzerinden lexical arama ile fallback yap
            return Ok(self.find_graph_nodes(query, limit).await);
        }

        let scorer = HybridScorer::default();
        let mut semantic_scores: HashMap<String, f32> = HashMap::new();
        let mut semantic_content: HashMap<String, String> = HashMap::new();

        for (id, content, distance) in hits {
            let node_id = crate::normalize_node_id(&id);
            let sem_score = HybridScorer::semantic_score(distance);

            match semantic_scores.get_mut(&node_id) {
                Some(existing) => {
                    if sem_score > *existing {
                        *existing = sem_score;
                        semantic_content.insert(node_id.clone(), content);
                    }
                }
                None => {
                    semantic_scores.insert(node_id.clone(), sem_score);
                    semantic_content.insert(node_id, content);
                }
            }
        }

        let seed_ids: Vec<String> = semantic_scores.keys().cloned().collect();
        let graph = self.graph.read().await;
        let graph_scores = scorer.collect_graph_scores(&graph, &seed_ids);

        let mut candidate_ids: HashSet<String> = HashSet::new();
        candidate_ids.extend(semantic_scores.keys().cloned());
        candidate_ids.extend(graph_scores.keys().cloned());

        let mut candidates = Vec::with_capacity(candidate_ids.len());
        for id in candidate_ids {
            let graph_score = graph_scores.get(&id).copied().unwrap_or(0.0);
            let semantic_score = semantic_scores.get(&id).copied().unwrap_or(0.0);
            let spatial_score = graph
                .find_node_by_id(&id)
                .map(|node| repo_priority_score(&extract_file_path(&node.id)))
                .unwrap_or(0.0);
            let combined_score = scorer.combine(graph_score, semantic_score, spatial_score, 0.0);
            candidates.push(HybridCandidate {
                id,
                graph_score,
                semantic_score,
                path_score: spatial_score,
                combined_score,
            });
        }

        candidates.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // düşük sinyalli adayları filtrele — yalnızca combined veya graph sinyali olan adaylar kalır
        let min_combined = min_score_threshold("CCM_MIN_COMBINED_SCORE", 0.05);
        candidates.retain(|c| c.combined_score >= min_combined || c.graph_score > 0.0);

        let mut top_scores = Vec::new();
        for candidate in &candidates {
            if let Some(node) = graph.find_node_by_id(&candidate.id) {
                let is_file_node = matches!(node.node_type, NodeType::File);
                let is_data_node = matches!(node.node_type, NodeType::DataFile);
                if !is_file_node && (include_data_files || !is_data_node) {
                    top_scores.push(candidate.combined_score);
                }
            } else if semantic_content.contains_key(&candidate.id) {
                top_scores.push(candidate.combined_score);
            }

            if top_scores.len() >= 2 {
                break;
            }
        }

        let top1 = top_scores.first().copied().unwrap_or(0.0);
        let top2 = top_scores.get(1).copied().unwrap_or(0.0);

        // docs/hybrid-ranking.md'deki fallback kurallarını uygula
        let low_confidence = top1 < 0.55 || (top1 - top2) < 0.05;
        let fallback_prefix: &str = if low_confidence {
            let top_graph = candidates.first().map(|c| c.graph_score).unwrap_or(0.0);
            let top_semantic = candidates.first().map(|c| c.semantic_score).unwrap_or(0.0);
            if top_graph >= 0.6 && top_graph >= top_semantic {
                // graph sinyali güçlü → graph-ağırlıklı sıralama
                candidates.sort_by(|a, b| {
                    b.graph_score
                        .partial_cmp(&a.graph_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                "GraphFallback "
            } else if top_semantic >= 0.6 {
                // semantic sinyal güçlü → semantic-ağırlıklı sıralama
                candidates.sort_by(|a, b| {
                    b.semantic_score
                        .partial_cmp(&a.semantic_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                "SemanticFallback "
            } else {
                // her iki sinyal de zayıf → hibrit + uyarı
                "LowConf "
            }
        } else {
            ""
        };

        let mut results = Vec::new();
        for candidate in candidates {
            if results.len() >= limit {
                break;
            }

            if let Some(node) = graph.find_node_by_id(&candidate.id) {
                if node.node_type == NodeType::File
                    || (!include_data_files && node.node_type == NodeType::DataFile)
                {
                    continue;
                }

                let confidence = scorer.confidence(candidate.combined_score, top1, top2);
                results.push(ContextSuggestion {
                    node_id: Some(node.id.clone()),
                    file_path: Some(extract_file_path(&node.id)),
                    start_line: Some(node.start_line),
                    end_line: Some(node.end_line),
                    node_type: Some(format!("{:?}", node.node_type)),
                    title: format!("{:?}: {}", node.node_type, node.name),
                    content: node.content.clone(),
                    relevance_score: candidate.combined_score,
                    reason: format!(
                        "{}Hybrid (graph {:.2}, semantic {:.2}, path {:.2}, conf {:.2})",
                        fallback_prefix,
                        candidate.graph_score,
                        candidate.semantic_score,
                        candidate.path_score,
                        confidence
                    ),
                });
            } else if let Some(content) = semantic_content.get(&candidate.id) {
                let confidence = scorer.confidence(candidate.combined_score, top1, top2);
                results.push(ContextSuggestion {
                    node_id: Some(candidate.id.clone()),
                    file_path: Some(extract_file_path(&candidate.id)),
                    start_line: None,
                    end_line: None,
                    node_type: None,
                    title: "Semantic Match".to_string(),
                    content: content.clone(),
                    relevance_score: candidate.combined_score,
                    reason: format!(
                        "{}Hybrid (semantic {:.2}, path {:.2}, conf {:.2})",
                        fallback_prefix, candidate.semantic_score, candidate.path_score, confidence
                    ),
                });
            }
        }

        Ok(results)
    }

    /// Retrieves a node from the graph by its ID.
    /// Kesin eşleşme bulunamazsa yakın satır numaralı aynı tür node'u döndürür (fuzzy).
    pub async fn get_node_by_id(&self, id: &str) -> Option<CodeNode> {
        self.graph.read().await.find_node_fuzzy_by_id(id)
    }

    /// Verilen node'u çağıran / kullanan tüm node'ları döndürür (ters bağlantı analizi).
    pub async fn find_usages(&self, node_id: &str, limit: usize) -> Vec<ContextSuggestion> {
        use petgraph::Direction;

        let graph = self.graph.read().await;

        // Önce fuzzy ile node'u bul
        let target_node = match graph.find_node_fuzzy_by_id(node_id) {
            Some(n) => n,
            None => return Vec::new(),
        };

        let Some(target_idx) = graph.find_node_index_by_id(&target_node.id) else {
            return Vec::new();
        };

        let mut results = Vec::new();

        // Tüm node'ları tara — bu node'a Calls/Imports/Defines kenarı olan kaynaklara bak
        for edge in graph.graph.edges_directed(target_idx, Direction::Incoming) {
            if results.len() >= limit {
                break;
            }
            let source = &graph.graph[edge.source()];
            if matches!(source.node_type, NodeType::File | NodeType::DataFile) {
                continue;
            }
            let rel = match edge.weight() {
                crate::graph::EdgeType::Calls => "Calls",
                crate::graph::EdgeType::Imports => "Imports",
                crate::graph::EdgeType::Defines => "Defines",
                crate::graph::EdgeType::Contains => "Contains",
                crate::graph::EdgeType::Inherits => "Inherits",
                crate::graph::EdgeType::Reads => "Reads",
                crate::graph::EdgeType::Writes => "Writes",
            };
            results.push(ContextSuggestion {
                node_id: Some(source.id.clone()),
                file_path: Some(extract_file_path(&source.id)),
                start_line: Some(source.start_line),
                end_line: Some(source.end_line),
                node_type: Some(format!("{:?}", source.node_type)),
                title: format!("{:?}: {}", source.node_type, source.name),
                content: source.content.clone(),
                relevance_score: 1.0,
                reason: format!("{} → {}", rel, target_node.name),
            });
        }

        results
    }

    /// from_id'den to_id'ye giden çağrı zincirini BFS ile bulur.
    pub async fn trace_call_chain(
        &self,
        from_id: &str,
        to_id: &str,
        max_depth: usize,
    ) -> Vec<ContextSuggestion> {
        use std::collections::VecDeque;

        let graph = self.graph.read().await;

        let from_node = match graph.find_node_fuzzy_by_id(from_id) {
            Some(n) => n,
            None => return Vec::new(),
        };
        let to_node = match graph.find_node_fuzzy_by_id(to_id) {
            Some(n) => n,
            None => return Vec::new(),
        };

        let Some(from_idx) = graph.find_node_index_by_id(&from_node.id) else {
            return Vec::new();
        };
        let Some(to_idx) = graph.find_node_index_by_id(&to_node.id) else {
            return Vec::new();
        };

        // BFS: (current_idx, path_so_far)
        let mut queue: VecDeque<(petgraph::graph::NodeIndex, Vec<petgraph::graph::NodeIndex>)> =
            VecDeque::new();
        let mut visited = std::collections::HashSet::new();
        queue.push_back((from_idx, vec![from_idx]));
        visited.insert(from_idx);

        while let Some((current, path)) = queue.pop_front() {
            if current == to_idx {
                return path
                    .iter()
                    .enumerate()
                    .map(|(i, &idx)| {
                        let node = &graph.graph[idx];
                        ContextSuggestion {
                            node_id: Some(node.id.clone()),
                            file_path: Some(extract_file_path(&node.id)),
                            start_line: Some(node.start_line),
                            end_line: Some(node.end_line),
                            node_type: Some(format!("{:?}", node.node_type)),
                            title: format!("Step {}: {:?} {}", i + 1, node.node_type, node.name),
                            content: node.content.clone(),
                            relevance_score: 1.0 - (i as f32 * 0.05),
                            reason: format!("Call chain step {}/{}", i + 1, path.len()),
                        }
                    })
                    .collect();
            }

            if path.len() >= max_depth {
                continue;
            }

            for edge in graph
                .graph
                .edges_directed(current, petgraph::Direction::Outgoing)
            {
                if matches!(
                    edge.weight(),
                    crate::graph::EdgeType::Calls | crate::graph::EdgeType::Defines
                ) {
                    let neighbor = edge.target();
                    if visited.insert(neighbor) {
                        let mut new_path = path.clone();
                        new_path.push(neighbor);
                        queue.push_back((neighbor, new_path));
                    }
                }
            }
        }

        Vec::new() // yol bulunamadı
    }

    /// Bir dosyanın değişmesi durumunda etkilenecek dosya ve node'ları döndürür.
    pub async fn impact_of_change(&self, file_path: &str, limit: usize) -> Vec<ContextSuggestion> {
        let graph = self.graph.read().await;

        // Dosyaya ait tüm node'ları bul
        let file_node_idx = match graph.find_file_node(file_path) {
            Some(idx) => idx,
            None => {
                // ./prefix ile yeniden dene
                let alt = if file_path.starts_with("./") {
                    file_path.to_string()
                } else {
                    format!("./{}", file_path)
                };
                match graph.find_file_node(&alt) {
                    Some(idx) => idx,
                    None => return Vec::new(),
                }
            }
        };

        let mut file_nodes: std::collections::HashSet<petgraph::graph::NodeIndex> =
            std::collections::HashSet::new();
        let mut stack = vec![file_node_idx];
        file_nodes.insert(file_node_idx);

        while let Some(idx) = stack.pop() {
            for edge in graph
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
            {
                if matches!(edge.weight(), crate::graph::EdgeType::Contains) {
                    let neighbor = edge.target();
                    if file_nodes.insert(neighbor) {
                        stack.push(neighbor);
                    }
                }
            }
        }

        // Bu node'lara bağlanan dış node'ları topla
        let mut impacted: std::collections::HashMap<String, ContextSuggestion> =
            std::collections::HashMap::new();

        for &node_idx in &file_nodes {
            for edge in graph
                .graph
                .edges_directed(node_idx, petgraph::Direction::Incoming)
            {
                let source_idx = edge.source();
                if file_nodes.contains(&source_idx) {
                    continue; // aynı dosya içi bağlantı
                }
                let source = &graph.graph[source_idx];
                if matches!(source.node_type, NodeType::File | NodeType::DataFile) {
                    continue;
                }
                let file = extract_file_path(&source.id);
                impacted.entry(source.id.clone()).or_insert_with(|| {
                    let target = &graph.graph[node_idx];
                    ContextSuggestion {
                        node_id: Some(source.id.clone()),
                        file_path: Some(file),
                        start_line: Some(source.start_line),
                        end_line: Some(source.end_line),
                        node_type: Some(format!("{:?}", source.node_type)),
                        title: format!("{:?}: {}", source.node_type, source.name),
                        content: source.content.clone(),
                        relevance_score: 1.0,
                        reason: format!("Depends on {} in changed file", target.name),
                    }
                });
            }
        }

        let mut results: Vec<ContextSuggestion> = impacted.into_values().collect();
        results.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        results.truncate(limit);
        results
    }

    /// Son N gündeki git commit değişikliklerini ve etkilenen graph node'larını döndürür.
    pub async fn diff_context(
        &self,
        project_root: &str,
        days: u32,
        limit: usize,
    ) -> Vec<ContextSuggestion> {
        let git = match GitIntegrator::new(project_root) {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let changed_files = match git.get_changed_files_since_days(days) {
            Ok(files) => files,
            Err(_) => {
                // fallback: staged/unstaged değişiklikler
                git.get_changed_files().unwrap_or_default()
            }
        };

        if changed_files.is_empty() {
            return Vec::new();
        }

        let graph = self.graph.read().await;
        let mut results = Vec::new();
        let root = std::path::Path::new(project_root);

        for path in &changed_files {
            if results.len() >= limit {
                break;
            }
            // Git returns absolute paths; convert to graph-relative "./path" format
            let rel = match crate::normalize_file_id(root, path) {
                Some(r) => r,
                None => continue,
            };
            if let Some(file_idx) = graph.find_file_node(&rel) {
                let mut stack = vec![file_idx];
                let mut visited = std::collections::HashSet::new();
                visited.insert(file_idx);

                while let Some(idx) = stack.pop() {
                    if results.len() >= limit {
                        break;
                    }
                    let node = &graph.graph[idx];
                    if !matches!(node.node_type, NodeType::File | NodeType::DataFile) {
                        results.push(ContextSuggestion {
                            node_id: Some(node.id.clone()),
                            file_path: Some(extract_file_path(&node.id)),
                            start_line: Some(node.start_line),
                            end_line: Some(node.end_line),
                            node_type: Some(format!("{:?}", node.node_type)),
                            title: format!("{:?}: {}", node.node_type, node.name),
                            content: node.content.clone(),
                            relevance_score: 1.0,
                            reason: format!("Recently changed (last {} days)", days),
                        });
                    }
                    for edge in graph
                        .graph
                        .edges_directed(idx, petgraph::Direction::Outgoing)
                    {
                        if matches!(edge.weight(), crate::graph::EdgeType::Contains) {
                            let neighbor = edge.target();
                            if visited.insert(neighbor) {
                                stack.push(neighbor);
                            }
                        }
                    }
                }
            }
        }

        results
    }

    /// Retrieves neighbors of a node, categorized by relationship type.
    /// Returns (calls: nodes this function calls, called_by: nodes that call this function)
    pub async fn get_node_neighbors(&self, id: &str) -> Option<NodeNeighbors> {
        use crate::graph::EdgeType;
        use petgraph::visit::EdgeRef;
        use petgraph::Direction;

        let mut calls = Vec::new();
        let mut called_by = Vec::new();
        let mut contains = Vec::new();

        let graph = self.graph.read().await;

        // Use the index directly if it exists in RAM graph
        if let Some(idx) = graph.find_node_index_by_id(id) {
            // Outgoing edges
            for edge in graph.graph.edges_directed(idx, Direction::Outgoing) {
                let target_node = &graph.graph[edge.target()];
                match edge.weight() {
                    EdgeType::Calls => calls.push(target_node.name.clone()),
                    EdgeType::Contains => contains.push(target_node.name.clone()),
                    _ => {}
                }
            }

            // Incoming edges
            for edge in graph.graph.edges_directed(idx, Direction::Incoming) {
                let source_node = &graph.graph[edge.source()];
                if let EdgeType::Calls = edge.weight() {
                    called_by.push(source_node.name.clone());
                }
            }
        } else {
            // Fallback to storage-based edges if not in RAM
            let outgoing = graph.get_outgoing_edges(id);
            for (target_id, weight) in outgoing {
                if let Some(node) = graph.find_node_by_id(&target_id) {
                    match weight {
                        EdgeType::Calls => calls.push(node.name.clone()),
                        EdgeType::Contains => contains.push(node.name.clone()),
                        _ => {}
                    }
                }
            }
        }

        Some(NodeNeighbors {
            calls,
            called_by,
            contains,
        })
    }

    pub async fn find_graph_nodes(&self, query: &str, limit: usize) -> Vec<ContextSuggestion> {
        let normalized_query = query.trim().to_lowercase();
        if normalized_query.is_empty() {
            return Vec::new();
        }

        let graph = self.graph.read().await;
        let mut results = Vec::new();

        // O(n) substring scan: unavoidable without a secondary trigram/inverted index.
        // Acceptable in practice — graph is already in RAM and results are capped by limit.
        for node in graph.graph.node_weights() {
            let node_name = node.name.to_lowercase();
            let node_id = node.id.to_lowercase();
            let file_path = extract_file_path(&node.id);

            if node_name.contains(&normalized_query)
                || node_id.contains(&normalized_query)
                || file_path.to_lowercase().contains(&normalized_query)
            {
                results.push(ContextSuggestion {
                    node_id: Some(node.id.clone()),
                    file_path: Some(file_path),
                    start_line: Some(node.start_line),
                    end_line: Some(node.end_line),
                    node_type: Some(format!("{:?}", node.node_type)),
                    title: format!("{:?}: {}", node.node_type, node.name),
                    content: node.content.clone(),
                    relevance_score: 1.0,
                    reason: "Graph node match".to_string(),
                });
            }
        }

        results.sort_by(|a, b| a.title.cmp(&b.title));
        results.truncate(limit);
        results
    }

    /// Predicts relevant code context based on the user's cursor position.
    /// Uses a hybrid approach: Graph (Structural) + Vector (Semantic).
    pub async fn predict_context(&self, cursor: &CursorPosition) -> Result<Vec<ContextSuggestion>> {
        let mut suggestions = Vec::new();

        // 1. Spatial Query: Find which node (Function/Class) the cursor is in.
        let node_idx_opt = self
            .graph
            .read()
            .await
            .find_node_in_file(&cursor.file_path, cursor.line);
        if let Some(node_idx) = node_idx_opt {
            let graph = self.graph.read().await;
            let current_node = &graph.graph[node_idx];

            // Add the current node itself as context
            suggestions.push(ContextSuggestion {
                node_id: Some(current_node.id.clone()),
                file_path: Some(extract_file_path(&current_node.id)),
                start_line: Some(current_node.start_line),
                end_line: Some(current_node.end_line),
                node_type: Some(format!("{:?}", current_node.node_type)),
                title: format!("Current: {}", current_node.name),
                content: focused_content_window(
                    &current_node.content,
                    current_node.start_line,
                    Some(cursor.line),
                    120,
                    12_000,
                ),
                relevance_score: 1.0,
                reason: "Active Focus".to_string(),
            });

            // 2. Structural Retrieval: Find immediate neighbors (Callers/Callees)
            let neighbors = graph.graph.neighbors(node_idx);
            for neighbor_idx in neighbors {
                let neighbor = &graph.graph[neighbor_idx];

                if matches!(neighbor.node_type, NodeType::File | NodeType::DataFile) {
                    continue;
                }

                suggestions.push(ContextSuggestion {
                    node_id: Some(neighbor.id.clone()),
                    file_path: Some(extract_file_path(&neighbor.id)),
                    start_line: Some(neighbor.start_line),
                    end_line: Some(neighbor.end_line),
                    node_type: Some(format!("{:?}", neighbor.node_type)),
                    title: format!("Related: {}", neighbor.name),
                    content: focused_content_window(
                        &neighbor.content,
                        neighbor.start_line,
                        Some(cursor.line),
                        80,
                        8_000,
                    ),
                    relevance_score: 0.8,
                    reason: "Structural Relation".to_string(),
                });
            }

            // 3. Semantic Retrieval (Hybrid): "What else is like this function?"
            // We use the current function's signature/docstring as a query
            let query = format!("related to {}", current_node.name);
            // Limiting to top 2 semantic matches to avoid noise
            if let Ok(semantic_hits) = self.search_code(&query, 2).await {
                for mut hit in semantic_hits {
                    hit.reason = "Semantic Similarity to Active Node".to_string();
                    hit.relevance_score *= 0.7; // Weigh semantic less than structural
                    suggestions.push(hit);
                }
            }
        }

        Ok(suggestions)
    }
}

fn extract_file_path(node_id: &str) -> String {
    node_id
        .rsplitn(4, ':')
        .last()
        .unwrap_or(node_id)
        .to_string()
}

/// Embedding için zenginleştirilmiş metin üretir.
/// Debug format ({:?}) yerine doğal dil kullanır; dosya yolunu ve içeriği dahil eder.
/// Çok büyük içerikleri ~6000 karakter ile kırpar.
fn build_embedding_text(node: &CodeNode) -> String {
    let type_label = match node.node_type {
        NodeType::Function => "function",
        NodeType::Method => "method",
        NodeType::Class => "class",
        NodeType::Struct => "struct",
        NodeType::Module => "module",
        NodeType::Variable => "variable",
        NodeType::Import => "import",
        NodeType::DataFile => "file",
        NodeType::File => "file",
    };
    let file_path = extract_file_path(&node.id);
    let max_embed_chars: usize = std::env::var("CCM_MAX_EMBED_CHARS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6000)
        .max(1);
    let content = if node.content.len() > max_embed_chars {
        // Walk back to a valid char boundary: direct byte-slice panics on multi-byte chars.
        let mut end = max_embed_chars;
        while end > 0 && !node.content.is_char_boundary(end) {
            end -= 1;
        }
        &node.content[..end]
    } else {
        &node.content
    };
    format!(
        "{} {}\nfile: {}\n{}",
        type_label, node.name, file_path, content
    )
}

fn repo_priority_score(file_path: &str) -> f32 {
    let normalized = file_path.to_ascii_lowercase();
    let mut score = 0.5_f32;

    if normalized.contains("/src/")
        || normalized.contains("/app/")
        || normalized.contains("/frontend/")
        || normalized.contains("/backend/")
        || normalized.contains("/server/")
        || normalized.contains("/cmd/")
        || normalized.contains("/go-scraper/")
    {
        score += 0.30;
    }

    if normalized.ends_with("/main.rs")
        || normalized.ends_with("/main.py")
        || normalized.ends_with("/main.ts")
        || normalized.ends_with("/main.js")
        || normalized.ends_with("/index.ts")
        || normalized.ends_with("/index.js")
    {
        score += 0.20;
    }

    if normalized.contains("/scripts/")
        || normalized.contains("/examples/")
        || normalized.contains("/docs/")
        || normalized.contains("/test/")
        || normalized.contains("/tests/")
        || normalized.contains("/fixtures/")
    {
        score -= 0.40;
    }

    if normalized.contains("/node_modules/")
        || normalized.contains("/target/")
        || normalized.contains("/dist/")
        || normalized.contains("/build/")
        || normalized.contains("/vendor/")
    {
        score -= 0.50;
    }

    score.clamp(0.0, 1.0)
}

fn focused_content_window(
    content: &str,
    section_start_line: usize,
    focus_line: Option<usize>,
    max_lines: usize,
    max_chars: usize,
) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let focus_index = focus_line
        .map(|line| line.saturating_sub(section_start_line))
        .unwrap_or(0)
        .min(lines.len().saturating_sub(1));

    let half = max_lines / 2;
    let mut start = focus_index.saturating_sub(half);
    let end = (start + max_lines).min(lines.len());
    if end - start < max_lines {
        start = end.saturating_sub(max_lines);
    }

    let slice = lines[start..end].join("\n");
    if slice.len() <= max_chars {
        return slice;
    }

    let mut end_idx = max_chars;
    while !slice.is_char_boundary(end_idx) {
        end_idx = end_idx.saturating_sub(1);
        if end_idx == 0 {
            break;
        }
    }

    format!(
        "{}\n\n... [context window truncated: showing focused section around line {}]",
        &slice[..end_idx],
        focus_line.unwrap_or(section_start_line)
    )
}

fn min_score_threshold(env_var: &str, default: f32) -> f32 {
    std::env::var(env_var)
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(default)
}

fn embed_data_files_enabled() -> bool {
    std::env::var("CCM_EMBED_DATA_FILES")
        .or_else(|_| std::env::var("CCM_EMBED_DATA"))
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn classify_incremental_read_error(path: &str, error: anyhow::Error) -> IndexIssue {
    match error.downcast::<FileReadError>() {
        Ok(file_error) => match file_error {
            FileReadError::TooLarge {
                size_bytes,
                limit_bytes,
                ..
            } => IndexIssue {
                path: path.to_string(),
                reason: IndexIssueReason::FileTooLarge,
                detail: format!(
                    "File is too large ({} bytes > {} bytes)",
                    size_bytes, limit_bytes
                ),
                suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::FileTooLarge),
            },
            FileReadError::BinaryNul { .. } => IndexIssue {
                path: path.to_string(),
                reason: IndexIssueReason::BinaryFile,
                detail: "Binary file detected (contains NUL bytes)".to_string(),
                suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::BinaryFile),
            },
            FileReadError::NonUtf8 { source, .. } => IndexIssue {
                path: path.to_string(),
                reason: IndexIssueReason::NonUtf8File,
                detail: format!("File is not UTF-8 text: {}", source),
                suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::NonUtf8File),
            },
            FileReadError::Metadata { source, .. } => IndexIssue {
                path: path.to_string(),
                reason: IndexIssueReason::MetadataError,
                detail: format!("Failed to read file metadata: {}", source),
                suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::MetadataError),
            },
            FileReadError::Read { source, .. } => IndexIssue {
                path: path.to_string(),
                reason: IndexIssueReason::ReadError,
                detail: format!("Failed to read file content: {}", source),
                suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::ReadError),
            },
        },
        Err(other) => IndexIssue {
            path: path.to_string(),
            reason: IndexIssueReason::ReadError,
            detail: other.to_string(),
            suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::ReadError),
        },
    }
}
