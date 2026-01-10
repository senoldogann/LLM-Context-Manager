use anyhow::{anyhow, Result};
use tree_sitter::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedLanguage {
    Rust,
    Python,
    TypeScript,
    Data, // New variant for non-code files (JSON, MD, etc.)
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
}
