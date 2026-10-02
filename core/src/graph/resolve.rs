//! Python sözdizimi olgularını kenarlara çeviren çözümleyici.
//!
//! Kurallar sırayla denenir: aynı dosyadaki tanım, import bağı (paket yeniden dışa
//! aktarması dahil), `self`/`cls`/`super()` ve sınıf üyeleri. Bunlar kesin kenar
//! üretir. Alıcısı bilinmeyen çağrılar az sayıda adaya "olası", import edilmemiş
//! tek proje tanımına düşen çıplak adlar "çıkarım" kenarı üretir. Projeden çıkan
//! importlar ve yerleşik adlar kenar üretmez.

use std::collections::{HashMap, HashSet};

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::references::{
    python_module_path, CallTarget, ImportBinding, ReferenceFacts, SyntaxFacts,
};
use super::{graph_node_file_path, is_reference_target_type, CodeGraph, EdgeType, NodeType};

/// Alıcısı bilinmeyen bir çağrının en çok kaç tanıma "olası" kenar üreteceği;
/// daha çok aday varsa çağrı hangi tanımı kastettiği hakkında bilgi taşımaz.
pub(crate) const MAX_POSSIBLE_TARGETS: usize = 5;
/// Paket `__init__.py` yeniden dışa aktarma zincirinde izlenecek en çok adım.
const MAX_REEXPORT_DEPTH: usize = 3;
/// Taban sınıf zincirinde aranacak en çok adım.
const MAX_BASE_DEPTH: usize = 3;

/// Python yerleşik adları: projede tanımlı ya da import edilmiş değilse kenar üretmez.
const PYTHON_BUILTINS: &[&str] = &[
    "abs",
    "aiter",
    "all",
    "anext",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
    "__import__",
];

/// Bir çağrının çözüm sonucu.
enum Resolution {
    /// Kapsam kurallarıyla bulunan hedef(ler).
    Exact(Vec<NodeIndex>),
    /// Import edilmemiş ama projede tek tanımı olan ad.
    Inferred(NodeIndex),
    /// Alıcısı bilinmeyen çağrının az sayıdaki adayı.
    Possible(Vec<NodeIndex>),
    /// Proje dışı, yerleşik ya da bilgi taşımayan çağrı.
    External,
}

/// Noktalı modül yolunu Python dosyalarına eşler.
pub(crate) struct PythonModules {
    /// Son bileşen → (tüm bileşenler, dosya kimliği)
    by_last: HashMap<String, Vec<(Vec<String>, String)>>,
    /// `__init__.py` taşıyan dizinler (paketler), modül yolu bileşenleri olarak.
    packages: HashSet<Vec<String>>,
}

impl PythonModules {
    pub(crate) fn new(graph: &CodeGraph) -> Self {
        let mut by_last: HashMap<String, Vec<(Vec<String>, String)>> = HashMap::new();
        let mut packages: HashSet<Vec<String>> = HashSet::new();
        for file_id in graph.file_nodes_index.keys() {
            let Some(parts) = python_module_path(file_id) else {
                continue;
            };
            if file_id.ends_with("__init__.py") {
                packages.insert(parts.clone());
            }
            if let Some(last) = parts.last().cloned() {
                by_last
                    .entry(last)
                    .or_default()
                    .push((parts, file_id.clone()));
            }
        }
        Self { by_last, packages }
    }

    /// Yolun dosyaları: kök göreli tam eşleşme varsa yalnız o; yoksa yolu sonek
    /// olarak taşıyan dosyalar (`src/` düzeni). Sonek eşleşmesinde atılan önek bir
    /// paket olamaz: `src/flask/json/` standart kütüphanenin `json`'u değildir.
    pub(crate) fn files(&self, module: &str) -> Vec<&str> {
        let wanted: Vec<&str> = module.split('.').collect();
        let Some(candidates) = wanted.last().and_then(|last| self.by_last.get(*last)) else {
            return Vec::new();
        };
        let same = |parts: &[String]| parts.iter().map(String::as_str).eq(wanted.iter().copied());
        let exact: Vec<&str> = candidates
            .iter()
            .filter(|(parts, _)| same(parts))
            .map(|(_, file)| file.as_str())
            .collect();
        if !exact.is_empty() {
            return exact;
        }
        candidates
            .iter()
            .filter(|(parts, _)| {
                parts.len() > wanted.len()
                    && same(&parts[parts.len() - wanted.len()..])
                    && !self.packages.contains(&parts[..parts.len() - wanted.len()])
            })
            .map(|(_, file)| file.as_str())
            .collect()
    }
}

/// Bir kaynağın gördüğü kapsam.
struct Scope<'g> {
    source_idx: NodeIndex,
    file_id: &'g str,
    /// Önce kaynağın kendi bağları, sonra kapsayan düğümlerinki (iç kapsam önce).
    bindings: Vec<&'g ImportBinding>,
    /// Kaynağı içeren en yakın sınıf.
    class_idx: Option<NodeIndex>,
}

impl<'g> Scope<'g> {
    fn of(graph: &'g CodeGraph, source_idx: NodeIndex, facts: &'g SyntaxFacts) -> Self {
        let mut bindings: Vec<&'g ImportBinding> = facts.imports.iter().collect();
        let mut class_idx = None;
        let mut current = parent_of(graph, source_idx);
        while let Some(idx) = current {
            let node = &graph.graph[idx];
            if class_idx.is_none() && node.node_type == NodeType::Class {
                class_idx = Some(idx);
            }
            if let ReferenceFacts::Syntax(outer) = &node.facts {
                bindings.extend(outer.imports.iter());
            }
            current = parent_of(graph, idx);
        }
        Self {
            source_idx,
            file_id: graph_node_file_path(&graph.graph[source_idx].id),
            bindings,
            class_idx,
        }
    }

    fn binding(&self, local: &str) -> Option<&'g ImportBinding> {
        self.bindings
            .iter()
            .copied()
            .find(|binding| binding.local == local)
    }

    fn wildcards(&self) -> impl Iterator<Item = &'g ImportBinding> + '_ {
        self.bindings
            .iter()
            .copied()
            .filter(|binding| binding.symbol.as_deref() == Some("*"))
    }
}

/// Bir Python kaynağının olgularını kenarlara çevirir; her (hedef, tür) bir kez.
pub(crate) fn python_references<'g>(
    graph: &'g CodeGraph,
    modules: &PythonModules,
    source_idx: NodeIndex,
    facts: &'g SyntaxFacts,
) -> Vec<(NodeIndex, NodeIndex, EdgeType)> {
    let scope = Scope::of(graph, source_idx, facts);
    let mut strongest: HashMap<NodeIndex, EdgeType> = HashMap::new();
    for call in &facts.calls {
        for (target, edge) in call_edges(resolve_target(graph, modules, &scope, &call.target)) {
            if target == source_idx {
                continue;
            }
            let entry = strongest.entry(target).or_insert_with(|| edge.clone());
            if edge_rank(&edge) < edge_rank(entry) {
                *entry = edge;
            }
        }
    }
    // Çağrılmadan kullanılan adlar; aynı hedefe çağrı kenarı varsa o yeterlidir.
    let mut referenced: Vec<NodeIndex> = Vec::new();
    for name in &facts.names {
        for target in reference_targets(graph, modules, &scope, name) {
            if target != source_idx && !strongest.contains_key(&target) {
                referenced.push(target);
            }
        }
    }
    let mut references: Vec<(NodeIndex, NodeIndex, EdgeType)> = strongest
        .into_iter()
        .map(|(target, edge)| (source_idx, target, edge))
        .collect();
    references.extend(
        referenced
            .into_iter()
            .map(|target| (source_idx, target, EdgeType::References)),
    );
    let mut imported: HashMap<NodeIndex, EdgeType> = HashMap::new();
    for binding in &facts.imports {
        let Some(symbol) = binding.symbol.as_deref().filter(|symbol| *symbol != "*") else {
            continue;
        };
        let targets = symbol_in_module(graph, modules, &binding.module, symbol, 0);
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
    references.extend(
        imported
            .into_iter()
            .map(|(target, edge)| (source_idx, target, edge)),
    );
    for base in &facts.bases {
        if let Resolution::Exact(targets) = resolve_target(graph, modules, &scope, base) {
            for target in targets {
                if graph.graph[target].node_type == NodeType::Class && target != source_idx {
                    references.push((source_idx, target, EdgeType::Inherits));
                }
            }
        }
    }
    references.sort_by_key(|(_, target, edge)| (target.index(), edge_rank(edge)));
    references.dedup();
    references
}

/// Kesinlik sırası: küçük olan daha güçlüdür.
fn edge_rank(edge: &EdgeType) -> u8 {
    match edge {
        EdgeType::Calls => 0,
        EdgeType::CallInferred => 1,
        EdgeType::CallAmbiguous => 2,
        EdgeType::References => 3,
        EdgeType::Imports => 4,
        EdgeType::ImportAmbiguous => 5,
        EdgeType::Inherits => 6,
        EdgeType::Defines | EdgeType::Contains | EdgeType::Reads | EdgeType::Writes => 7,
    }
}

/// Çağrılmadan kullanılan adın hedefleri: aynı dosyadaki tanım, import bağı ya da
/// yıldız import. Tahmini ya da olası hedef üretmez; çözülemeyen ad yereldir.
fn reference_targets(
    graph: &CodeGraph,
    modules: &PythonModules,
    scope: &Scope<'_>,
    name: &str,
) -> Vec<NodeIndex> {
    let local = module_scope_definitions(graph, scope.file_id, name, scope.source_idx);
    if !local.is_empty() {
        return local;
    }
    if let Some(binding) = scope.binding(name) {
        return match binding.symbol.as_deref() {
            Some(symbol) if symbol != "*" => {
                symbol_in_module(graph, modules, &binding.module, symbol, 0)
            }
            _ => Vec::new(),
        };
    }
    scope
        .wildcards()
        .flat_map(|binding| symbol_in_module(graph, modules, &binding.module, name, 0))
        .collect()
}

fn call_edges(resolution: Resolution) -> Vec<(NodeIndex, EdgeType)> {
    match resolution {
        Resolution::Exact(targets) if targets.len() == 1 => vec![(targets[0], EdgeType::Calls)],
        Resolution::Exact(targets) | Resolution::Possible(targets) => targets
            .into_iter()
            .map(|target| (target, EdgeType::CallAmbiguous))
            .collect(),
        Resolution::Inferred(target) => vec![(target, EdgeType::CallInferred)],
        Resolution::External => Vec::new(),
    }
}

fn resolve_target(
    graph: &CodeGraph,
    modules: &PythonModules,
    scope: &Scope<'_>,
    target: &CallTarget,
) -> Resolution {
    match target {
        CallTarget::Bare(name) => resolve_bare(graph, modules, scope, name),
        CallTarget::SelfMember(name) => match scope.class_idx {
            Some(class_idx) => exact_or_possible(
                graph,
                scope,
                name,
                members_with_bases(graph, modules, class_idx, name, 0),
            ),
            None => possible(graph, scope.source_idx, name),
        },
        CallTarget::SuperMember(name) => match scope.class_idx {
            Some(class_idx) => exact_or_possible(
                graph,
                scope,
                name,
                base_members(graph, modules, class_idx, name, 0),
            ),
            None => possible(graph, scope.source_idx, name),
        },
        CallTarget::Member { qualifier, name } => {
            resolve_member(graph, modules, scope, qualifier, name)
        }
        CallTarget::Chained(name) => possible(graph, scope.source_idx, name),
    }
}

fn resolve_bare(
    graph: &CodeGraph,
    modules: &PythonModules,
    scope: &Scope<'_>,
    name: &str,
) -> Resolution {
    let local = module_scope_definitions(graph, scope.file_id, name, scope.source_idx);
    if !local.is_empty() {
        return Resolution::Exact(local);
    }
    // Özyineleme: ad kaynağın kendisiyse başka dosyadaki aynı adlı tanıma düşülmez.
    if refers_to_itself(graph, scope.source_idx, name) {
        return Resolution::External;
    }
    if let Some(binding) = scope.binding(name) {
        return match binding.symbol.as_deref() {
            Some(symbol) if symbol != "*" => {
                let targets = symbol_in_module(graph, modules, &binding.module, symbol, 0);
                if !targets.is_empty() {
                    Resolution::Exact(targets)
                } else if modules.files(&binding.module).is_empty() {
                    Resolution::External
                } else {
                    // Modül projede ama ad bulunamadı (`__all__`, dinamik dışa
                    // aktarma): projede tek tanım varsa çıkarım olarak bağlanır.
                    unique_python_definition(graph, scope.source_idx, symbol)
                        .map_or(Resolution::External, Resolution::Inferred)
                }
            }
            // Modül adı çağrılmaz.
            _ => Resolution::External,
        };
    }
    for binding in scope.wildcards() {
        let targets = symbol_in_module(graph, modules, &binding.module, name, 0);
        if !targets.is_empty() {
            return Resolution::Exact(targets);
        }
    }
    if PYTHON_BUILTINS.contains(&name) {
        return Resolution::External;
    }
    unique_python_definition(graph, scope.source_idx, name)
        .map_or(Resolution::External, Resolution::Inferred)
}

fn resolve_member(
    graph: &CodeGraph,
    modules: &PythonModules,
    scope: &Scope<'_>,
    qualifier: &str,
    name: &str,
) -> Resolution {
    let (head, rest) = match qualifier.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (qualifier, None),
    };
    if let Some(binding) = scope.binding(head) {
        let mut path = match binding.symbol.as_deref() {
            Some(symbol) if symbol != "*" => format!("{}.{symbol}", binding.module),
            _ => binding.module.clone(),
        };
        if let Some(rest) = rest {
            path.push('.');
            path.push_str(rest);
        }
        let in_module = symbol_in_module(graph, modules, &path, name, 0);
        if !in_module.is_empty() {
            return Resolution::Exact(in_module);
        }
        // Bulunamadıysa proje dışı bir modül ya da sınıftır.
        return non_empty_exact(class_members_at(graph, modules, &path, name));
    }
    if rest.is_none() {
        let classes: Vec<NodeIndex> =
            module_scope_definitions(graph, scope.file_id, head, scope.source_idx)
                .into_iter()
                .filter(|idx| graph.graph[*idx].node_type == NodeType::Class)
                .collect();
        if !classes.is_empty() {
            let members: Vec<NodeIndex> = classes
                .into_iter()
                .flat_map(|class_idx| members_with_bases(graph, modules, class_idx, name, 0))
                .collect();
            return exact_or_possible(graph, scope, name, members);
        }
    }
    possible(graph, scope.source_idx, name)
}

fn non_empty_exact(targets: Vec<NodeIndex>) -> Resolution {
    if targets.is_empty() {
        Resolution::External
    } else {
        Resolution::Exact(targets)
    }
}

fn exact_or_possible(
    graph: &CodeGraph,
    scope: &Scope<'_>,
    name: &str,
    targets: Vec<NodeIndex>,
) -> Resolution {
    if targets.is_empty() {
        possible(graph, scope.source_idx, name)
    } else {
        Resolution::Exact(targets)
    }
}

/// Alıcısı bilinmeyen öznitelik çağrısı (`x.ad()`): aynı adlı Python sınıf
/// metotları, en çok `MAX_POSSIBLE_TARGETS` aday varsa. Modül düzeyindeki
/// fonksiyonlar aday değildir: modül alıcıları import bağıyla zaten kesin
/// çözülür; kalan eşleşmeler (`", ".join` → şablon filtresi `join`) yanlıştır.
fn possible(graph: &CodeGraph, source_idx: NodeIndex, name: &str) -> Resolution {
    let candidates: Vec<NodeIndex> = graph
        .find_nodes_by_name(name)
        .iter()
        .copied()
        .filter(|idx| {
            *idx != source_idx
                && matches!(
                    graph.graph[*idx].node_type,
                    NodeType::Function | NodeType::Method
                )
                && graph_node_file_path(&graph.graph[*idx].id).ends_with(".py")
                && is_class_member(graph, *idx)
        })
        .collect();
    if candidates.is_empty() || candidates.len() > MAX_POSSIBLE_TARGETS {
        Resolution::External
    } else {
        Resolution::Possible(candidates)
    }
}

/// Dosyada çıplak adla görünen tanımlar: sınıf üyesi olmayan fonksiyon ve sınıflar.
fn module_scope_definitions(
    graph: &CodeGraph,
    file_id: &str,
    name: &str,
    exclude: NodeIndex,
) -> Vec<NodeIndex> {
    graph
        .find_nodes_by_name(name)
        .iter()
        .copied()
        .filter(|idx| {
            let node = &graph.graph[*idx];
            *idx != exclude
                && is_reference_target_type(&node.node_type)
                && graph_node_file_path(&node.id) == file_id
                && !is_class_member(graph, *idx)
        })
        .collect()
}

/// Projede sınıf üyesi olmayan tek Python tanımı.
fn unique_python_definition(
    graph: &CodeGraph,
    source_idx: NodeIndex,
    name: &str,
) -> Option<NodeIndex> {
    let mut candidates = graph
        .find_nodes_by_name(name)
        .iter()
        .copied()
        .filter(|idx| {
            let node = &graph.graph[*idx];
            *idx != source_idx
                && is_reference_target_type(&node.node_type)
                && graph_node_file_path(&node.id).ends_with(".py")
                && !is_class_member(graph, *idx)
        });
    match (candidates.next(), candidates.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Modülün üst düzeyindeki `symbol`; yoksa paketin yeniden dışa aktardığı tanım.
fn symbol_in_module(
    graph: &CodeGraph,
    modules: &PythonModules,
    module: &str,
    symbol: &str,
    depth: usize,
) -> Vec<NodeIndex> {
    let mut targets = Vec::new();
    for file_id in modules.files(module) {
        let defined = top_level_named(graph, file_id, symbol);
        if !defined.is_empty() {
            targets.extend(defined);
            continue;
        }
        if depth >= MAX_REEXPORT_DEPTH {
            continue;
        }
        let Some(file_idx) = graph.find_file_node(file_id) else {
            continue;
        };
        let ReferenceFacts::Syntax(file_facts) = &graph.graph[file_idx].facts else {
            continue;
        };
        let mut reexported = Vec::new();
        for binding in file_facts
            .imports
            .iter()
            .filter(|binding| binding.local == symbol)
        {
            if let Some(original) = binding
                .symbol
                .as_deref()
                .filter(|original| *original != "*")
            {
                reexported.extend(symbol_in_module(
                    graph,
                    modules,
                    &binding.module,
                    original,
                    depth + 1,
                ));
            }
        }
        // `from .alt import *` yeniden dışa aktarması: `__all__` olmadan Python
        // alt çizgiyle başlamayan adları taşır (`__all__` M3'te).
        if reexported.is_empty() && !symbol.starts_with('_') {
            for binding in file_facts
                .imports
                .iter()
                .filter(|binding| binding.symbol.as_deref() == Some("*"))
            {
                reexported.extend(symbol_in_module(
                    graph,
                    modules,
                    &binding.module,
                    symbol,
                    depth + 1,
                ));
            }
        }
        targets.extend(reexported);
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// `a.b.Klass` yolundaki sınıfın (ve tabanlarının) `name` üyeleri.
fn class_members_at(
    graph: &CodeGraph,
    modules: &PythonModules,
    path: &str,
    name: &str,
) -> Vec<NodeIndex> {
    let Some((module, class_name)) = path.rsplit_once('.') else {
        return Vec::new();
    };
    symbol_in_module(graph, modules, module, class_name, 0)
        .into_iter()
        .filter(|idx| graph.graph[*idx].node_type == NodeType::Class)
        .flat_map(|class_idx| members_with_bases(graph, modules, class_idx, name, 0))
        .collect()
}

/// Sınıfın `name` üyeleri; yoksa tabanlarınınki.
fn members_with_bases(
    graph: &CodeGraph,
    modules: &PythonModules,
    class_idx: NodeIndex,
    name: &str,
    depth: usize,
) -> Vec<NodeIndex> {
    let own = class_members(graph, class_idx, name);
    if !own.is_empty() || depth >= MAX_BASE_DEPTH {
        return own;
    }
    base_members(graph, modules, class_idx, name, depth)
}

/// Sınıfın tabanlarındaki `name` üyeleri (`super()` ve kalıtım).
fn base_members(
    graph: &CodeGraph,
    modules: &PythonModules,
    class_idx: NodeIndex,
    name: &str,
    depth: usize,
) -> Vec<NodeIndex> {
    let ReferenceFacts::Syntax(facts) = &graph.graph[class_idx].facts else {
        return Vec::new();
    };
    let scope = Scope::of(graph, class_idx, facts);
    let mut members = Vec::new();
    for base in &facts.bases {
        if let Resolution::Exact(targets) = resolve_target(graph, modules, &scope, base) {
            for base_idx in targets {
                if graph.graph[base_idx].node_type == NodeType::Class && base_idx != class_idx {
                    members.extend(members_with_bases(
                        graph,
                        modules,
                        base_idx,
                        name,
                        depth + 1,
                    ));
                }
            }
        }
    }
    members
}

fn class_members(graph: &CodeGraph, class_idx: NodeIndex, name: &str) -> Vec<NodeIndex> {
    graph
        .graph
        .edges_directed(class_idx, Direction::Outgoing)
        .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| edge.target())
        .filter(|idx| {
            let node = &graph.graph[*idx];
            node.name == name && matches!(node.node_type, NodeType::Function | NodeType::Method)
        })
        .collect()
}

/// Dosyanın üst düzeyindeki (ebeveyni `File` olan) aynı adlı tanımlar.
fn top_level_named(graph: &CodeGraph, file_id: &str, name: &str) -> Vec<NodeIndex> {
    graph
        .find_nodes_by_name(name)
        .iter()
        .copied()
        .filter(|idx| {
            let node = &graph.graph[*idx];
            is_reference_target_type(&node.node_type)
                && graph_node_file_path(&node.id) == file_id
                && parent_of(graph, *idx)
                    .is_some_and(|parent| graph.graph[parent].node_type == NodeType::File)
        })
        .collect()
}

/// Artımlı yenilemede etkilenen adları Python'un dolaylı bağımlılıklarıyla
/// genişletir: değişen dosyalardaki sınıfların kalıtılan üye adları (değişiklik
/// sonrası durum) ve paket yeniden dışa aktarma zincirlerinin takma adları
/// (`from .core import run as start_app` → `run` etkilenirse `start_app` de).
pub(crate) fn expand_affected_names(
    graph: &CodeGraph,
    changed_files: &HashSet<String>,
    names: &HashSet<String>,
) -> HashSet<String> {
    let mut expanded = names.clone();
    for file_id in changed_files {
        expanded.extend(inherited_member_names_in_file(graph, file_id));
    }
    let bindings: Vec<&ImportBinding> = graph
        .graph
        .node_weights()
        .filter(|node| node.node_type == NodeType::File)
        .filter_map(|node| match &node.facts {
            ReferenceFacts::Syntax(facts) => Some(facts),
            ReferenceFacts::Lexical => None,
        })
        .flat_map(|facts| facts.imports.iter())
        .collect();
    for _ in 0..MAX_REEXPORT_DEPTH {
        let aliases: Vec<String> = bindings
            .iter()
            .filter(|binding| {
                binding
                    .symbol
                    .as_deref()
                    .is_some_and(|symbol| symbol != "*" && expanded.contains(symbol))
                    && !expanded.contains(&binding.local)
            })
            .map(|binding| binding.local.clone())
            .collect();
        if aliases.is_empty() {
            break;
        }
        expanded.extend(aliases);
    }
    expanded
}

/// Dosyadaki sınıfların tabanlarından (en çok `MAX_BASE_DEPTH` adım) kalıtılan
/// üye adları.
pub(crate) fn inherited_member_names_in_file(graph: &CodeGraph, file_id: &str) -> HashSet<String> {
    let classes: Vec<NodeIndex> = graph
        .find_nodes_by_file(file_id)
        .iter()
        .copied()
        .filter(|idx| graph.graph[*idx].node_type == NodeType::Class)
        .collect();
    let mut names = HashSet::new();
    if classes.is_empty() {
        return names;
    }
    let modules = PythonModules::new(graph);
    for class_idx in classes {
        collect_inherited_names(graph, &modules, class_idx, 0, &mut names);
    }
    names
}

fn collect_inherited_names(
    graph: &CodeGraph,
    modules: &PythonModules,
    class_idx: NodeIndex,
    depth: usize,
    names: &mut HashSet<String>,
) {
    if depth >= MAX_BASE_DEPTH {
        return;
    }
    let ReferenceFacts::Syntax(facts) = &graph.graph[class_idx].facts else {
        return;
    };
    let scope = Scope::of(graph, class_idx, facts);
    for base in &facts.bases {
        let Resolution::Exact(targets) = resolve_target(graph, modules, &scope, base) else {
            continue;
        };
        for base_idx in targets {
            if graph.graph[base_idx].node_type != NodeType::Class || base_idx == class_idx {
                continue;
            }
            names.extend(
                graph
                    .graph
                    .edges_directed(base_idx, Direction::Outgoing)
                    .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
                    .map(|edge| graph.graph[edge.target()].name.clone()),
            );
            collect_inherited_names(graph, modules, base_idx, depth + 1, names);
        }
    }
}

/// Çıplak ad, kapsamda kaynağın kendisini mi gösteriyor (sınıf üyesi olmayan
/// fonksiyonun kendini çağırması)?
fn refers_to_itself(graph: &CodeGraph, source_idx: NodeIndex, name: &str) -> bool {
    graph.graph[source_idx].name == name && !is_class_member(graph, source_idx)
}

fn is_class_member(graph: &CodeGraph, idx: NodeIndex) -> bool {
    parent_of(graph, idx).is_some_and(|parent| graph.graph[parent].node_type == NodeType::Class)
}

fn parent_of(graph: &CodeGraph, idx: NodeIndex) -> Option<NodeIndex> {
    graph
        .graph
        .edges_directed(idx, Direction::Incoming)
        .find(|edge| matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| edge.source())
}
