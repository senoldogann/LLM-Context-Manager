//! Python sözdizimi ağacından referans olguları: çağrı yerleri, import bağları ve
//! taban sınıflar. Yorum ve string içerikleri ağaçta `call` düğümü üretmediği için
//! kenar kaynağı olamaz; iç içe tanımlar kendi düğümlerinin olgularıdır.

use tree_sitter::Node;

use crate::graph::{
    python_package, CallSite, CallTarget, ImportBinding, SyntaxFacts, SyntaxLanguage,
};

/// Fonksiyonun olguları: dekoratörleri ve gövdesi (iç içe tanımlar hariç).
pub fn function_facts(definition: Node, source: &str, file_id: &str) -> SyntaxFacts {
    let package = python_package(file_id);
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Python);
    collect_decorators(definition, source, &package, &mut facts);
    if let Some(body) = definition.child_by_field_name("body") {
        collect_scope(body, source, &package, &mut facts);
    }
    finish(facts)
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
    finish(facts)
}

/// Modül düzeyindeki çağrılar ve importlar (fonksiyon ve sınıf gövdeleri hariç).
pub fn module_facts(root: Node, source: &str, file_id: &str) -> SyntaxFacts {
    let package = python_package(file_id);
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Python);
    collect_scope(root, source, &package, &mut facts);
    finish(facts)
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
        "identifier" => {
            if is_reference_position(node) {
                if let Some(name) = text(node, source) {
                    facts.names.push(name);
                }
            }
        }
        _ => collect_scope(node, source, package, facts),
    }
}

/// Tanımlayıcı çağrılmadan kullanılan bir ad mı? Öznitelik adı (`x.ad`), anahtar
/// argüman adı, çağrının kendisi (çağrı yeri olarak sayılır), atama ve döngü
/// hedefleri ile lambda parametreleri yeni bağ ya da ad değildir.
fn is_reference_position(identifier: Node) -> bool {
    let Some(parent) = identifier.parent() else {
        return false;
    };
    let is_field = |field: &str| parent.child_by_field_name(field) == Some(identifier);
    match parent.kind() {
        "attribute" => !is_field("attribute"),
        "keyword_argument" | "default_parameter" => !is_field("name"),
        "call" => !is_field("function"),
        "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => {
            !is_field("left")
        }
        "lambda_parameters" | "global_statement" | "nonlocal_statement" | "dotted_name"
        | "as_pattern_target" => false,
        _ => true,
    }
}

/// Adları tekilleştirir: bir kaynak aynı adı kaç kez anarsa ansın tek referanstır.
fn finish(mut facts: SyntaxFacts) -> SyntaxFacts {
    facts.names.sort_unstable();
    facts.names.dedup();
    facts
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
        && node
            .child_by_field_name("function")
            .is_some_and(|function| {
                function.kind() == "identifier"
                    && text(function, source).as_deref() == Some("super")
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
