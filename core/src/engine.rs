pub mod hybrid;

use crate::engine::hybrid::HybridScorer;
use crate::fs_utils::{detect_language, read_text_file_limited};
use crate::git::GitIntegrator;
use crate::graph::{CodeGraph, CodeNode, EdgeType, NodeType};
use crate::normalize_file_id;
use crate::parser::CodeParser;
use crate::parser::SupportedLanguage;
use crate::vector::extractor::Extractor;
use crate::vector::store::LanceDbStore;
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

        // Iterate over all nodes in the graph
        for node in graph.graph.node_weights() {
            // Index Functions, Classes, Structs, and Methods.
            // We consciously exclude 'File' nodes from embeddings in Phase 3 because they are too large
            // and duplicate the content of their children. We want granular retrieval.
            if matches!(
                node.node_type,
                NodeType::Function | NodeType::Method | NodeType::Class | NodeType::Struct
            ) || (embed_data_files && matches!(node.node_type, NodeType::DataFile))
            {
                // Combine name, type, and content for better embedding context
                // Format: "Type: Name \n Content"
                // Content already includes Docstring from Extractor.
                let text_representation =
                    format!("{:?}: {}\n{}", node.node_type, node.name, node.content);

                // Basic Token Limit Check (Approx 1 token ~= 4 chars)
                // If node is > 8000 chars (~2k tokens), we might want to split it further or truncated?
                // For Phase 3, we index it but log a warning if extremely large.
                if text_representation.len() > 8000 {
                    tracing::warn!(
                        node = %node.id,
                        len = text_representation.len(),
                        "Node is very large; retrieval accuracy might degrade"
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
                    tracing::warn!(
                        path = %relative_path,
                        error = %e,
                        "Skipping file during incremental indexing"
                    );
                    stats.files_failed += 1;
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
                    tracing::warn!(
                        path = %relative_path,
                        error = %e,
                        "Skipping file due to parse error"
                    );
                    stats.files_failed += 1;
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
                stats.files_failed += 1;
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
            let texts: Vec<String> = nodes_to_index
                .iter()
                .map(|n| format!("{:?}: {}\n{}", n.node_type, n.name, n.content))
                .collect();

            // Deduplicate?

            self.vector_store.add_documents(ids, texts).await?;
        }

        tracing::info!("Incremental update complete.");

        stats.nodes_created = {
            let graph = self.graph.read().await;
            graph.graph.node_count()
        };

        Ok(stats)
    }

    /// Performs a purely semantic search using vectors.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Vec<ContextSuggestion>> {
        let hits = self.vector_store.search(query, limit).await?;

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
                    relevance_score: 1.0 - score, // Rough normalization
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
        let hits = self.vector_store.search(query, seed_limit).await?;
        let include_data_files = embed_data_files_enabled();

        if hits.is_empty() {
            return Ok(Vec::new());
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
            let combined_score = scorer.combine(graph_score, semantic_score, 0.0, 0.0);
            candidates.push(HybridCandidate {
                id,
                graph_score,
                semantic_score,
                combined_score,
            });
        }

        candidates.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

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
                        "Hybrid (graph {:.2}, semantic {:.2}, conf {:.2})",
                        candidate.graph_score, candidate.semantic_score, confidence
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
                        "Hybrid (semantic {:.2}, conf {:.2})",
                        candidate.semantic_score, confidence
                    ),
                });
            }
        }

        Ok(results)
    }

    /// Retrieves a node from the graph by its ID.
    pub async fn get_node_by_id(&self, id: &str) -> Option<CodeNode> {
        self.graph.read().await.find_node_by_id(id)
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
                content: current_node.content.clone(),
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
                    content: neighbor.content.clone(),
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

fn embed_data_files_enabled() -> bool {
    std::env::var("CCM_EMBED_DATA_FILES")
        .or_else(|_| std::env::var("CCM_EMBED_DATA"))
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}
