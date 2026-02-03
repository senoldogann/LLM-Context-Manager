use crate::parser::SupportedLanguage;
use anyhow::{anyhow, Result};
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
        _ => SupportedLanguage::Data,
    }
}

pub(crate) fn read_text_file_limited(path: &Path) -> Result<String> {
    let max_bytes = max_file_bytes();
    read_text_file_with_limit(path, max_bytes)
}

pub(crate) fn read_text_file_with_limit(path: &Path, max_bytes: u64) -> Result<String> {
    let meta = fs::metadata(path)?;
    if meta.len() > max_bytes {
        return Err(anyhow!(
            "File size {} bytes exceeds limit of {} bytes",
            meta.len(),
            max_bytes
        ));
    }

    let bytes = fs::read(path)?;

    if bytes.iter().take(BINARY_SNIFF_LEN).any(|byte| *byte == 0) {
        return Err(anyhow!("Binary file detected (NUL byte)"));
    }

    String::from_utf8(bytes).map_err(|_| anyhow!("Binary or non-UTF-8 file"))
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
        assert!(err.to_string().contains("Binary"));
    }

    #[test]
    fn read_text_file_with_limit_rejects_large_files() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("large.txt");
        let payload = vec![b'a'; 16];
        fs::write(&file_path, payload).unwrap();

        let err = read_text_file_with_limit(&file_path, 8).unwrap_err();
        assert!(err.to_string().contains("exceeds limit"));
    }
}
