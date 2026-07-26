use crate::parser::SupportedLanguage;
use anyhow::Result;
use std::fs;
use std::path::Path;

const DEFAULT_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024; // 2 MB
const BINARY_SNIFF_LEN: usize = 4096;

fn max_file_bytes() -> u64 {
    std::env::var("CCM_MAX_FILE_BYTES")
        .ok()
        .and_then(|val| val.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_FILE_BYTES)
}

pub(crate) fn detect_language(path: &Path) -> SupportedLanguage {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "rs" => SupportedLanguage::Rust,
        "py" => SupportedLanguage::Python,
        "ts" | "js" | "tsx" | "jsx" => SupportedLanguage::TypeScript,
        "go" => SupportedLanguage::Go,
        "java" => SupportedLanguage::Java,
        "kt" | "kts" => SupportedLanguage::Kotlin,
        "cs" => SupportedLanguage::CSharp,
        "c" | "h" => SupportedLanguage::C,
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => SupportedLanguage::Cpp,
        "rb" | "rake" | "gemspec" => SupportedLanguage::Ruby,
        "php" | "phtml" => SupportedLanguage::Php,
        "swift" => SupportedLanguage::Swift,
        _ => SupportedLanguage::Data,
    }
}

pub(crate) fn read_text_file_limited(path: &Path) -> Result<String> {
    let max_bytes = max_file_bytes();
    read_text_file_with_limit(path, max_bytes).map_err(Into::into)
}

#[cfg(test)]
mod language_tests {
    use super::detect_language;
    use crate::parser::SupportedLanguage;
    use std::path::Path;

    #[test]
    fn detects_extended_language_extensions() {
        let cases = [
            ("main.c", SupportedLanguage::C),
            ("main.cpp", SupportedLanguage::Cpp),
            ("task.rb", SupportedLanguage::Ruby),
            ("index.php", SupportedLanguage::Php),
            ("App.swift", SupportedLanguage::Swift),
        ];
        for (path, expected) in cases {
            assert_eq!(detect_language(Path::new(path)), expected);
        }
    }
}

#[derive(Debug)]
pub enum FileReadError {
    Metadata {
        path: String,
        source: std::io::Error,
    },
    TooLarge {
        path: String,
        size_bytes: u64,
        limit_bytes: u64,
    },
    Read {
        path: String,
        source: std::io::Error,
    },
    BinaryNul {
        path: String,
    },
    NonUtf8 {
        path: String,
        source: std::string::FromUtf8Error,
    },
}

impl std::fmt::Display for FileReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Metadata { path, source } => {
                write!(f, "Failed to read metadata for '{}': {}", path, source)
            }
            Self::TooLarge {
                path,
                size_bytes,
                limit_bytes,
            } => write!(
                f,
                "File '{}' is too large: {} bytes > limit {} bytes",
                path, size_bytes, limit_bytes
            ),
            Self::Read { path, source } => {
                write!(f, "Failed to read file '{}': {}", path, source)
            }
            Self::BinaryNul { path } => write!(f, "Binary file detected (NUL byte): '{}'", path),
            Self::NonUtf8 { path, source } => {
                write!(f, "Non UTF-8 content in '{}': {}", path, source)
            }
        }
    }
}

impl std::error::Error for FileReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata { source, .. } => Some(source),
            Self::Read { source, .. } => Some(source),
            Self::NonUtf8 { source, .. } => Some(source),
            Self::TooLarge { .. } | Self::BinaryNul { .. } => None,
        }
    }
}

pub(crate) fn read_text_file_with_limit(
    path: &Path,
    max_bytes: u64,
) -> std::result::Result<String, FileReadError> {
    let path_str = path.to_string_lossy().to_string();
    let meta = fs::metadata(path).map_err(|source| FileReadError::Metadata {
        path: path_str.clone(),
        source,
    })?;
    if meta.len() > max_bytes {
        return Err(FileReadError::TooLarge {
            path: path_str,
            size_bytes: meta.len(),
            limit_bytes: max_bytes,
        });
    }

    let bytes = fs::read(path).map_err(|source| FileReadError::Read {
        path: path.to_string_lossy().to_string(),
        source,
    })?;

    if bytes.iter().take(BINARY_SNIFF_LEN).any(|byte| *byte == 0) {
        return Err(FileReadError::BinaryNul {
            path: path.to_string_lossy().to_string(),
        });
    }

    String::from_utf8(bytes).map_err(|source| FileReadError::NonUtf8 {
        path: path.to_string_lossy().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn read_text_file_with_limit_rejects_binary() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("binary.bin");
        fs::write(&file_path, vec![0u8, 159u8, 146u8]).unwrap();

        let err = read_text_file_with_limit(&file_path, 1024).unwrap_err();
        assert!(matches!(err, FileReadError::BinaryNul { .. }));
    }

    #[test]
    fn read_text_file_with_limit_rejects_large_files() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("large.txt");
        let payload = vec![b'a'; 16];
        fs::write(&file_path, payload).unwrap();

        let err = read_text_file_with_limit(&file_path, 8).unwrap_err();
        assert!(matches!(err, FileReadError::TooLarge { .. }));
    }
}
