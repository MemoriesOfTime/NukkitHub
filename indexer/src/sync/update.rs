use super::builder::{
    BuildOptions, build_plugins_from_nukkit_with_tree_options, find_plugin_manifest_paths,
    release_jar_files,
};
use super::discover::fork_has_own_versions;
use crate::github::client;
use crate::plugin::Plugin;
use std::collections::{HashMap, HashSet};
use tracing::{debug, debug_span, info, warn};

pub struct UpdateResult {
    pub updated: Vec<Plugin>,
    pub unchanged: Vec<String>,
    pub deleted: Vec<String>,
    pub errors: Vec<(String, String)>,
    pub processed_ids: HashSet<String>,
    pub stopped_by_rate_limit: bool,
}

fn should_mark_update_processed(status: &Result<UpdateStatus, String>) -> bool {
    status.is_ok()
}

fn is_missing_repo_error(error: &str) -> bool {
    error == "not found" || error.contains("404")
}

pub fn update_existing_plugins(plugins: &[Plugin], force: bool) -> UpdateResult {
    if plugins.is_empty() {
        return UpdateResult {
            updated: Vec::new(),
            unchanged: Vec::new(),
            deleted: Vec::new(),
            errors: Vec::new(),
            processed_ids: HashSet::new(),
            stopped_by_rate_limit: false,
        };
    }

    let batch = client().execute_parallel(plugins.to_vec(), move |plugin, _| {
        let _span = debug_span!("update_plugin", id = %plugin.id).entered();
        (plugin.id.clone(), update_plugin(&plugin, force))
    });

    let mut updated = Vec::new();
    let mut unchanged = Vec::new();
    let mut deleted = Vec::new();
    let mut errors = Vec::new();
    let mut processed_ids = HashSet::new();

    for (id, status) in batch.results {
        if should_mark_update_processed(&status) {
            processed_ids.insert(id.clone());
        }
        match status {
            Ok(UpdateStatus::Updated(plugin)) => updated.push(*plugin),
            Ok(UpdateStatus::Unchanged) => unchanged.push(id),
            Ok(UpdateStatus::Deleted) => deleted.push(id),
            Err(e) => errors.push((id, e)),
        }
    }

    info!(
        processed = batch.processed,
        total = batch.total,
        api_remaining = client().rate_limit.remaining(),
        "Batch processed"
    );

    if batch.stopped_by_rate_limit {
        warn!(
            processed = batch.processed,
            total = batch.total,
            "Stopped due to rate limit"
        );
    }

    UpdateResult {
        updated,
        unchanged,
        deleted,
        errors,
        processed_ids,
        stopped_by_rate_limit: batch.stopped_by_rate_limit,
    }
}

#[derive(Debug)]
enum UpdateStatus {
    Updated(Box<Plugin>),
    Unchanged,
    Deleted,
}

fn update_plugin(plugin: &Plugin, force: bool) -> Result<UpdateStatus, String> {
    // Parse GitHub URL to extract owner and repo
    let (owner, repo_name) =
        if let Some(url_path) = plugin.source.strip_prefix("https://github.com/") {
            match url_path.split_once('/') {
                Some((o, r)) => (o, r),
                None => return Ok(UpdateStatus::Unchanged),
            }
        } else {
            // Fallback for non-URL format (e.g., "owner/repo")
            match plugin.source.split_once('/') {
                Some((o, r)) => (o, r),
                None => return Ok(UpdateStatus::Unchanged),
            }
        };

    let repo = match client().get_repository(owner, repo_name) {
        Ok(r) => r,
        Err(e) if is_missing_repo_error(&e) => {
            debug!(id = %plugin.id, "Plugin repo not found, marking deleted");
            return Ok(UpdateStatus::Deleted);
        }
        Err(e) => return Err(e),
    };

    if repo.archived {
        debug!(id = %plugin.id, "Plugin repo archived, marking deleted");
        return Ok(UpdateStatus::Deleted);
    }
    if repo.topics.iter().any(|t| t == "noindex") {
        debug!(id = %plugin.id, "Plugin has noindex topic, marking deleted");
        return Ok(UpdateStatus::Deleted);
    }

    // Find plugin manifests in repository
    let tree = match crate::github::client().get_tree(
        owner,
        repo_name,
        &repo
            .default_branch
            .clone()
            .unwrap_or_else(|| "main".to_string()),
    ) {
        Ok(t) => t.tree,
        Err(e) => {
            return Err(format!("Failed to get tree: {}", e));
        }
    };

    let manifest_paths = manifest_paths_for_update(&tree, &plugin.manifest_path);
    if manifest_paths.is_empty() {
        debug!(id = %plugin.id, "No plugin manifest found in tree, marking deleted");
        return Ok(UpdateStatus::Deleted);
    }

    let new_plugins = build_plugins_from_nukkit_with_tree_options(
        &repo,
        &manifest_paths,
        Some(tree),
        BuildOptions::without_ai_categories(),
    );

    let new_plugin = new_plugins.into_iter().find(|p| p.id == plugin.id);

    let mut new_plugin = match new_plugin {
        Some(p) => p,
        None => {
            debug!(id = %plugin.id, "Plugin no longer in repo, marking deleted");
            return Ok(UpdateStatus::Deleted);
        }
    };

    merge_preserved_fields(plugin, &mut new_plugin);
    merge_gallery_created(plugin, &mut new_plugin);
    merge_preserved_categories(plugin, &mut new_plugin);

    // 版本终审。两条破坏性路径都要求"确认"而非"缺失":
    // - 新空旧有:builder 的 get_releases 失败会被静默成空列表,复查确认
    //   版本真消失了才放行清空,否则沿用盘上旧版本
    // - fork 双空:向 GitHub 确认 fork 确无自身 release 才删除
    if new_plugin.versions.is_empty() && !plugin.versions.is_empty() {
        if versions_gone_confirmed(owner, repo_name) {
            debug!(id = %plugin.id, "Versions gone, downgrading to pending");
        } else {
            keep_last_known_versions(plugin, &mut new_plugin);
        }
    }
    match resolve_pending(plugin, new_plugin.versions.is_empty(), repo.fork) {
        PendingDecision::Pending(pending) => new_plugin.pending = pending,
        PendingDecision::DeleteFork => {
            if fork_has_own_versions(&repo) {
                // fork 仍有自己的 release(或查询失败无法确认):降级 pending,下轮再裁
                new_plugin.pending = true;
            } else {
                debug!(id = %plugin.id, "Mirror fork without own releases, marking deleted");
                return Ok(UpdateStatus::Deleted);
            }
        }
    }

    if force || plugin_changed(plugin, &new_plugin) {
        Ok(UpdateStatus::Updated(Box::new(new_plugin)))
    } else {
        Ok(UpdateStatus::Unchanged)
    }
}

#[derive(Debug, PartialEq)]
enum PendingDecision {
    /// 插件保留,携带给定的 pending 状态
    Pending(bool),
    /// fork 且索引两侧均无版本:镜像副本候选,删除前还需 GitHub 侧确认
    DeleteFork,
}

/// 基于"确认无版本 + 旧记录是否零版本 + 是否 fork"裁决 pending 与删除。
/// "新空旧有"的瞬态复查由 update_plugin 完成后再传入。
fn resolve_pending(old: &Plugin, confirmed_no_versions: bool, repo_fork: bool) -> PendingDecision {
    if !confirmed_no_versions {
        // 版本出现(或保持存在):转正
        return PendingDecision::Pending(false);
    }
    if old.versions.is_empty() && repo_fork {
        return PendingDecision::DeleteFork;
    }
    // 零版本(或版本确认消失):保留跟踪,降级 pending
    PendingDecision::Pending(true)
}

/// get_releases 瞬态失败在 builder 中被静默成空列表:此时沿用盘上旧版本,
/// 等下一轮成功抓取再裁决。若放任空版本写盘,下一轮会把被冲空的旧记录
/// 当成"双空"确认,误删仍有 release 的 fork
fn keep_last_known_versions(old: &Plugin, new_plugin: &mut Plugin) {
    new_plugin.versions = old.versions.clone();
}

/// 复查版本是否真的消失。成功响应有 ETag 缓存(builder 刚查过则为零成本),
/// 仅当 motci 索引加载正常(否则 motci-only 插件的快照消失无法区分故障与
/// 真没版本)且查询成功、所有 release 都无 .jar 资产才确认;其余视为瞬态
fn versions_gone_confirmed(owner: &str, repo_name: &str) -> bool {
    if !crate::jenkins::jenkins_index().loaded() {
        return false;
    }
    match client().get_releases(owner, repo_name) {
        Ok(releases) => releases
            .iter()
            .all(|release| release_jar_files(release).is_empty()),
        Err(_) => false,
    }
}

fn merge_preserved_categories(old: &Plugin, new: &mut Plugin) {
    new.categories = old.categories.clone();
}

/// 树扫描只能找到 yml 清单;注解型 PNX 插件(@PluginMeta,无 yml)需要
/// 用上次索引保存的 manifest_path 兜底,否则会被误判为已删除。
fn manifest_paths_for_update(
    tree: &[crate::github::GitTreeEntry],
    stored_manifest_path: &str,
) -> Vec<String> {
    let mut manifest_paths = find_plugin_manifest_paths(tree);

    if !stored_manifest_path.is_empty()
        && tree
            .iter()
            .any(|entry| entry.entry_type == "blob" && entry.path == stored_manifest_path)
        && !manifest_paths
            .iter()
            .any(|path| path == stored_manifest_path)
    {
        manifest_paths.push(stored_manifest_path.to_string());
    }

    manifest_paths
}

fn merge_preserved_fields(old: &Plugin, new: &mut Plugin) {
    if old.preserved_fields.is_empty() {
        return;
    }

    let mut new_json = match serde_json::to_value(&*new) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => return,
    };

    for (key, value) in &old.preserved_fields {
        new_json.insert(key.clone(), value.clone());
    }

    if let Ok(merged) = serde_json::from_value(serde_json::Value::Object(new_json)) {
        *new = merged;
    }

    new.preserved_fields = old.preserved_fields.clone();
}

fn merge_gallery_created(old: &Plugin, new: &mut Plugin) {
    let old_created: HashMap<&str, &str> = old
        .gallery
        .iter()
        .filter(|g| !g.created.is_empty())
        .map(|g| (g.url.as_str(), g.created.as_str()))
        .collect();

    let now = chrono::Utc::now().format("%Y-%m-%d").to_string();

    for item in &mut new.gallery {
        if let Some(&created) = old_created.get(item.url.as_str()) {
            item.created = created.to_string();
        } else if item.created.is_empty() {
            item.created = now.clone();
        }
    }
}

fn plugin_changed(old: &Plugin, new: &Plugin) -> bool {
    old.name != new.name
        || old.targets != new.targets
        || old.primary_target != new.primary_target
        || old.manifest_path != new.manifest_path
        || old.detection_confidence != new.detection_confidence
        || old.summary != new.summary
        || old.updated_at != new.updated_at
        || old.stars != new.stars
        || old.downloads != new.downloads
        || old.license != new.license
        || old.authors != new.authors
        || old.categories != new.categories
        || old.pending != new.pending
        || versions_changed(&old.versions, &new.versions)
}

fn versions_changed(old: &[crate::plugin::Version], new: &[crate::plugin::Version]) -> bool {
    if old.len() != new.len() {
        return true;
    }

    for (o, n) in old.iter().zip(new.iter()) {
        if o.version != n.version || o.downloads != n.downloads || o.files != n.files {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::{
        PendingDecision, UpdateStatus, is_missing_repo_error, keep_last_known_versions,
        manifest_paths_for_update, merge_preserved_categories, plugin_changed, resolve_pending,
        should_mark_update_processed,
    };
    use crate::github::GitTreeEntry;
    use crate::plugin::Plugin;

    fn tree_with_paths(paths: &[&str]) -> Vec<GitTreeEntry> {
        paths
            .iter()
            .map(|path| GitTreeEntry {
                path: (*path).to_string(),
                entry_type: "blob".to_string(),
                sha: String::new(),
                size: Some(1),
            })
            .collect()
    }

    #[test]
    fn update_keeps_stored_annotation_manifest_missing_from_tree_scan() {
        let tree = tree_with_paths(&["pom.xml", "src/main/java/io/github/foo/Bar.java"]);

        let paths = manifest_paths_for_update(&tree, "src/main/java/io/github/foo/Bar.java");

        assert_eq!(paths, vec!["src/main/java/io/github/foo/Bar.java"]);
    }

    #[test]
    fn update_does_not_keep_stored_manifest_removed_from_tree() {
        let tree = tree_with_paths(&["pom.xml"]);

        let paths = manifest_paths_for_update(&tree, "src/main/resources/plugin.yml");

        assert!(paths.is_empty());
    }

    #[test]
    fn update_does_not_duplicate_stored_manifest() {
        let tree = tree_with_paths(&["src/main/resources/plugin.yml"]);

        let paths = manifest_paths_for_update(&tree, "src/main/resources/plugin.yml");

        assert_eq!(paths, vec!["src/main/resources/plugin.yml"]);
    }

    fn plugin_with_updated_at(updated_at: u64) -> Plugin {
        let mut plugin: Plugin = serde_json::from_value(serde_json::json!({
            "id": "owner/repo",
            "name": "Plugin",
            "source": "https://github.com/owner/repo"
        }))
        .unwrap();
        plugin.updated_at = updated_at;
        plugin
    }

    fn plugin_with_state(pending: bool, version_count: usize) -> Plugin {
        let mut plugin: Plugin = serde_json::from_value(serde_json::json!({
            "id": "owner/repo",
            "name": "Plugin",
            "source": "https://github.com/owner/repo",
            "versions": (0..version_count).map(|i| serde_json::json!({
                "version": format!("1.0.{}", i),
                "files": [{ "filename": "p.jar", "url": "https://example.com/p.jar" }]
            })).collect::<Vec<_>>()
        }))
        .unwrap();
        plugin.pending = pending;
        plugin
    }

    #[test]
    fn resolve_pending_deletes_fork_with_both_sides_empty() {
        let old = plugin_with_state(false, 0);
        assert_eq!(
            resolve_pending(&old, true, true),
            PendingDecision::DeleteFork
        );
    }

    #[test]
    fn resolve_pending_marks_non_fork_zero_version_as_pending() {
        let old = plugin_with_state(false, 0);
        assert_eq!(
            resolve_pending(&old, true, false),
            PendingDecision::Pending(true)
        );
    }

    #[test]
    fn resolve_pending_downgrades_when_versions_confirmed_gone() {
        // 版本确认消失(复查通过,非瞬态):降级 pending,不再保留旧状态
        let old = plugin_with_state(false, 2);
        assert_eq!(
            resolve_pending(&old, true, false),
            PendingDecision::Pending(true)
        );
        assert_eq!(
            resolve_pending(&old, true, true),
            PendingDecision::Pending(true)
        );
    }

    #[test]
    fn keep_last_known_versions_restores_versions_on_transient_empty() {
        // 新空旧有且复查未确认(瞬态失败):沿用盘上旧版本,等下轮裁决
        let old = plugin_with_state(false, 2);
        let mut rebuilt = plugin_with_state(false, 0);

        keep_last_known_versions(&old, &mut rebuilt);

        assert_eq!(rebuilt.versions.len(), 2);
        assert_eq!(rebuilt.versions[0].version, old.versions[0].version);
        assert_eq!(rebuilt.versions[1].version, old.versions[1].version);
    }

    #[test]
    fn resolve_pending_promotes_once_versions_appear() {
        let old = plugin_with_state(true, 0);
        assert_eq!(
            resolve_pending(&old, false, false),
            PendingDecision::Pending(false)
        );
        assert_eq!(
            resolve_pending(&old, false, true),
            PendingDecision::Pending(false)
        );
    }

    #[test]
    fn plugin_changed_detects_pending_transition() {
        let mut old = plugin_with_updated_at(1_612_325_106);
        old.pending = true;

        let mut new = plugin_with_updated_at(1_612_325_106);
        new.pending = false;

        assert!(plugin_changed(&old, &new));
    }

    #[test]
    fn plugin_changed_detects_updated_at_corrections() {
        let old = plugin_with_updated_at(1_777_593_600);
        let new = plugin_with_updated_at(1_612_325_106);

        assert!(plugin_changed(&old, &new));
    }

    #[test]
    fn plugin_changed_detects_category_changes() {
        let mut old = plugin_with_updated_at(1_612_325_106);
        old.categories = vec!["utility".to_string()];

        let mut new = plugin_with_updated_at(1_612_325_106);
        new.categories = vec!["economy".to_string()];

        assert!(plugin_changed(&old, &new));
    }

    // sha256 回填场景:已有索引无 sha256,重取后文件带上 sha256,
    // 必须判定为已变更才会重写 JSON
    #[test]
    fn plugin_changed_detects_new_file_sha256() {
        let file_json = |sha256: Option<&str>| {
            serde_json::json!({
                "filename": "p.jar",
                "url": "https://github.com/owner/repo/releases/download/v1/p.jar",
                "size": 10,
                "primary": true,
                "sha256": sha256,
            })
        };
        let mut old = plugin_with_updated_at(1_612_325_106);
        old.versions = vec![
            serde_json::from_value(serde_json::json!({
                "version": "v1",
                "files": [file_json(None)],
            }))
            .unwrap(),
        ];
        let mut new = plugin_with_updated_at(1_612_325_106);
        new.versions = vec![
            serde_json::from_value(serde_json::json!({
                "version": "v1",
                "files": [file_json(Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                ))],
            }))
            .unwrap(),
        ];

        assert!(plugin_changed(&old, &new));
    }

    #[test]
    fn update_preserves_existing_categories() {
        let mut old = plugin_with_updated_at(1_612_325_106);
        old.categories = vec!["economy".to_string(), "management".to_string()];

        let mut new = plugin_with_updated_at(1_612_325_106);
        new.categories = vec!["utility".to_string()];

        merge_preserved_categories(&old, &mut new);

        assert_eq!(new.categories, old.categories);
    }

    #[test]
    fn only_successful_updates_are_marked_processed() {
        assert!(should_mark_update_processed(&Ok(UpdateStatus::Unchanged)));
        assert!(should_mark_update_processed(&Ok(UpdateStatus::Deleted)));
        assert!(!should_mark_update_processed(&Err(
            "HTTP status 500".to_string()
        )));
    }

    #[test]
    fn detects_missing_repo_errors() {
        assert!(is_missing_repo_error("HTTP status 404"));
        assert!(is_missing_repo_error("not found"));
        assert!(!is_missing_repo_error("HTTP status 500"));
    }
}
