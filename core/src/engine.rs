pub mod hybrid;

use crate::engine::hybrid::HybridScorer;
use crate::fs_utils::{detect_language, read_text_file_limited, FileReadError};
use crate::git::GitIntegrator;
use crate::graph::{CodeGraph, CodeNode, NodeType};
use crate::normalize_file_id_with_root;
use crate::parser::CodeParser;
use crate::parser::SupportedLanguage;
use crate::policy::RetrievalPolicy;
use crate::trajectory::{current_context, record_if_enabled, RetrievalEvent, RetrievalResultItem};
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
    pub active_policy: RetrievalPolicy,
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
            active_policy: RetrievalPolicy::baseline().with_env_overrides(),
        }
    }

    /// Runtime için policy store'dan aktif policy'yi yükler; store yoksa veya
    /// aktif policy yoksa baseline kullanılır. Böylece promote edilen policy
    /// gerçek arama yoluna uygulanır (H1 düzeltmesi).
    pub fn new_with_active_policy(
        graph: Arc<RwLock<CodeGraph>>,
        vector_store: LanceDbStore,
        store_path: Option<&std::path::Path>,
    ) -> Self {
        let mut policy = RetrievalPolicy::baseline();
        if let Some(path) = store_path {
            match crate::policy::store::PolicyStore::load(path) {
                Ok(store) => {
                    let active = store.active();
                    if active.version != 1 {
                        tracing::info!(
                            version = active.version,
                            task_type = %active.task_type.as_str(),
                            "Aktif retrieval policy yüklendi"
                        );
                        policy = active.clone();
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        path = %path.display(),
                        "Policy store yüklenemedi; baseline kullanılıyor"
                    );
                }
            }
        }
        policy = policy.with_env_overrides();
        Self {
            graph,
            vector_store,
            active_policy: policy,
        }
    }

    /// Belirli bir policy ile engine kurar (evaluation/optimizer yolu).
    pub fn with_policy(
        graph: Arc<RwLock<CodeGraph>>,
        vector_store: LanceDbStore,
        policy: RetrievalPolicy,
    ) -> Self {
        Self {
            graph,
            vector_store,
            active_policy: policy,
        }
    }

    /// Indexes the current graph into the vector store.
    /// This should be called after parsing/populating the graph.
    pub async fn index_graph(&self) -> Result<()> {
        let embed_data_files = embed_data_files_enabled();

        // Node clone'ları Arc içerik taşır; pahalı metin/embedding üretimi lock dışında yapılır.
        let nodes = {
            let graph = self.graph.read().await;
            let mut nodes = Vec::new();

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
                    nodes.push(node.clone());
                }
            }

            nodes
        };

        self.index_nodes_in_bounded_batches(&nodes).await
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

            let Some(relative_path) = normalize_file_id_with_root(&root_path, &abs_path) else {
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

            // Silinen dosya için yeni içerik hazırlanamayacağı için doğrudan kaldırılır.
            if !abs_path.exists() {
                self.vector_store
                    .delete_by_prefix(&relative_path)
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "Failed to delete vectors for removed file '{}': {}",
                            relative_path,
                            error
                        )
                    })?;
                self.graph.write().await.remove_file_nodes(&relative_path);
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
            let mut staged_graph = CodeGraph::new();

            if matches!(lang, SupportedLanguage::Data) {
                let end_line = content.lines().count().max(1);
                let node = CodeNode {
                    id: relative_path.clone(),
                    node_type: NodeType::DataFile,
                    name: relative_path.clone(),
                    content: content.into(),
                    start_line: 1,
                    end_line,
                };
                staged_graph.add_node(node);
            } else {
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
                if let Err(error) = extractor.extract(&tree, &mut staged_graph, &relative_path) {
                    let issue = IndexIssue {
                        path: relative_path,
                        reason: IndexIssueReason::ExtractError,
                        detail: error.to_string(),
                        suggested_ignore: suggestion_for_issue(
                            &abs_path.to_string_lossy(),
                            &IndexIssueReason::ExtractError,
                        ),
                    };
                    register_issue(&mut stats, issue, false);
                    continue;
                }
            }

            for node in staged_graph.graph.node_weights() {
                if (matches!(
                    node.node_type,
                    NodeType::Function | NodeType::Method | NodeType::Class | NodeType::Struct
                ) || (embed_data_files && matches!(node.node_type, NodeType::DataFile)))
                    && indexed_node_ids.insert(node.id.clone())
                {
                    nodes_to_index.push(node.clone());
                }
            }

            self.vector_store
                .delete_by_prefix(&relative_path)
                .await
                .map_err(|error| {
                    anyhow::anyhow!(
                        "Failed to replace vectors for '{}': {}",
                        relative_path,
                        error
                    )
                })?;
            let mut graph = self.graph.write().await;
            graph.remove_file_nodes(&relative_path);
            graph.append_graph(&staged_graph);
            stats.files_indexed += 1;
        }

        {
            let mut graph = self.graph.write().await;
            let reference_edges = graph.rebuild_reference_edges();
            tracing::info!(
                edges = reference_edges,
                "Rebuilt references after incremental update"
            );
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
            self.index_nodes_in_bounded_batches(&nodes_to_index).await?;
        }

        tracing::info!("Incremental update complete.");

        stats.nodes_created = {
            let graph = self.graph.read().await;
            graph.graph.node_count().saturating_sub(initial_node_count)
        };

        Ok(stats)
    }

    async fn index_nodes_in_bounded_batches(&self, nodes: &[CodeNode]) -> Result<()> {
        if nodes.is_empty() {
            return Ok(());
        }
        let batch_size = std::env::var("CCM_INDEX_NODE_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(128)
            .clamp(1, 4_096);
        let batch_bytes = std::env::var("CCM_INDEX_BATCH_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(4 * 1024 * 1024)
            .clamp(64 * 1024, 64 * 1024 * 1024);
        tracing::info!(
            count = nodes.len(),
            batch_size,
            batch_bytes,
            "Indexing nodes into vector store"
        );

        let mut start = 0usize;
        let mut batch_index = 0usize;
        while start < nodes.len() {
            let mut end = start;
            let mut bytes = 0usize;
            while end < nodes.len() && end - start < batch_size {
                let node_bytes = nodes[end].content.len().saturating_add(nodes[end].id.len());
                if end > start && bytes.saturating_add(node_bytes) > batch_bytes {
                    break;
                }
                bytes = bytes.saturating_add(node_bytes);
                end += 1;
            }
            let batch = &nodes[start..end];
            let ids = batch.iter().map(|node| node.id.clone()).collect();
            let texts = batch.iter().map(build_embedding_text).collect();
            self.vector_store.add_documents(ids, texts).await?;
            if batch_index.is_multiple_of(20) || end == nodes.len() {
                tracing::info!(
                    batch = batch_index + 1,
                    indexed = end,
                    total = nodes.len(),
                    "Vector indexing batch progress"
                );
            }
            start = end;
            batch_index += 1;
        }
        Ok(())
    }

    /// Performs a purely semantic search using vectors.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Vec<ContextSuggestion>> {
        let hits = match self.vector_store.search(query, limit).await {
            Ok(h) => h,
            Err(e) if e.to_string().contains("Embedder not initialized") => vec![],
            Err(e) => return Err(e),
        };

        let mut results = Vec::new();
        let mut seen_node_ids: HashSet<String> = HashSet::new();
        let graph = self.graph.read().await;
        for (id, content, score) in hits {
            // Distance in LanceDB is usually L2 or Cosine distance.
            // Lower is better for L2, higher is better for Cosine similarity.
            // Assuming default L2 for now, we invert/normalize simplified score.

            // 1. Strip chunk suffix if present (e.g., "func:10:20#chunk0" -> "func:10:20")
            let node_id = id.split('#').next().unwrap_or(&id);

            // Aynı node'un birden fazla chunk'ı sonuç kümesini doldurmasın:
            // en yüksek benzerlikli chunk kazanır (search_code_hybrid ile aynı kural).
            if !seen_node_ids.insert(node_id.to_string()) {
                continue;
            }

            // 2. Lookup the real Node in the Graph to get metadata (Name, Type)
            let mut title = "Semantic Match".to_string();
            if let Some(node) = graph.find_node_fuzzy_by_id(node_id) {
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
        let started = std::time::Instant::now();
        let seed_multiplier = self.active_policy.seed_multiplier as usize;
        let seed_limit = limit.saturating_mul(seed_multiplier).max(limit);
        // Embedder kapalıyken vector store boş sonuç yerine hata döndürür.
        // Bu durumda çağırana anlamsız bir iç hata taşımak yerine graph fallback'i kullanılır.
        let hits = match self.vector_store.search(query, seed_limit).await {
            Ok(h) => h,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("Embedder not initialized")
                    || msg.contains("was not found")
                    || msg.contains("not found")
                    || msg.contains("DatasetNotFound")
                {
                    vec![]
                } else {
                    return Err(e);
                }
            }
        };
        let include_data_files = embed_data_files_enabled();

        if hits.is_empty() {
            // vector sonuç yoksa graph üzerinden lexical arama ile fallback yap
            let results = self.find_graph_nodes(query, limit).await;
            record_search_event(
                query,
                &results,
                self.active_policy.version,
                self.active_policy.task_type,
                started.elapsed().as_millis() as u64,
            );
            return Ok(results);
        }

        let scorer = HybridScorer::from_policy(&self.active_policy);
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
                .then_with(|| a.id.cmp(&b.id))
        });

        // düşük sinyalli adayları filtrele — yalnızca combined veya graph sinyali olan adaylar kalır
        candidates.retain(|c| c.combined_score >= scorer.min_score || c.graph_score > 0.0);

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

        // docs/hybrid-ranking.md'deki fallback kurallarını uygula.
        // Policy `fallback_enabled=false` ise (optimizer eval'i) fallback atlanır.
        let low_confidence =
            top1 < scorer.confidence_threshold || (top1 - top2) < scorer.confidence_margin;
        let fallback_prefix: &str = if low_confidence && self.active_policy.fallback_enabled {
            let top_graph = candidates.first().map(|c| c.graph_score).unwrap_or(0.0);
            let top_semantic = candidates.first().map(|c| c.semantic_score).unwrap_or(0.0);
            if top_graph >= 0.6 && top_graph >= top_semantic {
                // graph sinyali güçlü → graph-ağırlıklı sıralama
                candidates.sort_by(|a, b| {
                    b.graph_score
                        .partial_cmp(&a.graph_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.id.cmp(&b.id))
                });
                "GraphFallback "
            } else if top_semantic >= 0.6 {
                // semantic sinyal güçlü → semantic-ağırlıklı sıralama
                candidates.sort_by(|a, b| {
                    b.semantic_score
                        .partial_cmp(&a.semantic_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.id.cmp(&b.id))
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
                    content: node.content.to_string(),
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

        record_search_event(
            query,
            &results,
            self.active_policy.version,
            self.active_policy.task_type,
            started.elapsed().as_millis() as u64,
        );
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
                crate::graph::EdgeType::CallAmbiguous => "Calls (ambiguous name match)",
                crate::graph::EdgeType::Imports => "Imports",
                crate::graph::EdgeType::ImportAmbiguous => "Imports (ambiguous name match)",
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
                content: source.content.to_string(),
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

        // BFS: parent-pointer tabanlı (path clone'u yok → bellek patlaması önlenir).
        // Bulunan yoldaki her düğüm için parent haritada tutulur, sonunda geri çözülür.
        let mut queue: VecDeque<petgraph::graph::NodeIndex> = VecDeque::new();
        let mut visited = std::collections::HashSet::new();
        let mut parent: std::collections::HashMap<
            petgraph::graph::NodeIndex,
            petgraph::graph::NodeIndex,
        > = std::collections::HashMap::new();
        queue.push_back(from_idx);
        visited.insert(from_idx);

        let mut found: Option<petgraph::graph::NodeIndex> = None;
        while let Some(current) = queue.pop_front() {
            if current == to_idx {
                found = Some(current);
                break;
            }

            if parent.len() >= max_depth && current != from_idx {
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
                        parent.insert(neighbor, current);
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        let Some(found) = found else {
            return Vec::new();
        };

        // Parent zincirini geri çöz ve path'i baştan sona sırala.
        let mut path: Vec<petgraph::graph::NodeIndex> = Vec::new();
        let mut cursor = found;
        loop {
            path.push(cursor);
            match parent.get(&cursor) {
                Some(prev) => cursor = *prev,
                None => break,
            }
        }
        path.reverse();

        path.iter()
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
                    content: node.content.to_string(),
                    relevance_score: 1.0 - (i as f32 * 0.05),
                    reason: format!("Call chain step {}/{}", i + 1, path.len()),
                }
            })
            .collect()
    }

    /// Bir dosyanın değişmesi durumunda etkilenecek dosya ve node'ları döndürür.
    pub async fn impact_of_change(&self, file_path: &str, limit: usize) -> Vec<ContextSuggestion> {
        use std::collections::VecDeque;

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

        // Walk incoming dependency edges breadth-first so direct dependents stay
        // ahead of transitive callers while the blast radius remains bounded.
        const MAX_IMPACT_DEPTH: usize = 3;
        let mut impacted_ids = std::collections::HashSet::new();
        let mut visited = file_nodes.clone();
        let mut initial_nodes: Vec<_> = file_nodes.iter().copied().collect();
        initial_nodes.sort_by_key(|idx| idx.index());
        let mut queue: VecDeque<(petgraph::graph::NodeIndex, usize)> =
            initial_nodes.into_iter().map(|idx| (idx, 0)).collect();
        let mut results = Vec::new();

        while let Some((node_idx, depth)) = queue.pop_front() {
            if results.len() >= limit {
                break;
            }
            for edge in graph
                .graph
                .edges_directed(node_idx, petgraph::Direction::Incoming)
            {
                if !matches!(
                    edge.weight(),
                    crate::graph::EdgeType::Calls
                        | crate::graph::EdgeType::Imports
                        | crate::graph::EdgeType::Defines
                        | crate::graph::EdgeType::Inherits
                        | crate::graph::EdgeType::Reads
                        | crate::graph::EdgeType::Writes
                ) {
                    continue;
                }
                let source_idx = edge.source();
                if file_nodes.contains(&source_idx) {
                    continue;
                }
                let source = &graph.graph[source_idx];
                if matches!(source.node_type, NodeType::File | NodeType::DataFile) {
                    continue;
                }
                let file = extract_file_path(&source.id);
                if impacted_ids.insert(source.id.clone()) {
                    let target = &graph.graph[node_idx];
                    results.push(ContextSuggestion {
                        node_id: Some(source.id.clone()),
                        file_path: Some(file),
                        start_line: Some(source.start_line),
                        end_line: Some(source.end_line),
                        node_type: Some(format!("{:?}", source.node_type)),
                        title: format!("{:?}: {}", source.node_type, source.name),
                        content: source.content.to_string(),
                        relevance_score: (1.0 - depth as f32 * 0.15).max(0.55),
                        reason: if depth == 0 {
                            format!("Directly depends on {} in changed file", target.name)
                        } else {
                            format!(
                                "Transitively depends on changed file via {} (depth {})",
                                target.name,
                                depth + 1
                            )
                        },
                    });
                }
                if depth < MAX_IMPACT_DEPTH && visited.insert(source_idx) {
                    queue.push_back((source_idx, depth + 1));
                }
                if results.len() >= limit {
                    break;
                }
            }
        }

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

        let recent_files = git.get_changed_files_since_days(days).unwrap_or_default();
        let worktree_files = git.get_changed_files().unwrap_or_default();
        let mut changed_files: std::collections::HashMap<PathBuf, (bool, bool)> =
            std::collections::HashMap::new();
        for path in recent_files {
            changed_files.entry(path).or_default().0 = true;
        }
        for path in worktree_files {
            changed_files.entry(path).or_default().1 = true;
        }

        if changed_files.is_empty() {
            return Vec::new();
        }

        let graph = self.graph.read().await;
        let mut results = Vec::new();
        // Root bir kez canonicalize edilir; döngü içinde tekrarlanmaz.
        let root = std::fs::canonicalize(project_root)
            .unwrap_or_else(|_| std::path::PathBuf::from(project_root));

        let mut changed_files: Vec<(PathBuf, (bool, bool))> = changed_files.into_iter().collect();
        changed_files.sort_by(|(path_a, _), (path_b, _)| path_a.cmp(path_b));

        for (path, (recent, worktree)) in &changed_files {
            if results.len() >= limit {
                break;
            }
            // Git returns absolute paths; convert to graph-relative "./path" format
            let rel = match normalize_file_id_with_root(&root, path) {
                Some(r) => r,
                None => continue,
            };
            let reason = match (*recent, *worktree) {
                (true, true) => format!(
                    "Changed in the last {} days and in the current worktree",
                    days
                ),
                (true, false) => format!("Changed in the last {} days", days),
                (false, true) => {
                    "Changed in the current worktree (including untracked)".to_string()
                }
                (false, false) => continue,
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
                            content: node.content.to_string(),
                            relevance_score: 1.0,
                            reason: reason.clone(),
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
        let mut scored_results = Vec::new();
        let file_targeted = normalized_query.contains('/')
            || normalized_query.contains('\\')
            || normalized_query.contains('.');

        // O(n) substring scan: unavoidable without a secondary trigram/inverted index.
        // Acceptable in practice — graph is already in RAM and results are capped by limit.
        for node in graph.graph.node_weights() {
            let node_name = node.name.to_lowercase();
            let node_id = node.id.to_lowercase();
            let file_path = extract_file_path(&node.id);
            let file_path_lower = file_path.to_lowercase();

            // İçeriği küçük harfe çevirmek pahalıdır; önce ucuz alanlar taranır.
            // Skorlama mantığında içerik yalnızca name/path eşleşmesi yokken
            // kullanıldığı için, eşleşme varsa içerik hiç dönüştürülmez.
            let cheap_match = node_name.contains(&normalized_query)
                || file_path_lower.contains(&normalized_query)
                || node_id.contains(&normalized_query);
            let content_lower_storage = if cheap_match {
                None
            } else {
                Some(node.content.to_lowercase())
            };
            let content_lower = content_lower_storage.as_deref().unwrap_or("");

            if let Some(score) = lexical_graph_score(
                &normalized_query,
                &node_name,
                &node_id,
                &file_path_lower,
                content_lower,
                &node.node_type,
                file_targeted,
            ) {
                scored_results.push((
                    score,
                    ContextSuggestion {
                        node_id: Some(node.id.clone()),
                        file_path: Some(file_path),
                        start_line: Some(node.start_line),
                        end_line: Some(node.end_line),
                        node_type: Some(format!("{:?}", node.node_type)),
                        title: format!("{:?}: {}", node.node_type, node.name),
                        content: node.content.to_string(),
                        relevance_score: (score / 100.0).clamp(0.0, 1.0),
                        reason: format!("Ranked lexical graph match ({score:.0})"),
                    },
                ));
            }
        }

        scored_results.sort_by(|(score_a, result_a), (score_b, result_b)| {
            score_b
                .total_cmp(score_a)
                .then_with(|| result_a.title.cmp(&result_b.title))
        });
        let mut results: Vec<ContextSuggestion> = scored_results
            .into_iter()
            .map(|(_, result)| result)
            .collect();
        results.truncate(limit);
        results
    }

    /// Predicts relevant code context based on the user's cursor position.
    /// Uses a hybrid approach: Graph (Structural) + Vector (Semantic).
    pub async fn predict_context(&self, cursor: &CursorPosition) -> Result<Vec<ContextSuggestion>> {
        let started = std::time::Instant::now();
        let mut suggestions = Vec::new();

        // 1. Spatial Query + 2. Structural Retrieval tek lock scope'unda yapılır.
        // node_idx lock bırakıldıktan sonra ikinci bir lock'ta kullanılırsa araya
        // giren bir write (incremental index) index'i geçersiz kılıp panic'e yol açar.
        let semantic_query = {
            let graph = self.graph.read().await;
            let Some(node_idx) = graph.find_node_in_file(&cursor.file_path, cursor.line) else {
                return Ok(suggestions);
            };
            let current_node = &graph.graph[node_idx];
            let scope_indices = graph.find_enclosing_scopes(&cursor.file_path, cursor.line);
            let primary_idx = scope_indices.first().copied().unwrap_or(node_idx);
            let primary_node = &graph.graph[primary_idx];

            // Prefer the enclosing function/class over a leaf assignment so the
            // caller receives a coherent implementation unit.
            suggestions.push(ContextSuggestion {
                node_id: Some(primary_node.id.clone()),
                file_path: Some(extract_file_path(&primary_node.id)),
                start_line: Some(primary_node.start_line),
                end_line: Some(primary_node.end_line),
                node_type: Some(format!("{:?}", primary_node.node_type)),
                title: format!("Current: {}", primary_node.name),
                content: focused_content_window(
                    &primary_node.content,
                    primary_node.start_line,
                    Some(cursor.line),
                    self.active_policy.primary_window_lines,
                    self.active_policy.primary_window_chars,
                ),
                relevance_score: 1.0,
                reason: "Enclosing semantic scope".to_string(),
            });

            if node_idx != primary_idx {
                suggestions.push(ContextSuggestion {
                    node_id: Some(current_node.id.clone()),
                    file_path: Some(extract_file_path(&current_node.id)),
                    start_line: Some(current_node.start_line),
                    end_line: Some(current_node.end_line),
                    node_type: Some(format!("{:?}", current_node.node_type)),
                    title: format!("Active element: {}", current_node.name),
                    content: current_node.content.to_string(),
                    relevance_score: 0.95,
                    reason: "Exact cursor element".to_string(),
                });
            }

            // 2. Structural Retrieval: Find immediate neighbors (Callers/Callees)
            let neighbors = graph.graph.neighbors(primary_idx);
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
                        self.active_policy.related_window_lines,
                        self.active_policy.related_window_chars,
                    ),
                    relevance_score: 0.8,
                    reason: "Structural Relation".to_string(),
                });
            }

            // 3. Semantic sorgu için aktif node'un adı saklanır.
            format!("related to {}", primary_node.name)
        };

        // Semantic Retrieval (Hybrid): "What else is like this function?"
        // Lock bırakıldıktan sonra çalıştırılır; search_code kendi read lock'unu alır.
        // Limiting semantic matches to the policy budget to avoid noise
        if let Ok(semantic_hits) = self
            .search_code(&semantic_query, self.active_policy.semantic_hits)
            .await
        {
            for mut hit in semantic_hits {
                hit.reason = "Semantic Similarity to Active Node".to_string();
                hit.relevance_score *= self.active_policy.semantic_discount; // Weigh semantic less than structural
                suggestions.push(hit);
            }
        }

        record_predict_event(
            cursor,
            &suggestions,
            self.active_policy.version,
            self.active_policy.task_type,
            started.elapsed().as_millis() as u64,
        );
        Ok(suggestions)
    }
}

pub(crate) fn extract_file_path(node_id: &str) -> String {
    if let Some((path_and_kind, _)) = node_id.split_once(":symbol:") {
        if let Some((path, _kind)) = path_and_kind.rsplit_once(':') {
            return path.to_string();
        }
    }
    node_id
        .rsplitn(4, ':')
        .last()
        .unwrap_or(node_id)
        .to_string()
}

fn lexical_graph_score(
    query: &str,
    node_name: &str,
    node_id: &str,
    file_path: &str,
    content: &str,
    node_type: &NodeType,
    file_targeted: bool,
) -> Option<f32> {
    let name_match = node_name.contains(query);
    let path_match = file_path.contains(query) || node_id.contains(query);
    let content_match = content.contains(query);
    if !name_match && !path_match && !content_match {
        return None;
    }

    let mut score = if node_name == query {
        100.0
    } else if node_name.starts_with(query) {
        88.0
    } else if name_match {
        78.0
    } else if content_match && !matches!(node_type, NodeType::File | NodeType::DataFile) {
        60.0
    } else if file_targeted {
        68.0
    } else {
        48.0
    };

    if matches!(
        node_type,
        NodeType::Function | NodeType::Method | NodeType::Class | NodeType::Struct
    ) {
        score += 8.0;
    }
    if !file_targeted && matches!(node_type, NodeType::File | NodeType::DataFile) {
        score -= 35.0;
    }

    Some(score)
}

/// Embedding için zenginleştirilmiş metin üretir.
/// Debug format ({:?}) yerine doğal dil kullanır; dosya yolunu ve içeriği dahil eder.
/// Büyük içerikler daha sonra semantik sınırlarda parçalara ayrılır.
pub(crate) fn build_embedding_text(node: &CodeNode) -> String {
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
    format!(
        "{} {}\nfile: {}\n{}",
        type_label, node.name, file_path, node.content
    )
}

pub(crate) fn repo_priority_score(file_path: &str) -> f32 {
    let normalized = file_path.to_ascii_lowercase();
    let mut score = 0.5_f32;

    if normalized.contains("/src/")
        || normalized.contains("/app/")
        || normalized.contains("/frontend/")
        || normalized.contains("/backend/")
        || normalized.contains("/server/")
        || normalized.contains("/cmd/")
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

fn embed_data_files_enabled() -> bool {
    std::env::var("CCM_EMBED_DATA_FILES")
        .or_else(|_| std::env::var("CCM_EMBED_DATA"))
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn record_search_event(
    query: &str,
    results: &[ContextSuggestion],
    policy_version: u32,
    task_type: crate::policy::TaskType,
    latency_ms: u64,
) {
    let items: Vec<RetrievalResultItem> = results
        .iter()
        .enumerate()
        .map(|(rank, result)| RetrievalResultItem {
            node_id: result.node_id.clone(),
            file_path: result.file_path.clone(),
            relevance_score: result.relevance_score,
            rank: rank + 1,
        })
        .collect();
    let estimated_tokens: usize = results.iter().map(|result| result.content.len() / 4).sum();
    let trajectory_context = current_context();
    record_if_enabled(RetrievalEvent {
        session_id: std::env::var("CCM_TRAJECTORY_SESSION").unwrap_or_else(|_| "cli".to_string()),
        tool_name: trajectory_context
            .as_ref()
            .map(|context| context.tool_name.clone()),
        request_id: trajectory_context.and_then(|context| context.request_id),
        task_type,
        policy_version,
        query: Some(query.to_string()),
        cursor: None,
        results: items,
        estimated_tokens,
        latency_ms,
        timestamp_ms: crate::trajectory::now_ms(),
    });
}

fn record_predict_event(
    cursor: &CursorPosition,
    results: &[ContextSuggestion],
    policy_version: u32,
    task_type: crate::policy::TaskType,
    latency_ms: u64,
) {
    let items: Vec<RetrievalResultItem> = results
        .iter()
        .enumerate()
        .map(|(rank, result)| RetrievalResultItem {
            node_id: result.node_id.clone(),
            file_path: result.file_path.clone(),
            relevance_score: result.relevance_score,
            rank: rank + 1,
        })
        .collect();
    let estimated_tokens: usize = results.iter().map(|result| result.content.len() / 4).sum();
    let trajectory_context = current_context();
    record_if_enabled(RetrievalEvent {
        session_id: std::env::var("CCM_TRAJECTORY_SESSION").unwrap_or_else(|_| "cli".to_string()),
        tool_name: trajectory_context
            .as_ref()
            .map(|context| context.tool_name.clone()),
        request_id: trajectory_context.and_then(|context| context.request_id),
        task_type,
        policy_version,
        query: None,
        cursor: Some(crate::trajectory::CursorRef {
            file_path: cursor.file_path.clone(),
            line: cursor.line,
        }),
        results: items,
        estimated_tokens,
        latency_ms,
        timestamp_ms: crate::trajectory::now_ms(),
    });
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

#[cfg(test)]
mod retrieval_regression_tests {
    use super::{extract_file_path, lexical_graph_score};
    use crate::engine::RetrievalEngine;
    use crate::graph::{CodeGraph, CodeNode, NodeType};
    use crate::trajectory::{with_context, RetrievalEvent, TrajectoryContext};
    use crate::vector::store::LanceDbStore;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[test]
    fn stable_node_ids_report_the_actual_file_path() {
        let id = "./src/detector/yolo.py:class_definition:symbol:0123456789abcdef:0";
        assert_eq!(extract_file_path(id), "./src/detector/yolo.py");
    }

    #[test]
    fn code_symbols_rank_above_generic_data_file_path_matches() {
        let symbol = lexical_graph_score(
            "vision",
            "visionpipeline",
            "symbol-id",
            "./src/vision.py",
            "class VisionPipeline: pass",
            &NodeType::Class,
            false,
        )
        .unwrap();
        let data_file = lexical_graph_score(
            "vision",
            "./data/vision_memory.db",
            "./data/vision_memory.db",
            "./data/vision_memory.db",
            "",
            &NodeType::DataFile,
            false,
        )
        .unwrap();

        assert!(symbol > data_file);
    }

    #[tokio::test]
    async fn lexical_fallback_records_scoped_trajectory_event() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let trajectory_path = directory.path().join("experiences.jsonl");
        std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        std::env::set_var("CCM_TRAJECTORY_LOG", "1");
        std::env::set_var("CCM_TRAJECTORY_PATH", &trajectory_path);

        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "./src/main.rs:function_item:symbol:0000000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "alpha".to_string(),
            content: "fn alpha() {}".into(),
            start_line: 1,
            end_line: 1,
        });
        let store = LanceDbStore::new(
            directory.path().join("db").to_string_lossy().as_ref(),
            "code_vectors",
        )
        .await?;
        let engine = RetrievalEngine::new(Arc::new(RwLock::new(graph)), store);
        let results = with_context(
            TrajectoryContext {
                tool_name: "search_code".to_string(),
                request_id: Some("request-7".to_string()),
            },
            engine.search_code_hybrid("alpha", 5),
        )
        .await?;
        assert!(!results.is_empty());

        let line = std::fs::read_to_string(&trajectory_path)?;
        let event: RetrievalEvent = serde_json::from_str(line.trim())?;
        assert_eq!(event.tool_name.as_deref(), Some("search_code"));
        assert_eq!(event.request_id.as_deref(), Some("request-7"));
        assert_eq!(event.query.as_deref(), Some("alpha"));
        assert!(!event.results.is_empty());

        std::env::remove_var("CCM_TRAJECTORY_LOG");
        std::env::remove_var("CCM_TRAJECTORY_PATH");
        Ok(())
    }
}
