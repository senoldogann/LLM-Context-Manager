//! Süreç içinde çalışan yerleşik embedding modeli (ONNX Runtime, fastembed).
//!
//! Model süreç başına bir kez yüklenir ve tüm vektör depoları tarafından
//! paylaşılır. Dosyalar eksikse ilk kullanımda sabitlenmiş revizyondan
//! indirilir; her dosya yüklenirken sabitlenmiş SHA-256 ile doğrulanır.
//! Çıkarım tokio çalışma iş parçacıklarını meşgul etmemek için
//! `spawn_blocking` içinde, ONNX Runtime'ın kendi iş parçacığı havuzunda
//! yapılır.
//!
//! Varsayılan olarak her ONNX çıkarımı tek metin alır. IBM'in int8 dosyası
//! aktivasyonları çağrı başına, tüm batch tensörü üzerinden dinamik quantize
//! eder: aynı çağrıdaki metinler (ve dolgu) birbirinin vektörünü değiştirir.
//! Tek metinle vektör yalnızca metnin fonksiyonudur; parça yeniden kullanımı ve
//! canlı indeks bu belirlenimciliğe dayanır. `CCM_EMBED_BATCH_SIZE` çıkarım
//! batch'ini açıkça büyütür.

use crate::vector::embedder::embed_batch_size_from_env;
use crate::vector::local_model::{
    download_missing_files, model_dir, read_verified, LocalModelSpec, ModelSource,
    DEFAULT_LOCAL_MODEL,
};
use anyhow::Result;
use fastembed::{
    InitOptionsUserDefined, Pooling, QuantizationMode, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// `CCM_EMBED_BATCH_SIZE` verilmediğinde tek ONNX çıkarımındaki metin sayısı
/// (bkz. modül belgesi).
const DEFAULT_INFERENCE_BATCH: usize = 1;

/// Yüklenmiş yerel embedding modeli.
pub struct LocalEmbedder {
    /// fastembed çıkarımı `&mut` ister; aynı anda tek çıkarım çalışır, ONNX
    /// Runtime onu tüm iş parçacıklarına yayar. Kilit çıkarım başına alınır.
    model: Mutex<TextEmbedding>,
    spec: &'static LocalModelSpec,
    threads: usize,
    /// Tek ONNX çıkarımındaki en fazla metin sayısı.
    inference_batch: usize,
    directory: PathBuf,
}

static SHARED: tokio::sync::OnceCell<Arc<LocalEmbedder>> = tokio::sync::OnceCell::const_new();

/// Süreç genelinde paylaşılan varsayılan yerel modeli döndürür. İlk çağrı
/// eksik dosyaları indirir ve ONNX oturumunu kurar; başarısız bir yükleme
/// önbelleğe alınmaz, sonraki çağrı yeniden dener.
pub async fn shared_local_embedder() -> Result<Arc<LocalEmbedder>> {
    SHARED
        .get_or_try_init(|| load(&DEFAULT_LOCAL_MODEL))
        .await
        .map(Arc::clone)
}

async fn load(spec: &'static LocalModelSpec) -> Result<Arc<LocalEmbedder>> {
    let directory = model_dir(spec)?;
    let source = ModelSource::from_env(spec);
    download_missing_files(&spec.files(), &directory, &source).await?;
    let threads = embedding_threads()?;
    let inference_batch = embed_batch_size_from_env()?.unwrap_or(DEFAULT_INFERENCE_BATCH);
    tokio::task::spawn_blocking(move || {
        LocalEmbedder::load(spec, directory, &source, threads, inference_batch)
    })
    .await
    .map_err(|error| anyhow::anyhow!("Local embedding model loader task failed: {}", error))?
    .map(Arc::new)
}

/// ONNX Runtime iş parçacığı sayısı: `CCM_EMBED_THREADS` ya da fiziksel
/// çekirdek sayısı. Geçersiz değer açık bir hatadır.
fn embedding_threads() -> Result<usize> {
    match std::env::var("CCM_EMBED_THREADS") {
        Ok(value) => value
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|threads| *threads > 0)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "CCM_EMBED_THREADS must be a positive integer, got '{}'",
                    value
                )
            }),
        Err(std::env::VarError::NotPresent) => Ok(num_cpus::get_physical().max(1)),
        Err(error) => Err(anyhow::anyhow!("CCM_EMBED_THREADS is not valid: {}", error)),
    }
}

impl LocalEmbedder {
    /// Dosyaları okuyup doğrular ve ONNX oturumunu kurar (bloklar).
    fn load(
        spec: &'static LocalModelSpec,
        directory: PathBuf,
        source: &ModelSource,
        threads: usize,
        inference_batch: usize,
    ) -> Result<Self> {
        let started = std::time::Instant::now();
        let read = |file| read_verified(&directory, file, source);
        let model = UserDefinedEmbeddingModel::new(
            read(&spec.onnx)?,
            TokenizerFiles {
                tokenizer_file: read(&spec.tokenizer)?,
                config_file: read(&spec.config)?,
                special_tokens_map_file: read(&spec.special_tokens_map)?,
                tokenizer_config_file: read(&spec.tokenizer_config)?,
            },
        )
        .with_pooling(Pooling::Cls)
        // IBM'in int8 dosyası aktivasyonları çalışma anında (dinamik) quantize
        // eder; fastembed bu modda çağrıya verilen metinleri tek batch olarak
        // çalıştırır. Çağrı başına metin sayısını `embed_blocking` belirler.
        .with_quantization(QuantizationMode::Dynamic);
        let options = InitOptionsUserDefined::new()
            .with_max_length(spec.max_tokens)
            .with_intra_threads(threads);
        let embedding =
            TextEmbedding::try_new_from_user_defined(model, options).map_err(|error| {
                anyhow::anyhow!(
                    "Local embedding model {}@{} could not be loaded from '{}': {}",
                    spec.repo,
                    spec.revision,
                    directory.display(),
                    error
                )
            })?;
        tracing::info!(
            model = spec.repo,
            revision = spec.revision,
            directory = %directory.display(),
            threads,
            inference_batch,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Loaded local embedding model"
        );
        Ok(Self {
            model: Mutex::new(embedding),
            spec,
            threads,
            inference_batch,
            directory,
        })
    }

    /// Metinleri giriş sırasıyla embed eder; her ONNX çıkarımı en fazla
    /// `inference_batch` metin alır (varsayılan tek metin).
    pub async fn embed(self: Arc<Self>, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        tokio::task::spawn_blocking(move || self.embed_blocking(&texts))
            .await
            .map_err(|error| anyhow::anyhow!("Local embedding task failed: {}", error))?
    }

    fn embed_blocking(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(self.inference_batch) {
            vectors.extend(self.run_inference(batch)?);
        }
        Ok(vectors)
    }

    /// Tek ONNX çıkarımı: `batch` tek tensör olarak çalışır. Kilit yalnızca bu
    /// çıkarım süresince tutulur; eşzamanlı sorgu embedding'i uzun bir indeksleme
    /// çağrısının bitmesini beklemez.
    fn run_inference(&self, batch: &[String]) -> Result<Vec<Vec<f32>>> {
        let vectors = {
            let mut model = self
                .model
                .lock()
                .map_err(|_| anyhow::anyhow!("Local embedding model lock is poisoned"))?;
            model.embed(batch, None).map_err(|error| {
                anyhow::anyhow!(
                    "Local embedding model failed on a batch of {} text(s): {}",
                    batch.len(),
                    error
                )
            })?
        };
        if vectors.len() != batch.len() {
            anyhow::bail!(
                "Local embedding model returned {} vectors for {} texts",
                vectors.len(),
                batch.len()
            );
        }
        if let Some(vector) = vectors.iter().find(|vector| vector.len() != self.spec.dim) {
            anyhow::bail!(
                "Local embedding model returned a {}-d vector; {} is pinned to {}-d",
                vector.len(),
                self.spec.repo,
                self.spec.dim
            );
        }
        Ok(vectors)
    }

    /// Tanılama çıktısı için kısa özet.
    pub fn describe(&self) -> String {
        format!(
            "local model '{}' @ {} ({}-d, {} threads, {} text(s) per inference) from {}",
            self.spec.repo,
            &self.spec.revision[..12],
            self.spec.dim,
            self.threads,
            self.inference_batch,
            self.directory.display()
        )
    }

    /// Model dosyalarının dizini.
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

/// Gerçek modeli kullanan testler: ~120 MB indirir, bu yüzden varsayılan
/// koşuda atlanır. `CCM_TEST_LOCAL_MODEL=1 cargo test -p ccm-core -- --ignored local_model`
/// ile çalıştırılır.
#[cfg(test)]
mod tests {
    use super::shared_local_embedder;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    fn opted_in() -> bool {
        std::env::var("CCM_TEST_LOCAL_MODEL").as_deref() == Ok("1")
    }

    /// Model kartındaki (fp32) sorgu/pasaj benzerlik matrisi. CLS pooling ve
    /// L2 normalizasyonu doğruysa int8 modelin matrisi aynı sırayı korur.
    #[tokio::test]
    #[ignore = "downloads the ~120 MB local model; set CCM_TEST_LOCAL_MODEL=1"]
    async fn local_model_reproduces_the_model_card_similarities() -> anyhow::Result<()> {
        if !opted_in() {
            return Ok(());
        }
        let embedder = shared_local_embedder().await?;
        let queries = vec![
            "What is the tallest mountain in Japan?".to_string(),
            "Wer hat das Lied Achy Breaky Heart geschrieben?".to_string(),
            "ドイツの首都はどこですか？".to_string(),
        ];
        let passages = vec![
            "富士山は、静岡県と山梨県にまたがる活火山で、標高3776.12 mで日本最高峰の独立峰である。".to_string(),
            "Achy Breaky Heart is a country song written by Don Von Tress. Originally titled Don't Tell My Heart and performed by The Marcy Brothers in 1991.".to_string(),
            "Berlin ist die Hauptstadt und ein Land der Bundesrepublik Deutschland. Die Stadt ist with rund 3,7 Millionen Einwohnern die bevölkerungsreichste Kommune Deutschlands.".to_string(),
        ];
        let card = [
            [0.8869, 0.6658, 0.7213],
            [0.6792, 0.9577, 0.6420],
            [0.7534, 0.6771, 0.9112],
        ];
        let query_vectors = embedder.clone().embed(queries).await?;
        let passage_vectors = embedder.clone().embed(passages).await?;
        for (row, query) in query_vectors.iter().enumerate() {
            let norm: f32 = query.iter().map(|value| value * value).sum();
            assert!((norm - 1.0).abs() < 1e-4, "vectors must be L2-normalized");
            let similarities: Vec<f32> = passage_vectors
                .iter()
                .map(|passage| cosine(query, passage))
                .collect();
            for (column, similarity) in similarities.iter().enumerate() {
                assert!(
                    (similarity - card[row][column]).abs() < 0.1,
                    "similarity[{row}][{column}] = {similarity}, model card {}",
                    card[row][column]
                );
            }
            let best = similarities
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, _)| index);
            assert_eq!(
                best,
                Some(row),
                "each query must rank its own passage first"
            );
        }
        Ok(())
    }

    /// Varsayılan ayarla (`CCM_EMBED_BATCH_SIZE` tanımsız) vektör yalnızca metnin
    /// fonksiyonudur: aynı metin tek başına ve farklı uzunluktaki metinlerle aynı
    /// çağrıda embed edildiğinde birebir aynı vektörü verir. Parça yeniden
    /// kullanımı ve canlı indeks bu belirlenimciliğe dayanır.
    #[tokio::test]
    #[ignore = "downloads the ~120 MB local model; set CCM_TEST_LOCAL_MODEL=1"]
    async fn local_model_vectors_do_not_depend_on_batch_mates() -> anyhow::Result<()> {
        if !opted_in() {
            return Ok(());
        }
        assert!(
            std::env::var_os("CCM_EMBED_BATCH_SIZE").is_none(),
            "this test checks the default inference batch; unset CCM_EMBED_BATCH_SIZE"
        );
        let embedder = shared_local_embedder().await?;
        let target = "pub fn compute_invoice_tax(amount: f64, rate: f64) -> f64 { amount * rate }"
            .to_string();
        let texts = vec![
            "fn noop() {}".to_string(),
            target.clone(),
            "/// Opens a TCP connection and retries with a fixed delay until the deadline passes.\n\
             pub fn connect_with_retry(host: &str, port: u16, deadline: std::time::Instant) -> std::io::Result<std::net::TcpStream> {\n\
                 loop {\n\
                     match std::net::TcpStream::connect((host, port)) {\n\
                         Ok(stream) => return Ok(stream),\n\
                         Err(_) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),\n\
                         Err(error) => return Err(error),\n\
                     }\n\
                 }\n\
             }"
            .to_string(),
        ];
        let alone = embedder.clone().embed(vec![target]).await?;
        let together = embedder.clone().embed(texts).await?;
        assert_eq!(
            alone[0], together[1],
            "a chunk's vector must not depend on the texts embedded with it"
        );
        Ok(())
    }

    /// fp32 ONNX modeliyle (onnxruntime 1.24, CLS, L2) üretilmiş referans
    /// vektörlere kosinüs yakınlığı: int8 quantizasyonun bozulma payını sınırlar.
    #[tokio::test]
    #[ignore = "downloads the ~120 MB local model; set CCM_TEST_LOCAL_MODEL=1"]
    async fn local_model_stays_close_to_fp32_reference_vectors() -> anyhow::Result<()> {
        if !opted_in() {
            return Ok(());
        }
        #[derive(serde::Deserialize)]
        struct Reference {
            texts: Vec<String>,
            vectors: Vec<Vec<f32>>,
        }
        let reference: Reference = serde_json::from_str(include_str!(
            "../../tests/fixtures/granite_fp32_reference.json"
        ))?;
        let embedder = shared_local_embedder().await?;
        for (text, expected) in reference.texts.iter().zip(&reference.vectors) {
            // Tek başına embed: dinamik quantizasyon batch'teki diğer metinlerden etkilenmez.
            let actual = embedder.clone().embed(vec![text.clone()]).await?;
            let similarity = cosine(&actual[0], expected);
            assert!(
                similarity > 0.93,
                "cosine to the fp32 reference is {similarity} for {text:?}"
            );
        }
        Ok(())
    }
}
