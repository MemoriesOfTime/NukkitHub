use std::sync::LazyLock;

use regex::Regex;

use super::PostProcessContext;

// 链接文本允许一层嵌套中括号,否则徽章语法 [![alt](img)](href) 会从内层
// 图片的 [ 开始匹配,把图片地址当成 href,真正的相对链接被漏掉。
static MD_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[((?:[^\[\]]|\[[^\[\]]*\])*)\]\(([^)]+)\)").unwrap());

static HTML_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<a\s+[^>]*href\s*=\s*["']([^"']+)["'][^>]*>"#).unwrap());

fn is_relative_path(src: &str) -> bool {
    !src.starts_with("http://")
        && !src.starts_with("https://")
        && !src.starts_with("data:")
        && !src.starts_with('#')
        && !src.starts_with("mailto:")
}

fn to_blob_url(src: &str, ctx: &PostProcessContext) -> String {
    let path = src.trim_start_matches("./");
    format!(
        "https://github.com/{}/{}/blob/{}/{}",
        ctx.owner, ctx.repo, ctx.branch, path
    )
}

pub fn process_md_links(content: &str, ctx: &PostProcessContext) -> String {
    let mut result = content.to_string();

    for cap in MD_LINK_RE.captures_iter(content) {
        let full_match = cap.get(0).unwrap();
        let start = full_match.start();

        if start > 0 && content.as_bytes()[start - 1] == b'!' {
            continue;
        }

        let text = cap.get(1).unwrap().as_str();
        let href = cap.get(2).unwrap().as_str();

        if is_relative_path(href) {
            let blob_url = to_blob_url(href, ctx);
            let new_md = format!("[{}]({})", text, blob_url);
            result = result.replace(full_match.as_str(), &new_md);
        }
    }

    result
}

pub fn process_html_links(content: &str, ctx: &PostProcessContext) -> String {
    let mut result = content.to_string();

    for cap in HTML_LINK_RE.captures_iter(content) {
        let full_match = cap.get(0).unwrap().as_str();
        let href = cap.get(1).unwrap().as_str();

        if is_relative_path(href) {
            let blob_url = to_blob_url(href, ctx);
            let new_tag = full_match.replace(href, &blob_url);
            result = result.replace(full_match, &new_tag);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::process_md_links;
    use crate::sync::builder::PostProcessContext;

    fn ctx() -> PostProcessContext<'static> {
        PostProcessContext {
            owner: "owner",
            repo: "repo",
            branch: "main",
        }
    }

    #[test]
    fn rewrites_plain_relative_link() {
        assert_eq!(
            process_md_links("see [docs](./LICENSE)", &ctx()),
            "see [docs](https://github.com/owner/repo/blob/main/LICENSE)"
        );
    }

    #[test]
    fn rewrites_badge_target_without_touching_image_url() {
        let input = "[![License](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)";
        assert_eq!(
            process_md_links(input, &ctx()),
            "[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/owner/repo/blob/main/LICENSE)"
        );
    }
}
