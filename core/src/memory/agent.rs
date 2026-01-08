use std::collections::HashMap;

pub struct MemoryManager {
    pub active_context: HashMap<String, String>, // Mock: FilePath -> Content
    pub page_size: usize,
}

impl MemoryManager {
    pub fn new() -> Self {
        Self {
            active_context: HashMap::new(),
            page_size: 4096, // 4KB pages
        }
    }

    pub fn load_page(&mut self, file_path: &str, content: String) {
        println!("Loading page for: {}", file_path);
        self.active_context.insert(file_path.to_string(), content);
    }

    pub fn evict_page(&mut self, file_path: &str) {
        println!("Evicting page for: {}", file_path);
        self.active_context.remove(file_path);
    }
}
