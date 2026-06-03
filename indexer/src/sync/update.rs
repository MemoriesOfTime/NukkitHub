use super::builder::{
    BuildOptions, build_plugins_from_nukkit_with_tree_options, find_plugin_manifest_paths,
};
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

    let manifest_paths = find_plugin_manifest_paths(&tree);
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

    if force || plugin_changed(plugin, &new_plugin) {
        Ok(UpdateStatus::Updated(Box::new(new_plugin)))
    } else {
        Ok(UpdateStatus::Unchanged)
    }
}

fn merge_preserved_categories(old: &Plugin, new: &mut Plugin) {
    new.categories = old.categories.clone();
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
        || versions_changed(&old.versions, &new.versions)
}

fn versions_changed(old: &[crate::plugin::Version], new: &[crate::plugin::Version]) -> bool {
    if old.len() != new.len() {
        return true;
    }

    for (o, n) in old.iter().zip(new.iter()) {
        if o.version != n.version || o.downloads != n.downloads {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::{
        UpdateStatus, is_missing_repo_error, merge_preserved_categories, plugin_changed,
        should_mark_update_processed,
    };
    use crate::plugin::Plugin;

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
