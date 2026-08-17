# Cognitive Codebase Matrix (CCM)

<p align="center">
  <img src="docs/assets/cover.png" width="400" alt="LLM Context Manager">
</p>

[English](./README.md) | Turkce

> **🧠 Otonom Yapay Zeka Ajanlari Icin Norel Omurga**

> CCM, kod tabanınız ile yapay zeka editörünüz arasındaki boşluğu kapatır. Statik kaynak kodu dinamik ve sorgulanabilir bir bilgi grafına dönüştürür; böylece ajanlar projenizi daha doğru gezebilir, anlayabilir ve akıl yürütebilir.

> **Güncel sürüm: v0.3.11.** `mode:"quick"` ile hızlı graph-first indeksleme,
> eşzamanlı `index_now` aracı, hibrit arama için semantic tie-break onarımı ve
> embedding arka ucu kapalıyken üretilmiş graph-only indekslerin otomatik
> onarımı.

[![Rust](https://img.shields.io/badge/Built%20With-Rust-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![MCP Ready](https://img.shields.io/badge/MCP-Compatible-blue.svg?style=flat-square&logo=google-cloud)](https://modelcontextprotocol.io/)
[![Graph-RAG](https://img.shields.io/badge/Engine-Graph--RAG-purple.svg?style=flat-square)](https://github.com/senoldogann/LLM-Context-Manager)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)](LICENSE)
[![Agent Skill](https://img.shields.io/badge/Agent-SKILL.md-blueviolet.svg?style=flat-square)](SKILL.md)

---

## Neden CCM?

Modern yapay zeka kod asistanlari guclu olsa da ciddi bir **baglam korlugu** yasar:

| Problem | Etki |
|---------|------|
| **Baglam Limiti** | 100.000 satirlik projeyi butun olarak goremez |
| **Halusinasyon** | Yapilari tam bilmeden bagimlilik tahmin eder |
| **Baglam Kaybi** | Vektor arama baglantili mantigi degil, benzer kelimeleri bulur |

CCM, yapay zekayi yalnizca bir *metin tahminleyici* olmaktan cikarip daha cok bir **kidemli yazilim mimarina** yaklastirir.

### Ajan Oncelikli Fark

CCM, ham kod yiginlari vermek yerine **AI-Optimized Context** uretir:

- **Mantiksal Akil Yurutme** - Icerigin neden getirildigini aciklar
- **Iliskisel Kenarlar** - Dosyalarin ve sembollerin nasil baglandigini gosterir
- **Guven Skorlari** - Sonuclarin ne kadar guvenli oldugunu belirtir

### CCM ve Alternatifler

Cogu AI-baglam araci *semantik aramada* durur: dosyalari gomup yalnizca benzer
gorunen parcalari dondurur. CCM bunun ustune gercek bir bagimlilik grafi koyar.

| Yetenek | CCM | Yalnizca semantik RAG (Cline, Continue.dev, Aider tarzi) |
|---------|-----|----------------------------------------------------------|
| Semantik arama | Var | Var |
| Cagri grafi: "bunu kim cagiriyor?" | Var (`find_usages`) | Yalnizca sembol grep |
| Etki analizi: degisikligin patlama yaricapi | Var (`impact_of_change`) | Yok |
| Iki dugum arasi BFS cagri zinciri | Var (`trace_call_chain`) | Yok |
| AST'den dosyalar arasi bagimlilik kenarlari | Var (tree-sitter, 13 dil) | Yok |
| Imlec seviyesinde baglam (`dosya:satir`) | Var (`get_context`) | Degisken |

**Tek cumlelik pitch:** *"Kod tabanini sadece arama, haritalandir."* CCM ajana
benzer gorunen metin degil, bagimlilik grafini verir. "Bunu degistirirsem ne
kirilir?" gibi sorulari tahminden sorgulanabilir gercege donusturur.

---

## Temel Ozellikler

### Bağlı Zeka (Graph Navigator)
- **İki Aşamalı İndeksleme** - Fonksiyon tanımlarını çağrı noktalarına bağlar
- **Artırmalı Güncelleme** - İlk çalışmadan sonra yalnızca eklenen, değişen, yeniden adlandırılan veya silinen dosyaları işler
- **Derin Gezinti** - "Bunu kim çağırıyor?" gibi sorulara daha doğru cevap verir

### Yüksek Performanslı Çekirdek
- **Rust Tabanlı** - Hızlı indeksleme ve sorgulama
- **Toplu Embedding** - Büyük kod bloklarını kısa sürede işler
- **LanceDB** - Düşük gecikmeli vektör depolama
- **Tree-sitter** - Rust, Python, TypeScript, JavaScript, Go, Java, Kotlin, C#, C, C++, Ruby, PHP ve Swift için sağlam AST analizi

### Production Sertlestirme
- **Binary Checksums** - Release artifact'lari `checksums.txt` ile dogrulanir
- **MCP Allowlist** - `CCM_ALLOWED_ROOTS` ile erisim alani kisitlanabilir
- **Guvenli Varsayilanlar** - Timeout ve dosya boyutu limitleri ayarlanabilir

### Evrensel MCP Uyumlulugu
- **Tak ve Calistir** - Installer, Codex, Cursor, Claude Desktop ve Antigravity ayarlarini yapar
- **Acik Indeksleme** - Eksik indeks hizli hata verir ve `index_project` aracina yonlendirir
- **Dusuk Konfigurasyon** - Proje koku otomatik tespit edilir

---

## Kurulum

### Otomatik Kurulum

```bash
# 1. AI editorunuz icin MCP ayarlarini yapin
npx @senoldogann/context-manager install

# 2. Projeyi indexleyin
npx @senoldogann/context-manager index --path .
```

### Ilk Calistirma Dogrulamasi

Kurulumdan sonra mutlu yolu hizlica dogrulayin:

```bash
# CLI cevap veriyor mu kontrol et
npx @senoldogann/context-manager query --text "src/main.rs:1"

# MCP server'i dogrudan baslat
npx @senoldogann/context-manager mcp
```

Beklenen sonuc:
- Wrapper, isletim sistemi ve mimariye uygun binary'yi indirir
- `index`, proje icinde `data/ccm_db` olusturur
- `mcp`, JSON-RPC parse hatasi vermeden stdio uzerinde beklemeye gecer

### Editor Uyumlulugu

| Host | Durum | Kurulum Yolu |
|------|-------|--------------|
| Codex | Destekleniyor | `~/.codex/config.toml` atomik olarak güncellenir |
| Cursor | Destekleniyor | `~/.cursor/mcp.json` |
| Claude Desktop | Destekleniyor | Yerel desktop config |
| Antigravity | Destekleniyor | Yerel host config |

Editor otomatik tespit edilmezse installer'in verdigi manuel MCP config'i kullanabilirsiniz.

### 🤖 Ajan Skill'i

CCM, tum 9 MCP aracini, stable node ID formatini, onerilen akisi ve yaygin hatalari anlatan bir [`SKILL.md`](SKILL.md) ile geliyor. Dosya hem kaynak repoda hem npm paketinde bulunur.

Ajan skill dizininize kopyalayin, birinci sinif arac referansi olarak kullanin:
```bash
cp SKILL.md ~/.agents/skills/context-manager/SKILL.md
```

### Manuel Derleme (Rust)

```bash
# Lokal source build hata verirse once protoc kurun.
# macOS: brew install protobuf

git clone https://github.com/senoldogann/LLM-Context-Manager.git
cd LLM-Context-Manager
cargo build --release
```

**Rust kurmadan mi?** [GETTING_STARTED.md](GETTING_STARTED.md) icindeki Docker secenegini kullanin.

### Manuel npm yayini (maintainer)

`package.json` repo kokunde degil, bilerek `npm/` dizinindedir:

```bash
cd npm
npm test
npm pack --dry-run
npm publish --access public --provenance
```

---

## Konfigurasyon

`~/.ccm/.env` dosyasi olusturun (veya repodaki `.env.example` dosyasini baz alin):

```ini
# Secenek A: Lokal (onerilen)
EMBEDDING_PROVIDER=ollama
EMBEDDING_HOST=http://127.0.0.1:11434
EMBEDDING_MODEL=mxbai-embed-large

# Secenek B: Bulut (OpenAI)
EMBEDDING_PROVIDER=openai
EMBEDDING_API_KEY=sk-your-key
EMBEDDING_MODEL=text-embedding-3-small

# Ag ve limitler
EMBEDDING_TIMEOUT_SECS=30
CCM_MAX_FILE_BYTES=2097152

# MCP guvenligi (strict allowlist varsayilan olarak ACIK)
CCM_ALLOWED_ROOTS=/Users/you/projects:/Users/you/sandbox
CCM_REQUIRE_ALLOWED_ROOTS=1

# MCP runtime
CCM_MCP_ENGINE_CACHE_SIZE=8
CCM_MCP_DEBUG=0

# Opsiyonel: embedding'i kapat
CCM_DISABLE_EMBEDDER=0

# Opsiyonel: md/json/yaml dosyalarını vektör aramaya dahil et
CCM_EMBED_DATA_FILES=0

# Binary checksum dogrulama (0 = zorunlu, 1 = bypass)
CCM_ALLOW_UNVERIFIED_BINARIES=0

# Opsiyonel indirme ayarlari
CCM_DOWNLOAD_TIMEOUT_MS=120000
CCM_DOWNLOAD_ATTEMPTS=3
```

Gelismis ayarlar:
- `CCM_PROJECT_ROOT`, npm wrapper ve MCP fallback engine icin varsayilan proje kokunu sabitler.
- `CCM_DB_PATH`, varsayılan MCP vektör veritabanı konumunu değiştirir.
- Chunking, batch size, hibrit agirliklar ve `OPENAI_API_KEY`, `CCM_SKIP_CHECKSUM`, `CCM_MCP_REQUIRE_ALLOWED_ROOTS`, `CCM_EMBED_DATA`, `EMBEDDING_DISABLED` gibi uyumluluk alias'lari icin `.env.example` dosyasina bakin.
- Hibrit skor agirliklari icin [`docs/hybrid-ranking.md`](./docs/hybrid-ranking.md) dosyasini kullanin.

**Not:** Lokal embedding icin Ollama'nin calisiyor olmasi gerekir (`ollama serve`) ve modelin indirilmis olmasi gerekir (`ollama pull mxbai-embed-large`).

**Guvenlik:** MCP varsayilan olarak strict allowlist uygular; yalnizca `CCM_ALLOWED_ROOTS` (yoksa `CCM_PROJECT_ROOT`) altindaki dizinler indekslenebilir/okunabilir. Genis erisim gerekiyorsa `CCM_REQUIRE_ALLOWED_ROOTS=0` verilebilir; bu modda bile erisim baslangic proje kokuyle sinirli kalir.

---

## Kullanim

### CLI Komutlari

```bash
# Projeyi indexle
ccm-cli index --path .

# Semantik arama
ccm-cli query --text "authentication logic"

# Cursor tahmini (file:line format)
ccm-cli query --text "src/main.rs:50"

# Watch mode
ccm-cli index --path . --watch

# Kurulum, allowlist ve index uyumlulugunu denetle
ccm-cli doctor --path .

# Degerlendirme calistir
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json
```

### MCP Tool'lari

| Tool | Amac | Ornek |
|------|------|-------|
| `search_code` | Hibrit semantik + graf arama | "Auth handling'i bul" |
| `get_context` | Dosya ve satira gore baglam | file:line baglami |
| `find_nodes` | Isim veya yola gore node bul | "find_nodes query=UserService" |
| `read_graph` | Belirli bir node'u incele | Node detaylari + graf baglantilari |
| `index_project` | Proje indexini yenile | Arttirmali yeniden indexleme |
| `find_usages` | Bir node'un tum kullanimlarini bul | "Bu fonksiyonu kim cagiriyor?" |
| `trace_call_chain` | Iki node arasi BFS cagri zinciri | from_id → to_id yolu |
| `impact_of_change` | Bir dosya degisikliginin etki alani | Kod tabanindaki bagimlilar |
| `diff_context` | Git'ten son degisiklikler | Son N gunun degisiklikleri |

### Arttirmali indexleme davranisi

İlk `index` tüm projeyi indeksler. Sonraki `index_project` veya `index --watch` çalışmaları dosya manifestini karşılaştırır; yalnızca yeni ya da değişen dosyaları günceller ve silinen dosyaların node'larını kaldırır. Hiçbir şey değişmediyse vektör veritabanı baştan oluşturulmaz. İndeks eksikse veya güncelleniyorsa retrieval araçları hızlı hata verir; arama çağrısını gizli bir yeniden oluşturma işlemi için bekletmek yerine `index_project` açıkça çağrılır.

Büyük repolarda MCP `index_project`, istemci zaman aşımından önce yanıt verir ve
indeksleme arka planda sürer. Son indeksleme istatistikleri gelene kadar aynı
aracı tekrar çağırarak durumu sorgulayabilirsiniz.

Tam yeniden oluşturma önce staging neslinde hazırlanır. Tarama, ayrıştırma,
embedding veya vektör yazma hatası önceki graf, manifest ve vektör tablosunu
yerinde bırakır. Dosya parmak izleri içeriği de kapsadığı için boyutu aynı kalan
ve zaman damgası korunmuş değişiklikler algılanır.

---

## Mimari

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ AI Agent    │────▶│ MCP Server  │────▶│ Core Engine │
│ (Codex vb.) │◀────│ (ccm-mcp)   │◀────│ (Rust)      │
└─────────────┘     └─────────────┘     └─────────────┘
                                                  │
                    ┌────────────────────────────┼────────────────────────────┐
                    ▼                            ▼                            ▼
             ┌─────────────┐            ┌─────────────┐            ┌─────────────┐
             │ Code Graph  │            │  Vector DB  │            │  Parser     │
             │ (Petgraph)  │            │  (LanceDB)  │            │(Tree-sitter)│
             └─────────────┘            └─────────────┘            └─────────────┘
```

---

## Desteklenen Diller

| Dil | Uzanti | Analiz |
|-----|--------|--------|
| Rust | `.rs` | Tam AST |
| Python | `.py` | Tam AST |
| TypeScript | `.ts`, `.tsx` | Tam AST |
| JavaScript | `.js`, `.jsx` | Tam AST |
| Go | `.go` | Tam AST |
| Java | `.java` | Tam AST |
| Kotlin | `.kt`, `.kts` | Tam AST |
| C# | `.cs` | Tam AST |
| C | `.c`, `.h` | Tam AST |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx` | Tam AST |
| Ruby | `.rb`, `.rake`, `.gemspec` | Tam AST |
| PHP | `.php`, `.phtml` | Tam AST |
| Swift | `.swift` | Tam AST |
| Config/Data | `.md`, `.json`, `.yaml` | Tam dosya |

---

## Degerlendirme

CCM, golden task tabanli bir evaluation framework ile gelir:

```bash
# Evaluation calistir
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json

# Structural vs hybrid karsilastir
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json --compare
```

Evaluation index'i yoksa CCM skorlama oncesi otomatik hazirlar.
Semantik `search_code` gorevleri icin embedder gerekir.

**Kayitli sonuclar:** [`eval/`](./eval) altindaki raporlara bakin.

---

## Release Guvenilirligi

CCM release akisinda kurulum guvenilirligi icin su noktalar yer alir:

- GitHub Releases, platform binary'leri ve `checksums.txt` yayinlar
- npm wrapper, indirilen binary'leri ilk kullanimdan once dogrular
- MCP transport request size limit uygular ve debug payload'larda hassas degerleri maskeler
- Release workflow, asset yuklemeden once Linux, macOS ve Windows build'lerini alir
- npm yayini, GitHub Release asset'lari tamamlandiktan sonra `npm/` dizininden manuel yapilir
- README quick-start adimlari ilk kurulum smoke test akisi ile aynidir

Lokal source build icin `cargo build --release` komutu halen makinenizde `protoc` kurulu olmasini gerektirir.

---

## Sorun Giderme

### "No context found"
1. Once `ccm-cli index --path .` calistirin
2. Override ettiyseniz `CCM_PROJECT_ROOT` degerinin indexlenen dizinle ayni oldugunu kontrol edin
3. Ollama'nin ayakta oldugundan emin olun

### Yavas indexleme
- Ilk calisma embedding modelini indirir
- Sonraki calismalar incremental oldugu icin daha hizlidir

### "Checksum manifest not found" / "Checksum mismatch"
1. GitHub release icinde `checksums.txt` oldugunu kontrol edin
2. Kurulumu tekrar deneyin
3. Son care olarak `CCM_ALLOW_UNVERIFIED_BINARIES=1` kullanin

### "Project path is not allowed"
- Strict allowlist modu varsayilan olarak aktiftir
- `CCM_ALLOWED_ROOTS` icine proje kokunu ekleyin
- Gercekten gerekiyorsa `CCM_REQUIRE_ALLOWED_ROOTS=0` kullanin (erisim yine baslangic proje kokuyle sinirli kalir)

### Büyük veya binary dosyalar atlanıyor
- Gerekirse `CCM_MAX_FILE_BYTES` degerini artirin

### Data dosyalari search'te gorunmuyor
- Varsayilan olarak `.md`, `.json`, `.yaml` dosyalari indexlenir ama embed edilmez
- `CCM_EMBED_DATA_FILES=1` ile semantik aramaya dahil edebilirsiniz

---

## Kaynaklar

- **NPM Paketi:** [@senoldogann/context-manager](https://www.npmjs.com/package/@senoldogann/context-manager)
- **English README:** [README.md](./README.md)
- **Baslangic Rehberi:** [GETTING_STARTED.md](./GETTING_STARTED.md)
- **Ornek Ortam Dosyasi:** [.env.example](./.env.example)
- **Hybrid Ranking Notlari:** [docs/hybrid-ranking.md](./docs/hybrid-ranking.md)
- **Katki:** [CONTRIBUTING.md](./CONTRIBUTING.md)

---

## Yildiz Gecmisi

[![Star History Chart](https://api.star-history.com/svg?repos=senoldogann/LLM-Context-Manager&type=Date)](https://star-history.com/#senoldogann/LLM-Context-Manager&Date)

---

## Lisans

MIT License - Acik kaynak ve ucretsiz.
