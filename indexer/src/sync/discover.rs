use super::builder::{build_plugins_from_nukkit_with_tree, find_plugin_manifest_paths};
use crate::github::client;
use crate::plugin::Plugin;
use chrono::{Datelike, Duration, NaiveDate, Utc};
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
    "topic:powernukkitx-plugin fork:true",
    "topic:pnx-plugin fork:true",
    "topic:lumi-plugin fork:true",
];

const KEYWORD_QUERIES: &[&str] = &[
    "nukkit plugin in:name,description,readme fork:true",
    "nukkit-mot plugin in:name,description,readme fork:true",
    "powernukkitx plugin in:name,description,readme fork:true",
    "pnx plugin in:name,description,readme fork:true",
    "lumi plugin in:name,description,readme fork:true",
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
    pub collection_complete: bool,
    progress: Option<DiscoverProgress>,
}

impl DiscoverResult {
    pub fn save_progress(&self) {
        if !self.can_resume_progress() {
            return;
        }
        if let Some(progress) = &self.progress {
            progress.save();
        }
    }

    pub fn can_resume_progress(&self) -> bool {
        self.collection_complete
    }

    pub fn can_finalize_sync(&self) -> bool {
        self.collection_complete && self.processed == self.total && self.errors.is_empty()
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

    if checkpoint.collection_rate_limited {
        info!("Ignoring discover progress collected under rate limit");
        return None;
    }

    Some(DiscoverProgress::from_checkpoint(checkpoint))
}

struct CollectResult {
    matches: Vec<RepoMatch>,
    rate_limited: bool,
    complete: bool,
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
    let (mut progress, collection_complete) = if let Some(progress) = saved_progress {
        info!(
            processed = progress.processed_repos.len(),
            total = progress.candidates.len(),
            "Resuming discover progress"
        );
        (progress, true)
    } else {
        let collect = {
            let _span = info_span!("collect_repos").entered();
            match last_sync {
                Some(date) => collect_repo_matches_incremental(existing_repos, date),
                None => collect_repo_matches_full(existing_repos),
            }
        };

        (
            DiscoverProgress {
                scan_key,
                candidates: collect.matches,
                processed_repos: HashSet::new(),
                collection_rate_limited: collect.rate_limited,
            },
            collect.complete,
        )
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
            collection_complete,
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
        collection_complete,
        progress: Some(progress),
    }
}

fn collect_repo_matches_incremental(
    existing_repos: &HashSet<String>,
    since: &str,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;

    let code_collect =
        collect_repo_matches_by_code_queries(CODE_SEARCH_QUERIES, existing_repos, Some(since));
    merge_collect_result(&mut matches, &mut rate_limited, &mut complete, code_collect);

    let topic_collect = collect_repo_matches_by_repository_queries(
        TOPIC_QUERIES,
        "topic",
        existing_repos,
        Some(since),
    );
    merge_collect_result(
        &mut matches,
        &mut rate_limited,
        &mut complete,
        topic_collect,
    );

    let keyword_collect = collect_repo_matches_by_repository_queries(
        KEYWORD_QUERIES,
        "keyword",
        existing_repos,
        Some(since),
    );
    merge_collect_result(
        &mut matches,
        &mut rate_limited,
        &mut complete,
        keyword_collect,
    );

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_full(existing_repos: &HashSet<String>) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;

    let code_collect =
        collect_repo_matches_by_code_queries(CODE_SEARCH_QUERIES, existing_repos, None);
    merge_collect_result(&mut matches, &mut rate_limited, &mut complete, code_collect);

    let topic_collect =
        collect_repo_matches_by_repository_queries(TOPIC_QUERIES, "topic", existing_repos, None);
    merge_collect_result(
        &mut matches,
        &mut rate_limited,
        &mut complete,
        topic_collect,
    );

    let keyword_collect = collect_repo_matches_by_repository_queries(
        KEYWORD_QUERIES,
        "keyword",
        existing_repos,
        None,
    );
    merge_collect_result(
        &mut matches,
        &mut rate_limited,
        &mut complete,
        keyword_collect,
    );

    info!(count = matches.len(), "Merged repository search results");

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_by_code_queries(
    queries: &[&str],
    existing_repos: &HashSet<String>,
    since: Option<&str>,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;

    for query in queries {
        let collect = collect_repo_matches_for_code_query(query, existing_repos, since);
        merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect);
    }

    info!(count = matches.len(), "Found repos via code search");

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_for_code_query(
    query: &str,
    existing_repos: &HashSet<String>,
    since: Option<&str>,
) -> CollectResult {
    if let Some(since) = since {
        let incremental_query = pushed_after_query(query, since);
        match collect_repo_matches_from_code_search(&incremental_query, existing_repos) {
            Ok(collect) => collect,
            Err(total) => {
                info!(
                    total = total,
                    query = %query,
                    since = %since,
                    "Incremental code query truncated, using month shards"
                );
                let since_date = match parse_sync_date(since) {
                    Some(date) => date,
                    None => {
                        warn!(since = %since, query = %query, "Invalid since date, skipping code query");
                        return CollectResult {
                            matches: Vec::new(),
                            rate_limited: false,
                            complete: false,
                        };
                    }
                };
                collect_repo_matches_by_code_month(
                    query,
                    existing_repos,
                    since_date,
                    Utc::now().date_naive(),
                )
            }
        }
    } else {
        match collect_repo_matches_from_code_search(query, existing_repos) {
            Ok(collect) => collect,
            Err(total) => {
                info!(
                    total = total,
                    query = %query,
                    "Code query exceeds 1000 results, using year-based sharding"
                );
                collect_repo_matches_by_code_year(query, existing_repos)
            }
        }
    }
}

fn collect_repo_matches_by_code_year(
    query: &str,
    existing_repos: &HashSet<String>,
) -> CollectResult {
    let today = Utc::now().date_naive();
    let current_year = today.year();
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;

    for year in START_YEAR..=current_year {
        let start = match NaiveDate::from_ymd_opt(year, 1, 1) {
            Some(date) => date,
            None => continue,
        };
        let end = if year == current_year {
            today
        } else {
            match NaiveDate::from_ymd_opt(year, 12, 31) {
                Some(date) => date,
                None => continue,
            }
        };

        let _span = debug_span!("search_code_year", year = year).entered();
        let year_query = pushed_range_query(query, start, end);
        match collect_repo_matches_from_code_search(&year_query, existing_repos) {
            Ok(collect) => {
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect)
            }
            Err(total) => {
                info!(
                    year = year,
                    total = total,
                    query = %query,
                    "Code year shard exceeds 1000 results, using month-based sharding"
                );
                let month_collect =
                    collect_repo_matches_by_code_month(query, existing_repos, start, end);
                merge_collect_result(
                    &mut matches,
                    &mut rate_limited,
                    &mut complete,
                    month_collect,
                );
            }
        }
    }

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_by_code_month(
    query: &str,
    existing_repos: &HashSet<String>,
    start: NaiveDate,
    end: NaiveDate,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;
    let mut cursor = start;

    while cursor <= end {
        let month_start = cursor;
        let month_end = last_day_of_month(cursor).min(end);
        let month_query = pushed_range_query(query, month_start, month_end);

        let _span = debug_span!(
            "search_code_month",
            year = month_start.year(),
            month = month_start.month()
        )
        .entered();
        match collect_repo_matches_from_code_search(&month_query, existing_repos) {
            Ok(collect) => {
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect)
            }
            Err(total) => {
                info!(
                    year = month_start.year(),
                    month = month_start.month(),
                    total = total,
                    query = %query,
                    "Code month shard exceeds 1000 results, using day-based sharding"
                );
                let day_collect =
                    collect_repo_matches_by_code_day(query, existing_repos, month_start, month_end);
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, day_collect);
            }
        }

        cursor = month_end + Duration::days(1);
    }

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_by_code_day(
    query: &str,
    existing_repos: &HashSet<String>,
    start: NaiveDate,
    end: NaiveDate,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;
    let mut cursor = start;

    while cursor <= end {
        let day_query = pushed_range_query(query, cursor, cursor);
        let _span = debug_span!("search_code_day", date = %cursor).entered();
        match collect_repo_matches_from_code_search(&day_query, existing_repos) {
            Ok(collect) => {
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect)
            }
            Err(total) => {
                complete = false;
                warn!(
                    date = %cursor,
                    total = total,
                    query = %query,
                    "Code day shard exceeds 1000 results, skipping shard"
                );
            }
        }
        cursor += Duration::days(1);
    }

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_by_repository_queries(
    queries: &[&str],
    source: &str,
    existing_repos: &HashSet<String>,
    since: Option<&str>,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;

    for query in queries {
        let collect = collect_repo_matches_for_repository_query(query, existing_repos, since);
        merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect);
    }

    info!(
        source = source,
        count = matches.len(),
        "Found repos via repository search"
    );

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_for_repository_query(
    query: &str,
    existing_repos: &HashSet<String>,
    since: Option<&str>,
) -> CollectResult {
    if let Some(since) = since {
        let incremental_query = pushed_after_query(query, since);
        match collect_repo_matches_from_repository_search(&incremental_query, existing_repos) {
            Ok(collect) => collect,
            Err(total) => {
                info!(
                    total = total,
                    query = %query,
                    since = %since,
                    "Incremental repository query truncated, using month shards"
                );
                let since_date = match parse_sync_date(since) {
                    Some(date) => date,
                    None => {
                        warn!(since = %since, query = %query, "Invalid since date, skipping query");
                        return CollectResult {
                            matches: Vec::new(),
                            rate_limited: false,
                            complete: false,
                        };
                    }
                };
                collect_repo_matches_by_month(
                    query,
                    existing_repos,
                    since_date,
                    Utc::now().date_naive(),
                )
            }
        }
    } else {
        match collect_repo_matches_from_repository_search(query, existing_repos) {
            Ok(collect) => collect,
            Err(total) => {
                info!(
                    total = total,
                    query = %query,
                    "Repository query exceeds 1000 results, using year-based sharding"
                );
                collect_repo_matches_by_year(query, existing_repos)
            }
        }
    }
}

fn collect_repo_matches_by_year(query: &str, existing_repos: &HashSet<String>) -> CollectResult {
    let today = Utc::now().date_naive();
    let current_year = today.year();
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;

    for year in START_YEAR..=current_year {
        let start = match NaiveDate::from_ymd_opt(year, 1, 1) {
            Some(date) => date,
            None => continue,
        };
        let end = if year == current_year {
            today
        } else {
            match NaiveDate::from_ymd_opt(year, 12, 31) {
                Some(date) => date,
                None => continue,
            }
        };

        let _span = debug_span!("search_year", year = year).entered();
        let year_query = pushed_range_query(query, start, end);
        match collect_repo_matches_from_repository_search(&year_query, existing_repos) {
            Ok(collect) => {
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect)
            }
            Err(total) => {
                info!(
                    year = year,
                    total = total,
                    query = %query,
                    "Year shard exceeds 1000 results, using month-based sharding"
                );
                let month_collect =
                    collect_repo_matches_by_month(query, existing_repos, start, end);
                merge_collect_result(
                    &mut matches,
                    &mut rate_limited,
                    &mut complete,
                    month_collect,
                );
            }
        }
    }

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_by_month(
    query: &str,
    existing_repos: &HashSet<String>,
    start: NaiveDate,
    end: NaiveDate,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;
    let mut cursor = start;

    while cursor <= end {
        let month_start = cursor;
        let month_end = last_day_of_month(cursor).min(end);
        let month_query = pushed_range_query(query, month_start, month_end);

        let _span = debug_span!(
            "search_month",
            year = month_start.year(),
            month = month_start.month()
        )
        .entered();
        match collect_repo_matches_from_repository_search(&month_query, existing_repos) {
            Ok(collect) => {
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect)
            }
            Err(total) => {
                info!(
                    year = month_start.year(),
                    month = month_start.month(),
                    total = total,
                    query = %query,
                    "Month shard exceeds 1000 results, using day-based sharding"
                );
                let day_collect =
                    collect_repo_matches_by_day(query, existing_repos, month_start, month_end);
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, day_collect);
            }
        }

        cursor = month_end + Duration::days(1);
    }

    CollectResult {
        matches,
        rate_limited,
        complete,
    }
}

fn collect_repo_matches_by_day(
    query: &str,
    existing_repos: &HashSet<String>,
    start: NaiveDate,
    end: NaiveDate,
) -> CollectResult {
    let mut matches = Vec::new();
    let mut rate_limited = false;
    let mut complete = true;
    let mut cursor = start;

    while cursor <= end {
        let day_query = pushed_range_query(query, cursor, cursor);
        let _span = debug_span!("search_day", date = %cursor).entered();
        match collect_repo_matches_from_repository_search(&day_query, existing_repos) {
            Ok(collect) => {
                merge_collect_result(&mut matches, &mut rate_limited, &mut complete, collect)
            }
            Err(total) => {
                complete = false;
                warn!(
                    date = %cursor,
                    total = total,
                    query = %query,
                    "Day shard exceeds 1000 results, skipping shard"
                );
            }
        }
        cursor = cursor + Duration::days(1);
    }

    CollectResult {
        matches,
        rate_limited,
        complete,
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

fn merge_collect_result(
    base: &mut Vec<RepoMatch>,
    rate_limited: &mut bool,
    complete: &mut bool,
    collect: CollectResult,
) {
    *rate_limited = *rate_limited || collect.rate_limited;
    *complete = *complete && collect.complete;
    merge_repo_matches(base, collect.matches);
}

fn mark_collect_incomplete_on_search_timeout(
    complete: &mut bool,
    incomplete_results: bool,
    source: &str,
    query: &str,
    page: u32,
) {
    if !incomplete_results {
        return;
    }

    *complete = false;
    warn!(
        source = source,
        page = page,
        query = %query,
        "Search returned incomplete results; collection will not be treated as complete"
    );
}

fn should_mark_repo_processed(result: &Result<Vec<Plugin>, String>) -> bool {
    result.is_ok()
}

fn is_repo_missing_error(error: &str) -> bool {
    error == "not found" || error.contains("404")
}

fn collect_repo_matches_from_code_search(
    query: &str,
    existing_repos: &HashSet<String>,
) -> Result<CollectResult, u64> {
    let first = match client().search_code(query, 1) {
        Ok(r) => r,
        Err(e) => {
            let rate_limited = e.contains("Rate limited");
            warn!(error = %e, query = %query, "Code search error");
            return Ok(CollectResult {
                matches: Vec::new(),
                rate_limited,
                complete: false,
            });
        }
    };

    if first.total_count > SHARD_LIMIT {
        return Err(first.total_count);
    }

    let mut repo_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut rate_limited = false;
    let mut complete = true;

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

    mark_collect_incomplete_on_search_timeout(
        &mut complete,
        first.incomplete_results,
        "code",
        query,
        1,
    );
    process_items(&first.items);

    if first.items.len() >= 100 {
        for page in 2..=10 {
            match client().search_code(query, page) {
                Ok(result) => {
                    if result.items.is_empty() {
                        break;
                    }
                    mark_collect_incomplete_on_search_timeout(
                        &mut complete,
                        result.incomplete_results,
                        "code",
                        query,
                        page,
                    );
                    process_items(&result.items);
                    if result.items.len() < 100 {
                        break;
                    }
                }
                Err(e) => {
                    complete = false;
                    if e.contains("Rate limited") {
                        rate_limited = true;
                    }
                    warn!(error = %e, page = page, query = %query, "Code search error");
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
        complete,
    })
}

fn collect_repo_matches_from_repository_search(
    query: &str,
    existing_repos: &HashSet<String>,
) -> Result<CollectResult, u64> {
    let first = match client().search_repositories(query, 1) {
        Ok(r) => r,
        Err(e) => {
            let rate_limited = e.contains("Rate limited");
            warn!(error = %e, query = %query, "Repository search error");
            return Ok(CollectResult {
                matches: Vec::new(),
                rate_limited,
                complete: false,
            });
        }
    };

    if first.total_count > SHARD_LIMIT {
        return Err(first.total_count);
    }

    let mut repo_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut rate_limited = false;
    let mut complete = true;

    let mut process_items = |items: &[crate::github::Repository]| {
        for item in items {
            let name = &item.full_name;
            if item.fork && !query.contains("fork:true") {
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
    };

    mark_collect_incomplete_on_search_timeout(
        &mut complete,
        first.incomplete_results,
        "repository",
        query,
        1,
    );
    process_items(&first.items);

    if first.items.len() >= 100 {
        for page in 2..=10 {
            match client().search_repositories(query, page) {
                Ok(result) => {
                    if result.items.is_empty() {
                        break;
                    }
                    mark_collect_incomplete_on_search_timeout(
                        &mut complete,
                        result.incomplete_results,
                        "repository",
                        query,
                        page,
                    );
                    process_items(&result.items);
                    if result.items.len() < 100 {
                        break;
                    }
                }
                Err(e) => {
                    complete = false;
                    if e.contains("Rate limited") {
                        rate_limited = true;
                    }
                    warn!(error = %e, page = page, query = %query, "Repository search error");
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
        complete,
    })
}

fn pushed_after_query(query: &str, since: &str) -> String {
    format!("{} pushed:>{}", query, since)
}

fn pushed_range_query(query: &str, start: NaiveDate, end: NaiveDate) -> String {
    format!(
        "{} pushed:{}..{}",
        query,
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d")
    )
}

fn parse_sync_date(since: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(since, "%Y-%m-%d").ok()
}

fn last_day_of_month(date: NaiveDate) -> NaiveDate {
    let first_of_next_month = if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1)
    }
    .expect("valid month boundary");

    first_of_next_month - Duration::days(1)
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
        let mark_processed = should_mark_repo_processed(&res);
        match res {
            Ok(plugins) => {
                if mark_processed {
                    processed_repos.insert(full_name.clone());
                }
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

    let repo = match client().get_repository(parts[0], parts[1]) {
        Ok(repo) => repo,
        Err(e) if is_repo_missing_error(&e) => {
            debug!(repo = %full_name, "Repository no longer exists, skip candidate");
            return Ok(Vec::new());
        }
        Err(e) => return Err(e),
    };

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
    use super::{
        CODE_SEARCH_QUERIES, CollectResult, DiscoverProgress, DiscoverResult, KEYWORD_QUERIES,
        RepoMatch, TOPIC_QUERIES, is_repo_missing_error, last_day_of_month,
        mark_collect_incomplete_on_search_timeout, merge_collect_result, parse_sync_date,
        pushed_after_query, pushed_range_query, should_mark_repo_processed,
    };
    use chrono::NaiveDate;
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

    #[test]
    fn topic_queries_cover_supported_runtime_topics() {
        assert!(TOPIC_QUERIES.contains(&"topic:nukkit-plugin fork:true"));
        assert!(TOPIC_QUERIES.contains(&"topic:nukkit-mot-plugin fork:true"));
        assert!(TOPIC_QUERIES.contains(&"topic:powernukkitx-plugin fork:true"));
        assert!(TOPIC_QUERIES.contains(&"topic:pnx-plugin fork:true"));
        assert!(TOPIC_QUERIES.contains(&"topic:lumi-plugin fork:true"));
    }

    #[test]
    fn code_search_queries_cover_supported_manifest_files() {
        assert!(
            CODE_SEARCH_QUERIES
                .iter()
                .any(|query| query.contains("filename:plugin.yml"))
        );
        assert!(
            CODE_SEARCH_QUERIES
                .iter()
                .any(|query| query.contains("filename:powernukkitx.yml"))
        );
    }

    #[test]
    fn keyword_queries_cover_non_topic_fallbacks() {
        assert!(
            KEYWORD_QUERIES
                .iter()
                .any(|query| query.contains("nukkit plugin in:name,description,readme"))
        );
        assert!(
            KEYWORD_QUERIES
                .iter()
                .any(|query| query.contains("powernukkitx plugin in:name,description,readme"))
        );
        assert!(
            KEYWORD_QUERIES
                .iter()
                .any(|query| query.contains("lumi plugin in:name,description,readme"))
        );
    }

    #[test]
    fn builds_pushed_queries_for_incremental_and_range_search() {
        let start = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();

        assert_eq!(
            pushed_after_query("topic:nukkit-plugin fork:true", "2026-05-22"),
            "topic:nukkit-plugin fork:true pushed:>2026-05-22"
        );
        assert_eq!(
            pushed_range_query(
                "nukkit plugin in:name,description,readme fork:true",
                start,
                end
            ),
            "nukkit plugin in:name,description,readme fork:true pushed:2026-05-01..2026-05-31"
        );
    }

    #[test]
    fn parses_sync_dates_and_computes_month_end() {
        let date = parse_sync_date("2026-05-22").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 5, 22).unwrap());

        let february = NaiveDate::from_ymd_opt(2024, 2, 10).unwrap();
        assert_eq!(
            last_day_of_month(february),
            NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
        );
    }

    #[test]
    fn discover_result_only_resumes_with_complete_collection() {
        let resumable = DiscoverResult {
            new_plugins: Vec::new(),
            errors: Vec::new(),
            stopped_by_rate_limit: true,
            processed: 1,
            total: 2,
            collection_complete: true,
            progress: None,
        };
        let incomplete = DiscoverResult {
            new_plugins: Vec::new(),
            errors: Vec::new(),
            stopped_by_rate_limit: true,
            processed: 1,
            total: 2,
            collection_complete: false,
            progress: None,
        };

        assert!(resumable.can_resume_progress());
        assert!(!incomplete.can_resume_progress());
    }

    fn sample_discover_result() -> DiscoverResult {
        DiscoverResult {
            new_plugins: Vec::new(),
            errors: Vec::new(),
            stopped_by_rate_limit: false,
            processed: 2,
            total: 2,
            collection_complete: true,
            progress: None,
        }
    }

    #[test]
    fn discover_result_only_finalizes_when_all_candidates_succeed() {
        let complete = sample_discover_result();
        let with_errors = DiscoverResult {
            errors: vec![("owner/repo".to_string(), "boom".to_string())],
            ..sample_discover_result()
        };
        let with_pending = DiscoverResult {
            processed: 1,
            total: 2,
            ..sample_discover_result()
        };

        assert!(complete.can_finalize_sync());
        assert!(!with_errors.can_finalize_sync());
        assert!(!with_pending.can_finalize_sync());
    }

    #[test]
    fn collect_result_tracks_incomplete_shards() {
        let complete = CollectResult {
            matches: Vec::new(),
            rate_limited: false,
            complete: true,
        };
        let incomplete = CollectResult {
            complete: false,
            ..complete
        };

        assert!(complete.complete);
        assert!(!incomplete.complete);
    }

    #[test]
    fn merge_collect_result_propagates_incomplete_state() {
        let mut matches = Vec::new();
        let mut rate_limited = false;
        let mut complete = true;

        merge_collect_result(
            &mut matches,
            &mut rate_limited,
            &mut complete,
            CollectResult {
                matches: vec![RepoMatch {
                    full_name: "owner/repo".to_string(),
                    manifest_paths: Vec::new(),
                }],
                rate_limited: false,
                complete: false,
            },
        );

        assert_eq!(matches.len(), 1);
        assert!(!rate_limited);
        assert!(!complete);
    }

    #[test]
    fn incomplete_search_results_mark_collection_incomplete() {
        let mut complete = true;

        mark_collect_incomplete_on_search_timeout(
            &mut complete,
            true,
            "repository",
            "topic:nukkit-plugin",
            1,
        );

        assert!(!complete);
    }

    #[test]
    fn repo_errors_remain_pending_for_retry() {
        assert!(should_mark_repo_processed(&Ok(Vec::new())));
        assert!(!should_mark_repo_processed(&Err(
            "HTTP status 500".to_string()
        )));
        assert!(!should_mark_repo_processed(&Err(
            "Rate limited after 3 attempts".to_string()
        )));
    }

    #[test]
    fn detects_missing_repo_errors() {
        assert!(is_repo_missing_error("HTTP status 404"));
        assert!(is_repo_missing_error("not found"));
        assert!(!is_repo_missing_error("HTTP status 500"));
    }
}
