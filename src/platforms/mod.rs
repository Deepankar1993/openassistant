// src/platforms/mod.rs
pub mod data_sources;

/// OpenHumans-style platform integrations for data management
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub members: Vec<String>,
    pub data_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    pub name: String,
    pub description: String,
    pub url: String,
    pub data_type: String,
}
