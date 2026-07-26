use anyhow::{anyhow, Result};
use tree_sitter::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedLanguage {
    Rust,
    Python,
    TypeScript,
    Go,
    Java,
    Kotlin,
    CSharp,
    C,
    Cpp,
    Ruby,
    Php,
    Swift,
    Data, // Kod olmayan dosyalar (JSON, MD, vb.)
}

pub struct CodeParser {
    parser: Parser,
}

impl Default for CodeParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    pub fn parse(&mut self, content: &str, lang: SupportedLanguage) -> Result<String> {
        let tree = self.parse_tree(content, lang)?;
        Ok(tree.root_node().to_sexp())
    }

    /// Returns the raw Tree-sitter Tree for advanced AST traversal.
    pub fn parse_tree(
        &mut self,
        content: &str,
        lang: SupportedLanguage,
    ) -> Result<tree_sitter::Tree> {
        let language = match lang {
            SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            SupportedLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SupportedLanguage::Go => tree_sitter_go::LANGUAGE.into(),
            SupportedLanguage::Java => tree_sitter_java::LANGUAGE.into(),
            SupportedLanguage::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            SupportedLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            SupportedLanguage::C => tree_sitter_c::LANGUAGE.into(),
            SupportedLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            SupportedLanguage::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            SupportedLanguage::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            SupportedLanguage::Swift => tree_sitter_swift::LANGUAGE.into(),
            SupportedLanguage::Data => {
                return Err(anyhow!("Data files do not support AST parsing"))
            }
        };

        self.parser.set_language(&language)?;

        self.parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse code"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust() {
        let mut parser = CodeParser::new();
        let code = "fn main() { println!(\"Hello\"); }";
        let sexp = parser.parse(code, SupportedLanguage::Rust).unwrap();
        assert!(sexp.contains("function_item"));
        assert!(sexp.contains("identifier"));
    }

    #[test]
    fn test_parse_python() {
        let mut parser = CodeParser::new();
        let code = "def hello(): print('world')";
        let sexp = parser.parse(code, SupportedLanguage::Python).unwrap();
        assert!(sexp.contains("function_definition"));
    }

    #[test]
    fn parses_extended_open_source_languages() {
        let cases = [
            (SupportedLanguage::C, "int answer(void) { return 42; }"),
            (
                SupportedLanguage::Cpp,
                "class Greeter { public: void hello() {} };",
            ),
            (
                SupportedLanguage::Ruby,
                "class Greeter\n def hello; end\nend",
            ),
            (
                SupportedLanguage::Php,
                "<?php class Greeter { public function hello() {} }",
            ),
            (
                SupportedLanguage::Swift,
                "struct Greeter { func hello() {} }",
            ),
        ];

        for (language, source) in cases {
            let mut parser = CodeParser::new();
            let tree = parser.parse_tree(source, language).unwrap();
            assert!(!tree.root_node().has_error(), "{language:?} parse failed");
        }
    }
}
