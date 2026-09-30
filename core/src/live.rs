//! Süreç içi canlı indeks bağlayıcısı. MCP otomatik yenilemesi etkin
//! generation'ın engine'ini (graf + vektör tablosu) ve manifestini bellekte
//! tutar; dosya değişikliklerini worker süreci ve generation kopyası olmadan
//! yerinde uygular, ardından grafı ve manifesti etkin generation dizinine atomik
//! olarak yazar.
//!
//! Eşzamanlılık modeli:
//! - Ayrıştırma, mevcut vektörleri okuma ve embedding kilitsizdir. Yalnızca etkin
//!   işaretçinin denetimi, vektör satırlarının silinip eklenmesi ve grafın
//!   değiştirilmesi generation aktivasyonunun kilidi altında yapılır. Başka bir
//!   süreç (CLI, elle indeksleme) yeni generation kurduysa hazırlanan iş atılır,
//!   bağlayıcı `Superseded` olur ve çağıran yeni generation'ı yükler.
//! - Vektör tablosu yerinde değiştirilir; `update_index` tabloyu aynı kilit
//!   altında kopyalar.
//! - Graf yalnızca tüm hata verebilen adımlardan sonra ve tek adımda değişir;
//!   yarıda kalan bir uygulama grafı bozmaz, dosyaları sonraki turda yeniden
//!   uygulanır.
//! - Graf manifestten önce yazılır ve okuyucular manifesti graftan önce okur;
//!   diskteki manifest graftan yeni olamaz, yeni graf değişiklikleri yeniden
//!   uygulatır (idempotent). Kalıcılaştırma atlanırsa sonraki tam karşılaştırma
//!   diski yakalar.

use crate::engine::{EmbeddedNodes, RetrievalEngine};
use crate::graph::CodeGraph;
use crate::vector::store::LanceDbStore;
use crate::{
    artifact_temp_path, build_manifest, diff_manifest, diff_manifest_scope, file_id_to_path,
    fixture_namespace_for_db, index_artifact_paths, read_current_pointer_value, read_manifest,
    relative_file_id, replace_file_atomically, resolve_index_artifacts, resolve_requested_db_path,
    restore_retry_files, scan_manifest_scope, semantic_node_counts, sync_directory, unix_now_secs,
    write_manifest_file, ActivationLock, FileFingerprint, IndexArtifactPaths, IndexManifest,
    IndexStats,
};
use anyhow::Result;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Değişikliklerinde tüm dosya kümesi değişebilecek ignore dosyaları; bunlardan
/// biri değişince hedefli tarama yerine tam karşılaştırma yapılır.
const IGNORE_RULE_FILES: [&str; 3] = [".gitignore", ".ignore", ".ccmignore"];

/// Hedefli taramanın üst sınırı. Kapsamlı tarayıcı ziyaret ettiği her girdiyi
/// tüm yollarla karşılaştırır; toplu değişikliklerde (ör. `git checkout`) tek bir
/// tam yürüyüş daha ucuzdur.
const MAX_TARGETED_PATHS: usize = 128;

/// Bir projenin etkin generation'ına bağlı canlı indeks.
pub struct LiveIndex {
    project_root: PathBuf,
    /// Artefakt dışlamaları için istenen (generation öncesi) DB yolu.
    requested_db_path: PathBuf,
    artifacts: IndexArtifactPaths,
    engine: Arc<RetrievalEngine>,
    state: Mutex<LiveState>,
    /// Kalıcılaştırmalar sırayla yapılır; eski bir anlık görüntü yenisinin
    /// üzerine yazılmaz.
    persist_gate: Mutex<()>,
    /// Tazelik satırının kilitsiz okuduğu son uygulama zamanı (unix saniye).
    applied_at: std::sync::Mutex<Option<u64>>,
}

struct LiveState {
    manifest: IndexManifest,
    /// Her uygulanan değişiklikte artar.
    version: u64,
    /// Diske yazılan son sürüm.
    persisted_version: u64,
    /// Sonraki uygulama tam karşılaştırma yapmalı mı: yüklemeyle ilk yenileme
    /// arasında kaçan olaylar ve yarıda kalan bir uygulama böyle yakalanır.
    needs_rescan: bool,
    /// Etkin işaretçi başka generation'ı gösteriyor; bu bağlayıcı kullanılmaz.
    superseded: bool,
    /// Graf ve vektör tablosu diskteki içerikle eşleşmeyebilecek dosyalar: vektör
    /// yazımı yarıda kalan bir uygulamanın dosyaları ve okunamadığı için yeniden
    /// denenecek dosyalar. Sonraki uygulama bunları diskten yeniden kurar.
    dirty_files: BTreeSet<String>,
    /// Embedding servisine ulaşılamadığı için vektörü eksik kalan değişiklik
    /// varsa nedeni. Bağlayıcı yeniden yüklenene kadar gösterilir; eksik
    /// vektörler elle indekslemede onarılır.
    semantic_unavailable: Option<String>,
}

/// Bir yenileme turunun sonucu.
#[derive(Debug)]
pub enum LiveRefresh {
    /// Diskteki değişiklikler (varsa) canlı engine'e uygulandı.
    Applied(Box<IndexStats>),
    /// Etkin işaretçi başka generation'ı gösteriyor; hiçbir şey uygulanmadı.
    Superseded,
}

/// Kalıcılaştırmanın sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePersist {
    /// Graf ve manifest etkin generation'a yazıldı.
    Persisted,
    /// Son yazımdan beri uygulanan değişiklik yok.
    UpToDate,
    /// Etkin işaretçi başka generation'ı gösteriyor; hiçbir şey yazılmadı.
    Superseded,
}

/// Diskteki değişiklikler ve uygulandıktan sonra geçerli olacak manifest.
struct ManifestDelta {
    changed: Vec<String>,
    deleted: Vec<String>,
    next_manifest: IndexManifest,
}

/// Kalıcılaştırılacak tutarlı anlık görüntü.
struct LiveSnapshot {
    graph: CodeGraph,
    manifest: IndexManifest,
}

/// Anlık görüntüyü yazma girişiminin sonucu.
enum SnapshotWrite {
    Written,
    Superseded,
}

impl LiveIndex {
    /// Projenin etkin generation'ını yükler. Manifest graftan önce okunur;
    /// kalıcılaştırma grafı önce yazdığı için yüklenen graf manifestten eski
    /// olamaz.
    pub async fn load(
        project_path: &str,
        db_path: Option<&str>,
        policy_path: Option<&Path>,
    ) -> Result<Self> {
        let project_root = std::fs::canonicalize(project_path).map_err(|error| {
            anyhow::anyhow!(
                "Project root '{}' could not be resolved: {}",
                project_path,
                error
            )
        })?;
        let requested_db_path = resolve_requested_db_path(&project_root, db_path)?;
        let artifacts = resolve_index_artifacts(project_path, db_path)?;
        let manifest = read_manifest(&artifacts.manifest_path)?;
        let graph_path = artifacts.graph_path.to_string_lossy().to_string();
        let graph = CodeGraph::load_from_file(&graph_path).map_err(|error| {
            anyhow::anyhow!(
                "Project graph '{}' could not be loaded: {}. Run index_project to rebuild it.",
                graph_path,
                error
            )
        })?;
        tracing::info!(
            project = %project_root.display(),
            generation = ?artifacts.generation_id,
            nodes = graph.graph.node_count(),
            "Loaded live index"
        );
        let store = LanceDbStore::new_with_fixture_namespace(
            &artifacts.db_path.to_string_lossy(),
            "code_vectors",
            Some(&fixture_namespace_for_db(&requested_db_path)),
        )
        .await?;
        let engine = Arc::new(RetrievalEngine::new_with_active_policy(
            Arc::new(RwLock::new(graph)),
            store,
            policy_path,
        ));
        let applied_at = manifest.indexed_at;
        Ok(Self {
            project_root,
            requested_db_path,
            artifacts,
            engine,
            state: Mutex::new(LiveState {
                manifest,
                version: 0,
                persisted_version: 0,
                needs_rescan: true,
                superseded: false,
                dirty_files: BTreeSet::new(),
                semantic_unavailable: None,
            }),
            persist_gate: Mutex::new(()),
            applied_at: std::sync::Mutex::new(applied_at),
        })
    }

    pub fn engine(&self) -> Arc<RetrievalEngine> {
        self.engine.clone()
    }

    /// Bağlı olunan generation; düz (eski) yerleşimde `None`.
    pub fn generation_id(&self) -> Option<&str> {
        self.artifacts.generation_id.as_deref()
    }

    /// İndeksin diski en son yansıttığı an: son yenilemenin başladığı zaman.
    pub fn indexed_at(&self) -> Option<u64> {
        *self
            .applied_at
            .lock()
            .expect("live index timestamp lock poisoned")
    }

    /// Watcher'ın bildirdiği yolları uygular: yalnızca bu yollar (dizinse
    /// altları) tam taramanın kurallarıyla taranır ve manifestle karşılaştırılır.
    /// Yüklemeden sonraki ilk tur, yarıda kalmış bir turun ardından gelen tur,
    /// ignore dosyası değişikliği ve `MAX_TARGETED_PATHS`'i aşan toplu değişiklik
    /// tam karşılaştırma yapar.
    pub async fn apply_paths(&self, paths: &[PathBuf]) -> Result<LiveRefresh> {
        let started_at = unix_now_secs();
        let mut state = self.state.lock().await;
        if state.superseded {
            return Ok(LiveRefresh::Superseded);
        }
        let delta = if state.needs_rescan
            || touches_ignore_rules(paths)
            || paths.len() > MAX_TARGETED_PATHS
        {
            tracing::info!(
                project = %self.project_root.display(),
                paths = paths.len(),
                "Live refresh falls back to a full comparison"
            );
            self.scan_all(&state.manifest).await?
        } else {
            self.scan_paths(&state.manifest, paths).await?
        };
        self.apply_delta(&mut state, delta, started_at).await
    }

    /// Tüm projeyi manifestle karşılaştırır (stat önbelleğiyle) ve farkı uygular.
    pub async fn apply_rescan(&self) -> Result<LiveRefresh> {
        let started_at = unix_now_secs();
        let mut state = self.state.lock().await;
        if state.superseded {
            return Ok(LiveRefresh::Superseded);
        }
        let delta = self.scan_all(&state.manifest).await?;
        self.apply_delta(&mut state, delta, started_at).await
    }

    /// Son uygulanan durumu etkin generation dizinine yazar: graf ve manifest
    /// geçici dosyalara yazılır, ardından aktivasyon kilidi altında işaretçi
    /// denetlenip ikisi sırayla (önce graf) atomik olarak değiştirilir.
    pub async fn persist(&self) -> Result<LivePersist> {
        let _gate = self.persist_gate.lock().await;
        let (snapshot, version) = {
            let state = self.state.lock().await;
            if state.superseded {
                return Ok(LivePersist::Superseded);
            }
            if state.version == state.persisted_version {
                return Ok(LivePersist::UpToDate);
            }
            let graph = self.engine.graph.read().await;
            let snapshot = LiveSnapshot {
                graph: CodeGraph {
                    graph: graph.graph.clone(),
                    ..CodeGraph::default()
                },
                manifest: state.manifest.clone(),
            };
            (snapshot, state.version)
        };
        let artifact_parent = self.artifact_parent()?.to_path_buf();
        let expected_generation = self.artifacts.generation_id.clone();
        let graph_path = self.artifacts.graph_path.clone();
        let manifest_path = self.artifacts.manifest_path.clone();
        let started = std::time::Instant::now();
        let written = tokio::task::spawn_blocking(move || {
            write_snapshot(
                &artifact_parent,
                &expected_generation,
                &graph_path,
                &manifest_path,
                snapshot,
            )
        })
        .await
        .map_err(|error| anyhow::anyhow!("Live index persistence task failed: {}", error))??;
        let mut state = self.state.lock().await;
        match written {
            SnapshotWrite::Written => {
                state.persisted_version = state.persisted_version.max(version);
                tracing::info!(
                    project = %self.project_root.display(),
                    generation = ?self.artifacts.generation_id,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Persisted live index into the active generation"
                );
                Ok(LivePersist::Persisted)
            }
            SnapshotWrite::Superseded => {
                state.superseded = true;
                Ok(LivePersist::Superseded)
            }
        }
    }

    fn artifact_parent(&self) -> Result<&Path> {
        self.requested_db_path.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid DB path '{}': cannot determine parent directory",
                self.requested_db_path.display()
            )
        })
    }

    fn excluded_paths(&self) -> Result<Vec<PathBuf>> {
        Ok(index_artifact_paths(
            self.artifact_parent()?,
            &self.requested_db_path,
        ))
    }

    /// Tam tarama: `update_index` ile aynı manifest yürüyüşü.
    async fn scan_all(&self, previous: &IndexManifest) -> Result<ManifestDelta> {
        let project_root = self.project_root.clone();
        let excluded = self.excluded_paths()?;
        let base = previous.clone();
        let walked =
            tokio::task::spawn_blocking(move || build_manifest(&project_root, &excluded, &base))
                .await
                .map_err(|error| anyhow::anyhow!("Project snapshot task failed: {}", error))??;
        let (changed, deleted) = diff_manifest(previous, &walked);
        Ok(ManifestDelta {
            changed,
            deleted,
            next_manifest: walked,
        })
    }

    /// Hedefli tarama: yalnızca verilen yollar ve altları. Manifestin geri kalanı
    /// ve zaman damgası olduğu gibi kalır.
    async fn scan_paths(
        &self,
        previous: &IndexManifest,
        paths: &[PathBuf],
    ) -> Result<ManifestDelta> {
        let (scope, outside): (Vec<PathBuf>, Vec<PathBuf>) = paths
            .iter()
            .cloned()
            .partition(|path| path.starts_with(&self.project_root));
        if !outside.is_empty() {
            tracing::warn!(
                project = %self.project_root.display(),
                ignored = ?outside,
                "Live refresh ignored paths outside the project root"
            );
        }
        let scope_ids: Vec<String> = scope
            .iter()
            .filter_map(|path| relative_file_id(&self.project_root, path))
            .collect();
        if scope_ids.is_empty() {
            return Ok(ManifestDelta {
                changed: Vec::new(),
                deleted: Vec::new(),
                next_manifest: previous.clone(),
            });
        }
        let project_root = self.project_root.clone();
        let excluded = self.excluded_paths()?;
        let base = previous.clone();
        let scanned = tokio::task::spawn_blocking(move || {
            scan_manifest_scope(&project_root, &excluded, &base, &scope)
        })
        .await
        .map_err(|error| anyhow::anyhow!("Project snapshot task failed: {}", error))??;
        let (changed, deleted) = diff_manifest_scope(previous, &scope_ids, &scanned);
        let next_manifest = manifest_with_scope_changes(previous, &scanned, &deleted);
        Ok(ManifestDelta {
            changed,
            deleted,
            next_manifest,
        })
    }

    /// Farkı canlı engine'e uygular. Dosyalar kilitsiz hazırlanır (ayrıştırma,
    /// mevcut vektörler, embedding); aktivasyon kilidi yalnızca işaretçi denetimi,
    /// vektör satırlarının değiştirilmesi ve graf değişimi süresince tutulur.
    /// Önceki yarım uygulamanın dosyaları farkta olmasa da yeniden uygulanır.
    async fn apply_delta(
        &self,
        state: &mut LiveState,
        delta: ManifestDelta,
        started_at: u64,
    ) -> Result<LiveRefresh> {
        let mut files: BTreeSet<String> = state.dirty_files.clone();
        files.extend(delta.changed.iter().cloned());
        files.extend(delta.deleted.iter().cloned());
        if files.is_empty() {
            state.manifest = delta.next_manifest;
            state.needs_rescan = false;
            self.record_applied_at(started_at);
            return Ok(LiveRefresh::Applied(Box::new(IndexStats {
                semantic_unavailable: state.semantic_unavailable.clone(),
                ..IndexStats::default()
            })));
        }

        // Tur yarıda kalırsa sonraki tur tam karşılaştırma yapar.
        state.needs_rescan = true;
        let changed_files: Vec<PathBuf> = files
            .iter()
            .map(|file_id| file_id_to_path(&self.project_root, file_id))
            .collect();
        let project_root = self.project_root.to_string_lossy().to_string();
        let prepared = self
            .engine
            .prepare_file_changes(&project_root, &changed_files)
            .await?;
        let embedded = match self.engine.embed_prepared(&prepared).await {
            Ok(embedded) => embedded,
            // Graf değişikliği yine uygulanır; yalnızca yeni düğümlerin vektörleri eksik kalır.
            Err(error) if crate::vector::remote::is_embedder_unavailable(&error) => {
                tracing::warn!(
                    project = %self.project_root.display(),
                    error = %error,
                    "Embedding service unreachable during live refresh; graph changes are applied without vectors"
                );
                state.semantic_unavailable = Some(error.to_string());
                EmbeddedNodes::default()
            }
            Err(error) => return Err(error),
        };

        let artifact_parent = self.artifact_parent()?.to_path_buf();
        let lock_parent = artifact_parent.clone();
        let _activation_lock =
            tokio::task::spawn_blocking(move || ActivationLock::acquire(&lock_parent))
                .await
                .map_err(|error| anyhow::anyhow!("Activation lock task failed: {}", error))??;
        let current = read_current_pointer_value(&artifact_parent)?;
        if current != self.artifacts.generation_id {
            tracing::info!(
                project = %self.project_root.display(),
                live_generation = ?self.artifacts.generation_id,
                active_generation = ?current,
                "Another process activated a new index generation; discarding the prepared live changes"
            );
            state.superseded = true;
            return Ok(LiveRefresh::Superseded);
        }

        let file_count = files.len();
        state.dirty_files = files;
        let counts = embedded.counts;
        self.engine.write_file_vectors(&prepared, embedded).await?;
        fail_before_graph_swap_for_tests()?;
        let mut stats = self.engine.swap_file_graphs(prepared).await;
        stats.embedded_chunks = counts.embedded;
        stats.reused_chunks = counts.reused;
        // Okunamayan dosyaların eski düğümleri kaldı; sonraki tur onları yeniden dener.
        state.dirty_files = stats.retry_files.iter().cloned().collect();
        state.manifest =
            restore_retry_files(&state.manifest, delta.next_manifest, &stats.retry_files);
        state.version += 1;
        state.needs_rescan = false;
        stats.semantic_unavailable = state.semantic_unavailable.clone();
        self.record_applied_at(started_at);
        tracing::info!(
            project = %self.project_root.display(),
            files = file_count,
            changed = delta.changed.len(),
            deleted = delta.deleted.len(),
            files_indexed = stats.files_indexed,
            files_failed = stats.files_failed,
            "Applied live index refresh"
        );
        Ok(LiveRefresh::Applied(Box::new(stats)))
    }

    fn record_applied_at(&self, started_at: u64) {
        *self
            .applied_at
            .lock()
            .expect("live index timestamp lock poisoned") = Some(started_at);
    }
}

/// Test kancası: `CCM_INTERNAL_LIVE_TEST_FAIL_BEFORE_GRAPH_SWAP` tanımlıysa
/// uygulama vektör satırları değiştikten sonra, graf değişmeden önce başarısız
/// olur. Yarıda kalan bir uygulamanın grafı bozmadığını ve sonraki turun
/// onardığını doğrulamak içindir.
fn fail_before_graph_swap_for_tests() -> Result<()> {
    if std::env::var_os("CCM_INTERNAL_LIVE_TEST_FAIL_BEFORE_GRAPH_SWAP").is_some() {
        anyhow::bail!(
            "Live refresh failed on purpose before the graph swap (CCM_INTERNAL_LIVE_TEST_FAIL_BEFORE_GRAPH_SWAP)"
        );
    }
    Ok(())
}

/// Yollardan biri, değişince tüm dosya kümesini etkileyebilen bir ignore dosyası mı?
fn touches_ignore_rules(paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| IGNORE_RULE_FILES.contains(&name))
    })
}

/// Hedefli taramanın sonucunu manifeste uygular; zaman damgası ve commit
/// değişmez (taranmayan dosyaların parmak izleri eski zamana aittir).
fn manifest_with_scope_changes(
    previous: &IndexManifest,
    scanned: &HashMap<String, FileFingerprint>,
    deleted: &[String],
) -> IndexManifest {
    let mut files = previous.files.clone();
    for file_id in deleted {
        files.remove(file_id);
    }
    files.extend(
        scanned
            .iter()
            .map(|(file_id, fingerprint)| (file_id.clone(), fingerprint.clone())),
    );
    IndexManifest {
        files,
        ..previous.clone()
    }
}

/// Anlık görüntüyü geçici dosyalara yazar; aktivasyon kilidi altında işaretçi
/// hâlâ beklenen generation'ı gösteriyorsa önce grafı sonra manifesti yerine
/// koyar. Kilit yalnızca denetim ve iki rename süresince tutulur.
fn write_snapshot(
    artifact_parent: &Path,
    expected_generation: &Option<String>,
    graph_path: &Path,
    manifest_path: &Path,
    snapshot: LiveSnapshot,
) -> Result<SnapshotWrite> {
    let graph_temp = artifact_temp_path(graph_path);
    let manifest_temp = artifact_temp_path(manifest_path);
    let written = write_snapshot_files(
        artifact_parent,
        expected_generation,
        (graph_path, &graph_temp),
        (manifest_path, &manifest_temp),
        snapshot,
    );
    if !matches!(written, Ok(SnapshotWrite::Written)) {
        for temp in [&graph_temp, &manifest_temp] {
            if let Err(error) = std::fs::remove_file(temp) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %temp.display(), error = %error, "Temporary index file could not be removed");
                }
            }
        }
    }
    written
}

fn write_snapshot_files(
    artifact_parent: &Path,
    expected_generation: &Option<String>,
    (graph_path, graph_temp): (&Path, &Path),
    (manifest_path, manifest_temp): (&Path, &Path),
    snapshot: LiveSnapshot,
) -> Result<SnapshotWrite> {
    if read_current_pointer_value(artifact_parent)? != *expected_generation {
        return Ok(SnapshotWrite::Superseded);
    }
    let graph_bytes = snapshot.graph.write_json_file(graph_temp)?;
    let manifest = IndexManifest {
        semantic_nodes: Some(semantic_node_counts(&snapshot.graph)),
        graph_bytes: Some(graph_bytes),
        ..snapshot.manifest
    };
    write_manifest_file(manifest_temp, &manifest)?;

    let _activation_lock = ActivationLock::acquire(artifact_parent)?;
    if read_current_pointer_value(artifact_parent)? != *expected_generation {
        return Ok(SnapshotWrite::Superseded);
    }
    replace_file_atomically(graph_temp, graph_path).map_err(|error| {
        anyhow::anyhow!(
            "Live graph '{}' could not be replaced: {}",
            graph_path.display(),
            error
        )
    })?;
    replace_file_atomically(manifest_temp, manifest_path).map_err(|error| {
        anyhow::anyhow!(
            "Live manifest '{}' could not be replaced: {}",
            manifest_path.display(),
            error
        )
    })?;
    if let Some(directory) = graph_path.parent() {
        sync_directory(directory)?;
    }
    Ok(SnapshotWrite::Written)
}
