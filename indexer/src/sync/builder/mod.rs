mod image;
mod link;

use crate::github::{Contributor, GitTreeEntry, Release, ReleaseAsset, Repository, client};
use crate::jenkins::{JenkinsBuildInfo, artifact_url};
use crate::plugin::{
    Author, Dependency, GalleryItem, License, Links, Plugin, Version, VersionFile,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use tracing::debug;

fn parse_timestamp(iso_string: &str) -> u64 {
    use chrono::{DateTime, Utc};
    DateTime::parse_from_rfc3339(iso_string)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).timestamp() as u64)
        .unwrap_or(0)
}

fn project_updated_timestamp(repo: &Repository, releases: &[Release]) -> u64 {
    let pushed_at = parse_timestamp(&repo.pushed_at);
    let latest_release_at = releases
        .iter()
        .map(|release| parse_timestamp(&release.published_at))
        .max()
        .unwrap_or(0);

    let activity_at = pushed_at.max(latest_release_at);
    if activity_at > 0 {
        activity_at
    } else {
        parse_timestamp(&repo.updated_at)
    }
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

const MANIFEST_FILENAMES: &[&str] = &["plugin.yml", "powernukkitx.yml", "nukkit.yml"];

#[derive(Debug, Clone, Copy)]
pub struct BuildOptions {
    pub allow_ai_categories: bool,
}

impl BuildOptions {
    pub fn without_ai_categories() -> Self {
        Self {
            allow_ai_categories: false,
        }
    }
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            allow_ai_categories: true,
        }
    }
}

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

type ImageProcessorFn =
    fn(&str, &PostProcessContext, &mut Vec<GalleryItem>, &HashSet<&str>) -> String;
type LinkProcessorFn = fn(&str, &PostProcessContext) -> String;

static IMAGE_PROCESSORS: &[ImageProcessorFn] =
    &[image::process_html_images, image::process_md_images];
static LINK_PROCESSORS: &[LinkProcessorFn] = &[link::process_md_links, link::process_html_links];

fn process_readme(
    readme: &str,
    ctx: &PostProcessContext,
    tree: &[GitTreeEntry],
) -> (String, Vec<GalleryItem>) {
    let mut content = readme.to_string();
    let mut gallery = Vec::new();
    let blobs: HashSet<&str> = tree
        .iter()
        .filter(|entry| entry.entry_type == "blob")
        .map(|entry| entry.path.as_str())
        .collect();

    for processor in IMAGE_PROCESSORS {
        content = processor(&content, ctx, &mut gallery, &blobs);
    }

    for processor in LINK_PROCESSORS {
        content = processor(&content, ctx);
    }

    (content, gallery)
}

/// 截断的 git tree 会漏掉模块清单与 README 资源,不能当作完整数据使用。
pub(crate) const TREE_TRUNCATED: &str = "tree truncated by GitHub";

fn get_tree(owner: &str, repo: &str, branch: &str) -> Result<Vec<GitTreeEntry>, String> {
    let tree = client()
        .get_tree(owner, repo, branch)
        .map_err(|e| format!("Failed to get tree: {}", e))?;
    if tree.truncated {
        return Err(TREE_TRUNCATED.to_string());
    }
    Ok(tree.tree)
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

fn manifest_filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 服务端插件加载器真正认识的描述符:NukkitX/Nukkit-MOT 读 nukkit.yml 再读
/// plugin.yml,PNX 读 powernukkitx.yml → nukkit.yml → plugin.yml。
fn is_nukkit_descriptor(path: &str) -> bool {
    manifest_filename(path) == "nukkit.yml"
}

pub(crate) fn is_plugin_manifest_path(path: &str) -> bool {
    path.contains("src/main/resources") && MANIFEST_FILENAMES.contains(&manifest_filename(path))
}

/// PNX `@PluginMeta` 注解型清单:源码中没有 yml,由 PNX-APT 在编译期
/// 生成 powernukkitx.yml。此类清单只能来自代码搜索命中或历史索引,
/// 不会由文件树扫描产生(树里无法确认注解内容)。
pub(crate) fn is_annotation_manifest_path(path: &str) -> bool {
    path.ends_with(".java")
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
    for marker in ["src/main/resources", "src/main/java"] {
        if let Some(pos) = path.find(marker) {
            return path[..pos].trim_end_matches('/').to_string();
        }
    }
    String::new()
}

fn group_manifest_paths(manifest_paths: &[String]) -> Vec<Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in manifest_paths
        .iter()
        .filter(|path| is_plugin_manifest_path(path) || is_annotation_manifest_path(path))
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
    let module_key = module_key_from_manifest_path(plugin_yml_path);

    let build_names = ["build.gradle.kts", "build.gradle", "pom.xml"];

    let mut expected: Vec<String> = Vec::new();

    // Module-level build files (for multi-module projects)
    if !module_key.is_empty() {
        for name in &build_names {
            expected.push(format!("{}/{}", module_key, name));
        }
    }

    // Root-level build files
    for name in &build_names {
        expected.push(name.to_string());
    }

    // Gradle version catalog
    expected.push("gradle/libs.versions.toml".to_string());

    tree.iter()
        .filter(|e| {
            e.entry_type == "blob" && expected.iter().any(|exp| e.path.eq_ignore_ascii_case(exp))
        })
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
            "powernukkitx" | "pnx" => {
                targets.insert("pnx");
                confidence.promote(DetectionConfidence::Medium);
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
    const NKX_INDICATORS: &[&str] = &["cloudburstmc", "repo.nukkitx.com"];
    const NKMOT_INDICATORS: &[&str] = &["memoriesoftime", "nukkit-mot"];
    // 2026-07 起 PNX 包名由 cn.powernukkitx 迁移到 org.powernukkitx,
    // 新官方 maven 仓库为 repo.powernukkitx.org。cn.powernukkitx 域名已被
    // Allay 重定向,不再作为官方信号列出;旧构建仍会命中泛 powernukkitx 子串
    const PNX_INDICATORS: &[&str] = &[
        "org.powernukkitx",
        "powernukkitx",
        "powernukkit/powernukkitx",
        "repo.powernukkitx.org",
    ];
    const LUMI_INDICATORS: &[&str] = &[
        "repo.lumi.su",
        "com.koshakmine:lumi",
        "<groupid>com.koshakmine</groupid>",
        "<artifactid>lumi</artifactid>",
    ];
    const LUMI_AUXILIARY_INDICATORS: &[&str] = &["repo.luminiadev.com"];

    let mut targets = BTreeSet::new();
    let has_lumi_auxiliary_hint = contains_any(content_lower, LUMI_AUXILIARY_INDICATORS);

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
        // `repo.luminiadev.com` (legacy Lumi domain) is a weak ecosystem hint. It only
        // expands the generic `cn.nukkit` fallback instead of deciding the runtime on its own.
        if has_lumi_auxiliary_hint {
            targets.insert("lumi");
        }
        targets.insert("nkx");
        targets.insert("nkmot");
        return (targets, DetectionConfidence::Medium);
    }

    (targets, DetectionConfidence::Low)
}

fn select_primary_manifest_path(manifest_paths: &[String]) -> Option<&str> {
    ordered_manifest_candidates(manifest_paths)
        .first()
        .map(|path| path.as_str())
}

/// 由清单本身推断目标运行时:
/// - powernukkitx.yml / @PluginMeta 注解是 PNX 专属信号
/// - nukkit.yml 是 Nukkit 系(NukkitX / Nukkit-MOT / PNX)加载器都支持的描述符
fn manifest_implied_targets(
    manifest_paths: &[String],
) -> (BTreeSet<&'static str>, DetectionConfidence) {
    let mut targets = BTreeSet::new();
    let mut confidence = DetectionConfidence::Low;

    if manifest_paths
        .iter()
        .any(|path| manifest_filename(path) == "powernukkitx.yml")
    {
        targets.insert("pnx");
        confidence.promote(DetectionConfidence::High);
    }
    if manifest_paths
        .iter()
        .any(|path| is_annotation_manifest_path(path))
    {
        targets.insert("pnx");
        confidence.promote(DetectionConfidence::High);
    }
    if manifest_paths.iter().any(|path| is_nukkit_descriptor(path)) {
        targets.insert("nkx");
        targets.insert("nkmot");
        targets.insert("pnx");
        confidence.promote(DetectionConfidence::Medium);
    }

    (targets, confidence)
}

fn detect_targets(
    tree: &[GitTreeEntry],
    repo: &Repository,
    manifest_paths: &[String],
) -> Result<Option<TargetDetection>, String> {
    let Some((owner, repo_name)) = repo.full_name.split_once('/') else {
        return Ok(None);
    };
    let Some(primary_manifest_path) = select_primary_manifest_path(manifest_paths) else {
        return Ok(None);
    };
    let mut targets = BTreeSet::new();
    let mut confidence = DetectionConfidence::Low;

    let (topic_targets, topic_confidence) = detect_targets_from_topics(&repo.topics);
    targets.extend(topic_targets);
    confidence.promote(topic_confidence);

    let (manifest_targets, manifest_confidence) = manifest_implied_targets(manifest_paths);
    targets.extend(manifest_targets);
    confidence.promote(manifest_confidence);

    let build_paths = find_build_file_paths(tree, primary_manifest_path);
    if build_paths.is_empty() && targets.is_empty() {
        debug!(repo = %repo.full_name, module = %module_key_from_manifest_path(primary_manifest_path), "No target evidence found");
        return Ok(None);
    }

    for path in &build_paths {
        let content = client()
            .get_file_content(owner, repo_name, path)
            .map_err(|e| format!("Failed to read build file {}: {}", path, e))?;
        let lower = content.to_lowercase();
        let (build_targets, build_confidence) = detect_targets_from_build_content(&lower);

        if !build_targets.is_empty() {
            targets.extend(build_targets);
            confidence.promote(build_confidence);
        }
    }

    if targets.is_empty() {
        debug!(repo = %repo.full_name, module = %module_key_from_manifest_path(primary_manifest_path), "No supported targets detected");
        return Ok(None);
    }

    Ok(Some(TargetDetection {
        targets: ordered_targets(&targets),
        confidence,
    }))
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

pub fn build_plugins_from_nukkit_with_tree(
    repo: &Repository,
    manifest_paths: &[String],
    prefetched_tree: Option<Vec<GitTreeEntry>>,
) -> Result<Vec<Plugin>, String> {
    build_plugins_from_nukkit_with_tree_options(
        repo,
        manifest_paths,
        prefetched_tree,
        BuildOptions::default(),
    )
}

pub fn build_plugins_from_nukkit_with_tree_options(
    repo: &Repository,
    manifest_paths: &[String],
    prefetched_tree: Option<Vec<GitTreeEntry>>,
    options: BuildOptions,
) -> Result<Vec<Plugin>, String> {
    let (owner, repo_name) = match repo.full_name.split_once('/') {
        Some((o, r)) => (o, r),
        None => {
            debug!(repo = %repo.full_name, "Skip: invalid repo name");
            return Ok(Vec::new());
        }
    };

    let default_branch = repo.default_branch.as_deref().unwrap_or("main");

    let manifest_groups = group_manifest_paths(manifest_paths);
    if manifest_groups.is_empty() {
        debug!(repo = %repo.full_name, "Skip: no plugin manifests found");
        return Ok(Vec::new());
    }

    let tree = match prefetched_tree {
        Some(tree) => tree,
        None => get_tree(owner, repo_name, default_branch)?,
    };

    let releases = client()
        .get_releases(owner, repo_name)
        .map_err(|e| format!("Failed to get releases for {}: {}", repo.full_name, e))?;
    let readme = client()
        .get_readme(owner, repo_name)
        .map_err(|e| format!("Failed to get readme for {}: {}", repo.full_name, e))?;
    let repo_categories = detect_categories(repo, &readme, options);

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
    let contributors = client()
        .get_contributors_by_url(&repo.contributors_url)
        .map_err(|e| format!("Failed to get contributors for {}: {}", repo.full_name, e))?;

    let is_multi_module = manifest_groups.len() > 1;
    let mut plugins = Vec::new();

    for manifest_group in manifest_groups {
        let Some(primary_manifest_path) = select_primary_manifest_path(&manifest_group) else {
            continue;
        };

        let Some(target_detection) = detect_targets(&tree, repo, &manifest_group)? else {
            debug!(repo = %repo.full_name, manifest = %primary_manifest_path, "Skip: no supported targets detected");
            continue;
        };

        let Some((manifest_path, nukkit_yml)) =
            parse_manifest_group(owner, repo_name, &repo.full_name, &manifest_group)?
        else {
            continue;
        };

        if let Some(plugin) = nukkit_yml_to_plugin(
            nukkit_yml,
            repo,
            &releases,
            &readme,
            &license,
            owner,
            repo_name,
            default_branch,
            &contributors,
            &icon_url,
            repo_gallery.clone(),
            &repo_categories,
            &manifest_path,
            &target_detection,
            is_multi_module,
            &tree,
        ) {
            plugins.push(plugin);
        }
    }

    Ok(plugins)
}

/// 按优先级排列模块内的清单候选:plugin.yml > powernukkitx.yml >
/// nukkit.yml > 其他 yml > @PluginMeta 注解源文件;同优先级按路径排序。
fn ordered_manifest_candidates(manifest_group: &[String]) -> Vec<&String> {
    let mut candidates: Vec<&String> = manifest_group.iter().collect();
    candidates.sort_by_key(|path| (manifest_priority(path), path.as_str()));
    candidates
}

fn manifest_priority(path: &str) -> u8 {
    match manifest_filename(path) {
        "plugin.yml" => 0,
        "powernukkitx.yml" => 1,
        "nukkit.yml" => 2,
        _ if is_annotation_manifest_path(path) => 3,
        _ => 4,
    }
}

/// 逐个尝试解析模块内清单,返回第一个可用的。
/// 代码搜索返回的 .java 命中可能包含未标注 @PluginMeta 的类,
/// 因此注解清单必须逐候选尝试而非只取第一个。
fn parse_manifest_group(
    owner: &str,
    repo_name: &str,
    repo_full_name: &str,
    manifest_group: &[String],
) -> Result<Option<(String, crate::nukkit::NukkitPluginYml)>, String> {
    let mut candidates = 0;
    let mut missing = 0;
    for path in ordered_manifest_candidates(manifest_group) {
        candidates += 1;
        let content = match client().get_file_content(owner, repo_name, path) {
            Ok(content) => content,
            // 代码搜索/旧缓存给出的候选可能已从仓库删除,换下一个候选即可
            Err(e) if e == "not found" => {
                debug!(repo = %repo_full_name, manifest = %path, "Manifest no longer exists");
                missing += 1;
                continue;
            }
            Err(e) => return Err(format!("Failed to read manifest {}: {}", path, e)),
        };

        if is_annotation_manifest_path(path) {
            match crate::nukkit::parse_plugin_meta(&content, path) {
                Some(yml) => return Ok(Some((path.clone(), yml))),
                None => {
                    debug!(repo = %repo_full_name, manifest = %path, "No usable @PluginMeta annotation");
                }
            }
        } else {
            match crate::nukkit::NukkitPluginYml::from_str(&content) {
                Ok(yml) => return Ok(Some((path.clone(), yml))),
                Err(e) => {
                    debug!(repo = %repo_full_name, manifest = %path, error = %e, "Failed to parse manifest");
                }
            }
        }
    }

    // 树里还列着清单、内容却全部 404:更可能是 tree/contents 之间的竞态,
    // 按瞬态错误处理,让 update 保留旧记录、下一轮再判定删除。
    if candidates > 0 && missing == candidates {
        return Err(format!(
            "Manifest contents not found for {}",
            repo_full_name
        ));
    }

    Ok(None)
}

fn is_placeholder(s: &str) -> bool {
    // Detect unresolved placeholders like ${project.name} or @project.name@
    let trimmed = s.trim();
    (trimmed.starts_with("${") && trimmed.ends_with('}') && trimmed.len() > 3)
        || (trimmed.starts_with('@') && trimmed.ends_with('@') && trimmed.len() > 2)
}

fn resolve_authors(repo: &Repository, contributors: &[Contributor]) -> Vec<Author> {
    if !contributors.is_empty() {
        return contributors
            .iter()
            .map(|contributor| Author {
                name: contributor.login.clone(),
                url: contributor.html_url.clone(),
                avatar_url: contributor.avatar_url.clone(),
            })
            .collect();
    }

    vec![Author {
        name: repo.owner.login.clone(),
        url: repo.owner.html_url.clone(),
        avatar_url: repo.owner.avatar_url.clone(),
    }]
}

/// 判断 jar 文件名主干是否以 `name` 开头且落在词边界上
/// (避免 "Coin" 误匹配 "CoinSystem")。
fn jar_name_matches(stem: &str, name: &str) -> bool {
    let stem = stem.to_lowercase();
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return false;
    }
    match stem.strip_prefix(&name) {
        Some(rest) => rest.is_empty() || rest.starts_with(['-', '_', '.', ' ']),
        None => false,
    }
}

/// 提取 GitHub digest 字段中的 sha256 hex("sha256:<64 位十六进制>")。
/// 未知算法、缺失或格式异常返回 None,不影响索引其余字段。
fn sha256_from_digest(digest: Option<&str>) -> Option<String> {
    let hex = digest?.strip_prefix("sha256:")?.to_lowercase();
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(hex)
    } else {
        None
    }
}

/// 从 jar 文件名主干提取展示名:匹配到的名称前缀之后的版本部分,
/// 如 "Radio-1.2.0-SNAPSHOT" + "Radio" -> "1.2.0-SNAPSHOT";提取不出则回退主干。
fn jar_display_name(stem: &str, names: &[&str]) -> String {
    for name in names {
        let lower_stem = stem.to_lowercase();
        let lower_name = name.trim().to_lowercase();
        if let Some(rest) = lower_stem.strip_prefix(&lower_name)
            && (rest.is_empty() || rest.starts_with(['-', '_', '.', ' ']))
        {
            let cut = stem.len() - rest.len();
            let display = stem[cut..].trim_start_matches(['-', '_', '.', ' ']);
            if !display.is_empty() {
                return display.to_string();
            }
        }
    }
    stem.to_string()
}

/// 可安装的 jar:Maven 的 -sources / -javadoc 附件不是插件,面板装上即坏。
pub(crate) fn is_installable_jar(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.ends_with(".jar")
        && !lower
            .strip_suffix(".jar")
            .is_some_and(|stem| stem.ends_with("-sources") || stem.ends_with("-javadoc"))
}

fn jar_stem(filename: &str) -> &str {
    let cut = filename.len().saturating_sub(4);
    match (filename.get(..cut), filename.get(cut..)) {
        (Some(stem), Some(ext)) if ext.eq_ignore_ascii_case(".jar") => stem,
        _ => filename,
    }
}

/// 分隔符分词,用于模块/目标名匹配:保留完整单词,
/// 避免 "Coin" 误匹配另一个模块的 "CoinSystem"。
fn jar_tokens(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

/// 平台词检测用分词,额外按 CamelCase 切开:
/// "ShitBotSpigot" -> shit/bot/spigot,"PlanFabric" -> plan/fabric。
/// 只用于判断“是不是别的平台”,不用于模块归属。
fn jar_camel_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for part in value.split(|c: char| !c.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        tokens.push(part.to_ascii_lowercase());
        let chars: Vec<char> = part.chars().collect();
        let mut start = 0;
        for index in 1..chars.len() {
            let prev = chars[index - 1];
            if chars[index].is_uppercase() && (prev.is_lowercase() || prev.is_ascii_digit()) {
                tokens.push(
                    chars[start..index]
                        .iter()
                        .collect::<String>()
                        .to_ascii_lowercase(),
                );
                start = index;
            }
        }
        if start > 0 {
            tokens.push(
                chars[start..]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
        }
    }
    tokens
}

/// 词边界匹配:别名按分隔符/CamelCase 拆成多个 token 时要求全部命中
/// ("nukkit-mot" 能匹配 "NeverMiss-Nukkit-MOT-1.0",不会被拆开的单词误伤)。
fn jar_matches_tokens(stem: &str, aliases: &[String]) -> bool {
    let stem_tokens = jar_tokens(stem);
    aliases.iter().any(|alias| {
        let expected = jar_tokens(alias);
        !expected.is_empty() && expected.iter().all(|part| stem_tokens.contains(part))
    })
}

fn module_jar_tokens(manifest_path: &str) -> Vec<String> {
    module_key_from_manifest_path(manifest_path)
        .split('/')
        .filter(|segment| segment.len() >= 4)
        .map(|segment| segment.to_lowercase())
        .collect()
}

fn target_jar_tokens(targets: &[String]) -> Vec<String> {
    let mut tokens = Vec::new();
    for target in targets {
        let aliases: &[&str] = match target.as_str() {
            "nkx" => &["nkx", "nukkitx", "nukkit", "nk"],
            "nkmot" => &["nukkit-mot", "nkmot", "nk-mot"],
            "pnx" => &["powernukkitx", "powernukkit", "pnx"],
            "lumi" => &["lumi"],
            _ => &[],
        };
        for alias in aliases {
            tokens.push((*alias).to_string());
        }
    }
    tokens
}

/// 明显属于其他平台(Spigot/Fabric/... )的产物,不应挂到 Nukkit 系条目上。
const FOREIGN_PLATFORM_TOKENS: &[&str] = &[
    "bukkit",
    "spigot",
    "paper",
    "folia",
    "fabric",
    "forge",
    "neoforge",
    "velocity",
    "bungee",
    "bungeecord",
    "sponge",
    "hytale",
    "minestom",
];

fn is_foreign_platform_jar(stem: &str, own_names: &[String]) -> bool {
    let tokens = jar_camel_tokens(stem);
    FOREIGN_PLATFORM_TOKENS.iter().any(|platform| {
        // 插件/仓库/模块名里本身带平台词时不能据此判定归属
        // (例如名为 PaperChat 的 Nukkit 插件)
        tokens.iter().any(|token| token == platform)
            && !own_names.iter().any(|name| name == platform)
    })
}

/// 明确的运行时依赖/引擎 jar,不是插件本体。
/// 仅在某 release 已有其它平台产物被剔除、且只剩这类文件时,才判定该
/// release 对本模块没有可安装产物;仅仅文件名不同(如 BlockHunt)不算。
const AUXILIARY_JAR_TOKENS: &[&str] = &["engine", "lib", "libs", "library", "dependencies", "deps"];

fn is_auxiliary_jar(stem: &str) -> bool {
    let tokens = jar_tokens(stem);
    AUXILIARY_JAR_TOKENS
        .iter()
        .any(|token| tokens.iter().any(|part| part == token))
}

/// primary 的分层:本模块标识 > primary_target > 精确的插件/仓库名 >
/// 带前缀的插件/仓库名 > 其它目标。同分时保留 release 资产顺序
/// (与旧版“第一个文件”行为一致),不按体积猜。
fn jar_primary_score(
    filename: &str,
    plugin_name: &str,
    repo_name: &str,
    manifest_path: &str,
    targets: &[String],
) -> u8 {
    let stem = jar_stem(filename);
    if jar_matches_tokens(stem, &module_jar_tokens(manifest_path)) {
        return 6;
    }
    if targets.first().is_some_and(|primary| {
        jar_matches_tokens(stem, &target_jar_tokens(std::slice::from_ref(primary)))
    }) {
        return 5;
    }
    if stem.eq_ignore_ascii_case(plugin_name.trim()) || stem.eq_ignore_ascii_case(repo_name.trim())
    {
        return 4;
    }
    if jar_name_matches(stem, plugin_name) || jar_name_matches(stem, repo_name) {
        return 3;
    }
    if !targets.is_empty() && jar_matches_tokens(stem, &target_jar_tokens(&targets[1..])) {
        return 2;
    }
    1
}

/// 将 motci.cn 的最近成功构建合成为一条快照版本:
/// - version 形如 `ci-{build}`,prerelease = true;
/// - 时间戳为 Jenkins 毫秒,转换为秒;
/// - 多模块仓库只认领文件名带本模块标识的 jar,根模块回退到插件/仓库名;
/// - published_at 不参与 project_updated_timestamp,避免 CI 构建刷"最近更新"。
fn compose_motci_snapshot(
    build: &JenkinsBuildInfo,
    plugin_name: &str,
    repo_name: &str,
    manifest_path: &str,
    targets: &[String],
    is_multi_module: bool,
) -> Option<Version> {
    let jars: Vec<&(String, String)> = build
        .artifacts
        .iter()
        .filter(|(filename, _)| is_installable_jar(filename))
        .collect();

    if jars.is_empty() {
        return None;
    }

    let matched: Vec<&(String, String)> = if is_multi_module {
        let mut module_tokens = module_jar_tokens(manifest_path);
        module_tokens.extend(target_jar_tokens(targets));
        let by_name = |(filename, _): &&(String, String)| {
            let stem = jar_stem(filename);
            jar_name_matches(stem, plugin_name) || jar_name_matches(stem, repo_name)
        };
        let matched: Vec<_> = if module_tokens.is_empty() {
            // 根模块没有目录标识,只能按插件/仓库名认领
            jars.iter().copied().filter(by_name).collect()
        } else {
            let by_module: Vec<_> = jars
                .iter()
                .copied()
                .filter(|(filename, _)| jar_matches_tokens(jar_stem(filename), &module_tokens))
                .collect();
            if by_module.is_empty() {
                // 产物名不带模块目录标识时回退到插件/仓库名(旧行为)
                jars.iter().copied().filter(by_name).collect()
            } else {
                by_module
            }
        };
        if matched.is_empty() {
            debug!(
                build = build.build_number,
                "No motci artifact matches module, skipping snapshot"
            );
            return None;
        }
        matched
    } else {
        jars
    };

    let names = [plugin_name, repo_name];
    let display_name = matched
        .first()
        .map(|(filename, _)| jar_display_name(jar_stem(filename), &names))
        .unwrap_or_else(|| format!("ci-{}", build.build_number));

    Some(Version {
        version: format!("ci-{}", build.build_number),
        name: display_name,
        prerelease: true,
        changelog: String::new(),
        files: matched
            .iter()
            .enumerate()
            .map(|(index, (filename, relative_path))| VersionFile {
                filename: filename.clone(),
                url: artifact_url(&build.job_url, build.build_number, relative_path),
                size: 0,
                primary: index == 0,
                sha256: None,
            })
            .collect(),
        downloads: 0,
        published_at: build.timestamp / 1000,
    })
}

/// 按发布时间倒序。GitHub releases 列表按创建顺序返回,不等于发布时间顺序。
fn sort_versions_newest_first(versions: &mut [Version]) {
    versions.sort_by(|a, b| b.published_at.cmp(&a.published_at));
}

/// release 中属于本模块、可安装的 .jar 资产。
/// 排除 -sources/-javadoc 附件与明显属于其他平台的产物(插件/仓库/模块名里
/// 自带平台词时豁免);模块标识与运行目标用于挑选唯一的 primary,并放到首位。
/// 筛选后为空说明该 release 对本模块没有可安装产物,整个版本不产出。
fn release_version_files(
    release: &Release,
    plugin_name: &str,
    repo_name: &str,
    manifest_path: &str,
    targets: &[String],
) -> Vec<VersionFile> {
    let installable: Vec<&ReleaseAsset> = release
        .assets
        .iter()
        .filter(|asset| is_installable_jar(&asset.name))
        .collect();

    if installable.is_empty() {
        return Vec::new();
    }

    let own_names: Vec<String> = jar_camel_tokens(plugin_name)
        .into_iter()
        .chain(jar_camel_tokens(repo_name))
        .chain(jar_camel_tokens(&module_key_from_manifest_path(
            manifest_path,
        )))
        .collect();

    let selected: Vec<&ReleaseAsset> = installable
        .iter()
        .copied()
        .filter(|asset| !is_foreign_platform_jar(jar_stem(&asset.name), &own_names))
        .collect();

    if selected.is_empty() {
        return Vec::new();
    }

    // 多模块仓库:有本模块标识的产物时只保留这些,避免把其他模块的 jar
    // 挂到本模块;产物名不含模块标识时保持现有集合(名称回退)。
    let module_tokens = module_jar_tokens(manifest_path);
    let module_specific: Vec<&ReleaseAsset> = if module_tokens.is_empty() {
        Vec::new()
    } else {
        selected
            .iter()
            .copied()
            .filter(|asset| jar_matches_tokens(jar_stem(&asset.name), &module_tokens))
            .collect()
    };
    let selected: Vec<&ReleaseAsset> = if module_specific.is_empty() {
        selected
    } else {
        module_specific
    };

    // 同分时保留 release 资产顺序,不按体积猜平台。
    let mut primary = 0;
    let mut primary_score = 0;
    for (index, asset) in selected.iter().enumerate() {
        let score = jar_primary_score(&asset.name, plugin_name, repo_name, manifest_path, targets);
        if score > primary_score {
            primary = index;
            primary_score = score;
        }
    }

    // 有其它平台产物被剔除、剩下的又全是引擎/依赖 jar,且没有任何文件能与
    // 插件/模块/目标关联:该 release 没有本模块的插件本体,不产出版本。
    // primary_score > 1 说明至少有一个文件是插件本体的候选,不能丢。
    if selected.len() < installable.len()
        && primary_score == 1
        && selected
            .iter()
            .all(|asset| is_auxiliary_jar(jar_stem(&asset.name)))
    {
        return Vec::new();
    }

    // primary 放到第一位:前端多处用 files[0] 兜底,避免出现两个“主文件”。
    let mut ordered: Vec<&ReleaseAsset> = Vec::with_capacity(selected.len());
    ordered.push(selected[primary]);
    for (index, asset) in selected.iter().enumerate() {
        if index != primary {
            ordered.push(asset);
        }
    }

    ordered
        .iter()
        .enumerate()
        .map(|(index, asset)| VersionFile {
            filename: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            size: asset.size,
            primary: index == 0,
            sha256: sha256_from_digest(asset.digest.as_deref()),
        })
        .collect()
}

/// update 的版本消失复查与 builder 共用同一判定,避免两套口径分叉。
pub(crate) fn release_has_module_files(
    release: &Release,
    plugin_name: &str,
    repo_name: &str,
    manifest_path: &str,
    targets: &[String],
) -> bool {
    !release_version_files(release, plugin_name, repo_name, manifest_path, targets).is_empty()
}

fn nukkit_yml_to_plugin(
    yml: crate::nukkit::NukkitPluginYml,
    repo: &Repository,
    releases: &[Release],
    readme: &str,
    license: &License,
    owner: &str,
    repo_name: &str,
    branch: &str,
    contributors: &[Contributor],
    icon_url: &str,
    repo_gallery: Vec<GalleryItem>,
    categories: &[String],
    manifest_path: &str,
    target_detection: &TargetDetection,
    is_multi_module: bool,
    tree: &[GitTreeEntry],
) -> Option<Plugin> {
    let ctx = PostProcessContext {
        owner,
        repo: repo_name,
        branch,
    };

    let (processed_readme, mut gallery) = process_readme(readme, &ctx, tree);
    gallery.extend(repo_gallery);

    let authors = resolve_authors(repo, contributors);

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

    // Build versions from releases;草稿 release 没有发布时间,也不可安装。
    // GitHub 返回顺序不是发布时间顺序,这里统一按 published_at 倒序。
    let mut versions: Vec<Version> = releases
        .iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let files = release_version_files(
                release,
                &plugin_name,
                repo_name,
                manifest_path,
                &target_detection.targets,
            );

            if files.is_empty() {
                debug!(
                    repo = %repo.full_name,
                    release = %release.tag_name,
                    "Skipping release without installable .jar assets"
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

    // motci CI 快照:GitHub 数据为准,motci 命中的仓库追加一条快照下载版本
    if let Some(build) = crate::jenkins::jenkins_index().get(&repo.full_name)
        && let Some(snapshot) = compose_motci_snapshot(
            build,
            &plugin_name,
            repo_name,
            manifest_path,
            &target_detection.targets,
            is_multi_module,
        )
        && versions.iter().all(|v| v.version != snapshot.version)
    {
        debug!(
            repo = %repo.full_name,
            version = %snapshot.version,
            "Attached motci snapshot version"
        );
        versions.push(snapshot);
    }
    // 快照也参与排序,最终 versions 始终按发布时间倒序。
    sort_versions_newest_first(&mut versions);

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
        categories: categories.to_vec(),
        license: license.clone(),
        links: yml.website.as_ref().map(|w| Links {
            homepage: w.clone(),
            wiki: String::new(),
            discord: String::new(),
        }),
        downloads: 0,
        stars: repo.stargazers_count,
        created_at: parse_timestamp(&repo.created_at),
        updated_at: project_updated_timestamp(repo, releases),
        icon_url: icon_url.to_string(),
        gallery,
        versions,
        api_version,
        server_version: String::new(),
        dependencies: all_dependencies,
        pending: false,
        preserved_fields: Default::default(),
    })
}

fn detect_categories(repo: &Repository, readme: &str, options: BuildOptions) -> Vec<String> {
    detect_categories_with_classifier(repo, readme, options, crate::ai::classify_readme_categories)
}

fn detect_categories_with_classifier<F>(
    repo: &Repository,
    readme: &str,
    options: BuildOptions,
    classify_readme_categories: F,
) -> Vec<String>
where
    F: FnOnce(&str, &[&str]) -> Vec<String>,
{
    let topic_categories = categories_from_topics(&repo.topics);

    if options.allow_ai_categories && crate::ai::category_classification_enabled() {
        let ai_categories = classify_readme_categories(readme, CATEGORIES);
        return combine_categories(topic_categories, ai_categories);
    }

    topic_categories
}

fn categories_from_topics(topics: &[String]) -> Vec<String> {
    topics
        .iter()
        .filter_map(|topic| {
            let normalized = topic.strip_prefix("nukkit-").unwrap_or(topic);
            if CATEGORIES.contains(&normalized) {
                Some(normalized.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn combine_categories(topic_categories: Vec<String>, ai_categories: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    topic_categories
        .into_iter()
        .chain(ai_categories)
        .filter(|category| seen.insert(category.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        BuildOptions, build_plugin_id, categories_from_topics, combine_categories,
        compose_motci_snapshot, detect_categories_with_classifier,
        detect_targets_from_build_content, detect_targets_from_topics, find_build_file_paths,
        group_manifest_paths, is_annotation_manifest_path, is_plugin_manifest_path,
        jar_display_name, jar_name_matches, manifest_implied_targets,
        module_key_from_manifest_path, ordered_manifest_candidates, parse_timestamp,
        project_updated_timestamp, release_version_files, resolve_authors,
        select_primary_manifest_path, sha256_from_digest, sort_versions_newest_first,
    };
    use crate::github::{Contributor, GitTreeEntry, Owner, Release, ReleaseAsset, Repository};
    use crate::jenkins::JenkinsBuildInfo;
    use crate::plugin::Version;

    fn motci_build(job_url: &str, artifacts: &[(&str, &str)]) -> JenkinsBuildInfo {
        JenkinsBuildInfo {
            job_url: job_url.to_string(),
            build_number: 60,
            timestamp: 1_777_593_600_000,
            artifacts: artifacts
                .iter()
                .map(|(f, p)| ((*f).to_string(), (*p).to_string()))
                .collect(),
        }
    }

    fn repo_with_dates(updated_at: &str, pushed_at: &str) -> Repository {
        Repository {
            id: 1,
            full_name: "owner/repo".to_string(),
            name: "repo".to_string(),
            description: None,
            html_url: "https://github.com/owner/repo".to_string(),
            stargazers_count: 0,
            forks_count: 0,
            created_at: "2019-01-01T00:00:00Z".to_string(),
            updated_at: updated_at.to_string(),
            pushed_at: pushed_at.to_string(),
            owner: Owner {
                login: "owner".to_string(),
                avatar_url: String::new(),
                html_url: String::new(),
            },
            license: None,
            topics: Vec::new(),
            is_template: false,
            fork: false,
            archived: false,
            default_branch: Some("main".to_string()),
            contributors_url: String::new(),
            parent: None,
            private: false,
        }
    }

    fn release_published_at(published_at: &str) -> Release {
        Release {
            id: 1,
            tag_name: "v1.0.0".to_string(),
            name: None,
            body: None,
            prerelease: false,
            draft: false,
            created_at: published_at.to_string(),
            published_at: published_at.to_string(),
            assets: Vec::new(),
        }
    }

    #[test]
    fn sha256_from_digest_accepts_known_shape_only() {
        let hex = "a".repeat(64);
        assert_eq!(
            sha256_from_digest(Some(&format!("sha256:{hex}"))),
            Some(hex.clone())
        );
        // 大写 hex 归一为小写
        let upper: String = hex.to_uppercase();
        assert_eq!(
            sha256_from_digest(Some(&format!("sha256:{upper}"))),
            Some(hex.clone())
        );
        assert_eq!(
            sha256_from_digest(Some("sha1:da39a3ee5e6b4b0d3255bfef95601890afd80709")),
            None
        );
        assert_eq!(sha256_from_digest(Some("sha256:deadbeef")), None);
        assert_eq!(sha256_from_digest(None), None);
    }

    #[test]
    fn motci_snapshot_for_single_module_repo() {
        let build = motci_build(
            "https://motci.cn/job/Radio/",
            &[(
                "Radio-1.2.0-SNAPSHOT.jar",
                "target/Radio-1.2.0-SNAPSHOT.jar",
            )],
        );

        let snapshot = compose_motci_snapshot(
            &build,
            "Radio",
            "Radio",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
            false,
        )
        .expect("snapshot");

        assert_eq!(snapshot.version, "ci-60");
        assert_eq!(snapshot.name, "1.2.0-SNAPSHOT");
        assert!(snapshot.prerelease);
        assert_eq!(snapshot.published_at, 1_777_593_600);
        assert_eq!(snapshot.downloads, 0);
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].filename, "Radio-1.2.0-SNAPSHOT.jar");
        assert_eq!(
            snapshot.files[0].url,
            "https://motci.cn/job/Radio/60/artifact/target/Radio-1.2.0-SNAPSHOT.jar"
        );
        assert!(snapshot.files[0].primary);
    }

    #[test]
    fn motci_snapshot_skips_non_jar_artifacts() {
        let build = motci_build(
            "https://motci.cn/job/Foo",
            &[
                ("Foo-1.0.jar", "target/Foo-1.0.jar"),
                ("build.log", "build.log"),
                ("sources.zip", "target/sources.zip"),
            ],
        );

        let snapshot = compose_motci_snapshot(
            &build,
            "Foo",
            "Foo",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
            false,
        )
        .expect("snapshot");
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].filename, "Foo-1.0.jar");
    }

    // Maven -sources / -javadoc 附件以 .jar 结尾但不可安装,两条路径都要排除
    #[test]
    fn motci_snapshot_excludes_sources_and_javadoc_jars() {
        let single = motci_build(
            "https://motci.cn/job/Foo",
            &[
                ("Foo-1.0.jar", "target/Foo-1.0.jar"),
                ("Foo-1.0-sources.jar", "target/Foo-1.0-sources.jar"),
                ("Foo-1.0-javadoc.jar", "target/Foo-1.0-javadoc.jar"),
            ],
        );
        let snapshot = compose_motci_snapshot(
            &single,
            "Foo",
            "Foo",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
            false,
        )
        .expect("snapshot");
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].filename, "Foo-1.0.jar");

        // 多模块路径:模块标识命中 "Coin-1.0-sources" 时仍必须排除附件
        let multi = motci_build(
            "https://motci.cn/job/Multi",
            &[
                ("Coin-1.0.jar", "Coin/target/Coin-1.0.jar"),
                ("Coin-1.0-sources.jar", "Coin/target/Coin-1.0-sources.jar"),
            ],
        );
        let snapshot = compose_motci_snapshot(
            &multi,
            "Coin",
            "MultiRepo",
            "coin/src/main/resources/plugin.yml",
            &["nkx".to_string()],
            true,
        )
        .expect("snapshot");
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].filename, "Coin-1.0.jar");
    }

    #[test]
    fn motci_snapshot_multi_module_claims_matching_jar_only() {
        let build = motci_build(
            "https://motci.cn/job/Multi",
            &[
                (
                    "DCurrency-1.0.0.jar",
                    "DCurrency/target/DCurrency-1.0.0.jar",
                ),
                ("GameAPI-2.0.jar", "GameAPI/target/GameAPI-2.0.jar"),
            ],
        );

        let snapshot = compose_motci_snapshot(
            &build,
            "DCurrency",
            "MultiRepo",
            "dcurrency/src/main/resources/plugin.yml",
            &["nkx".to_string()],
            true,
        )
        .expect("snapshot");
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].filename, "DCurrency-1.0.0.jar");
        assert_eq!(snapshot.name, "1.0.0");
        assert_eq!(
            snapshot.files[0].url,
            "https://motci.cn/job/Multi/60/artifact/DCurrency/target/DCurrency-1.0.0.jar"
        );
    }

    #[test]
    fn motci_snapshot_multi_module_skips_when_no_artifact_matches() {
        let build = motci_build(
            "https://motci.cn/job/Multi",
            &[("CoinSystem-1.0.jar", "CoinSystem/target/CoinSystem-1.0.jar")],
        );

        assert!(
            compose_motci_snapshot(
                &build,
                "Coin",
                "MultiRepo",
                "coin/src/main/resources/plugin.yml",
                &["nkx".to_string()],
                true,
            )
            .is_none()
        );
    }

    #[test]
    fn motci_snapshot_root_module_falls_back_to_repo_name() {
        let build = motci_build(
            "https://motci.cn/job/Multi",
            &[("MultiRepo-1.0.jar", "MultiRepo/target/MultiRepo-1.0.jar")],
        );

        let snapshot = compose_motci_snapshot(
            &build,
            "MultiRepo",
            "MultiRepo",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
            true,
        )
        .expect("snapshot");

        assert_eq!(snapshot.files.len(), 1);
        assert!(snapshot.files[0].primary);
    }

    #[test]
    fn motci_snapshot_falls_back_to_plugin_name_when_module_is_not_in_artifact() {
        let build = motci_build(
            "https://motci.cn/job/Multi",
            &[("ShitBotNukkit-1.2.5.jar", "target/ShitBotNukkit-1.2.5.jar")],
        );

        let snapshot = compose_motci_snapshot(
            &build,
            "ShitBotNukkit",
            "ShitBot",
            "modules/platforms/nukkit/src/main/resources/plugin.yml",
            &["nkmot".to_string()],
            true,
        )
        .expect("snapshot");

        assert_eq!(snapshot.files[0].filename, "ShitBotNukkit-1.2.5.jar");
    }

    #[test]
    fn motci_snapshot_requires_jar_artifacts() {
        let build = motci_build("https://motci.cn/job/Foo", &[("notes.txt", "notes.txt")]);
        assert!(
            compose_motci_snapshot(
                &build,
                "Foo",
                "Foo",
                "src/main/resources/plugin.yml",
                &["nkx".to_string()],
                false,
            )
            .is_none()
        );
    }

    #[test]
    fn jar_name_matching_respects_word_boundaries() {
        assert!(jar_name_matches("Radio-1.2.0-SNAPSHOT", "Radio"));
        assert!(jar_name_matches("radio_1.0.jar", "Radio"));
        assert!(jar_name_matches("EconomyAPI-2.1.1", "EconomyAPI"));
        assert!(jar_name_matches("Radio", "Radio"));
        assert!(!jar_name_matches("CoinSystem-1.0", "Coin"));
        assert!(!jar_name_matches("MyRadio-1.0", "Radio"));
        assert!(!jar_name_matches("Radio-1.0", ""));
    }

    #[test]
    fn display_name_extracts_version_after_matched_prefix() {
        assert_eq!(
            jar_display_name("Radio-1.2.0-SNAPSHOT", &["Radio"]),
            "1.2.0-SNAPSHOT"
        );
        assert_eq!(
            jar_display_name("EconomyAPI_2.1.1", &["EconomyAPI"]),
            "2.1.1"
        );
        // 前缀不落在边界上时不裁剪,回退整个主干
        assert_eq!(
            jar_display_name("CoinSystem-1.0", &["Coin"]),
            "CoinSystem-1.0"
        );
        // 无任何名称命中时回退整个主干
        assert_eq!(jar_display_name("mystery-1.0", &["Other"]), "mystery-1.0");
    }

    #[test]
    fn authors_use_matching_contributor_avatars() {
        let repo = repo_with_dates("2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z");
        let contributors = vec![Contributor {
            login: "Alice".to_string(),
            avatar_url: "https://avatars.githubusercontent.com/u/1?v=4".to_string(),
            html_url: "https://github.com/Alice".to_string(),
            contributions: 10,
        }];

        let authors = resolve_authors(&repo, &contributors);

        assert_eq!(authors[0].name, "Alice");
        assert_eq!(authors[0].url, "https://github.com/Alice");
        assert_eq!(
            authors[0].avatar_url,
            "https://avatars.githubusercontent.com/u/1?v=4"
        );
    }

    #[test]
    fn authors_fall_back_to_repository_owner_when_contributors_are_empty() {
        let mut repo = repo_with_dates("2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z");
        repo.owner.login = "owner".to_string();
        repo.owner.avatar_url = "https://avatars.githubusercontent.com/u/2?v=4".to_string();
        repo.owner.html_url = "https://github.com/owner".to_string();

        let authors = resolve_authors(&repo, &[]);

        assert_eq!(authors[0].name, "owner");
        assert_eq!(authors[0].url, "https://github.com/owner");
        assert_eq!(
            authors[0].avatar_url,
            "https://avatars.githubusercontent.com/u/2?v=4"
        );
    }

    #[test]
    fn manifest_authors_are_ignored_when_github_contributors_exist() {
        let repo = repo_with_dates("2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z");
        let contributors = vec![
            Contributor {
                login: "Alice".to_string(),
                avatar_url: "https://avatars.githubusercontent.com/u/1?v=4".to_string(),
                html_url: "https://github.com/Alice".to_string(),
                contributions: 10,
            },
            Contributor {
                login: "Bob".to_string(),
                avatar_url: "https://avatars.githubusercontent.com/u/2?v=4".to_string(),
                html_url: "https://github.com/Bob".to_string(),
                contributions: 5,
            },
        ];

        let authors = resolve_authors(&repo, &contributors);

        assert_eq!(authors[0].name, "Alice");
        assert_eq!(authors[1].name, "Bob");
        assert_eq!(authors.len(), 2);
    }

    #[test]
    fn detects_supported_nukkit_markers() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://repo.nukkitx.com/maven-releases/") }
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
    fn does_not_treat_opencollab_repository_as_runtime_target_signal() {
        let pom = r#"
            <repositories>
                <repository>
                    <id>opencollab-snapshot</id>
                    <url>https://repo.opencollab.dev/main/</url>
                </repository>
            </repositories>
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&pom.to_lowercase());
        assert!(targets.is_empty());
        assert_eq!(confidence.as_str(), "low");
    }

    #[test]
    fn detects_powernukkitx_targets() {
        let gradle = r#"
            dependencies {
                compileOnly("org.powernukkitx:powernukkitx:1.20.0-r1")
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["pnx"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn detects_legacy_cn_powernukkitx_builds_via_generic_indicator() {
        // cn.powernukkitx 已不是官方信号,但迁移前的旧构建仍应通过
        // 泛 powernukkitx 子串命中,避免存量插件漏检
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
                maven { url = uri("https://repo.lumi.su/snapshots") }
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
    fn detects_lumi_from_repo_lumi_su_repository_alone() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://repo.lumi.su/releases") }
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["lumi"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn does_not_treat_legacy_repo_luminiadev_com_as_decisive_lumi_signal() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://repo.luminiadev.com/releases") }
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert!(targets.is_empty());
        assert_eq!(confidence.as_str(), "low");
    }

    #[test]
    fn legacy_repo_luminiadev_com_extends_cn_nukkit_shared_targets_with_lumi() {
        let gradle = r#"
            repositories {
                maven { url = uri("https://repo.luminiadev.com/releases") }
            }

            dependencies {
                compileOnly("cn.nukkit:nukkit:1.0-SNAPSHOT")
            }
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&gradle.to_lowercase());
        assert_eq!(
            targets.into_iter().collect::<Vec<_>>(),
            vec!["lumi", "nkmot", "nkx"]
        );
        assert_eq!(confidence.as_str(), "medium");
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
        assert!(is_plugin_manifest_path("src/main/resources/nukkit.yml"));
        assert!(!is_plugin_manifest_path(
            "src/main/resources/not-plugin.yaml"
        ));
    }

    #[test]
    fn detects_annotation_manifest_paths() {
        assert!(is_annotation_manifest_path(
            "src/main/java/io/github/foo/Bar.java"
        ));
        assert!(!is_annotation_manifest_path(
            "src/main/resources/plugin.yml"
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
    fn groups_annotation_manifest_into_same_module_as_yml() {
        assert_eq!(
            module_key_from_manifest_path("modules/foo/src/main/java/com/foo/Bar.java"),
            "modules/foo"
        );
        assert_eq!(
            module_key_from_manifest_path("modules/foo/src/main/resources/plugin.yml"),
            "modules/foo"
        );

        let groups = group_manifest_paths(&[
            "modules/foo/src/main/resources/plugin.yml".to_string(),
            "modules/foo/src/main/java/com/foo/Bar.java".to_string(),
            "modules/baz/src/main/java/com/baz/Baz.java".to_string(),
        ]);

        assert_eq!(groups.len(), 2);
        // BTreeMap 按 key 排序:modules/baz 在前,modules/foo 在后
        assert_eq!(groups[0].len(), 1);
        assert_eq!(groups[1].len(), 2);
    }

    #[test]
    fn prefers_yml_manifests_over_annotation_manifest() {
        let paths = vec![
            "src/main/java/com/foo/Foo.java".to_string(),
            "src/main/resources/plugin.yml".to_string(),
        ];
        assert_eq!(
            select_primary_manifest_path(&paths),
            Some("src/main/resources/plugin.yml")
        );

        let annotation_only = vec!["src/main/java/com/foo/Foo.java".to_string()];
        assert_eq!(
            select_primary_manifest_path(&annotation_only),
            Some("src/main/java/com/foo/Foo.java")
        );
    }

    #[test]
    fn orders_manifest_candidates_by_priority() {
        let group = vec![
            "src/main/java/com/foo/ZListener.java".to_string(),
            "src/main/resources/nukkit.yml".to_string(),
            "src/main/java/com/foo/AMain.java".to_string(),
            "src/main/resources/plugin.yml".to_string(),
        ];
        let candidates = ordered_manifest_candidates(&group);

        let paths: Vec<&str> = candidates.iter().map(|p| p.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "src/main/resources/plugin.yml",
                "src/main/resources/nukkit.yml",
                // 同优先级保持字母序:主类 AMain 在误报 ZListener 之前
                "src/main/java/com/foo/AMain.java",
                "src/main/java/com/foo/ZListener.java",
            ]
        );
    }

    #[test]
    fn manifest_implies_pnx_from_powernukkitx_yml_and_annotation() {
        let (targets, confidence) =
            manifest_implied_targets(&["src/main/resources/powernukkitx.yml".to_string()]);
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["pnx"]);
        assert_eq!(confidence.as_str(), "high");

        let (targets, confidence) =
            manifest_implied_targets(&["src/main/java/com/foo/Foo.java".to_string()]);
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["pnx"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn nukkit_yml_manifest_implies_shared_family_targets() {
        let (targets, confidence) =
            manifest_implied_targets(&["src/main/resources/nukkit.yml".to_string()]);
        assert_eq!(
            targets.into_iter().collect::<Vec<_>>(),
            vec!["nkmot", "nkx", "pnx"]
        );
        assert_eq!(confidence.as_str(), "medium");
    }

    #[test]
    fn detects_org_powernukkitx_targets() {
        let pom = r#"
            <dependency>
                <groupId>org.powernukkitx</groupId>
                <artifactId>server</artifactId>
            </dependency>
            <repository>
                <url>https://repo.powernukkitx.org/releases</url>
            </repository>
        "#;

        let (targets, confidence) = detect_targets_from_build_content(&pom.to_lowercase());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["pnx"]);
        assert_eq!(confidence.as_str(), "high");
    }

    #[test]
    fn plain_powernukkitx_topics_map_to_pnx() {
        let (targets, confidence) =
            detect_targets_from_topics(&["powernukkitx".to_string(), "pnx".to_string()]);
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec!["pnx"]);
        assert_eq!(confidence.as_str(), "medium");
    }

    #[test]
    fn categories_use_nukkit_prefixed_topics() {
        let categories = categories_from_topics(&[
            "nukkit-economy".to_string(),
            "utility".to_string(),
            "nukkit-plugin".to_string(),
        ]);

        assert_eq!(categories, vec!["economy", "utility"]);
    }

    #[test]
    fn category_detection_keeps_topics_when_ai_returns_no_categories() {
        let categories = combine_categories(vec!["economy".to_string()], Vec::new());

        assert_eq!(categories, vec!["economy"]);
    }

    #[test]
    fn category_detection_appends_ai_categories_without_duplicates() {
        let categories = combine_categories(
            vec!["economy".to_string(), "utility".to_string()],
            vec!["utility".to_string(), "management".to_string()],
        );

        assert_eq!(categories, vec!["economy", "utility", "management"]);
    }

    #[test]
    fn disabled_category_classification_uses_topics_without_calling_ai() {
        let mut repo = repo_with_dates("2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z");
        repo.topics = vec!["nukkit-economy".to_string()];

        let categories = detect_categories_with_classifier(
            &repo,
            "README",
            BuildOptions::without_ai_categories(),
            |_, _| panic!("AI classifier should not be called for update builds"),
        );

        assert_eq!(categories, vec!["economy"]);
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

    #[test]
    fn project_updated_timestamp_uses_push_activity_over_repository_metadata_updates() {
        let repo = repo_with_dates("2026-05-01T00:00:00Z", "2021-02-03T04:05:06Z");

        assert_eq!(
            project_updated_timestamp(&repo, &[]),
            parse_timestamp("2021-02-03T04:05:06Z")
        );
    }

    #[test]
    fn project_updated_timestamp_includes_newer_release_activity() {
        let repo = repo_with_dates("2026-05-01T00:00:00Z", "2021-02-03T04:05:06Z");
        let releases = vec![release_published_at("2022-06-07T08:09:10Z")];

        assert_eq!(
            project_updated_timestamp(&repo, &releases),
            parse_timestamp("2022-06-07T08:09:10Z")
        );
    }

    #[test]
    fn project_updated_timestamp_falls_back_to_repository_update_when_activity_dates_are_missing() {
        let repo = repo_with_dates("2026-05-01T00:00:00Z", "");

        assert_eq!(
            project_updated_timestamp(&repo, &[]),
            parse_timestamp("2026-05-01T00:00:00Z")
        );
    }

    fn release_with_assets(tag: &str, assets: &[(&str, u64)]) -> Release {
        Release {
            id: 1,
            tag_name: tag.to_string(),
            name: None,
            body: None,
            prerelease: false,
            draft: false,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            published_at: "2024-01-01T00:00:00Z".to_string(),
            assets: assets
                .iter()
                .enumerate()
                .map(|(index, (name, size))| ReleaseAsset {
                    id: index as u64,
                    name: (*name).to_string(),
                    size: *size,
                    download_count: 0,
                    browser_download_url: format!(
                        "https://github.com/owner/repo/releases/download/{}/{}",
                        tag, name
                    ),
                    content_type: "application/java-archive".to_string(),
                    digest: None,
                })
                .collect(),
        }
    }

    fn version_at(name: &str, published_at: u64) -> Version {
        Version {
            version: name.to_string(),
            name: name.to_string(),
            prerelease: false,
            changelog: String::new(),
            files: Vec::new(),
            downloads: 0,
            published_at,
        }
    }

    #[test]
    fn release_files_exclude_attachments_and_foreign_platform_jars() {
        let release = release_with_assets(
            "v1.0.0",
            &[
                ("Thing-1.0.0.jar", 10),
                ("Thing-1.0.0-sources.jar", 5),
                ("Thing-Bukkit-1.0.0.jar", 20),
                ("Thing-Nukkit-1.0.0.jar", 30),
            ],
        );

        let files = release_version_files(
            &release,
            "Thing",
            "thing-repo",
            "src/main/resources/plugin.yml",
            &["nkx".to_string(), "nkmot".to_string()],
        );

        let names: Vec<&str> = files.iter().map(|file| file.filename.as_str()).collect();
        assert_eq!(names, vec!["Thing-Nukkit-1.0.0.jar", "Thing-1.0.0.jar"]);
        assert_eq!(files.iter().filter(|file| file.primary).count(), 1);
        assert!(files[0].primary);
        assert_eq!(files[0].filename, "Thing-Nukkit-1.0.0.jar");
    }

    #[test]
    fn release_files_put_module_jar_first_and_drop_other_platforms() {
        let release = release_with_assets(
            "v1.5.0",
            &[
                ("KraftLink-API-1.5.0.jar", 7),
                ("KraftLink-Bukkit-1.5.0.jar", 481),
                ("KraftLink-Nukkit-1.5.0.jar", 470),
                ("KraftLink.jar", 900),
            ],
        );

        let files = release_version_files(
            &release,
            "KraftLink",
            "KraftLink",
            "nukkit/src/main/resources/nukkit.yml",
            &["nkx".to_string(), "nkmot".to_string(), "pnx".to_string()],
        );

        let names: Vec<&str> = files.iter().map(|file| file.filename.as_str()).collect();
        assert_eq!(names, vec!["KraftLink-Nukkit-1.5.0.jar"]);
        assert!(files[0].primary);
    }

    #[test]
    fn release_files_drop_all_foreign_platform_jars() {
        let release = release_with_assets(
            "5.5.11",
            &[
                ("LuckPerms-Fabric-5.5.11.jar", 100),
                ("LuckPerms-NeoForge-5.5.11.jar", 100),
            ],
        );

        let files = release_version_files(
            &release,
            "LuckPerms",
            "LuckPerms",
            "nukkit/loader/src/main/resources/plugin.yml",
            &["nkx".to_string(), "nkmot".to_string()],
        );

        assert!(files.is_empty());
    }

    #[test]
    fn camel_case_platform_suffix_is_recognised() {
        let release = release_with_assets(
            "1.2.5",
            &[
                ("ShitBotApi-1.2.5.jar", 10),
                ("ShitBotBungee-1.2.5.jar", 20),
                ("ShitBotNukkit-1.2.5.jar", 30),
                ("ShitBotRenderer-1.2.5.jar", 40),
                ("ShitBotSpigot-1.2.5.jar", 50),
                ("ShitBotVelocity-1.2.5.jar", 60),
            ],
        );

        let files = release_version_files(
            &release,
            "ShitBotNukkit",
            "ShitBot",
            "modules/platforms/nukkit/src/main/resources/plugin.yml",
            &["nkmot".to_string()],
        );

        let names: Vec<&str> = files.iter().map(|file| file.filename.as_str()).collect();
        // CamelCase 平台词被剔除;拼接式产物名匹配不到模块目录标识,
        // 因此 API/Renderer 仍保留,但 primary 必须是 Nukkit 本体。
        assert_eq!(
            names,
            vec![
                "ShitBotNukkit-1.2.5.jar",
                "ShitBotApi-1.2.5.jar",
                "ShitBotRenderer-1.2.5.jar"
            ]
        );
        assert!(files[0].primary);
    }

    #[test]
    fn primary_prefers_primary_target_over_other_targets() {
        let release = release_with_assets(
            "1.4.3",
            &[
                ("Whitelister-1.4.3-Nukkit.jar", 76401),
                ("Whitelister-1.4.3-PNX.jar", 76403),
            ],
        );

        let files = release_version_files(
            &release,
            "Whitelister",
            "Whitelister",
            "src/main/resources/plugin.yml",
            &["nkx".to_string(), "nkmot".to_string(), "pnx".to_string()],
        );

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].filename, "Whitelister-1.4.3-Nukkit.jar");
        assert!(files[0].primary);
    }

    #[test]
    fn primary_keeps_asset_order_when_no_name_matches() {
        let release = release_with_assets(
            "1.0.0",
            &[("small-first.jar", 10), ("large-second.jar", 900)],
        );

        let files = release_version_files(
            &release,
            "Unrelated",
            "unrelated-repo",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
        );

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].filename, "small-first.jar");
        assert!(files[0].primary);
    }

    #[test]
    fn primary_prefers_exact_plugin_name_over_prefixed_library() {
        let release = release_with_assets(
            "v1.3.3",
            &[
                ("KraftLink-API-1.3.3.jar", 7),
                ("KraftLink-Bukkit-1.3.3.jar", 481),
                ("KraftLink.jar", 900),
            ],
        );

        let files = release_version_files(
            &release,
            "KraftLink",
            "KraftLink",
            "nukkit/src/main/resources/nukkit.yml",
            &["nkx".to_string()],
        );

        let names: Vec<&str> = files.iter().map(|file| file.filename.as_str()).collect();
        assert_eq!(names, vec!["KraftLink.jar", "KraftLink-API-1.3.3.jar"]);
        assert!(files[0].primary);
    }

    #[test]
    fn release_with_only_engine_jars_after_platform_filter_has_no_files() {
        let release = release_with_assets(
            "v1.18.1",
            &[
                ("HuHoBot-Engine-GraalJs-1.18.1.jar", 10),
                ("HuHoBot-Engine-GraalPy-1.18.1.jar", 20),
                ("HuHoBot-Penguin_Spigot-1.18.1.jar", 30),
            ],
        );

        let files = release_version_files(
            &release,
            "HuHoBotPenguin",
            "PenguinAgent",
            "server/Nukkit/src/main/resources/plugin.yml",
            &["nkx".to_string(), "nkmot".to_string()],
        );

        assert!(files.is_empty());
    }

    #[test]
    fn plugin_named_engine_is_not_dropped_as_auxiliary() {
        let release = release_with_assets(
            "v1.0",
            &[("Engine-1.0.jar", 10), ("Engine-Bukkit-1.0.jar", 20)],
        );

        let files = release_version_files(
            &release,
            "Engine",
            "Engine",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
        );

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "Engine-1.0.jar");
    }

    #[test]
    fn plugin_name_with_platform_word_is_not_treated_as_foreign() {
        let release = release_with_assets("v1.0", &[("PaperBridge-1.0.jar", 10)]);

        let files = release_version_files(
            &release,
            "PaperBridge",
            "paperbridge",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
        );

        assert_eq!(files.len(), 1);
        assert!(files[0].primary);
    }

    #[test]
    fn single_module_release_with_only_foreign_jars_has_no_files() {
        let release = release_with_assets("v1.0", &[("Thing-Bukkit-1.0.jar", 10)]);

        let files = release_version_files(
            &release,
            "Thing",
            "thing",
            "src/main/resources/plugin.yml",
            &["nkx".to_string()],
        );

        assert!(files.is_empty());
    }

    #[test]
    fn multi_module_keeps_jars_whose_platform_word_belongs_to_the_name() {
        let release = release_with_assets(
            "v1.0",
            &[("PaperChat-1.0.jar", 10), ("PaperChat-Bukkit-1.0.jar", 20)],
        );

        let files = release_version_files(
            &release,
            "PaperChat",
            "PaperChat",
            "nukkit/src/main/resources/plugin.yml",
            &["nkx".to_string()],
        );

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "PaperChat-1.0.jar");
        assert!(files[0].primary);
    }

    #[test]
    fn versions_are_sorted_newest_first() {
        let mut versions = vec![
            version_at("old", 100),
            version_at("new", 300),
            version_at("mid", 200),
        ];

        sort_versions_newest_first(&mut versions);

        let names: Vec<&str> = versions
            .iter()
            .map(|version| version.version.as_str())
            .collect();
        assert_eq!(names, vec!["new", "mid", "old"]);
    }

    #[test]
    fn finds_build_file_with_different_case() {
        let tree = vec![GitTreeEntry {
            path: "tutorialplugin/build.gradle".to_string(),
            entry_type: "blob".to_string(),
            sha: String::new(),
            size: None,
        }];

        let paths = find_build_file_paths(&tree, "TutorialPlugin/src/main/resources/plugin.yml");

        assert_eq!(paths, vec!["tutorialplugin/build.gradle"]);
    }

    #[test]
    fn rejects_non_manifest_filenames() {
        assert!(!is_plugin_manifest_path("src/main/resources/myplugin.yml"));
        assert!(!is_plugin_manifest_path(
            "src/main/resources/not-plugin.yaml"
        ));
    }
}
