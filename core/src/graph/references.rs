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

/// Dosya kimliğinin yol bileşenleri; kimlikler `./göreli/yol` biçimindedir ve
/// baştaki `./` bir bileşen değildir.
fn path_components(file_id: &str) -> Vec<String> {
    file_id
        .strip_prefix("./")
        .unwrap_or(file_id)
        .split(['/', '\\'])
        .map(str::to_string)
        .collect()
}

/// Python dosyasının paketi, kök göreli yol bileşenleri olarak
/// (`./app/core.py` → `[app]`, `./app/__init__.py` → `[app]`).
pub fn python_package(file_id: &str) -> Vec<String> {
    let mut parts = path_components(file_id);
    parts.pop();
    parts
}

/// Python dosyasının modül yolu bileşenleri (`./app/core.py` → `[app, core]`,
/// `./app/__init__.py` → `[app]`); `.py` dosyası değilse `None`.
pub fn python_module_path(file_id: &str) -> Option<Vec<String>> {
    let stem = file_id.strip_suffix(".py")?;
    let mut parts = path_components(stem);
    if parts.last().is_some_and(|last| last == "__init__") {
        parts.pop();
    }
    (!parts.is_empty()).then_some(parts)
}
