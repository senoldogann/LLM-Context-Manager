//! Arama ön çalışması: eşleşme konumlarının sözdizimsel türünü ve kapsayan tanımı bulur.
//! Girdi (stdin, satır başına JSON): {"path": "...", "points": [[satir, sutun], ...]}
//! Çıktı (stdout, satır başına JSON): {"path": "...", "lang": ..., "results": [...], "error": ...}
//! Satır 1 tabanlı, sütun 0 tabanlı bayt konumudur. Grammar sürümleri CCM ile aynıdır.

use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufWriter, Write};
use tree_sitter::{Language, Node, Parser, Point};

#[derive(Deserialize)]
struct Request {
    path: String,
    points: Vec<(usize, usize)>,
}

#[derive(Serialize)]
struct Classification {
    line: usize,
    kind: &'static str,
    scope: Option<String>,
    name: Option<String>,
}

#[derive(Serialize)]
struct Response {
    path: String,
    lang: Option<&'static str>,
    results: Vec<Classification>,
    error: Option<String>,
}

/// Dosya uzantısından dili ve grammar'ı seçer; tanınmayan uzantılar yapısız kalır.
fn select_language(path: &str) -> Option<(&'static str, Language)> {
    let file_name = path.rsplit('/').next()?;
    let (_, extension) = file_name.rsplit_once('.')?;
    match extension {
        "py" | "pyi" => Some(("python", tree_sitter_python::LANGUAGE.into())),
        "rs" => Some(("rust", tree_sitter_rust::LANGUAGE.into())),
        "ts" | "mts" | "cts" => Some((
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        )),
        "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
            Some(("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()))
        }
        "swift" => Some(("swift", tree_sitter_swift::LANGUAGE.into())),
        "go" => Some(("go", tree_sitter_go::LANGUAGE.into())),
        _ => None,
    }
}

/// Çok satırlı adları (ör. jenerik impl türleri) tek satıra indirir.
fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// Düğüm bir tanımsa adını ve varsa ad düğümünü döndürür.
fn definition_info<'a>(node: Node<'a>, source: &[u8]) -> Option<(String, Option<Node<'a>>)> {
    let name_node = match node.kind() {
        "function_definition" | "class_definition" | "function_item"
        | "function_signature_item" | "struct_item" | "enum_item" | "trait_item" | "mod_item"
        | "union_item" | "type_item" | "macro_definition" | "function_declaration"
        | "generator_function_declaration" | "class_declaration" | "abstract_class_declaration"
        | "interface_declaration" | "enum_declaration" | "type_alias_declaration"
        | "method_definition" | "method_signature" | "abstract_method_signature"
        | "protocol_declaration" | "protocol_function_declaration" | "typealias_declaration"
        | "method_declaration" | "type_spec" => node.child_by_field_name("name"),
        "impl_item" => node.child_by_field_name("type"),
        "variable_declarator" | "public_field_definition" => {
            let value = node.child_by_field_name("value")?;
            match value.kind() {
                "arrow_function" | "function_expression" | "function" | "class" => {
                    node.child_by_field_name("name")
                }
                _ => None,
            }
        }
        "init_declaration" => return Some(("init".to_string(), None)),
        "deinit_declaration" => return Some(("deinit".to_string(), None)),
        _ => None,
    }?;
    let name = name_node.utf8_text(source).ok()?;
    Some((single_line(name), Some(name_node)))
}

fn is_comment(kind: &str) -> bool {
    kind.contains("comment")
}

fn is_string(kind: &str) -> bool {
    kind == "string"
        || kind == "template_string"
        || kind == "string_content"
        || kind == "string_fragment"
        || kind == "char_literal"
        || kind.ends_with("string_literal")
}

fn is_import(kind: &str) -> bool {
    matches!(
        kind,
        "import_statement"
            | "import_from_statement"
            | "future_import_statement"
            | "use_declaration"
            | "extern_crate_declaration"
            | "import_declaration"
    )
}

/// Bir konumun türünü (def, import, comment, string, code) ve kapsayan tanım zincirini bulur.
fn classify(root: Node, source: &[u8], line: usize, column: usize) -> Classification {
    let point = Point {
        row: line.saturating_sub(1),
        column,
    };
    let Some(start) = root.descendant_for_point_range(point, point) else {
        return Classification {
            line,
            kind: "code",
            scope: None,
            name: None,
        };
    };
    let mut in_comment = false;
    let mut in_string = false;
    let mut in_import = false;
    let mut defined: Option<String> = None;
    let mut scope: Vec<String> = Vec::new();
    let mut current = Some(start);
    while let Some(node) = current {
        let kind = node.kind();
        if is_comment(kind) {
            in_comment = true;
        } else if is_string(kind) {
            in_string = true;
        } else if is_import(kind) {
            in_import = true;
        }
        if let Some((name, name_node)) = definition_info(node, source) {
            // Eşleşme adın içinde ya da adın bulunduğu tanım satırındaysa tanım satırıdır.
            let on_definition_line = name_node
                .map(|n| {
                    (n.start_byte() <= start.start_byte() && start.end_byte() <= n.end_byte())
                        || n.start_position().row == point.row
                })
                .unwrap_or(false);
            if on_definition_line && defined.is_none() && scope.is_empty() {
                defined = Some(name);
            } else {
                scope.push(name);
            }
        }
        current = node.parent();
    }
    scope.reverse();
    let kind = if in_comment {
        "comment"
    } else if in_string {
        "string"
    } else if defined.is_some() {
        "def"
    } else if in_import {
        "import"
    } else {
        "code"
    };
    Classification {
        line,
        kind,
        scope: if scope.is_empty() {
            None
        } else {
            Some(scope.join("."))
        },
        name: defined,
    }
}

/// Tek dosya isteğini işler; okuma ve ayrıştırma hataları yanıtta açıkça döner.
fn handle(parser: &mut Parser, request: Request) -> Response {
    let Some((lang, language)) = select_language(&request.path) else {
        return Response {
            path: request.path,
            lang: None,
            results: Vec::new(),
            error: None,
        };
    };
    let source = match std::fs::read(&request.path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Response {
                path: request.path,
                lang: Some(lang),
                results: Vec::new(),
                error: Some(format!("read failed: {error}")),
            }
        }
    };
    if let Err(error) = parser.set_language(&language) {
        return Response {
            path: request.path,
            lang: Some(lang),
            results: Vec::new(),
            error: Some(format!("grammar failed: {error}")),
        };
    }
    let Some(tree) = parser.parse(&source, None) else {
        return Response {
            path: request.path,
            lang: Some(lang),
            results: Vec::new(),
            error: Some("parse failed".to_string()),
        };
    };
    let root = tree.root_node();
    let results: Vec<Classification> = request
        .points
        .iter()
        .map(|&(line, column)| classify(root, &source, line, column))
        .collect();
    Response {
        path: request.path,
        lang: Some(lang),
        results,
        error: None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut parser = Parser::new();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)?;
        let response = handle(&mut parser, request);
        serde_json::to_writer(&mut out, &response)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
}
