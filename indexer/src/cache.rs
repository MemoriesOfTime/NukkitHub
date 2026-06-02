use crate::github::{Contributor, GitTree, Release, Repository};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::Path;
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
    pub contributors: HashMap<String, CacheEntry<Vec<Contributor>>>,
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
        self.save_to_path(Path::new(CACHE_FILE));
    }

    fn save_to_path(&self, path: &Path) {
        let count = self.entry_count();
        if count == 0 {
            match fs::remove_file(path) {
                Ok(_) => info!("Cleared empty data cache file"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => info!(error = %e, "Failed to clear empty data cache file"),
            }
            return;
        }

        let bytes = match postcard::to_allocvec(self) {
            Ok(b) => b,
            Err(e) => {
                info!(error = %e, "Failed to serialize cache");
                return;
            }
        };

        let file = match File::create(path) {
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
        self.repositories.len()
            + self.trees.len()
            + self.releases.len()
            + self.contributors.len()
            + self.raw_contents.len()
    }
}

pub fn clear_data_cache() {
    match fs::remove_file(CACHE_FILE) {
        Ok(_) => info!("Cleared data cache"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => info!(error = %e, "Failed to clear data cache"),
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
        assert!(cache.contributors.is_empty());
        assert!(cache.raw_contents.is_empty());
    }

    #[test]
    fn empty_cache_save_removes_existing_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "nukkithub-cache-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let cache_file = temp_dir.join("cache.bin.gz");
        fs::write(&cache_file, b"stale").unwrap();

        DataCache::default().save_to_path(&cache_file);

        assert!(!cache_file.exists());
        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
