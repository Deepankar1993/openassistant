// src/memory/search.rs
use anyhow::Result;
use super::store::{MemoryStore, MemoryEntry};

pub struct SearchEngine {
    store: MemoryStore,
}

impl SearchEngine {
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.store.search_fts(query, limit)
    }

    pub fn search_by_category(&self, category: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.store.list_by_category(category, limit)
    }
}
