use crate::github::{GitTree, Release, Repository};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use tracing::info;

const CACHE_FILE: &str = ".data_cache.bin.gz";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    pub data: T,
    pub etag: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DataCache {
    #[serde(default)]
    pub repositories: HashMap<String, CacheEntry<Repository>>,
    #[serde(default)]
    pub trees: HashMap<String, CacheEntry<GitTree>>,
    #[serde(default)]
    pub releases: HashMap<String, CacheEntry<Vec<Release>>>,
    #[serde(default)]
    pub raw_contents: HashMap<String, CacheEntry<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LegacyDataCache {
    repositories: HashMap<String, CacheEntry<Repository>>,
    trees: HashMap<String, CacheEntry<GitTree>>,
}

impl DataCache {
    pub fn load() -> Self {
        let file = match File::open(CACHE_FILE) {
            Ok(f) => f,
            Err(_) => return Self::default(),
        };

        let mut decoder = GzDecoder::new(BufReader::new(file));
        let mut bytes = Vec::new();
        if decoder.read_to_end(&mut bytes).is_err() {
            return Self::default();
        }

        match Self::from_bytes(&bytes) {
            Ok(cache) => cache,
            Err(e) => {
                info!(error = %e, "Failed to load cache, starting fresh");
                Self::default()
            }
        }
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if let Ok(cache) = postcard::from_bytes::<DataCache>(bytes) {
            let count = cache.entry_count();
            if count > 0 {
                info!(entries = count, "Loaded data cache");
            }
            return Ok(cache);
        }

        let legacy = postcard::from_bytes::<LegacyDataCache>(bytes)
            .map_err(|e| format!("postcard decode error: {}", e))?;
        let cache = Self {
            repositories: legacy.repositories,
            trees: legacy.trees,
            ..Self::default()
        };
        let count = cache.entry_count();
        if count > 0 {
            info!(entries = count, "Loaded legacy data cache");
        }
        Ok(cache)
    }

    pub fn save(&self) {
        let count = self.entry_count();
        if count == 0 {
            return;
        }

        let bytes = match postcard::to_allocvec(self) {
            Ok(b) => b,
            Err(e) => {
                info!(error = %e, "Failed to serialize cache");
                return;
            }
        };

        let file = match File::create(CACHE_FILE) {
            Ok(f) => f,
            Err(e) => {
                info!(error = %e, "Failed to create cache file");
                return;
            }
        };

        let mut encoder = GzEncoder::new(file, Compression::default());
        match encoder.write_all(&bytes) {
            Ok(_) => info!(entries = count, "Saved data cache"),
            Err(e) => info!(error = %e, "Failed to write cache"),
        }
    }

    fn entry_count(&self) -> usize {
        self.repositories.len() + self.trees.len() + self.releases.len() + self.raw_contents.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_legacy_cache_shape() {
        let legacy = LegacyDataCache::default();
        let bytes = postcard::to_allocvec(&legacy).unwrap();

        let cache = DataCache::from_bytes(&bytes).unwrap();

        assert!(cache.repositories.is_empty());
        assert!(cache.trees.is_empty());
        assert!(cache.releases.is_empty());
        assert!(cache.raw_contents.is_empty());
    }
}
