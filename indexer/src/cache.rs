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

/// 带版本头的缓存格式(见 from_bytes 的说明)
const CACHE_MAGIC: &[u8] = b"NHCACHE2";

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

// ReleaseAsset 尚无 digest 字段时期的缓存格式(镜像 + 回退升级,理由同上)
#[derive(Debug, Serialize, Deserialize)]
struct LegacyReleaseAsset {
    id: u64,
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    content_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct LegacyRelease {
    id: u64,
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    published_at: String,
    assets: Vec<LegacyReleaseAsset>,
}

impl From<LegacyRelease> for crate::github::Release {
    fn from(legacy: LegacyRelease) -> Self {
        Self {
            id: legacy.id,
            tag_name: legacy.tag_name,
            name: legacy.name,
            body: legacy.body,
            prerelease: legacy.prerelease,
            draft: legacy.draft,
            created_at: legacy.created_at,
            published_at: legacy.published_at,
            assets: legacy
                .assets
                .into_iter()
                .map(|a| crate::github::ReleaseAsset {
                    id: a.id,
                    name: a.name,
                    size: a.size,
                    download_count: a.download_count,
                    browser_download_url: a.browser_download_url,
                    content_type: a.content_type,
                    digest: None,
                })
                .collect(),
        }
    }
}

/// 升级无 digest 的旧 releases 缓存条目。etag 必须清空:digest 只在 200
/// 响应体里返回,带着旧 etag 重验证只会得到 304,sha256 将永远填不上;
/// 清空后下一次 sync 对每个仓库做一次全量取回,即可捕获 digest 并以新格式
/// 重新入缓存(一次性成本)。
fn upgrade_legacy_releases(
    releases: HashMap<String, CacheEntry<Vec<LegacyRelease>>>,
) -> HashMap<String, CacheEntry<Vec<crate::github::Release>>> {
    releases
        .into_iter()
        .map(|(key, entry)| {
            (
                key,
                CacheEntry {
                    data: entry.data.into_iter().map(Into::into).collect(),
                    etag: None,
                },
            )
        })
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DataCacheNoParent {
    #[serde(default)]
    repositories: HashMap<String, CacheEntry<LegacyRepository>>,
    #[serde(default)]
    trees: HashMap<String, CacheEntry<GitTree>>,
    #[serde(default)]
    releases: HashMap<String, CacheEntry<Vec<LegacyRelease>>>,
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
            releases: upgrade_legacy_releases(self.releases),
            contributors: self.contributors,
            raw_contents: self.raw_contents,
            compares: HashMap::new(),
        }
    }
}

/// ReleaseAsset 尚无 digest 字段时期(Repository 已有 parent)的缓存格式
#[derive(Debug, Default, Serialize, Deserialize)]
struct DataCacheNoDigest {
    #[serde(default)]
    repositories: HashMap<String, CacheEntry<Repository>>,
    #[serde(default)]
    trees: HashMap<String, CacheEntry<GitTree>>,
    #[serde(default)]
    releases: HashMap<String, CacheEntry<Vec<LegacyRelease>>>,
    #[serde(default)]
    contributors: HashMap<String, CacheEntry<Vec<Contributor>>>,
    #[serde(default)]
    raw_contents: HashMap<String, CacheEntry<String>>,
    #[serde(default)]
    compares: HashMap<String, CacheEntry<CompareResult>>,
}

impl DataCacheNoDigest {
    fn upgrade(self) -> DataCache {
        DataCache {
            repositories: self.repositories,
            trees: self.trees,
            releases: upgrade_legacy_releases(self.releases),
            contributors: self.contributors,
            raw_contents: self.raw_contents,
            compares: self.compares,
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
        // 新格式带 magic 头,确定性识别。不能靠"试解 DataCache"识别:
        // postcard 按字段顺序解码,新结构解旧字节可能错位后"成功"而非
        // 失败(实测 ReleaseAsset.digest 会吞掉后续 etag 的 Option 标签
        // 字节),静默产出脏数据
        if let Some(stripped) = bytes.strip_prefix(CACHE_MAGIC) {
            let cache = postcard::from_bytes::<DataCache>(stripped)
                .map_err(|e| format!("postcard decode error: {}", e))?;
            let count = cache.entry_count();
            if count > 0 {
                info!(entries = count, "Loaded data cache");
            }
            return Ok(cache);
        }

        if let Ok(no_digest) = postcard::from_bytes::<DataCacheNoDigest>(bytes) {
            let cache = no_digest.upgrade();
            let count = cache.entry_count();
            if count > 0 {
                info!(entries = count, "Loaded pre-digest data cache");
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

        let mut bytes = CACHE_MAGIC.to_vec();
        bytes.extend_from_slice(&match postcard::to_allocvec(self) {
            Ok(b) => b,
            Err(e) => {
                info!(error = %e, "Failed to serialize cache");
                return;
            }
        });

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

    // ReleaseAsset 尚无 digest 字段时期的缓存通过 DataCacheNoDigest 兜底升级:
    // releases 数据保留但 etag 被清空,强制下一次 sync 全量取回以捕获 digest
    #[test]
    fn loads_pre_digest_cache_and_clears_release_etags() {
        let pre_digest = DataCacheNoDigest {
            releases: [(
                "owner/repo".to_string(),
                CacheEntry {
                    data: vec![LegacyRelease {
                        id: 42,
                        tag_name: "v1.0.0".to_string(),
                        name: Some("v1.0.0".to_string()),
                        body: None,
                        prerelease: false,
                        draft: false,
                        created_at: String::new(),
                        published_at: "2026-01-01T00:00:00Z".to_string(),
                        assets: vec![LegacyReleaseAsset {
                            id: 7,
                            name: "plugin.jar".to_string(),
                            size: 123,
                            download_count: 0,
                            browser_download_url: String::new(),
                            content_type: String::new(),
                        }],
                    }],
                    etag: Some("release-etag".to_string()),
                },
            )]
            .into_iter()
            .collect(),
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
            ..DataCacheNoDigest::default()
        };
        let bytes = postcard::to_allocvec(&pre_digest).unwrap();

        let cache = DataCache::from_bytes(&bytes).unwrap();

        let entry = cache.releases.get("owner/repo").unwrap();
        assert_eq!(entry.data.len(), 1);
        assert_eq!(entry.data[0].tag_name, "v1.0.0");
        assert_eq!(entry.data[0].assets.len(), 1);
        assert_eq!(entry.data[0].assets[0].name, "plugin.jar");
        assert!(entry.data[0].assets[0].digest.is_none());
        assert!(
            entry.etag.is_none(),
            "release etag must be cleared to refetch digests"
        );
        assert_eq!(cache.compares.len(), 1);
    }

    // 现格式字节必须由 magic 主路径直接加载,不会走到兜底升级而丢失数据
    #[test]
    fn current_cache_bytes_load_without_downgrade() {
        let current = DataCache {
            releases: [(
                "owner/repo".to_string(),
                CacheEntry {
                    data: vec![crate::github::Release {
                        id: 1,
                        tag_name: "v1".to_string(),
                        name: None,
                        body: None,
                        prerelease: false,
                        draft: false,
                        created_at: String::new(),
                        published_at: String::new(),
                        assets: vec![crate::github::ReleaseAsset {
                            id: 2,
                            name: "p.jar".to_string(),
                            size: 10,
                            download_count: 0,
                            browser_download_url: String::new(),
                            content_type: String::new(),
                            digest: Some("sha256:aaaa".to_string()),
                        }],
                    }],
                    etag: Some("etag".to_string()),
                },
            )]
            .into_iter()
            .collect(),
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
        let mut bytes = CACHE_MAGIC.to_vec();
        bytes.extend_from_slice(&postcard::to_allocvec(&current).unwrap());

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
        let release = reloaded.releases.get("owner/repo").unwrap();
        assert_eq!(
            release.data[0].assets[0].digest.as_deref(),
            Some("sha256:aaaa")
        );
        assert_eq!(release.etag.as_deref(), Some("etag"));
    }

    // 完全未知形状的字节必须显式报错而非错位误解析(与 magic 设计互为对照:
    // 认识的格式确定性识别,不认识的格式失败留给 load() 降级为 fresh cache)
    #[test]
    fn undecodable_bytes_fail_loudly() {
        let err = DataCache::from_bytes(&[0xff, 0xfe, 0x00, 0x01]).unwrap_err();
        assert!(err.contains("postcard decode error"), "got: {err}");
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
