//! Rust sözdizimi ağacından referans olguları: çağrı yerleri, `use` bağları, trait
//! tabanları ve tür adları.
//!
//! Yol bileşenleri `.` ile birleştirilir: `crate::a::B::f()` →
//! `Member { qualifier: "crate.a.B", name: "f" }`. Yorum ve string içerikleri
//! çağrı üretmez; makro argümanları token ağacından taranır; iç içe tanımlar
//! kendi düğümlerinin olgularıdır.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::graph::{CallSite, CallTarget, ImportBinding, SyntaxFacts, SyntaxLanguage};

/// Kendi düğümü olan iç içe tanımlar; kapsayan düğümün olgusu değildir.
const NESTED_ITEMS: [&str; 7] = [
    "function_item",
    "impl_item",
    "struct_item",
    "enum_item",
    "trait_item",
    "mod_item",
    "macro_definition",
];

/// Çağrı ya da referans içermeyen düğümler.
const INERT: [&str; 7] = [
    "attribute_item",
    "inner_attribute_item",
    "line_comment",
    "block_comment",
    "string_literal",
    "raw_string_literal",
    "char_literal",
];

/// Fonksiyonun olguları: imza türleri ve gövdesi (iç içe tanımlar hariç).
pub fn function_facts(definition: Node, source: &str) -> SyntaxFacts {
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Rust);
    let generics = type_parameter_names(definition, source);
    for field in ["parameters", "return_type"] {
        if let Some(node) = definition.child_by_field_name(field) {
            collect_types(node, source, &generics, &mut facts);
        }
    }
    collect_where_clause(definition, source, &generics, &mut facts);
    if let Some(body) = definition.child_by_field_name("body") {
        collect_children(body, source, &generics, &mut facts);
    }
    finish(facts)
}

/// `impl`, `struct`, `enum`, `trait` ve gövdesiz trait metodunun olguları: trait
/// tabanları ve tür adları. `impl` bloğunun metotları kendi düğümleridir.
pub fn item_facts(definition: Node, source: &str) -> SyntaxFacts {
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Rust);
    let generics = type_parameter_names(definition, source);
    match definition.kind() {
        "impl_item" => {
            if let Some(trait_node) = definition.child_by_field_name("trait") {
                if let Some(base) = type_target(trait_node, source) {
                    facts.bases.push(base);
                }
                if let Some(arguments) = trait_node.child_by_field_name("type_arguments") {
                    collect_types(arguments, source, &generics, &mut facts);
                }
            }
            if let Some(type_node) = definition.child_by_field_name("type") {
                collect_types(type_node, source, &generics, &mut facts);
            }
        }
        "trait_item" => {
            if let Some(bounds) = definition.child_by_field_name("bounds") {
                let mut cursor = bounds.walk();
                for bound in bounds.named_children(&mut cursor) {
                    if let Some(base) = type_target(bound, source) {
                        facts.bases.push(base);
                    }
                }
            }
        }
        "struct_item" | "enum_item" => {
            if let Some(body) = definition.child_by_field_name("body") {
                collect_types(body, source, &generics, &mut facts);
            }
        }
        "function_signature_item" => {
            for field in ["parameters", "return_type"] {
                if let Some(node) = definition.child_by_field_name(field) {
                    collect_types(node, source, &generics, &mut facts);
                }
            }
        }
        _ => {}
    }
    collect_where_clause(definition, source, &generics, &mut facts);
    finish(facts)
}

/// Dosyanın ya da satır içi `mod` bloğunun olguları: `use` bağları ve
/// `const`/`static` değerlerindeki çağrılar.
pub fn module_facts(scope: Node, source: &str) -> SyntaxFacts {
    let mut facts = SyntaxFacts::empty(SyntaxLanguage::Rust);
    let items = match scope.kind() {
        "mod_item" => scope.child_by_field_name("body"),
        _ => Some(scope),
    };
    let no_generics = HashSet::new();
    if let Some(items) = items {
        let mut cursor = items.walk();
        for child in items.children(&mut cursor) {
            match child.kind() {
                "use_declaration" => facts.imports.extend(use_bindings(child, source)),
                "const_item" | "static_item" => {
                    if let Some(type_node) = child.child_by_field_name("type") {
                        collect_types(type_node, source, &no_generics, &mut facts);
                    }
                    if let Some(value) = child.child_by_field_name("value") {
                        collect_node(value, source, &no_generics, &mut facts);
                    }
                }
                _ => {}
            }
        }
    }
    finish(facts)
}

fn collect_children(node: Node, source: &str, generics: &HashSet<String>, facts: &mut SyntaxFacts) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, source, generics, facts);
    }
}

fn collect_node(node: Node, source: &str, generics: &HashSet<String>, facts: &mut SyntaxFacts) {
    let kind = node.kind();
    if NESTED_ITEMS.contains(&kind) || INERT.contains(&kind) {
        return;
    }
    match kind {
        "use_declaration" => facts.imports.extend(use_bindings(node, source)),
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                if let Some(target) = call_target(function, source) {
                    facts.calls.push(CallSite {
                        target,
                        line: node.start_position().row + 1,
                    });
                }
                collect_callee(function, source, generics, facts);
            }
            if let Some(arguments) = node.child_by_field_name("arguments") {
                collect_children(arguments, source, generics, facts);
            }
        }
        "macro_invocation" => {
            // Makronun adı çağrı değildir; argümanlar token ağacında taranır.
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "token_tree" {
                    scan_token_tree(child, source, facts);
                }
            }
        }
        "struct_expression" => {
            if let Some(target) = node
                .child_by_field_name("name")
                .and_then(|name| type_target(name, source))
            {
                facts.calls.push(CallSite {
                    target,
                    line: node.start_position().row + 1,
                });
            }
            if let Some(body) = node.child_by_field_name("body") {
                collect_children(body, source, generics, facts);
            }
        }
        // Çağrı olmayan değer yolu (`Mode::Slow`): ad olarak kaydedilir.
        "scoped_identifier" => {
            if let Some(path) = path_segments(node, source) {
                facts.names.push(path.join("."));
            }
        }
        "type_identifier" | "scoped_type_identifier" | "generic_type" => {
            collect_types(node, source, generics, facts);
        }
        // Yalın tanımlayıcılar çoğunlukla yereldir; kenar üretmez.
        "identifier" => {}
        _ => collect_children(node, source, generics, facts),
    }
}

/// Çağrılan ifadenin içindeki alıcı çağrıları ve tür argümanları
/// (`a.b().c()` içindeki `a.b()`, `f::<Tür>()` içindeki `Tür`).
fn collect_callee(
    function: Node,
    source: &str,
    generics: &HashSet<String>,
    facts: &mut SyntaxFacts,
) {
    match function.kind() {
        "field_expression" => {
            if let Some(value) = function.child_by_field_name("value") {
                collect_node(value, source, generics, facts);
            }
        }
        "generic_function" => {
            if let Some(arguments) = function.child_by_field_name("type_arguments") {
                collect_types(arguments, source, generics, facts);
            }
            if let Some(inner) = function.child_by_field_name("function") {
                collect_callee(inner, source, generics, facts);
            }
        }
        _ => {}
    }
}

/// Çağrılan ifadenin hedef biçimi; çözülemeyen biçimler (`(f)()`, `x[0]()`)
/// için `None`.
fn call_target(function: Node, source: &str) -> Option<CallTarget> {
    match function.kind() {
        "identifier" => text(function, source).map(CallTarget::Bare),
        "scoped_identifier" => {
            let segments = path_segments(function, source)?;
            path_call_target(&segments)
        }
        "field_expression" => {
            let field = function.child_by_field_name("field")?;
            if field.kind() != "field_identifier" {
                return None;
            }
            let name = text(field, source)?;
            let receiver = function.child_by_field_name("value")?;
            if receiver.kind() == "self" {
                Some(CallTarget::SelfMember(name))
            } else {
                Some(CallTarget::Chained(name))
            }
        }
        "generic_function" => call_target(function.child_by_field_name("function")?, source),
        _ => None,
    }
}

/// `a::b::f` yolunun hedef biçimi: `Self::f` → `SelfMember`, tek bileşen → `Bare`.
fn path_call_target(segments: &[String]) -> Option<CallTarget> {
    let (name, qualifier) = segments.split_last()?;
    Some(match qualifier {
        [] => CallTarget::Bare(name.clone()),
        [only] if only == "Self" => CallTarget::SelfMember(name.clone()),
        _ => CallTarget::Member {
            qualifier: qualifier.join("."),
            name: name.clone(),
        },
    })
}

/// Tür ifadesinin hedef biçimi (trait tabanı, yapı ifadesinin adı).
fn type_target(node: Node, source: &str) -> Option<CallTarget> {
    match node.kind() {
        "type_identifier" | "scoped_type_identifier" | "scoped_identifier" | "identifier" => {
            let segments = path_segments(node, source)?;
            path_call_target(&segments)
        }
        "generic_type" | "generic_type_with_turbofish" => {
            type_target(node.child_by_field_name("type")?, source)
        }
        _ => None,
    }
}

/// Yol bileşenleri: `crate::a::B` → `[crate, a, B]`. Nitelikli tür yolu
/// (`<T as Trait>::f`) gibi çözülemeyen biçimler için `None`.
fn path_segments(node: Node, source: &str) -> Option<Vec<String>> {
    match node.kind() {
        "identifier" | "type_identifier" | "crate" | "self" | "super" => {
            Some(vec![text(node, source)?])
        }
        "scoped_identifier" | "scoped_type_identifier" => {
            let mut segments = match node.child_by_field_name("path") {
                Some(path) => path_segments(path, source)?,
                None => Vec::new(),
            };
            segments.push(text(node.child_by_field_name("name")?, source)?);
            Some(segments)
        }
        "generic_type" | "generic_type_with_turbofish" => {
            path_segments(node.child_by_field_name("type")?, source)
        }
        _ => None,
    }
}

/// Tür ifadesindeki adlar: tür tanımlayıcıları ve nitelikli tür yolları.
/// Generic parametreler ve `Self` ad değildir.
fn collect_types(node: Node, source: &str, generics: &HashSet<String>, facts: &mut SyntaxFacts) {
    match node.kind() {
        "type_identifier" => {
            if let Some(name) = text(node, source) {
                if name != "Self" && !generics.contains(&name) {
                    facts.names.push(name);
                }
            }
        }
        "scoped_type_identifier" => {
            if let Some(path) = path_segments(node, source) {
                facts.names.push(path.join("."));
            }
        }
        "primitive_type" | "lifetime" | "macro_invocation" => {}
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_types(child, source, generics, facts);
            }
        }
    }
}

fn collect_where_clause(
    definition: Node,
    source: &str,
    generics: &HashSet<String>,
    facts: &mut SyntaxFacts,
) {
    let mut cursor = definition.walk();
    for child in definition.children(&mut cursor) {
        if child.kind() == "where_clause" {
            collect_types(child, source, generics, facts);
        }
    }
}

/// Tanımın generic tür ve const parametrelerinin adları.
fn type_parameter_names(definition: Node, source: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    if let Some(parameters) = definition.child_by_field_name("type_parameters") {
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            if let Some(name) = parameter
                .child_by_field_name("name")
                .and_then(|name| text(name, source))
            {
                names.insert(name);
            }
        }
    }
    names
}

/// Makro token ağacında çağrı görünümleri: hemen ardından `(` ile açılan bir
/// token ağacı gelen tanımlayıcı. Önündeki `.` alıcıyı, `::` yolu belirtir.
fn scan_token_tree(tree: Node, source: &str, facts: &mut SyntaxFacts) {
    let mut cursor = tree.walk();
    let tokens: Vec<Node> = tree.children(&mut cursor).collect();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind() {
            "token_tree" => scan_token_tree(*token, source, facts),
            "identifier" => {
                let opens_call = tokens.get(index + 1).is_some_and(|next| {
                    next.kind() == "token_tree"
                        && next.child(0).is_some_and(|open| open.kind() == "(")
                });
                if !opens_call {
                    continue;
                }
                if let Some(target) = token_call_target(&tokens, index, source) {
                    facts.calls.push(CallSite {
                        target,
                        line: token.start_position().row + 1,
                    });
                }
            }
            _ => {}
        }
    }
}

/// `index`'teki tanımlayıcının çağrı biçimi, önündeki tokenlara göre.
fn token_call_target(tokens: &[Node], index: usize, source: &str) -> Option<CallTarget> {
    let name = text(tokens[index], source)?;
    let previous = index.checked_sub(1).map(|position| tokens[position]);
    match previous.map(|token| token.kind()) {
        Some(".") => {
            let receiver = index.checked_sub(2).map(|position| tokens[position]);
            let receiver_is_self = receiver.is_some_and(|token| {
                token.kind() == "self" || text(token, source).as_deref() == Some("self")
            });
            Some(if receiver_is_self {
                CallTarget::SelfMember(name)
            } else {
                CallTarget::Chained(name)
            })
        }
        Some("::") => {
            let mut qualifier: Vec<String> = Vec::new();
            let mut position = index;
            while position >= 2 && tokens[position - 1].kind() == "::" {
                let segment = tokens[position - 2];
                if !matches!(segment.kind(), "identifier" | "crate" | "self" | "super") {
                    break;
                }
                qualifier.push(text(segment, source)?);
                position -= 2;
            }
            qualifier.reverse();
            let mut segments = qualifier;
            segments.push(name);
            path_call_target(&segments)
        }
        _ => Some(CallTarget::Bare(name)),
    }
}

/// `use` bildiriminin kapsamda kurduğu bağlar.
fn use_bindings(declaration: Node, source: &str) -> Vec<ImportBinding> {
    let mut bindings = Vec::new();
    if let Some(argument) = declaration.child_by_field_name("argument") {
        collect_use_tree(argument, &[], source, &mut bindings);
    }
    bindings
}

/// `use` ağacını bağlara açar: `a::b` → `{b, a, b}`, `a::{self}` → `{a, …, a}`,
/// `a::b as c` → `{c, a, b}`, `a::*` → `{*, a, *}`.
fn collect_use_tree(
    node: Node,
    prefix: &[String],
    source: &str,
    bindings: &mut Vec<ImportBinding>,
) {
    match node.kind() {
        "identifier" | "crate" | "self" | "super" | "scoped_identifier" => {
            let Some(segments) = path_segments(node, source) else {
                return;
            };
            let mut path = prefix.to_vec();
            path.extend(segments);
            push_path_binding(&path, None, bindings);
        }
        "use_as_clause" => {
            let (Some(path_node), Some(alias)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("alias")
                    .and_then(|alias| text(alias, source)),
            ) else {
                return;
            };
            let Some(segments) = path_segments(path_node, source) else {
                return;
            };
            let mut path = prefix.to_vec();
            path.extend(segments);
            push_path_binding(&path, Some(alias), bindings);
        }
        "use_wildcard" => {
            let mut path = prefix.to_vec();
            let mut cursor = node.walk();
            if let Some(segments) = node
                .named_children(&mut cursor)
                .next()
                .and_then(|module| path_segments(module, source))
            {
                path.extend(segments);
            }
            bindings.push(ImportBinding {
                local: "*".to_string(),
                module: path.join("."),
                symbol: Some("*".to_string()),
            });
        }
        "scoped_use_list" => {
            let mut path = prefix.to_vec();
            if let Some(segments) = node
                .child_by_field_name("path")
                .and_then(|path_node| path_segments(path_node, source))
            {
                path.extend(segments);
            }
            if let Some(list) = node.child_by_field_name("list") {
                collect_use_tree(list, &path, source, bindings);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_use_tree(child, prefix, source, bindings);
            }
        }
        _ => {}
    }
}

/// Tam yolun bağı: son bileşen yerel addır; `…::self` önekin son bileşenini bağlar.
fn push_path_binding(path: &[String], alias: Option<String>, bindings: &mut Vec<ImportBinding>) {
    let Some((last, module)) = path.split_last() else {
        return;
    };
    let (symbol, module) = if last == "self" {
        let Some((symbol, module)) = module.split_last() else {
            return;
        };
        (symbol.clone(), module)
    } else {
        (last.clone(), module)
    };
    bindings.push(ImportBinding {
        local: alias.unwrap_or_else(|| symbol.clone()),
        module: module.join("."),
        symbol: Some(symbol),
    });
}

fn text(node: Node, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes()).ok().map(str::to_string)
}

fn finish(mut facts: SyntaxFacts) -> SyntaxFacts {
    facts.names.sort_unstable();
    facts.names.dedup();
    facts
}
