//! Rust crate'leri ve modül yolları: indeksteki `Cargo.toml` dosyalarından paket
//! adları, dosya yollarından modül yolları.
//!
//! Paket dizini `D` için `D/src/lib.rs` kütüphane (başka crate'lerden paket adıyla,
//! `-` yerine `_` ile görünür), `D/src/main.rs` ikili crate köküdür; `D/src/bin`,
//! `D/tests`, `D/benches` ve `D/examples` altındaki doğrudan `.rs` dosyaları kendi
//! kökleridir. Paketsiz dosyalar dizinlerindeki `main.rs`/`lib.rs`'ye, o da yoksa
//! kendilerine bağlanır. `#[path]` öznitelikleri izlenmez.

use std::collections::BTreeSet;

use super::{CodeGraph, NodeType};

/// Crate'in türü: kütüphane başka crate'lerden paket adıyla görünür.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CrateKind {
    Lib,
    Bin,
    /// `tests/`, `benches/`, `examples/`, `src/bin/` kökleri ve gevşek dosyalar.
    Other,
}

/// Bir crate: modül yolları `root_dir`'e görelidir.
#[derive(Debug, Clone)]
pub(crate) struct RustCrate {
    pub(crate) name: String,
    pub(crate) kind: CrateKind,
    pub(crate) root_file: String,
    pub(crate) root_dir: String,
}

/// Dosyanın crate'i ve crate içindeki modül yolu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RustModule {
    pub(crate) krate: usize,
    pub(crate) path: Vec<String>,
}

/// Projedeki Rust crate'leri ve indekslenmiş `.rs` dosyaları.
pub(crate) struct RustCrates {
    pub(crate) crates: Vec<RustCrate>,
    files: BTreeSet<String>,
}

/// `Cargo.toml`'daki adlar: paket adı ve varsa `[lib] name`, `_` ile.
struct ManifestNames {
    package: String,
    lib: Option<String>,
}

/// Kökler dizinlerin sırası: aynı dizinde kütüphane ikiliden önce gelir.
const KIND_ORDER: [CrateKind; 3] = [CrateKind::Lib, CrateKind::Bin, CrateKind::Other];

impl RustCrates {
    pub(crate) fn new(graph: &CodeGraph) -> Self {
        let files: BTreeSet<String> = graph
            .file_nodes_index
            .keys()
            .filter(|id| {
                id.ends_with(".rs")
                    && graph
                        .find_file_node(id)
                        .is_some_and(|idx| graph.graph[idx].node_type == NodeType::File)
            })
            .cloned()
            .collect();
        let mut crates = Vec::new();
        for (file_id, nodes) in &graph.file_nodes_index {
            let Some(package_dir) = file_id.strip_suffix("/Cargo.toml") else {
                continue;
            };
            let Some(names) = nodes
                .iter()
                .map(|idx| &graph.graph[*idx])
                .find(|node| node.node_type == NodeType::DataFile)
                .and_then(|manifest| manifest_names(&manifest.content))
            else {
                continue;
            };
            crates.extend(package_crates(package_dir, &names, &files));
        }
        crates.extend(loose_crates(&files, &crates));
        Self { crates, files }
    }

    /// Dosyanın crate'i ve modül yolu; `.rs` dosyası değilse `None`.
    pub(crate) fn module_of_file(&self, file_id: &str) -> Option<RustModule> {
        if let Some(krate) = self
            .crates
            .iter()
            .position(|krate| krate.root_file == file_id)
        {
            return Some(RustModule {
                krate,
                path: Vec::new(),
            });
        }
        let krate = self
            .crates
            .iter()
            .enumerate()
            .filter(|(_, krate)| is_under(file_id, &krate.root_dir))
            .min_by_key(|(_, krate)| {
                (
                    std::cmp::Reverse(krate.root_dir.len()),
                    KIND_ORDER
                        .iter()
                        .position(|kind| *kind == krate.kind)
                        .unwrap_or(KIND_ORDER.len()),
                )
            })
            .map(|(index, _)| index)?;
        let relative = file_id
            .strip_prefix(self.crates[krate].root_dir.as_str())?
            .trim_start_matches('/')
            .strip_suffix(".rs")?;
        let mut path: Vec<String> = relative.split('/').map(str::to_string).collect();
        if path.last().is_some_and(|last| last == "mod") {
            path.pop();
        }
        Some(RustModule { krate, path })
    }

    /// Paket adıyla görünen kütüphane crate'i.
    pub(crate) fn lib_named(&self, name: &str) -> Option<usize> {
        self.crates
            .iter()
            .position(|krate| krate.kind == CrateKind::Lib && krate.name == name)
    }

    /// Crate içindeki modül yolunun dosyası: `a/b.rs` ya da `a/b/mod.rs`.
    pub(crate) fn file_of(&self, krate: usize, path: &[String]) -> Option<String> {
        let krate = self.crates.get(krate)?;
        if path.is_empty() {
            return Some(krate.root_file.clone());
        }
        let base = format!("{}/{}", krate.root_dir, path.join("/"));
        [format!("{base}.rs"), format!("{base}/mod.rs")]
            .into_iter()
            .find(|candidate| self.files.contains(candidate))
    }
}

/// Dosya dizinin içinde mi (`./a/src` → `./a/src/x.rs`).
fn is_under(file_id: &str, dir: &str) -> bool {
    file_id
        .strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// `dir` ile göreli yolu birleştirir; kök paket dizini `.`'dır.
fn join(dir: &str, relative: &str) -> String {
    format!("{dir}/{relative}")
}

/// Paketin crate kökleri.
fn package_crates(dir: &str, names: &ManifestNames, files: &BTreeSet<String>) -> Vec<RustCrate> {
    let mut crates = Vec::new();
    let src = join(dir, "src");
    let lib_root = join(&src, "lib.rs");
    if files.contains(&lib_root) {
        crates.push(RustCrate {
            name: names.lib.clone().unwrap_or_else(|| names.package.clone()),
            kind: CrateKind::Lib,
            root_file: lib_root,
            root_dir: src.clone(),
        });
    }
    let bin_root = join(&src, "main.rs");
    if files.contains(&bin_root) {
        crates.push(RustCrate {
            name: names.package.clone(),
            kind: CrateKind::Bin,
            root_file: bin_root,
            root_dir: src.clone(),
        });
    }
    for target_dir in [
        join(&src, "bin"),
        join(dir, "tests"),
        join(dir, "benches"),
        join(dir, "examples"),
    ] {
        for file in files.iter().filter(|file| {
            file.strip_prefix(target_dir.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|rest| !rest.contains('/'))
        }) {
            crates.push(RustCrate {
                name: file_stem(file),
                kind: CrateKind::Other,
                root_file: file.clone(),
                root_dir: target_dir.clone(),
            });
        }
    }
    crates
}

/// Hiçbir crate dizininde olmayan dosyaların kökleri: dizindeki `main.rs` ya da
/// `lib.rs` kardeşlerini kapsar, kalan dosyalar kendi köküdür.
fn loose_crates(files: &BTreeSet<String>, crates: &[RustCrate]) -> Vec<RustCrate> {
    let covered = |file: &String| {
        crates
            .iter()
            .any(|krate| krate.root_file == *file || is_under(file, &krate.root_dir))
    };
    let loose: Vec<&String> = files.iter().filter(|file| !covered(file)).collect();
    let mut roots: Vec<RustCrate> = loose
        .iter()
        .filter(|file| file.ends_with("/main.rs") || file.ends_with("/lib.rs"))
        .map(|file| RustCrate {
            name: file_stem(file),
            kind: CrateKind::Other,
            root_file: (*file).clone(),
            root_dir: parent_dir(file),
        })
        .collect();
    let singles: Vec<RustCrate> = loose
        .iter()
        .filter(|file| {
            !roots
                .iter()
                .any(|root| root.root_file == ***file || is_under(file, &root.root_dir))
        })
        .map(|file| RustCrate {
            name: file_stem(file),
            kind: CrateKind::Other,
            root_file: (*file).clone(),
            root_dir: parent_dir(file),
        })
        .collect();
    roots.extend(singles);
    roots
}

fn parent_dir(file: &str) -> String {
    file.rsplit_once('/')
        .map_or_else(|| ".".to_string(), |(dir, _)| dir.to_string())
}

fn file_stem(file: &str) -> String {
    let name = file.rsplit_once('/').map_or(file, |(_, name)| name);
    name.strip_suffix(".rs").unwrap_or(name).to_string()
}

/// `Cargo.toml` içeriğinin tanımladığı crate adları (paket ve `[lib] name`);
/// artımlı yenileme bu adları değişiklikten önce ve sonra toplar.
pub(crate) fn manifest_crate_names(content: &str) -> Vec<String> {
    manifest_names(content)
        .map(|names| {
            let mut crate_names = vec![names.package];
            crate_names.extend(names.lib);
            crate_names
        })
        .unwrap_or_default()
}

/// `[package]` ve `[lib]` tablolarındaki `name`; satır tabanlı, TOML bağımlılığı
/// olmadan. Paket adı yoksa crate tanımlanmaz.
fn manifest_names(content: &str) -> Option<ManifestNames> {
    let mut table = String::new();
    let mut package = None;
    let mut lib = None;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            table = line.to_string();
            continue;
        }
        let Some(value) = name_value(line) else {
            continue;
        };
        match table.as_str() {
            "[package]" => package = Some(value),
            "[lib]" => lib = Some(value),
            _ => {}
        }
    }
    Some(ManifestNames {
        package: package?.replace('-', "_"),
        lib: lib.map(|name| name.replace('-', "_")),
    })
}

/// `name = "x"` ya da `name = 'x'` satırının değeri; sondaki yorum yok sayılır.
fn name_value(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("name")?
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let quote = rest.chars().next().filter(|ch| *ch == '"' || *ch == '\'')?;
    let value = rest[1..].split(quote).next()?;
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CodeNode, NodeType, ReferenceFacts};

    fn add(graph: &mut CodeGraph, id: &str, node_type: NodeType, content: &str) {
        graph.add_node(CodeNode {
            id: id.to_string(),
            node_type,
            name: id.to_string(),
            content: content.into(),
            start_line: 1,
            end_line: 1,
            facts: ReferenceFacts::Lexical,
        });
    }

    /// İki paketli çalışma alanı, bir test crate'i ve gevşek bir dosya.
    fn workspace() -> CodeGraph {
        let mut graph = CodeGraph::new();
        add(
            &mut graph,
            "./Cargo.toml",
            NodeType::DataFile,
            "[workspace]\nmembers = [\"app_core\", \"app_cli\"]\n",
        );
        add(
            &mut graph,
            "./app_core/Cargo.toml",
            NodeType::DataFile,
            "[package]\nname = \"app-core\" # yorum\nversion = \"0.1.0\"\n\n[dependencies]\nname = \"not-the-package\"\n",
        );
        add(
            &mut graph,
            "./app_cli/Cargo.toml",
            NodeType::DataFile,
            "[package]\nname = 'app-cli'\n",
        );
        for file in [
            "./app_core/src/lib.rs",
            "./app_core/src/util.rs",
            "./app_core/src/graph/mod.rs",
            "./app_core/src/graph/map.rs",
            "./app_core/tests/common/mod.rs",
            "./app_core/tests/flow.rs",
            "./app_cli/src/main.rs",
            "./app_cli/src/args.rs",
            "./scratch.rs",
        ] {
            add(&mut graph, file, NodeType::File, "");
        }
        graph
    }

    fn module(crates: &RustCrates, file: &str) -> (String, Vec<String>) {
        let module = crates
            .module_of_file(file)
            .unwrap_or_else(|| panic!("{file} has no module"));
        (crates.crates[module.krate].name.clone(), module.path)
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn rust_files_map_to_crates_and_module_paths() {
        let crates = RustCrates::new(&workspace());
        assert_eq!(
            module(&crates, "./app_core/src/lib.rs"),
            ("app_core".into(), path(&[]))
        );
        assert_eq!(
            module(&crates, "./app_core/src/util.rs"),
            ("app_core".into(), path(&["util"]))
        );
        assert_eq!(
            module(&crates, "./app_core/src/graph/mod.rs"),
            ("app_core".into(), path(&["graph"]))
        );
        assert_eq!(
            module(&crates, "./app_core/src/graph/map.rs"),
            ("app_core".into(), path(&["graph", "map"]))
        );
        assert_eq!(
            module(&crates, "./app_cli/src/main.rs"),
            ("app_cli".into(), path(&[]))
        );
        assert_eq!(
            module(&crates, "./app_cli/src/args.rs"),
            ("app_cli".into(), path(&["args"]))
        );
        // Test crate'leri kendi köküdür; yardımcı modülleri `tests/` dizinine görelidir.
        assert_eq!(module(&crates, "./app_core/tests/flow.rs").1, path(&[]));
        assert_eq!(
            module(&crates, "./app_core/tests/common/mod.rs").1,
            path(&["common"])
        );
        // Paketsiz dosya kendi köküdür.
        assert_eq!(module(&crates, "./scratch.rs").1, path(&[]));

        let lib = crates.lib_named("app_core").expect("app_core lib");
        assert_eq!(crates.crates[lib].root_file, "./app_core/src/lib.rs");
        assert_eq!(
            crates.lib_named("app_cli"),
            None,
            "a binary crate is not importable"
        );
        assert_eq!(
            crates.file_of(lib, &path(&["graph", "map"])),
            Some("./app_core/src/graph/map.rs".to_string())
        );
        assert_eq!(
            crates.file_of(lib, &path(&["graph"])),
            Some("./app_core/src/graph/mod.rs".to_string())
        );
        assert_eq!(
            crates.file_of(lib, &path(&[])),
            Some("./app_core/src/lib.rs".to_string())
        );
        assert_eq!(crates.file_of(lib, &path(&["missing"])), None);
    }
}
