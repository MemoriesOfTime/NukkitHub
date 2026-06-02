use super::auth::GitHubAppAuth;
use super::types::*;
use crate::cache::{CacheEntry, DataCache};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, debug_span, info_span, warn};

const MAX_CONCURRENT: usize = 10;

const API_BASE: &str = "https://api.github.com";
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(3600 - 300);
const CORE_RATE_LIMIT_BUFFER: usize = 50;
const SEARCH_MIN_INTERVAL: Duration = Duration::from_secs(2);
const CODE_SEARCH_MIN_INTERVAL: Duration = Duration::from_secs(7);

const USER_AGENT: &str = concat!(
    "NukkitIndexer/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/MemoriesOfTime/NukkitHub)"
);
const GITHUB_PAGE_SIZE: usize = 100;
const MAX_GITHUB_PAGES: usize = 100;

struct ResponseCache {
    repositories: HashMap<String, CacheEntry<Repository>>,
    trees: HashMap<String, CacheEntry<GitTree>>,
    releases: HashMap<String, CacheEntry<Vec<Release>>>,
    contributors: HashMap<String, CacheEntry<Vec<Contributor>>>,
    raw_contents: HashMap<String, CacheEntry<String>>,
}

impl ResponseCache {
    fn from_data_cache(cache: DataCache) -> Self {
        Self {
            repositories: cache.repositories,
            trees: cache.trees,
            releases: cache.releases,
            contributors: cache.contributors,
            raw_contents: cache.raw_contents,
        }
    }

    fn to_data_cache(&self) -> DataCache {
        DataCache {
            repositories: self.repositories.clone(),
            trees: self.trees.clone(),
            releases: self.releases.clone(),
            contributors: self.contributors.clone(),
            raw_contents: self.raw_contents.clone(),
        }
    }

    fn clear_repository_related(&mut self, owner: &str, repo: &str) {
        let repo_key = format!("{}/{}", owner, repo);
        let readme_key = format!("readme/{}/{}", owner, repo);
        let contributors_prefix = format!("{}/{}/contributors?", owner, repo);
        let content_prefix = format!("contents/{}/{}/", owner, repo);
        let tree_prefix = format!("{}/{}/", owner, repo);

        self.repositories.remove(&repo_key);
        self.releases.remove(&repo_key);
        self.trees.retain(|key, _| !key.starts_with(&tree_prefix));
        self.contributors
            .retain(|key, _| key != &repo_key && !key.starts_with(&contributors_prefix));
        self.raw_contents
            .retain(|key, _| key != &readme_key && !key.starts_with(&content_prefix));
    }
}

fn with_pagination(url: &str, page: usize, per_page: usize) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}per_page={per_page}&page={page}")
}

fn repository_cache_key_from_url(url: &str) -> String {
    url.trim()
        .trim_start_matches(API_BASE)
        .trim_start_matches('/')
        .trim_start_matches("repos/")
        .to_string()
}

fn is_missing_repository_error(error: &str) -> bool {
    error == "not found" || error.contains("404")
}

fn should_use_cached_repository_on_error(error: &str) -> bool {
    !is_missing_repository_error(error)
}

#[derive(Clone)]
pub enum AuthMethod {
    Token(String),
    App(GitHubAppAuth),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RateLimitResource {
    Core,
    Search,
    CodeSearch,
    Graphql,
    Other,
}

impl RateLimitResource {
    fn from_url(url: &str) -> Self {
        if url.contains("/search/code") {
            Self::CodeSearch
        } else if url.contains("/search/") {
            Self::Search
        } else if url.contains("/graphql") {
            Self::Graphql
        } else {
            Self::Core
        }
    }

    fn from_header(value: Option<&str>, fallback: Self) -> Self {
        match value {
            Some("core") => Self::Core,
            // GitHub may report code search as the search resource. Keep the
            // endpoint-derived bucket so code search can be throttled separately.
            Some("search") if fallback == Self::CodeSearch => Self::CodeSearch,
            Some("search") => Self::Search,
            Some("code_search") => Self::CodeSearch,
            Some("graphql") => Self::Graphql,
            Some(_) => Self::Other,
            None => fallback,
        }
    }
}

#[derive(Clone)]
struct RateLimitBucket {
    remaining: Arc<AtomicUsize>,
    limit: Arc<AtomicUsize>,
    reset: Arc<AtomicUsize>,
}

impl RateLimitBucket {
    fn new() -> Self {
        Self {
            remaining: Arc::new(AtomicUsize::new(usize::MAX)),
            limit: Arc::new(AtomicUsize::new(0)),
            reset: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn remaining(&self) -> usize {
        self.remaining.load(Ordering::SeqCst)
    }

    fn limit(&self) -> usize {
        self.limit.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
pub struct RateLimit {
    core: RateLimitBucket,
    search: RateLimitBucket,
    code_search: RateLimitBucket,
    graphql: RateLimitBucket,
    other: RateLimitBucket,
}

impl RateLimit {
    fn new() -> Self {
        Self {
            core: RateLimitBucket::new(),
            search: RateLimitBucket::new(),
            code_search: RateLimitBucket::new(),
            graphql: RateLimitBucket::new(),
            other: RateLimitBucket::new(),
        }
    }

    fn bucket(&self, resource: RateLimitResource) -> &RateLimitBucket {
        match resource {
            RateLimitResource::Core => &self.core,
            RateLimitResource::Search => &self.search,
            RateLimitResource::CodeSearch => &self.code_search,
            RateLimitResource::Graphql => &self.graphql,
            RateLimitResource::Other => &self.other,
        }
    }

    pub fn remaining(&self) -> usize {
        self.core.remaining()
    }

    pub fn limit(&self) -> usize {
        self.core.limit()
    }

    pub fn search_remaining(&self) -> usize {
        self.search.remaining()
    }

    pub fn code_search_remaining(&self) -> usize {
        self.code_search.remaining()
    }

    pub fn has_core_remaining(&self) -> bool {
        self.core.remaining() > CORE_RATE_LIMIT_BUFFER
    }

    pub fn has_remaining(&self) -> bool {
        self.has_core_remaining()
    }
}

pub struct GitHubClient {
    auth: AuthMethod,
    cached_token: Arc<RwLock<Option<(String, Instant)>>>,
    pub rate_limit: RateLimit,
    api_calls: Arc<AtomicUsize>,
    cache_hits: Arc<AtomicUsize>,
    cache: Arc<RwLock<ResponseCache>>,
    search_throttle: Arc<Mutex<SearchThrottle>>,
}

struct SearchThrottle {
    search_next_at: Instant,
    code_search_next_at: Instant,
}

impl SearchThrottle {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            search_next_at: now,
            code_search_next_at: now,
        }
    }
}

impl Clone for GitHubClient {
    fn clone(&self) -> Self {
        Self {
            auth: self.auth.clone(),
            cached_token: Arc::clone(&self.cached_token),
            rate_limit: self.rate_limit.clone(),
            api_calls: Arc::clone(&self.api_calls),
            cache_hits: Arc::clone(&self.cache_hits),
            cache: Arc::clone(&self.cache),
            search_throttle: Arc::clone(&self.search_throttle),
        }
    }
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> Self {
        Self::new_with_cache(token, DataCache::default())
    }

    pub fn new_with_cache(token: Option<String>, data_cache: DataCache) -> Self {
        Self {
            auth: token.map(AuthMethod::Token).unwrap_or(AuthMethod::None),
            cached_token: Arc::new(RwLock::new(None)),
            rate_limit: RateLimit::new(),
            api_calls: Arc::new(AtomicUsize::new(0)),
            cache_hits: Arc::new(AtomicUsize::new(0)),
            cache: Arc::new(RwLock::new(ResponseCache::from_data_cache(data_cache))),
            search_throttle: Arc::new(Mutex::new(SearchThrottle::new())),
        }
    }

    pub fn with_app(app_auth: GitHubAppAuth) -> Self {
        Self::with_app_and_cache(app_auth, DataCache::default())
    }

    pub fn with_app_and_cache(app_auth: GitHubAppAuth, data_cache: DataCache) -> Self {
        Self {
            auth: AuthMethod::App(app_auth),
            cached_token: Arc::new(RwLock::new(None)),
            rate_limit: RateLimit::new(),
            api_calls: Arc::new(AtomicUsize::new(0)),
            cache_hits: Arc::new(AtomicUsize::new(0)),
            cache: Arc::new(RwLock::new(ResponseCache::from_data_cache(data_cache))),
            search_throttle: Arc::new(Mutex::new(SearchThrottle::new())),
        }
    }

    pub fn export_data_cache(&self) -> DataCache {
        self.cache.read().unwrap().to_data_cache()
    }

    pub fn api_calls(&self) -> usize {
        self.api_calls.load(Ordering::SeqCst)
    }

    pub fn cache_hits(&self) -> usize {
        self.cache_hits.load(Ordering::SeqCst)
    }

    fn get_token(&self) -> Result<Option<String>, String> {
        match &self.auth {
            AuthMethod::Token(t) => Ok(Some(t.clone())),
            AuthMethod::None => Ok(None),
            AuthMethod::App(app) => {
                {
                    let cached = self.cached_token.read().unwrap();
                    if let Some((token, created)) = cached.as_ref()
                        && created.elapsed() < TOKEN_REFRESH_MARGIN
                    {
                        return Ok(Some(token.clone()));
                    }
                }
                let token = app.get_token()?;
                let mut cached = self.cached_token.write().unwrap();
                *cached = Some((token.clone(), Instant::now()));
                Ok(Some(token))
            }
        }
    }

    fn update_rate_limit_from_headers(
        &self,
        resource: RateLimitResource,
        remaining: Option<&str>,
        limit: Option<&str>,
        reset: Option<&str>,
    ) {
        let bucket = self.rate_limit.bucket(resource);
        if let Some(r) = remaining.and_then(|s| s.parse().ok()) {
            bucket.remaining.store(r, Ordering::SeqCst);
        }
        if let Some(l) = limit.and_then(|s| s.parse().ok()) {
            bucket.limit.store(l, Ordering::SeqCst);
        }
        if let Some(r) = reset.and_then(|s| s.parse().ok()) {
            bucket.reset.store(r, Ordering::SeqCst);
        }
    }

    fn update_rate_limit_from_response_headers(
        &self,
        fallback_resource: RateLimitResource,
        headers: &ureq::http::HeaderMap,
    ) {
        let resource = RateLimitResource::from_header(
            headers
                .get("X-RateLimit-Resource")
                .and_then(|h| h.to_str().ok()),
            fallback_resource,
        );
        self.update_rate_limit_from_headers(
            resource,
            headers
                .get("X-RateLimit-Remaining")
                .and_then(|h| h.to_str().ok()),
            headers
                .get("X-RateLimit-Limit")
                .and_then(|h| h.to_str().ok()),
            headers
                .get("X-RateLimit-Reset")
                .and_then(|h| h.to_str().ok()),
        );
    }

    fn throttle_search(&self, resource: RateLimitResource) {
        let interval = match resource {
            RateLimitResource::Search => SEARCH_MIN_INTERVAL,
            RateLimitResource::CodeSearch => CODE_SEARCH_MIN_INTERVAL,
            _ => return,
        };

        let mut throttle = self.search_throttle.lock().unwrap();
        let next_at = match resource {
            RateLimitResource::Search => &mut throttle.search_next_at,
            RateLimitResource::CodeSearch => &mut throttle.code_search_next_at,
            _ => return,
        };

        let now = Instant::now();
        if *next_at > now {
            thread::sleep(*next_at - now);
        }
        *next_at = Instant::now() + interval;
    }

    fn request<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        self.request_with_etag(url, None).map(|(data, _)| data)
    }

    fn request_with_etag<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<(T, Option<String>), String> {
        let _span = debug_span!("api_request", url = %url).entered();
        let token = self.get_token()?;
        let fallback_resource = RateLimitResource::from_url(url);

        for attempt in 0..3 {
            let mut req = ureq::get(url)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", USER_AGENT)
                .header("X-GitHub-Api-Version", "2022-11-28");

            if let Some(t) = &token {
                req = req.header("Authorization", &format!("Bearer {}", t));
            }

            if let Some(etag_val) = etag {
                req = req.header("If-None-Match", etag_val);
            }

            let req = req.config().http_status_as_error(false).build();

            self.api_calls.fetch_add(1, Ordering::SeqCst);
            match req.call() {
                Ok(mut resp) => {
                    let status = resp.status().as_u16();
                    let headers = resp.headers();
                    self.update_rate_limit_from_response_headers(fallback_resource, headers);

                    if status == 304 {
                        self.cache_hits.fetch_add(1, Ordering::SeqCst);
                        return Err("not_modified".to_string());
                    }

                    if status == 403 || status == 429 {
                        if attempt < 2 {
                            let wait = 30u64 * (1 << attempt);
                            warn!(
                                code = status,
                                wait_secs = wait,
                                attempt = attempt + 1,
                                "Rate limited, exponential backoff"
                            );
                            thread::sleep(Duration::from_secs(wait));
                            continue;
                        }
                        return Err(format!("Rate limited after {} attempts", attempt + 1));
                    }

                    if status >= 400 {
                        return Err(format!("HTTP status {}", status));
                    }

                    let new_etag = resp
                        .headers()
                        .get("ETag")
                        .and_then(|h| h.to_str().ok())
                        .map(String::from);

                    let data = resp
                        .body_mut()
                        .read_json()
                        .map_err(|e| format!("Parse error: {}", e))?;

                    return Ok((data, new_etag));
                }
                Err(e) => return Err(format!("HTTP error: {}", e)),
            }
        }
        Err("Max retries exceeded".to_string())
    }

    fn request_raw_with_etag(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let _span = debug_span!("api_request_raw", url = %url).entered();
        let token = self.get_token()?;
        let fallback_resource = RateLimitResource::from_url(url);

        for attempt in 0..3 {
            let mut req = ureq::get(url)
                .header("Accept", "application/vnd.github.raw+json")
                .header("User-Agent", USER_AGENT)
                .header("X-GitHub-Api-Version", "2022-11-28");

            if let Some(t) = &token {
                req = req.header("Authorization", &format!("Bearer {}", t));
            }

            if let Some(etag_val) = etag {
                req = req.header("If-None-Match", etag_val);
            }

            let req = req.config().http_status_as_error(false).build();

            self.api_calls.fetch_add(1, Ordering::SeqCst);
            match req.call() {
                Ok(mut resp) => {
                    let status = resp.status().as_u16();
                    let headers = resp.headers();
                    self.update_rate_limit_from_response_headers(fallback_resource, headers);

                    if status == 304 {
                        self.cache_hits.fetch_add(1, Ordering::SeqCst);
                        return Err("not_modified".to_string());
                    }

                    if status == 404 {
                        return Err("not found".to_string());
                    }

                    if status == 403 || status == 429 {
                        if attempt < 2 {
                            let wait = 30u64 * (1 << attempt);
                            warn!(
                                code = status,
                                wait_secs = wait,
                                attempt = attempt + 1,
                                "Rate limited, exponential backoff"
                            );
                            thread::sleep(Duration::from_secs(wait));
                            continue;
                        }
                        return Err(format!("Rate limited after {} attempts", attempt + 1));
                    }

                    if status >= 400 {
                        return Err(format!("HTTP status {}", status));
                    }

                    let new_etag = resp
                        .headers()
                        .get("ETag")
                        .and_then(|h| h.to_str().ok())
                        .map(String::from);

                    let data = resp
                        .body_mut()
                        .read_to_string()
                        .map_err(|e| e.to_string())?;

                    return Ok((data, new_etag));
                }
                Err(e) => return Err(format!("HTTP error: {}", e)),
            }
        }
        Err("Max retries exceeded".to_string())
    }

    fn get_cached_raw(&self, cache_key: String, url: String) -> Result<String, String> {
        let cached = {
            let cache = self.cache.read().unwrap();
            cache.raw_contents.get(&cache_key).cloned()
        };

        let etag = cached.as_ref().and_then(|e| e.etag.as_deref());

        match self.request_raw_with_etag(&url, etag) {
            Ok((data, new_etag)) => {
                let mut cache = self.cache.write().unwrap();
                cache.raw_contents.insert(
                    cache_key,
                    CacheEntry {
                        data: data.clone(),
                        etag: new_etag,
                    },
                );
                Ok(data)
            }
            Err(e) if e == "not_modified" => {
                debug!(key = %cache_key, "Cache hit (304)");
                cached
                    .map(|entry| entry.data)
                    .ok_or_else(|| "not_modified without cached content".to_string())
            }
            Err(e) if e == "not found" => Err(e),
            Err(e) => {
                if let Some(entry) = cached {
                    warn!(key = %cache_key, error = %e, "API failed, using cached content");
                    Ok(entry.data)
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn get_repository(&self, owner: &str, repo: &str) -> Result<Repository, String> {
        let cache_key = format!("{}/{}", owner, repo);
        let url = format!("{}/repos/{}/{}", API_BASE, owner, repo);

        let cached = {
            let cache = self.cache.read().unwrap();
            cache.repositories.get(&cache_key).cloned()
        };

        let etag = cached.as_ref().and_then(|e| e.etag.as_deref());

        match self.request_with_etag::<Repository>(&url, etag) {
            Ok((data, new_etag)) => {
                let mut cache = self.cache.write().unwrap();
                cache.repositories.insert(
                    cache_key,
                    CacheEntry {
                        data: data.clone(),
                        etag: new_etag,
                    },
                );
                Ok(data)
            }
            Err(e) if e == "not_modified" => {
                debug!(key = %cache_key, "Cache hit (304)");
                cached
                    .map(|entry| entry.data)
                    .ok_or_else(|| "not_modified without cached repository".to_string())
            }
            Err(e) if !should_use_cached_repository_on_error(&e) => {
                let had_cached = cached.is_some();
                if had_cached {
                    let mut cache = self.cache.write().unwrap();
                    cache.clear_repository_related(owner, repo);
                }
                warn!(
                    key = %cache_key,
                    error = %e,
                    had_cached = had_cached,
                    "Repository missing, refusing stale cache fallback"
                );
                Err("not found".to_string())
            }
            Err(e) => {
                if let Some(entry) = cached {
                    warn!(key = %cache_key, error = %e, "API failed, using cached repository");
                    Ok(entry.data)
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn get_latest_release(&self, owner: &str, repo: &str) -> Result<Release, String> {
        let url = format!("{}/repos/{}/{}/releases/latest", API_BASE, owner, repo);
        self.request(&url)
    }

    pub fn get_releases(&self, owner: &str, repo: &str) -> Result<Vec<Release>, String> {
        let cache_key = format!("{}/{}", owner, repo);
        let url = format!("{}/repos/{}/{}/releases?per_page=30", API_BASE, owner, repo);

        let cached = {
            let cache = self.cache.read().unwrap();
            cache.releases.get(&cache_key).cloned()
        };

        let etag = cached.as_ref().and_then(|e| e.etag.as_deref());

        match self.request_with_etag::<Vec<Release>>(&url, etag) {
            Ok((data, new_etag)) => {
                let mut cache = self.cache.write().unwrap();
                cache.releases.insert(
                    cache_key,
                    CacheEntry {
                        data: data.clone(),
                        etag: new_etag,
                    },
                );
                Ok(data)
            }
            Err(e) if e == "not_modified" => {
                debug!(key = %cache_key, "Cache hit (304)");
                cached
                    .map(|entry| entry.data)
                    .ok_or_else(|| "not_modified without cached releases".to_string())
            }
            Err(e) => {
                if let Some(entry) = cached {
                    warn!(key = %cache_key, error = %e, "API failed, using cached releases");
                    Ok(entry.data)
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn search_repositories(&self, query: &str, page: u32) -> Result<SearchResult, String> {
        self.throttle_search(RateLimitResource::Search);
        let url = format!(
            "{}/search/repositories?q={}&per_page=100&page={}",
            API_BASE,
            urlencoded(query),
            page
        );
        self.request(&url)
    }

    pub fn search_code(&self, query: &str, page: u32) -> Result<CodeSearchResult, String> {
        self.throttle_search(RateLimitResource::CodeSearch);
        let url = format!(
            "{}/search/code?q={}&per_page=100&page={}",
            API_BASE,
            urlencoded(query),
            page
        );
        self.request(&url)
    }

    pub fn get_readme(&self, owner: &str, repo: &str) -> Result<String, String> {
        let cache_key = format!("readme/{}/{}", owner, repo);
        let url = format!("{}/repos/{}/{}/readme", API_BASE, owner, repo);
        match self.get_cached_raw(cache_key, url) {
            Ok(s) => Ok(s),
            Err(e) if e == "not found" => Ok(String::new()),
            Err(e) => Err(e),
        }
    }

    pub fn repository_exists(&self, owner: &str, repo: &str) -> bool {
        self.get_repository(owner, repo).is_ok()
    }

    pub fn get_file_content(&self, owner: &str, repo: &str, path: &str) -> Result<String, String> {
        let cache_key = format!("contents/{}/{}/{}", owner, repo, path);
        let url = format!("{}/repos/{}/{}/contents/{}", API_BASE, owner, repo, path);
        self.get_cached_raw(cache_key, url)
    }

    pub fn get_file_content_at_ref(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<String, String> {
        let cache_key = format!("contents/{}/{}/{}?ref={}", owner, repo, path, git_ref);
        let url = format!(
            "{}/repos/{}/{}/contents/{}?ref={}",
            API_BASE,
            owner,
            repo,
            path,
            urlencoded(git_ref)
        );
        self.get_cached_raw(cache_key, url)
    }

    pub fn list_directory(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
    ) -> Result<Vec<String>, String> {
        let url = format!("{}/repos/{}/{}/contents/{}", API_BASE, owner, repo, path);
        let items: Vec<ContentItem> = self.request(&url)?;
        Ok(items
            .into_iter()
            .filter(|i| i.item_type == "dir")
            .map(|i| i.name)
            .collect())
    }

    pub fn get_tree(&self, owner: &str, repo: &str, branch: &str) -> Result<GitTree, String> {
        let cache_key = format!("{}/{}/{}", owner, repo, branch);
        let url = format!(
            "{}/repos/{}/{}/git/trees/{}?recursive=1",
            API_BASE, owner, repo, branch
        );

        let cached = {
            let cache = self.cache.read().unwrap();
            cache.trees.get(&cache_key).cloned()
        };

        let etag = cached.as_ref().and_then(|e| e.etag.as_deref());

        match self.request_with_etag::<GitTree>(&url, etag) {
            Ok((data, new_etag)) => {
                // Do not cache truncated trees — they are incomplete and would cause
                // false "plugin not found" deletions on subsequent runs when the API
                // is rate-limited and falls back to this stale cache.
                if data.truncated {
                    warn!(
                        key = %cache_key,
                        "Tree truncated by GitHub, not caching (would cause incomplete data)"
                    );
                    return Ok(data);
                }
                let mut cache = self.cache.write().unwrap();
                cache.trees.insert(
                    cache_key,
                    CacheEntry {
                        data: data.clone(),
                        etag: new_etag,
                    },
                );
                Ok(data)
            }
            Err(e) if e == "not_modified" => {
                debug!(key = %cache_key, "Cache hit (304)");
                let Some(entry) = cached else {
                    return Err("not_modified without cached tree".to_string());
                };
                // Old cache files may already contain truncated trees. A 304 means
                // GitHub did not send a fresh body, so reject incomplete cache here
                // too instead of bypassing the non-cache path check above.
                if entry.data.truncated {
                    warn!(
                        key = %cache_key,
                        "Cached tree is truncated after 304, refusing to use incomplete data"
                    );
                    return Err("cached tree is truncated".to_string());
                }
                Ok(entry.data)
            }
            Err(e) => {
                if let Some(entry) = cached {
                    // Never use a truncated tree from cache — it is incomplete and
                    // would cause plugins to be falsely marked as deleted.
                    if entry.data.truncated {
                        warn!(
                            key = %cache_key,
                            error = %e,
                            "API failed and cached tree is truncated, refusing to use incomplete data"
                        );
                        return Err(e);
                    }
                    warn!(key = %cache_key, error = %e, "API failed, using cached tree");
                    Ok(entry.data)
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn get_contributors_by_url(&self, url: &str) -> Result<Vec<Contributor>, String> {
        if url.is_empty() {
            return Ok(Vec::new());
        }

        let mut contributors = Vec::new();

        for page in 1..=MAX_GITHUB_PAGES {
            let page_url = with_pagination(url, page, GITHUB_PAGE_SIZE);
            let cache_key = repository_cache_key_from_url(&page_url);
            let cached = {
                let cache = self.cache.read().unwrap();
                cache.contributors.get(&cache_key).cloned()
            };

            let etag = cached.as_ref().and_then(|e| e.etag.as_deref());

            let page_contributors =
                match self.request_with_etag::<Vec<Contributor>>(&page_url, etag) {
                    Ok((data, new_etag)) => {
                        let mut cache = self.cache.write().unwrap();
                        cache.contributors.insert(
                            cache_key.clone(),
                            CacheEntry {
                                data: data.clone(),
                                etag: new_etag,
                            },
                        );
                        data
                    }
                    Err(e) if e == "not_modified" => {
                        debug!(key = %cache_key, "Cache hit (304)");
                        cached
                            .map(|entry| entry.data)
                            .ok_or_else(|| "not_modified without cached contributors".to_string())?
                    }
                    Err(e) => {
                        if let Some(entry) = cached {
                            warn!(
                                key = %cache_key,
                                error = %e,
                                "API failed, using cached contributors"
                            );
                            entry.data
                        } else {
                            return Err(e);
                        }
                    }
                };
            let page_count = page_contributors.len();

            if page_count == 0 {
                break;
            }

            contributors.extend(page_contributors);

            if page_count < GITHUB_PAGE_SIZE {
                break;
            }
        }

        Ok(contributors)
    }

    pub fn execute_parallel<T, R, F>(&self, items: Vec<T>, handler: F) -> BatchResult<R>
    where
        T: Send + Clone + 'static,
        R: Send + std::fmt::Debug + 'static,
        F: Fn(T, &GitHubClient) -> R + Send + Sync + 'static,
    {
        let _span = info_span!("execute_parallel", total = items.len()).entered();
        let results: Arc<Mutex<Vec<R>>> = Arc::new(Mutex::new(Vec::new()));
        let client = Arc::new(self.clone());
        let handler = Arc::new(handler);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let processed_count = Arc::new(AtomicUsize::new(0));
        let total = items.len();
        let num_chunks = total.div_ceil(MAX_CONCURRENT);

        for (chunk_idx, chunk) in items.chunks(MAX_CONCURRENT).enumerate() {
            if stop_flag.load(Ordering::SeqCst) {
                break;
            }

            let _chunk_span = debug_span!(
                "chunk",
                idx = chunk_idx,
                of = num_chunks,
                size = chunk.len()
            )
            .entered();
            let mut handles = Vec::new();

            for item in chunk {
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }

                let item = item.clone();
                let client = Arc::clone(&client);
                let handler = Arc::clone(&handler);
                let results = Arc::clone(&results);
                let stop_flag = Arc::clone(&stop_flag);
                let processed_count = Arc::clone(&processed_count);

                let handle = thread::spawn(move || {
                    if stop_flag.load(Ordering::SeqCst) {
                        return;
                    }

                    if !client.rate_limit.has_core_remaining() {
                        stop_flag.store(true, Ordering::SeqCst);
                        return;
                    }

                    let result = handler(item, &client);

                    if !client.rate_limit.has_core_remaining() {
                        stop_flag.store(true, Ordering::SeqCst);
                    }

                    results.lock().unwrap().push(result);
                    processed_count.fetch_add(1, Ordering::SeqCst);
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.join();
            }

            let current = processed_count.load(Ordering::SeqCst);
            debug!(processed = current, total = total, "Chunk completed");

            if chunk_idx < num_chunks - 1 && !stop_flag.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(1500));
            }
        }

        let results = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
        let processed = results.len();
        let stopped = stop_flag.load(Ordering::SeqCst);

        if stopped {
            warn!(
                processed = processed,
                total = total,
                "Batch stopped by rate limit"
            );
        }

        BatchResult {
            results,
            processed,
            total,
            stopped_by_rate_limit: stopped,
        }
    }
}

pub struct BatchResult<R> {
    pub results: Vec<R>,
    pub processed: usize,
    pub total: usize,
    pub stopped_by_rate_limit: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_repository() -> Repository {
        serde_json::from_value(json!({
            "id": 1,
            "full_name": "owner/repo",
            "name": "repo",
            "owner": {
                "login": "owner"
            }
        }))
        .unwrap()
    }

    #[test]
    fn classifies_search_rate_limit_resources_from_url() {
        assert_eq!(
            RateLimitResource::from_url("https://api.github.com/search/code?q=plugin.yml"),
            RateLimitResource::CodeSearch
        );
        assert_eq!(
            RateLimitResource::from_url("https://api.github.com/search/repositories?q=nukkit"),
            RateLimitResource::Search
        );
        assert_eq!(
            RateLimitResource::from_url("https://api.github.com/repos/owner/repo"),
            RateLimitResource::Core
        );
    }

    #[test]
    fn keeps_search_remaining_out_of_core_bucket() {
        let client = GitHubClient::new(Some("token".to_string()));

        client.update_rate_limit_from_headers(
            RateLimitResource::Search,
            Some("9"),
            Some("10"),
            Some("1"),
        );

        assert_eq!(client.rate_limit.search_remaining(), 9);
        assert_eq!(client.rate_limit.remaining(), usize::MAX);
        assert!(client.rate_limit.has_core_remaining());

        client.update_rate_limit_from_headers(
            RateLimitResource::Core,
            Some("49"),
            Some("5000"),
            Some("1"),
        );

        assert_eq!(client.rate_limit.remaining(), 49);
        assert!(!client.rate_limit.has_core_remaining());
    }

    #[test]
    fn cloned_clients_share_rate_limit_and_metrics() {
        let client = GitHubClient::new(Some("token".to_string()));
        let cloned = client.clone();

        cloned.update_rate_limit_from_headers(
            RateLimitResource::Core,
            Some("123"),
            Some("5000"),
            Some("1"),
        );
        cloned.api_calls.fetch_add(3, Ordering::SeqCst);

        assert_eq!(client.rate_limit.remaining(), 123);
        assert_eq!(client.api_calls(), 3);
    }

    #[test]
    fn updates_rate_limit_from_response_headers_using_fallback_resource() {
        let client = GitHubClient::new(Some("token".to_string()));
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert("X-RateLimit-Resource", "search".parse().unwrap());
        headers.insert("X-RateLimit-Remaining", "7".parse().unwrap());
        headers.insert("X-RateLimit-Limit", "10".parse().unwrap());
        headers.insert("X-RateLimit-Reset", "123".parse().unwrap());

        client.update_rate_limit_from_response_headers(RateLimitResource::CodeSearch, &headers);

        assert_eq!(client.rate_limit.code_search_remaining(), 7);
        assert_eq!(client.rate_limit.search_remaining(), usize::MAX);
    }

    #[test]
    fn missing_repository_errors_never_use_cached_repository() {
        assert!(!should_use_cached_repository_on_error("HTTP status 404"));
        assert!(!should_use_cached_repository_on_error("not found"));
        assert!(should_use_cached_repository_on_error(
            "Rate limited after 3 attempts"
        ));
    }

    #[test]
    fn evicts_repository_related_cache_entries() {
        let repo = sample_repository();
        let mut cache = ResponseCache::from_data_cache(DataCache {
            repositories: [(
                "owner/repo".to_string(),
                CacheEntry {
                    data: repo.clone(),
                    etag: Some("repo-etag".to_string()),
                },
            )]
            .into_iter()
            .collect(),
            trees: [(
                "owner/repo/main".to_string(),
                CacheEntry {
                    data: GitTree {
                        sha: "sha".to_string(),
                        tree: Vec::new(),
                        truncated: false,
                    },
                    etag: Some("tree-etag".to_string()),
                },
            )]
            .into_iter()
            .collect(),
            releases: [(
                "owner/repo".to_string(),
                CacheEntry {
                    data: Vec::new(),
                    etag: Some("release-etag".to_string()),
                },
            )]
            .into_iter()
            .collect(),
            contributors: [(
                "owner/repo/contributors?per_page=100&page=1".to_string(),
                CacheEntry {
                    data: vec![Contributor {
                        login: "owner".to_string(),
                        avatar_url: "https://example.com/avatar.png".to_string(),
                        html_url: "https://github.com/owner".to_string(),
                        contributions: 1,
                    }],
                    etag: Some("contributors-etag".to_string()),
                },
            )]
            .into_iter()
            .collect(),
            raw_contents: [
                (
                    "readme/owner/repo".to_string(),
                    CacheEntry {
                        data: "readme".to_string(),
                        etag: Some("readme-etag".to_string()),
                    },
                ),
                (
                    "contents/owner/repo/plugin.yml".to_string(),
                    CacheEntry {
                        data: "name: test".to_string(),
                        etag: Some("content-etag".to_string()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        });

        cache.clear_repository_related("owner", "repo");

        assert!(cache.repositories.is_empty());
        assert!(cache.trees.is_empty());
        assert!(cache.releases.is_empty());
        assert!(cache.contributors.is_empty());
        assert!(cache.raw_contents.is_empty());
    }

    #[test]
    fn only_evicts_tree_cache_for_exact_repository_prefix() {
        let mut cache = ResponseCache::from_data_cache(DataCache {
            repositories: HashMap::new(),
            trees: [
                (
                    "owner/repo/main".to_string(),
                    CacheEntry {
                        data: GitTree {
                            sha: "sha-1".to_string(),
                            tree: Vec::new(),
                            truncated: false,
                        },
                        etag: Some("tree-etag-1".to_string()),
                    },
                ),
                (
                    "owner/repo2/main".to_string(),
                    CacheEntry {
                        data: GitTree {
                            sha: "sha-2".to_string(),
                            tree: Vec::new(),
                            truncated: false,
                        },
                        etag: Some("tree-etag-2".to_string()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
            releases: HashMap::new(),
            contributors: HashMap::new(),
            raw_contents: HashMap::new(),
        });

        cache.clear_repository_related("owner", "repo");

        assert!(!cache.trees.contains_key("owner/repo/main"));
        assert!(cache.trees.contains_key("owner/repo2/main"));
    }

    #[test]
    fn only_evicts_contributor_cache_for_exact_repository_prefix() {
        let mut cache = ResponseCache::from_data_cache(DataCache {
            repositories: HashMap::new(),
            trees: HashMap::new(),
            releases: HashMap::new(),
            contributors: [
                (
                    "owner/repo/contributors?per_page=100&page=1".to_string(),
                    CacheEntry {
                        data: vec![Contributor {
                            login: "owner".to_string(),
                            avatar_url: "https://example.com/avatar.png".to_string(),
                            html_url: "https://github.com/owner".to_string(),
                            contributions: 1,
                        }],
                        etag: Some("contributors-etag-1".to_string()),
                    },
                ),
                (
                    "owner/repo2/contributors?per_page=100&page=1".to_string(),
                    CacheEntry {
                        data: vec![Contributor {
                            login: "owner".to_string(),
                            avatar_url: "https://example.com/avatar.png".to_string(),
                            html_url: "https://github.com/owner".to_string(),
                            contributions: 1,
                        }],
                        etag: Some("contributors-etag-2".to_string()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
            raw_contents: HashMap::new(),
        });

        cache.clear_repository_related("owner", "repo");

        assert!(
            !cache
                .contributors
                .contains_key("owner/repo/contributors?per_page=100&page=1")
        );
        assert!(
            cache
                .contributors
                .contains_key("owner/repo2/contributors?per_page=100&page=1")
        );
    }

    #[test]
    fn exports_contributor_cache_entries() {
        let mut cache = ResponseCache::from_data_cache(DataCache::default());
        cache.contributors.insert(
            "owner/repo/contributors?per_page=100&page=1".to_string(),
            CacheEntry {
                data: vec![Contributor {
                    login: "owner".to_string(),
                    avatar_url: "https://example.com/avatar.png".to_string(),
                    html_url: "https://github.com/owner".to_string(),
                    contributions: 1,
                }],
                etag: Some("contributors-etag".to_string()),
            },
        );

        let exported = cache.to_data_cache();

        assert_eq!(
            exported
                .contributors
                .get("owner/repo/contributors?per_page=100&page=1")
                .unwrap()
                .etag
                .as_deref(),
            Some("contributors-etag")
        );
    }
}

fn urlencoded(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            ':' => result.push_str("%3A"),
            ' ' => result.push_str("%20"),
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    result
}
