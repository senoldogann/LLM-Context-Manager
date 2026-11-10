//! `context` cevabındaki çağrı yeri satırları ve grep eşdeğeri tamlık denetimi.
//!
//! Pilotlarda ajanlar doğru caller listesini aldıktan sonra aynı `rg` aramasını ve
//! çağrı satırlarını okuyan `sed` komutlarını yine çalıştırdı. Bu modül o işi sunucuda
//! yapar: hedef adın indekslenmiş dosyalardaki her metinsel geçişini bulur, grafın
//! listelediği caller'larla karşılaştırır ve sonucu tek satırda söyler. "Tam" yalnız
//! her geçiş listelenen bir caller'ın içindeyse yazılır; aksi halde dışarıda kalan
//! satırlar gösterilir. Eşleştirme ve sınıflandırma saf fonksiyonlardır; disk okuması
//! yalnız `read_sources` adaptöründedir.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};

/// Çağıran başına gösterilen en çok çağrı satırı.
pub(crate) const SITES_PER_CALLER: usize = 2;
/// Dışarıda kalan geçişlerden gösterilen en çok satır.
const OUTSIDE_LINES: usize = 6;
/// Gösterilen kaynak satırının en çok karakteri.
const SOURCE_LINE_CHARS: usize = 120;

/// Aranan metin: çıplak ad (`ensure_sync`) ya da receiver ifadesi (`req.get`).
/// `only_file` doluysa metin yalnız o dosyada aranır (`this.get` yalnız tanım dosyasında
/// aynı nesnenin metodudur).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Needle {
    pub text: String,
    pub only_file: Option<String>,
}

/// Denetimin neyi saydığı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckKind {
    /// Çıplak ad: çağrılar ve çağrı dışı kullanımlar (`@ad`, `map(ad, …)`) sayılır.
    NameUses,
    /// Receiver ifadeleri: yalnız çağrılar sayılır; `req.get = …` tanımdır.
    MemberCalls,
    /// `self` almayan Rust fonksiyonu: `NameUses` gibi, ama `x.ad(` metot sözdizimi
    /// onu çağıramaz; bu geçişler başka bir tipin metodudur.
    RustNoSelfUses,
}

/// Denetimin tanımı: aranan metinler, sayım türü ve isteğe bağlı yol kapsamı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckSpec {
    pub needles: Vec<Needle>,
    pub kind: CheckKind,
    pub scope: Option<String>,
}

/// Kaynak dili; yorum işareti ve Python import kuralları buna göre seçilir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    Python,
    Rust,
    JavaScript,
    Other,
}

/// Bir geçişin türü.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitKind {
    /// Ardından `(` (Rust'ta isteğe bağlı turbofish) gelen geçiş.
    Call,
    /// Çağrı olmayan kullanım: dekoratör, callback, atama, öznitelik.
    Use,
    /// Projenin kendi modülünden import satırı (`as` olmadan).
    Import,
    /// `as` ile yeniden adlandıran import: takma adla yapılan çağrılar aranmadı.
    AliasImport,
    /// Python'da proje dışı modüle ait geçiş (`re.escape(`, `from html import escape`).
    External,
    /// Dizgi ya da docstring içindeki sözcük: kod değildir.
    Text,
    /// Rust'ta `x.ad(` metot çağrısı; hedef serbest fonksiyon olduğu için başka tiptir.
    OtherTypeMethod,
}

/// Bir dosyadaki tek geçiş; `line` 1 tabanlıdır.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hit {
    pub file: String,
    pub line: usize,
    pub text: String,
    pub kind: HitKind,
}

/// İndeksteki bir sembolün satır aralığı ve içeriği; en içteki kapsamı bulmak ve
/// diskteki satırın indeksle aynı olduğunu doğrulamak için kullanılır.
#[derive(Debug, Clone)]
pub(crate) struct SymbolSpan {
    /// Dal alternatiflerini birleştiren anahtar (bkz. `listed_key`).
    pub key: String,
    pub start: usize,
    pub end: usize,
    pub content: Arc<str>,
}

/// İndekslenmiş bir dosya: yol (`./` olmadan), dosya düğümünün anahtarı ve sembolleri.
#[derive(Debug, Clone)]
pub(crate) struct IndexedFile {
    pub path: String,
    pub file_key: String,
    pub symbols: Vec<SymbolSpan>,
}

/// Denetimin sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckOutcome {
    /// Tarama bitti; `outside` listelenen caller'ların dışında kalan çağrı ve
    /// kullanımlardır (boşsa liste tamdır).
    Checked {
        counts: CheckCounts,
        outside: Vec<Hit>,
    },
    /// Diskteki satır indeksle uyuşmuyor: aralıklar güvenilmez, iddia yazılmaz.
    Stale { file: String, line: usize },
}

/// Denetimin sayıları.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckCounts {
    pub files: usize,
    pub calls: usize,
    pub uses: usize,
    pub external: usize,
    pub external_modules: BTreeSet<String>,
    pub test_hits: usize,
    pub aliases: Vec<Hit>,
    /// Grafın aynı adlı başka bir tanıma bağladığı geçişler ve o tanımların yerleri.
    pub elsewhere: usize,
    pub elsewhere_targets: BTreeSet<String>,
    pub text: usize,
    /// Hedefin kendi gövdesindeki (özyinelemeli) çağrılar.
    pub recursive: usize,
    /// `self` almayan Rust hedefinde `x.ad(` metot çağrıları (başka tip).
    pub other_type_methods: usize,
}

/// Dosya uzantısından dil.
pub(crate) fn lang_of(path: &str) -> Lang {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("py" | "pyi") => Lang::Python,
        Some("rs") => Lang::Rust,
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts") => Lang::JavaScript,
        _ => Lang::Other,
    }
}

fn is_ident_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

/// Satırda `needle` metninin sözcük sınırlı geçişlerinin başlangıç baytları.
/// Öncesinde tanımlayıcı karakteri olmamalı (`.` serbesttir: `self.ad`, `this.req.get`),
/// sonrasında da olmamalı.
pub(crate) fn needle_positions(line: &str, needle: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut from = 0;
    while let Some(found) = line[from..].find(needle) {
        let start = from + found;
        let end = start + needle.len();
        let before_ok = line[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_ident_char(c));
        let after_ok = line[end..].chars().next().is_none_or(|c| !is_ident_char(c));
        if before_ok && after_ok {
            positions.push(start);
        }
        from = end;
    }
    positions
}

/// Geçişin ardından çağrı parantezi geliyor mu: boşluk, Rust turbofish (`::<T>`) ve `(`.
fn followed_by_call(rest: &str, lang: Lang) -> bool {
    let rest = rest.trim_start();
    if rest.starts_with('(') {
        return true;
    }
    if lang == Lang::Rust {
        if let Some(generic) = rest.strip_prefix("::<") {
            let mut depth = 1usize;
            for (index, character) in generic.char_indices() {
                match character {
                    '<' => depth += 1,
                    '>' => {
                        depth -= 1;
                        if depth == 0 {
                            return generic[index + 1..].trim_start().starts_with('(');
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    false
}

/// Geçiş satır yorumunun içinde mi (dizgi içindeki işaretler yaklaşık kabul edilir).
fn in_comment(line: &str, position: usize, lang: Lang) -> bool {
    let prefix = &line[..position];
    let trimmed = line.trim_start();
    match lang {
        Lang::Python => prefix.contains('#'),
        Lang::Rust | Lang::JavaScript => {
            prefix.contains("//") || trimmed.starts_with('*') || trimmed.starts_with("/*")
        }
        Lang::Other => false,
    }
}

/// Python üçlü tırnak durumları: her satırın başında açık bir docstring/üçlü
/// dizginin içinde olunup olunmadığı.
fn triple_quote_states(source: &str, lang: Lang) -> Vec<bool> {
    let mut states = Vec::new();
    let mut open = false;
    for line in source.lines() {
        states.push(open);
        if lang == Lang::Python
            && (line.matches("\"\"\"").count() + line.matches("'''").count()) % 2 == 1
        {
            open = !open;
        }
    }
    states
}

/// Geçiş bir dizginin ya da docstring'in içinde mi (yaklaşık: satır içindeki
/// kaçışsız tırnakların teklik sayısı). Rust'ta `'` ömür işareti olduğu için sayılmaz.
fn in_text(line: &str, position: usize, lang: Lang, doc_open: bool) -> bool {
    let prefix = &line[..position];
    if lang == Lang::Python {
        let triples = prefix.matches("\"\"\"").count() + prefix.matches("'''").count();
        if doc_open ^ (triples % 2 == 1) {
            return true;
        }
    }
    let quotes: &[char] = match lang {
        Lang::Python => &['"', '\''],
        Lang::JavaScript => &['"', '\'', '`'],
        Lang::Rust => &['"'],
        Lang::Other => &[],
    };
    let cleaned = prefix
        .replace("\"\"\"", "")
        .replace("'''", "")
        .replace("\\\\", "")
        .replace("\\\"", "")
        .replace("\\'", "");
    let mut inside: Option<char> = None;
    for character in cleaned.chars() {
        match inside {
            Some(quote) if character == quote => inside = None,
            Some(_) => {}
            None if quotes.contains(&character) => inside = Some(character),
            None => {}
        }
    }
    inside.is_some()
}

/// Geçiş bir tanımın adı mı (`def ad`, `async def ad`, `pub fn ad`, `function ad`).
fn is_definition(line: &str, position: usize) -> bool {
    let prefix = line[..position].trim_end();
    [
        "def",
        "fn",
        "function",
        "class",
        "struct",
        "enum",
        "trait",
        "type",
        "interface",
    ]
    .iter()
    .any(|keyword| {
        prefix
            .strip_suffix(keyword)
            .is_some_and(|before| before.chars().next_back().is_none_or(|c| !is_ident_char(c)))
    })
}

/// Satır bir import bildirimi mi; parantezli Python importunun devam satırları
/// `python_import_lines` ile ayrıca işaretlenir.
fn is_import_line(line: &str, lang: Lang) -> bool {
    let trimmed = line.trim_start();
    match lang {
        Lang::Python => trimmed.starts_with("import ") || trimmed.starts_with("from "),
        Lang::Rust => {
            trimmed.starts_with("use ")
                || trimmed.starts_with("pub use ")
                || trimmed.starts_with("pub(crate) use ")
        }
        Lang::JavaScript => {
            trimmed.starts_with("import ")
                || (trimmed.starts_with("export ") && trimmed.contains(" from "))
                || trimmed.contains("require(")
        }
        Lang::Other => false,
    }
}

/// Geçişten sonra `as` ile yeniden adlandırma var mı (`escape as html_escape`).
fn renamed_after(rest: &str) -> bool {
    rest.trim_start()
        .strip_prefix("as")
        .is_some_and(|after| after.starts_with(char::is_whitespace))
}

/// Python dosyasındaki proje dışı bağlar: modül adları (`import re`) ve adlar
/// (`from html import escape`). Kaynağı belirsiz bağ dış sayılmaz.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PythonExternals {
    pub modules: HashMap<String, String>,
    pub names: HashMap<String, String>,
    /// Parantezli `from … import (` bildiriminin devam satırları (1 tabanlı).
    pub import_lines: HashSet<usize>,
    /// Proje dışı modülden import eden bildirimlerin tüm satırları (1 tabanlı).
    pub external_lines: HashSet<usize>,
}

/// İndekslenmiş Python dosyalarından projenin üst düzey modül adları: `django/utils/x.py`
/// → `django`; `src/flask/app.py` → `flask`; `setup.py` → `setup`.
pub(crate) fn python_roots(paths: &[&str]) -> HashSet<String> {
    paths
        .iter()
        .filter(|path| lang_of(path) == Lang::Python)
        .filter_map(|path| {
            let path = path.strip_prefix("src/").unwrap_or(path);
            let first = path.split('/').next()?;
            Some(first.strip_suffix(".py").unwrap_or(first).to_string())
        })
        .collect()
}

fn module_is_external(module: &str, roots: &HashSet<String>) -> bool {
    if module.starts_with('.') {
        return false;
    }
    let first = module.split('.').next().unwrap_or(module);
    !first.is_empty() && !roots.contains(first)
}

/// Python import bildirimlerinden dış bağları çıkarır. Parantezli çok satırlı
/// `from M import (…)` birleştirilir; `\` devamı ve dinamik import desteklenmez.
pub(crate) fn python_externals(source: &str, roots: &HashSet<String>) -> PythonExternals {
    let mut externals = PythonExternals::default();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let first_line = index + 1;
        if let Some(rest) = trimmed.strip_prefix("import ") {
            for part in rest.split(',') {
                let mut words = part.split_whitespace();
                let Some(module) = words.next() else { continue };
                let bound = match (words.next(), words.next()) {
                    (Some("as"), Some(alias)) => alias.to_string(),
                    _ => module.split('.').next().unwrap_or(module).to_string(),
                };
                if module_is_external(module, roots) {
                    externals.modules.insert(bound, module.to_string());
                    externals.external_lines.insert(first_line);
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from ") {
            let Some((module, names)) = rest.split_once(" import ") else {
                index += 1;
                continue;
            };
            let module = module.trim();
            let mut names = names.to_string();
            if names.trim_start().starts_with('(') && !names.contains(')') {
                while index + 1 < lines.len() {
                    index += 1;
                    externals.import_lines.insert(index + 1);
                    names.push(' ');
                    names.push_str(lines[index]);
                    if lines[index].contains(')') {
                        break;
                    }
                }
            }
            if module_is_external(module, roots) {
                externals.external_lines.extend(first_line..=index + 1);
                let cleaned = names.replace(['(', ')'], " ");
                for part in cleaned.split(',') {
                    let mut words = part.split_whitespace();
                    let Some(name) = words.next() else { continue };
                    let bound = match (words.next(), words.next()) {
                        (Some("as"), Some(alias)) => alias,
                        _ => name,
                    };
                    externals
                        .names
                        .insert(bound.to_string(), module.to_string());
                }
            }
        }
        index += 1;
    }
    externals
}

/// `x.ad` biçiminde, noktadan önceki tanımlayıcı (`re.escape` → `re`); zincirde
/// yalnız son halka alınır (`a.b.escape` → `b`).
fn receiver_before(line: &str, position: usize) -> Option<&str> {
    let prefix = line[..position].strip_suffix('.')?;
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, c)| !is_ident_char(*c))
        .map_or(0, |(index, c)| index + c.len_utf8());
    let receiver = &prefix[start..];
    let chained = prefix[..start].ends_with('.');
    (!receiver.is_empty() && !chained).then_some(receiver)
}

/// Bir dosyanın kaynağındaki geçişler, sınıflandırılmış olarak.
pub(crate) fn file_hits(
    path: &str,
    source: &str,
    spec: &CheckSpec,
    target_name: &str,
    externals: &PythonExternals,
) -> Vec<Hit> {
    let lang = lang_of(path);
    let needles: Vec<&Needle> = spec
        .needles
        .iter()
        .filter(|needle| needle.only_file.as_deref().is_none_or(|file| file == path))
        .collect();
    let mut hits = Vec::new();
    let doc_states = triple_quote_states(source, lang);
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        for needle in &needles {
            for position in needle_positions(line, &needle.text) {
                if in_comment(line, position, lang) || is_definition(line, position) {
                    continue;
                }
                if in_text(line, position, lang, doc_states[index]) {
                    // Dizgideki noktalı yol (`"pkg.mod.ad"`) dinamik referans olabilir.
                    let dotted = line[..position].ends_with('.');
                    if spec.kind != CheckKind::MemberCalls {
                        hits.push(Hit {
                            file: path.to_string(),
                            line: number,
                            text: source_line(line),
                            kind: if dotted { HitKind::Use } else { HitKind::Text },
                        });
                    }
                    continue;
                }
                let rest = &line[position + needle.text.len()..];
                let call = followed_by_call(rest, lang);
                let kind = if is_import_line(line, lang) || externals.import_lines.contains(&number)
                {
                    if externals.external_lines.contains(&number) {
                        HitKind::External
                    } else if renamed_after(rest) {
                        HitKind::AliasImport
                    } else {
                        HitKind::Import
                    }
                } else if lang == Lang::Python
                    && match receiver_before(line, position) {
                        Some(receiver) => externals.modules.contains_key(receiver),
                        None => externals.names.contains_key(target_name),
                    }
                {
                    HitKind::External
                } else if spec.kind == CheckKind::RustNoSelfUses
                    && lang == Lang::Rust
                    && line[..position].ends_with('.')
                {
                    HitKind::OtherTypeMethod
                } else if call {
                    HitKind::Call
                } else {
                    HitKind::Use
                };
                if spec.kind == CheckKind::MemberCalls && kind != HitKind::Call {
                    continue;
                }
                hits.push(Hit {
                    file: path.to_string(),
                    line: number,
                    text: source_line(line),
                    kind,
                });
            }
        }
    }
    hits.dedup_by(|right, left| right.line == left.line && right.kind == left.kind);
    hits
}

/// Rust fonksiyon imzası `self` alıyor mu (`self`, `&self`, `&mut self`, `self: T`).
/// Almayan fonksiyon metot sözdizimiyle (`x.ad(`) çağrılamaz.
/// İmza `fn <ad>` ile bulunur (belge yorumundaki bir `fn ` metni yanıltmasın);
/// imza bulunamazsa `true` döner: metot olabilecek hedefte iddia daralmaz.
pub(crate) fn rust_takes_self(content: &str, name: &str) -> bool {
    let signature = format!("fn {name}");
    let Some(fn_at) = content
        .match_indices(&signature)
        .map(|(index, _)| index)
        .find(|index| {
            let line_start = content[..*index].rfind('\n').map_or(0, |at| at + 1);
            let line = content[line_start..].trim_start();
            !line.starts_with("//")
                && !line.starts_with('*')
                && !line.starts_with("/*")
                && content[index + signature.len()..]
                    .chars()
                    .next()
                    .is_some_and(|next| next == '(' || next == '<' || next.is_whitespace())
        })
    else {
        return true;
    };
    let after = &content[fn_at..];
    let Some(open) = after.find('(') else {
        return false;
    };
    let params = &after[open + 1..];
    let mut depth = 1usize;
    let mut end = params.len();
    for (index, character) in params.char_indices() {
        match character {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => {
                depth -= 1;
                if depth == 0 {
                    end = index;
                    break;
                }
            }
            _ => {}
        }
    }
    let first = params[..end].split(',').next().unwrap_or("").trim();
    let first = first.trim_start_matches('&').trim_start();
    let first = first.strip_prefix("mut ").unwrap_or(first).trim_start();
    let first = first.strip_prefix('\'').map_or(first, |rest| {
        rest.split_once(' ')
            .map_or(rest, |(_, after)| after.trim_start())
    });
    let first = first.strip_prefix("mut ").unwrap_or(first);
    first == "self" || first.starts_with("self:") || first.starts_with("self ")
}

/// Gösterilecek kaynak satırı: baştaki boşluksuz ve sınırlı uzunlukta.
pub(crate) fn source_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= SOURCE_LINE_CHARS {
        return trimmed.to_string();
    }
    let mut clipped: String = trimmed.chars().take(SOURCE_LINE_CHARS).collect();
    clipped.push('…');
    clipped
}

/// İçeriğin ilk satırının dosyadaki numarası. Rust düğümlerinin içeriği tanımdan
/// önceki belge yorumlarını da taşır; `start` ise tanım satırıdır.
pub(crate) fn content_first_line(start: usize, end: usize, content: &str) -> usize {
    let span = end.saturating_sub(start) + 1;
    let lines = content.lines().count();
    start.saturating_sub(lines.saturating_sub(span)).max(1)
}

/// Satırı kapsayan en içteki sembol.
fn innermost(symbols: &[SymbolSpan], line: usize) -> Option<&SymbolSpan> {
    symbols
        .iter()
        .filter(|symbol| symbol.start <= line && line <= symbol.end)
        .min_by_key(|symbol| symbol.end - symbol.start)
}

/// Diskteki satır indeksteki sembol içeriğiyle aynı mı; ilk satır sütun ortasından
/// başlayabildiği için kırpılmış içerik satırın içinde aranır.
fn line_matches_index(symbol: &SymbolSpan, line: usize, disk_line: &str) -> bool {
    let first = content_first_line(symbol.start, symbol.end, &symbol.content);
    let Some(indexed) = symbol.content.lines().nth(line - first) else {
        return false;
    };
    let indexed = indexed.trim();
    if indexed.is_empty() {
        return disk_line.trim().is_empty();
    }
    disk_line.contains(indexed)
}

/// Bir dosyanın kaynağı ve geçişleri; `read_sources` sonucundan üretilir.
pub(crate) struct ScannedFile<'a> {
    pub file: &'a IndexedFile,
    pub source: String,
}

/// İndeksten türetilen, karşılaştırmanın girdileri.
pub(crate) struct IndexFacts {
    /// Listelenen caller ve kullanım düğümlerinin anahtarları.
    pub listed: HashSet<String>,
    /// Projenin üst düzey Python modülleri (`python_roots`).
    pub roots: HashSet<String>,
    /// Kapsamdaki test dışı dosya sayısı.
    pub files_in_scope: usize,
    /// Aynı adlı başka tanıma bağlanan kapsamlar (`kapsam anahtarı → yol:satır`).
    pub elsewhere: HashMap<String, String>,
    /// Hedefin kendi anahtarı: gövdesindeki geçişler özyinelemedir, eksik caller değil.
    pub target_key: String,
}

/// Geçişleri listelenen kullanımlarla karşılaştırır; test dosyalarındaki geçişler
/// yalnız sayılır.
pub(crate) fn reconcile(
    scanned: &[ScannedFile<'_>],
    spec: &CheckSpec,
    target_name: &str,
    facts: &IndexFacts,
    is_test: &dyn Fn(&str) -> bool,
) -> CheckOutcome {
    let IndexFacts {
        listed,
        roots,
        files_in_scope,
        elsewhere,
        target_key,
    } = facts;
    let mut counts = CheckCounts {
        files: *files_in_scope,
        calls: 0,
        uses: 0,
        external: 0,
        external_modules: BTreeSet::new(),
        test_hits: 0,
        aliases: Vec::new(),
        elsewhere: 0,
        elsewhere_targets: BTreeSet::new(),
        text: 0,
        recursive: 0,
        other_type_methods: 0,
    };
    let mut outside = Vec::new();
    for scanned_file in scanned {
        let file = scanned_file.file;
        if is_test(&file.path) {
            let externals = PythonExternals::default();
            counts.test_hits += file_hits(
                &file.path,
                &scanned_file.source,
                spec,
                target_name,
                &externals,
            )
            .iter()
            .filter(|hit| matches!(hit.kind, HitKind::Call | HitKind::Use))
            .count();
            continue;
        }
        let externals = if lang_of(&file.path) == Lang::Python {
            python_externals(&scanned_file.source, roots)
        } else {
            PythonExternals::default()
        };
        let lines: Vec<&str> = scanned_file.source.lines().collect();
        for hit in file_hits(
            &file.path,
            &scanned_file.source,
            spec,
            target_name,
            &externals,
        ) {
            match hit.kind {
                HitKind::External => {
                    counts.external += 1;
                    // Aynı satırdaki her geçişin modülü (`escape(x) + re.escape(x)`).
                    if let Some(line) = lines.get(hit.line - 1) {
                        for position in needle_positions(line, target_name) {
                            let module = match receiver_before(line, position) {
                                Some(receiver) => externals.modules.get(receiver),
                                None => externals.names.get(target_name),
                            };
                            if let Some(module) = module {
                                counts.external_modules.insert(module.clone());
                            }
                        }
                    }
                    continue;
                }
                HitKind::Text => {
                    counts.text += 1;
                    continue;
                }
                HitKind::OtherTypeMethod => {
                    counts.other_type_methods += 1;
                    continue;
                }
                HitKind::AliasImport => {
                    counts.aliases.push(hit);
                    continue;
                }
                HitKind::Import => {
                    if listed.contains(&file.file_key) {
                        continue;
                    }
                    outside.push(hit);
                    continue;
                }
                HitKind::Call | HitKind::Use => {}
            }
            let scope = innermost(&file.symbols, hit.line);
            if let Some(symbol) = scope {
                if !line_matches_index(symbol, hit.line, lines[hit.line - 1]) {
                    return CheckOutcome::Stale {
                        file: file.path.clone(),
                        line: hit.line,
                    };
                }
            }
            let key = scope.map_or(file.file_key.as_str(), |symbol| symbol.key.as_str());
            // Atama zincirinin takma adları (`res.contentType = res.type = function`)
            // aynı satırda biten iç içe düğümlerdir; biri listelenmişse satır onundur.
            let alias_listed = scope.is_some_and(|symbol| {
                file.symbols.iter().any(|other| {
                    other.end == symbol.end
                        && other.start <= symbol.start
                        && listed.contains(&other.key)
                })
            });
            // Hedefin kendi gövdesi (ya da onun içindeki iç ad): özyineleme.
            let inside_target = file.symbols.iter().any(|other| {
                other.key == *target_key && other.start <= hit.line && hit.line <= other.end
            });
            if inside_target && !listed.contains(key) {
                counts.recursive += 1;
                continue;
            }
            if listed.contains(key) || alias_listed {
                if hit.kind == HitKind::Call {
                    counts.calls += 1;
                } else {
                    counts.uses += 1;
                }
                continue;
            }
            match elsewhere.get(key) {
                Some(location) => {
                    counts.elsewhere += 1;
                    counts.elsewhere_targets.insert(location.clone());
                }
                None => outside.push(hit),
            }
        }
    }
    CheckOutcome::Checked { counts, outside }
}

/// Kaynakları diskten okur: yalnız aranan metinlerden birini içeren dosyalar döner.
/// İndekslenmiş bir dosya okunamazsa denetim hatayla durur (sessiz atlama yok).
pub(crate) fn read_sources<'a>(
    root: &Path,
    files: &'a [IndexedFile],
    spec: &CheckSpec,
) -> Result<Vec<ScannedFile<'a>>> {
    let probes: Vec<&str> = spec
        .needles
        .iter()
        .map(|needle| needle.text.rsplit('.').next().unwrap_or(&needle.text))
        .collect();
    let mut scanned = Vec::new();
    for file in files {
        if spec
            .scope
            .as_deref()
            .is_some_and(|scope| !file.path.starts_with(scope))
        {
            continue;
        }
        let full = root.join(&file.path);
        let bytes = std::fs::read(&full)
            .with_context(|| format!("call-site check could not read {}", full.display()))?;
        if !probes.iter().any(|probe| {
            bytes
                .windows(probe.len())
                .any(|window| window == probe.as_bytes())
        }) {
            continue;
        }
        let source = String::from_utf8(bytes)
            .with_context(|| format!("call-site check: {} is not UTF-8", full.display()))?;
        scanned.push(ScannedFile { file, source });
    }
    Ok(scanned)
}

/// Bir caller içeriğindeki çağrı yeri satırları: `  satır: kaynak`, en çok
/// `SITES_PER_CALLER` satır, fazlası `+n more` ile sayılır.
pub(crate) fn caller_site_lines(
    path: &str,
    content: &str,
    start_line: usize,
    end_line: usize,
    spec: &CheckSpec,
    target_name: &str,
) -> Vec<String> {
    let first = content_first_line(start_line, end_line, content);
    let externals = PythonExternals::default();
    let hits: Vec<Hit> = file_hits(path, content, spec, target_name, &externals)
        .into_iter()
        .filter(|hit| matches!(hit.kind, HitKind::Call | HitKind::Use))
        .collect();
    let mut lines: Vec<String> = hits
        .iter()
        .take(SITES_PER_CALLER)
        .map(|hit| format!("  {}: {}", first + hit.line - 1, hit.text))
        .collect();
    if hits.len() > SITES_PER_CALLER {
        lines.push(format!("  +{} more", hits.len() - SITES_PER_CALLER));
    }
    lines
}

/// Aranan metinlerin okunur adı: `` `ensure_sync` `` ya da `` `req.get(`, `this.get(` (lib/request.js) ``.
fn needle_label(spec: &CheckSpec) -> String {
    spec.needles
        .iter()
        .map(|needle| match (&needle.only_file, spec.kind) {
            (Some(file), CheckKind::MemberCalls) => format!("`{}(` ({file})", needle.text),
            (None, CheckKind::MemberCalls) => format!("`{}(`", needle.text),
            (Some(file), CheckKind::NameUses | CheckKind::RustNoSelfUses) => {
                format!("`{}` ({file})", needle.text)
            }
            (None, CheckKind::NameUses | CheckKind::RustNoSelfUses) => format!("`{}`", needle.text),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Denetim sonucunun pakete yazılan satırları.
pub(crate) fn outcome_text(
    spec: &CheckSpec,
    outcome: &CheckOutcome,
    caller_count: usize,
) -> String {
    let label = needle_label(spec);
    let scope = spec
        .scope
        .as_deref()
        .map_or(String::new(), |scope| format!(" under {scope}"));
    let (counts, outside) = match outcome {
        CheckOutcome::Stale { file, line } => {
            return format!(
                "Check skipped: {file}:{line} differs from the index; retry after the index refreshes.\n"
            )
        }
        CheckOutcome::Checked { counts, outside } => (counts, outside),
    };
    let calls_out: Vec<&Hit> = outside
        .iter()
        .filter(|hit| hit.kind == HitKind::Call)
        .collect();
    let uses_out: Vec<&Hit> = outside
        .iter()
        .filter(|hit| hit.kind != HitKind::Call)
        .collect();
    let mut text = if calls_out.is_empty() {
        format!(
            "Check: call list complete. A grep for {label} in the {} indexed non-test files{scope} finds {} call sites, all inside the {caller_count} callers below; no other grep is needed.\n",
            counts.files, counts.calls
        )
    } else {
        let mut text = format!(
            "Check: NOT complete. A grep for {label} in the {} indexed non-test files{scope} finds {} call site(s) outside the callers below; the graph links none of them to this target:\n",
            counts.files,
            calls_out.len()
        );
        text.push_str(&hit_lines(&calls_out));
        text
    };
    if !uses_out.is_empty() {
        text.push_str(&format!(
            "{} non-call use(s) outside the callers (the name is passed, stored or imported, not called there):\n",
            uses_out.len()
        ));
        text.push_str(&hit_lines(&uses_out));
    }
    let suffix = counts_suffix(spec, counts);
    if !suffix.is_empty() {
        text.push_str(suffix.trim_start());
        text.push('\n');
    }
    text
}

/// En çok `OUTSIDE_LINES` geçiş satırı; fazlası sayılır.
fn hit_lines(hits: &[&Hit]) -> String {
    let mut text = String::new();
    for hit in hits.iter().take(OUTSIDE_LINES) {
        text.push_str(&format!("  {}:{}: {}\n", hit.file, hit.line, hit.text));
    }
    if hits.len() > OUTSIDE_LINES {
        text.push_str(&format!("  … and {} more\n", hits.len() - OUTSIDE_LINES));
    }
    text
}

fn counts_suffix(spec: &CheckSpec, counts: &CheckCounts) -> String {
    let mut text = String::new();
    if counts.elsewhere > 0 {
        let targets: Vec<&str> = counts
            .elsewhere_targets
            .iter()
            .map(String::as_str)
            .collect();
        text.push_str(&format!(
            " {} more match(es) are calls the graph resolves to another definition with this name ({}) and are not listed.",
            counts.elsewhere,
            targets.join(", ")
        ));
    }
    if counts.external > 0 {
        let modules: Vec<&str> = counts.external_modules.iter().map(String::as_str).collect();
        text.push_str(&format!(
            " {} more match(es) belong to external modules ({}) and are not listed.",
            counts.external,
            modules.join(", ")
        ));
    }
    for alias in &counts.aliases {
        text.push_str(&format!(
            " Aliased import at {}:{} (`{}`): calls through the alias are not checked.",
            alias.file, alias.line, alias.text
        ));
    }
    if counts.recursive > 0 {
        text.push_str(&format!(
            " {} call(s) inside the target itself (recursion) are not listed.",
            counts.recursive
        ));
    }
    if counts.text > 0 {
        text.push_str(&format!(
            " {} match(es) inside strings or docstrings are not listed.",
            counts.text
        ));
    }
    if counts.test_hits > 0 {
        text.push_str(&format!(
            " {} match(es) in test files are not listed.",
            counts.test_hits
        ));
    }
    if counts.other_type_methods > 0 {
        text.push_str(&format!(
            " {} `.{}(` method call(s) belong to other types (this function takes no `self`) and are not listed.",
            counts.other_type_methods,
            spec.needles.first().map_or("", |needle| needle.text.as_str())
        ));
    }
    match spec.kind {
        CheckKind::NameUses | CheckKind::RustNoSelfUses => text.push_str(
            " Calls that never name it in source (getattr, runtime aliases) are not visible to text or graph.",
        ),
        CheckKind::MemberCalls => text.push_str(
            " Other receivers, destructuring and aliases are not checked.",
        ),
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_spec(name: &str) -> CheckSpec {
        CheckSpec {
            needles: vec![Needle {
                text: name.to_string(),
                only_file: None,
            }],
            kind: CheckKind::NameUses,
            scope: None,
        }
    }

    #[test]
    fn classifies_calls_uses_definitions_comments_and_externals() {
        let roots: HashSet<String> = ["django".to_string()].into_iter().collect();
        let source = "import re\nfrom html import escape as stdlib_escape\nfrom django.utils.html import escape\n\ndef escape(x):\n    # escape(x) in a comment\n    return re.escape(x)\n\n@escape\ndef f():\n    return escape(1) + self.escape (2)\n";
        let externals = python_externals(source, &roots);
        let hits = file_hits(
            "django/a.py",
            source,
            &name_spec("escape"),
            "escape",
            &externals,
        );
        let kinds: Vec<(usize, HitKind)> = hits.iter().map(|hit| (hit.line, hit.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (2, HitKind::External),
                (3, HitKind::Import),
                (7, HitKind::External),
                (9, HitKind::Use),
                (11, HitKind::Call),
            ]
        );
    }

    #[test]
    fn stdlib_from_import_marks_bare_calls_external() {
        let roots: HashSet<String> = ["django".to_string()].into_iter().collect();
        let source = "from html import escape\n\ndef f():\n    return escape(1)\n";
        let externals = python_externals(source, &roots);
        let hits = file_hits(
            "django/t.py",
            source,
            &name_spec("escape"),
            "escape",
            &externals,
        );
        assert!(
            hits.iter().all(|hit| hit.kind == HitKind::External),
            "{hits:?}"
        );
    }

    #[test]
    fn rust_turbofish_and_paths_are_calls() {
        let spec = name_spec("with_bound");
        let source = "pub fn with_bound() {}\nfn a() { bound::with_bound(); with_bound::<T>(x); with_self_bound(); }\n";
        let hits = file_hits(
            "src/b.rs",
            source,
            &spec,
            "with_bound",
            &PythonExternals::default(),
        );
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].kind, HitKind::Call);
        assert_eq!(hits[0].line, 2);
    }

    #[test]
    fn member_needles_count_only_calls() {
        let spec = CheckSpec {
            needles: vec![Needle {
                text: "req.get".to_string(),
                only_file: None,
            }],
            kind: CheckKind::MemberCalls,
            scope: None,
        };
        let source = "req.get = function () {};\nfunction a() { return this.req.get('x') + otherreq.get('y'); }\n";
        let hits = file_hits(
            "lib/r.js",
            source,
            &spec,
            "get",
            &PythonExternals::default(),
        );
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].line, 2);
    }

    #[test]
    fn strings_and_docstrings_are_text_but_dotted_paths_are_uses() {
        let source = "def f():\n    \"\"\"Call escape() here.\n    escape(x) again\n    \"\"\"\n    if name in (\"escape\", \"safe\"):\n        return escape(1)\n    return \"pkg.html.escape\"\n";
        let hits = file_hits(
            "app/s.py",
            source,
            &name_spec("escape"),
            "escape",
            &PythonExternals::default(),
        );
        let kinds: Vec<(usize, HitKind)> = hits.iter().map(|hit| (hit.line, hit.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (2, HitKind::Text),
                (3, HitKind::Text),
                (5, HitKind::Text),
                (6, HitKind::Call),
                (7, HitKind::Use),
            ]
        );
    }

    #[test]
    fn rust_lifetimes_do_not_open_strings() {
        let source = "fn a<'de>(x: &'de str) { with_bound(x) }\n";
        let hits = file_hits(
            "src/a.rs",
            source,
            &name_spec("with_bound"),
            "with_bound",
            &PythonExternals::default(),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, HitKind::Call);
    }

    #[test]
    fn rust_self_detection_and_method_calls_of_other_types() {
        assert!(rust_takes_self("pub fn f(&self, x: u8) {}", "f"));
        assert!(rust_takes_self("fn f(&'a mut self) {}", "f"));
        assert!(rust_takes_self("fn f(self: Box<Self>) {}", "f"));
        assert!(rust_takes_self("/// like fn f(x)\nfn f(&self) {}", "f"));
        assert!(rust_takes_self("fn other(x: u8) {}", "f"));
        assert!(!rust_takes_self(
            "fn deserialize_struct(params: &Parameters, fields: &[Field]) {}",
            "deserialize_struct"
        ));
        assert!(!rust_takes_self("fn f<T: Into<u8>>(selfish: T) {}", "f"));
        let spec = CheckSpec {
            needles: vec![Needle {
                text: "deserialize_struct".to_string(),
                only_file: None,
            }],
            kind: CheckKind::RustNoSelfUses,
            scope: None,
        };
        let source = "fn a() { d.deserialize_struct(1); deserialize_struct(2); }\n";
        let kinds: Vec<HitKind> = file_hits(
            "src/a.rs",
            source,
            &spec,
            "deserialize_struct",
            &PythonExternals::default(),
        )
        .iter()
        .map(|hit| hit.kind)
        .collect();
        assert_eq!(kinds, vec![HitKind::OtherTypeMethod, HitKind::Call]);
    }

    #[test]
    fn parenthesized_python_imports_are_import_lines() {
        let roots: HashSet<String> = ["app".to_string()].into_iter().collect();
        let source = "from app.util import (\n    helper,\n    other,\n)\n";
        let externals = python_externals(source, &roots);
        let hits = file_hits(
            "app/x.py",
            source,
            &name_spec("helper"),
            "helper",
            &externals,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, HitKind::Import);
    }
}
