use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::plugin::GalleryItem;

use super::PostProcessContext;

static IMG_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<img\s+[^>]*src\s*=\s*["']([^"']+)["'][^>]*>"#).unwrap());
static IMG_ALT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"alt\s*=\s*["']([^"']*)["']"#).unwrap());
static MD_IMG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\(([^)]+)\)").unwrap());

fn is_relative_path(src: &str) -> bool {
    !src.starts_with("http://") && !src.starts_with("https://") && !src.starts_with("data:")
}

/// README 里的图片地址可能带 query/fragment、Markdown title、前导斜杠或
/// percent-encoding;比较 git tree 前先归一化,避免把存在的图片误判为死链。
fn normalized_path(src: &str) -> String {
    let mut path = src.trim();
    if let Some(index) = path.find(['?', '#']) {
        path = &path[..index];
    }
    if let Some(index) = path.find(" \"") {
        path = &path[..index];
    }
    if let Some(index) = path.find(" '") {
        path = &path[..index];
    }
    let trimmed = path.trim_start_matches("./").trim_start_matches('/');
    percent_decode(trimmed)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push(high * 16 + low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn to_raw_url(path: &str, ctx: &PostProcessContext) -> String {
    super::to_raw_url(ctx.owner, ctx.repo, ctx.branch, path)
}

fn now_date() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

pub fn process_html_images(
    content: &str,
    ctx: &PostProcessContext,
    gallery: &mut Vec<GalleryItem>,
    blobs: &HashSet<&str>,
) -> String {
    let mut result = content.to_string();

    for cap in IMG_TAG_RE.captures_iter(content) {
        let full_match = cap.get(0).unwrap().as_str();
        let src = cap.get(1).unwrap().as_str();

        if is_relative_path(src) {
            let path = normalized_path(src);
            let raw_url = to_raw_url(&path, ctx);
            let new_tag = full_match.replace(src, &raw_url);
            result = result.replace(full_match, &new_tag);

            // 仓库里不存在的相对图片不进修图库,否则前端会拿到死链。
            if !blobs.contains(path.as_str()) {
                continue;
            }

            let title = IMG_ALT_RE
                .captures(full_match)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();

            gallery.push(GalleryItem {
                url: raw_url,
                title,
                description: String::new(),
                created: now_date(),
            });
        }
    }

    result
}

pub fn process_md_images(
    content: &str,
    ctx: &PostProcessContext,
    gallery: &mut Vec<GalleryItem>,
    blobs: &HashSet<&str>,
) -> String {
    let mut result = content.to_string();

    for cap in MD_IMG_RE.captures_iter(content) {
        let full_match = cap.get(0).unwrap().as_str();
        let alt = cap.get(1).unwrap().as_str();
        let src = cap.get(2).unwrap().as_str();

        if is_relative_path(src) {
            let path = normalized_path(src);
            let raw_url = to_raw_url(&path, ctx);
            let new_md = format!("![{}]({})", alt, raw_url);
            result = result.replace(full_match, &new_md);

            // 仓库里不存在的相对图片不进修图库,否则前端会拿到死链。
            if !blobs.contains(path.as_str()) {
                continue;
            }

            gallery.push(GalleryItem {
                url: raw_url,
                title: alt.to_string(),
                description: String::new(),
                created: now_date(),
            });
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::process_md_images;
    use crate::plugin::GalleryItem;
    use crate::sync::builder::PostProcessContext;
    use std::collections::HashSet;

    #[test]
    fn only_existing_relative_images_reach_gallery() {
        let ctx = PostProcessContext {
            owner: "owner",
            repo: "repo",
            branch: "main",
        };
        let blobs: HashSet<&str> = ["img/there.png", "img/space name.png"]
            .into_iter()
            .collect();
        let mut gallery: Vec<GalleryItem> = Vec::new();

        let result = process_md_images(
            "![missing](img/missing.png)\n![there](./img/there.png)\n![query](img/there.png?raw=true)\n![encoded](img%2Fspace%20name.png \"title\")",
            &ctx,
            &mut gallery,
            &blobs,
        );

        assert!(result.contains("raw.githubusercontent.com/owner/repo/main/img/missing.png"));
        assert_eq!(gallery.len(), 3);
        assert_eq!(
            gallery[0].url,
            "https://raw.githubusercontent.com/owner/repo/main/img/there.png"
        );
        assert_eq!(
            gallery[1].url,
            "https://raw.githubusercontent.com/owner/repo/main/img/there.png"
        );
        assert_eq!(
            gallery[2].url,
            "https://raw.githubusercontent.com/owner/repo/main/img/space name.png"
        );
    }
}
