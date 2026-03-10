mod image;
mod link;

use crate::github::{Contributor, GitTreeEntry, Release, Repository, client};
use crate::plugin::{
    Author, Dependency, GalleryItem, License, Links, Plugin, Version, VersionFile,
};
use std::collections::{BTreeMap, BTreeSet};
use tracing::debug;

fn parse_timestamp(iso_string: &str) -> u64 {
    use chrono::{DateTime, Utc};
    DateTime::parse_from_rfc3339(iso_string)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).timestamp() as u64)
        .unwrap_or(0)
}

const CATEGORIES: &[&str] = &[
    "adventure",
    "cursed",
    "decoration",
    "economy",
    "equipment",
    "food",
    "game-mechanics",
    "library",
    "magic",
    "management",
    "minigame",
    "mobs",
    "optimization",
    "social",
    "storage",
    "technology",
    "transportation",
    "utility",
    "world-generation",
];

pub const TARGET_IDS: &[&str] = &["nkx", "nkmot", "pnx", "lumi"];

const MANIFEST_FILENAMES: &[&str] = &["plugin.yml", "powernukkitx.yml"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DetectionConfidence {
    Low,
    Medium,
    High,
}

impl DetectionConfidence {
    fn as_str(self) -> &'static str {
        match self {
            DetectionConfidence::Low => "low",
            DetectionConfidence::Medium => "medium",
            DetectionConfidence::High => "high",
        }
    }

    fn promote(&mut self, other: DetectionConfidence) {
        if other > *self {
            *self = other;
        }
    }
}

#[derive(Debug, Clone)]
struct TargetDetection {
    targets: Vec<String>,
    confidence: DetectionConfidence,
}

pub struct PostProcessContext<'a> {
    pub owner: &'a str,
    pub repo: &'a str,
    pub branch: &'a str,
}

type ImageProcessorFn = fn(&str, &PostProcessContext, &mut Vec<GalleryItem>) -> String;
type LinkProcessorFn = fn(&str, &PostProcessContext) -> String;

static IMAGE_PROCESSORS: &[ImageProcessorFn] =
    &[image::process_html_images, image::process_md_images];
static LINK_PROCESSORS: &[LinkProcessorFn] = &[link::process_md_links, link::process_html_links];

fn process_readme(readme: &str, ctx: &PostProcessContext) -> (String, Vec<GalleryItem>) {
    let mut content = readme.to_string();
    let mut gallery = Vec::new();

    for processor in IMAGE_PROCESSORS {
        content = processor(&content, ctx, &mut gallery);
    }

    for processor in LINK_PROCESSORS {
        content = processor(&content, ctx);
    }

    (content, gallery)
}

fn get_tree(owner: &str, repo: &str, branch: &str) -> Vec<GitTreeEntry> {
    client()
        .get_tree(owner, repo, branch)
        .map(|t| t.tree)
        .unwrap_or_default()
}

fn find_logo_url(tree: &[GitTreeEntry], owner: &str, repo: &str, branch: &str) -> Option<String> {
    let logo_paths = [
        ".github/img/logo.png",
        ".github/img/icon.png",
        "logo.png",
        "icon.png",
    ];

    for path in &logo_paths {
        if tree.iter().any(|e| e.path == *path) {
            return Some(format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                owner, repo, branch, path
            ));
        }
    }
    None
}

fn to_raw_url(owner: &str, repo: &str, branch: &str, path: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        owner, repo, branch, path
    )
}

fn find_gallery_items(
    tree: &[GitTreeEntry],
    owner: &str,
    repo: &str,
    branch: &str,
) -> Vec<GalleryItem> {
    let gallery_dir = ".github/img/";
    let excluded = ["logo.png", "icon.png"];

    tree.iter()
        .filter(|e| {
            e.entry_type == "blob"
                && e.path.starts_with(gallery_dir)
                && e.path.ends_with(".png")
                && !excluded.iter().any(|ex| e.path.ends_with(ex))
        })
        .map(|e| GalleryItem {
            url: format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                owner, repo, branch, e.path
            ),
            title: String::new(),
            description: String::new(),
            created: String::new(),
        })
        .collect()
}

pub(crate) fn is_plugin_manifest_path(path: &str) -> bool {
    path.contains("src/main/resources")
        && MANIFEST_FILENAMES.iter().any(|name| path.ends_with(name))
}

pub(crate) fn find_plugin_manifest_paths(tree: &[GitTreeEntry]) -> Vec<String> {
    let mut paths: Vec<String> = tree
        .iter()
        .filter(|entry| entry.entry_type == "blob" && is_plugin_manifest_path(&entry.path))
        .map(|entry| entry.path.clone())
        .collect();

    paths.sort();
    paths.dedup();
    paths
}

fn ordered_targets(targets: &BTreeSet<&'static str>) -> Vec<String> {
    TARGET_IDS
        .iter()
        .filter(|target| targets.contains(**target))
        .map(|target| (*target).to_string())
        .collect()
}

fn module_key_from_manifest_path(path: &str) -> String {
    path.find("src/main/resources")
        .map(|pos| path[..pos].trim_end_matches('/').to_string())
        .unwrap_or_default()
}

fn group_manifest_paths(manifest_paths: &[String]) -> Vec<Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in manifest_paths
        .iter()
        .filter(|path| is_plugin_manifest_path(path))
    {
        groups
            .entry(module_key_from_manifest_path(path))
            .or_default()
            .push(path.clone());
    }

    for paths in groups.values_mut() {
        paths.sort();
        paths.dedup();
    }

    groups.into_values().collect()
}

fn find_build_file_paths<'a>(tree: &'a [GitTreeEntry], plugin_yml_path: &str) -> Vec<&'a str> {
    let module_prefix = plugin_yml_path
        .find("src/main/resources")
        .map(|pos| &plugin_yml_path[..pos])
        .unwrap_or("");

    let build_names = ["build.gradle.kts", "build.gradle", "pom.xml"];

    let mut expected: Vec<String> = Vec::new();

    // Module-level build files (for multi-module projects)
    if !module_prefix.is_empty() {
        for name in &build_names {
            expected.push(format!("{}{}", module_prefix, name));
        }
    }

    // Root-level build files
    for name in &build_names {
        expected.push(name.to_string());
    }

    // Gradle version catalog
    expected.push("gradle/libs.versions.toml".to_string());

    tree.iter()
        .filter(|e| e.entry_type == "blob" && expected.iter().any(|exp| e.path == *exp))
        .map(|e| e.path.as_str())
        .collect()
}

fn contains_any(content_lower: &str, indicators: &[&str]) -> bool {
    indicators
        .iter()
        .any(|indicator| content_lower.contains(indicator))
}

fn detect_targets_from_topics(topics: &[String]) -> (BTreeSet<&'static str>, DetectionConfidence) {
    let mut targets = BTreeSet::new();
    let mut confidence = DetectionConfidence::Low;

    for topic in topics {
        match topic.as_str() {
            "nukkit-plugin" => {
                targets.insert("nkx");
                targets.insert("nkmot");
                confidence.promote(DetectionConfidence::Medium);
            }
            "nukkit-mot-plugin" => {
                targets.insert("nkmot");
                confidence.promote(DetectionConfidence::Medium);
            }
            "powernukkitx-plugin" | "pnx-plugin" => {
                targets.insert("pnx");
                confidence.promote(DetectionConfidence::High);
            }
            "lumi-plugin" => {
                targets.insert("lumi");
                confidence.promote(DetectionConfidence::High);
            }
            _ => {}
        }
    }

    (targets, confidence)
}

fn detect_targets_from_build_content(
    content_lower: &str,
) -> (BTreeSet<&'static str>, DetectionConfidence) {
    const NKX_INDICATORS: &[&str] = &["cloudburstmc", "opencollab.dev", "repo.nukkitx.com"];
    const NKMOT_INDICATORS: &[&str] = &["memoriesoftime", "nukkit-mot"];
    const PNX_INDICATORS: &[&str] = &[
        "cn.powernukkitx",
        "powernukkitx",
        "powernukkit/powernukkitx",
    ];
    const LUMI_INDICATORS: &[&str] = &[
        "repo.luminiadev.com",
        "com.koshakmine:lumi",
        "<groupid>com.koshakmine</groupid>",
        "<artifactid>lumi</artifactid>",
    ];

    let mut targets = BTreeSet::new();

    if contains_any(content_lower, PNX_INDICATORS) {
        targets.insert("pnx");
    }
    if contains_any(content_lower, LUMI_INDICATORS) {
        targets.insert("lumi");
    }
    if contains_any(content_lower, NKX_INDICATORS) {
        targets.insert("nkx");
    }
    if contains_any(content_lower, NKMOT_INDICATORS) {
        targets.insert("nkmot");
    }

    if !targets.is_empty() {
        return (targets, DetectionConfidence::High);
    }

    if content_lower.contains("cn.nukkit") {
        targets.insert("nkx");
        targets.insert("nkmot");
        return (targets, DetectionConfidence::Medium);
    }

    (targets, DetectionConfidence::Low)
}

fn select_primary_manifest_path(manifest_paths: &[String]) -> Option<&str> {
    manifest_paths
        .iter()
        .find(|path| path.ends_with("plugin.yml"))
        .or_else(|| {
            manifest_paths
                .iter()
                .find(|path| path.ends_with("powernukkitx.yml"))
        })
        .or_else(|| manifest_paths.first())
        .map(|path| path.as_str())
}

fn detect_targets(
    tree: &[GitTreeEntry],
    repo: &Repository,
    manifest_paths: &[String],
) -> Option<TargetDetection> {
    let (owner, repo_name) = repo.full_name.split_once('/')?;
    let primary_manifest_path = select_primary_manifest_path(manifest_paths)?;
    let mut targets = BTreeSet::new();
    let mut confidence = DetectionConfidence::Low;

    let (topic_targets, topic_confidence) = detect_targets_from_topics(&repo.topics);
    targets.extend(topic_targets);
    confidence.promote(topic_confidence);

    if manifest_paths
        .iter()
        .any(|path| path.ends_with("powernukkitx.yml"))
    {
        targets.insert("pnx");
        confidence.promote(DetectionConfidence::High);
    }
    let build_paths = find_build_file_paths(tree, primary_manifest_path);
    if build_paths.is_empty() && targets.is_empty() {
        debug!(repo = %repo.full_name, module = %module_key_from_manifest_path(primary_manifest_path), "No target evidence found");
        return None;
    }

    for path in &build_paths {
        match client().get_file_content(owner, repo_name, path) {
            Ok(content) => {
                let lower = content.to_lowercase();
                let (build_targets, build_confidence) = detect_targets_from_build_content(&lower);

                if !build_targets.is_empty() {
                    targets.extend(build_targets);
                    confidence.promote(build_confidence);
                }
            }
            Err(e) => {
                debug!(repo = %repo.full_name, file = %path, error = %e, "Failed to read build file");
            }
        }
    }

    if targets.is_empty() {
        debug!(repo = %repo.full_name, module = %module_key_from_manifest_path(primary_manifest_path), "No supported targets detected");
        return None;
    }

    Some(TargetDetection {
        targets: ordered_targets(&targets),
        confidence,
    })
}

fn slugify_segment(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}

fn build_plugin_id(
    owner: &str,
    repo_name: &str,
    manifest_path: &str,
    is_multi_module: bool,
) -> String {
    if !is_multi_module {
        return format!("{}/{}", owner, repo_name);
    }

    let module_key = module_key_from_manifest_path(manifest_path);
    let suffix = if module_key.is_empty() {
        "root".to_string()
    } else {
        slugify_segment(&module_key)
    };

    format!("{}/{}--{}", owner, repo_name, suffix)
}

pub fn build_plugins_from_nukkit(repo: &Repository, manifest_paths: &[String]) -> Vec<Plugin> {
    build_plugins_from_nukkit_with_tree(repo, manifest_paths, None)
}

pub fn build_plugins_from_nukkit_with_tree(
    repo: &Repository,
    manifest_paths: &[String],
    prefetched_tree: Option<Vec<GitTreeEntry>>,
) -> Vec<Plugin> {
    let (owner, repo_name) = match repo.full_name.split_once('/') {
        Some((o, r)) => (o, r),
        None => {
            debug!(repo = %repo.full_name, "Skip: invalid repo name");
            return Vec::new();
        }
    };

    let default_branch = repo.default_branch.as_deref().unwrap_or("main");

    let manifest_groups = group_manifest_paths(manifest_paths);
    if manifest_groups.is_empty() {
        debug!(repo = %repo.full_name, "Skip: no plugin manifests found");
        return Vec::new();
    }

    let tree = prefetched_tree.unwrap_or_else(|| get_tree(owner, repo_name, default_branch));

    let releases = client().get_releases(owner, repo_name).unwrap_or_default();
    let readme = client().get_readme(owner, repo_name).unwrap_or_default();
    let contributors = client()
        .get_contributors_by_url(&repo.contributors_url)
        .unwrap_or_default();

    let license = repo.license.as_ref().map_or_else(
        || License {
            id: "ARR".to_string(),
            name: "All Rights Reserved".to_string(),
            url: String::new(),
        },
        |l| {
            let spdx_id = &l.spdx_id;
            let is_valid_spdx = !spdx_id.is_empty()
                && spdx_id != "NOASSERTION"
                && !spdx_id.starts_with("LicenseRef");

            let url = if let Some(html_url) = &l.html_url {
                html_url.clone()
            } else if is_valid_spdx {
                format!("https://spdx.org/licenses/{}.html", spdx_id)
            } else {
                format!("{}/blob/{}/LICENSE", repo.html_url, default_branch)
            };

            License {
                id: spdx_id.clone(),
                name: l.name.clone(),
                url,
            }
        },
    );

    let icon_url = find_logo_url(&tree, owner, repo_name, default_branch)
        .unwrap_or_else(|| repo.owner.avatar_url.clone());
    let repo_gallery = find_gallery_items(&tree, owner, repo_name, default_branch);

    let is_multi_module = manifest_groups.len() > 1;
    let mut plugins = Vec::new();

    for manifest_group in manifest_groups {
        let Some(manifest_path) = select_primary_manifest_path(&manifest_group) else {
            continue;
        };

        let Some(target_detection) = detect_targets(&tree, repo, &manifest_group) else {
            debug!(repo = %repo.full_name, manifest = %manifest_path, "Skip: no supported targets detected");
            continue;
        };

        let yml_content = match client().get_file_content(owner, repo_name, manifest_path) {
            Ok(content) => content,
            Err(e) => {
                debug!(repo = %repo.full_name, manifest = %manifest_path, error = %e, "Failed to read manifest");
                continue;
            }
        };

        let nukkit_yml = match crate::nukkit::NukkitPluginYml::from_str(&yml_content) {
            Ok(yml) => yml,
            Err(e) => {
                debug!(repo = %repo.full_name, manifest = %manifest_path, error = %e, "Failed to parse manifest");
                continue;
            }
        };

        if let Some(plugin) = nukkit_yml_to_plugin(
            nukkit_yml,
            repo,
            &releases,
            &readme,
            &license,
            &contributors,
            owner,
            repo_name,
            default_branch,
            &icon_url,
            repo_gallery.clone(),
            manifest_path,
            &target_detection,
            is_multi_module,
        ) {
            plugins.push(plugin);
        }
    }

    plugins
}

fn is_placeholder(s: &str) -> bool {
    // Detect unresolved placeholders like ${project.name} or @project.name@
    let trimmed = s.trim();
    (trimmed.starts_with("${") && trimmed.ends_with('}') && trimmed.len() > 3)
        || (trimmed.starts_with('@') && trimmed.ends_with('@') && trimmed.len() > 2)
}

fn nukkit_yml_to_plugin(
    yml: crate::nukkit::NukkitPluginYml,
    repo: &Repository,
    releases: &[Release],
    readme: &str,
    license: &License,
    _contributors: &[Contributor],
    owner: &str,
    repo_name: &str,
    branch: &str,
    icon_url: &str,
    repo_gallery: Vec<GalleryItem>,
    manifest_path: &str,
    target_detection: &TargetDetection,
    is_multi_module: bool,
) -> Option<Plugin> {
    let ctx = PostProcessContext {
        owner,
        repo: repo_name,
        branch,
    };

    let (processed_readme, mut gallery) = process_readme(readme, &ctx);
    gallery.extend(repo_gallery);

    let authors = yml
        .all_authors()
        .into_iter()
        .map(|name| Author {
            name,
            url: String::new(),
            avatar_url: String::new(),
        })
        .collect();

    let mut all_dependencies: Vec<Dependency> = yml
        .depend
        .iter()
        .map(|name| Dependency {
            plugin_id: name.clone(),
            version_range: String::new(),
            dependency_type: "required".to_string(),
        })
        .collect();

    all_dependencies.extend(yml.softdepend.iter().map(|name| Dependency {
        plugin_id: name.clone(),
        version_range: String::new(),
        dependency_type: "optional".to_string(),
    }));

    // Build versions from releases
    let versions: Vec<Version> = releases
        .iter()
        .filter_map(|release| {
            let files: Vec<VersionFile> = release
                .assets
                .iter()
                .filter(|a| a.name.ends_with(".jar"))
                .map(|a| VersionFile {
                    filename: a.name.clone(),
                    url: a.browser_download_url.clone(),
                    size: a.size,
                    primary: true,
                })
                .collect();

            if files.is_empty() {
                debug!(
                    repo = %repo.full_name,
                    release = %release.tag_name,
                    "Skipping release without .jar assets"
                );
                return None;
            }

            Some(Version {
                version: release.tag_name.clone(),
                name: release
                    .name
                    .clone()
                    .unwrap_or_else(|| release.tag_name.clone()),
                prerelease: release.prerelease,
                changelog: release.body.clone().unwrap_or_default(),
                files,
                downloads: 0,
                published_at: parse_timestamp(&release.published_at),
            })
        })
        .collect();
    let api_version = yml
        .api
        .primary()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && !is_placeholder(v))
        .unwrap_or_else(|| "unknown".to_string());

    let categories: Vec<String> = repo
        .topics
        .iter()
        .filter_map(|t| {
            // Strip "nukkit-" prefix if present
            let normalized = t.strip_prefix("nukkit-").unwrap_or(t);
            if CATEGORIES.contains(&normalized) {
                Some(normalized.to_string())
            } else {
                None
            }
        })
        .collect();

    // Handle placeholder names by falling back to repo name
    let plugin_name = if is_placeholder(&yml.name) || yml.name.trim().is_empty() {
        debug!(
            repo = %repo.full_name,
            original = %yml.name,
            "Using repo name as fallback for placeholder"
        );
        repo_name.to_string()
    } else {
        yml.name.trim().to_string()
    };

    let summary = match yml.description.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() && !is_placeholder(text) => text.to_string(),
        _ => repo
            .description
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string(),
    };

    Some(Plugin {
        id: build_plugin_id(owner, repo_name, manifest_path, is_multi_module),
        name: plugin_name,
        source: repo.html_url.clone(),
        targets: target_detection.targets.clone(),
        primary_target: target_detection
            .targets
            .first()
            .cloned()
            .unwrap_or_default(),
        manifest_path: manifest_path.to_string(),
        detection_confidence: target_detection.confidence.as_str().to_string(),
        summary,
        description: processed_readme,
        authors,
        categories,
        license: license.clone(),
        links: yml.website.as_ref().map(|w| Links {
            homepage: w.clone(),
            wiki: String::new(),
            discord: String::new(),
        }),
        downloads: 0,
        stars: repo.stargazers_count,
        created_at: parse_timestamp(&repo.created_at),
        updated_at: parse_timestamp(&repo.updated_at),
        icon_url: icon_url.to_string(),
        gallery,
        versions,
        api_version,
        server_version: String::new(),
        dependencies: all_dependencies,
        preserved_fields: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_plugin_id, detect_targets_from_build_content, group_manifest_paths,
        is_plugin_manifest_path,
    };

    #[test]
    fn detects_supported_nukkit_markers() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://repo.opencollab.dev/maven-releases/") }
            }

            dependencies {
                compileOnly("cn.nukkit:nukkit:1.0-SNAPSHOT")
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["nkx"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn detects_powernukkitx_targets() {
        let gradle = r#"
            dependencies {
                compileOnly("cn.powernukkitx:powernukkitx:1.20.0-r1")
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["pnx"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn detects_nukkit_mot_targets_without_motci() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://repo.nukkit-mot.com/releases") }
            }

            dependencies {
                compileOnly("com.memoriesoftime:nukkit-mot:1.0.0")
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["nkmot"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn does_not_treat_motci_as_runtime_target_signal() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://motci.cn/repository/maven-public/") }
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert!(targets.is_empty());
        assert_eq!(confidence.as_str(), "low");
    }

    #[test]
    fn detects_lumi_targets() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://repo.luminiadev.com/snapshots") }
            }

            dependencies {
                compileOnly("com.koshakmine:Lumi:1.5.0-SNAPSHOT")
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["lumi"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn detects_lumi_targets_in_maven_pom() {
        let pom = r#"
            <dependency>
                <groupId>com.koshakmine</groupId>
                <artifactId>Lumi</artifactId>
                <version>1.5.0-SNAPSHOT</version>
            </dependency>
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&pom.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["lumi"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn treats_cn_nukkit_as_shared_nkx_and_nkmot_support() {
        let gradle = r#"
            dependencies {
                compileOnly("cn.nukkit:nukkit:1.0-SNAPSHOT")
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(
            targets.into_iter().collect::<Vec<_>>(),
            vec!["nkmot", "nkx"]
        );
        assert_eq!(confidence.as_str(), "medium");
    }

    #[test]
    fn detects_supported_manifest_paths() {
        assert!(is_plugin_manifest_path("src/main/resources/plugin.yml"));
        assert!(is_plugin_manifest_path(
            "modules/foo/src/main/resources/powernukkitx.yml"
        ));
        assert!(!is_plugin_manifest_path(
            "src/main/resources/not-plugin.yaml"
        ));
    }

    #[test]
    fn groups_manifest_paths_by_module() {
        let groups = group_manifest_paths(&[
            "src/main/resources/plugin.yml".to_string(),
            "src/main/resources/powernukkitx.yml".to_string(),
            "modules/economy/src/main/resources/plugin.yml".to_string(),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 2);
        assert_eq!(groups[1].len(), 1);
    }

    #[test]
    fn uses_module_suffix_for_multi_module_plugin_ids() {
        assert_eq!(
            build_plugin_id("owner", "repo", "src/main/resources/plugin.yml", false),
            "owner/repo"
        );
        assert_eq!(
            build_plugin_id(
                "owner",
                "repo",
                "modules/economy/src/main/resources/plugin.yml",
                true,
            ),
            "owner/repo--modules-economy"
        );
    }
}
