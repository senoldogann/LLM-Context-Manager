//! MCP Server request handling logic.

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::protocol::{
    create_error_response, create_success_response, JsonRpcRequest, JsonRpcResponse,
    ResourcesCapability, ServerCapabilities, ServerInfo, ToolAnnotations, ToolDefinition,
    ToolsCapability,
};
use crate::tools;

use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::CodeGraph;
use ccm_core::live::{IndexMigrationRequired, LiveIndex};
use ccm_core::vector::store::LanceDbStore;

const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] =
    [LATEST_PROTOCOL_VERSION, "2025-06-18", "2025-03-26"];
/// Okuma araçlarının süren yenilemeyi bekleyeceği en uzun süre.
const FRESHNESS_WAIT_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);

/// Okuma araçları indeksi yalnızca okur; etki alanı yerel projedir.
const READ_ONLY_TOOL: ToolAnnotations = ToolAnnotations {
    read_only_hint: true,
    destructive_hint: false,
    idempotent_hint: true,
    open_world_hint: false,
};
/// İndeksleme araçları indeksi yazar ama proje dosyalarını silmez; aynı girdiyle
/// tekrar çağrı aynı indeksi üretir.
const INDEXING_TOOL: ToolAnnotations = ToolAnnotations {
    read_only_hint: false,
    destructive_hint: false,
    idempotent_hint: true,
    open_world_hint: false,
};

/// Holds the server's shared state.
pub struct ServerState {
    /// Yalnızca proje kökü olmadan başlatıldığında dolu (ev dizinindeki depo).
    pub default_engine: RwLock<Option<Arc<RetrievalEngine>>>,
    pub engines: RwLock<EngineCache>,
    index_locks:
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    pub(crate) index_jobs: std::sync::Mutex<std::collections::HashMap<String, IndexJob>>,
    pub(crate) next_index_job_id: std::sync::atomic::AtomicU64,
    default_project_root: Option<PathBuf>,
    /// `CCM_PROJECT_ROOT` açıkça verildiyse istemci roots'u varsayılan kökü değiştirmez.
    project_root_is_explicit: bool,
    default_db_path: PathBuf,
    allowed_roots: Vec<PathBuf>,
    require_allowed_roots: bool,
    /// İstemcinin MCP `roots/list` ile bildirdiği çalışma alanı kökleri. Host
    /// uygulaması (model değil) bildirdiği için izin listesine dahildir.
    client_roots: std::sync::RwLock<Vec<PathBuf>>,
    client_supports_roots: std::sync::atomic::AtomicBool,
    pending_roots_request_id: std::sync::Mutex<Option<String>>,
    next_client_request_id: std::sync::atomic::AtomicU64,
    outgoing_requests: std::sync::Mutex<Vec<Value>>,
    /// Proje başına otomatik yenileme durumu (anahtar: kanonik proje yolu).
    freshness:
        std::sync::Mutex<std::collections::HashMap<String, Arc<crate::freshness::FreshnessHandle>>>,
    /// Süren semantik yükseltme sayısı, proje başına (anahtar: kanonik proje yolu).
    /// Aynı projede üst üste hızlı indeks alınırsa yükseltmeler çakışır; sayaç
    /// yenilemenin ancak sonuncusu bitince başlamasını sağlar.
    semantic_upgrades: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

#[derive(Clone)]
pub(crate) struct IndexJob {
    pub id: u64,
    pub receiver: tokio::sync::watch::Receiver<Option<crate::protocol::ToolResult>>,
}

const DEFAULT_ENGINE_CACHE_SIZE: usize = 8;

fn engine_cache_size() -> usize {
    std::env::var("CCM_MCP_ENGINE_CACHE_SIZE")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .map(|val| val.max(1))
        .unwrap_or(DEFAULT_ENGINE_CACHE_SIZE)
}

use lru::LruCache;
use std::num::NonZeroUsize;

/// Önbellekteki engine ve otomatik yenilemenin onu nasıl güncellediği.
#[derive(Clone)]
pub struct CachedEngine {
    pub engine: Arc<RetrievalEngine>,
    pub source: EngineSource,
}

/// Engine'in kaynağı.
#[derive(Clone)]
pub enum EngineSource {
    /// Etkin generation'a bağlı canlı indeks; yenilemeler yerinde uygulanır.
    Live(Arc<LiveIndex>),
    /// Canlı güncellenemeyen (eski şema ya da dosya yolu biçimi) indeks. Okumalar
    /// onu olduğu gibi kullanır; otomatik yenileme onu önce worker'ın
    /// `update_index`'iyle taşır, ardından yeni generation canlı yüklenir.
    NeedsMigration {
        reason: IndexMigrationRequired,
        generation_id: Option<String>,
        indexed_at: Option<u64>,
    },
    /// Kök dizinsiz varsayılan depo; otomatik yenilemesi yoktur.
    Rootless,
}

/// Önbellekten alınan engine'in yeni mi yüklendiği.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineLoad {
    Cached,
    Loaded,
}

impl CachedEngine {
    fn live(live: LiveIndex) -> Self {
        let live = Arc::new(live);
        Self {
            engine: live.engine(),
            source: EngineSource::Live(live),
        }
    }

    /// İndeksin diski en son yansıttığı an (unix saniye). Canlı indekste her
    /// uygulanan yenilemeyle birlikte ilerler.
    pub fn indexed_at(&self) -> Option<u64> {
        match &self.source {
            EngineSource::Live(live) => live.indexed_at(),
            EngineSource::NeedsMigration { indexed_at, .. } => *indexed_at,
            EngineSource::Rootless => None,
        }
    }

    /// Engine'in yüklendiği generation; düz (eski) yerleşimde `None`.
    fn generation_id(&self) -> Option<&str> {
        match &self.source {
            EngineSource::Live(live) => live.generation_id(),
            EngineSource::NeedsMigration { generation_id, .. } => generation_id.as_deref(),
            EngineSource::Rootless => None,
        }
    }
}

pub struct EngineCache {
    cache: LruCache<String, CachedEngine>,
}

impl EngineCache {
    fn new(max: usize) -> Self {
        let cap = NonZeroUsize::new(max).unwrap_or_else(|| NonZeroUsize::new(1).unwrap());
        Self {
            cache: LruCache::new(cap),
        }
    }

    fn get(&mut self, key: &str) -> Option<CachedEngine> {
        self.cache.get(key).cloned()
    }

    #[allow(dead_code)]
    fn peek(&self, key: &str) -> Option<CachedEngine> {
        self.cache.peek(key).cloned()
    }

    /// Projenin yeni generation'ını ekler ve aynı projenin eski
    /// generation'larını düşürür. Otomatik yenileme her kayıtta yeni generation
    /// ürettiği için aksi halde eski graflar bellekte birikir; eski engine'i
    /// kullanan istekler `Arc` sayesinde bitene kadar onu korur.
    fn insert(&mut self, project_key: &str, key: String, engine: CachedEngine) -> CachedEngine {
        let prefix = format!("{}#", project_key);
        let stale: Vec<String> = self
            .cache
            .iter()
            .map(|(existing, _)| existing.clone())
            .filter(|existing| existing.starts_with(&prefix) && *existing != key)
            .collect();
        for existing in stale {
            self.cache.pop(&existing);
        }
        self.cache.put(key, engine.clone());
        engine
    }
}

impl ServerState {
    pub(crate) fn project_index_lock(
        &self,
        project_path: &str,
    ) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        let mut locks = self.index_locks.lock().unwrap();
        locks
            .entry(cache_key)
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub(crate) fn index_job_in_progress(&self, project_path: &str) -> bool {
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        self.index_jobs
            .lock()
            .unwrap()
            .get(&cache_key)
            .is_some_and(|job| job.receiver.borrow().is_none())
    }

    pub(crate) fn remove_index_job_if_id(&self, job_key: &str, job_id: u64) {
        let mut jobs = self.index_jobs.lock().unwrap();
        if jobs.get(job_key).is_some_and(|job| job.id == job_id) {
            jobs.remove(job_key);
        }
    }

    /// Proje kilidini haritadan çıkarır, ama yalnızca başka kimse tutmuyorsa
    /// (`Arc` sayısı 1: yalnızca harita). Kilidi tutan ya da bekleyen biri (ör.
    /// yenileme görevi) varsa girdi kalır; aksi halde sonraki çağrı yeni bir mutex
    /// üretir ve iki indeksleme aynı projede eşzamanlı çalışır.
    pub(crate) fn release_index_lock(&self, job_key: &str) {
        let mut locks = self.index_locks.lock().unwrap();
        if locks
            .get(job_key)
            .is_some_and(|lock| std::sync::Arc::strong_count(lock) == 1)
        {
            locks.remove(job_key);
        }
    }

    pub(crate) fn project_db_path(&self, project_path: &str) -> Result<PathBuf> {
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        if self
            .default_project_root
            .as_ref()
            .is_some_and(|root| canonical_path == *root)
        {
            return Ok(self.default_db_path.clone());
        }
        let candidate = canonical_path.join("data/ccm_db");
        ccm_core::resolve_artifact_path(&canonical_path, &candidate)
    }

    /// Projenin etkin indeksi diskte var mı? `get_engine` ile aynı artefakt denetimini
    /// kullanır; otomatik yenileme silinmiş bir indeksi yeniden kurmamak için sorar.
    pub(crate) fn project_index_exists(&self, project_key: &str) -> Result<bool> {
        Ok(index_artifacts_exist(&self.project_artifacts(project_key)?))
    }

    fn project_artifacts(&self, project_path: &str) -> Result<ccm_core::IndexArtifactPaths> {
        let requested_db = self.project_db_path(project_path)?;
        ccm_core::resolve_index_artifacts(
            project_path,
            Some(requested_db.to_string_lossy().as_ref()),
        )
    }

    pub async fn new() -> Result<Self> {
        tracing::info!("Initializing CCM Core Engine for MCP...");

        let explicit_project_root = std::env::var("CCM_PROJECT_ROOT")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let require_allowed_roots = require_allowed_roots();
        let allowed_roots = load_allowed_roots();
        let home_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(|home| canonicalize_project_path(Path::new(&home)));
        let launch_dir = std::env::current_dir()
            .ok()
            .map(|cwd| canonicalize_project_path(&cwd));
        let default_project_root = resolve_startup_project_root(
            explicit_project_root.as_deref().map(Path::new),
            &allowed_roots,
            launch_dir.as_deref(),
            home_dir.as_deref(),
        );
        let project_root = default_project_root
            .as_ref()
            .map(|root| root.to_string_lossy().to_string());

        // Prefer the selected project's shared index. A home-directory fallback is
        // only used when no project root is available.
        let db_path = if let Ok(path) = std::env::var("CCM_DB_PATH") {
            path
        } else if let Some(root) = &default_project_root {
            let candidate = root.join("data/ccm_db");
            // v0.3.9 kapsama güvencesi: `data` dış dizine symlink ise ya da
            // yol kök dışına çözülürse sessizce dış dizine bağlanma; server
            // başlatılamaz (tool çağrıları da aynı hatayı görür).
            ccm_core::resolve_artifact_path(root, &candidate)?
                .to_string_lossy()
                .to_string()
        } else {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            let path = PathBuf::from(home)
                .join(".ccm")
                .join("mcp")
                .join("data")
                .join("ccm_mcp_db");
            let _ = std::fs::create_dir_all(&path);
            path.to_string_lossy().to_string()
        };

        tracing::info!(path = %db_path, "Using Vector DB path");

        // Auto-indexing removed to allow agentic control.
        if let Some(root) = &project_root {
            tracing::info!(
                root = %root,
                "Project root detected. Indexing is available on demand."
            );
        }

        let default_db_path = PathBuf::from(&db_path);
        // Proje kökü varken motorlar proje bazında (get_engine) yüklenir. Başlangıçta
        // proje deposunu açmak projeye yan etkiyle `data/` yazar; varsayılan motor
        // yalnızca hiç kök yokken ev dizinindeki depo için kurulur.
        let default_engine = match &default_project_root {
            Some(_) => None,
            None => Some(load_rootless_engine(&default_db_path).await?),
        };
        let cache_size = engine_cache_size();

        if require_allowed_roots && allowed_roots.is_empty() {
            tracing::warn!(
                "CCM_ALLOWED_ROOTS is required but empty. MCP will reject all project paths."
            );
        }

        Ok(Self {
            default_engine: RwLock::new(default_engine),
            engines: RwLock::new(EngineCache::new(cache_size)),
            index_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
            index_jobs: std::sync::Mutex::new(std::collections::HashMap::new()),
            next_index_job_id: std::sync::atomic::AtomicU64::new(1),
            default_project_root,
            project_root_is_explicit: explicit_project_root.is_some(),
            default_db_path,
            allowed_roots,
            require_allowed_roots,
            client_roots: std::sync::RwLock::new(Vec::new()),
            client_supports_roots: std::sync::atomic::AtomicBool::new(false),
            pending_roots_request_id: std::sync::Mutex::new(None),
            next_client_request_id: std::sync::atomic::AtomicU64::new(1),
            outgoing_requests: std::sync::Mutex::new(Vec::new()),
            freshness: std::sync::Mutex::new(std::collections::HashMap::new()),
            semantic_upgrades: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Varsayılan proje kökü: açık `CCM_PROJECT_ROOT` > istemcinin ilk root'u >
    /// başlangıçta çözülen kök (izinli cwd veya tek allowlist girdisi).
    fn effective_project_root(&self) -> Option<PathBuf> {
        if self.project_root_is_explicit {
            return self.default_project_root.clone();
        }
        self.client_roots
            .read()
            .unwrap()
            .first()
            .cloned()
            .or_else(|| self.default_project_root.clone())
    }

    /// İstekteki `project_path` ya da etkin proje kökü için kanonik anahtar;
    /// ikisi de yoksa `None` (ev dizini deposu; tazelik satırı eklenmez).
    pub(crate) fn project_key(&self, project_path: Option<&str>) -> Option<String> {
        match project_path {
            Some(path) => Some(project_key_for_path(path)),
            None => self
                .effective_project_root()
                .map(|root| project_key_for_path(&root.to_string_lossy())),
        }
    }

    /// Proje için otomatik yenilemeyi bir kez başlatır. Kapalıysa ya da izlenen
    /// proje sınırı doluysa ilgili durumla kaydedilir; sonraki çağrılar bir şey
    /// yapmaz.
    pub(crate) fn ensure_auto_refresh(self: &Arc<Self>, project_key: &str) {
        let mut handles = self.freshness.lock().unwrap();
        if handles.contains_key(project_key) {
            return;
        }
        let handle = if !crate::freshness::auto_refresh_enabled() {
            crate::freshness::inactive_handle(crate::freshness::WatcherStatus::Disabled)
        } else if handles.len() >= engine_cache_size() {
            crate::freshness::inactive_handle(crate::freshness::WatcherStatus::Unavailable(
                "watcher limit reached".to_string(),
            ))
        } else {
            match self.project_db_path(project_key) {
                Ok(db_path) => crate::freshness::start_auto_refresh(
                    self.clone(),
                    project_key.to_string(),
                    db_path,
                ),
                Err(error) => {
                    crate::freshness::inactive_handle(crate::freshness::WatcherStatus::Unavailable(
                        format!("index path could not be resolved: {error}"),
                    ))
                }
            }
        };
        handles.insert(project_key.to_string(), handle);
    }

    pub(crate) fn freshness_handle(
        &self,
        project_key: &str,
    ) -> Option<Arc<crate::freshness::FreshnessHandle>> {
        self.freshness.lock().unwrap().get(project_key).cloned()
    }

    /// Elle indeksleme sonrası otomatik yenilemeden durum doğrulaması ister.
    pub(crate) fn request_refresh(&self, project_key: &str) {
        if let Some(handle) = self.freshness_handle(project_key) {
            crate::freshness::request_rescan(&handle, project_key);
        }
    }

    /// Hızlı indeksin semantik yükseltmesi başladı; otomatik yenileme ertelenir.
    pub(crate) fn begin_semantic_upgrade(&self, project_key: &str) {
        *self
            .semantic_upgrades
            .lock()
            .unwrap()
            .entry(project_key.to_string())
            .or_insert(0) += 1;
    }

    /// Yükseltme bitti (başarılı ya da değil). Projede başka yükseltme kalmadıysa
    /// ertelenen yenileme uyandırılır; çakışan bir yükseltme sürüyorsa ertelenme
    /// onun bitişine kadar sürer.
    pub(crate) fn end_semantic_upgrade(&self, project_key: &str) {
        let remaining = {
            let mut upgrades = self.semantic_upgrades.lock().unwrap();
            match upgrades.get_mut(project_key) {
                Some(count) if *count > 1 => {
                    *count -= 1;
                    *count
                }
                Some(_) => {
                    upgrades.remove(project_key);
                    0
                }
                None => {
                    tracing::warn!(
                        project = %project_key,
                        "Semantic upgrade ended without a registered start"
                    );
                    0
                }
            }
        };
        if remaining == 0 {
            self.request_refresh(project_key);
        }
    }

    pub(crate) fn semantic_upgrade_running(&self, project_key: &str) -> bool {
        self.semantic_upgrades
            .lock()
            .unwrap()
            .get(project_key)
            .is_some_and(|count| *count > 0)
    }

    /// Projenin bekleyen yenilemesini en fazla `budget` kadar bekler; otomatik
    /// yenileme kaydı yoksa kapalı durum döner.
    pub(crate) async fn wait_until_fresh(
        &self,
        project_key: &str,
        budget: std::time::Duration,
    ) -> crate::freshness::ProjectFreshness {
        match self.freshness_handle(project_key) {
            Some(handle) => crate::freshness::wait_until_fresh(&handle, budget).await,
            None => crate::freshness::disabled_freshness(),
        }
    }

    /// Projenin anlık tazelik durumu (beklemeden); otomatik yenileme kaydı yoksa
    /// kapalı durum.
    pub(crate) fn current_freshness(
        &self,
        project_key: &str,
    ) -> crate::freshness::ProjectFreshness {
        match self.freshness_handle(project_key) {
            Some(handle) => handle.state.borrow().clone(),
            None => crate::freshness::disabled_freshness(),
        }
    }

    /// İstemciye gönderilmeyi bekleyen JSON-RPC isteklerini (ör. `roots/list`) boşaltır.
    pub fn take_outgoing_requests(&self) -> Vec<Value> {
        std::mem::take(&mut *self.outgoing_requests.lock().unwrap())
    }

    fn queue_roots_request(&self) {
        if !self
            .client_supports_roots
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let sequence = self
            .next_client_request_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let request_id = format!("ccm-roots-{sequence}");
        *self.pending_roots_request_id.lock().unwrap() = Some(request_id.clone());
        self.outgoing_requests.lock().unwrap().push(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "roots/list"
        }));
    }

    /// İstemcinin sunucu isteğine verdiği yanıtı işler (şu an yalnızca `roots/list`).
    fn handle_client_response(&self, response: &serde_json::Map<String, Value>) {
        let response_id = response.get("id").and_then(Value::as_str);
        let mut pending = self.pending_roots_request_id.lock().unwrap();
        if response_id.is_none() || pending.as_deref() != response_id {
            tracing::warn!(id = ?response.get("id"), "Ignoring response to an unknown server request");
            return;
        }
        *pending = None;
        drop(pending);

        if let Some(error) = response.get("error") {
            tracing::warn!(error = %error, "Client rejected roots/list; using the startup project root");
            return;
        }
        let roots = parse_client_roots(response.get("result"));
        tracing::info!(roots = ?roots, "Client workspace roots updated");
        *self.client_roots.write().unwrap() = roots;
    }

    /// Retrieves the engine for a specific project path, or defaults to the startup engine.
    /// Loads the engine dynamically if it's not in the cache.
    pub async fn get_engine(&self, project_path: Option<&str>) -> Result<CachedEngine> {
        let path = match project_path {
            Some(path) => path.to_string(),
            None => {
                let Some(root) = self.effective_project_root() else {
                    if self.require_allowed_roots {
                        return Err(anyhow::anyhow!(
                            "No default project root is available and strict allowlist mode is enabled. Set CCM_PROJECT_ROOT and CCM_ALLOWED_ROOTS."
                        ));
                    }
                    return self
                        .default_engine
                        .read()
                        .await
                        .clone()
                        .map(|engine| CachedEngine {
                            engine,
                            source: EngineSource::Rootless,
                        })
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "No project root is available. Pass 'project_path' or set CCM_PROJECT_ROOT."
                            )
                        });
                };
                root.to_string_lossy().to_string()
            }
        };

        if !self.is_path_allowed(&path) {
            return Err(anyhow::anyhow!(
                "Project path '{}' is not allowed. Set CCM_ALLOWED_ROOTS to permit access.",
                path
            ));
        }

        // Cache key normalize edilir; "/repo" ile "/repo/" ayrı entry oluşturmasın.
        let canonical_path = canonicalize_project_path(Path::new(&path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        let (engine, load) = self.cached_project_engine(&cache_key).await?;
        // Okuma yeni bir generation yükledi (başka bir süreç kurdu ya da önbellekten
        // düştü): bu generation canlı uygulanmış değişiklikleri içermeyebilir. Tam
        // karşılaştırma istenir ve bekleyen iş hemen yayınlanır; okumalar onu
        // bekler ya da bayat raporlanır, yanlışlıkla "fresh" görünmez.
        if load == EngineLoad::Loaded {
            self.request_refresh(&cache_key);
        }
        Ok(engine)
    }

    /// Otomatik yenilemenin kullandığı engine. `get_engine`'den farkı: yeni
    /// yüklenen generation için ayrıca tam karşılaştırma istemez; yenileme turu
    /// yeni canlı indekste zaten tam karşılaştırma yapar.
    pub(crate) async fn refresh_engine(&self, project_key: &str) -> Result<CachedEngine> {
        if !self.is_path_allowed(project_key) {
            return Err(anyhow::anyhow!(
                "Project path '{}' is not allowed. Set CCM_ALLOWED_ROOTS to permit access.",
                project_key
            ));
        }
        let (engine, _) = self.cached_project_engine(project_key).await?;
        Ok(engine)
    }

    /// Projenin etkin generation'ına ait engine'i önbellekten döndürür ya da yükler.
    async fn cached_project_engine(&self, cache_key: &str) -> Result<(CachedEngine, EngineLoad)> {
        let artifacts = self.project_artifacts(cache_key)?;
        let engine_cache_key = format!(
            "{}#{}",
            cache_key,
            artifacts.generation_id.as_deref().unwrap_or("legacy")
        );

        // Check cache (write lock needed for LRU order update)
        {
            let mut engines = self.engines.write().await;
            if let Some(engine) = engines.get(&engine_cache_key) {
                return Ok((engine, EngineLoad::Cached));
            }
        }

        tracing::info!(path = %cache_key, "Loading context for project");

        // Uzun full index retrieval çağrısının içinde çalıştırılmaz.
        if !index_artifacts_exist(&artifacts) {
            // İlk indeksleme sürerken okunacak generation yoktur; iş bitince aynı
            // çağrı çalışır. Var olan generation ise yeniden indeksleme sırasında
            // okunmaya devam eder (generation geçişi atomiktir).
            if self.index_job_in_progress(cache_key) {
                return Err(anyhow::anyhow!(
                    "Project indexing is in progress. Retry this tool after index_project reports completion."
                ));
            }
            return Err(anyhow::anyhow!(
                "Project index is missing. Call index_project first; large indexes run in the background."
            ));
        }

        let engine = self.load_project_engine(cache_key).await?;
        // İşaretçi yükleme sırasında ilerlemiş olabilir; anahtar yüklenen
        // generation'dan alınır.
        let engine_cache_key = project_engine_cache_key(cache_key, &engine);
        let mut engines = self.engines.write().await;
        if let Some(existing) = engines.get(&engine_cache_key) {
            return Ok((existing, EngineLoad::Cached));
        }
        Ok((
            engines.insert(cache_key, engine_cache_key, engine),
            EngineLoad::Loaded,
        ))
    }

    pub async fn refresh_project_engine(&self, project_path: &str) -> Result<()> {
        // get_engine ile aynı normalize key kullanılır ki cache tutarlı kalsın.
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        let engine = self.load_project_engine(&cache_key).await?;
        let engine_cache_key = project_engine_cache_key(&cache_key, &engine);
        self.engines
            .write()
            .await
            .insert(&cache_key, engine_cache_key, engine);
        Ok(())
    }

    /// Projenin etkin generation'ını canlı indeks olarak yükler. Canlı
    /// güncellenemeyen (taşınması gereken) indeks okumalar için olduğu gibi
    /// yüklenir; otomatik yenileme onu worker'la taşır.
    async fn load_project_engine(&self, cache_key: &str) -> Result<CachedEngine> {
        let requested_db_path = self.project_db_path(cache_key)?;
        let db_path = requested_db_path.to_string_lossy().to_string();
        let policy_path = requested_db_path
            .parent()
            .map(|parent| parent.join("ccm_learn/policies.json"));
        match LiveIndex::load(cache_key, Some(&db_path), policy_path.as_deref()).await {
            Ok(live) => Ok(CachedEngine::live(live)),
            Err(error) => match error.downcast::<IndexMigrationRequired>() {
                Ok(reason) => {
                    tracing::info!(
                        project = %cache_key,
                        reason = %reason,
                        "Index cannot be refreshed live until a full re-index migrates it; serving it read-only"
                    );
                    load_migration_engine(cache_key, &db_path, policy_path.as_deref(), reason).await
                }
                Err(error) => Err(error),
            },
        }
    }

    fn is_path_allowed(&self, path: &str) -> bool {
        let candidate = canonicalize_project_path(Path::new(path));
        if self
            .client_roots
            .read()
            .unwrap()
            .iter()
            .any(|root| candidate.starts_with(root))
        {
            return true;
        }
        if self.allowed_roots.is_empty() {
            if self.require_allowed_roots {
                return false;
            }
            // Strict mod kapalıyken bile keyfi yollara izin verilmez:
            // yalnızca başlangıçta seçilen default proje kökü kabul edilir.
            return self
                .default_project_root
                .as_ref()
                .is_some_and(|root| candidate.starts_with(root));
        }
        self.allowed_roots
            .iter()
            .any(|root| candidate.starts_with(root))
    }
}

/// Engine önbelleği anahtarı: proje ve yüklenen generation (düz yerleşimde
/// `legacy`).
fn project_engine_cache_key(project_key: &str, engine: &CachedEngine) -> String {
    format!(
        "{}#{}",
        project_key,
        engine.generation_id().unwrap_or("legacy")
    )
}

/// Canlı yüklenemeyen indeksin okumalar için engine'i: graf ve vektör tablosu
/// olduğu gibi okunur, indeks taşınana kadar güncellenmez.
async fn load_migration_engine(
    project_key: &str,
    db_path: &str,
    policy_path: Option<&Path>,
    reason: IndexMigrationRequired,
) -> Result<CachedEngine> {
    let artifacts = ccm_core::resolve_index_artifacts(project_key, Some(db_path))?;
    let graph_path = artifacts.graph_path.to_string_lossy().to_string();
    let graph = CodeGraph::load_from_file(&graph_path).map_err(|error| {
        anyhow::anyhow!(
            "Project graph '{}' could not be loaded: {}. Run index_project to rebuild it.",
            graph_path,
            error
        )
    })?;
    let store = LanceDbStore::new(&artifacts.db_path.to_string_lossy(), "code_vectors").await?;
    let indexed_at = ccm_core::read_index_timestamp(&artifacts.manifest_path)?;
    Ok(CachedEngine {
        engine: Arc::new(RetrievalEngine::new_with_active_policy(
            Arc::new(RwLock::new(graph)),
            store,
            policy_path,
        )),
        source: EngineSource::NeedsMigration {
            reason,
            generation_id: artifacts.generation_id,
            indexed_at,
        },
    })
}

/// Etkin generation'ın veritabanı, graf ve manifest artefaktlarının üçü de diskte mi?
fn index_artifacts_exist(artifacts: &ccm_core::IndexArtifactPaths) -> bool {
    artifacts.db_path.exists()
        && artifacts.graph_path.is_file()
        && artifacts.manifest_path.is_file()
}

fn load_allowed_roots() -> Vec<PathBuf> {
    let raw = std::env::var("CCM_ALLOWED_ROOTS").unwrap_or_default();
    let mut roots: Vec<PathBuf> = if raw.trim().is_empty() {
        Vec::new()
    } else {
        let parts: Vec<&str> = if cfg!(windows) {
            raw.split([';', ',']).collect()
        } else {
            raw.split([':', ';', ',']).collect()
        };

        parts
            .into_iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(|item| canonicalize_project_path(Path::new(item)))
            .collect()
    };

    if roots.is_empty() {
        if let Ok(root) = std::env::var("CCM_PROJECT_ROOT") {
            roots.push(canonicalize_project_path(Path::new(&root)));
        }
    }

    roots
}

/// Proje kökü olmadan başlatılan sunucunun ev dizinindeki depoya bağlı motorunu kurar.
async fn load_rootless_engine(db_path: &Path) -> Result<Arc<RetrievalEngine>> {
    let artifact_parent = db_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let graph_path = artifact_parent.join("ccm_graph.json");
    let graph = if graph_path.exists() {
        CodeGraph::load_from_file(&graph_path.to_string_lossy()).map_err(|error| {
            anyhow::anyhow!(
                "Graph '{}' could not be loaded: {}. Remove it or run index_project.",
                graph_path.display(),
                error
            )
        })?
    } else {
        CodeGraph::new()
    };
    let store = LanceDbStore::new(&db_path.to_string_lossy(), "code_vectors").await?;
    let policy_path = artifact_parent.join("ccm_learn/policies.json");
    Ok(Arc::new(RetrievalEngine::new_with_active_policy(
        Arc::new(RwLock::new(graph)),
        store,
        Some(policy_path.as_path()),
    )))
}

/// Başlangıç varsayılan kökü: açık kök > izinli başlatma dizini (cwd) > tek
/// allowlist girdisi. `/` ve ev dizini örtük kök olamaz; host uygulamaları
/// (ör. Claude Desktop) sunucuyu çoğu zaman bu dizinlerde başlatır.
fn resolve_startup_project_root(
    explicit_root: Option<&Path>,
    allowed_roots: &[PathBuf],
    launch_dir: Option<&Path>,
    home_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(root) = explicit_root {
        return Some(canonicalize_project_path(root));
    }
    let usable_launch_dir = launch_dir.filter(|dir| {
        dir.parent().is_some()
            && home_dir != Some(*dir)
            && (allowed_roots.is_empty() || allowed_roots.iter().any(|root| dir.starts_with(root)))
    });
    if let Some(dir) = usable_launch_dir {
        return Some(dir.to_path_buf());
    }
    match allowed_roots {
        [single_root] => Some(single_root.clone()),
        _ => None,
    }
}

/// `roots/list` sonucundaki `file://` URI'lerini kanonik yollara çevirir.
fn parse_client_roots(result: Option<&Value>) -> Vec<PathBuf> {
    let Some(entries) = result
        .and_then(|value| value.get("roots"))
        .and_then(Value::as_array)
    else {
        tracing::warn!("roots/list result has no 'roots' array");
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let uri = entry.get("uri").and_then(Value::as_str)?;
            let path = url::Url::parse(uri)
                .ok()
                .filter(|url| url.scheme() == "file")
                .and_then(|url| url.to_file_path().ok());
            if path.is_none() {
                tracing::warn!(uri = %uri, "Ignoring non-file workspace root");
            }
            path.map(|path| canonicalize_project_path(&path))
        })
        .collect()
}

fn require_allowed_roots() -> bool {
    // Varsayılan strict: allowlist zorunlu. Geniş erişim için açıkça
    // CCM_REQUIRE_ALLOWED_ROOTS=0 verilmesi gerekir.
    std::env::var("CCM_REQUIRE_ALLOWED_ROOTS")
        .or_else(|_| std::env::var("CCM_MCP_REQUIRE_ALLOWED_ROOTS"))
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(true)
}

fn canonicalize_project_path(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    std::fs::canonicalize(&abs).unwrap_or_else(|_| normalize_path(&abs))
}

/// Proje yolundan önbellek ve tazelik durumu için kanonik anahtar üretir.
pub(crate) fn project_key_for_path(path: &str) -> String {
    canonicalize_project_path(Path::new(path))
        .to_string_lossy()
        .to_string()
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(std::path::MAIN_SEPARATOR_STR),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => result.push(part),
        }
    }
    result
}

/// Main request dispatcher.
/// Returns Ok(Some(response)) for requests, Ok(None) for notifications.
pub async fn handle_request(
    state: &Arc<ServerState>,
    raw_request: &str,
) -> Result<Option<JsonRpcResponse>> {
    let raw_value: Value = serde_json::from_str(raw_request)?;
    let Some(object) = raw_value.as_object() else {
        return Ok(Some(create_error_response(
            None,
            -32600,
            "Invalid Request: JSON-RPC payload must be an object",
        )));
    };
    let request_id = object.get("id").cloned();
    let is_notification = !object.contains_key("id");
    if request_id
        .as_ref()
        .is_some_and(|id| !(id.is_null() || id.is_string() || id.is_number()))
    {
        return Ok(Some(create_error_response(
            None,
            -32600,
            "Invalid Request: id must be a string, number, or null",
        )));
    }
    // Sunucunun istemciye gönderdiği isteklerin (ör. roots/list) yanıtları.
    if !object.contains_key("method")
        && (object.contains_key("result") || object.contains_key("error"))
    {
        state.handle_client_response(object);
        return Ok(None);
    }
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || object.get("method").and_then(Value::as_str).is_none()
    {
        return Ok(Some(create_error_response(
            request_id,
            -32600,
            "Invalid Request: jsonrpc must be '2.0' and method must be a string",
        )));
    }
    if object
        .get("params")
        .is_some_and(|params| !(params.is_object() || params.is_array()))
    {
        if is_notification {
            return Ok(None);
        }
        return Ok(Some(create_error_response(
            request_id,
            -32602,
            "Invalid params: params must be an object or array",
        )));
    }
    let request: JsonRpcRequest = serde_json::from_value(raw_value)?;

    if request.jsonrpc != "2.0" {
        return Ok(Some(create_error_response(
            request.id,
            -32600,
            "Invalid Request: jsonrpc must be '2.0'",
        )));
    }

    let response = match request.method.as_str() {
        "initialize" => {
            let supports_roots = request
                .params
                .as_ref()
                .and_then(|params| params.pointer("/capabilities/roots"))
                .is_some_and(Value::is_object);
            state
                .client_supports_roots
                .store(supports_roots, std::sync::atomic::Ordering::SeqCst);
            handle_initialize(request.id, request.params.as_ref()).map(Some)
        }
        "initialized" | "notifications/initialized" => {
            state.queue_roots_request();
            Ok(Some(create_success_response(request.id, json!({}))))
        }
        "notifications/roots/list_changed" => {
            state.queue_roots_request();
            Ok(None)
        }
        "ping" => Ok(Some(create_success_response(request.id, json!({})))),
        "tools/list" => handle_list_tools(request.id).map(Some),
        "resources/list" => Ok(Some(create_success_response(
            request.id,
            json!({ "resources": [] }),
        ))),
        "resources/templates/list" => Ok(Some(create_success_response(
            request.id,
            json!({ "resourceTemplates": [] }),
        ))),
        "tools/call" => {
            // `tools/call` notification'ı MCP sözleşmesinin parçası değildir.
            // Yanıt üretmediği için ağır bir tool'u (örn. index_project) bu
            // yoldan çalıştırmak ana JSON-RPC loop'unu bloklar ve sonraki
            // gerçek istekleri geciktirir. Notification olarak gelen
            // tools/call güvenle yok sayılır.
            if is_notification {
                tracing::warn!("tools/call notification ignored; use a request with an id instead");
                Ok(None)
            } else {
                handle_call_tool(state, request.id, request.params)
                    .await
                    .map(Some)
            }
        }
        _ => Ok(Some(create_error_response(
            request.id,
            -32601,
            &format!("Method not found: {}", request.method),
        ))),
    };

    if is_notification {
        if let Err(error) = response {
            tracing::warn!(method = %request.method, error = %error, "Notification failed");
        }
        Ok(None)
    } else {
        response
    }
}

fn handle_initialize(id: Option<Value>, params: Option<&Value>) -> Result<JsonRpcResponse> {
    let result = json!({
        "protocolVersion": negotiate_protocol_version(params),
        "capabilities": ServerCapabilities {
            tools: ToolsCapability { list_changed: false },
            resources: ResourcesCapability {
                subscribe: false,
                list_changed: false,
            },
        },
        "serverInfo": ServerInfo {
            name: "ccm-mcp".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    });
    Ok(create_success_response(id, result))
}

fn negotiate_protocol_version(params: Option<&Value>) -> &'static str {
    let requested = params
        .and_then(|value| value.get("protocolVersion"))
        .and_then(Value::as_str);

    match requested {
        Some(version) => SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .find(|supported| *supported == version)
            .unwrap_or(LATEST_PROTOCOL_VERSION),
        None => LATEST_PROTOCOL_VERSION,
    }
}

fn handle_list_tools(id: Option<Value>) -> Result<JsonRpcResponse> {
    let tools_list = vec![
        ToolDefinition {
            name: "get_context".to_string(),
            title: "Get Code Context".to_string(),
            description: Some("Get code context for a given file and line.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "The file path" },
                    "line": { "type": "integer", "minimum": 1, "description": "The line number" },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["file", "line"]
            }),
            annotations: READ_ONLY_TOOL,
        },
        ToolDefinition {
            name: "search_code".to_string(),
            title: "Search Code".to_string(),
            description: Some("Search the codebase using hybrid semantic and graph-aware ranking. Returns node IDs and location metadata so results can be chained into read_graph.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query (e.g. 'how does authentication work?')" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Optional maximum number of results to return. Defaults to 5." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["query"]
            }),
            annotations: READ_ONLY_TOOL,
        },
        ToolDefinition {
            name: "find_nodes".to_string(),
            title: "Find Graph Nodes".to_string(),
            description: Some("Find graph nodes by name, file path, or node ID fragment. Use this before read_graph when you do not already know the node ID.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "A node name, file path fragment, or node ID fragment to search for." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Optional maximum number of matches to return. Defaults to 10." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["query"]
            }),
            annotations: READ_ONLY_TOOL,
        },
        ToolDefinition {
            name: "read_graph".to_string(),
            title: "Read Graph Node".to_string(),
            description: Some("Get details of a specific code node by ID.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "The ID of the node to retrieve." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["node_id"]
            }),
            annotations: READ_ONLY_TOOL,
        },
        ToolDefinition {
            name: "index_project".to_string(),
            title: "Index Project".to_string(),
            description: Some("Refresh the project index. Usually performs an incremental update and reports when the existing index is already up to date. Use mode:'quick' for a fast graph-only index with deferred background semantic embeddings.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_path": { "type": "string", "description": "Absolute path to the project root to index." },
                    "mode": { "type": "string", "enum": ["full", "quick", "upgrade"], "description": "Index mode. 'full' (default) embeds semantics inline, 'quick' builds graph only and upgrades semantics in the background, 'upgrade' fills missing semantics for the active index." }
                },
                "required": ["project_path"]
            }),
            annotations: INDEXING_TOOL,
        },
        ToolDefinition {
            name: "index_now".to_string(),
            title: "Index Project and Wait".to_string(),
            description: Some("Synchronously index the project and return the final stats when complete. Use mode:'quick' to return after graph-only indexing, or 'full' to wait for semantic embeddings.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_path": { "type": "string", "description": "Absolute path to the project root to index." },
                    "mode": { "type": "string", "enum": ["full", "quick", "upgrade"], "description": "Index mode. 'full' (default), 'quick' graph-only, 'upgrade' semantic-only." }
                },
                "required": ["project_path"]
            }),
            annotations: INDEXING_TOOL,
        },
        ToolDefinition {
            name: "find_usages".to_string(),
            title: "Find Usages".to_string(),
            description: Some("Find all nodes that call or reference a given node. Answers 'who calls this function?'.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "Node ID to find usages for (from read_graph or search_code results)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Max usages to return. Defaults to 20." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["node_id"]
            }),
            annotations: READ_ONLY_TOOL,
        },
        ToolDefinition {
            name: "trace_call_chain".to_string(),
            title: "Trace Call Chain".to_string(),
            description: Some("Find the BFS call chain between two nodes. Shows how execution flows from one function to another.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "from_id": { "type": "string", "description": "Starting node ID." },
                    "to_id": { "type": "string", "description": "Target node ID." },
                    "max_depth": { "type": "integer", "minimum": 1, "maximum": 32, "description": "Max hops to search. Defaults to 8." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["from_id", "to_id"]
            }),
            annotations: READ_ONLY_TOOL,
        },
        ToolDefinition {
            name: "impact_of_change".to_string(),
            title: "Impact of Change".to_string(),
            description: Some("Analyze the blast radius of changing a file. Returns all dependents across the codebase. Essential for safe refactoring.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Relative path of the file to analyze (e.g. 'src/engine.rs')." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Max dependents to return. Defaults to 30." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["file"]
            }),
            annotations: READ_ONLY_TOOL,
        },
        ToolDefinition {
            name: "diff_context".to_string(),
            title: "Recently Changed Code".to_string(),
            description: Some("Get graph nodes for recently changed files based on git history. Shows what code has changed in the last N days.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_path": { "type": "string", "description": "Absolute path to the project root (must be a git repo)." },
                    "days": { "type": "integer", "minimum": 1, "maximum": 3650, "description": "Days to look back in git history. Defaults to 7." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Max nodes to return. Defaults to 30." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["project_path"]
            }),
            annotations: READ_ONLY_TOOL,
        },
    ];

    Ok(create_success_response(id, json!({ "tools": tools_list })))
}

async fn handle_call_tool(
    state: &Arc<ServerState>,
    id: Option<Value>,
    params: Option<Value>,
) -> Result<JsonRpcResponse> {
    let request_id = id.as_ref().map(|value| match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    });
    let tool_name = params
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let context = ccm_core::trajectory::TrajectoryContext {
        tool_name,
        request_id,
    };
    ccm_core::trajectory::with_context(context, handle_call_tool_inner(state, id, params)).await
}

async fn handle_call_tool_inner(
    state: &Arc<ServerState>,
    id: Option<Value>,
    params: Option<Value>,
) -> Result<JsonRpcResponse> {
    let Some(params) = params else {
        return Ok(create_error_response(
            id,
            -32602,
            "Missing tools/call params",
        ));
    };
    let Some(tool_name) = params.get("name").and_then(|v| v.as_str()) else {
        return Ok(create_error_response(id, -32602, "Missing tool name"));
    };
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    if let Err(message) = validate_tool_arguments(tool_name, &arguments) {
        return Ok(create_error_response(id, -32602, &message));
    }

    // Extract project_path if present
    let project_path = arguments.get("project_path").and_then(|v| v.as_str());

    if tool_name == "index_project" || tool_name == "index_now" {
        let path = project_path.ok_or_else(|| anyhow::anyhow!("Missing project_path"))?;
        if !state.is_path_allowed(path) {
            return Ok(create_error_response(
                id,
                -32602,
                "Project path is not allowed. Set CCM_ALLOWED_ROOTS (or disable CCM_REQUIRE_ALLOWED_ROOTS).",
            ));
        }

        let result = if tool_name == "index_now" {
            tools::index_now(state.clone(), &arguments).await?
        } else {
            tools::index_project(state.clone(), &arguments).await?
        };
        return Ok(create_success_response(id, json!(result)));
    }

    let project_key = state.project_key(project_path);
    // İlk yükleme izin listesini ve indeksin varlığını doğrular; watcher yalnızca
    // izinli ve indeksi olan projelerde başlar.
    if let Err(error) = state.get_engine(project_path).await {
        return Ok(engine_error_response(id, tool_name, &error));
    }
    if let Some(key) = &project_key {
        state.ensure_auto_refresh(key);
        state.wait_until_fresh(key, FRESHNESS_WAIT_BUDGET).await;
    }
    // Bekleme sırasında yeni generation aktive edilmiş olabilir.
    let loaded = match state.get_engine(project_path).await {
        Ok(loaded) => loaded,
        Err(error) => return Ok(engine_error_response(id, tool_name, &error)),
    };
    // Satır son engine yüklemesinden sonraki durumdan üretilir: bu yükleme yeni bir
    // generation getirdiyse tam karşılaştırma istenmiştir ve satır bayat görünür.
    let freshness = project_key
        .as_deref()
        .map(|key| state.current_freshness(key));
    let engine = loaded.engine.clone();

    let result = match tool_name {
        "get_context" => tools::get_context(&engine, &arguments).await?,
        "search_code" => tools::search_code(&engine, &arguments).await?,
        "find_nodes" => tools::find_nodes(&engine, &arguments).await?,
        "read_graph" => tools::read_graph(&engine, &arguments).await?,
        "find_usages" => tools::find_usages(&engine, &arguments).await?,
        "trace_call_chain" => tools::trace_call_chain(&engine, &arguments).await?,
        "impact_of_change" => tools::impact_of_change(&engine, &arguments).await?,
        "diff_context" => tools::diff_context(&engine, &arguments).await?,
        _ => {
            return Ok(create_error_response(
                id,
                -32602,
                &format!("Unknown tool: {}", tool_name),
            ))
        }
    };

    let result = match freshness {
        Some(freshness) => crate::freshness::with_freshness_line(
            result,
            &crate::freshness::format_freshness_line(
                &freshness,
                loaded.indexed_at(),
                ccm_core::unix_now_secs(),
            ),
        ),
        None => result,
    };

    Ok(create_success_response(id, serde_json::to_value(result)?))
}

/// Engine yüklenemediğinde istemciye dönen JSON-RPC hata yanıtını üretir.
fn engine_error_response(
    id: Option<Value>,
    tool_name: &str,
    error: &anyhow::Error,
) -> JsonRpcResponse {
    tracing::warn!(error = %error, tool = %tool_name, "Failed to load project context");
    let message = if error.to_string().contains("Project index is missing") {
        "Project index is missing. Call index_project first.".to_string()
    } else if error
        .to_string()
        .contains("Project indexing is in progress")
    {
        "Project indexing is in progress. Poll index_project before retrying this tool.".to_string()
    } else if error.to_string().contains("not allowed")
        || error.to_string().contains("No default project root")
    {
        error.to_string()
    } else {
        "Failed to load project context. Check project_path, allowlist, and index state."
            .to_string()
    };
    create_error_response(id, -32603, &message)
}

fn validate_tool_arguments(tool_name: &str, arguments: &Value) -> std::result::Result<(), String> {
    let required_strings: &[&str] = match tool_name {
        "get_context" => &["file"],
        "search_code" | "find_nodes" => &["query"],
        "read_graph" | "find_usages" => &["node_id"],
        "trace_call_chain" => &["from_id", "to_id"],
        "impact_of_change" => &["file"],
        "diff_context" | "index_project" | "index_now" => &["project_path"],
        _ => return Err(format!("Unknown tool: {}", tool_name)),
    };

    for name in required_strings {
        let valid = arguments
            .get(*name)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if !valid {
            return Err(format!(
                "Missing or invalid '{}' argument for {}",
                name, tool_name
            ));
        }
    }

    if tool_name == "get_context"
        && arguments
            .get("line")
            .and_then(Value::as_u64)
            .is_none_or(|line| line == 0)
    {
        return Err("Missing or invalid 'line' argument for get_context".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{negotiate_protocol_version, resolve_startup_project_root};
    use std::path::{Path, PathBuf};

    #[test]
    fn startup_root_skips_filesystem_root_and_home_as_implicit_roots() {
        let home = Path::new("/home/dev");
        let pinned = vec![PathBuf::from("/work/app")];
        let resolve = |allowed: &[PathBuf], launch: &str| {
            resolve_startup_project_root(None, allowed, Some(Path::new(launch)), Some(home))
        };

        // `/` ve ev dizini örtük kök olamaz; tek allowlist girdisi kullanılır.
        assert_eq!(resolve(&pinned, "/"), Some(PathBuf::from("/work/app")));
        assert_eq!(
            resolve(&pinned, "/home/dev"),
            Some(PathBuf::from("/work/app"))
        );
        assert_eq!(resolve(&[], "/"), None);
        // Allowlist içindeki başlatma dizini (alt proje) tercih edilir.
        assert_eq!(
            resolve(&[PathBuf::from("/work")], "/work/app"),
            Some(PathBuf::from("/work/app"))
        );
        // Allowlist dışındaki başlatma dizini varsayılan kök olmaz.
        assert_eq!(
            resolve(&pinned, "/tmp/other"),
            Some(PathBuf::from("/work/app"))
        );
        assert_eq!(
            resolve(&[], "/tmp/other"),
            Some(PathBuf::from("/tmp/other"))
        );
        // Birden çok allowlist girdisinde belirsiz varsayılan seçilmez.
        let many = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        assert_eq!(resolve(&many, "/"), None);
    }
    use serde_json::json;

    #[test]
    fn initialize_prefers_latest_when_client_omits_version() {
        assert_eq!(negotiate_protocol_version(None), "2025-11-25");
    }

    #[test]
    fn initialize_honors_supported_client_version() {
        let params = json!({
            "protocolVersion": "2025-06-18"
        });

        assert_eq!(negotiate_protocol_version(Some(&params)), "2025-06-18");
    }

    #[test]
    fn initialize_falls_back_to_latest_for_unknown_versions() {
        let params = json!({
            "protocolVersion": "2024-11-05"
        });

        assert_eq!(negotiate_protocol_version(Some(&params)), "2025-11-25");
    }
}
