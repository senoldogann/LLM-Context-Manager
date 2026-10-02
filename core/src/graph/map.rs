//! Projenin haritası: kod dosyaları, diğer dosyaların onları ne kadar kullandığına
//! göre sıralı; her dosyada en çok kullanılan semboller.

use std::fmt;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::{
    graph_node_file_path, is_reference_target_type, is_rust_impl_node, CodeGraph, EdgeType,
    NodeType,
};

/// Bir dosya satırında gösterilen en çok sembol sayısı.
const SYMBOLS_PER_FILE: usize = 5;
/// Bütçe tahmini: bir token yaklaşık dört karakter.
const CHARS_PER_TOKEN: usize = 4;

/// Harita sorgusunun açık hataları.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// İndekste hiç kod dosyası yok.
    EmptyIndex,
    /// Önekin altında indekslenmiş kod dosyası yok.
    NoCodeUnder(String),
}

impl fmt::Display for MapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::EmptyIndex => {
                write!(
                    formatter,
                    "the index has no code files; run index_project first"
                )
            }
            MapError::NoCodeUnder(prefix) => write!(
                formatter,
                "no indexed code under {prefix}; pass a directory relative to the project root"
            ),
        }
    }
}

impl std::error::Error for MapError {}

/// Sembol ve diğer dosyalardan aldığı kesin kullanım sayısı.
struct RankedSymbol {
    name: String,
    uses: usize,
    start_line: usize,
}

/// Dosya ve sembolleri; dosyanın ağırlığı sembol kullanımlarının toplamıdır.
struct RankedFile {
    path: String,
    uses: usize,
    symbols: Vec<RankedSymbol>,
}

/// `prefix` altındaki kod dosyalarının `max_tokens` bütçesindeki haritası.
///
/// Başlık dosya, sembol ve dosyalar arası referans sayısını verir. Dosyalar diğer
/// dosyalardan aldıkları kesin kullanımlara (eşitlikte yola) göre sıralanır;
/// her satır `yol — ad(kullanım), …` biçiminde en çok kullanılan beş sembolü,
/// hiçbiri kullanılmıyorsa ilk beş tanımı listeler. Belirsiz kenarlar
/// (`may call`, `may import`) sayılmaz: sık metot adları gerçek çekirdek
/// API'lerin önüne geçmesin. Bütçeye sığmayan dosyalar sonda sayılır.
pub fn project_map(graph: &CodeGraph, prefix: &str, max_tokens: usize) -> Result<String, MapError> {
    let prefix = prefix.trim().trim_start_matches("./").trim_end_matches('/');
    let mut files: Vec<RankedFile> = graph
        .file_nodes_index
        .iter()
        .filter(|(file_id, nodes)| {
            is_under(file_id.trim_start_matches("./"), prefix)
                && nodes
                    .iter()
                    .any(|idx| graph.graph[*idx].node_type == NodeType::File)
        })
        .map(|(file_id, nodes)| rank_file(graph, file_id, nodes))
        .collect();
    if files.is_empty() {
        return Err(if prefix.is_empty() {
            MapError::EmptyIndex
        } else {
            MapError::NoCodeUnder(prefix.to_string())
        });
    }
    files.sort_by(|left, right| {
        right
            .uses
            .cmp(&left.uses)
            .then_with(|| left.path.cmp(&right.path))
    });

    let symbols: usize = files.iter().map(|file| file.symbols.len()).sum();
    let references: usize = files.iter().map(|file| file.uses).sum();
    let mut output = format!(
        "{} files, {symbols} symbols, {references} cross-file references; most used first.\n",
        files.len()
    );
    // Bütçe karakterle ölçülür (bir token ≈ dört karakter), bayt değil.
    let budget = max_tokens.saturating_mul(CHARS_PER_TOKEN);
    let mut used = output.chars().count();
    for (index, file) in files.iter().enumerate() {
        let line = file_line(file);
        let line_chars = line.chars().count() + 1;
        if used + line_chars > budget {
            output.push_str(&format!(
                "… {} more files (raise max_tokens or pass path)\n",
                files.len() - index
            ));
            return Ok(output);
        }
        output.push_str(&line);
        output.push('\n');
        used += line_chars;
    }
    Ok(output)
}

/// Yol önekin kendisi ya da altında mı; boş önek her yolu kapsar.
fn is_under(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn rank_file(graph: &CodeGraph, file_id: &str, nodes: &[NodeIndex]) -> RankedFile {
    let mut symbols: Vec<RankedSymbol> = nodes
        .iter()
        .copied()
        // Rust impl blokları tür adını taşıyan ayrı düğümlerdir; tür bir kez
        // listelenir, metotları `Tür.metot` olarak görünür.
        .filter(|idx| {
            let node = &graph.graph[*idx];
            is_reference_target_type(&node.node_type) && !is_rust_impl_node(node)
        })
        .map(|idx| RankedSymbol {
            name: qualified_name(graph, idx),
            uses: uses_from_other_files(graph, idx, file_id),
            start_line: graph.graph[idx].start_line,
        })
        .collect();
    symbols.sort_by(|left, right| {
        right
            .uses
            .cmp(&left.uses)
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.name.cmp(&right.name))
    });
    RankedFile {
        path: file_id.trim_start_matches("./").to_string(),
        uses: symbols.iter().map(|symbol| symbol.uses).sum(),
        symbols,
    }
}

/// Başka dosyalardaki düğümlerden gelen kesin kullanım kenarları.
fn uses_from_other_files(graph: &CodeGraph, idx: NodeIndex, file_id: &str) -> usize {
    graph
        .graph
        .edges_directed(idx, Direction::Incoming)
        .filter(|edge| {
            is_certain_use(edge.weight())
                && graph_node_file_path(&graph.graph[edge.source()].id) != file_id
        })
        .count()
}

fn is_certain_use(edge: &EdgeType) -> bool {
    match edge {
        EdgeType::Calls
        | EdgeType::CallInferred
        | EdgeType::References
        | EdgeType::Imports
        | EdgeType::Inherits
        | EdgeType::Reads
        | EdgeType::Writes => true,
        EdgeType::CallAmbiguous
        | EdgeType::ImportAmbiguous
        | EdgeType::Contains
        | EdgeType::Defines => false,
    }
}

/// Sınıf ya da yapı üyesi için `Sahip.ad`, diğerleri için ad; `explain` bu
/// biçimi hedef olarak kabul eder.
fn qualified_name(graph: &CodeGraph, idx: NodeIndex) -> String {
    let name = &graph.graph[idx].name;
    graph
        .graph
        .edges_directed(idx, Direction::Incoming)
        .find(|edge| {
            matches!(edge.weight(), EdgeType::Contains)
                && matches!(
                    graph.graph[edge.source()].node_type,
                    NodeType::Class | NodeType::Struct
                )
        })
        .map_or_else(
            || name.clone(),
            |edge| format!("{}.{name}", graph.graph[edge.source()].name),
        )
}

/// `yol — ad(kullanım), …`; hiçbir sembol kullanılmıyorsa ilk tanımlar sayısız.
fn file_line(file: &RankedFile) -> String {
    let used: Vec<String> = file
        .symbols
        .iter()
        .filter(|symbol| symbol.uses > 0)
        .take(SYMBOLS_PER_FILE)
        .map(|symbol| format!("{}({})", symbol.name, symbol.uses))
        .collect();
    let shown: Vec<String> = if used.is_empty() {
        file.symbols
            .iter()
            .take(SYMBOLS_PER_FILE)
            .map(|symbol| symbol.name.clone())
            .collect()
    } else {
        used
    };
    if shown.is_empty() {
        return file.path.clone();
    }
    format!("{} — {}", file.path, shown.join(", "))
}
