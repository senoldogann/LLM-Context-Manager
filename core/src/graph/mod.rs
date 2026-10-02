use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub mod map;
pub mod references;
mod resolve;
pub mod usages;

pub use map::{project_map, MapError};

pub use usages::{
    explanation_of, usages_of, Explanation, Usage, UsageError, UsageRelation, UsageReport,
};

pub use references::{
    python_module_path, python_package, CallSite, CallTarget, ImportBinding, ReferenceFacts,
    SyntaxFacts, SyntaxLanguage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    File,
    DataFile,
    Module,
    Class,
    Function,
    Method,
    Variable,
    Import,
    Struct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeNode {
    pub id: String,
    pub node_type: NodeType,
    pub name: String,
    pub content: Arc<str>,
    pub start_line: usize,
    pub end_line: usize,
    /// Referans olguları; eski indekslerde alan yoktur ve sözcüksel sayılır.
    #[serde(default)]
    pub facts: ReferenceFacts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeType {
    Calls,
    /// Olası çağrı: sözcüksel yolda aynı dosyada aynı adlı birden çok hedef,
    /// Python'da alıcı türü bilinmeyen çağrının en çok beş adayı ya da birden çok
    /// kesin hedef.
    CallAmbiguous,
    /// İmport ya da yerel tanımla çözülemeyen, projede tek tanımı olan ada bağlanan
    /// çıplak Python çağrısı (ör. proje dışı yıldız import ya da `__all__` ile
    /// dışa aktarılan ad). Kesin değildir.
    CallInferred,
    Defines,
    Imports,
    /// Birden çok aynı isimli hedef olduğunda üretilen belirsiz import kenarı.
    ImportAmbiguous,
    Contains,
    Inherits,
    Reads,
    Writes,
    /// Python'da çağrılmadan kullanım: argüman, öznitelik zincirinin başı,
    /// atamanın sağ tarafı ya da tip ipucu (`isinstance(x, User)`, `User.objects`).
    References,
}

#[derive(Clone)]
pub struct CodeGraph {
    pub graph: DiGraph<CodeNode, EdgeType>,
    pub id_index: std::collections::HashMap<String, NodeIndex>,
    pub name_index: std::collections::HashMap<String, Vec<NodeIndex>>,
    pub file_nodes_index: std::collections::HashMap<String, Vec<NodeIndex>>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self {
            graph: DiGraph::new(),
            id_index: std::collections::HashMap::new(),
            name_index: std::collections::HashMap::new(),
            file_nodes_index: std::collections::HashMap::new(),
        }
    }
}

impl CodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: CodeNode) -> NodeIndex {
        let id = node.id.clone();
        let name = node.name.clone();
        let file_id = graph_node_file_path(&node.id).to_string();
        let idx = self.graph.add_node(node);
        self.id_index.insert(id, idx);
        self.name_index.entry(name).or_default().push(idx);
        self.file_nodes_index.entry(file_id).or_default().push(idx);
        idx
    }

    pub fn add_edge(&mut self, source: NodeIndex, target: NodeIndex, weight: EdgeType) {
        if self
            .graph
            .edges_connecting(source, target)
            .any(|edge| edge.weight() == &weight)
        {
            return;
        }
        self.graph.add_edge(source, target, weight);
    }

    pub(crate) fn append_graph(&mut self, other: &CodeGraph) {
        let mut indices = HashMap::new();
        for old_index in other.graph.node_indices() {
            let new_index = self.add_node(other.graph[old_index].clone());
            indices.insert(old_index, new_index);
        }
        for edge in other.graph.edge_references() {
            let Some(source) = indices.get(&edge.source()).copied() else {
                continue;
            };
            let Some(target) = indices.get(&edge.target()).copied() else {
                continue;
            };
            self.add_edge(source, target, edge.weight().clone());
        }
    }

    /// Rebuilds cross-symbol references from the current semantic node contents.
    ///
    /// Definitions are extracted per file, but callers can be indexed before their
    /// targets. Rebuilding after all changed definitions are present makes reference
    /// edges deterministic. Every source is resolved with the same per-source rules
    /// as [`CodeGraph::refresh_reference_edges`].
    pub fn rebuild_reference_edges(&mut self) -> usize {
        let sources: Vec<NodeIndex> = self
            .graph
            .node_indices()
            .filter(|idx| is_reference_source(&self.graph[*idx]))
            .collect();
        let references = self.resolve_references(&sources);
        let count = references.len();
        let all_sources = vec![true; self.graph.node_count()];
        self.replace_reference_edges(&all_sources, references);
        count
    }

    /// Recomputes reference edges only for the sources an incremental update can
    /// affect and returns how many sources and edges were recomputed.
    ///
    /// Affected sources are the sources inside `changed_files` plus every other
    /// source that mentions one of `affected_names` (the referenceable names the
    /// changed files defined before or define after the update), widened for
    /// Python by the names a source can reach without mentioning them: members
    /// inherited through the changed classes' bases and the aliases of package
    /// re-export chains (both up to the resolver's depth limits). Any other
    /// source only mentions names whose target sets did not change, so its edges
    /// stay exactly what a full [`CodeGraph::rebuild_reference_edges`] would
    /// produce.
    pub fn refresh_reference_edges(
        &mut self,
        changed_files: &HashSet<String>,
        affected_names: &HashSet<String>,
    ) -> ReferenceRefresh {
        let affected_names = resolve::expand_affected_names(self, changed_files, affected_names);
        let sources: Vec<NodeIndex> = self
            .graph
            .node_indices()
            .filter(|idx| {
                let node = &self.graph[*idx];
                is_reference_source(node)
                    && (changed_files.contains(graph_node_file_path(&node.id))
                        || source_mentions_any(node, &affected_names))
            })
            .collect();
        let references = self.resolve_references(&sources);
        let refreshed = ReferenceRefresh {
            sources: sources.len(),
            edges: references.len(),
        };
        let mut stale_sources = vec![false; self.graph.node_count()];
        for source in &sources {
            stale_sources[source.index()] = true;
        }
        self.replace_reference_edges(&stale_sources, references);
        refreshed
    }

    /// Dosyanın referans hedefi olabilen düğümlerinin adları ve Python dosyasında
    /// import bağlarının yerel adları (paket yeniden dışa aktarmaları). Artımlı
    /// güncelleme bu adları dosya değişmeden önce ve sonra toplar; bu adlardan
    /// birini anan kaynakların kenarları yeniden hesaplanır.
    pub fn reference_target_names(&self, file_id: &str) -> HashSet<String> {
        let mut names: HashSet<String> = self
            .find_nodes_by_file(file_id)
            .iter()
            .map(|idx| &self.graph[*idx])
            .filter(|node| {
                is_reference_target_type(&node.node_type) && is_referenceable_symbol(&node.name)
            })
            .map(|node| node.name.clone())
            .collect();
        if let Some(file_idx) = self.find_file_node(file_id) {
            if let ReferenceFacts::Syntax(facts) = &self.graph[file_idx].facts {
                names.extend(
                    facts
                        .imports
                        .iter()
                        .filter(|binding| binding.local != "*")
                        .map(|binding| binding.local.clone()),
                );
            }
        }
        // Taban listesi değişen sınıfın alt sınıflarındaki `self.ad()` çağrıları
        // kalıtılan üye adlarıyla yeniden çözülür (değişiklik öncesi durum).
        names.extend(resolve::inherited_member_names_in_file(self, file_id));
        names
    }

    /// Verilen kaynakların referans kenarlarını tek kural kümesiyle çözer.
    ///
    /// Kaynak başına her (ad, çağrı biçimi) çifti bir kez çözülür; farklı adlar
    /// farklı hedeflere, farklı kaynaklar farklı kenarlara gittiğinden sonuç
    /// tekrarsızdır.
    fn resolve_references(&self, sources: &[NodeIndex]) -> Vec<(NodeIndex, NodeIndex, EdgeType)> {
        let mut symbols = SymbolTable::new(self);
        let modules = resolve::PythonModules::new(self);
        let mut references = Vec::new();
        for source_idx in sources {
            match &self.graph[*source_idx].facts {
                ReferenceFacts::Lexical => {
                    self.resolve_source_references(*source_idx, &mut symbols, &mut references)
                }
                ReferenceFacts::Syntax(facts) => match facts.language {
                    SyntaxLanguage::Python => references.extend(resolve::python_references(
                        self,
                        &modules,
                        *source_idx,
                        facts,
                    )),
                },
            }
        }
        references
    }

    /// Tek bir kaynağın içeriğindeki tanımlayıcıları hedef sembollere çözer ve
    /// kenarları `references`'a ekler.
    fn resolve_source_references<'g>(
        &'g self,
        source_idx: NodeIndex,
        symbols: &mut SymbolTable<'g>,
        references: &mut Vec<(NodeIndex, NodeIndex, EdgeType)>,
    ) {
        let source = &self.graph[source_idx];
        let source_file = graph_node_file_path(&source.id);
        let source_name = source.name.as_str();
        let mut tokens = identifier_tokens(&source.content);
        // Kaynak düğümün kendi bildirim adı, doc-comment/string/yorum
        // içeriğinde geçse bile gerçek bir çağrı değildir. Ad, gövdedeki
        // gerçek çağrılardan ayrıştırılırken kendi adına eşit olan token'lar
        // hedef seçimi için sayılmaz; sahte ters Calls kenarları böylece
        // yalnızca bildirimde değil yorum/string varyantlarında da engellenir.
        let declaration_token_present = tokens
            .iter()
            .any(|(name, call_like)| *name == source_name && !call_like);
        // Kaynak düğümün kendi bildirim adı yalnızca kendi içeriğinde bir kez
        // geçiyorsa (ör. `func startRecording() {`), bu bir çağrı değildir ve
        // başka dosyadaki aynı isimli fonksiyonla sahte ters çağrı kenarı
        // üretmemelidir.
        let source_name_occurs_once = tokens
            .iter()
            .filter(|(name, _)| *name == source_name)
            .count()
            == 1;
        tokens.sort_unstable();
        tokens.dedup();

        for (name, call_like) in tokens {
            // Kendi bildirim adına eşit non-call token'lar (bildirim satırı,
            // doc-comment, string, yorum) çağrı sayılmaz; call-like gerçek
            // gövde çağrıları (ör. `recorder.startRecording()`) korunur.
            if name == source_name
                && ((declaration_token_present && !call_like) || source_name_occurs_once)
            {
                continue;
            }
            let targets = symbols.targets(name);
            // Kaynak düğümün kendisi adaylardan çıkarılır: aynı dosyada tek
            // "eşleşme" yalnızca kaynağın kendisiyse (örn. kendi adıyla
            // çağrılan üye), farklı dosyadaki gerçek hedef hiç bağlanmıyordu.
            let same_file: Vec<NodeIndex> = targets
                .in_file(source_file)
                .filter(|target_idx| *target_idx != source_idx)
                .collect();
            let (resolved, ambiguous): (Vec<NodeIndex>, bool) = match same_file.len() {
                0 => {
                    let mut candidates =
                        targets.all().filter(|target_idx| *target_idx != source_idx);
                    match (candidates.next(), candidates.next()) {
                        (Some(only), None) => (vec![only], false),
                        _ => continue,
                    }
                }
                1 => (same_file, false),
                // Aynı dosyada aynı isimde birden çok hedef (overload,
                // shadowing): hangisinin kastedildiği name-match ile
                // bilinemez. Yanlış kenar üretmek yerine belirsiz kenar
                // üret ve görünür kıl.
                _ => (same_file, true),
            };

            for target_idx in resolved {
                let target = &self.graph[target_idx];
                let edge_type = if call_like {
                    if ambiguous {
                        EdgeType::CallAmbiguous
                    } else {
                        EdgeType::Calls
                    }
                } else if matches!(
                    target.node_type,
                    NodeType::Class | NodeType::Struct | NodeType::Module
                ) {
                    if ambiguous {
                        EdgeType::ImportAmbiguous
                    } else {
                        EdgeType::Imports
                    }
                } else {
                    continue;
                };
                references.push((source_idx, target_idx, edge_type));
            }
        }
    }

    /// `stale_sources` ile işaretli kaynakların referans kenarlarını `references`
    /// ile değiştirir; diğer tüm kenarlar korunur.
    ///
    /// petgraph'ta tek tek `remove_edge` her silmede bağlı listeleri yürür; kenar
    /// listesi bunun yerine tek geçişte yeniden kurulur. `references` tekrarsız
    /// olduğu ve eski referans kenarları atıldığı için yinelenen kenar denetimi
    /// gerekmez.
    fn replace_reference_edges(
        &mut self,
        stale_sources: &[bool],
        references: Vec<(NodeIndex, NodeIndex, EdgeType)>,
    ) {
        let kept: Vec<(NodeIndex, NodeIndex, EdgeType)> = self
            .graph
            .edge_references()
            .filter(|edge| {
                !(is_reference_edge(edge.weight()) && stale_sources[edge.source().index()])
            })
            .map(|edge| (edge.source(), edge.target(), edge.weight().clone()))
            .collect();
        self.graph.clear_edges();
        for (source, target, edge_type) in kept.into_iter().chain(references) {
            self.graph.add_edge(source, target, edge_type);
        }
    }

    /// Finds the node corresponding to a specific file path.
    pub fn find_file_node(&self, file_path: &str) -> Option<NodeIndex> {
        // O(n) tarama yerine dosya bazlı index kullanılır; bu fonksiyon
        // cursor/impact/diff gibi sıcak path'lerde çağrılıyor.
        self.file_nodes_index
            .get(file_path)?
            .iter()
            .copied()
            .find(|&idx| {
                matches!(
                    self.graph[idx].node_type,
                    NodeType::File | NodeType::DataFile
                )
            })
    }

    /// Finds the deepest node within a file hierarchy that covers the given line.
    pub fn find_node_in_file(&self, file_path: &str, line: usize) -> Option<NodeIndex> {
        let file_node_idx = self.find_file_node(file_path)?;

        // En dar aralıklı sembol node tercih edilir; File/DataFile node yalnızca
        // hiçbir sembol satırı kapsamıyorsa fallback olarak döner.
        let mut best_match: Option<NodeIndex> = None;
        let mut min_len = usize::MAX;

        // BFS/DFS to visit all children of the file
        let mut stack = vec![file_node_idx];

        while let Some(idx) = stack.pop() {
            let node = &self.graph[idx];

            // Check if node covers the line
            if node.start_line <= line && node.end_line >= line {
                if !matches!(node.node_type, NodeType::File | NodeType::DataFile) {
                    let len = node.end_line - node.start_line;
                    if len < min_len {
                        min_len = len;
                        best_match = Some(idx);
                    }
                }

                // Çocuklara inmek için yalnızca Contains kenarları izlenir.
                // find_edge kullanılmaz: aynı çift üzerinde Calls gibi ikinci bir
                // kenar varsa belirsiz olanı döner ve Contains atlanır.
                for edge in self
                    .graph
                    .edges_directed(idx, petgraph::Direction::Outgoing)
                {
                    if matches!(edge.weight(), EdgeType::Contains) {
                        stack.push(edge.target());
                    }
                }
            }
        }

        Some(best_match.unwrap_or(file_node_idx))
    }

    /// Returns enclosing semantic scopes from the narrowest to the widest.
    pub fn find_enclosing_scopes(&self, file_path: &str, line: usize) -> Vec<NodeIndex> {
        let Some(file_node_idx) = self.find_file_node(file_path) else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        let mut stack = vec![file_node_idx];

        while let Some(idx) = stack.pop() {
            let node = &self.graph[idx];
            if node.start_line <= line && node.end_line >= line {
                if matches!(
                    node.node_type,
                    NodeType::Function
                        | NodeType::Method
                        | NodeType::Class
                        | NodeType::Struct
                        | NodeType::Module
                ) {
                    matches.push(idx);
                }
                for edge in self
                    .graph
                    .edges_directed(idx, petgraph::Direction::Outgoing)
                {
                    if matches!(edge.weight(), EdgeType::Contains) {
                        stack.push(edge.target());
                    }
                }
            }
        }

        matches.sort_by_key(|idx| {
            let node = &self.graph[*idx];
            node.end_line.saturating_sub(node.start_line)
        });
        matches
    }
    pub fn find_node_by_id(&self, id: &str) -> Option<CodeNode> {
        // O(1) lookup via id_index — previously was O(n) linear scan
        self.id_index.get(id).map(|&idx| self.graph[idx].clone())
    }

    /// Kesin ID eşleşmesi bulunamazsa aynı dosya+tür+yakın satır ile düzeltilmiş arama yapar.
    /// Kod değişiklikleri nedeniyle satır numarası kaymış node_id'leri çözmek için kullanılır.
    pub fn find_node_fuzzy_by_id(&self, id: &str) -> Option<CodeNode> {
        if let Some(node) = self.find_node_by_id(id) {
            return Some(node);
        }

        // Hedef id stable (symbol) formatındaysa row hash'tir; satır hedefi
        // bilinemez. Aynı dosya+türde birden çok aday varsa hangisinin
        // kastedildiği belirsizdir: deterministik ama yanlış "ilk node"
        // eşleşmesi üretmek yerine çözüm reddedilir. Legacy formatında satır
        // hedefi doğrudan kullanılır.
        let (path_part, kind_part, target_row): (&str, &str, Option<usize>) =
            if let Some((path, kind)) = parse_stable_node_id(id) {
                (path, kind, None)
            } else {
                let (path, kind, row) = parse_node_id(id)?;
                (path, kind, row.parse().ok())
            };

        let mut best: Option<&CodeNode> = None;
        let mut best_dist = usize::MAX;
        let mut stable_candidates = 0usize;

        for node in self.graph.node_weights() {
            let (node_path, node_kind, node_row) =
                if let Some((path, kind)) = parse_stable_node_id(&node.id) {
                    (path, kind, node.start_line.saturating_sub(1))
                } else if let Some((path, kind, row)) = parse_node_id(&node.id) {
                    let Ok(row) = row.parse::<usize>() else {
                        continue;
                    };
                    (path, kind, row)
                } else {
                    continue;
                };
            if node_path != path_part || node_kind != kind_part {
                continue;
            }
            if target_row.is_none() {
                // Stable id'de occurrence hash'i çözülemez; birden çok aday
                // belirsizlik oluşturur. Tek aday güvenli kabul edilir.
                stable_candidates += 1;
            }
            let dist = match target_row {
                Some(target) => node_row.abs_diff(target),
                // Stable hedef: aynı path+türdeki en erken occurrence (satır en küçük)
                // deterministik olarak seçilir.
                None => node_row,
            };
            // 200 satırlık tolerans yalnızca legacy id'lerin satır kayması için
            // geçerlidir; stable id'lerde hedef satır bilinmez ve dosyanın her
            // satırındaki node aday olabilir (ilk 200 satır kısıtlaması yanlıştı).
            let within_limit = match target_row {
                Some(_) => dist <= 200,
                None => true,
            };
            if dist < best_dist && within_limit {
                best_dist = dist;
                best = Some(node);
            }
        }

        if target_row.is_none() && stable_candidates > 1 {
            tracing::debug!(
                requested = %id,
                candidates = stable_candidates,
                "Ambiguous stable node ID; refusing fuzzy match"
            );
            return None;
        }

        if let Some(node) = best {
            tracing::debug!(
                requested = %id,
                found = %node.id,
                drift = %best_dist,
                "Fuzzy node ID matched"
            );
            return Some(node.clone());
        }

        None
    }

    /// Retrieves outgoing edges for a given node ID.
    /// Returns Vec<(TargetID, EdgeType)>.
    pub fn get_outgoing_edges(&self, id: &str) -> Vec<(String, EdgeType)> {
        let mut edges = Vec::new();

        if let Some(idx) = self.find_node_index_by_id(id) {
            for edge in self
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
            {
                let target_node = &self.graph[edge.target()];
                edges.push((target_node.id.clone(), edge.weight().clone()));
            }
        }

        edges
    }

    pub fn find_node_index_by_id(&self, id: &str) -> Option<NodeIndex> {
        self.id_index.get(id).cloned()
    }

    /// Saves the graph to a JSON file.
    pub fn save_to_file(&self, path: &str) -> anyhow::Result<()> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
        self.write_json_file(&temp_path)?;
        std::fs::rename(temp_path, path)?;
        Ok(())
    }

    /// Grafı verilen dosyaya yazar ve diske senkronlar; yazılan bayt sayısını
    /// döndürür. Atomik değiştirme (geçici dosya + rename) çağırana aittir.
    pub fn write_json_file(&self, path: &std::path::Path) -> anyhow::Result<u64> {
        #[cfg(unix)]
        let file = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?
        };
        #[cfg(not(unix))]
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut writer, &self.graph)?;
        use std::io::Write;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        Ok(writer.get_ref().metadata()?.len())
    }

    /// Loads the graph from a JSON file.
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let graph: DiGraph<CodeNode, EdgeType> = serde_json::from_reader(reader)?;
        let mut g = Self {
            graph,
            id_index: std::collections::HashMap::new(),
            name_index: std::collections::HashMap::new(),
            file_nodes_index: std::collections::HashMap::new(),
        };
        g.rebuild_index();
        Ok(g)
    }

    /// Rebuilds the HashMap index from the current graph nodes
    pub fn rebuild_index(&mut self) {
        self.id_index.clear();
        self.name_index.clear();
        self.file_nodes_index.clear();
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            self.id_index.insert(node.id.clone(), idx);
            self.name_index
                .entry(node.name.clone())
                .or_default()
                .push(idx);
            let file_id = graph_node_file_path(&node.id).to_string();
            self.file_nodes_index.entry(file_id).or_default().push(idx);
        }
    }

    /// Returns all node indices matching the given symbol/node name.
    pub fn find_nodes_by_name(&self, name: &str) -> &[NodeIndex] {
        self.name_index
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Returns all node indices belonging to the given file ID.
    pub fn find_nodes_by_file(&self, file_id: &str) -> &[NodeIndex] {
        self.file_nodes_index
            .get(file_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Dosyada satırı kapsayan en dar sembol (fonksiyon, metot, sınıf, yapı,
    /// modül); eşit genişlikte önce başlayan seçilir. Sembol yoksa `None`.
    pub fn symbol_at(&self, file_id: &str, line: usize) -> Option<NodeIndex> {
        self.find_nodes_by_file(file_id)
            .iter()
            .copied()
            .filter(|idx| {
                let node = &self.graph[*idx];
                is_reference_target_type(&node.node_type)
                    && node.start_line <= line
                    && line <= node.end_line
            })
            .min_by_key(|idx| {
                let node = &self.graph[*idx];
                (node.end_line - node.start_line, node.start_line)
            })
    }

    /// Adı verilen semboller, dosya ve satıra göre sıralı. `Sahip.üye`
    /// biçiminde adı `Sahip` olan sınıf ya da yapıların doğrudan üyeleri döner.
    pub fn symbols_named(&self, name: &str) -> Vec<NodeIndex> {
        let mut found: Vec<NodeIndex> = match name.rsplit_once('.') {
            Some((owner, member)) => self
                .find_nodes_by_name(owner)
                .iter()
                .copied()
                .filter(|idx| {
                    matches!(
                        self.graph[*idx].node_type,
                        NodeType::Class | NodeType::Struct
                    )
                })
                .flat_map(|owner_idx| {
                    self.graph
                        .edges_directed(owner_idx, petgraph::Direction::Outgoing)
                        .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
                        .map(|edge| edge.target())
                        .filter(|idx| {
                            let node = &self.graph[*idx];
                            node.name == member
                                && matches!(node.node_type, NodeType::Function | NodeType::Method)
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
            None => self
                .find_nodes_by_name(name)
                .iter()
                .copied()
                .filter(|idx| is_reference_target_type(&self.graph[*idx].node_type))
                .collect(),
        };
        found.sort_by(|left, right| {
            let left = &self.graph[*left];
            let right = &self.graph[*right];
            (graph_node_file_path(&left.id), left.start_line)
                .cmp(&(graph_node_file_path(&right.id), right.start_line))
        });
        found
    }

    /// Alias for load_from_file to match API conventions
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        Self::load_from_file(path)
    }

    /// Removes all nodes belonging to a specific file.
    /// Used for incremental indexing (clearing old state).
    pub fn remove_file_nodes(&mut self, file_path: &str) {
        // 1. Find the File Node
        let file_node_idx = match self.find_file_node(file_path) {
            Some(idx) => idx,
            None => return, // File not in graph, nothing to remove
        };

        // 2. Collect all descendants via "Contains" edges (DFS)
        let mut to_remove = HashSet::new();
        let mut stack = vec![file_node_idx];

        // We include the file node itself in the removal
        to_remove.insert(file_node_idx);

        while let Some(idx) = stack.pop() {
            // find_edge belirsiz kenar döndürebilir; tüm kenarlar taranır.
            for edge in self
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
            {
                if matches!(edge.weight(), EdgeType::Contains) {
                    to_remove.insert(edge.target());
                    stack.push(edge.target());
                }
            }
        }

        // 3. Büyükten küçüğe silinir: petgraph son düğümü silinen konuma taşır ve
        // taşınan düğüm her zaman silinmeyecek bir düğümdür; kalan indeksler geçerli
        // kalır ve dizinler tüm grafı yeniden kurmadan güncellenir.
        let mut ordered: Vec<NodeIndex> = to_remove.into_iter().collect();
        ordered.sort_unstable_by(|left, right| right.cmp(left));
        for idx in ordered {
            self.remove_indexed_node(idx);
        }
    }

    /// Düğümü siler ve üç dizini günceller. `remove_node` son düğümü silinen
    /// konuma taşıdığından yalnızca silinen ve taşınan düğümün girdileri değişir.
    fn remove_indexed_node(&mut self, idx: NodeIndex) {
        let last = NodeIndex::new(self.graph.node_count().saturating_sub(1));
        let Some(removed) = self.graph.remove_node(idx) else {
            return;
        };
        let CodeGraph {
            graph,
            id_index,
            name_index,
            file_nodes_index,
        } = self;
        if id_index.get(&removed.id) == Some(&idx) {
            id_index.remove(&removed.id);
        }
        remove_from_bucket(name_index, &removed.name, idx);
        remove_from_bucket(file_nodes_index, graph_node_file_path(&removed.id), idx);
        if idx == last {
            return;
        }
        let moved = &graph[idx];
        if let Some(position) = id_index.get_mut(&moved.id) {
            *position = idx;
        }
        replace_in_bucket(name_index, &moved.name, last, idx);
        replace_in_bucket(file_nodes_index, graph_node_file_path(&moved.id), last, idx);
    }
}

/// Artımlı referans yenilemesinin kapsamı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceRefresh {
    /// Kenarları yeniden hesaplanan kaynak düğüm sayısı.
    pub sources: usize,
    /// Bu kaynaklardan üretilen referans kenarı sayısı.
    pub edges: usize,
}

/// Bir adın referans hedefleri; dosya yoluna göre sıralı tutulur, böylece
/// kaynağın dosyasındaki hedefler ikili aramayla bulunur.
struct SymbolTargets<'g> {
    by_file: Vec<(&'g str, NodeIndex)>,
}

impl<'g> SymbolTargets<'g> {
    fn all(&self) -> impl Iterator<Item = NodeIndex> + '_ {
        self.by_file.iter().map(|(_, idx)| *idx)
    }

    fn in_file<'a>(&'a self, file: &'a str) -> impl Iterator<Item = NodeIndex> + 'a {
        let start = self.by_file.partition_point(|(path, _)| *path < file);
        self.by_file[start..]
            .iter()
            .take_while(move |(path, _)| *path == file)
            .map(|(_, idx)| *idx)
    }
}

/// Ad → hedef tablosu; her ad ilk kullanımda bir kez hesaplanır.
struct SymbolTable<'g> {
    graph: &'g CodeGraph,
    entries: HashMap<&'g str, SymbolTargets<'g>>,
}

impl<'g> SymbolTable<'g> {
    fn new(graph: &'g CodeGraph) -> Self {
        Self {
            graph,
            entries: HashMap::new(),
        }
    }

    fn targets(&mut self, name: &'g str) -> &SymbolTargets<'g> {
        let graph = self.graph;
        self.entries
            .entry(name)
            .or_insert_with(|| symbol_targets(graph, name))
    }
}

/// Adın referans hedeflerini hesaplar. Rust impl blokları tipin adını taşıyan
/// Class düğümleridir; aynı adlı struct varken hedefi belirsizleştirip dosyalar
/// arası kenarı engellerler, bu yüzden o ad için elenirler ve tip referansı
/// struct tanımına bağlanır. Kural ad başınadır.
fn symbol_targets<'g>(graph: &'g CodeGraph, name: &str) -> SymbolTargets<'g> {
    if !is_referenceable_symbol(name) {
        return SymbolTargets {
            by_file: Vec::new(),
        };
    }
    let candidates: Vec<NodeIndex> = graph
        .find_nodes_by_name(name)
        .iter()
        .copied()
        .filter(|idx| is_reference_target_type(&graph.graph[*idx].node_type))
        .collect();
    let has_struct = candidates
        .iter()
        .any(|idx| graph.graph[*idx].node_type == NodeType::Struct);
    let mut by_file: Vec<(&'g str, NodeIndex)> = candidates
        .into_iter()
        .filter(|idx| !(has_struct && is_rust_impl_node(&graph.graph[*idx])))
        .map(|idx| (graph_node_file_path(&graph.graph[idx].id), idx))
        .collect();
    by_file.sort_unstable();
    SymbolTargets { by_file }
}

/// Referans kenarı türleri; hepsi düğüm içeriğinden türetilir ve yeniden kurulumda
/// birlikte atılır.
fn is_reference_edge(edge: &EdgeType) -> bool {
    matches!(
        edge,
        EdgeType::Calls
            | EdgeType::CallAmbiguous
            | EdgeType::CallInferred
            | EdgeType::Imports
            | EdgeType::ImportAmbiguous
            | EdgeType::Inherits
            | EdgeType::References
    )
}

/// Referans üreten düğümler: sözcüksel düğümlerde türe göre, sözdizimi
/// olgularında referansı olan her düğüm (fonksiyon, sınıf, dosya).
fn is_reference_source(node: &CodeNode) -> bool {
    match &node.facts {
        ReferenceFacts::Lexical => matches!(
            node.node_type,
            NodeType::Function | NodeType::Method | NodeType::Variable | NodeType::Import
        ),
        ReferenceFacts::Syntax(facts) => facts.has_references(),
    }
}

/// Referans hedefi olabilen düğüm türleri.
fn is_reference_target_type(node_type: &NodeType) -> bool {
    matches!(
        node_type,
        NodeType::Function
            | NodeType::Method
            | NodeType::Class
            | NodeType::Struct
            | NodeType::Module
    )
}

/// Kaynak adlardan birini anıyor mu? Sözcüksel düğümde içerik, sözdizimi
/// olgularında çağrı/bağ/taban/ad listeleri taranır (Python `File` düğümünün
/// içeriği boştur; modül düzeyi çağrı ve importları yalnız olgularındadır).
fn source_mentions_any(node: &CodeNode, names: &HashSet<String>) -> bool {
    match &node.facts {
        ReferenceFacts::Lexical => mentions_any_name(&node.content, names),
        ReferenceFacts::Syntax(facts) => facts.mentions_any(names),
    }
}

/// İçerik, adlardan birine birebir eşit bir tanımlayıcı içeriyor mu? Karar
/// referans çözümüyle aynı tokenizer'a aittir.
fn mentions_any_name(content: &str, names: &HashSet<String>) -> bool {
    !names.is_empty()
        && identifier_spans(content).any(|(start, end)| names.contains(&content[start..end]))
}

fn remove_from_bucket(index: &mut HashMap<String, Vec<NodeIndex>>, key: &str, idx: NodeIndex) {
    let Some(bucket) = index.get_mut(key) else {
        return;
    };
    bucket.retain(|existing| *existing != idx);
    if bucket.is_empty() {
        index.remove(key);
    }
}

fn replace_in_bucket(
    index: &mut HashMap<String, Vec<NodeIndex>>,
    key: &str,
    from: NodeIndex,
    to: NodeIndex,
) {
    if let Some(slot) = index
        .get_mut(key)
        .and_then(|bucket| bucket.iter_mut().find(|existing| **existing == from))
    {
        *slot = to;
    }
}

fn is_referenceable_symbol(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_identifier_start)
        && chars.all(|ch| is_identifier_start(ch) || ch.is_numeric())
}

pub(crate) fn is_rust_impl_node(node: &CodeNode) -> bool {
    // Rust'ta sınıf yoktur; .rs dosyasındaki Class düğümleri impl bloklarıdır.
    node.node_type == NodeType::Class && graph_node_file_path(&node.id).ends_with(".rs")
}

fn graph_node_file_path(node_id: &str) -> &str {
    if let Some((path_and_kind, _)) = node_id.split_once(":symbol:") {
        return path_and_kind
            .rsplit_once(':')
            .map(|(path, _)| path)
            .unwrap_or(node_id);
    }
    node_id.rsplitn(4, ':').last().unwrap_or(node_id)
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

/// İçerikteki tanımlayıcıların bayt aralıkları. Referans çözümü ve etkilenen
/// kaynak araması aynı tanımı kullanır.
fn identifier_spans(content: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut chars = content.char_indices().peekable();
    std::iter::from_fn(move || {
        while let Some((start, first)) = chars.next() {
            if !is_identifier_start(first) {
                continue;
            }
            let mut end = start + first.len_utf8();
            while let Some(&(index, ch)) = chars.peek() {
                if !is_identifier_start(ch) && !ch.is_numeric() {
                    break;
                }
                chars.next();
                end = index + ch.len_utf8();
            }
            return Some((start, end));
        }
        None
    })
}

/// Tanımlayıcılar ve her birinin çağrı biçiminde (`ad(`) geçip geçmediği.
fn identifier_tokens(content: &str) -> Vec<(&str, bool)> {
    identifier_spans(content)
        .map(|(start, end)| {
            let call_like = content[end..]
                .chars()
                .find(|ch| !ch.is_whitespace())
                .is_some_and(|ch| ch == '(');
            (&content[start..end], call_like)
        })
        .collect()
}

/// "<path>:<kind>:<row>:<col>" formatındaki node ID'sini parçalarına ayırır.
/// Başarılı olursa (path, kind, row) döndürür.
fn parse_node_id(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.rsplitn(4, ':');
    let _col = parts.next()?;
    let row = parts.next()?;
    let kind = parts.next()?;
    let path = parts.next()?;
    Some((path, kind, row))
}

fn parse_stable_node_id(id: &str) -> Option<(&str, &str)> {
    let mut parts = id.rsplitn(5, ':');
    let _occurrence = parts.next()?;
    let _hash = parts.next()?;
    if parts.next()? != "symbol" {
        return None;
    }
    let kind = parts.next()?;
    let path = parts.next()?;
    Some((path, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node() {
        let mut graph = CodeGraph::new();
        let node = CodeNode {
            id: "test_func".to_string(),
            node_type: NodeType::Function,
            name: "test".to_string(),
            content: "fn test() {}".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        };
        let index = graph.add_node(node);
        assert_eq!(index.index(), 0);
    }

    #[test]
    fn test_find_nodes_by_name_and_file() {
        let mut graph = CodeGraph::new();
        let node1 = CodeNode {
            id: "./src/lib.rs:function_definition:symbol:1:0".to_string(),
            node_type: NodeType::Function,
            name: "process_data".to_string(),
            content: "fn process_data() {}".into(),
            start_line: 1,
            end_line: 5,
            facts: crate::graph::ReferenceFacts::Lexical,
        };
        let node2 = CodeNode {
            id: "./src/lib.rs:function_definition:symbol:10:0".to_string(),
            node_type: NodeType::Function,
            name: "process_data".to_string(),
            content: "fn process_data() {}".into(),
            start_line: 10,
            end_line: 15,
            facts: crate::graph::ReferenceFacts::Lexical,
        };

        graph.add_node(node1);
        graph.add_node(node2);

        let by_name = graph.find_nodes_by_name("process_data");
        assert_eq!(by_name.len(), 2);

        let by_file = graph.find_nodes_by_file("./src/lib.rs");
        assert_eq!(by_file.len(), 2);
    }

    #[test]
    fn remove_file_nodes_only_removes_target_file() {
        let mut graph = CodeGraph::new();

        let file_a_idx = graph.add_node(CodeNode {
            id: "./a.rs".to_string(),
            node_type: NodeType::File,
            name: "./a.rs".to_string(),
            content: "fn foo() {}".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let func_a_idx = graph.add_node(CodeNode {
            id: "./a.rs:func:foo".to_string(),
            node_type: NodeType::Function,
            name: "foo".to_string(),
            content: "fn foo() {}".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        graph.add_edge(file_a_idx, func_a_idx, EdgeType::Contains);

        let file_b_idx = graph.add_node(CodeNode {
            id: "./b.rs".to_string(),
            node_type: NodeType::File,
            name: "./b.rs".to_string(),
            content: "fn bar() {}".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let func_b_idx = graph.add_node(CodeNode {
            id: "./b.rs:func:bar".to_string(),
            node_type: NodeType::Function,
            name: "bar".to_string(),
            content: "fn bar() {}".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        graph.add_edge(file_b_idx, func_b_idx, EdgeType::Contains);

        graph.remove_file_nodes("./a.rs");

        assert!(graph.find_file_node("./a.rs").is_none());
        assert!(graph.find_file_node("./b.rs").is_some());
        assert!(graph
            .graph
            .node_weights()
            .all(|node| !node.id.starts_with("./a.rs")));
    }

    #[test]
    fn remove_file_nodes_follows_contains_even_with_parallel_call_edge() {
        // rebuild_reference_edges, Contains'tan sonra aynı node çifti üzerine
        // Calls kenarı ekleyebilir. find_edge bu durumda Calls'u döndürürdü
        // ve Contains kenarı izlenmeden inner node silinmeden kalırdı.
        let mut graph = CodeGraph::new();

        let file_idx = graph.add_node(CodeNode {
            id: "./f.rs".to_string(),
            node_type: NodeType::File,
            name: "./f.rs".to_string(),
            content: "".into(),
            start_line: 1,
            end_line: 10,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let outer_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "outer".to_string(),
            content: "fn outer() { fn inner() {} inner(); }".into(),
            start_line: 1,
            end_line: 10,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let inner_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000002:0".to_string(),
            node_type: NodeType::Function,
            name: "inner".to_string(),
            content: "fn inner() {}".into(),
            start_line: 3,
            end_line: 5,
            facts: crate::graph::ReferenceFacts::Lexical,
        });

        graph.add_edge(file_idx, outer_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Calls);

        graph.remove_file_nodes("./f.rs");

        assert_eq!(graph.graph.node_count(), 0);
    }

    #[test]
    fn find_node_in_file_follows_contains_even_with_parallel_call_edge() {
        let mut graph = CodeGraph::new();

        let file_idx = graph.add_node(CodeNode {
            id: "./f.rs".to_string(),
            node_type: NodeType::File,
            name: "./f.rs".to_string(),
            content: "".into(),
            start_line: 1,
            end_line: 10,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let outer_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "outer".to_string(),
            content: "fn outer() { fn inner() {} inner(); }".into(),
            start_line: 1,
            end_line: 10,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let inner_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000002:0".to_string(),
            node_type: NodeType::Function,
            name: "inner".to_string(),
            content: "fn inner() {}".into(),
            start_line: 3,
            end_line: 5,
            facts: crate::graph::ReferenceFacts::Lexical,
        });

        graph.add_edge(file_idx, outer_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Calls);

        let found = graph
            .find_node_in_file("./f.rs", 4)
            .expect("node for line 4");
        assert_eq!(found, inner_idx);
    }

    #[test]
    fn stable_id_fuzzy_resolves_nodes_beyond_line_200() {
        let mut graph = CodeGraph::new();
        // 201. satırdaki bir fonksiyonun stable id'si hash değişse bile fuzzy
        // çözüm dosyanın ilk 200 satırıyla sınırlı kalmamalı.
        graph.add_node(CodeNode {
            id: "./src/tall.rs:function_item:symbol:aaaa000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "deep_function".to_string(),
            content: "fn deep_function() {}".into(),
            start_line: 201,
            end_line: 205,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let found =
            graph.find_node_fuzzy_by_id("./src/tall.rs:function_item:symbol:bbbb000000000001:0");
        assert!(
            found.is_some(),
            "201. satırdaki node stable-id farkıyla çözülebilmeli"
        );
        assert_eq!(found.unwrap().name, "deep_function");
    }

    #[test]
    fn stable_id_fuzzy_refuses_ambiguous_multi_node_files() {
        let mut graph = CodeGraph::new();
        // Aynı dosya+türde birden çok aday varsa stable-id hash'i çözülemez;
        // "ilk node" eşleşmesi yanlış olacağından çözüm reddedilir.
        graph.add_node(CodeNode {
            id: "./src/multi.rs:function_item:symbol:aaaa000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "first".to_string(),
            content: "fn first() {}".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        graph.add_node(CodeNode {
            id: "./src/multi.rs:function_item:symbol:aaaa000000000002:0".to_string(),
            node_type: NodeType::Function,
            name: "second".to_string(),
            content: "fn second() {}".into(),
            start_line: 201,
            end_line: 201,
            facts: crate::graph::ReferenceFacts::Lexical,
        });

        let found =
            graph.find_node_fuzzy_by_id("./src/multi.rs:function_item:symbol:bbbb000000000001:0");
        assert!(found.is_none(), "belirsiz stable-id çözümü reddedilmeli");
    }

    #[test]
    fn rebuild_reference_edges_links_imports_constructors_and_type_annotations() {
        let mut graph = CodeGraph::new();
        let class_idx = graph.add_node(CodeNode {
            id: "./detector.py:class_definition:symbol:1:0".to_string(),
            node_type: NodeType::Class,
            name: "YoloDetector".to_string(),
            content: "class YoloDetector: pass".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let import_idx = graph.add_node(CodeNode {
            id: "./camera.py:import_from_statement:symbol:2:0".to_string(),
            node_type: NodeType::Import,
            name: "from detector import YoloDetector".to_string(),
            content: "from detector import YoloDetector".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let function_idx = graph.add_node(CodeNode {
            id: "./camera.py:function_definition:symbol:3:0".to_string(),
            node_type: NodeType::Function,
            name: "open_camera".to_string(),
            content: "def open_camera(detector: YoloDetector):\n    return YoloDetector()".into(),
            start_line: 3,
            end_line: 4,
            facts: crate::graph::ReferenceFacts::Lexical,
        });

        graph.rebuild_reference_edges();

        assert!(graph
            .graph
            .edges_connecting(import_idx, class_idx)
            .any(|edge| matches!(edge.weight(), EdgeType::Imports)));
        assert!(graph
            .graph
            .edges_connecting(function_idx, class_idx)
            .any(|edge| matches!(edge.weight(), EdgeType::Calls)));
    }

    #[test]
    fn rebuild_reference_edges_skips_ambiguous_cross_file_symbols() {
        let mut graph = CodeGraph::new();
        for (id, file) in [
            ("./a.py:function_definition:symbol:1:0", "./a.py"),
            ("./b.py:function_definition:symbol:2:0", "./b.py"),
        ] {
            graph.add_node(CodeNode {
                id: id.to_string(),
                node_type: NodeType::Function,
                name: "load".to_string(),
                content: format!("def load(): return '{file}'").into(),
                start_line: 1,
                end_line: 1,
                facts: crate::graph::ReferenceFacts::Lexical,
            });
        }
        let source_idx = graph.add_node(CodeNode {
            id: "./caller.py:function_definition:symbol:3:0".to_string(),
            node_type: NodeType::Function,
            name: "run".to_string(),
            content: "def run(): return load()".into(),
            start_line: 1,
            end_line: 1,
            facts: crate::graph::ReferenceFacts::Lexical,
        });

        graph.rebuild_reference_edges();

        assert_eq!(
            graph
                .graph
                .edges_directed(source_idx, petgraph::Direction::Outgoing)
                .filter(|edge| matches!(edge.weight(), EdgeType::Calls))
                .count(),
            0
        );
    }

    #[test]
    fn rebuild_reference_edges_marks_same_file_overloads_ambiguous() {
        use petgraph::visit::EdgeRef;
        let mut graph = CodeGraph::new();
        let file_idx = graph.add_node(CodeNode {
            id: "./src/a.rs".into(),
            node_type: NodeType::File,
            name: "./src/a.rs".into(),
            content: String::new().into(),
            start_line: 1,
            end_line: 10,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let caller_idx = graph.add_node(CodeNode {
            id: "./src/a.rs:function_item:1:0".into(),
            node_type: NodeType::Function,
            name: "run".into(),
            content: "fn run() { helper(); }".into(),
            start_line: 1,
            end_line: 3,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let helper_a = graph.add_node(CodeNode {
            id: "./src/a.rs:function_item:4:0".into(),
            node_type: NodeType::Function,
            name: "helper".into(),
            content: "fn helper() {}".into(),
            start_line: 4,
            end_line: 5,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        let helper_b = graph.add_node(CodeNode {
            id: "./src/a.rs:function_item:6:0".into(),
            node_type: NodeType::Function,
            name: "helper".into(),
            content: "fn helper(x: u32) {}".into(),
            start_line: 6,
            end_line: 7,
            facts: crate::graph::ReferenceFacts::Lexical,
        });
        graph.add_edge(file_idx, caller_idx, EdgeType::Contains);
        graph.add_edge(file_idx, helper_a, EdgeType::Contains);
        graph.add_edge(file_idx, helper_b, EdgeType::Contains);

        graph.rebuild_reference_edges();

        let mut ambiguous = 0usize;
        let mut from_caller = 0usize;
        for edge in graph.graph.edge_references() {
            if matches!(
                edge.weight(),
                EdgeType::CallAmbiguous | EdgeType::ImportAmbiguous
            ) {
                ambiguous += 1;
                if edge.source() == caller_idx {
                    from_caller += 1;
                }
            }
        }
        assert!(
            ambiguous >= 2,
            "overload kenarları ambiguous işaretlenmeli ({} >= 2)",
            ambiguous
        );
        assert_eq!(from_caller, 2, "çağıran iki belirsiz hedefe de bağlanmalı");
    }

    #[test]
    fn test_code_node_serde_arc_str() {
        // 1. Standard node
        let original_node = CodeNode {
            id: "./src/lib.rs:func:1".to_string(),
            node_type: NodeType::Function,
            name: "test_func".to_string(),
            content: "fn test_func() { println!(\"hello\"); }".into(),
            start_line: 1,
            end_line: 5,
            facts: crate::graph::ReferenceFacts::Lexical,
        };

        let json = serde_json::to_string(&original_node).expect("failed to serialize node");
        let deserialized: CodeNode =
            serde_json::from_str(&json).expect("failed to deserialize node");

        assert_eq!(deserialized.id, original_node.id);
        assert_eq!(deserialized.node_type, original_node.node_type);
        assert_eq!(deserialized.name, original_node.name);
        assert_eq!(
            deserialized.content.as_ref(),
            "fn test_func() { println!(\"hello\"); }"
        );
        assert_eq!(deserialized.start_line, original_node.start_line);
        assert_eq!(deserialized.end_line, original_node.end_line);

        // 2. Empty content string
        let empty_node = CodeNode {
            id: "./empty.rs".to_string(),
            node_type: NodeType::File,
            name: "./empty.rs".to_string(),
            content: "".into(),
            start_line: 0,
            end_line: 0,
            facts: crate::graph::ReferenceFacts::Lexical,
        };
        let empty_json =
            serde_json::to_string(&empty_node).expect("failed to serialize empty node");
        let deserialized_empty: CodeNode =
            serde_json::from_str(&empty_json).expect("failed to deserialize empty node");
        assert_eq!(deserialized_empty.content.as_ref(), "");

        // 3. Long content string (10KB+)
        let long_text = "x".repeat(15_000);
        let long_node = CodeNode {
            id: "./large.rs".to_string(),
            node_type: NodeType::File,
            name: "./large.rs".to_string(),
            content: long_text.as_str().into(),
            start_line: 1,
            end_line: 1000,
            facts: crate::graph::ReferenceFacts::Lexical,
        };
        let long_json = serde_json::to_string(&long_node).expect("failed to serialize large node");
        let deserialized_long: CodeNode =
            serde_json::from_str(&long_json).expect("failed to deserialize large node");
        assert_eq!(deserialized_long.content.len(), 15_000);
        assert_eq!(deserialized_long.content.as_ref(), long_text.as_str());
    }

    #[test]
    fn syntax_facts_survive_a_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("graph.json");
        let facts = SyntaxFacts {
            language: SyntaxLanguage::Python,
            calls: vec![CallSite {
                target: CallTarget::SelfMember("stop".to_string()),
                line: 7,
            }],
            imports: vec![ImportBinding {
                local: "helper".to_string(),
                module: "app.util".to_string(),
                symbol: Some("helper".to_string()),
            }],
            bases: Vec::new(),
            names: vec!["User".to_string()],
        };
        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "app/core.py:function_definition:symbol:0000000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "start".to_string(),
            content: "def start(self):\n    self.stop()\n".into(),
            start_line: 6,
            end_line: 7,
            facts: ReferenceFacts::Syntax(facts.clone()),
        });
        graph.save_to_file(&path.to_string_lossy()).expect("save");
        let loaded = CodeGraph::load_from_file(&path.to_string_lossy()).expect("load");
        let node = loaded.graph.node_weights().next().expect("node");
        assert_eq!(node.facts, ReferenceFacts::Syntax(facts));
    }
}
