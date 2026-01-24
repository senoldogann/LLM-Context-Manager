use crate::graph::{CodeNode, EdgeType};
use anyhow::{Context, Result};
use sled::{Db, Tree};
use std::path::Path;

/// Trait for backing the CodeGraph with persistent storage.
pub trait GraphStorage: Send + Sync {
    fn save_node(&self, node: &CodeNode) -> Result<()>;
    fn save_edge(&self, source_id: &str, target_id: &str, weight: EdgeType) -> Result<()>;
    fn get_node(&self, id: &str) -> Result<Option<CodeNode>>;
    fn get_edges(&self, source_id: &str) -> Result<Vec<(String, EdgeType)>>;
    fn flush(&self) -> Result<()>;
}

/// Sled-based implementation of GraphStorage.
///
/// Schema:
/// - `nodes`: NodeID (String) -> CodeNode (Bincode)
/// - `edges`: SourceID (String) -> Vec<(TargetID, EdgeType)> (Bincode)
/// - `meta`: "node_count" -> u64
pub struct SledStorage {
    db: Db,
    nodes_tree: Tree,
    edges_tree: Tree,
}

impl SledStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path).context("Failed to open Sled database")?;
        let nodes_tree = db.open_tree("nodes")?;
        let edges_tree = db.open_tree("edges")?;

        Ok(Self {
            db,
            nodes_tree,
            edges_tree,
        })
    }
}

impl GraphStorage for SledStorage {
    fn save_node(&self, node: &CodeNode) -> Result<()> {
        let key = node.id.as_bytes();
        let value = bincode::serialize(node).context("Failed to serialize node")?;
        self.nodes_tree.insert(key, value)?;
        Ok(())
    }

    fn save_edge(&self, source_id: &str, target_id: &str, weight: EdgeType) -> Result<()> {
        let key = source_id.as_bytes();

        // Load existing edges for this source
        let mut edges: Vec<(String, EdgeType)> = match self.edges_tree.get(key)? {
            Some(ivec) => bincode::deserialize(&ivec).context("Failed to deserialize edges")?,
            None => Vec::new(),
        };

        // Add new edge if strict duplicate check passes?
        // For now, we append. Graph logic handles unique constraints usually.
        edges.push((target_id.to_string(), weight));

        let value = bincode::serialize(&edges).context("Failed to serialize edge list")?;
        self.edges_tree.insert(key, value)?;
        Ok(())
    }

    fn get_node(&self, id: &str) -> Result<Option<CodeNode>> {
        match self.nodes_tree.get(id.as_bytes())? {
            Some(ivec) => {
                let node: CodeNode = bincode::deserialize(&ivec)?;
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    fn get_edges(&self, source_id: &str) -> Result<Vec<(String, EdgeType)>> {
        match self.edges_tree.get(source_id.as_bytes())? {
            Some(ivec) => {
                let edges: Vec<(String, EdgeType)> = bincode::deserialize(&ivec)?;
                Ok(edges)
            }
            None => Ok(Vec::new()),
        }
    }

    fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
}
