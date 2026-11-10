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
    /// Sınıf dışındaki Python fonksiyonu: `NameUses` gibi, ama `x.ad(` yalnız `x` o
    /// dosyada bir import'la bağlanmış bir adsa (modül) ona ulaşabilir; yerel nesne,
    /// parametre ya da `self` üzerindeki çağrı başka bir nesnenin metodudur.
    PythonModuleFunctionUses,
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
    /// Kapsamı başka tanıma bağlı geçişler: kapsam bazında (satır bazında değil)
    /// atandıkları için tamlık başlığı onları açıkça gösterir.
    pub elsewhere_hits: Vec<Hit>,
    /// Başka tanıma bağlı olduğu dosyanın import'uyla kanıtlanan geçişler.
    pub elsewhere_proven: usize,
    pub text: usize,
    /// Hedefin kendi gövdesindeki (özyinelemeli) çağrılar.
    pub recursive: usize,
    /// `self` almayan Rust hedefinde `x.ad(` metot çağrıları (başka tip).
    pub other_type_methods: usize,
    /// Sınıf dışı Python fonksiyonunda, caller'ların dışında kalan ve import edilmemiş
    /// bir ad üzerinden yapılan `x.ad(` çağrıları (nesne modül değilse başka metottur).
    pub object_method_hits: Vec<Hit>,
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

/// Satırın bir konumunun bölgesi: kod, dizgi içi ya da satır yorumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Code,
    Text,
    Comment,
}

/// Tarayıcı yığınının çerçevesi: açık dizgi (enterpolasyonlu mu) ya da dizgi
/// içindeki `{…}`/`${…}` kod bölümü (iç içe süslü parantez derinliği).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Str { quote: char, interpolating: bool },
    Interp { depth: usize },
}

/// Python dizgi öneki f-string mi (`f"`, `rf'`, `Fr"`): tırnaktan hemen önceki en
/// çok iki harf `f` içeriyor ve onlardan önce tanımlayıcı karakteri yok.
fn python_fstring_prefix(chars: &[char], quote_at: usize) -> bool {
    let mut start = quote_at;
    while start > 0 && quote_at - start < 2 && chars[start - 1].is_ascii_alphabetic() {
        start -= 1;
    }
    let prefix: String = chars[start..quote_at].iter().collect();
    let standalone = start == 0 || !is_ident_char(chars[start - 1]);
    standalone && prefix.to_ascii_lowercase().contains('f')
}

/// `prefix` sonundaki konumun bölgesi. Yorum işareti yalnız kodda, enterpolasyon
/// yalnız f-string ve JS şablon dizgisinde, Rust karakter sabiti (`'"'`) atlanarak.
fn scan_region(prefix: &str, lang: Lang) -> Region {
    let chars: Vec<char> = prefix.chars().collect();
    let mut stack: Vec<Frame> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        let next = chars.get(index + 1).copied();
        match stack.last().copied() {
            Some(Frame::Str {
                quote,
                interpolating,
            }) => {
                if character == '\\' {
                    index += 2;
                    continue;
                }
                if character == quote {
                    stack.pop();
                } else if interpolating
                    && lang == Lang::JavaScript
                    && character == '$'
                    && next == Some('{')
                {
                    stack.push(Frame::Interp { depth: 1 });
                    index += 2;
                    continue;
                } else if interpolating && lang == Lang::Python && character == '{' {
                    if next == Some('{') {
                        index += 2;
                        continue;
                    }
                    stack.push(Frame::Interp { depth: 1 });
                }
            }
            frame => {
                if lang == Lang::Python && character == '#' {
                    return Region::Comment;
                }
                if matches!(lang, Lang::Rust | Lang::JavaScript)
                    && character == '/'
                    && next == Some('/')
                {
                    return Region::Comment;
                }
                if lang == Lang::Rust && character == '\'' {
                    // Karakter sabiti: `'x'` ya da `'\x'`; ömür (`'a`) kapanmaz, atlanmaz.
                    if next == Some('\\') {
                        if let Some(close) =
                            chars[index + 2..].iter().take(10).position(|c| *c == '\'')
                        {
                            index += close + 3;
                            continue;
                        }
                    } else if chars.get(index + 2) == Some(&'\'') {
                        index += 3;
                        continue;
                    }
                }
                if let Some(Frame::Interp { depth }) = frame {
                    if character == '{' {
                        stack.pop();
                        stack.push(Frame::Interp { depth: depth + 1 });
                    } else if character == '}' {
                        stack.pop();
                        if depth > 1 {
                            stack.push(Frame::Interp { depth: depth - 1 });
                        }
                    }
                }
                let opens = match lang {
                    Lang::Python => character == '"' || character == '\'',
                    Lang::JavaScript => matches!(character, '"' | '\'' | '`'),
                    Lang::Rust => character == '"',
                    Lang::Other => false,
                };
                if opens {
                    let interpolating = match lang {
                        Lang::JavaScript => character == '`',
                        Lang::Python => python_fstring_prefix(&chars, index),
                        Lang::Rust | Lang::Other => false,
                    };
                    stack.push(Frame::Str {
                        quote: character,
                        interpolating,
                    });
                }
            }
        }
        index += 1;
    }
    match stack.last() {
        Some(Frame::Str { .. }) => Region::Text,
        Some(Frame::Interp { .. }) | None => Region::Code,
    }
}

/// Geçiş satır yorumunun ya da blok yorumu satırının içinde mi. Blok yorum satırı
/// `* `, `*/`, `/*` ile başlar; `*slot = f(x)` gibi dereferans kod sayılır.
fn in_comment(line: &str, position: usize, lang: Lang) -> bool {
    let trimmed = line.trim_start();
    let block_line = matches!(lang, Lang::Rust | Lang::JavaScript)
        && (trimmed == "*"
            || trimmed.starts_with("* ")
            || trimmed.starts_with("*/")
            || trimmed.starts_with("/*"));
    block_line || scan_region(&line[..position], lang) == Region::Comment
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

/// Geçiş bir dizginin ya da docstring'in içinde mi. Python üçlü tırnakları
/// satır başı durumundan izlenir; satır içi dizgiler `scan_region` ile.
fn in_text(line: &str, position: usize, lang: Lang, doc_open: bool) -> bool {
    let prefix = &line[..position];
    if lang == Lang::Python {
        let triples = prefix.matches("\"\"\"").count() + prefix.matches("\'\'\'").count();
        if doc_open ^ (triples % 2 == 1) {
            return true;
        }
        let without_triples = prefix.replace("\"\"\"", "").replace("\'\'\'", "");
        return scan_region(&without_triples, lang) == Region::Text;
    }
    scan_region(prefix, lang) == Region::Text
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
    /// Dosyada herhangi bir import'un bağladığı adlar (iç ve dış modüller).
    pub imported: HashSet<String>,
    /// Proje dışı modülden import eden bildirimlerin tüm satırları (1 tabanlı).
    pub external_lines: HashSet<usize>,
}

/// İndekslenmiş Python dosyalarından projenin içe aktarılabilir üst düzey adları:
/// bir dosyanın `__init__.py` taşıyan kesintisiz üst dizin zincirinin en üstteki
/// dizini (`lib/mypkg/util.py` → `mypkg`, `django/utils/html.py` → `django`); paket
/// dışındaki dosyada dosyanın kendisi (`setup.py` → `setup`).
pub(crate) fn python_roots(paths: &[&str]) -> HashSet<String> {
    let packages: HashSet<&str> = paths
        .iter()
        .filter_map(|path| path.strip_suffix("/__init__.py"))
        .collect();
    paths
        .iter()
        .filter(|path| lang_of(path) == Lang::Python)
        .filter_map(|path| {
            let (mut directory, file) = match path.rsplit_once('/') {
                Some((directory, file)) => (Some(directory), file),
                None => (None, *path),
            };
            let mut root: Option<&str> = None;
            while let Some(current) = directory.filter(|dir| packages.contains(dir)) {
                root = Some(current.rsplit('/').next().unwrap_or(current));
                directory = current.rsplit_once('/').map(|(parent, _)| parent);
            }
            let name = root.unwrap_or_else(|| file.strip_suffix(".py").unwrap_or(file));
            (!name.is_empty()).then(|| name.to_string())
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
                externals.imported.insert(bound.clone());
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
            let external = module_is_external(module, roots);
            if external {
                externals.external_lines.extend(first_line..=index + 1);
            }
            let cleaned = names.replace(['(', ')'], " ");
            for part in cleaned.split(',') {
                let mut words = part.split_whitespace();
                let Some(name) = words.next() else { continue };
                let bound = match (words.next(), words.next()) {
                    (Some("as"), Some(alias)) => alias,
                    _ => name,
                };
                // İç modüllerden alınan adlar da (`from django.utils import http`) modül
                // olabilir: `http.ad(` çağrısı başka nesne sayılmaz.
                externals.imported.insert(bound.to_string());
                if external {
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

/// Python dosyasında `from M import ad` (parantezli ya da `as ad` dahil) ile adın
/// bağlandığı modül; yoksa `None`. Göreli modül (`.x`) olduğu gibi döner.
fn python_import_module(source: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some((module, names)) = trimmed
            .strip_prefix("from ")
            .and_then(|rest| rest.split_once(" import "))
        {
            let mut names = names.to_string();
            if names.trim_start().starts_with('(') {
                while !names.contains(')') && index + 1 < lines.len() {
                    index += 1;
                    names.push(' ');
                    names.push_str(lines[index]);
                }
            }
            let cleaned = names.replace(['(', ')'], " ");
            let binds = cleaned.split(',').any(|part| {
                let words: Vec<&str> = part.split_whitespace().collect();
                match words.as_slice() {
                    [imported] => *imported == name,
                    [_, "as", alias] => *alias == name,
                    _ => false,
                }
            });
            if binds {
                return Some(module.trim().to_string());
            }
        }
        index += 1;
    }
    None
}

/// Python dosya yolunun modül adı: `src/` öneki ve `__init__` atılır
/// (`django/template/defaultfilters.py` → `django.template.defaultfilters`).
fn python_module_of_path(path: &str) -> String {
    let path = path.trim_start_matches("./");
    let path = path.strip_prefix("src/").unwrap_or(path);
    let path = path.strip_suffix(".py").unwrap_or(path);
    let path = path.strip_suffix("/__init__").unwrap_or(path);
    path.replace('/', ".")
}

/// Dosya adı, `yol:satır` konumundaki başka tanımın modülünden mi içe aktarıyor.
/// Göreli import yalnız aynı paketteki modül adıyla eşleşir (son bileşen).
fn import_binds_to_definition(source: &str, name: &str, location: &str) -> bool {
    let Some(module) = python_import_module(source, name) else {
        return false;
    };
    let path = location.rsplit_once(':').map_or(location, |(path, _)| path);
    let definition = python_module_of_path(path);
    match module.strip_prefix('.') {
        Some(relative) => {
            !relative.is_empty()
                && !relative.starts_with('.')
                && definition.ends_with(&format!(".{relative}"))
        }
        None => module == definition,
    }
}

/// Rust standart kütüphane kökleri: bu kökle başlayan yol çalışma alanındaki bir
/// fonksiyona ulaşamaz.
const RUST_STD_ROOTS: [&str; 3] = ["std", "core", "alloc"];

/// Rust dosyasında `use std::…` bildirimlerinin bağladığı adlar → tam yol
/// (`use std::panic;` → `panic` → `std::panic`; `use std::{self as s, panic::{self, X}}`).
/// Yalnız standart kütüphane kökleri izlenir; çalışma alanı yolları dokunulmaz kalır.
pub(crate) fn rust_std_aliases(source: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    let mut rest = source;
    while let Some(start) = rest.find("use ") {
        let before_ok = rest[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_ident_char(c));
        let after = &rest[start + 4..];
        let Some(end) = after.find(';') else { break };
        if before_ok {
            let statement: String = after[..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let statement = statement.trim_start_matches(':');
            if RUST_STD_ROOTS
                .iter()
                .any(|root| statement.starts_with(&format!("{root}::")))
            {
                expand_use_tree("", statement, &mut aliases);
            }
        }
        rest = &after[end + 1..];
    }
    aliases
}

/// Bir `use` ağacını (`a::{b, c::{self, D as E}}`) bağlanan ad → tam yol olarak açar.
fn expand_use_tree(prefix: &str, tree: &str, aliases: &mut HashMap<String, String>) {
    let tree = tree.trim();
    if let Some(open) = tree.find('{') {
        let head = tree[..open].trim().trim_end_matches("::");
        let body = tree[open + 1..].trim_end().trim_end_matches('}');
        let base = join_path(prefix, head);
        let mut depth = 0usize;
        let mut part_start = 0;
        for (index, character) in body.char_indices() {
            match character {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    expand_use_tree(&base, &body[part_start..index], aliases);
                    part_start = index + 1;
                }
                _ => {}
            }
        }
        expand_use_tree(&base, &body[part_start..], aliases);
        return;
    }
    let (path, alias) = match tree.split_once(" as ") {
        Some((path, alias)) => (path.trim(), Some(alias.trim())),
        None => (tree, None),
    };
    if path.is_empty() || path == "*" || path.ends_with("::*") {
        return;
    }
    let full = if path == "self" {
        prefix.to_string()
    } else {
        join_path(prefix, path)
    };
    let bound = alias.unwrap_or_else(|| full.rsplit("::").next().unwrap_or(&full));
    if !bound.is_empty() && bound != "_" {
        aliases.insert(bound.to_string(), full);
    }
}

fn join_path(prefix: &str, path: &str) -> String {
    match (prefix.is_empty(), path.is_empty()) {
        (true, _) => path.to_string(),
        (_, true) => prefix.to_string(),
        _ => format!("{prefix}::{path}"),
    }
}

/// Geçişten önceki Rust yol niteleyicisi (`std::panic::catch_unwind` → `std::panic`);
/// `<T as Trait>::` gibi nitelikli tip yolları için `None`.
fn rust_qualifier(line: &str, position: usize) -> Option<&str> {
    let prefix = line[..position].strip_suffix("::")?;
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, c)| !(is_ident_char(*c) || *c == ':'))
        .map_or(0, |(index, c)| index + c.len_utf8());
    let qualifier = prefix[start..].trim_start_matches("::");
    (!qualifier.is_empty() && !prefix[..start].ends_with('>')).then_some(qualifier)
}

/// Niteleyici standart kütüphaneye çıkıyorsa onun tam yolu (`panic` → `std::panic`).
fn rust_external_module(qualifier: &str, aliases: &HashMap<String, String>) -> Option<String> {
    let (root, rest) = qualifier
        .split_once("::")
        .map_or((qualifier, ""), |(root, rest)| (root, rest));
    if RUST_STD_ROOTS.contains(&root) {
        return Some(qualifier.to_string());
    }
    aliases.get(root).map(|full| join_path(full, rest))
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
                    // Python dizgisindeki noktalı yol (`"pkg.mod.ad"`) dinamik referans olabilir
                    // (`import_string`); diğer dillerde dizgi içi eşleşme metindir
                    // (`trace_span!("runtime.resource.async_op")`).
                    let dotted = lang == Lang::Python && line[..position].ends_with('.');
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
                        // `self.helper.ad(` ya da `obj().ad(`: zincir ya da çağrı alıcısı
                        // dış modül adı değildir.
                        None => {
                            !line[..position].ends_with('.')
                                && externals.names.contains_key(target_name)
                        }
                    }
                {
                    HitKind::External
                } else if lang == Lang::Rust
                    && rust_qualifier(line, position)
                        .and_then(|qualifier| rust_external_module(qualifier, &externals.modules))
                        .is_some()
                {
                    // `std::panic::catch_unwind(` ya da `use std::panic;` sonrası
                    // `panic::catch_unwind(`: standart kütüphanenin fonksiyonudur.
                    HitKind::External
                } else if spec.kind == CheckKind::RustNoSelfUses
                    && lang == Lang::Rust
                    && line[..position].ends_with('.')
                {
                    HitKind::OtherTypeMethod
                } else if spec.kind == CheckKind::PythonModuleFunctionUses
                    && lang == Lang::Python
                    && receiver_before(line, position)
                        .is_some_and(|receiver| !externals.imported.contains(receiver))
                {
                    // Zincirli alıcı (`a.b.ad(`) modül yolu olabilir; sayılmaya devam eder.
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
    // İçeriğin ilk satırı sütun ortasından başlayabilir; diğerleri birebir eşleşmeli.
    if line == first {
        disk_line.trim_end().ends_with(indexed.trim())
    } else {
        disk_line.trim() == indexed.trim()
    }
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
        elsewhere_hits: Vec::new(),
        elsewhere_proven: 0,
        text: 0,
        recursive: 0,
        other_type_methods: 0,
        object_method_hits: Vec::new(),
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
        let externals = match lang_of(&file.path) {
            Lang::Python => python_externals(&scanned_file.source, roots),
            Lang::Rust => PythonExternals {
                modules: rust_std_aliases(&scanned_file.source),
                ..PythonExternals::default()
            },
            Lang::JavaScript | Lang::Other => PythonExternals::default(),
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
                            let module = if lang_of(&file.path) == Lang::Rust {
                                rust_qualifier(line, position).and_then(|qualifier| {
                                    rust_external_module(qualifier, &externals.modules)
                                })
                            } else {
                                match receiver_before(line, position) {
                                    Some(receiver) => externals.modules.get(receiver).cloned(),
                                    None => externals.names.get(target_name).cloned(),
                                }
                            };
                            if let Some(module) = module {
                                counts.external_modules.insert(module);
                            }
                        }
                    }
                    continue;
                }
                HitKind::Text => {
                    counts.text += 1;
                    continue;
                }
                HitKind::OtherTypeMethod if spec.kind == CheckKind::PythonModuleFunctionUses => {
                    // Python'da parametre ya da yerel nesne bir modül de olabilir: satır
                    // gizlenmez, listelenmiş caller'ların dışındaysa başlıkta gösterilir.
                    let scope = innermost(&file.symbols, hit.line);
                    let key = scope.map_or(file.file_key.as_str(), |symbol| symbol.key.as_str());
                    if !listed.contains(key) {
                        counts.object_method_hits.push(hit);
                    }
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
                Some(location)
                    if lang_of(&file.path) == Lang::Python
                        && import_binds_to_definition(
                            &scanned_file.source,
                            target_name,
                            location,
                        ) =>
                {
                    // Dosya adı o tanımın modülünden içe aktarıyor: satırın hedefe
                    // ait olmadığı kanıtlı, tamlık başlığını koşullu yapmaz.
                    counts.elsewhere += 1;
                    counts.elsewhere_targets.insert(location.clone());
                    counts.elsewhere_proven += 1;
                }
                Some(location) => {
                    counts.elsewhere += 1;
                    counts.elsewhere_targets.insert(location.clone());
                    counts.elsewhere_hits.push(hit);
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

/// İndekste olmayan bir adın metin taraması: kaynaktaki tanım satırları, test dışı
/// dosyalardaki çağrılar ve sayılmayan geçişler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnindexedReport {
    pub definitions: Vec<(String, usize)>,
    pub calls: Vec<Hit>,
    pub test_hits: usize,
    pub external: usize,
}

/// Satır adın bir tanımı mı (`fn ad`, `def ad`, `function ad`, `class ad`, `struct ad`).
fn defines_name(line: &str, name: &str) -> bool {
    needle_positions(line, name)
        .into_iter()
        .any(|position| is_definition(line, position) && !in_comment(line, position, Lang::Other))
}

/// Adı indekste olmayan bir sembol için taranan dosyalardan rapor üretir. Çağrı
/// sınıflandırması denetimle aynıdır (dizgi, yorum, dış modül ayrılır).
pub(crate) fn unindexed_name_report(
    scanned: &[ScannedFile<'_>],
    name: &str,
    roots: &HashSet<String>,
    is_test: &dyn Fn(&str) -> bool,
) -> UnindexedReport {
    let spec = CheckSpec {
        needles: vec![Needle {
            text: name.to_string(),
            only_file: None,
        }],
        kind: CheckKind::NameUses,
        scope: None,
    };
    let mut report = UnindexedReport {
        definitions: Vec::new(),
        calls: Vec::new(),
        test_hits: 0,
        external: 0,
    };
    for scanned_file in scanned {
        let path = &scanned_file.file.path;
        for (index, line) in scanned_file.source.lines().enumerate() {
            if defines_name(line, name) {
                report.definitions.push((path.clone(), index + 1));
            }
        }
        let externals = match lang_of(path) {
            Lang::Python => python_externals(&scanned_file.source, roots),
            Lang::Rust => PythonExternals {
                modules: rust_std_aliases(&scanned_file.source),
                ..PythonExternals::default()
            },
            Lang::JavaScript | Lang::Other => PythonExternals::default(),
        };
        for hit in file_hits(path, &scanned_file.source, &spec, name, &externals) {
            match hit.kind {
                HitKind::Call if is_test(path) => report.test_hits += 1,
                HitKind::Call => report.calls.push(hit),
                HitKind::External => report.external += 1,
                _ => {}
            }
        }
    }
    report
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
            (
                Some(file),
                CheckKind::NameUses
                | CheckKind::RustNoSelfUses
                | CheckKind::PythonModuleFunctionUses,
            ) => {
                format!("`{}` ({file})", needle.text)
            }
            (
                None,
                CheckKind::NameUses
                | CheckKind::RustNoSelfUses
                | CheckKind::PythonModuleFunctionUses,
            ) => format!("`{}`", needle.text),
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
    let elsewhere_calls: Vec<&Hit> = counts
        .elsewhere_hits
        .iter()
        .filter(|hit| hit.kind == HitKind::Call)
        .collect();
    let mut text = if !counts.aliases.is_empty() {
        let aliases: Vec<&Hit> = counts.aliases.iter().collect();
        let mut text = "Check: NOT complete. It is imported under another name; calls through that alias are not checked:\n".to_string();
        text.push_str(&hit_lines(&aliases));
        text
    } else if spec.kind == CheckKind::MemberCalls && counts.calls == 0 {
        format!(
            "Check: no call of {label} found in the {} indexed non-test files{scope}; calls through other names or receivers are not checked.\n",
            counts.files
        )
    } else if calls_out.is_empty() && !elsewhere_calls.is_empty() {
        let mut text = format!(
            "Check: call list complete if the graph is right about {} line(s) it resolves to another definition with this name ({}); every other call site of {label} in the {} indexed non-test files{scope} is inside the {caller_count} callers below:\n",
            elsewhere_calls.len(),
            counts.elsewhere_targets.iter().map(String::as_str).collect::<Vec<_>>().join(", "),
            counts.files
        );
        text.push_str(&hit_lines(&elsewhere_calls));
        text
    } else if calls_out.is_empty() && !counts.object_method_hits.is_empty() {
        let objects: Vec<&Hit> = counts.object_method_hits.iter().collect();
        let mut text = format!(
            "Check: call list complete for direct and module-qualified calls: a grep for {label} in the {} indexed non-test files{scope} finds {} such call sites, all inside the {caller_count} callers below. {} `.{}(` call(s) on parameters or local objects reach this module-level function only if that object is its module:\n",
            counts.files,
            counts.calls,
            objects.len(),
            spec.needles.first().map_or("", |needle| needle.text.as_str())
        );
        text.push_str(&hit_lines(&objects));
        text
    } else if calls_out.is_empty() {
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
    // Nesne üzerindeki çağrılar başlık bunları anlatmadıysa da gösterilir: hiçbir
    // başlık dalında gizli kalmazlar.
    if !counts.object_method_hits.is_empty() && !text.contains("on parameters or local objects") {
        let objects: Vec<&Hit> = counts.object_method_hits.iter().collect();
        text.push_str(&format!(
            "{} `.{}(` call(s) on parameters or local objects reach this module-level function only if that object is its module:\n",
            objects.len(),
            spec.needles.first().map_or("", |needle| needle.text.as_str())
        ));
        text.push_str(&hit_lines(&objects));
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
    let elsewhere_uses = counts
        .elsewhere_hits
        .iter()
        .filter(|hit| hit.kind != HitKind::Call)
        .count();
    if counts.elsewhere_proven > 0 {
        text.push_str(&format!(
            " {} match(es) are in files that import this name from another definition ({}) and are not listed.",
            counts.elsewhere_proven,
            counts
                .elsewhere_targets
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if elsewhere_uses > 0 {
        text.push_str(&format!(
            " {elsewhere_uses} other use(s) sit in functions the graph links to another definition with this name."
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
            spec.needles
                .first()
                .map_or("", |needle| needle.text.as_str())
        ));
    }
    match spec.kind {
        CheckKind::NameUses
        | CheckKind::RustNoSelfUses
        | CheckKind::PythonModuleFunctionUses => text.push_str(
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
    fn calls_in_interpolations_and_after_string_markers_are_calls() {
        let ext = PythonExternals::default();
        let kinds = |path: &str, source: &str, name: &str| -> Vec<HitKind> {
            file_hits(path, source, &name_spec(name), name, &ext)
                .iter()
                .map(|hit| hit.kind)
                .collect()
        };
        assert_eq!(
            kinds("a.py", "x = f\"{escape(v)}\"\n", "escape"),
            vec![HitKind::Call]
        );
        assert_eq!(
            kinds("a.py", "x = f\"{{escape(v)}}\"\n", "escape"),
            vec![HitKind::Text]
        );
        assert_eq!(
            kinds("a.js", "s = `a ${build(x)}`;\n", "build"),
            vec![HitKind::Call]
        );
        assert_eq!(
            kinds("a.py", "return '#' + escape(x)\n", "escape"),
            vec![HitKind::Call]
        );
        assert_eq!(
            kinds("a.js", "return 'http://a/' + build(x);\n", "build"),
            vec![HitKind::Call]
        );
        assert_eq!(
            kinds("a.rs", "    *slot = compute(x);\n", "compute"),
            vec![HitKind::Call]
        );
        assert_eq!(
            kinds("a.rs", "if c == '\"' { parse_str(x) }\n", "parse_str"),
            vec![HitKind::Call]
        );
        assert!(kinds("a.rs", "     * compute(x) in a doc block\n", "compute").is_empty());
        assert!(kinds("a.py", "y = 1  # escape(x)\n", "escape").is_empty());
    }

    #[test]
    fn chained_receivers_are_not_external_modules() {
        let roots: HashSet<String> = ["app".to_string()].into_iter().collect();
        let source = "from html import escape\n\ndef f(self):\n    return self.helper.escape(1) + obj().escape(2)\n";
        let externals = python_externals(source, &roots);
        let hits = file_hits(
            "app/t.py",
            source,
            &name_spec("escape"),
            "escape",
            &externals,
        );
        let calls: Vec<HitKind> = hits.iter().skip(1).map(|hit| hit.kind).collect();
        assert_eq!(calls, vec![HitKind::Call]);
    }

    #[test]
    fn python_roots_follow_package_directories() {
        let paths = [
            "lib/mypkg/__init__.py",
            "lib/mypkg/util.py",
            "django/__init__.py",
            "django/utils/__init__.py",
            "django/utils/html.py",
            "setup.py",
        ];
        let roots = python_roots(&paths);
        for expected in ["mypkg", "django", "setup"] {
            assert!(roots.contains(expected), "{roots:?}");
        }
        assert!(
            !roots.contains("html") && !roots.contains("lib"),
            "{roots:?}"
        );
    }

    #[test]
    fn imports_prove_lines_of_another_definition() {
        let source = "from django.template.defaultfilters import capfirst, linebreaksbr\n";
        assert!(import_binds_to_definition(
            source,
            "capfirst",
            "django/template/defaultfilters.py:73"
        ));
        assert!(!import_binds_to_definition(
            source,
            "capfirst",
            "django/utils/text.py:25"
        ));
        assert!(import_binds_to_definition(
            "from .defaultfilters import (\n    capfirst,\n)\n",
            "capfirst",
            "django/template/defaultfilters.py:73"
        ));
        assert!(!import_binds_to_definition(
            "import x\n",
            "capfirst",
            "a/b.py:1"
        ));
    }

    #[test]
    fn python_method_calls_on_local_objects_are_not_module_function_calls() {
        let roots: HashSet<String> = ["django".to_string()].into_iter().collect();
        let source = "from django.utils import http\nfrom django.utils.http import urlencode\n\ndef f(request, query_dict):\n    a = urlencode(x)\n    b = http.urlencode(x)\n    c = query_dict.urlencode()\n    d = self.urlencode()\n    e = django.utils.http.urlencode(x)\n";
        let spec = CheckSpec {
            needles: vec![Needle {
                text: "urlencode".to_string(),
                only_file: None,
            }],
            kind: CheckKind::PythonModuleFunctionUses,
            scope: None,
        };
        let externals = python_externals(source, &roots);
        let kinds: Vec<HitKind> = file_hits("django/x.py", source, &spec, "urlencode", &externals)
            .iter()
            .filter(|hit| hit.line > 2)
            .map(|hit| hit.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                HitKind::Call,
                HitKind::Call,
                HitKind::OtherTypeMethod,
                HitKind::OtherTypeMethod,
                HitKind::Call
            ]
        );
    }

    #[test]
    fn rust_std_qualified_calls_are_external() {
        let source = "use std::panic;\nuse std::{sync::Arc, panic::{self as p, AssertUnwindSafe}};\n\nfn f() {\n    panic::catch_unwind(x);\n    std::panic::catch_unwind(x);\n    p::catch_unwind(x);\n    catch_unwind(x);\n    crate::util::catch_unwind(x);\n}\n";
        let aliases = rust_std_aliases(source);
        assert_eq!(aliases.get("panic").map(String::as_str), Some("std::panic"));
        assert_eq!(aliases.get("p").map(String::as_str), Some("std::panic"));
        assert_eq!(
            aliases.get("Arc").map(String::as_str),
            Some("std::sync::Arc")
        );
        let externals = PythonExternals {
            modules: aliases,
            ..PythonExternals::default()
        };
        let kinds: Vec<HitKind> = file_hits(
            "src/a.rs",
            source,
            &name_spec("catch_unwind"),
            "catch_unwind",
            &externals,
        )
        .iter()
        .map(|hit| hit.kind)
        .collect();
        assert_eq!(
            kinds,
            vec![
                HitKind::External,
                HitKind::External,
                HitKind::External,
                HitKind::Call,
                HitKind::Call
            ]
        );
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
