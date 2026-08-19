//! Deterministik sentetik corpus + embedding fixture üretimi.
//!
//! Phase 1 kapsamı: `eval/fixtures/repos/repo_a` ve `repo_b` kaynak ağaçlarından
//! golden task'lar ve offline embedding fixture'ı üretir. Her şey deterministiktir:
//! SplitMix64(seed=42) seçimi, token-hash vektörleri ve dosya sıralaması sabittir.
//! Bu sayede CI'da embedder/API bağımlılığı olmadan birebir aynı corpus elde edilir.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;

use crate::graph::{CodeGraph, CodeNode, NodeType};
use crate::rng::SplitMix64;
use crate::vector::hash_embed::embed_text;
use crate::vector::store::semantic_chunks;

pub const FIXTURE_SEED: u64 = 42;
/// Fixture vektör boyutu. v0.3.12'de 64 → 512'ye çıkarıldı: düşük boyutlu
/// trigram-hash çakışmaları `search_code` recall'ını düşürüyordu; 256 boyut
/// hâlâ dar adlarda gürültüye yeniliyordu. 512 boyut sembol adlarını içerik
/// gürültüsünden ayırt edecek kapasite sağlar.
pub const FIXTURE_EMBED_DIM: usize = 512;
pub const SEARCH_TASKS: usize = 25;
pub const CONTEXT_TASKS: usize = 25;
pub const GRAPH_TASKS: usize = 25;
pub const PREDICT_TASKS: usize = 15;
pub const FIXTURE_CHUNK_MAX_CHARS: usize = 1000;
pub const FIXTURE_CHUNK_OVERLAP: usize = 100;

const TASK_TYPES: [&str; 6] = [
    "bug_fix",
    "feature",
    "refactor",
    "investigation",
    "test",
    "unknown",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureSplit {
    Train,
    Holdout,
}

impl FixtureSplit {
    pub fn as_str(&self) -> &'static str {
        match self {
            FixtureSplit::Train => "train",
            FixtureSplit::Holdout => "holdout",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GeneratedFixture {
    pub task_count: usize,
    pub doc_vector_count: usize,
    pub query_vector_count: usize,
}

/// `out_dir/repos/{repo_a,repo_b}` kaynaklarını indeksler, golden task'ları ve
/// embedding fixture'ını `out_dir` altına yazar.
pub async fn generate_all(out_dir: &Path) -> Result<GeneratedFixture> {
    let repos_dir = out_dir.join("repos");
    let mut tasks: Vec<Value> = Vec::new();
    let mut embedding_lines = vec![meta_line()];
    let mut doc_vectors = 0usize;
    let mut query_vectors = 0usize;

    for (repo_name, split) in [
        ("repo_a", FixtureSplit::Train),
        ("repo_b", FixtureSplit::Holdout),
    ] {
        let generated =
            generate_repo(&repos_dir.join(repo_name), repo_name, split, out_dir).await?;
        tasks.extend(generated.tasks);
        embedding_lines.extend(generated.embedding_lines);
        doc_vectors += generated.doc_vector_count;
        query_vectors += generated.query_vector_count;
    }

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("fixture çıktı dizini oluşturulamadı: {}", out_dir.display()))?;
    write_golden_tasks(out_dir, &tasks)?;
    write_embeddings(out_dir, &embedding_lines)?;

    // Generator embedder kapalı indekslediği için repo data dizinlerinde YALNIZCA
    // graph/manifest bulunur; Lance tabloları boştur. Optimize/CI, fixture moduyla
    // yeniden kurulsun diye bu ara dizinler silinir (gitignore kapsamındadır).
    for repo_name in ["repo_a", "repo_b"] {
        let data_dir = repos_dir.join(repo_name).join("data");
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir).with_context(|| {
                format!(
                    "fixture repo data dizini temizlenemedi: {}",
                    data_dir.display()
                )
            })?;
        }
    }

    Ok(GeneratedFixture {
        task_count: tasks.len(),
        doc_vector_count: doc_vectors,
        query_vector_count: query_vectors,
    })
}

/// Commit'li kaynak repo ağaçlarını hedef dizine kopyalar (testler temp dizin
/// kullanır; üretim CLI'ı kaynakların yerinde olduğunu varsayar).
pub fn copy_source_repos(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)
        .with_context(|| format!("hedef dizin oluşturulamadı: {}", to.display()))?;
    for entry in std::fs::read_dir(from)
        .with_context(|| format!("kaynak repo dizini okunamadı: {}", from.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        copy_dir_recursive(&entry.path(), &to.join(&name))?;
    }
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)
        .with_context(|| format!("dizin oluşturulamadı: {}", to.display()))?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

struct RepoFixture {
    tasks: Vec<Value>,
    embedding_lines: Vec<String>,
    doc_vector_count: usize,
    query_vector_count: usize,
}

struct TaskSpec<'a> {
    id: &'a str,
    repo_name: &'a str,
    repo_ref_path: &'a Path,
    split: FixtureSplit,
    task_type: &'a str,
    query: Value,
    node_id: &'a str,
    file_path: &'a str,
    tags: &'a [&'a str],
}

async fn generate_repo(
    repo_path: &Path,
    repo_name: &str,
    split: FixtureSplit,
    out_dir: &Path,
) -> Result<RepoFixture> {
    if !repo_path.join("src").is_dir() {
        bail!(
            "fixture repo kaynağı eksik: {} (src/ dizini yok)",
            repo_path.display()
        );
    }

    // Embedder kapalı: yalnızca graph/manifest gerekir; ağ çağrısı yapılmaz.
    // Değişiklik process-global env'i etkiler; tamamlanınca önceki değer geri yüklenir.
    // İndeks mutlaka canonical (mutlak) yolla kurulur; relative yolla kurulan
    // indekslerde node id'leri bazen "./eval/..." biçiminde üretilir ve bu
    // determinizmi bozar.
    let prev_disable = std::env::var("CCM_DISABLE_EMBEDDER").ok();
    let canonical_repo = std::fs::canonicalize(repo_path).with_context(|| {
        format!(
            "fixture repo canonicalize edilemedi: {}",
            repo_path.display()
        )
    })?;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let index_result = crate::update_index(&canonical_repo.to_string_lossy(), None).await;
    match prev_disable {
        Some(value) => std::env::set_var("CCM_DISABLE_EMBEDDER", value),
        None => std::env::remove_var("CCM_DISABLE_EMBEDDER"),
    }
    index_result
        .with_context(|| format!("fixture repo indeksi kurulamadı: {}", repo_path.display()))?;

    let graph_path =
        crate::resolve_index_artifacts(&canonical_repo.to_string_lossy(), None)?.graph_path;
    let graph = CodeGraph::from_file(&graph_path.to_string_lossy())
        .with_context(|| format!("fixture graph okunamadı: {}", graph_path.display()))?;

    let mut nodes: Vec<&CodeNode> = graph
        .graph
        .node_weights()
        .filter(|node| {
            matches!(
                node.node_type,
                NodeType::Function | NodeType::Method | NodeType::Class | NodeType::Struct
            )
        })
        .collect();
    if nodes.len() < SEARCH_TASKS + CONTEXT_TASKS {
        bail!(
            "{}: yeterli uygun node yok ({} < {})",
            repo_name,
            nodes.len(),
            SEARCH_TASKS + CONTEXT_TASKS
        );
    }
    nodes.sort_by(|a, b| {
        (crate::engine::extract_file_path(&a.id), a.start_line, &a.id).cmp(&(
            crate::engine::extract_file_path(&b.id),
            b.start_line,
            &b.id,
        ))
    });
    SplitMix64::new(FIXTURE_SEED).shuffle(&mut nodes);

    let repo_ref_path = out_dir.join("repos").join(repo_name);
    let mut tasks = Vec::new();
    let mut embedding_lines = Vec::new();
    let mut doc_vector_count = 0usize;

    // Doc vektörleri: embedding kümesiyle birebir (Function/Method/Class/Struct).
    for node in &nodes {
        let text = crate::engine::build_embedding_text(node);
        let chunk_ids = chunk_ids_for(&node.id, &text);
        let chunks = semantic_chunks(&text, FIXTURE_CHUNK_MAX_CHARS, FIXTURE_CHUNK_OVERLAP);
        for (chunk_id, chunk) in chunk_ids.iter().zip(chunks.iter()) {
            embedding_lines.push(
                json!({
                    "kind": "doc",
                    "ns": repo_name,
                    "id": chunk_id,
                    "vector": embed_text(chunk, FIXTURE_EMBED_DIM),
                })
                .to_string(),
            );
            doc_vector_count += 1;
        }
    }

    let count = nodes.len();
    let search_nodes: Vec<&CodeNode> = nodes[0..SEARCH_TASKS].to_vec();
    let context_nodes: Vec<&CodeNode> = (0..CONTEXT_TASKS)
        .map(|i| nodes[(SEARCH_TASKS + i) % count])
        .collect();
    let graph_nodes: Vec<&CodeNode> = (0..GRAPH_TASKS)
        .map(|i| nodes[(SEARCH_TASKS + CONTEXT_TASKS + i) % count])
        .collect();
    let predict_nodes: Vec<&CodeNode> = (0..PREDICT_TASKS)
        .map(|i| nodes[(SEARCH_TASKS + CONTEXT_TASKS + GRAPH_TASKS + i) % count])
        .collect();

    let mut seq = 0usize;
    let mut search_seq = 0usize;
    let mut context_seq = 0usize;
    let mut graph_seq = 0usize;
    let mut predict_seq = 0usize;
    for node in &search_nodes {
        seq += 1;
        search_seq += 1;
        let task_type = TASK_TYPES[(seq - 1) % TASK_TYPES.len()];
        let query_text = format!("find where {} is implemented", node.name);
        let id = format!("syn-{}-search-{:03}", short_repo(repo_name), search_seq);
        tasks.push(task_json(TaskSpec {
            id: &id,
            repo_name,
            repo_ref_path: &repo_ref_path,
            split,
            task_type,
            query: json!({
                "type": "search_code",
                "text": query_text,
            }),
            node_id: &node.id,
            file_path: &crate::engine::extract_file_path(&node.id),
            tags: &["synthetic", "search_code", "direct", split.as_str()],
        }));
        embedding_lines.push(
            json!({
                "kind": "query",
                "ns": repo_name,
                "id": query_text,
                "vector": embed_text(&query_text, FIXTURE_EMBED_DIM),
            })
            .to_string(),
        );
    }

    for node in context_nodes {
        seq += 1;
        context_seq += 1;
        let task_type = TASK_TYPES[(seq - 1) % TASK_TYPES.len()];
        let file_path = crate::engine::extract_file_path(&node.id);
        let id = format!("syn-{}-context-{:03}", short_repo(repo_name), context_seq);
        tasks.push(task_json(TaskSpec {
            id: &id,
            repo_name,
            repo_ref_path: &repo_ref_path,
            split,
            task_type,
            query: json!({
                "type": "get_context",
                "file_path": file_path,
                "line": node.start_line,
            }),
            node_id: &node.id,
            file_path: &file_path,
            tags: &["synthetic", "get_context", split.as_str()],
        }));
    }

    for node in graph_nodes {
        seq += 1;
        graph_seq += 1;
        let task_type = TASK_TYPES[(seq - 1) % TASK_TYPES.len()];
        let id = format!("syn-{}-graph-{:03}", short_repo(repo_name), graph_seq);
        tasks.push(task_json(TaskSpec {
            id: &id,
            repo_name,
            repo_ref_path: &repo_ref_path,
            split,
            task_type,
            query: json!({
                "type": "read_graph",
                "node_id": node.id,
            }),
            node_id: &node.id,
            file_path: &crate::engine::extract_file_path(&node.id),
            tags: &["synthetic", "read_graph", split.as_str()],
        }));
    }

    for node in predict_nodes {
        seq += 1;
        predict_seq += 1;
        let task_type = TASK_TYPES[(seq - 1) % TASK_TYPES.len()];
        let file_path = crate::engine::extract_file_path(&node.id);
        let id = format!("syn-{}-predict-{:03}", short_repo(repo_name), predict_seq);
        tasks.push(task_json(TaskSpec {
            id: &id,
            repo_name,
            repo_ref_path: &repo_ref_path,
            split,
            task_type,
            query: json!({
                "type": "predict_context",
                "file_path": file_path,
                "line": node.start_line,
            }),
            node_id: &node.id,
            file_path: &file_path,
            tags: &["synthetic", "predict_context", split.as_str()],
        }));
    }

    Ok(RepoFixture {
        tasks,
        embedding_lines,
        doc_vector_count,
        query_vector_count: SEARCH_TASKS,
    })
}

fn task_json(spec: TaskSpec<'_>) -> Value {
    json!({
        "id": spec.id,
        "repo": {
            "name": spec.repo_name,
            "path": spec.repo_ref_path.to_string_lossy(),
            "languages": ["rust"],
        },
        "query": spec.query,
        "expected": {
            "node_ids": [spec.node_id],
            "file_paths": [spec.file_path],
            "min_recall": 1,
            "max_rank": 5,
        },
        "tags": spec.tags,
        "priority": 1,
        "notes": "Deterministic synthetic fixture task",
        "split": spec.split.as_str(),
        "task_type": spec.task_type,
    })
}

/// Store.add_documents ile birebir aynı chunk-id şemasını üretir.
fn chunk_ids_for(node_id: &str, text: &str) -> Vec<String> {
    if text.len() <= FIXTURE_CHUNK_MAX_CHARS {
        vec![node_id.to_string()]
    } else {
        let chunks = semantic_chunks(text, FIXTURE_CHUNK_MAX_CHARS, FIXTURE_CHUNK_OVERLAP);
        (0..chunks.len())
            .map(|index| format!("{}#chunk{}", node_id, index))
            .collect()
    }
}

fn short_repo(repo_name: &str) -> &str {
    repo_name.trim_start_matches("repo_")
}

fn meta_line() -> String {
    json!({
        "kind": "meta",
        "method": "token_hash_v1",
        "dim": FIXTURE_EMBED_DIM,
        "seed": FIXTURE_SEED,
        "chunk_max_chars": FIXTURE_CHUNK_MAX_CHARS,
        "chunk_overlap": FIXTURE_CHUNK_OVERLAP,
        "generator": "ccm learn fixtures",
    })
    .to_string()
}

fn write_golden_tasks(out_dir: &Path, tasks: &[Value]) -> Result<()> {
    let path = out_dir.join("golden_tasks.synthetic.json");
    let document = json!({
        "schema_version": 1,
        "tasks": tasks,
    });
    let content = serde_json::to_string_pretty(&document)?;
    std::fs::write(&path, content)
        .with_context(|| format!("golden task yazılamadı: {}", path.display()))
}

fn write_embeddings(out_dir: &Path, lines: &[String]) -> Result<()> {
    let path = out_dir.join("embeddings.ndjson");
    let mut content = lines.join("\n");
    content.push('\n');
    std::fs::write(&path, content)
        .with_context(|| format!("embedding fixture yazılamadı: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn short_chunk_id_keeps_plain_node_id() {
        let ids = chunk_ids_for("./src/utils.rs:function_item:1:0", "short text");
        assert_eq!(ids, vec!["./src/utils.rs:function_item:1:0"]);
    }

    #[test]
    fn long_text_gets_chunk_suffixes() {
        let long = "x".repeat(FIXTURE_CHUNK_MAX_CHARS + 10);
        let ids = chunk_ids_for("./src/a.rs:function_item:1:0", &long);
        assert!(ids.len() > 1);
        assert!(ids[0].ends_with("#chunk0"));
    }

    #[test]
    fn repo_short_name_is_stable() {
        assert_eq!(short_repo("repo_a"), "a");
        assert_eq!(short_repo("repo_b"), "b");
    }

    #[test]
    fn fixture_split_as_str_is_stable() {
        assert_eq!(FixtureSplit::Train.as_str(), "train");
        assert_eq!(FixtureSplit::Holdout.as_str(), "holdout");
    }

    #[test]
    fn copy_source_repos_copies_nested_dirs_and_skips_root_files() -> Result<()> {
        let source = tempdir()?;
        std::fs::create_dir_all(source.path().join("repo_a/src/nested"))?;
        std::fs::write(source.path().join("repo_a/src/main.rs"), "fn main() {}\n")?;
        std::fs::write(
            source.path().join("repo_a/src/nested/helper.rs"),
            "fn helper() {}\n",
        )?;
        std::fs::write(source.path().join("README.md"), "root file\n")?;

        let target = tempdir()?;
        copy_source_repos(source.path(), target.path())?;
        assert!(target.path().join("repo_a/src/main.rs").is_file());
        assert!(target.path().join("repo_a/src/nested/helper.rs").is_file());
        assert!(!target.path().join("README.md").exists());
        Ok(())
    }

    #[test]
    fn copy_dir_recursive_preserves_file_contents() -> Result<()> {
        let source = tempdir()?;
        std::fs::create_dir_all(source.path().join("nested"))?;
        std::fs::write(source.path().join("nested/data.txt"), "payload\n")?;
        let target = tempdir()?;
        copy_dir_recursive(source.path(), target.path())?;
        let copied = std::fs::read_to_string(target.path().join("nested/data.txt"))?;
        assert_eq!(copied, "payload\n");
        Ok(())
    }

    #[test]
    fn task_json_serializes_full_spec() {
        let repo_ref = Path::new("/tmp/repos/repo_a");
        let task = task_json(TaskSpec {
            id: "syn-a-search-001",
            repo_name: "repo_a",
            repo_ref_path: repo_ref,
            split: FixtureSplit::Train,
            task_type: "bug_fix",
            query: json!({
                "type": "search_code",
                "text": "find main",
            }),
            node_id: "./src/main.rs:function_item:1:0",
            file_path: "./src/main.rs",
            tags: &["synthetic", "search_code", "direct", "train"],
        });
        assert_eq!(task["id"], "syn-a-search-001");
        assert_eq!(task["repo"]["name"], "repo_a");
        assert_eq!(task["repo"]["path"], "/tmp/repos/repo_a");
        assert_eq!(task["query"]["type"], "search_code");
        assert_eq!(task["expected"]["min_recall"], 1);
        assert_eq!(task["expected"]["max_rank"], 5);
        assert_eq!(task["split"], "train");
        assert_eq!(task["task_type"], "bug_fix");
        assert_eq!(task["priority"], 1);
    }

    #[test]
    fn meta_line_matches_fixture_contract() {
        let meta: Value = serde_json::from_str(&meta_line()).expect("meta satırı JSON olmalı");
        assert_eq!(meta["kind"], "meta");
        assert_eq!(meta["method"], "token_hash_v1");
        assert_eq!(meta["dim"], FIXTURE_EMBED_DIM);
        assert_eq!(meta["seed"], FIXTURE_SEED);
        assert_eq!(meta["chunk_max_chars"], FIXTURE_CHUNK_MAX_CHARS);
        assert_eq!(meta["chunk_overlap"], FIXTURE_CHUNK_OVERLAP);
    }

    #[test]
    fn write_golden_tasks_persists_schema_and_tasks() -> Result<()> {
        let out_dir = tempdir()?;
        let tasks = vec![
            json!({"id": "syn-a-search-001"}),
            json!({"id": "syn-a-context-001"}),
        ];
        write_golden_tasks(out_dir.path(), &tasks)?;
        let document: Value = serde_json::from_str(&std::fs::read_to_string(
            out_dir.path().join("golden_tasks.synthetic.json"),
        )?)?;
        assert_eq!(document["schema_version"], 1);
        assert_eq!(document["tasks"].as_array().expect("tasks dizisi").len(), 2);
        Ok(())
    }

    #[test]
    fn write_embeddings_persists_lines_with_trailing_newline() -> Result<()> {
        let out_dir = tempdir()?;
        let lines = vec![
            "{\"kind\":\"meta\"}".to_string(),
            "{\"kind\":\"doc\"}".to_string(),
        ];
        write_embeddings(out_dir.path(), &lines)?;
        let content = std::fs::read_to_string(out_dir.path().join("embeddings.ndjson"))?;
        assert!(content.ends_with('\n'));
        assert!(content.contains("{\"kind\":\"meta\"}"));
        assert!(content.contains("{\"kind\":\"doc\"}"));
        Ok(())
    }
}
