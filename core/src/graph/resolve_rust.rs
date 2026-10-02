//! Rust sözdizimi olgularını kenarlara çeviren çözümleyici.
//!
//! Çıplak ad sırasıyla kapsayan fonksiyonların ve modülün tanımlarında, `use`
//! bağlarında ve yıldız importlarında aranır; bulunamayan ad proje dışıdır (std,
//! prelude, dış crate) ve kenar üretmez. Yollar `crate`, `self`, `super`, `Self`,
//! kapsamdaki bir ad ya da çalışma alanı kütüphanesinin adıyla başlar; sonraki
//! bileşenler modüllere (dosya, satır içi `mod`, `pub use` yeniden dışa aktarması)
//! ya da bir türün `impl` metotlarına iner. Bir `impl` bloğunun türü ve trait'i
//! bloğun kendi kapsamında çözülür; aynı adlı başka türlerin metotları karışmaz.
//! Alıcısının türü bilinmeyen metot çağrıları en çok `MAX_POSSIBLE_TARGETS` metoda
//! "olası" kenar üretir. Modül-ad aramaları bir çözüm turu boyunca önbelleğe
//! alınır: yıldız importları zinciri her çağrıda yeniden yürünmez.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::references::{CallTarget, ImportBinding, ReferenceFacts, SyntaxFacts, SyntaxLanguage};
use super::resolve::{call_edges, edge_rank, Resolution, MAX_POSSIBLE_TARGETS};
use super::rust_modules::{RustCrates, RustModule};
use super::{graph_node_file_path, is_rust_impl_node, CodeGraph, EdgeType, NodeType};

/// `use` ve yeniden dışa aktarma zincirinde izlenecek en çok adım.
const MAX_HOPS: usize = 4;

/// Yolun bir bileşeninin çözümü.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    /// Fonksiyon, metot, tür ya da sabit.
    Node(NodeIndex),
    /// Modül: dosya ya da satır içi `mod`.
    Module(RustModule),
    /// Türe ulaşıldı ama üyesi bulunamadı (enum varyantı, ilişkili sabit).
    TypeOnly(NodeIndex),
}

/// Kaynağın kapsamı.
struct Scope {
    source: NodeIndex,
    /// Kaynak ve onu saran fonksiyonlar, içten dışa.
    functions: Vec<NodeIndex>,
    /// Kaynağı saran modül.
    module: RustModule,
    /// Kaynağı saran `impl` ya da `trait` düğümü.
    container: Option<NodeIndex>,
}

/// Bir çözüm turunun (tam kurulum ya da artımlı yenileme) Rust bağlamı: crate
/// modeli, `impl` bloklarının çözülmüş türleri ve modül-ad aramalarının önbelleği.
pub(crate) struct RustContext<'g> {
    graph: &'g CodeGraph,
    crates: RustCrates,
    /// (modül, ad, adım) → öğeler.
    items: RefCell<HashMap<(RustModule, String, usize), Vec<Item>>>,
    /// `impl` düğümü → uyguladığı proje türü (yapı ya da enum).
    impl_types: HashMap<NodeIndex, Vec<NodeIndex>>,
    /// Tür → onu uygulayan `impl` düğümleri.
    type_impls: HashMap<NodeIndex, Vec<NodeIndex>>,
}

impl<'g> RustContext<'g> {
    pub(crate) fn new(graph: &'g CodeGraph) -> Self {
        let mut context = Self {
            graph,
            crates: RustCrates::new(graph),
            items: RefCell::new(HashMap::new()),
            impl_types: HashMap::new(),
            type_impls: HashMap::new(),
        };
        let impl_types: HashMap<NodeIndex, Vec<NodeIndex>> = graph
            .graph
            .node_indices()
            .filter(|idx| is_rust_impl_node(&graph.graph[*idx]))
            .map(|idx| (idx, context.implemented_types(idx)))
            .collect();
        let mut type_impls: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        for (impl_idx, types) in &impl_types {
            for type_idx in types {
                type_impls.entry(*type_idx).or_default().push(*impl_idx);
            }
        }
        for impls in type_impls.values_mut() {
            impls.sort();
        }
        context.impl_types = impl_types;
        context.type_impls = type_impls;
        context
    }

    /// Kaynağın olgularını kenarlara çevirir.
    pub(crate) fn references(
        &self,
        source_idx: NodeIndex,
        facts: &SyntaxFacts,
    ) -> Vec<(NodeIndex, NodeIndex, EdgeType)> {
        let Some(scope) = self.scope_of(source_idx) else {
            return Vec::new();
        };
        let mut strongest: HashMap<NodeIndex, EdgeType> = HashMap::new();
        let mut referenced: Vec<NodeIndex> = Vec::new();
        for call in &facts.calls {
            let (resolution, types) = self.call(&scope, &call.target, call.line);
            referenced.extend(types);
            for (target, edge) in call_edges(resolution) {
                if target == source_idx {
                    continue;
                }
                let entry = strongest.entry(target).or_insert_with(|| edge.clone());
                if edge_rank(&edge) < edge_rank(entry) {
                    *entry = edge;
                }
            }
        }
        for name in &facts.names {
            for item in self.resolve_segments(&segments(name), &scope, 0) {
                if let Item::Node(idx) | Item::TypeOnly(idx) = item {
                    if is_referable(&self.graph.graph[idx].node_type) {
                        referenced.push(idx);
                    }
                }
            }
        }
        let mut references: Vec<(NodeIndex, NodeIndex, EdgeType)> = strongest
            .iter()
            .map(|(target, edge)| (source_idx, *target, edge.clone()))
            .collect();
        references.extend(
            referenced
                .into_iter()
                .filter(|target| *target != source_idx && !strongest.contains_key(target))
                .map(|target| (source_idx, target, EdgeType::References)),
        );
        references.extend(self.import_edges(&scope, &facts.imports));
        for base in &facts.bases {
            for item in self.resolve_segments(&target_segments(base), &scope, 0) {
                if let Item::Node(idx) = item {
                    if self.graph.graph[idx].node_type == NodeType::Trait && idx != source_idx {
                        references.push((source_idx, idx, EdgeType::Inherits));
                    }
                }
            }
        }
        references.sort_by_key(|(_, target, edge)| (target.index(), edge_rank(edge)));
        references.dedup();
        references
    }

    /// `impl` bloğunun türü, bloğun kapsamında çözülmüş; proje dışı ya da generic
    /// tür (`impl<T> Tr for T`) için boş.
    fn implemented_types(&self, impl_idx: NodeIndex) -> Vec<NodeIndex> {
        let ReferenceFacts::Syntax(facts) = &self.graph.graph[impl_idx].facts else {
            return Vec::new();
        };
        let (Some(target), Some(scope)) = (&facts.impl_type, self.scope_of(impl_idx)) else {
            return Vec::new();
        };
        self.resolve_segments(&target_segments(target), &scope, 0)
            .into_iter()
            .filter_map(|item| match item {
                Item::Node(idx)
                    if matches!(
                        self.graph.graph[idx].node_type,
                        NodeType::Struct | NodeType::Enum
                    ) =>
                {
                    Some(idx)
                }
                _ => None,
            })
            .collect()
    }

    /// `impl` bloğunun uyguladığı trait'ler, bloğun kapsamında çözülmüş.
    fn implemented_traits(&self, impl_idx: NodeIndex) -> Vec<NodeIndex> {
        let ReferenceFacts::Syntax(facts) = &self.graph.graph[impl_idx].facts else {
            return Vec::new();
        };
        let Some(scope) = self.scope_of(impl_idx) else {
            return Vec::new();
        };
        facts
            .bases
            .iter()
            .flat_map(|base| self.resolve_segments(&target_segments(base), &scope, 0))
            .filter_map(|item| match item {
                Item::Node(idx) if self.graph.graph[idx].node_type == NodeType::Trait => Some(idx),
                _ => None,
            })
            .collect()
    }

    /// Kaynağın modülü, saran fonksiyonları ve `impl`/`trait` düğümü.
    fn scope_of(&self, source: NodeIndex) -> Option<Scope> {
        let file_id = graph_node_file_path(&self.graph.graph[source].id);
        let file_module = self.crates.module_of_file(file_id)?;
        let mut functions = Vec::new();
        let mut container = None;
        let mut inline_mods = Vec::new();
        let mut current = Some(source);
        while let Some(idx) = current {
            let node = &self.graph.graph[idx];
            match node.node_type {
                NodeType::Function | NodeType::Method => functions.push(idx),
                NodeType::Module => inline_mods.push(node.name.clone()),
                NodeType::Trait => {
                    container.get_or_insert(idx);
                }
                NodeType::Class if is_rust_impl_node(node) => {
                    container.get_or_insert(idx);
                }
                NodeType::File => break,
                _ => {}
            }
            current = self.parent_of(idx);
        }
        inline_mods.reverse();
        let mut path = file_module.path;
        path.extend(inline_mods);
        Some(Scope {
            source,
            functions,
            module: RustModule {
                krate: file_module.krate,
                path,
            },
            container,
        })
    }

    fn parent_of(&self, idx: NodeIndex) -> Option<NodeIndex> {
        self.graph
            .graph
            .edges_directed(idx, Direction::Incoming)
            .find(|edge| matches!(edge.weight(), EdgeType::Contains))
            .map(|edge| edge.source())
    }

    fn children(&self, owner: NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
        self.graph
            .graph
            .edges_directed(owner, Direction::Outgoing)
            .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
            .map(|edge| edge.target())
    }

    fn imports_of(&self, node: NodeIndex) -> &'g [ImportBinding] {
        match &self.graph.graph[node].facts {
            ReferenceFacts::Syntax(facts) => &facts.imports,
            ReferenceFacts::Lexical => &[],
        }
    }

    /// Modülün düğümü: en uzun dosya öneki, kalan bileşenler satır içi `mod`.
    fn module_node(&self, module: &RustModule) -> Option<NodeIndex> {
        for split in (0..=module.path.len()).rev() {
            let Some(file) = self.crates.file_of(module.krate, &module.path[..split]) else {
                continue;
            };
            let mut node = self.graph.find_file_node(&file)?;
            for name in &module.path[split..] {
                node = self.children(node).find(|child| {
                    let child_node = &self.graph.graph[*child];
                    child_node.node_type == NodeType::Module && child_node.name == *name
                })?;
            }
            return Some(node);
        }
        None
    }

    /// Sahibin adı verilen doğrudan tanımları; `impl` blokları tanım değildir.
    fn children_named(&self, owner: NodeIndex, name: &str, module: &RustModule) -> Vec<Item> {
        self.children(owner)
            .filter_map(|child| {
                let node = &self.graph.graph[child];
                if node.name != name || is_rust_impl_node(node) {
                    return None;
                }
                match node.node_type {
                    NodeType::Module => Some(Item::Module(child_module(module, name))),
                    NodeType::Function
                    | NodeType::Method
                    | NodeType::Struct
                    | NodeType::Enum
                    | NodeType::Trait
                    | NodeType::Variable => Some(Item::Node(child)),
                    _ => None,
                }
            })
            .collect()
    }

    /// Modülde `name` (önbellekli): tanımlar, dosya alt modülleri, `use` bağları,
    /// yıldız importları.
    fn items_named(&self, module: &RustModule, name: &str, depth: usize) -> Vec<Item> {
        let key = (module.clone(), name.to_string(), depth);
        let cached = self.items.borrow().get(&key).cloned();
        if let Some(found) = cached {
            return found;
        }
        let found = self.compute_items_named(module, name, depth);
        self.items.borrow_mut().insert(key, found.clone());
        found
    }

    fn compute_items_named(&self, module: &RustModule, name: &str, depth: usize) -> Vec<Item> {
        let Some(node) = self.module_node(module) else {
            return Vec::new();
        };
        let mut found = self.children_named(node, name, module);
        let child = child_module(module, name);
        if !found.contains(&Item::Module(child.clone()))
            && self.crates.file_of(child.krate, &child.path).is_some()
        {
            found.push(Item::Module(child));
        }
        if !found.is_empty() || depth >= MAX_HOPS {
            return found;
        }
        let imports = self.imports_of(node);
        let bound: Vec<Item> = imports
            .iter()
            .filter(|binding| binding.local == name)
            .flat_map(|binding| self.resolve_binding(binding, module, depth + 1))
            .collect();
        if !bound.is_empty() {
            return bound;
        }
        let module_scope = module_scope(module);
        imports
            .iter()
            .filter(|binding| binding.local == "*")
            .flat_map(|binding| {
                self.resolve_segments(&segments(&binding.module), &module_scope, depth + 1)
            })
            .flat_map(|item| match item {
                Item::Module(target) => self.items_named(&target, name, depth + 1),
                Item::Node(_) | Item::TypeOnly(_) => Vec::new(),
            })
            .collect()
    }

    /// `use` bağının hedefi, bağın bulunduğu modüle göre.
    fn resolve_binding(
        &self,
        binding: &ImportBinding,
        module: &RustModule,
        depth: usize,
    ) -> Vec<Item> {
        let Some(symbol) = binding.symbol.as_deref().filter(|symbol| *symbol != "*") else {
            return Vec::new();
        };
        let mut path = segments(&binding.module);
        path.push(symbol.to_string());
        self.resolve_segments(&path, &module_scope(module), depth)
    }

    /// Kapsamda ad: saran fonksiyonların tanımları ve `use` bağları, sonra modül.
    fn lookup(&self, name: &str, scope: &Scope, depth: usize) -> Vec<Item> {
        for function in &scope.functions {
            // `let` bağlamaları öğe değildir; gölgeleme `shadowed_by_let`'tedir.
            let local: Vec<Item> = self
                .children_named(*function, name, &scope.module)
                .into_iter()
                .filter(|item| {
                    !matches!(item, Item::Node(idx)
                        if self.graph.graph[*idx].node_type == NodeType::Variable)
                })
                .collect();
            if !local.is_empty() {
                return local;
            }
            let bound: Vec<Item> = self
                .imports_of(*function)
                .iter()
                .filter(|binding| binding.local == name)
                .flat_map(|binding| self.resolve_binding(binding, &scope.module, depth + 1))
                .collect();
            if !bound.is_empty() {
                return bound;
            }
        }
        self.items_named(&scope.module, name, depth)
    }

    /// Çağrı satırından önce biten aynı adlı `let` bağlaması (`let f = |..| ..;
    /// f(..)`) adı gölgeler; `let x = x();`'in sağ tarafı gölgelenmez.
    fn shadowed_by_let(&self, scope: &Scope, name: &str, line: usize) -> bool {
        scope.functions.first().is_some_and(|function| {
            self.children(*function).any(|child| {
                let node = &self.graph.graph[child];
                node.node_type == NodeType::Variable
                    && node.name == name
                    && node.end_line < line
                    && node.content.trim_start().starts_with("let ")
            })
        })
    }

    /// Yolu baştan sona çözer.
    fn resolve_segments(&self, path: &[String], scope: &Scope, depth: usize) -> Vec<Item> {
        let Some((first, rest)) = path.split_first() else {
            return Vec::new();
        };
        let mut current = self.resolve_first(first, scope, depth);
        for segment in rest {
            let mut next: Vec<Item> = Vec::new();
            for item in current {
                for stepped in self.step(item, segment, depth) {
                    if !next.contains(&stepped) {
                        next.push(stepped);
                    }
                }
            }
            current = next;
            if current.is_empty() {
                break;
            }
        }
        current
    }

    fn resolve_first(&self, first: &str, scope: &Scope, depth: usize) -> Vec<Item> {
        match first {
            "crate" => vec![Item::Module(RustModule {
                krate: scope.module.krate,
                path: Vec::new(),
            })],
            "self" => vec![Item::Module(scope.module.clone())],
            "super" => parent_module(&scope.module)
                .map(Item::Module)
                .into_iter()
                .collect(),
            "Self" => self
                .container_types(scope.container)
                .into_iter()
                .map(Item::Node)
                .collect(),
            _ => {
                let scoped = self.lookup(first, scope, depth);
                if !scoped.is_empty() {
                    return scoped;
                }
                self.crates
                    .lib_named(first)
                    .map(|krate| {
                        Item::Module(RustModule {
                            krate,
                            path: Vec::new(),
                        })
                    })
                    .into_iter()
                    .collect()
            }
        }
    }

    /// Bir sonraki yol bileşeni: modülün tanımı ya da türün ilişkili öğesi.
    fn step(&self, item: Item, segment: &str, depth: usize) -> Vec<Item> {
        match item {
            Item::Module(module) => match segment {
                "super" => parent_module(&module)
                    .map(Item::Module)
                    .into_iter()
                    .collect(),
                "self" => vec![Item::Module(module)],
                _ => self.items_named(&module, segment, depth),
            },
            Item::Node(idx) => match self.graph.graph[idx].node_type {
                NodeType::Struct | NodeType::Enum | NodeType::Trait => {
                    let members = self.associated(idx, segment);
                    if members.is_empty() {
                        vec![Item::TypeOnly(idx)]
                    } else {
                        members.into_iter().map(Item::Node).collect()
                    }
                }
                _ => Vec::new(),
            },
            Item::TypeOnly(idx) => vec![Item::TypeOnly(idx)],
        }
    }

    /// `Self` türü: saran `impl`'in çözülmüş türü ya da saran `trait`.
    fn container_types(&self, container: Option<NodeIndex>) -> Vec<NodeIndex> {
        let Some(container) = container else {
            return Vec::new();
        };
        if self.graph.graph[container].node_type == NodeType::Trait {
            return vec![container];
        }
        self.impl_types.get(&container).cloned().unwrap_or_default()
    }

    /// Türün ilişkili fonksiyonları: trait'in kendi metotları ya da yapı/enum'un
    /// `impl` metotları.
    fn associated(&self, type_idx: NodeIndex, name: &str) -> Vec<NodeIndex> {
        if self.graph.graph[type_idx].node_type == NodeType::Trait {
            return self.functions_named(type_idx, name);
        }
        self.methods_of_type(type_idx, name)
    }

    /// Türü uygulayan `impl` bloklarının metotları; bulunamazsa bu blokların
    /// uyguladığı trait'lerin varsayılan metotları.
    fn methods_of_type(&self, type_idx: NodeIndex, name: &str) -> Vec<NodeIndex> {
        let impls: &[NodeIndex] = self
            .type_impls
            .get(&type_idx)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let found: Vec<NodeIndex> = impls
            .iter()
            .flat_map(|impl_idx| self.functions_named(*impl_idx, name))
            .collect();
        if !found.is_empty() {
            return found;
        }
        let mut defaults: Vec<NodeIndex> = impls
            .iter()
            .flat_map(|impl_idx| self.implemented_traits(*impl_idx))
            .flat_map(|trait_idx| self.functions_named(trait_idx, name))
            .collect();
        defaults.sort();
        defaults.dedup();
        defaults
    }

    fn functions_named(&self, owner: NodeIndex, name: &str) -> Vec<NodeIndex> {
        self.children(owner)
            .filter(|child| {
                let node = &self.graph.graph[*child];
                node.name == name && matches!(node.node_type, NodeType::Function | NodeType::Method)
            })
            .collect()
    }

    /// Çağrının çözümü ve üyesi bulunamayan yol türleri (referans kenarı alır).
    fn call(
        &self,
        scope: &Scope,
        target: &CallTarget,
        line: usize,
    ) -> (Resolution, Vec<NodeIndex>) {
        match target {
            CallTarget::Bare(name) => {
                if self.shadowed_by_let(scope, name, line) {
                    return (Resolution::External, Vec::new());
                }
                callable(self.graph, self.lookup(name, scope, 0))
            }
            CallTarget::Member { qualifier, name } => {
                let mut path = segments(qualifier);
                path.push(name.clone());
                callable(self.graph, self.resolve_segments(&path, scope, 0))
            }
            CallTarget::SelfMember(name) => {
                let found = self.self_members(scope, name);
                if found.is_empty() {
                    (self.possible(name, scope.source), Vec::new())
                } else {
                    (Resolution::Exact(found), Vec::new())
                }
            }
            // Rust çıkarımı `SuperMember` üretmez (Python `super()`); alıcısı
            // bilinmeyen çağrı gibi ele alınır.
            CallTarget::Chained(name) | CallTarget::SuperMember(name) => {
                (self.possible(name, scope.source), Vec::new())
            }
        }
    }

    /// `self.ad()` / `Self::ad()`: aynı `impl` bloğundaki metot, yoksa bloğun
    /// türünün metotları; tür çözülemezse bloğun trait varsayılanları. Saran trait
    /// için trait'in metotları.
    fn self_members(&self, scope: &Scope, name: &str) -> Vec<NodeIndex> {
        let Some(container) = scope.container else {
            return Vec::new();
        };
        if self.graph.graph[container].node_type == NodeType::Trait {
            return self.functions_named(container, name);
        }
        let same_block = self.functions_named(container, name);
        if !same_block.is_empty() {
            return same_block;
        }
        let types = self.container_types(Some(container));
        let mut found: Vec<NodeIndex> = types
            .iter()
            .flat_map(|type_idx| self.methods_of_type(*type_idx, name))
            .collect();
        if types.is_empty() {
            found = self
                .implemented_traits(container)
                .into_iter()
                .flat_map(|trait_idx| self.functions_named(trait_idx, name))
                .collect();
        }
        found.sort();
        found.dedup();
        found
    }

    /// Alıcısının türü bilinmeyen metot çağrısı: Rust `impl`/`trait` üyesi aynı
    /// adlı metotlar, en çok `MAX_POSSIBLE_TARGETS` aday varsa.
    fn possible(&self, name: &str, source: NodeIndex) -> Resolution {
        let candidates: Vec<NodeIndex> = self
            .graph
            .find_nodes_by_name(name)
            .iter()
            .copied()
            .filter(|idx| {
                let node = &self.graph.graph[*idx];
                *idx != source
                    && matches!(node.node_type, NodeType::Function | NodeType::Method)
                    && graph_node_file_path(&node.id).ends_with(".rs")
                    && self.parent_of(*idx).is_some_and(|parent| {
                        let parent_node = &self.graph.graph[parent];
                        is_rust_impl_node(parent_node) || parent_node.node_type == NodeType::Trait
                    })
            })
            .collect();
        if candidates.is_empty() || candidates.len() > MAX_POSSIBLE_TARGETS {
            Resolution::External
        } else {
            Resolution::Possible(candidates)
        }
    }

    /// `use` bağlarının `Imports` kenarları; modül importu modülün düğümüne
    /// gider. Yıldız importları kenar üretmez.
    fn import_edges(
        &self,
        scope: &Scope,
        imports: &[ImportBinding],
    ) -> Vec<(NodeIndex, NodeIndex, EdgeType)> {
        let mut imported: HashMap<NodeIndex, EdgeType> = HashMap::new();
        for binding in imports.iter().filter(|binding| binding.local != "*") {
            let targets: Vec<NodeIndex> = self
                .resolve_binding(binding, &scope.module, 0)
                .into_iter()
                .filter_map(|item| match item {
                    Item::Node(idx) => Some(idx),
                    Item::Module(module) => self.module_node(&module),
                    Item::TypeOnly(_) => None,
                })
                .filter(|target| *target != scope.source)
                .collect();
            let edge = if targets.len() == 1 {
                EdgeType::Imports
            } else {
                EdgeType::ImportAmbiguous
            };
            for target in targets {
                let entry = imported.entry(target).or_insert_with(|| edge.clone());
                if matches!(edge, EdgeType::Imports) {
                    *entry = EdgeType::Imports;
                }
            }
        }
        imported
            .into_iter()
            .map(|(target, edge)| (scope.source, target, edge))
            .collect()
    }
}

/// Artımlı yenilemede etkilenen adların Rust kapanışı: yolu ya da sembolü
/// etkilenen bir adı anan `use` bağının yerel adı da etkilenir (yeniden dışa
/// aktarma ve crate adı zincirleri, en çok `MAX_HOPS` tur).
pub(crate) fn expand_affected_names(graph: &CodeGraph, names: &HashSet<String>) -> HashSet<String> {
    let bindings: Vec<&ImportBinding> = rust_bindings(graph).collect();
    let mut expanded = names.clone();
    for _ in 0..MAX_HOPS {
        let before = expanded.len();
        for binding in &bindings {
            if binding.local != "*" && binding_mentions(binding, &expanded) {
                expanded.insert(binding.local.clone());
            }
        }
        if expanded.len() == before {
            break;
        }
    }
    expanded
}

/// Yolu etkilenen bir adı anan yıldız importunun dosyaları: yıldızla gelen
/// adlar olgularda görünmediğinden dosyanın tüm kaynakları yeniden çözülür.
pub(crate) fn files_with_affected_globs(
    graph: &CodeGraph,
    names: &HashSet<String>,
) -> HashSet<String> {
    graph
        .graph
        .node_weights()
        .filter_map(|node| match &node.facts {
            ReferenceFacts::Syntax(facts)
                if facts
                    .imports
                    .iter()
                    .any(|binding| binding.local == "*" && binding_mentions(binding, names)) =>
            {
                Some(graph_node_file_path(&node.id).to_string())
            }
            _ => None,
        })
        .collect()
}

/// Modül yapısı değişebilen paketlerin Rust dosyaları: değişen dosya bir crate
/// kökü ya da `mod` bildirimi taşıyorsa paketindeki tüm `.rs` dosyaları (dosyaların
/// crate'i değişebilir); adı etkilenen bir modül adıyla eşleşen dosyalar da
/// (kaldırılan `mod x;` bildirimi) yeniden çözülür.
pub(crate) fn files_needing_module_refresh(
    graph: &CodeGraph,
    changed_files: &HashSet<String>,
    names: &HashSet<String>,
) -> HashSet<String> {
    let rust_files: Vec<&String> = graph
        .file_nodes_index
        .keys()
        .filter(|file| file.ends_with(".rs"))
        .collect();
    let packages: HashSet<String> = changed_files
        .iter()
        .filter(|file| file.ends_with(".rs"))
        .filter(|file| {
            is_crate_root_path(file)
                || graph
                    .find_nodes_by_file(file)
                    .iter()
                    .any(|idx| graph.graph[*idx].node_type == NodeType::Module)
        })
        .map(|file| package_dir(graph, file))
        .collect();
    rust_files
        .into_iter()
        .filter(|file| {
            packages.iter().any(|package| is_in_package(file, package))
                || module_stem(file).is_some_and(|stem| names.contains(stem))
        })
        .cloned()
        .collect()
}

/// Crate kökü olabilen dosya: `lib.rs`, `main.rs` ya da `bin`/`tests`/`benches`/
/// `examples` altındaki doğrudan dosya.
fn is_crate_root_path(file: &str) -> bool {
    let mut parts = file.rsplit('/');
    let name = parts.next().unwrap_or(file);
    let parent = parts.next().unwrap_or("");
    name == "lib.rs"
        || name == "main.rs"
        || matches!(parent, "bin" | "tests" | "benches" | "examples")
}

/// Dosyanın paket dizini: `Cargo.toml` taşıyan en yakın üst dizin, yoksa kök.
fn package_dir(graph: &CodeGraph, file: &str) -> String {
    let mut dir = file;
    while let Some((parent, _)) = dir.rsplit_once('/') {
        if graph
            .file_nodes_index
            .contains_key(&format!("{parent}/Cargo.toml"))
        {
            return parent.to_string();
        }
        dir = parent;
    }
    ".".to_string()
}

fn is_in_package(file: &str, package: &str) -> bool {
    package == "."
        || file
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Dosyanın modül adı: `x.rs` → `x`, `x/mod.rs` → `x`.
fn module_stem(file: &str) -> Option<&str> {
    let without = file.strip_suffix(".rs")?;
    let (parent, name) = without.rsplit_once('/')?;
    if name == "mod" {
        return parent.rsplit_once('/').map(|(_, dir)| dir);
    }
    Some(name)
}

fn rust_bindings(graph: &CodeGraph) -> impl Iterator<Item = &ImportBinding> {
    graph
        .graph
        .node_weights()
        .flat_map(|node| match &node.facts {
            ReferenceFacts::Syntax(facts) if facts.language == SyntaxLanguage::Rust => {
                facts.imports.as_slice()
            }
            _ => &[],
        })
}

fn binding_mentions(binding: &ImportBinding, names: &HashSet<String>) -> bool {
    binding.module.split('.').any(|part| names.contains(part))
        || binding
            .symbol
            .as_ref()
            .is_some_and(|symbol| names.contains(symbol))
}

/// `a.b.c` → `[a, b, c]`; boş yol bileşensizdir.
fn segments(path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }
    path.split('.').map(str::to_string).collect()
}

/// Taban ya da `impl` türü ifadesinin yol bileşenleri.
fn target_segments(target: &CallTarget) -> Vec<String> {
    match target {
        CallTarget::Member { qualifier, name } => {
            let mut parts = segments(qualifier);
            parts.push(name.clone());
            parts
        }
        other => vec![other.name().to_string()],
    }
}

/// Çağrılabilen düğüm türleri: yapı adı yapıcı olarak çağrılır.
fn is_callable(node_type: &NodeType) -> bool {
    matches!(
        node_type,
        NodeType::Function | NodeType::Method | NodeType::Struct
    )
}

/// Ad olarak anılınca referans kenarı alan düğüm türleri.
fn is_referable(node_type: &NodeType) -> bool {
    matches!(
        node_type,
        NodeType::Function
            | NodeType::Method
            | NodeType::Struct
            | NodeType::Enum
            | NodeType::Trait
            | NodeType::Variable
    )
}

/// Fonksiyonsuz modül kapsamı: `use` yolları bulundukları modüle görelidir.
fn module_scope(module: &RustModule) -> Scope {
    Scope {
        source: NodeIndex::end(),
        functions: Vec::new(),
        module: module.clone(),
        container: None,
    }
}

/// Çözülen öğelerden çağrı hedefleri; üyesi bulunamayan türler referans olur.
fn callable(graph: &CodeGraph, items: Vec<Item>) -> (Resolution, Vec<NodeIndex>) {
    let mut targets = Vec::new();
    let mut types = Vec::new();
    for item in items {
        match item {
            Item::Node(idx) if is_callable(&graph.graph[idx].node_type) => targets.push(idx),
            Item::TypeOnly(idx) => types.push(idx),
            Item::Node(_) | Item::Module(_) => {}
        }
    }
    if targets.is_empty() {
        (Resolution::External, types)
    } else {
        (Resolution::Exact(targets), types)
    }
}

fn child_module(module: &RustModule, name: &str) -> RustModule {
    let mut path = module.path.clone();
    path.push(name.to_string());
    RustModule {
        krate: module.krate,
        path,
    }
}

fn parent_module(module: &RustModule) -> Option<RustModule> {
    let (_, parent) = module.path.split_last()?;
    Some(RustModule {
        krate: module.krate,
        path: parent.to_vec(),
    })
}
