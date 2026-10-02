//! Bir düğümü kullananlar: ilişki türüyle ve kesinlik sırasıyla.

use std::fmt;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::{graph_node_file_path, CodeGraph, CodeNode, EdgeType, NodeType};

/// Kullanımın türü; sıra kesinlik sırasıdır (en kesin önce).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsageRelation {
    /// Çağrı hedefi kapsam kurallarıyla (import, yerel tanım, self/super) çözüldü.
    Calls,
    /// Import edilmemiş tek proje tanımına bağlandı.
    CallsInferred,
    /// Alıcı türü bilinmiyor ya da birden çok aday var.
    MayCall,
    /// Çağırmadan kullanıyor (argüman, öznitelik erişimi, tip ipucu).
    References,
    /// Sembolü import ediyor.
    Imports,
    /// Aynı adlı birden çok tanımdan birini import ediyor olabilir.
    MayImport,
    /// Bu sınıftan türüyor.
    Inherits,
}

impl UsageRelation {
    /// Araç çıktısındaki etiket.
    pub fn label(self) -> &'static str {
        match self {
            UsageRelation::Calls => "calls",
            UsageRelation::CallsInferred => "calls (inferred: unique name, not imported)",
            UsageRelation::MayCall => "may call (receiver type unknown or several candidates)",
            UsageRelation::References => "references (uses without calling)",
            UsageRelation::Imports => "imports",
            UsageRelation::MayImport => "may import (ambiguous name match)",
            UsageRelation::Inherits => "inherits",
        }
    }

    fn from_edge(edge: &EdgeType) -> Option<Self> {
        match edge {
            EdgeType::Calls => Some(UsageRelation::Calls),
            EdgeType::CallInferred => Some(UsageRelation::CallsInferred),
            EdgeType::CallAmbiguous => Some(UsageRelation::MayCall),
            EdgeType::References => Some(UsageRelation::References),
            EdgeType::Imports => Some(UsageRelation::Imports),
            EdgeType::ImportAmbiguous => Some(UsageRelation::MayImport),
            EdgeType::Inherits => Some(UsageRelation::Inherits),
            EdgeType::Defines | EdgeType::Contains | EdgeType::Reads | EdgeType::Writes => None,
        }
    }
}

/// Kullanan düğüm ve ilişkisi.
#[derive(Debug, Clone)]
pub struct Usage {
    pub node: CodeNode,
    pub relation: UsageRelation,
}

/// Hedef ve onu kullananlar.
#[derive(Debug, Clone)]
pub struct UsageReport {
    pub target: CodeNode,
    pub usages: Vec<Usage>,
}

/// Bir sembolün tek bakışta özeti: tanımı, onu kullananlar, onun kullandıkları
/// ve doğrudan içerdiği tanımlar (sınıf üyeleri, dosyanın üst düzey sembolleri).
#[derive(Debug, Clone)]
pub struct Explanation {
    pub node: CodeNode,
    pub usages: Vec<Usage>,
    pub callees: Vec<Usage>,
    pub members: Vec<CodeNode>,
}

/// Kullanım sorgusunun açık hataları.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    /// Kimlik güncel indekste yok: dosya değişmiş ya da sembol silinmiş olabilir.
    NodeNotFound(String),
}

impl fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UsageError::NodeNotFound(id) => write!(
                formatter,
                "node '{id}' is not in the current index; its file may have changed since the ID \
                 was returned. Call find_nodes again to get a current ID."
            ),
        }
    }
}

impl std::error::Error for UsageError {}

/// Düğümü kullananlar; ilişkiye, sonra kullanan kimliğine göre sıralı. Kimlik
/// indekste yoksa boş liste değil `NodeNotFound` döner.
pub fn usages_of(graph: &CodeGraph, node_id: &str) -> Result<UsageReport, UsageError> {
    let idx =
        lookup(graph, node_id).ok_or_else(|| UsageError::NodeNotFound(node_id.to_string()))?;
    Ok(UsageReport {
        target: graph.graph[idx].clone(),
        usages: incoming_usages(graph, idx),
    })
}

/// Sembolün özeti; kullananlar `usages_of` ile, kullandıkları aynı ilişki
/// etiketleri ve sırasıyla. Kimlik indekste yoksa `NodeNotFound` döner.
pub fn explanation_of(graph: &CodeGraph, node_id: &str) -> Result<Explanation, UsageError> {
    let idx =
        lookup(graph, node_id).ok_or_else(|| UsageError::NodeNotFound(node_id.to_string()))?;
    let callees = collect_usages(
        graph,
        graph
            .graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|edge| (edge.weight(), edge.target())),
    );
    // Fonksiyonun yerel değişkenleri ve importları üye değildir; sınıf
    // değişkenleri (ör. model alanları) sınıfın üyesidir.
    let is_callable = matches!(
        graph.graph[idx].node_type,
        NodeType::Function | NodeType::Method
    );
    let mut members: Vec<CodeNode> = graph
        .graph
        .edges_directed(idx, Direction::Outgoing)
        .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| graph.graph[edge.target()].clone())
        .filter(|member| {
            !(is_callable && matches!(member.node_type, NodeType::Variable | NodeType::Import))
        })
        .collect();
    members.sort_by(|left, right| (left.start_line, &left.id).cmp(&(right.start_line, &right.id)));
    members.dedup_by(|left, right| left.id == right.id);
    Ok(Explanation {
        node: graph.graph[idx].clone(),
        usages: incoming_usages(graph, idx),
        callees,
        members,
    })
}

fn incoming_usages(graph: &CodeGraph, idx: NodeIndex) -> Vec<Usage> {
    collect_usages(
        graph,
        graph
            .graph
            .edges_directed(idx, Direction::Incoming)
            .map(|edge| (edge.weight(), edge.source())),
    )
}

/// (kenar türü, karşı düğüm) çiftlerinden kullanım listesi: kullanım olmayan
/// kenarlar atlanır; ilişkiye, sonra konuma (dosya, satır) göre sıralı ve
/// tekrarsız. Kararlı kimlikler özet taşıdığından kimlik sırası rastgeledir.
fn collect_usages<'g>(
    graph: &'g CodeGraph,
    links: impl Iterator<Item = (&'g EdgeType, NodeIndex)>,
) -> Vec<Usage> {
    let mut usages: Vec<Usage> = links
        .filter_map(|(edge, other)| {
            UsageRelation::from_edge(edge).map(|relation| Usage {
                node: graph.graph[other].clone(),
                relation,
            })
        })
        .collect();
    usages.sort_by(|left, right| {
        (
            left.relation,
            graph_node_file_path(&left.node.id),
            left.node.start_line,
            &left.node.id,
        )
            .cmp(&(
                right.relation,
                graph_node_file_path(&right.node.id),
                right.node.start_line,
                &right.node.id,
            ))
    });
    usages.dedup_by(|left, right| left.relation == right.relation && left.node.id == right.node.id);
    usages
}

/// Tam kimlik; kararlı kimliklerde (`:symbol:`) bulanık eşleşme başka bir sembolü
/// döndürebileceği için yalnız eski satır tabanlı kimliklerde kullanılır.
fn lookup(graph: &CodeGraph, node_id: &str) -> Option<NodeIndex> {
    if let Some(idx) = graph.find_node_index_by_id(node_id) {
        return Some(idx);
    }
    if node_id.contains(":symbol:") {
        return None;
    }
    graph
        .find_node_fuzzy_by_id(node_id)
        .and_then(|node| graph.find_node_index_by_id(&node.id))
}
