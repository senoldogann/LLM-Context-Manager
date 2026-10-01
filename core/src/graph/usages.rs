//! Bir düğümü kullananlar: ilişki türüyle ve kesinlik sırasıyla.

use std::fmt;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::{CodeGraph, CodeNode, EdgeType};

/// Kullanımın türü; sıra kesinlik sırasıdır (en kesin önce).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsageRelation {
    /// Çağrı hedefi kapsam kurallarıyla (import, yerel tanım, self/super) çözüldü.
    Calls,
    /// Import edilmemiş tek proje tanımına bağlandı.
    CallsInferred,
    /// Alıcı türü bilinmiyor ya da birden çok aday var.
    MayCall,
    /// Sembolü import ediyor.
    Imports,
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
            UsageRelation::Imports => "imports",
            UsageRelation::Inherits => "inherits",
        }
    }

    fn from_edge(edge: &EdgeType) -> Option<Self> {
        match edge {
            EdgeType::Calls => Some(UsageRelation::Calls),
            EdgeType::CallInferred => Some(UsageRelation::CallsInferred),
            EdgeType::CallAmbiguous => Some(UsageRelation::MayCall),
            EdgeType::Imports | EdgeType::ImportAmbiguous => Some(UsageRelation::Imports),
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
    let mut usages: Vec<Usage> = graph
        .graph
        .edges_directed(idx, Direction::Incoming)
        .filter_map(|edge| {
            UsageRelation::from_edge(edge.weight()).map(|relation| Usage {
                node: graph.graph[edge.source()].clone(),
                relation,
            })
        })
        .collect();
    usages.sort_by(|left, right| {
        (left.relation, &left.node.id).cmp(&(right.relation, &right.node.id))
    });
    usages.dedup_by(|left, right| left.relation == right.relation && left.node.id == right.node.id);
    Ok(UsageReport {
        target: graph.graph[idx].clone(),
        usages,
    })
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
