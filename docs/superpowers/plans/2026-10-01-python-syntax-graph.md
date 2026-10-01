# Python Sözdizimi Grafı (M1) Uygulama Planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Python projelerinde "bunu kim çağırıyor / bunu değiştirirsem ne bozulur" cevabını sözcük eşleştirmesinden sözdizimi ağacına ve import çözümüne taşımak, her kenara kesinlik etiketi vermek ve `find_usages`'ı dürüst yapmak.

**Architecture:** Çıkarıcı (tree-sitter yürüyüşü) Python düğümleri için çağrı yerlerini, import bağlarını ve taban sınıfları `CodeNode.facts` alanına yazar. Graf düzeyindeki çözümleyici bunları kapsam kurallarıyla `Calls` / `CallInferred` / `CallAmbiguous` / `Imports` / `Inherits` kenarlarına çevirir. Diğer 12 dil `ReferenceFacts::Lexical` ile bugünkü sözcüksel yoldan değişmeden geçer. Olgular graf JSON'uyla saklanır; indeks şeması 4 → 5.

**Tech Stack:** Rust 2021 (`ccm-core`, `ccm-mcp`), tree-sitter (mevcut Python grameri), petgraph, serde, tokio testleri; `benchmarks/freshness` için Python 3 + uv.

**Spec:** Bu belgedeki "Yol haritası" ve "Tasarım kararları" bölümleri. Bağlam: kullanıcının dört hedefi — az token, tek soruda doğru cevap (ajan dosyalarda kaybolmaz, değişikliğin etkisini bilir), kolay MCP, gerçek veriyle kanıt.

---

## Yol haritası (bu plan M1)

| Kilometre taşı | Ne teslim eder | Kabul ölçütü |
|---|---|---|
| **M1 — Python sözdizimi grafı (bu plan)** | Sözdiziminden çağrılar, import/yeniden dışa aktarma/`self`/`super()` çözümü, kesinlik etiketli kenarlar, dürüst `find_usages`, sabitlenmiş araç zinciri | Altın fikstür testleri; Python dışı davranış değişmez |
| M2 — Tek çağrıda cevap | 10 araç yerine görev odaklı 4–5 araç (`explain`, `impact`, `locate`, `status`), `max_tokens` bütçesi, kısa göreli çıktı; tazelik: ham olay gelir gelmez `stale · pending`; sözdizimi hatalı dosyada `refresh failed` etiketi; `learn`/`optimize`/policy araştırma yüzeyini park etme | LLM'siz sabit soru setinde çağrı sayısı ve cevap baytı; L1'in S1/S7'sinde etiketsiz bayat cevap = 0 |
| M3 — TypeScript/JavaScript ve Rust | Aynı olgu/çözümleyici çerçevesi: `import`/`require`/`use`, `this.`/`self.`, impl metotları; Python'da yerel alıcı türü (`x = K(); x.m()`) | Dil başına altın fikstür |
| M4 — Kanıt ve sürüm | L2: pyright/tsserver/rust-analyzer'a karşı precision/recall (≥200 düğüm, ≥30 hakemli uyuşmazlık); L3 ajan A/B pilotu (kapı G3, ~100 $); README iddiaları yalnız sonuçlardan; v0.4.0 | H2: en az iki dilde precision ≥%90, recall ≥%70 |

`docs/` altındaki üç izlenmeyen strateji belgesi (`agent-brief.md`, `product-assessment.md`, `productization-plan.md`) bu planın kapsamı dışındadır; dokunulmaz.

## Tasarım kararları

1. **Referans = sözdizimi düğümü.** Python `call` düğümleri, `import`/`from … import` ifadeleri ve `class X(Base)` tabanları çıkarılır. Yorum ve string içerikleri ağaçta `call` düğümü üretmediği için asla kenar üretmez.
2. **Sahiplik.** Bir çağrı, onu içeren en yakın fonksiyona aittir. Sınıf gövdesindeki çağrılar sınıfa, modül düzeyindekiler `File` düğümüne aittir. Dekoratörler süsledikleri tanıma aittir. Atama ve import düğümleri (`Variable`, `Import`) artık kenar üretmez; böylece bugünkü "fonksiyon + içindeki değişken" çift kenarı ortadan kalkar.
3. **Çözüm sırası** (kaynak `S`, dosyası `F`):
   - `ad()`: `F`'de sınıf üyesi olmayan aynı adlı tanım → import bağı (paket `__init__.py` yeniden dışa aktarması, ≤3 adım) → yıldız import → yerleşik ad (kenar yok) → projede tek Python tanımı (`CallInferred`).
   - `self.ad()` ve `cls.ad()`: kapsayan sınıf ve tabanları (≤3 adım) → olası.
   - `super().ad()`: tabanlar → olası.
   - `m.ad()` ve `a.b.ad()`: baş bileşen import bağıysa modülün ya da sınıfın üyesi aranır; projede bulunamazsa proje dışıdır ve kenar üretmez. Baş bileşen aynı dosyadaki bir sınıfsa o sınıfın üyesi aranır. Hiçbiri değilse olası.
   - `f().ad()` gibi alıcısı ifade olan çağrılar: olası.
4. **Kesinlik → kenar:** tek kesin hedef `Calls`; birden çok kesin hedef ya da olası hedefler `CallAmbiguous`; import edilmemiş tek proje tanımı `CallInferred`. Import bağları `Imports` (birden çok hedefte `ImportAmbiguous`), çözülen tabanlar `Inherits`. Kendine kenar yok. Aynı hedefe birden çok çağrıda en güçlü tür kalır (`Calls` > `CallInferred` > `CallAmbiguous`).
5. **Modül yolu:** `app/core.py` → `app.core`, `app/__init__.py` → `app`. Kök göreli tam eşleşme varsa yalnız o kullanılır; yoksa yolu sonek olarak taşıyan dosyalar kullanılır (`src/` düzeni).
6. **Kapsam dışı (M3'e):** yerel alıcı türü çıkarımı, `__all__`, dinamik import, tip ipucu referansları, parametre varsayılanlarındaki çağrılar.

## Global Constraints

- Cargo komutlarını rustup vekiliyle çalıştır: `~/.cargo/bin/cargo`. PATH'te önde olan Homebrew `cargo` 1.94'tür ve CI'ın (1.99) lint'lerini kaçırır. Task 0'dan sonra `rust-toolchain.toml` 1.99.0'ı seçer.
- Her task'ın sonunda üç komut geçmeli:
  - `~/.cargo/bin/cargo fmt --all -- --check`
  - `~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings`
  - task'ın kendi testi
- Python dışındaki 12 dilin davranışı değişmez; düğümleri `ReferenceFacts::Lexical` taşır.
- Kod yazım kuralları:
  - Yorumlar Türkçe.
  - Üretim kodunda `unwrap`/`expect` yok.
  - Hatalar açık türlerle verilir; sessiz yedek yol (fallback) yok.
  - Saf fonksiyonlar tercih edilir.
- Yeni birim test yazılmaz. Davranış `core/tests/python_references_test.rs` entegrasyon testinde doğrulanır. Tek istisna Task 1'deki serde gidiş-dönüş testidir (saf veri dönüşümü).
- Sabitler:
  - `MAX_POSSIBLE_TARGETS = 5`
  - `MAX_REEXPORT_DEPTH = 3`
  - `MAX_BASE_DEPTH = 3`
  - `INDEX_SCHEMA_VERSION = 5`
- Push, PR açma ve merge yalnız kullanıcının açık onayıyla yapılır (kapı G4).
- Commit mesajları Conventional Commits biçiminde ve İngilizce olur; son satır `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- README'ye ölçülmemiş sayı ya da iddia girmez; yalnız davranış tanımı.

## Review Focus

1. **Sözdizimi hatalı Python dosyası projede** → indeksleme sürer, diğer kenarlar doğru kalır. Task 3'teki fikstürde `app/broken.py` var.
2. **Aynı modül yolu iki yerde** (`app/util.py`, `tests/app/util.py`) → kök göreli tam eşleşme kazanır; `src/` düzeni sonekle çözülür. Task 3, test 2.
3. **Çok sınıfta ortak metot adı** (`get` ×6) → `x.get()` hiç kenar üretmez. Task 3, test 3.
4. **Diskte şema-4 indeks** → yeni sunucu indeksi yeniden kurar, hata vermez. Task 1, Step 6.
5. **Python dışı dil aynı kalır** → fikstürdeki `lib.rs` (`foo → bar` `Calls`) ve mevcut `incremental_calls_test`. Task 3.

## Dosya yapısı

| Dosya | Sorumluluk |
|---|---|
| `core/src/graph/references.rs` (yeni) | Olgu türleri + Python yol kuralları (`python_package`, `python_module_path`) |
| `core/src/graph/resolve.rs` (yeni) | Python çözümleyici, `PythonModules` |
| `core/src/graph/usages.rs` (yeni) | `usages_of`, `UsageRelation`, `UsageReport`, `UsageError` |
| `core/src/vector/python_facts.rs` (yeni) | Python sözdizimi yürüyüşü → `SyntaxFacts` |
| `core/tests/python_references_test.rs` (yeni) | Fikstür ve tüm M1 davranış testleri |
| `core/src/graph/mod.rs` | `CodeNode.facts`, `EdgeType::CallInferred`, kaynak seçimi, dispatch, artımlı ad kümesi |
| `core/src/vector/extractor.rs`, `core/src/vector/mod.rs` | Olguları düğüme yazma, modül bildirimi |
| `core/src/lib.rs:32` | Şema 5 |
| `core/src/engine.rs` (`find_usages`), `mcp/src/tools.rs` | Dürüst kullanım cevabı |
| `README.md`, `README.tr.md`, `SKILL.md` | Sınırlar paragrafı, araç açıklaması |

---

### Task 0: Entegrasyon ve temizlik

**Files:**
- Modify: `mcp/src/freshness.rs` (`inject_targeted_refresh_failure`; `feat/local-embedder`'da ~856. satır)
- Create: `rust-toolchain.toml`
- Modify: `.github/workflows/*.yml` (her `dtolnay/rust-toolchain` adımı)
- Modify: `benchmarks/freshness/PREREGISTRATION.md` (Deviations sonu)
- Commit: bu plan dosyası

**Interfaces:**
- Consumes: —
- Produces: `feat/syntax-graph` dalı (`main` + PR #7 + L1 benchmark), yerelde ve CI'da aynı araç zinciri

- [ ] **Step 1: PR #7'nin Clippy hatasını düzelt.** Rust 1.99, `AtomicUsize::fetch_update`'i kullanımdan kaldırdı ve CI'daki `Clippy Linting` bu yüzden kırık. Ana klasörde:

```bash
git switch feat/local-embedder
```

`mcp/src/freshness.rs` içindeki `inject_targeted_refresh_failure`'da şu bloğu:

```rust
    let injected = remaining
        .fetch_update(
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
            |count| count.checked_sub(1),
        )
        .is_ok();
```

her Rust sürümünde uyarısız derlenen döngüyle değiştir:

```rust
    // `fetch_update` Rust 1.99'da kullanımdan kaldırıldı; aynı atomik azaltma
    // her sürümde uyarısız derlenen bir karşılaştır-değiştir döngüsüdür.
    let injected = loop {
        let current = remaining.load(std::sync::atomic::Ordering::SeqCst);
        let Some(next) = current.checked_sub(1) else {
            break false;
        };
        if remaining
            .compare_exchange(
                current,
                next,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
        {
            break true;
        }
    };
```

- [ ] **Step 2: CI'ın araç zinciriyle doğrula.**

```bash
~/.cargo/bin/rustup toolchain install 1.99.0 --profile minimal -c clippy -c rustfmt
~/.cargo/bin/cargo +1.99.0 clippy --workspace --all-targets -- -D warnings
~/.cargo/bin/cargo +1.99.0 test -p ccm-mcp
```

Beklenen: clippy temiz, testler geçer.

- [ ] **Step 3: Commit.**

```bash
git add mcp/src/freshness.rs
git commit -m "fix(mcp): replace the deprecated fetch_update in the refresh test hook" -m "Rust 1.99 deprecates AtomicUsize::fetch_update, so clippy -D warnings fails in CI. A compare_exchange loop performs the same decrement on every toolchain." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 4 (KAPI G4 — kullanıcı onayı şart):** `git push origin feat/local-embedder` komutunu çalıştır ve `gh pr checks 7` ile CI'ı bir kez kontrol et. Yeşilse `gh pr merge 7 --squash` ile birleştir. Onay yoksa Step 5'te dalı `main` yerine `feat/local-embedder`'dan aç.

- [ ] **Step 5: Çalışma dalını kur ve L1 benchmark'ını al.** Rakip taslakları hariç tutulur; bunlar arşiv dalı `bench/ccm-bench`'te `b78f708`–`50d2f7e` aralığında kalır.

```bash
git switch main && git pull --ff-only
git switch -c feat/syntax-graph
git merge --no-ff 5125c0b -m "Merge the L1 freshness benchmark (CCM results)"
git cherry-pick c160040 00ef3d7
```

Çakışma çıkarsa (olası dosyalar `README.md` ve `benchmarks/README.md`) iki tarafın da değişikliği korunur. PR #7'nin "Rust-Powered" satırı ve L1 bölümü birlikte kalır.

- [ ] **Step 6: Ön-kayda notu ekle.** `benchmarks/freshness/PREREGISTRATION.md` dosyasındaki Deviations listesinin sonuna:

```markdown
2. **Competitor runs not performed (2026-10-01).** The project moved from the
   comparison to product work before any competitor was measured. Draft
   adapters, smoke runs and their notes are archived on branch
   `bench/ccm-bench` (commits `b78f708`–`50d2f7e`) and are not part of this
   harness. H1 is not measurable; the published CCM results above are unchanged.
```

- [ ] **Step 7: Araç zincirini sabitle.** `rust-toolchain.toml` oluştur:

```toml
[toolchain]
channel = "1.99.0"
components = ["clippy", "rustfmt"]
```

`.github/workflows/*.yml` içindeki her `uses: dtolnay/rust-toolchain@4cda84d5c5c54efe2404f9d843567869ab1699d4 # stable` adımına `toolchain: "1.99.0"` girdisini ekle. Adımda zaten bir `with:` bloğu varsa (ör. `components:`) girdiyi o bloğa ekle; yoksa yeni bir `with:` aç:

```yaml
        uses: dtolnay/rust-toolchain@4cda84d5c5c54efe2404f9d843567869ab1699d4 # stable
        with:
          toolchain: "1.99.0"
```

Doğrula:

```bash
rg -n "dtolnay/rust-toolchain" -A3 .github/workflows
```

Beklenen: her eşleşmenin altında `toolchain: "1.99.0"`.

- [ ] **Step 8: Commit.**

```bash
git add benchmarks/freshness/PREREGISTRATION.md rust-toolchain.toml .github/workflows docs/superpowers/plans/2026-10-01-python-syntax-graph.md
git commit -m "chore: pin Rust 1.99.0, record parked competitor runs, add the M1 plan" -m "A floating stable toolchain broke CI when 1.99 added a deprecation; local Homebrew 1.94 could not see it. The L1 pre-registration records that no competitor was measured." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 1: Olgu modeli, `CallInferred`, şema 5

**Files:**
- Create: `core/src/graph/references.rs`
- Modify: `core/src/graph/mod.rs` (modül bildirimleri, `CodeNode`, `EdgeType`, `is_reference_edge`, test modülü)
- Modify: tüm `CodeNode { … }` değişmezleri (derleyici listeler: `graph/mod.rs` ×29, `engine.rs` ×7, `eval.rs` ×4, `vector/extractor.rs` ×2, `engine/hybrid.rs` ×2, `lib.rs` ×1, `core/tests/incremental_filesystem_test.rs` ×1)
- Modify: `core/src/lib.rs:32`
- Test: `core/src/graph/mod.rs` test modülü

**Interfaces:**
- Produces:
  - `ccm_core::graph::{ReferenceFacts, SyntaxFacts, SyntaxLanguage, CallSite, CallTarget, ImportBinding, python_package, python_module_path}`
  - `CodeNode.facts: ReferenceFacts`
  - `EdgeType::CallInferred`
  - `SyntaxFacts::{empty, has_references, mentions_any}`
  - `CallTarget::name`

- [ ] **Step 1: Başarısız testi yaz.** `core/src/graph/mod.rs` içindeki `#[cfg(test)] mod tests` modülünün sonuna:

```rust
    #[test]
    fn syntax_facts_survive_a_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("graph.json");
        let facts = SyntaxFacts {
            language: SyntaxLanguage::Python,
            calls: vec![CallSite {
                target: CallTarget::SelfMember("stop".to_string()),
                line: 7,
            }],
            imports: vec![ImportBinding {
                local: "helper".to_string(),
                module: "app.util".to_string(),
                symbol: Some("helper".to_string()),
            }],
            bases: Vec::new(),
        };
        let mut graph = CodeGraph::new();
        graph.add_node(CodeNode {
            id: "app/core.py:function_definition:symbol:0000000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "start".to_string(),
            content: "def start(self):\n    self.stop()\n".into(),
            start_line: 6,
            end_line: 7,
            facts: ReferenceFacts::Syntax(facts.clone()),
        });
        graph.save_to_file(&path.to_string_lossy()).expect("save");
        let loaded = CodeGraph::load_from_file(&path.to_string_lossy()).expect("load");
        let node = loaded.graph.node_weights().next().expect("node");
        assert_eq!(node.facts, ReferenceFacts::Syntax(facts));
    }
```

- [ ] **Step 2: Derlenmediğini gör.**

Run: `~/.cargo/bin/cargo test -p ccm-core --lib syntax_facts_survive_a_save_and_load`

Expected: FAIL. Derleme hatası: `SyntaxFacts`, `ReferenceFacts` ve `facts` alanı bulunamıyor.

- [ ] **Step 3: `core/src/graph/references.rs` dosyasını oluştur.**

```rust
//! Sözdiziminden çıkarılan referans olguları ve Python yol kuralları.
//!
//! Çıkarıcı olguları düğüme yazar, çözümleyici (`resolve.rs`) kenarlara çevirir;
//! olgular graf JSON'uyla saklanır.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Olguların hangi dilin kurallarıyla çözüleceği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyntaxLanguage {
    Python,
}

/// Çağrılan ya da miras alınan ifadenin kaynakta yazıldığı biçim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallTarget {
    /// `ad(...)`
    Bare(String),
    /// `self.ad(...)` ya da `cls.ad(...)`
    SelfMember(String),
    /// `super().ad(...)`
    SuperMember(String),
    /// `a.b.ad(...)`: niteleyici yalnız tanımlayıcılardan oluşan noktalı yoldur.
    Member { qualifier: String, name: String },
    /// Alıcısı bir ifade olan çağrı (`f().ad()`, `x[0].ad()`); alıcının türü bilinmez.
    Chained(String),
}

impl CallTarget {
    /// Çağrılan adın kendisi.
    pub fn name(&self) -> &str {
        match self {
            CallTarget::Bare(name)
            | CallTarget::SelfMember(name)
            | CallTarget::SuperMember(name)
            | CallTarget::Chained(name)
            | CallTarget::Member { name, .. } => name,
        }
    }
}

/// Kaynaktaki bir çağrı ve 1 tabanlı satırı.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallSite {
    pub target: CallTarget,
    pub line: usize,
}

/// Bir `import` ifadesinin kapsamda kurduğu ad bağı.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImportBinding {
    /// Kapsamda bağlanan ad (`from a import b as c` → `c`, `import a.b` → `a`, `*`).
    pub local: String,
    /// Mutlak noktalı modül yolu; göreli importlar dosyanın paketine göre çözülmüştür.
    pub module: String,
    /// İçe aktarılan sembol; modül importunda `None`, yıldız importta `Some("*")`.
    pub symbol: Option<String>,
}

/// Bir düğümün sözdiziminden toplanan referansları.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxFacts {
    pub language: SyntaxLanguage,
    pub calls: Vec<CallSite>,
    pub imports: Vec<ImportBinding>,
    /// Sınıf düğümünde taban sınıf ifadeleri.
    pub bases: Vec<CallTarget>,
}

impl SyntaxFacts {
    /// Hiç referansı olmayan olgular.
    pub fn empty(language: SyntaxLanguage) -> Self {
        Self {
            language,
            calls: Vec::new(),
            imports: Vec::new(),
            bases: Vec::new(),
        }
    }

    /// Düğüm kenar üretebilir mi?
    pub fn has_references(&self) -> bool {
        !(self.calls.is_empty() && self.imports.is_empty() && self.bases.is_empty())
    }

    /// Olgular adlardan birini anıyor mu? Artımlı yenileme etkilenen kaynakları
    /// bununla seçer: çağrı adları, niteleyici bileşenleri, bağlar ve tabanlar.
    pub fn mentions_any(&self, names: &HashSet<String>) -> bool {
        let target_mentions = |target: &CallTarget| {
            names.contains(target.name())
                || matches!(target, CallTarget::Member { qualifier, .. }
                    if qualifier.split('.').any(|part| names.contains(part)))
        };
        self.calls.iter().any(|call| target_mentions(&call.target))
            || self.bases.iter().any(target_mentions)
            || self.imports.iter().any(|binding| {
                names.contains(&binding.local)
                    || binding
                        .symbol
                        .as_ref()
                        .is_some_and(|symbol| names.contains(symbol))
            })
    }
}

/// Bir düğümün referanslarının kaynağı.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceFacts {
    /// Dil için sözdizimi çıkarıcısı yok: düğüm içeriği sözcüksel taranır.
    #[default]
    Lexical,
    /// Referanslar sözdiziminden çıkarıldı; içerik taranmaz.
    Syntax(SyntaxFacts),
}

/// Python dosyasının paketi, kök göreli yol bileşenleri olarak
/// (`app/core.py` → `[app]`, `app/__init__.py` → `[app]`).
pub fn python_package(file_id: &str) -> Vec<String> {
    let mut parts: Vec<String> = file_id.split(['/', '\\']).map(str::to_string).collect();
    parts.pop();
    parts
}

/// Python dosyasının modül yolu bileşenleri (`app/core.py` → `[app, core]`,
/// `app/__init__.py` → `[app]`); `.py` dosyası değilse `None`.
pub fn python_module_path(file_id: &str) -> Option<Vec<String>> {
    let stem = file_id.strip_suffix(".py")?;
    let mut parts: Vec<String> = stem.split(['/', '\\']).map(str::to_string).collect();
    if parts.last().is_some_and(|last| last == "__init__") {
        parts.pop();
    }
    (!parts.is_empty()).then_some(parts)
}
```

- [ ] **Step 4: Grafa bağla.** `core/src/graph/mod.rs` dosyasında:

1. `use` satırlarının altına şunları ekle:

```rust
pub mod references;

pub use references::{
    python_module_path, python_package, CallSite, CallTarget, ImportBinding, ReferenceFacts,
    SyntaxFacts, SyntaxLanguage,
};
```

2. `CodeNode` içinde `pub end_line: usize,` satırının altına şunu ekle:

```rust
    /// Referans olguları; eski indekslerde alan yoktur ve sözcüksel sayılır.
    #[serde(default)]
    pub facts: ReferenceFacts,
```

3. `EdgeType` içinde `CallAmbiguous,` satırının altına şunu ekle:

```rust
    /// İmport ya da yerel tanımla çözülemeyen, projede tek tanımı olan ada bağlanan
    /// çağrı (ör. yıldız import). Kesin değildir.
    CallInferred,
```

4. `is_reference_edge` içindeki desene `| EdgeType::CallInferred` ekle.

5. `~/.cargo/bin/cargo check --workspace --all-targets` çalıştır. Raporlanan her `CodeNode { … }` değişmezine `facts: ReferenceFacts::Lexical,` alanını ekle. `crate::graph` dışındaki dosyalarda gerekli importu da ekle: `use ccm_core::graph::ReferenceFacts;` ya da `use crate::graph::ReferenceFacts;`.

6. Derleyicinin raporladığı her eksik `EdgeType` kolunda `CallInferred`'ı `Calls` ile aynı kola koy.

7. Sonra şu aramayı çalıştır. `CallAmbiguous`'ın çağrı kenarı olarak sayıldığı her `matches!` ve `|` desenine `EdgeType::CallInferred`'ı da ekle (ör. impact, trace ve hibrit sıralamadaki genişletme):

```bash
rg -n "EdgeType::CallAmbiguous" core mcp cli
```

- [ ] **Step 5: Şemayı yükselt.** `core/src/lib.rs:32`:

```rust
pub const INDEX_SCHEMA_VERSION: u32 = 5;
```

Sonra şu aramayı çalıştır. Testlerde şema 4'ü sabit yazan her yeri `INDEX_SCHEMA_VERSION` sabitine çevir:

```bash
rg -n "schema_version: 4|SCHEMA_VERSION, 4|== 4" core mcp cli --type rust
```

- [ ] **Step 6: Doğrula.**

```bash
~/.cargo/bin/cargo test -p ccm-core --lib syntax_facts_survive_a_save_and_load
~/.cargo/bin/cargo test --workspace
```

Expected: ikisi de PASS.

Şema-4 indeksi elle de dene (Review Focus 4):

```bash
D=$(mktemp -d) && printf 'def a():\n    return b()\n\n\ndef b():\n    return 1\n' > $D/m.py
CCM_DISABLE_EMBEDDER=1 ~/.cargo/bin/ccm-cli index --path $D
~/.cargo/bin/cargo build --release -p ccm-cli
CCM_DISABLE_EMBEDDER=1 target/release/ccm-cli index --path $D
```

Expected: ikinci indeksleme hatasız biter ve şema farkı nedeniyle yeniden kurulduğunu bildirir. Bunun yerine `SchemaMismatch` hatası çıkarsa dur ve kullanıcıya bildir.

- [ ] **Step 7: fmt, clippy ve commit.**

```bash
~/.cargo/bin/cargo fmt --all && ~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
git add -A core mcp cli
git commit -m "feat(graph): carry syntax reference facts on nodes and add inferred call edges" -m "Python resolution needs call sites, import bindings and bases per node, persisted with the graph. Other languages keep lexical facts. The index schema moves to 5 so old graphs are rebuilt." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Python olgu çıkarımı

**Files:**
- Create: `core/src/vector/python_facts.rs`
- Modify: `core/src/vector/mod.rs`, `core/src/vector/extractor.rs` (`extract`, `walk_node`)
- Test: `core/tests/python_references_test.rs` (yeni; Task 3–5 de bu dosyaya ekler)

**Interfaces:**
- Consumes: Task 1'in türleri
- Produces: Python `function_definition` / `class_definition` / `File` düğümlerinde `ReferenceFacts::Syntax`; Python `Variable` / `Import` düğümlerinde `Syntax(SyntaxFacts::empty(Python))`

- [ ] **Step 1: Fikstürü ve başarısız testi yaz.** `core/tests/python_references_test.rs`:

```rust
//! M1: Python referanslarının sözdiziminden çıkarılması ve çözümü.

use anyhow::Result;
use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::{CallTarget, CodeGraph, EdgeType, ImportBinding, ReferenceFacts};
use ccm_core::vector::store::LanceDbStore;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::{tempdir, TempDir};
use tokio::sync::RwLock;

const CLI: &str = r#"import app.other as other
from flask import Flask
from . import start_app


def main():
    other.helper()
    Flask(__name__)
    return start_app()


def persist(record):
    return record.save()
"#;

const FIXTURE: &[(&str, &str)] = &[
    ("app/__init__.py", "from .core import run as start_app\n"),
    ("app/util.py", "def helper():\n    return 1\n"),
    (
        "app/other.py",
        "def helper():\n    return 2\n\n\ndef start():\n    return 3\n",
    ),
    (
        "app/core.py",
        r#"from app.util import helper


class Engine:
    def start(self):
        self.stop()
        return helper()

    def stop(self):
        return 0


def run():
    # helper() yorumda: kenar değil
    text = "helper()"
    engine = Engine()
    return engine.start()
"#,
    ),
    ("app/cli.py", CLI),
    (
        "app/models.py",
        r#"class Base:
    def save(self):
        return 1


class User(Base):
    def save(self):
        return super().save()
"#,
    ),
    (
        "app/shim.py",
        "class Flask:\n    def __init__(self, name):\n        self.name = name\n",
    ),
    ("app/broken.py", "def broken(:\n    return helper(\n"),
    ("lib.rs", "fn bar() {}\nfn foo() { bar(); }\n"),
];

/// Dosyaları geçici bir projeye yazar ve gömücüsüz indeksler.
async fn index_fixture(files: &[(&str, &str)]) -> Result<(TempDir, RetrievalEngine)> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let dir = tempdir()?;
    let mut paths = Vec::new();
    for (path, content) in files {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, content)?;
        paths.push(PathBuf::from(path));
    }
    let db_path = dir.path().join("db");
    std::fs::create_dir_all(&db_path)?;
    let store = LanceDbStore::new(db_path.to_string_lossy().as_ref(), "code_vectors").await?;
    let engine = RetrievalEngine::new(Arc::new(RwLock::new(CodeGraph::new())), store);
    engine
        .incremental_index_paths(dir.path().to_string_lossy().as_ref(), &paths)
        .await?;
    Ok((dir, engine))
}

/// Dosyadaki adı tek olan düğüm.
fn node(graph: &CodeGraph, file: &str, name: &str) -> NodeIndex {
    let prefix = format!("{file}:");
    let found: Vec<NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|idx| graph.graph[*idx].name == name && graph.graph[*idx].id.starts_with(&prefix))
        .collect();
    assert_eq!(found.len(), 1, "expected one {name} in {file}, found {found:?}");
    found[0]
}

/// Sınıfın doğrudan üyesi.
fn member(graph: &CodeGraph, file: &str, class: &str, name: &str) -> NodeIndex {
    let class_idx = node(graph, file, class);
    let found: Vec<NodeIndex> = graph
        .graph
        .edges_directed(class_idx, Direction::Outgoing)
        .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| edge.target())
        .filter(|idx| graph.graph[*idx].name == name)
        .collect();
    assert_eq!(found.len(), 1, "expected one {class}.{name} in {file}");
    found[0]
}

/// Dosya düğümü.
fn file_node(graph: &CodeGraph, file: &str) -> NodeIndex {
    graph
        .graph
        .node_indices()
        .find(|idx| graph.graph[*idx].id == file)
        .unwrap_or_else(|| panic!("file node {file} missing"))
}

/// İki düğüm arasındaki `Contains` dışı kenar türleri.
fn edge_types(graph: &CodeGraph, from: NodeIndex, to: NodeIndex) -> Vec<EdgeType> {
    let mut types: Vec<EdgeType> = graph
        .graph
        .edges_connecting(from, to)
        .map(|edge| edge.weight().clone())
        .filter(|weight| !matches!(weight, EdgeType::Contains))
        .collect();
    types.sort_by_key(|weight| format!("{weight:?}"));
    types
}

/// Düğümün sözdizimi olguları; sözcükselse test düşer.
fn syntax(graph: &CodeGraph, idx: NodeIndex) -> &ccm_core::graph::SyntaxFacts {
    match &graph.graph[idx].facts {
        ReferenceFacts::Syntax(facts) => facts,
        ReferenceFacts::Lexical => panic!("{} has lexical facts", graph.graph[idx].id),
    }
}

#[tokio::test]
async fn python_facts_come_from_the_syntax_tree() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;

    let start = member(&graph, "app/core.py", "Engine", "start");
    let targets: Vec<&CallTarget> = syntax(&graph, start).calls.iter().map(|call| &call.target).collect();
    assert_eq!(
        targets,
        vec![
            &CallTarget::SelfMember("stop".to_string()),
            &CallTarget::Bare("helper".to_string())
        ]
    );

    // Yorum ve string içindeki `helper()` çağrı değildir.
    let run = node(&graph, "app/core.py", "run");
    assert!(syntax(&graph, run).calls.iter().all(|call| call.target.name() != "helper"));

    assert_eq!(
        syntax(&graph, file_node(&graph, "app/core.py")).imports,
        vec![ImportBinding {
            local: "helper".into(),
            module: "app.util".into(),
            symbol: Some("helper".into()),
        }]
    );
    let cli_imports = &syntax(&graph, file_node(&graph, "app/cli.py")).imports;
    assert!(cli_imports.contains(&ImportBinding {
        local: "start_app".into(),
        module: "app".into(),
        symbol: Some("start_app".into()),
    }));
    assert!(cli_imports.contains(&ImportBinding {
        local: "other".into(),
        module: "app.other".into(),
        symbol: None,
    }));

    let user = node(&graph, "app/models.py", "User");
    assert_eq!(syntax(&graph, user).bases, vec![CallTarget::Bare("Base".to_string())]);
    let user_save = member(&graph, "app/models.py", "User", "save");
    assert_eq!(
        syntax(&graph, user_save).calls.iter().map(|call| &call.target).collect::<Vec<_>>(),
        vec![
            &CallTarget::SuperMember("save".to_string()),
            &CallTarget::Bare("super".to_string())
        ]
    );

    // Sözdizimi çıkarıcısı olmayan diller sözcüksel kalır.
    let foo = node(&graph, "lib.rs", "foo");
    assert_eq!(graph.graph[foo].facts, ReferenceFacts::Lexical);
    Ok(())
}
```

`super().save()` çağrısı iki çağrı yeri üretir. Dış çağrı önce gelir: yürüyüş önce `call` düğümünü kaydeder, sonra çocuklarına (`super()`) iner.

- [ ] **Step 2: Başarısız olduğunu gör.**

Run: `~/.cargo/bin/cargo test -p ccm-core --test python_references_test python_facts_come_from_the_syntax_tree`

Expected: FAIL. Panik: `… has lexical facts`.

- [ ] **Step 3: `core/src/vector/python_facts.rs` dosyasını oluştur.**

```rust
//! Python sözdizimi ağacından referans olguları: çağrı yerleri, import bağları ve
//! taban sınıflar. Yorum ve string içerikleri ağaçta `call` düğümü üretmediği için
//! kenar kaynağı olamaz; iç içe tanımlar kendi düğümlerinin olgularıdır.

use tree_sitter::Node;

use crate::graph::{python_package, CallSite, CallTarget, ImportBinding, SyntaxFacts, SyntaxLanguage};

/// Fonksiyonun olguları: dekoratörleri ve gövdesi (iç içe tanımlar hariç).
pub fn function_facts(definition: Node, source: &str, file_id: &str) -> SyntaxFacts {
    let package = python_package(file_id);
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Python);
    collect_decorators(definition, source, &package, &mut facts);
    if let Some(body) = definition.child_by_field_name("body") {
        collect_scope(body, source, &package, &mut facts);
    }
    facts
}

/// Sınıfın olguları: taban sınıflar, dekoratörler ve sınıf gövdesindeki çağrılar
/// (metot gövdeleri hariç).
pub fn class_facts(definition: Node, source: &str, file_id: &str) -> SyntaxFacts {
    let package = python_package(file_id);
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Python);
    collect_decorators(definition, source, &package, &mut facts);
    if let Some(bases) = definition.child_by_field_name("superclasses") {
        let mut cursor = bases.walk();
        for base in bases.named_children(&mut cursor) {
            if let Some(target) = reference_target(base, source) {
                facts.bases.push(target);
            }
        }
    }
    if let Some(body) = definition.child_by_field_name("body") {
        collect_scope(body, source, &package, &mut facts);
    }
    facts
}

/// Modül düzeyindeki çağrılar ve importlar (fonksiyon ve sınıf gövdeleri hariç).
pub fn module_facts(root: Node, source: &str, file_id: &str) -> SyntaxFacts {
    let package = python_package(file_id);
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Python);
    collect_scope(root, source, &package, &mut facts);
    facts
}

/// `@dekoratör` uygulaması bir çağrıdır; tanımı saran `decorated_definition`
/// düğümündeki dekoratörler tanımın olgusuna eklenir.
fn collect_decorators(definition: Node, source: &str, package: &[String], facts: &mut SyntaxFacts) {
    let Some(parent) = definition.parent() else {
        return;
    };
    if parent.kind() != "decorated_definition" {
        return;
    }
    let mut cursor = parent.walk();
    for decorator in parent.named_children(&mut cursor) {
        if decorator.kind() != "decorator" {
            continue;
        }
        let line = decorator.start_position().row + 1;
        let mut inner = decorator.walk();
        for expression in decorator.named_children(&mut inner) {
            if expression.kind() == "call" {
                collect_node(expression, source, package, facts);
            } else if let Some(target) = reference_target(expression, source) {
                facts.calls.push(CallSite { target, line });
            }
        }
    }
}

fn collect_scope(node: Node, source: &str, package: &[String], facts: &mut SyntaxFacts) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, source, package, facts);
    }
}

fn collect_node(node: Node, source: &str, package: &[String], facts: &mut SyntaxFacts) {
    match node.kind() {
        // İç içe tanımlar kendi düğümlerinin olgularıdır.
        "function_definition" | "class_definition" | "decorated_definition" => {}
        "call" => {
            if let Some(target) = node
                .child_by_field_name("function")
                .and_then(|function| reference_target(function, source))
            {
                facts.calls.push(CallSite {
                    target,
                    line: node.start_position().row + 1,
                });
            }
            collect_scope(node, source, package, facts);
        }
        "import_statement" => facts.imports.extend(import_bindings(node, source)),
        "import_from_statement" => {
            facts
                .imports
                .extend(from_import_bindings(node, source, package));
        }
        _ => collect_scope(node, source, package, facts),
    }
}

/// Çağrılan ya da miras alınan ifadenin hedefi; adı olmayan ifadeler (`f()()`,
/// `x[0]()`) hedef taşımaz.
fn reference_target(expression: Node, source: &str) -> Option<CallTarget> {
    match expression.kind() {
        "identifier" => Some(CallTarget::Bare(text(expression, source)?)),
        "attribute" => {
            let name = text(expression.child_by_field_name("attribute")?, source)?;
            let object = expression.child_by_field_name("object")?;
            if is_super_call(object, source) {
                return Some(CallTarget::SuperMember(name));
            }
            Some(match dotted_path(object, source) {
                Some(path) if path == "self" || path == "cls" => CallTarget::SelfMember(name),
                Some(path) => CallTarget::Member {
                    qualifier: path,
                    name,
                },
                None => CallTarget::Chained(name),
            })
        }
        _ => None,
    }
}

fn is_super_call(node: Node, source: &str) -> bool {
    node.kind() == "call"
        && node.child_by_field_name("function").is_some_and(|function| {
            function.kind() == "identifier" && text(function, source).as_deref() == Some("super")
        })
}

/// Yalnız tanımlayıcılardan oluşan noktalı yol (`a`, `a.b.c`); başka ifade `None`.
fn dotted_path(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => text(node, source),
        "attribute" => {
            let object = dotted_path(node.child_by_field_name("object")?, source)?;
            let attribute = text(node.child_by_field_name("attribute")?, source)?;
            Some(format!("{object}.{attribute}"))
        }
        _ => None,
    }
}

/// `import a.b` (kapsamda `a` bağlanır) ve `import a.b as c`.
fn import_bindings(node: Node, source: &str) -> Vec<ImportBinding> {
    let mut bindings = Vec::new();
    let mut cursor = node.walk();
    for name in node.children_by_field_name("name", &mut cursor) {
        match name.kind() {
            "dotted_name" => {
                let Some(module) = text(name, source) else {
                    continue;
                };
                let head = module.split('.').next().unwrap_or_default().to_string();
                bindings.push(ImportBinding {
                    local: head.clone(),
                    module: head,
                    symbol: None,
                });
            }
            "aliased_import" => {
                let module = name
                    .child_by_field_name("name")
                    .and_then(|node| text(node, source));
                let alias = name
                    .child_by_field_name("alias")
                    .and_then(|node| text(node, source));
                if let (Some(module), Some(alias)) = (module, alias) {
                    bindings.push(ImportBinding {
                        local: alias,
                        module,
                        symbol: None,
                    });
                }
            }
            _ => {}
        }
    }
    bindings
}

/// `from m import x, y as z`, `from . import x`, `from m import *`.
fn from_import_bindings(node: Node, source: &str, package: &[String]) -> Vec<ImportBinding> {
    let Some(module) = node
        .child_by_field_name("module_name")
        .and_then(|module_name| absolute_module(module_name, source, package))
    else {
        return Vec::new();
    };
    let mut bindings = Vec::new();
    let mut cursor = node.walk();
    if node
        .named_children(&mut cursor)
        .any(|child| child.kind() == "wildcard_import")
    {
        bindings.push(ImportBinding {
            local: "*".to_string(),
            module: module.clone(),
            symbol: Some("*".to_string()),
        });
    }
    let mut cursor = node.walk();
    for name in node.children_by_field_name("name", &mut cursor) {
        let pair = match name.kind() {
            "dotted_name" => text(name, source).map(|symbol| (symbol.clone(), symbol)),
            "aliased_import" => name
                .child_by_field_name("name")
                .and_then(|node| text(node, source))
                .zip(
                    name.child_by_field_name("alias")
                        .and_then(|node| text(node, source)),
                ),
            _ => None,
        };
        if let Some((symbol, local)) = pair {
            bindings.push(ImportBinding {
                local,
                module: module.clone(),
                symbol: Some(symbol),
            });
        }
    }
    bindings
}

/// Modül adını mutlak noktalı yola çevirir; göreli importlar dosyanın paketine göre
/// çözülür (`from ..x import y`). Paketin dışına taşan göreli import `None`.
fn absolute_module(module_name: Node, source: &str, package: &[String]) -> Option<String> {
    match module_name.kind() {
        "dotted_name" => text(module_name, source),
        "relative_import" => {
            let mut level = 0;
            let mut rest = None;
            let mut cursor = module_name.walk();
            for child in module_name.children(&mut cursor) {
                match child.kind() {
                    "import_prefix" => {
                        level = text(child, source)?.chars().filter(|ch| *ch == '.').count();
                    }
                    "dotted_name" => rest = text(child, source),
                    _ => {}
                }
            }
            let keep = package.len().checked_sub(level.checked_sub(1)?)?;
            let mut parts: Vec<String> = package[..keep].to_vec();
            if let Some(rest) = rest {
                parts.extend(rest.split('.').map(str::to_string));
            }
            (!parts.is_empty()).then(|| parts.join("."))
        }
        _ => None,
    }
}

fn text(node: Node, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(str::to_string)
}
```

- [ ] **Step 4: Çıkarıcıya bağla.**

`core/src/vector/mod.rs` dosyasına şunu ekle:

```rust
pub mod python_facts;
```

`core/src/vector/extractor.rs` dosyasında:

1. Importları güncelle:

```rust
use crate::graph::{CodeGraph, CodeNode, EdgeType, NodeType, ReferenceFacts, SyntaxFacts, SyntaxLanguage};
use crate::vector::python_facts;
```

2. `extract` içinde `file_node` değişmezine `facts: ReferenceFacts::Lexical,` ekle. `self.walk_node(...)?;` satırından sonra şunu ekle:

```rust
        if matches!(self.language, SupportedLanguage::Python) {
            graph.graph[file_idx].facts = ReferenceFacts::Syntax(python_facts::module_facts(
                tree.root_node(),
                &self.source_code,
                file_id,
            ));
        }
```

3. `walk_node` içindeki `code_node` değişmezine `facts: self.reference_facts(&node, file_id),` ekle. `impl Extractor` içine şu metodu yaz:

```rust
    /// Düğümün referans olguları: Python'da sözdiziminden, diğer dillerde sözcüksel.
    fn reference_facts(&self, node: &Node, file_id: &str) -> ReferenceFacts {
        match self.language {
            SupportedLanguage::Python => ReferenceFacts::Syntax(match node.kind() {
                "function_definition" => {
                    python_facts::function_facts(*node, &self.source_code, file_id)
                }
                "class_definition" => python_facts::class_facts(*node, &self.source_code, file_id),
                // Atama ve import düğümleri kenar üretmez: çağrılar kapsayan
                // fonksiyona ya da dosyaya, import bağları kapsamın sahibine aittir.
                _ => SyntaxFacts::empty(SyntaxLanguage::Python),
            }),
            _ => ReferenceFacts::Lexical,
        }
    }
```

- [ ] **Step 5: Testi geçir.**

Run: `~/.cargo/bin/cargo test -p ccm-core --test python_references_test python_facts_come_from_the_syntax_tree`

Expected: PASS. Bir alan adı (`superclasses`, `module_name`, `alias`, `import_prefix`) farklıysa test hangisinin boş kaldığını gösterir. Bu durumda ilgili `node-types.json` dosyasını cargo kayıt kaynağından oku ve adı düzelt:

```bash
rg -l "superclasses" ~/.cargo/registry/src/*/tree-sitter-python-*/src/node-types.json
```

- [ ] **Step 6: fmt, clippy, tüm core testleri ve commit.**

```bash
~/.cargo/bin/cargo fmt --all && ~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
~/.cargo/bin/cargo test -p ccm-core
git add core
git commit -m "feat(index): extract Python call sites, imports and bases from the syntax tree" -m "Comments and strings no longer look like calls, calls belong to the innermost function, class or module, and relative imports are made absolute at extraction time." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Python çözümleyici

**Files:**
- Create: `core/src/graph/resolve.rs`
- Modify: `core/src/graph/mod.rs` (`mod resolve;`, `resolve_references`, `is_reference_source`)
- Test: `core/tests/python_references_test.rs`

**Interfaces:**
- Consumes:
  - `CodeGraph::{find_nodes_by_name, find_nodes_by_file, find_file_node, file_nodes_index}`
  - `graph_node_file_path`, `is_reference_target_type`
  - Task 1–2 olguları
- Produces:
  - `resolve::python_references(graph, modules, source_idx, facts) -> Vec<(NodeIndex, NodeIndex, EdgeType)>`
  - `resolve::PythonModules::{new, files}`
  - `resolve::MAX_POSSIBLE_TARGETS`

- [ ] **Step 1: Başarısız testleri yaz.** `python_references_test.rs` sonuna:

```rust
#[tokio::test]
async fn python_calls_resolve_through_scopes_and_imports() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;
    let calls = vec![EdgeType::Calls];
    let ambiguous = vec![EdgeType::CallAmbiguous];
    let none: Vec<EdgeType> = Vec::new();

    let start = member(&graph, "app/core.py", "Engine", "start");
    let stop = member(&graph, "app/core.py", "Engine", "stop");
    let util_helper = node(&graph, "app/util.py", "helper");
    let other_helper = node(&graph, "app/other.py", "helper");
    let other_start = node(&graph, "app/other.py", "start");
    let engine_class = node(&graph, "app/core.py", "Engine");
    let run = node(&graph, "app/core.py", "run");
    let main = node(&graph, "app/cli.py", "main");
    let persist = node(&graph, "app/cli.py", "persist");
    let base = node(&graph, "app/models.py", "Base");
    let user = node(&graph, "app/models.py", "User");
    let base_save = member(&graph, "app/models.py", "Base", "save");
    let user_save = member(&graph, "app/models.py", "User", "save");
    let shim_flask = node(&graph, "app/shim.py", "Flask");

    assert_eq!(edge_types(&graph, start, stop), calls, "self.stop()");
    assert_eq!(edge_types(&graph, start, util_helper), calls, "imported helper");
    assert_eq!(edge_types(&graph, start, other_helper), none, "same name, other module");
    assert_eq!(edge_types(&graph, run, engine_class), calls, "Engine() constructor");
    assert_eq!(edge_types(&graph, run, start), ambiguous, "engine.start(): receiver unknown");
    assert_eq!(edge_types(&graph, run, other_start), ambiguous, "engine.start(): receiver unknown");
    assert_eq!(edge_types(&graph, run, util_helper), none, "comment and string are not calls");
    assert_eq!(edge_types(&graph, main, other_helper), calls, "module alias other.helper()");
    assert_eq!(edge_types(&graph, main, util_helper), none);
    assert_eq!(edge_types(&graph, main, shim_flask), none, "flask is outside the project");
    assert_eq!(edge_types(&graph, main, run), calls, "re-exported start_app");
    assert_eq!(edge_types(&graph, persist, base_save), ambiguous);
    assert_eq!(edge_types(&graph, persist, user_save), ambiguous);
    assert_eq!(edge_types(&graph, user, base), vec![EdgeType::Inherits]);
    assert_eq!(edge_types(&graph, user_save, base_save), calls, "super().save()");
    assert_eq!(
        edge_types(&graph, file_node(&graph, "app/core.py"), util_helper),
        vec![EdgeType::Imports]
    );
    assert_eq!(
        edge_types(&graph, file_node(&graph, "app/__init__.py"), run),
        vec![EdgeType::Imports]
    );
    assert_eq!(
        edge_types(&graph, file_node(&graph, "app/cli.py"), run),
        vec![EdgeType::Imports]
    );
    // Python dışı diller değişmez.
    assert_eq!(
        edge_types(&graph, node(&graph, "lib.rs", "foo"), node(&graph, "lib.rs", "bar")),
        calls
    );
    Ok(())
}

#[tokio::test]
async fn root_relative_module_wins_over_a_suffix_match() -> Result<()> {
    let (_dir, engine) = index_fixture(&[
        ("app/util.py", "def helper():\n    return 1\n"),
        ("tests/app/util.py", "def helper():\n    return 2\n"),
        (
            "app/core.py",
            "from app.util import helper\n\n\ndef run():\n    return helper()\n",
        ),
        ("src/pkg/mod.py", "def work():\n    return 1\n"),
        (
            "src/pkg/use.py",
            "from pkg.mod import work\n\n\ndef go():\n    return work()\n",
        ),
    ])
    .await?;
    let graph = engine.graph.read().await;
    let run = node(&graph, "app/core.py", "run");
    assert_eq!(
        edge_types(&graph, run, node(&graph, "app/util.py", "helper")),
        vec![EdgeType::Calls]
    );
    assert!(edge_types(&graph, run, node(&graph, "tests/app/util.py", "helper")).is_empty());
    let go = node(&graph, "src/pkg/use.py", "go");
    assert_eq!(
        edge_types(&graph, go, node(&graph, "src/pkg/mod.py", "work")),
        vec![EdgeType::Calls]
    );
    Ok(())
}

#[tokio::test]
async fn a_method_name_shared_by_many_classes_produces_no_edge() -> Result<()> {
    let mut source = String::new();
    for index in 0..6 {
        source.push_str(&format!(
            "class C{index}:\n    def get(self):\n        return {index}\n\n\n"
        ));
    }
    source.push_str("def use(x):\n    return x.get()\n");
    let (_dir, engine) = index_fixture(&[("app/many.py", source.as_str())]).await?;
    let graph = engine.graph.read().await;
    let use_idx = node(&graph, "app/many.py", "use");
    let outgoing = graph
        .graph
        .edges_directed(use_idx, Direction::Outgoing)
        .filter(|edge| !matches!(edge.weight(), EdgeType::Contains))
        .count();
    assert_eq!(outgoing, 0, "x.get() with 6 candidates must not link");
    Ok(())
}
```

- [ ] **Step 2: Başarısız olduğunu gör.**

Run: `~/.cargo/bin/cargo test -p ccm-core --test python_references_test`

Expected: üç yeni test FAIL. Hâlâ sözcüksel kenarlar üretiliyor; örneğin `start → other_helper` boş değil ya da `user → base` `Inherits` değil.

- [ ] **Step 3: `core/src/graph/resolve.rs` dosyasını oluştur.**

```rust
//! Python sözdizimi olgularını kenarlara çeviren çözümleyici.
//!
//! Kurallar sırayla denenir: aynı dosyadaki tanım, import bağı (paket yeniden dışa
//! aktarması dahil), `self`/`cls`/`super()` ve sınıf üyeleri. Bunlar kesin kenar
//! üretir. Alıcısı bilinmeyen çağrılar az sayıda adaya "olası", import edilmemiş
//! tek proje tanımına düşen çıplak adlar "çıkarım" kenarı üretir. Projeden çıkan
//! importlar ve yerleşik adlar kenar üretmez.

use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::references::{python_module_path, CallTarget, ImportBinding, ReferenceFacts, SyntaxFacts};
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
    "abs", "aiter", "all", "anext", "any", "ascii", "bin", "bool", "breakpoint", "bytearray",
    "bytes", "callable", "chr", "classmethod", "compile", "complex", "delattr", "dict", "dir",
    "divmod", "enumerate", "eval", "exec", "filter", "float", "format", "frozenset", "getattr",
    "globals", "hasattr", "hash", "help", "hex", "id", "input", "int", "isinstance",
    "issubclass", "iter", "len", "list", "locals", "map", "max", "memoryview", "min", "next",
    "object", "oct", "open", "ord", "pow", "print", "property", "range", "repr", "reversed",
    "round", "set", "setattr", "slice", "sorted", "staticmethod", "str", "sum", "super",
    "tuple", "type", "vars", "zip", "__import__",
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
}

impl PythonModules {
    pub(crate) fn new(graph: &CodeGraph) -> Self {
        let mut by_last: HashMap<String, Vec<(Vec<String>, String)>> = HashMap::new();
        for file_id in graph.file_nodes_index.keys() {
            let Some(parts) = python_module_path(file_id) else {
                continue;
            };
            if let Some(last) = parts.last().cloned() {
                by_last
                    .entry(last)
                    .or_default()
                    .push((parts, file_id.clone()));
            }
        }
        Self { by_last }
    }

    /// Yolun dosyaları: kök göreli tam eşleşme varsa yalnız o; yoksa yolu sonek
    /// olarak taşıyan dosyalar (`src/` düzeni).
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
            .filter(|(parts, _)| parts.len() > wanted.len() && same(&parts[parts.len() - wanted.len()..]))
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
        self.bindings.iter().copied().find(|binding| binding.local == local)
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
    let mut references: Vec<(NodeIndex, NodeIndex, EdgeType)> = strongest
        .into_iter()
        .map(|(target, edge)| (source_idx, target, edge))
        .collect();
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
    references.extend(imported.into_iter().map(|(target, edge)| (source_idx, target, edge)));
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
        EdgeType::Imports => 3,
        EdgeType::ImportAmbiguous => 4,
        EdgeType::Inherits => 5,
        EdgeType::Defines | EdgeType::Contains | EdgeType::Reads | EdgeType::Writes => 6,
    }
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

fn resolve_bare(graph: &CodeGraph, modules: &PythonModules, scope: &Scope<'_>, name: &str) -> Resolution {
    let local = module_scope_definitions(graph, scope.file_id, name, scope.source_idx);
    if !local.is_empty() {
        return Resolution::Exact(local);
    }
    if let Some(binding) = scope.binding(name) {
        return match binding.symbol.as_deref() {
            Some(symbol) if symbol != "*" => {
                non_empty_exact(symbol_in_module(graph, modules, &binding.module, symbol, 0))
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
    unique_python_definition(graph, scope.source_idx, name).map_or(Resolution::External, Resolution::Inferred)
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
        let classes: Vec<NodeIndex> = module_scope_definitions(graph, scope.file_id, head, scope.source_idx)
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

fn exact_or_possible(graph: &CodeGraph, scope: &Scope<'_>, name: &str, targets: Vec<NodeIndex>) -> Resolution {
    if targets.is_empty() {
        possible(graph, scope.source_idx, name)
    } else {
        Resolution::Exact(targets)
    }
}

/// Alıcısı bilinmeyen çağrı: aynı adlı Python fonksiyon ve metotları, en çok
/// `MAX_POSSIBLE_TARGETS` aday varsa.
fn possible(graph: &CodeGraph, source_idx: NodeIndex, name: &str) -> Resolution {
    let candidates: Vec<NodeIndex> = graph
        .find_nodes_by_name(name)
        .iter()
        .copied()
        .filter(|idx| {
            *idx != source_idx
                && matches!(graph.graph[*idx].node_type, NodeType::Function | NodeType::Method)
                && graph_node_file_path(&graph.graph[*idx].id).ends_with(".py")
        })
        .collect();
    if candidates.is_empty() || candidates.len() > MAX_POSSIBLE_TARGETS {
        Resolution::External
    } else {
        Resolution::Possible(candidates)
    }
}

/// Dosyada çıplak adla görünen tanımlar: sınıf üyesi olmayan fonksiyon ve sınıflar.
fn module_scope_definitions(graph: &CodeGraph, file_id: &str, name: &str, exclude: NodeIndex) -> Vec<NodeIndex> {
    graph
        .find_nodes_by_name(name)
        .iter()
        .copied()
        .filter(|idx| {
            let node = &graph.graph[*idx];
            *idx != exclude
                && is_reference_target_type(&node.node_type)
                && graph_node_file_path(&node.id) == file_id
                && !parent_of(graph, *idx).is_some_and(|parent| graph.graph[parent].node_type == NodeType::Class)
        })
        .collect()
}

/// Projede sınıf üyesi olmayan tek Python tanımı.
fn unique_python_definition(graph: &CodeGraph, source_idx: NodeIndex, name: &str) -> Option<NodeIndex> {
    let mut candidates = graph.find_nodes_by_name(name).iter().copied().filter(|idx| {
        let node = &graph.graph[*idx];
        *idx != source_idx
            && is_reference_target_type(&node.node_type)
            && graph_node_file_path(&node.id).ends_with(".py")
            && !parent_of(graph, *idx).is_some_and(|parent| graph.graph[parent].node_type == NodeType::Class)
    });
    match (candidates.next(), candidates.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Modülün üst düzeyindeki `symbol`; yoksa paketin yeniden dışa aktardığı tanım.
fn symbol_in_module(graph: &CodeGraph, modules: &PythonModules, module: &str, symbol: &str, depth: usize) -> Vec<NodeIndex> {
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
        for binding in file_facts.imports.iter().filter(|binding| binding.local == symbol) {
            if let Some(original) = binding.symbol.as_deref().filter(|original| *original != "*") {
                targets.extend(symbol_in_module(graph, modules, &binding.module, original, depth + 1));
            }
        }
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// `a.b.Klass` yolundaki sınıfın (ve tabanlarının) `name` üyeleri.
fn class_members_at(graph: &CodeGraph, modules: &PythonModules, path: &str, name: &str) -> Vec<NodeIndex> {
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
fn members_with_bases(graph: &CodeGraph, modules: &PythonModules, class_idx: NodeIndex, name: &str, depth: usize) -> Vec<NodeIndex> {
    let own = class_members(graph, class_idx, name);
    if !own.is_empty() || depth >= MAX_BASE_DEPTH {
        return own;
    }
    base_members(graph, modules, class_idx, name, depth)
}

/// Sınıfın tabanlarındaki `name` üyeleri (`super()` ve kalıtım).
fn base_members(graph: &CodeGraph, modules: &PythonModules, class_idx: NodeIndex, name: &str, depth: usize) -> Vec<NodeIndex> {
    let ReferenceFacts::Syntax(facts) = &graph.graph[class_idx].facts else {
        return Vec::new();
    };
    let scope = Scope::of(graph, class_idx, facts);
    let mut members = Vec::new();
    for base in &facts.bases {
        if let Resolution::Exact(targets) = resolve_target(graph, modules, &scope, base) {
            for base_idx in targets {
                if graph.graph[base_idx].node_type == NodeType::Class && base_idx != class_idx {
                    members.extend(members_with_bases(graph, modules, base_idx, name, depth + 1));
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
                && parent_of(graph, *idx).is_some_and(|parent| graph.graph[parent].node_type == NodeType::File)
        })
        .collect()
}

fn parent_of(graph: &CodeGraph, idx: NodeIndex) -> Option<NodeIndex> {
    graph
        .graph
        .edges_directed(idx, Direction::Incoming)
        .find(|edge| matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| edge.source())
}
```

- [ ] **Step 4: Dispatch ve kaynak seçimi.** `core/src/graph/mod.rs` dosyasında:

1. `pub mod references;` satırının altına `mod resolve;` ekle.

2. `resolve_references` metodunu şununla değiştir:

```rust
    fn resolve_references(&self, sources: &[NodeIndex]) -> Vec<(NodeIndex, NodeIndex, EdgeType)> {
        let mut symbols = SymbolTable::new(self);
        let modules = resolve::PythonModules::new(self);
        let mut references = Vec::new();
        for source_idx in sources {
            match &self.graph[*source_idx].facts {
                ReferenceFacts::Lexical => {
                    self.resolve_source_references(*source_idx, &mut symbols, &mut references)
                }
                ReferenceFacts::Syntax(facts) => match facts.language {
                    SyntaxLanguage::Python => references.extend(resolve::python_references(
                        self,
                        &modules,
                        *source_idx,
                        facts,
                    )),
                },
            }
        }
        references
    }
```

3. `is_reference_source` fonksiyonunu şununla değiştir:

```rust
/// Referans üreten düğümler: sözcüksel düğümlerde türe göre, sözdizimi
/// olgularında referansı olan her düğüm (fonksiyon, sınıf, dosya).
fn is_reference_source(node: &CodeNode) -> bool {
    match &node.facts {
        ReferenceFacts::Lexical => matches!(
            node.node_type,
            NodeType::Function | NodeType::Method | NodeType::Variable | NodeType::Import
        ),
        ReferenceFacts::Syntax(facts) => facts.has_references(),
    }
}
```

4. `Inherits` kenarlarının bir referans kenarı olarak yenilenip yenilenemeyeceğini kontrol et:

```bash
rg -n "EdgeType::Inherits" core mcp cli
```

Bu çözümleyici dışında `Inherits` üreten bir yer yoksa `is_reference_edge` desenine `| EdgeType::Inherits` ekle. Başka bir üretici varsa dur ve kullanıcıya bildir: yenileme onun kenarlarını silerdi.

- [ ] **Step 5: Testleri geçir.**

Run: `~/.cargo/bin/cargo test -p ccm-core --test python_references_test`

Expected: dört test PASS.

- [ ] **Step 6: Gerileme kontrolü.**

```bash
~/.cargo/bin/cargo test --workspace
```

Expected: PASS. Sözcüksel Python kenarlarını (ör. `Variable → Calls`) sabit yazan eski bir test düşerse beklentisini yeni kurala göre güncelle. Güncellemenin gerekçesini commit mesajına yaz.

- [ ] **Step 7: fmt, clippy ve commit.**

```bash
~/.cargo/bin/cargo fmt --all && ~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
git add core
git commit -m "feat(graph): resolve Python calls through scopes, imports and class members" -m "Exact edges come from same-file definitions, import bindings (including package re-exports), self/cls/super() and class names. Unknown receivers produce at most five possible edges, imports that leave the project produce none, and comments or strings never do." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Artımlı yenileme

**Files:**
- Modify: `core/src/graph/mod.rs` (`reference_target_names`, `refresh_reference_edges`)
- Test: `core/tests/python_references_test.rs`

**Interfaces:**
- Consumes: `SyntaxFacts::mentions_any`, `ReferenceFacts`
- Produces: yeniden dışa aktarma ve takma ad değişikliklerinde doğru artımlı kenarlar

- [ ] **Step 1: Başarısız testi yaz.**

```rust
#[tokio::test]
async fn incremental_updates_follow_import_and_reexport_changes() -> Result<()> {
    let (dir, engine) = index_fixture(FIXTURE).await?;
    let root = dir.path().to_string_lossy().to_string();

    std::fs::write(
        dir.path().join("app/cli.py"),
        CLI.replace("import app.other as other", "import app.util as other"),
    )?;
    engine
        .incremental_index_paths(&root, &[PathBuf::from("app/cli.py")])
        .await?;
    {
        let graph = engine.graph.read().await;
        let main = node(&graph, "app/cli.py", "main");
        assert_eq!(
            edge_types(&graph, main, node(&graph, "app/util.py", "helper")),
            vec![EdgeType::Calls]
        );
        assert!(edge_types(&graph, main, node(&graph, "app/other.py", "helper")).is_empty());
    }

    std::fs::write(
        dir.path().join("app/__init__.py"),
        "from .other import start as start_app\n",
    )?;
    engine
        .incremental_index_paths(&root, &[PathBuf::from("app/__init__.py")])
        .await?;
    let graph = engine.graph.read().await;
    let main = node(&graph, "app/cli.py", "main");
    assert_eq!(
        edge_types(&graph, main, node(&graph, "app/other.py", "start")),
        vec![EdgeType::Calls]
    );
    assert!(edge_types(&graph, main, node(&graph, "app/core.py", "run")).is_empty());
    Ok(())
}
```

- [ ] **Step 2: Başarısız olduğunu gör.**

Run: `~/.cargo/bin/cargo test -p ccm-core --test python_references_test incremental_updates_follow_import_and_reexport_changes`

Expected: FAIL ikinci blokta. `main → other.start` boş, çünkü `__init__.py` hiçbir tanım içermiyor ve `start_app` etkilenen adlara girmiyor.

- [ ] **Step 3: Etkilenen adları ve kaynak seçimini düzelt.** `core/src/graph/mod.rs` dosyasında:

1. `reference_target_names` metodunu şununla değiştir:

```rust
    /// Dosyanın referans hedefi olabilen düğümlerinin adları ve Python dosyasında
    /// import bağlarının yerel adları (paket yeniden dışa aktarmaları). Artımlı
    /// güncelleme bu adları dosya değişmeden önce ve sonra toplar; bu adlardan
    /// birini anan kaynakların kenarları yeniden hesaplanır.
    pub fn reference_target_names(&self, file_id: &str) -> HashSet<String> {
        let mut names: HashSet<String> = self
            .find_nodes_by_file(file_id)
            .iter()
            .map(|idx| &self.graph[*idx])
            .filter(|node| {
                is_reference_target_type(&node.node_type) && is_referenceable_symbol(&node.name)
            })
            .map(|node| node.name.clone())
            .collect();
        if let Some(file_idx) = self.find_file_node(file_id) {
            if let ReferenceFacts::Syntax(facts) = &self.graph[file_idx].facts {
                names.extend(
                    facts
                        .imports
                        .iter()
                        .filter(|binding| binding.local != "*")
                        .map(|binding| binding.local.clone()),
                );
            }
        }
        names
    }
```

2. `refresh_reference_edges` içindeki filtrede `mentions_any_name(&node.content, affected_names)` ifadesini `source_mentions_any(node, affected_names)` ile değiştir. Python `File` düğümünün içeriği boş olduğundan içerik taraması modül düzeyindeki çağrıları kaçırıyordu. Dosyaya şunu ekle:

```rust
/// Kaynak adlardan birini anıyor mu? Sözcüksel düğümde içerik, sözdizimi
/// olgularında çağrı/bağ/taban adları taranır.
fn source_mentions_any(node: &CodeNode, names: &HashSet<String>) -> bool {
    match &node.facts {
        ReferenceFacts::Lexical => mentions_any_name(&node.content, names),
        ReferenceFacts::Syntax(facts) => facts.mentions_any(names),
    }
}
```

- [ ] **Step 4: Testleri geçir.**

```bash
~/.cargo/bin/cargo test -p ccm-core --test python_references_test
~/.cargo/bin/cargo test -p ccm-core --test incremental_calls_test --test incremental_filesystem_test --test live_index_parity_test
```

Expected: PASS. `live_index_parity_test` artımlı yolun tam yeniden kurulumla aynı kenarları verdiğini doğrular.

- [ ] **Step 5: fmt, clippy ve commit.**

```bash
~/.cargo/bin/cargo fmt --all && ~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
git add core
git commit -m "fix(index): refresh Python callers when an import or re-export changes" -m "A package __init__.py that re-exports a name defines nothing, so its callers were never recomputed, and module-level calls live on a File node whose content is empty. Affected names now include import bindings and syntax sources are matched by their facts." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Dürüst `find_usages`

**Files:**
- Create: `core/src/graph/usages.rs`
- Modify: `core/src/graph/mod.rs` (`pub mod usages;` + re-export), `core/src/engine.rs` (`find_usages`), `mcp/src/tools.rs` (`find_usages` işleyicisi)
- Test: `core/tests/python_references_test.rs`

**Interfaces:**
- Consumes:
  - `CodeGraph::find_node_index_by_id`
  - `CodeGraph`'in bulanık eşleştiricisi (adı Step 3'te doğrulanır)
- Produces:
  - `ccm_core::graph::{usages_of, Usage, UsageRelation, UsageReport, UsageError}`
  - `UsageRelation::label`

- [ ] **Step 1: Başarısız testi yaz.** Test dosyasının importlarına `use ccm_core::graph::{usages_of, UsageError, UsageRelation};` ekle. Sonra:

```rust
#[tokio::test]
async fn usages_report_relations_and_a_missing_node() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;

    let base_save = member(&graph, "app/models.py", "Base", "save");
    let report = usages_of(&graph, &graph.graph[base_save].id)?;
    let seen: Vec<(String, UsageRelation)> = report
        .usages
        .iter()
        .map(|usage| (usage.node.name.clone(), usage.relation))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("save".to_string(), UsageRelation::Calls),
            ("persist".to_string(), UsageRelation::MayCall)
        ]
    );

    let run = node(&graph, "app/core.py", "run");
    let relations: Vec<UsageRelation> = usages_of(&graph, &graph.graph[run].id)?
        .usages
        .iter()
        .map(|usage| usage.relation)
        .collect();
    assert_eq!(
        relations,
        vec![UsageRelation::Calls, UsageRelation::Imports, UsageRelation::Imports]
    );

    // Kararlı kimliği artık olmayan sembol, dosyadaki tek fonksiyona bulanık
    // eşleşmemeli; boş liste yerine açık hata dönmeli.
    let gone = "app/util.py:function_definition:symbol:ffffffffffffffff:0";
    assert_eq!(
        usages_of(&graph, gone).unwrap_err(),
        UsageError::NodeNotFound(gone.to_string())
    );
    Ok(())
}
```

- [ ] **Step 2: Derlenmediğini gör.**

Run: `~/.cargo/bin/cargo test -p ccm-core --test python_references_test usages_report_relations_and_a_missing_node`

Expected: FAIL. `usages_of` bulunamıyor.

- [ ] **Step 3: `core/src/graph/usages.rs` dosyasını oluştur.** Önce bulanık eşleştiricinin adını doğrula:

```bash
rg -n "fn find_node_fuzzy" core/src/graph/mod.rs
```

Aşağıdaki kod `find_node_fuzzy_by_id` adını kullanıyor; farklıysa koddaki adı düzelt.

```rust
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
    let idx = lookup(graph, node_id).ok_or_else(|| UsageError::NodeNotFound(node_id.to_string()))?;
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
```

`core/src/graph/mod.rs` dosyasına şunları ekle:

```rust
pub mod usages;

pub use usages::{usages_of, Usage, UsageError, UsageRelation, UsageReport};
```

- [ ] **Step 4: Testi geçir.**

Run: `~/.cargo/bin/cargo test -p ccm-core --test python_references_test usages_report_relations_and_a_missing_node`

Expected: PASS.

- [ ] **Step 5: Motoru ve aracı bağla.**

1. `core/src/engine.rs` içinde `find_usages` metodunu bul:

```bash
rg -n "fn find_usages" core/src/engine.rs
```

Gövdesini graf okuma kilidi altında `crate::graph::usages_of(&graph, node_id)` çağrısıyla değiştir. Dönüş türünü `Result<UsageReport, UsageError>` yap. Derleyicinin gösterdiği çağıranları düzelt. Eski davranışı (bulanık eşleşme + bulunamazsa boş liste + `Defines` kenarlarının kullanım sayılması) tamamen kaldır.

2. `mcp/src/tools.rs` içindeki `find_usages` işleyicisini düzelt:

```bash
rg -n "No usages found" mcp/src/tools.rs
```

- `Err(UsageError::NodeNotFound(_))` gelirse hatanın `Display` metniyle bir araç hatası döndür (`isError: true`). Bu dosyada araç hatası üreten mevcut yardımcıyı kullan.
- `Ok(report)` gelirse cevabın ilk satırı `usage_summary(&report.target.name, &report.usages)` olsun.
- Her kullanımın mevcut bloğu bugünkü biçimde kalsın; başlık satırının hemen altına `**Relation:** {label}` satırı eklensin.
- Kullanım yoksa özet satırı `0 usages of …` ile başlar. "No usages found" metni kalkar.

Yardımcı fonksiyon:

```rust
/// `find_usages` özet satırı: ilişki başına kullanım sayısı.
fn usage_summary(target_name: &str, usages: &[ccm_core::graph::Usage]) -> String {
    use ccm_core::graph::UsageRelation;
    let count = |relation: UsageRelation| usages.iter().filter(|usage| usage.relation == relation).count();
    format!(
        "{} usages of `{}`: {} calls, {} inferred, {} may call, {} imports, {} inherits",
        usages.len(),
        target_name,
        count(UsageRelation::Calls),
        count(UsageRelation::CallsInferred),
        count(UsageRelation::MayCall),
        count(UsageRelation::Imports),
        count(UsageRelation::Inherits),
    )
}
```

3. Araç açıklamasına (`find_usages` tanımındaki description) şu cümleyi ekle: `Each usage is labeled calls, calls (inferred), may call, imports or inherits; an unknown node ID is an error, not an empty result.`

- [ ] **Step 6: Doğrula.**

```bash
~/.cargo/bin/cargo test --workspace
```

Expected: PASS. `mcp_integration_test` gibi testler eski "No usages found" metnini ya da bulunamayan kimlikte boş cevabı bekliyorsa beklentiyi yeni sözleşmeye çevir: bulunamayan kimlikte `isError: true`, boş sonuçta `0 usages of`.

- [ ] **Step 7: fmt, clippy ve commit.**

```bash
~/.cargo/bin/cargo fmt --all && ~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
git add core mcp
git commit -m "feat(mcp): label every usage and report a vanished node as an error" -m "find_usages answered 'No usages found' when a node ID had disappeared and counted Defines edges as usages. Usages now carry calls, inferred, may call, imports or inherits, and a stable ID that is no longer indexed is an isError result instead of a fuzzy match to another symbol." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Belgeler, tam doğrulama, betimleyici istatistik, PR

**Files:**
- Modify: `README.md`, `README.tr.md` ("Limits" paragrafı), `SKILL.md` (`find_usages` açıklaması)

**Interfaces:**
- Consumes: Task 1–5
- Produces: incelemeye hazır dal

- [ ] **Step 1: README sınırlar paragrafı.**

`README.md` içinde "**Limits, stated up front.**" paragrafının "Call edges are resolved by name" ile başlayıp "produces no edge at all." ile biten ilk iki cümlesini şununla değiştir. Paragrafın geri kalanı aynen kalır.

```markdown
**Limits, stated up front.** In Python files, calls are read from the syntax
tree and resolved through the file's imports (`import`, `from … import`,
relative imports, package re-exports), `self`/`cls`/`super()` and class names;
a call on a receiver whose type is unknown is reported as a *possible* call to
at most five same-named definitions, and imports that leave the project produce
no edge. In the other 12 languages call edges are still resolved by name: a
call binds to a definition in the same file first, otherwise to the only
definition elsewhere; several same-file definitions produce edges marked
ambiguous, and a name defined in several other files produces no edge.
```

`README.tr.md` içinde "**Sınırlar, baştan.** Çağrı kenarları tür analiziyle değil isimle çözülür:" ile başlayıp "birden çok başka dosyada tanımlı bir isim hiç kenar üretmez." ile biten kısmı şununla değiştir:

```markdown
**Sınırlar, baştan.** Python dosyalarında çağrılar sözdizimi ağacından okunur
ve dosyanın importları (`import`, `from … import`, göreli importlar, paket
yeniden dışa aktarımları), `self`/`cls`/`super()` ve sınıf adlarıyla çözülür;
türü bilinmeyen bir alıcıdaki çağrı en çok beş aynı adlı tanıma *olası* çağrı
olarak raporlanır, projeden çıkan importlar kenar üretmez. Diğer 12 dilde çağrı
kenarları hâlâ isimle çözülür: bir çağrı önce aynı dosyadaki tanıma, yoksa başka
yerdeki tek tanıma bağlanır; aynı dosyadaki birden çok tanım belirsiz olarak
işaretlenmiş kenarlar üretir, birden çok başka dosyada tanımlı bir isim hiç
kenar üretmez.
```

- [ ] **Step 2: SKILL.md.** Önce yeri bul:

```bash
rg -n "find_usages" SKILL.md
```

`find_usages`'ın tanımlandığı satırın altına şu cümleyi ekle:

```markdown
Each usage carries a relation — `calls`, `calls (inferred)`, `may call`, `imports` or `inherits` — and an unknown node ID returns an error telling you to call `find_nodes` again.
```

- [ ] **Step 3: Tam doğrulama.**

```bash
~/.cargo/bin/cargo fmt --all -- --check
~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings
~/.cargo/bin/cargo test --workspace
```

Expected: hepsi temiz.

- [ ] **Step 4: Betimleyici kenar istatistiği.** Bu bir iddia değil, akıl sağlığı kontrolüdür ve yalnız PR açıklamasına girer. Eski sürüm (0.3.13) ile yeni sürümü Flask ve Django üzerinde karşılaştır:

```bash
~/.cargo/bin/cargo build --release -p ccm-cli
for bin in ~/.cargo/bin/ccm-cli target/release/ccm-cli; do
  for repo in benchmarks/corpus/flask benchmarks/corpus/django; do
    CCM_DISABLE_EMBEDDER=1 $bin index --path $repo >/dev/null
    graph=$(ls -td $repo/.ccm/.ccm-generations/*/ | head -1)ccm_graph.json
    python3 -c "import json,collections,sys; g=json.load(open(sys.argv[1])); print(sys.argv[2], sys.argv[3], dict(collections.Counter(e[2] for e in g['edges'] if e)))" "$graph" "$bin" "$repo"
  done
done
```

Expected: yeni sürümde `CallAmbiguous` sayısı makul kalır. Kaynak başına bir ad için en çok 5 hedef olur; bu, tasarım gereği sağlanır. Yeni sürümde `CallInferred` ve `Inherits` görünür. Sayıları PR açıklamasına "descriptive, not a claim" notuyla koy.

- [ ] **Step 5: Commit.**

```bash
git add README.md README.tr.md SKILL.md
git commit -m "docs: describe Python syntax resolution and usage relations" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6 (KAPI G4 — kullanıcı onayı şart):** `git push -u origin feat/syntax-graph` ve `gh pr create` ile PR aç. Açıklamada şunlar olsun:
  - Task 1–5'in özeti;
  - Step 4'teki tablo;
  - Review Focus maddeleri ve hangi testin hangisini kapsadığı;
  - "Python dışı dillerde davranış değişmez" notu.

  Açıklama `🤖 Generated with [Claude Code](https://claude.com/claude-code)` satırıyla biter.
