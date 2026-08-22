use crate::github::{
    CompareResult, Contributor, GitTree, Owner, Release, Repository, RepositoryLicense,
};
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
    #[serde(default)]
    pub compares: HashMap<String, CacheEntry<CompareResult>>,
}

// Repository 尚无 parent 字段时期的缓存格式(postcard 按字段顺序解码,
// 不支持缺失字段,旧格式必须整体镜像后逐层回退升级)
#[derive(Debug, Serialize, Deserialize)]
struct LegacyRepository {
    id: u64,
    full_name: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    forks_count: u64,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    pushed_at: String,
    owner: Owner,
    #[serde(default)]
    license: Option<RepositoryLicense>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    is_template: bool,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    contributors_url: String,
}

impl From<LegacyRepository> for Repository {
    fn from(legacy: LegacyRepository) -> Self {
        Self {
            id: legacy.id,
            full_name: legacy.full_name,
            name: legacy.name,
            description: legacy.description,
            html_url: legacy.html_url,
            stargazers_count: legacy.stargazers_count,
            forks_count: legacy.forks_count,
            created_at: legacy.created_at,
            updated_at: legacy.updated_at,
            pushed_at: legacy.pushed_at,
            owner: legacy.owner,
            license: legacy.license,
            topics: legacy.topics,
            is_template: legacy.is_template,
            fork: legacy.fork,
            archived: legacy.archived,
            default_branch: legacy.default_branch,
            contributors_url: legacy.contributors_url,
            parent: None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DataCacheNoParent {
    #[serde(default)]
    repositories: HashMap<String, CacheEntry<LegacyRepository>>,
    #[serde(default)]
    trees: HashMap<String, CacheEntry<GitTree>>,
    #[serde(default)]
    releases: HashMap<String, CacheEntry<Vec<Release>>>,
    #[serde(default)]
    contributors: HashMap<String, CacheEntry<Vec<Contributor>>>,
    #[serde(default)]
    raw_contents: HashMap<String, CacheEntry<String>>,
}

impl DataCacheNoParent {
    fn upgrade(self) -> DataCache {
        DataCache {
            repositories: self
                .repositories
                .into_iter()
                .map(|(key, entry)| {
                    (
                        key,
                        CacheEntry {
                            data: entry.data.into(),
                            etag: entry.etag,
                        },
                    )
                })
                .collect(),
            trees: self.trees,
            releases: self.releases,
            contributors: self.contributors,
            raw_contents: self.raw_contents,
            compares: HashMap::new(),
        }
    }
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

        if let Ok(no_parent) = postcard::from_bytes::<DataCacheNoParent>(bytes) {
            let cache = no_parent.upgrade();
            let count = cache.entry_count();
            if count > 0 {
                info!(entries = count, "Loaded pre-parent data cache");
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
            + self.compares.len()
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
        assert!(cache.compares.is_empty());
    }

    // 验证旧缓存文件(Repository 尚无 parent 字段、DataCache 尚无 compares 字段)
    // 能通过 DataCacheNoParent 兜底升级加载,不会导致缓存整体失效
    #[test]
    fn loads_pre_parent_cache_and_upgrades() {
        let pre_parent = DataCacheNoParent {
            repositories: [(
                "owner/repo".to_string(),
                CacheEntry {
                    data: LegacyRepository {
                        id: 1,
                        full_name: "owner/repo".to_string(),
                        name: "repo".to_string(),
                        description: None,
                        html_url: String::new(),
                        stargazers_count: 0,
                        forks_count: 0,
                        created_at: String::new(),
                        updated_at: String::new(),
                        pushed_at: String::new(),
                        owner: Owner {
                            login: "owner".to_string(),
                            avatar_url: String::new(),
                            html_url: String::new(),
                        },
                        license: None,
                        topics: Vec::new(),
                        is_template: false,
                        fork: true,
                        archived: false,
                        default_branch: Some("master".to_string()),
                        contributors_url: String::new(),
                    },
                    etag: Some("repo-etag".to_string()),
                },
            )]
            .into_iter()
            .collect(),
            trees: HashMap::new(),
            releases: HashMap::new(),
            contributors: HashMap::new(),
            raw_contents: HashMap::new(),
        };
        let bytes = postcard::to_allocvec(&pre_parent).unwrap();

        let cache = DataCache::from_bytes(&bytes).unwrap();

        assert!(cache.compares.is_empty());
        let entry = cache.repositories.get("owner/repo").unwrap();
        assert_eq!(entry.data.full_name, "owner/repo");
        assert!(entry.data.fork);
        assert_eq!(entry.data.default_branch.as_deref(), Some("master"));
        assert!(entry.data.parent.is_none());
        assert_eq!(entry.etag.as_deref(), Some("repo-etag"));
    }

    // 现格式字节必须由主路径直接加载,不会走到兜底升级而丢失 compares 数据
    #[test]
    fn current_cache_bytes_load_without_downgrade() {
        let current = DataCache {
            compares: [(
                "owner/repo/compare/up:main...owner:main".to_string(),
                CacheEntry {
                    data: CompareResult {
                        status: "ahead".to_string(),
                        ahead_by: 1,
                        behind_by: 0,
                    },
                    etag: None,
                },
            )]
            .into_iter()
            .collect(),
            ..DataCache::default()
        };
        let bytes = postcard::to_allocvec(&current).unwrap();

        let reloaded = DataCache::from_bytes(&bytes).unwrap();

        assert_eq!(reloaded.compares.len(), 1);
        assert_eq!(
            reloaded
                .compares
                .get("owner/repo/compare/up:main...owner:main")
                .unwrap()
                .data
                .ahead_by,
            1
        );
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
