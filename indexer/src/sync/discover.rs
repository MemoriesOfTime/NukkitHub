use super::builder::{build_plugins_from_nukkit_with_tree, find_plugin_manifest_paths};
use crate::github::client;
use crate::plugin::Plugin;
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use tracing::{debug, debug_span, info, info_span, warn};

const CODE_SEARCH_QUERIES: &[&str] = &[
    "filename:plugin.yml path:src/main/resources language:YAML",
    "filename:powernukkitx.yml path:src/main/resources language:YAML",
];

const TOPIC_QUERIES: &[&str] = &[
    "topic:nukkit-plugin fork:true",
    "topic:nukkit-mot-plugin fork:true",
];

const EXCLUDED_REPOS: &[&str] = &[];

const START_YEAR: i32 = 2015;
const SHARD_LIMIT: u64 = 1000;
const DISCOVER_PROGRESS_FILE: &str = ".discover_progress.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoMatch {
    full_name: String,
    manifest_paths: Vec<String>,
}

pub struct DiscoverResult {
    pub new_plugins: Vec<Plugin>,
    pub errors: Vec<(String, String)>,
    pub stopped_by_rate_limit: bool,
    pub processed: usize,
    pub total: usize,
    progress: Option<DiscoverProgress>,
}

impl DiscoverResult {
    pub fn save_progress(&self) {
        if let Some(progress) = &self.progress {
            progress.save();
        }
    }

    pub fn mark_repos_unprocessed(&mut self, repos: &HashSet<String>) {
        if let Some(progress) = &mut self.progress {
            for repo in repos {
                progress.processed_repos.remove(repo);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoverCheckpoint {
    scan_key: String,
    candidates: Vec<RepoMatch>,
    processed_repos: Vec<String>,
    collection_rate_limited: bool,
}

#[derive(Debug, Clone)]
struct DiscoverProgress {
    scan_key: String,
    candidates: Vec<RepoMatch>,
    processed_repos: HashSet<String>,
    collection_rate_limited: bool,
}

impl DiscoverProgress {
    fn from_checkpoint(checkpoint: DiscoverCheckpoint) -> Self {
        Self {
            scan_key: checkpoint.scan_key,
            candidates: checkpoint.candidates,
            processed_repos: checkpoint.processed_repos.into_iter().collect(),
            collection_rate_limited: checkpoint.collection_rate_limited,
        }
    }

    fn to_checkpoint(&self) -> DiscoverCheckpoint {
        let mut processed_repos: Vec<_> = self.processed_repos.iter().cloned().collect();
        processed_repos.sort();

        DiscoverCheckpoint {
            scan_key: self.scan_key.clone(),
            candidates: self.candidates.clone(),
            processed_repos,
            collection_rate_limited: self.collection_rate_limited,
        }
    }

    fn save(&self) {
        let checkpoint = self.to_checkpoint();
        let content = match serde_json::to_string_pretty(&checkpoint) {
            Ok(content) => content,
            Err(e) => {
                warn!(error = %e, "Failed to serialize discover progress");
                return;
            }
        };

        match fs::write(DISCOVER_PROGRESS_FILE, content) {
            Ok(_) => info!(
                processed = checkpoint.processed_repos.len(),
                total = checkpoint.candidates.len(),
                "Saved discover progress"
            ),
            Err(e) => warn!(error = %e, "Failed to save discover progress"),
        }
    }
}

pub fn clear_discover_progress() {
    match fs::remove_file(DISCOVER_PROGRESS_FILE) {
        Ok(_) => info!("Cleared discover progress"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(error = %e, "Failed to clear discover progress"),
    }
}

fn discover_scan_key(last_sync: Option<&str>) -> String {
    match last_sync {
        Some(date) => format!("incremental:{}", date),
        None => "full".to_string(),
    }
}

fn read_discover_progress(scan_key: &str) -> Option<DiscoverProgress> {
    let content = match fs::read_to_string(DISCOVER_PROGRESS_FILE) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn!(error = %e, "Failed to read discover progress");
            return None;
        }
    };

    let checkpoint: DiscoverCheckpoint = match serde_json::from_str(&content) {
        Ok(checkpoint) => checkpoint,
        Err(e) => {
            warn!(error = %e, "Failed to parse discover progress, ignoring it");
            return None;
        }
    };

    if checkpoint.scan_key != scan_key {
        info!(
            current = %scan_key,
            saved = %checkpoint.scan_key,
            "Ignoring stale discover progress"
        );
        return None;
    }

    Some(DiscoverProgress::from_checkpoint(checkpoint))
}

struct CollectResult {
    matches: Vec<RepoMatch>,
    rate_limited: bool,
}

struct ProcessReposResult {
    new_plugins: Vec<Plugin>,
    errors: Vec<(String, String)>,
    processed_repos: HashSet<String>,
    stopped_by_rate_limit: bool,
}

pub fn discover_new_plugins(
    existing_ids: &HashSet<String>,
    existing_repos: &HashSet<String>,
    last_sync: Option<&str>,
    resume_progress: bool,
) -> DiscoverResult {
    let scan_key = discover_scan_key(last_sync);
    let saved_progress = if resume_progress {
        read_discover_progress(&scan_key)
    } else {
        None
    };
    let mut progress = if let Some(progress) = saved_progress {
        info!(
            processed = progress.processed_repos.len(),
            total = progress.candidates.len(),
            "Resuming discover progress"
        );
        progress
    } else {
        let collect = {
            let _span = info_span!("collect_repos").entered();
            match last_sync {
                Some(date) => collect_repo_matches_incremental(existing_repos, date),
                None => collect_repo_matches_full(existing_repos),
            }
        };

        DiscoverProgress {
            scan_key,
            candidates: collect.matches,
            processed_repos: HashSet::new(),
            collection_rate_limited: collect.rate_limited,
        }
    };

    let collection_rate_limited = progress.collection_rate_limited;

    for candidate in &progress.candidates {
        if existing_repos.contains(&candidate.full_name) {
            progress.processed_repos.insert(candidate.full_name.clone());
        }
    }

    let pending: Vec<_> = progress
        .candidates
        .iter()
        .filter(|candidate| !progress.processed_repos.contains(&candidate.full_name))
        .cloned()
        .collect();

    info!(
        pending = pending.len(),
        total = progress.candidates.len(),
        processed = progress.processed_repos.len(),
        "Found repos to process"
    );
    if pending.is_empty() {
        return DiscoverResult {
            new_plugins: Vec::new(),
            errors: Vec::new(),
            stopped_by_rate_limit: collection_rate_limited,
            processed: progress.processed_repos.len(),
            total: progress.candidates.len(),
            progress: Some(progress),
        };
    }

    let batch = {
        let _span = info_span!("process_repos", count = pending.len()).entered();
        process_repos_parallel(pending, existing_ids)
    };

    progress.processed_repos.extend(batch.processed_repos);

    DiscoverResult {
        new_plugins: batch.new_plugins,
        errors: batch.errors,
        stopped_by_rate_limit: collection_rate_limited || batch.stopped_by_rate_limit,
        processed: progress.processed_repos.len(),
        total: progress.candidates.len(),
        progress: Some(progress),
    }
}

fn collect_repo_matches_incremental(
    existing_repos: &HashSet<String>,
    since: &str,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;

    for query in CODE_SEARCH_QUERIES {
        let query = format!("{} pushed:>{}", query, since);
        match collect_repo_matches(&query, existing_repos) {
            Ok(collect) => {
                rate_limited = rate_limited || collect.rate_limited;
                merge_repo_matches(&mut matches, collect.matches);
            }
            Err(total) => {
                warn!(total = total, query = %query, "Incremental query truncated, skipping shard")
            }
        }
    }

    let topic_collect = collect_repo_matches_by_topic(existing_repos, Some(since));
    rate_limited = rate_limited || topic_collect.rate_limited;
    merge_repo_matches(&mut matches, topic_collect.matches);

    CollectResult {
        matches,
        rate_limited,
    }
}

fn collect_repo_matches_full(existing_repos: &HashSet<String>) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;

    for query in CODE_SEARCH_QUERIES {
        let query_matches = match collect_repo_matches(query, existing_repos) {
            Ok(collect) => {
                rate_limited = rate_limited || collect.rate_limited;
                collect.matches
            }
            Err(total) => {
                info!(
                    total = total,
                    query = %query,
                    "Results exceed 1000, using year-based sharding"
                );
                let year_collect = collect_repo_matches_by_year(query, existing_repos);
                rate_limited = rate_limited || year_collect.rate_limited;
                year_collect.matches
            }
        };

        merge_repo_matches(&mut matches, query_matches);
    }

    let topic_collect = collect_repo_matches_by_topic(existing_repos, None);
    info!(
        code_count = matches.len(),
        topic_count = topic_collect.matches.len(),
        "Merging code search and topic search results"
    );
    rate_limited = rate_limited || topic_collect.rate_limited;
    merge_repo_matches(&mut matches, topic_collect.matches);

    CollectResult {
        matches,
        rate_limited,
    }
}

fn collect_repo_matches_by_year(query: &str, existing_repos: &HashSet<String>) -> CollectResult {
    let current_year = Utc::now().year();
    let mut repo_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut rate_limited = false;

    for year in START_YEAR..=current_year {
        let _span = debug_span!("search_year", year = year).entered();
        let year_query = format!("{} pushed:{}-01-01..{}-12-31", query, year, year);
        let year_result = match collect_repo_matches(&year_query, existing_repos) {
            Ok(collect) => {
                rate_limited = rate_limited || collect.rate_limited;
                collect.matches
            }
            Err(total) => {
                warn!(year = year, total = total, query = %year_query, "Year truncated (> 1000)");
                continue;
            }
        };

        for m in year_result {
            repo_map
                .entry(m.full_name)
                .or_default()
                .extend(m.manifest_paths);
        }
    }

    CollectResult {
        matches: repo_map
            .into_iter()
            .map(|(full_name, mut manifest_paths)| {
                manifest_paths.sort();
                manifest_paths.dedup();

                RepoMatch {
                    full_name,
                    manifest_paths,
                }
            })
            .collect(),
        rate_limited,
    }
}

fn collect_repo_matches_by_topic(
    existing_repos: &HashSet<String>,
    since: Option<&str>,
) -> CollectResult {
    let mut repo_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut rate_limited = false;

    for topic_query in TOPIC_QUERIES {
        let query = if let Some(date) = since {
            format!("{} pushed:>{}", topic_query, date)
        } else {
            topic_query.to_string()
        };

        for page in 1..=10 {
            let _span = debug_span!("topic_search", query = %query, page = page).entered();
            match client().search_repositories(&query, page) {
                Ok(result) => {
                    if result.items.is_empty() {
                        break;
                    }

                    for item in &result.items {
                        let name = &item.full_name;
                        if item.fork && !topic_query.contains("fork:true") {
                            debug!(repo = %name, "Skip fork");
                            continue;
                        }
                        if existing_repos.contains(name) {
                            debug!(repo = %name, "Skip existing");
                            continue;
                        }
                        if EXCLUDED_REPOS.contains(&name.as_str()) {
                            debug!(repo = %name, "Skip excluded");
                            continue;
                        }
                        repo_map.entry(name.clone()).or_default();
                    }

                    if result.items.len() < 100 {
                        break;
                    }
                }
                Err(e) => {
                    if e.contains("Rate limited") {
                        rate_limited = true;
                    }
                    warn!(error = %e, page = page, query = %query, "Topic search error");
                    break;
                }
            }
        }
    }

    info!(count = repo_map.len(), "Found repos via topic search");

    CollectResult {
        matches: repo_map
            .into_iter()
            .map(|(full_name, _)| RepoMatch {
                full_name,
                manifest_paths: Vec::new(),
            })
            .collect(),
        rate_limited,
    }
}

fn merge_repo_matches(base: &mut Vec<RepoMatch>, additions: Vec<RepoMatch>) {
    for addition in additions {
        if let Some(existing) = base.iter_mut().find(|candidate| {
            candidate
                .full_name
                .eq_ignore_ascii_case(&addition.full_name)
        }) {
            existing.manifest_paths.extend(addition.manifest_paths);
            existing.manifest_paths.sort();
            existing.manifest_paths.dedup();
        } else {
            base.push(addition);
        }
    }
}

fn collect_repo_matches(
    query: &str,
    existing_repos: &HashSet<String>,
) -> Result<CollectResult, u64> {
    let first = match client().search_code(query, 1) {
        Ok(r) => r,
        Err(e) => {
            let rate_limited = e.contains("Rate limited");
            warn!(error = %e, "Search error");
            return Ok(CollectResult {
                matches: Vec::new(),
                rate_limited,
            });
        }
    };

    if first.total_count > SHARD_LIMIT {
        return Err(first.total_count);
    }

    let mut repo_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut rate_limited = false;

    let mut process_items = |items: &[crate::github::CodeSearchItem]| {
        for item in items {
            let name = &item.repository.full_name;
            if item.repository.fork {
                debug!(repo = %name, "Skip fork");
                continue;
            }
            if existing_repos.contains(name) {
                debug!(repo = %name, "Skip existing");
                continue;
            }
            if EXCLUDED_REPOS.contains(&name.as_str()) {
                debug!(repo = %name, "Skip excluded");
                continue;
            }
            repo_map
                .entry(name.clone())
                .or_default()
                .push(item.path.clone());
        }
    };

    process_items(&first.items);

    if first.items.len() >= 100 {
        for page in 2..=10 {
            match client().search_code(query, page) {
                Ok(result) => {
                    if result.items.is_empty() {
                        break;
                    }
                    process_items(&result.items);
                    if result.items.len() < 100 {
                        break;
                    }
                }
                Err(e) => {
                    if e.contains("Rate limited") {
                        rate_limited = true;
                    }
                    warn!(error = %e, page = page, "Search error");
                    break;
                }
            }
        }
    }

    Ok(CollectResult {
        matches: repo_map
            .into_iter()
            .map(|(full_name, mut manifest_paths)| {
                manifest_paths.sort();
                manifest_paths.dedup();

                RepoMatch {
                    full_name,
                    manifest_paths,
                }
            })
            .collect(),
        rate_limited,
    })
}

fn process_repos_parallel(
    matches: Vec<RepoMatch>,
    existing_ids: &HashSet<String>,
) -> ProcessReposResult {
    let batch = client().execute_parallel(matches, |repo_match, _| {
        let _span = debug_span!("process_repo", repo = %repo_match.full_name).entered();
        let full_name = repo_match.full_name.clone();
        (full_name, process_single_repo(repo_match))
    });

    if batch.stopped_by_rate_limit {
        warn!(
            processed = batch.processed,
            total = batch.total,
            "Stopped early due to rate limit"
        );
    }

    let mut seen_ids: HashSet<String> = existing_ids.clone();
    let mut new_plugins = Vec::new();
    let mut errors = Vec::new();
    let mut processed_repos = HashSet::new();
    let mut stopped_by_rate_limit = batch.stopped_by_rate_limit;

    for (full_name, res) in batch.results {
        match res {
            Ok(plugins) => {
                processed_repos.insert(full_name.clone());
                for plugin in plugins {
                    if !seen_ids.contains(&plugin.id) {
                        seen_ids.insert(plugin.id.clone());
                        new_plugins.push(plugin);
                    } else {
                        debug!(id = %plugin.id, repo = %full_name, "Skip duplicate ID");
                    }
                }
            }
            Err(e) => {
                if e.contains("Rate limited") {
                    stopped_by_rate_limit = true;
                } else {
                    processed_repos.insert(full_name.clone());
                }
                errors.push((full_name, e));
            }
        }
    }

    ProcessReposResult {
        new_plugins,
        errors,
        processed_repos,
        stopped_by_rate_limit,
    }
}

fn process_single_repo(repo_match: RepoMatch) -> Result<Vec<Plugin>, String> {
    let RepoMatch {
        full_name,
        manifest_paths,
    } = repo_match;

    let parts: Vec<&str> = full_name.split('/').collect();
    if parts.len() != 2 {
        return Err("invalid repo name".to_string());
    }

    let repo = client().get_repository(parts[0], parts[1])?;

    if repo.is_template {
        debug!(repo = %full_name, "Skip template");
        return Ok(Vec::new());
    }
    if repo.archived {
        debug!(repo = %full_name, "Skip archived");
        return Ok(Vec::new());
    }
    if repo.topics.iter().any(|t| t == "noindex") {
        debug!(repo = %full_name, "Skip noindex");
        return Ok(Vec::new());
    }

    let mut prefetched_tree = None;
    let manifest_paths = if manifest_paths.is_empty() {
        match find_plugin_manifests(parts[0], parts[1], &repo)? {
            Some((paths, tree)) => {
                prefetched_tree = Some(tree);
                paths
            }
            None => Vec::new(),
        }
    } else {
        manifest_paths
    };

    if manifest_paths.is_empty() {
        debug!(repo = %full_name, "No plugin manifest found");
        return Ok(Vec::new());
    }

    let plugins = build_plugins_from_nukkit_with_tree(&repo, &manifest_paths, prefetched_tree);
    if plugins.is_empty() {
        debug!(repo = %full_name, "No plugins built");
    }

    Ok(plugins)
}

fn find_plugin_manifests(
    owner: &str,
    repo_name: &str,
    repo: &crate::github::Repository,
) -> Result<Option<(Vec<String>, Vec<crate::github::GitTreeEntry>)>, String> {
    let branch = repo.default_branch.as_deref().unwrap_or("main");

    match client().get_tree(owner, repo_name, branch) {
        Ok(tree) => {
            let tree_entries = tree.tree;
            let manifest_paths = find_plugin_manifest_paths(&tree_entries);
            Ok(Some((manifest_paths, tree_entries)))
        }
        Err(e) if e.contains("Rate limited") => Err(e),
        Err(e) => {
            debug!(repo = %format!("{}/{}", owner, repo_name), error = %e, "Failed to get tree");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiscoverProgress, RepoMatch};
    use std::collections::HashSet;

    #[test]
    fn discover_progress_checkpoint_sorts_processed_repos() {
        let progress = DiscoverProgress {
            scan_key: "incremental:2026-05-01".to_string(),
            candidates: vec![RepoMatch {
                full_name: "owner/repo".to_string(),
                manifest_paths: vec!["src/main/resources/plugin.yml".to_string()],
            }],
            processed_repos: HashSet::from(["z/repo".to_string(), "a/repo".to_string()]),
            collection_rate_limited: false,
        };

        let checkpoint = progress.to_checkpoint();

        assert_eq!(
            checkpoint.processed_repos,
            vec!["a/repo".to_string(), "z/repo".to_string()]
        );
    }
}
