use super::plugin_yml::{ApiVersion, Authors, NukkitPluginYml};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// PNX 注解名称。最新版 PowerNukkitX 官方模板(PluginTemplate)不再手写
/// plugin.yml / powernukkitx.yml,而是在主类上标注 `@PluginMeta`,
/// 由 PNX-APT 注解处理器在编译期生成 powernukkitx.yml。
const ANNOTATION_NAME: &str = "PluginMeta";

/// 从 Java 源码解析 `@PluginMeta` 注解,生成与 plugin.yml 等价的清单结构。
///
/// file_path 用于推导 main 类名(`package` 声明 + 文件名)。
/// 返回 None 表示该文件不是可识别的 PNX 插件主类(注解定义文件、
/// 缺少必填 name、未继承 PluginBase 等)。
pub fn parse_plugin_meta(source: &str, file_path: &str) -> Option<NukkitPluginYml> {
    let code = strip_java_comments(source);

    if is_annotation_definition(&code) {
        return None;
    }
    // PNX-APT 强制被注解类继承 PluginBase,主类源文件必然出现该类型名
    if !code.contains("PluginBase") {
        return None;
    }

    let inner = annotation_args(&code)?;

    let mut name = String::new();
    let mut version = String::new();
    let mut api: Vec<String> = Vec::new();
    let mut authors: Vec<String> = Vec::new();
    let mut description = String::new();
    let mut website = String::new();
    let mut depend: Vec<String> = Vec::new();
    let mut softdepend: Vec<String> = Vec::new();
    let mut load: Option<String> = None;

    for arg in split_annotation_args(&inner) {
        let Some((key, value)) = parse_annotation_pair(&arg) else {
            continue;
        };
        match key.as_str() {
            "name" => {
                if let AnnValue::Text(text) = value {
                    name = text;
                }
            }
            "version" => {
                if let AnnValue::Text(text) = value {
                    version = text;
                }
            }
            "api" => match value {
                AnnValue::Text(text) => api = vec![text],
                AnnValue::List(items) => api = items,
            },
            "authors" => match value {
                AnnValue::Text(text) => authors = vec![text],
                AnnValue::List(items) => authors = items,
            },
            "description" => {
                if let AnnValue::Text(text) = value {
                    description = text;
                }
            }
            "website" => {
                if let AnnValue::Text(text) = value {
                    website = text;
                }
            }
            "depend" => match value {
                AnnValue::Text(text) => depend = vec![text],
                AnnValue::List(items) => depend = items,
            },
            "softDepend" | "softdepend" => match value {
                AnnValue::Text(text) => softdepend = vec![text],
                AnnValue::List(items) => softdepend = items,
            },
            "order" => {
                if let AnnValue::Text(text) = value {
                    let variant = text.rsplit('.').next().unwrap_or_default();
                    if variant == "STARTUP" || variant == "POSTWORLD" {
                        load = Some(variant.to_string());
                    }
                }
            }
            // prefix / loadBefore / features 等字段没有索引等价物
            _ => {}
        }
    }

    if name.trim().is_empty() {
        return None;
    }

    Some(NukkitPluginYml {
        name,
        version,
        main: derive_main_class(&code, file_path),
        api: if api.len() == 1 {
            ApiVersion::Single(api.pop().unwrap_or_default())
        } else {
            ApiVersion::Multiple(api)
        },
        author: None,
        authors: if authors.len() == 1 {
            Authors::Single(authors.pop().unwrap_or_default())
        } else {
            Authors::Multiple(authors)
        },
        description: if description.is_empty() {
            None
        } else {
            Some(description)
        },
        website: if website.is_empty() {
            None
        } else {
            Some(website)
        },
        depend,
        softdepend,
        load,
        extra: HashMap::new(),
    })
}

enum AnnValue {
    Text(String),
    List(Vec<String>),
}

fn is_annotation_definition(code: &str) -> bool {
    let mut search_from = 0;
    while let Some(offset) = code[search_from..].find("@interface") {
        let after = &code[search_from + offset + "@interface".len()..];
        if after.trim_start().starts_with(ANNOTATION_NAME) {
            return true;
        }
        search_from += offset + "@interface".len();
    }
    false
}

/// 提取 `@PluginMeta(...)` 的参数文本。支持 `@PluginMeta` 与全限定
/// `@org.powernukkitx.plugin.annotation.PluginMeta` 两种写法。
fn annotation_args(code: &str) -> Option<String> {
    let chars: Vec<char> = code.chars().collect();
    let name: Vec<char> = ANNOTATION_NAME.chars().collect();
    let mut i = 0;

    while i + name.len() <= chars.len() {
        if chars[i..i + name.len()] != name[..] {
            i += 1;
            continue;
        }

        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        let prev_is_marker = matches!(prev, Some('@') | Some('.'));
        let mut j = i + name.len();
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }

        if prev_is_marker && j < chars.len() && chars[j] == '(' {
            return scan_balanced_parens(&chars, j);
        }

        i += name.len();
    }

    None
}

fn scan_balanced_parens(chars: &[char], open: usize) -> Option<String> {
    let mut depth = 1i32;
    let mut in_string = false;
    let mut in_char = false;
    let mut escaped = false;
    let mut inner = String::new();
    let mut k = open + 1;

    while k < chars.len() {
        let c = chars[k];
        if escaped {
            inner.push(c);
            escaped = false;
            k += 1;
            continue;
        }
        if (in_string || in_char) && c == '\\' {
            inner.push(c);
            escaped = true;
            k += 1;
            continue;
        }
        if in_string {
            inner.push(c);
            if c == '"' {
                in_string = false;
            }
            k += 1;
            continue;
        }
        if in_char {
            inner.push(c);
            if c == '\'' {
                in_char = false;
            }
            k += 1;
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                inner.push(c);
            }
            '\'' => {
                in_char = true;
                inner.push(c);
            }
            '(' => {
                depth += 1;
                inner.push(c);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(inner);
                }
                inner.push(c);
            }
            _ => inner.push(c),
        }
        k += 1;
    }

    None
}

fn split_annotation_args(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut in_char = false;
    let mut escaped = false;
    let mut current = String::new();

    for c in inner.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        if (in_string || in_char) && c == '\\' {
            current.push(c);
            escaped = true;
            continue;
        }
        if in_string {
            current.push(c);
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            current.push(c);
            if c == '\'' {
                in_char = false;
            }
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                current.push(c);
            }
            '\'' => {
                in_char = true;
                current.push(c);
            }
            '(' | '{' | '[' => {
                depth += 1;
                current.push(c);
            }
            ')' | '}' | ']' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                parts.push(current.clone());
                current.clear();
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

fn parse_annotation_pair(arg: &str) -> Option<(String, AnnValue)> {
    let eq = arg.find('=')?;
    let key = arg[..eq].trim().to_string();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }

    let value = parse_annotation_value(arg[eq + 1..].trim());
    Some((key, value))
}

fn parse_annotation_value(raw: &str) -> AnnValue {
    if raw.starts_with('"') {
        AnnValue::Text(parse_string_literal(raw).unwrap_or_default())
    } else if raw.starts_with('{') {
        AnnValue::List(parse_array_items(raw))
    } else {
        let end = raw
            .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(raw.len());
        AnnValue::Text(raw[..end].to_string())
    }
}

fn parse_array_items(raw: &str) -> Vec<String> {
    let inner = raw.trim().trim_start_matches('{').trim_end_matches('}');
    let mut items = Vec::new();

    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.starts_with('"') {
            if let Some(text) = parse_string_literal(part) {
                items.push(text);
            }
        } else {
            let end = part
                .find(|c: char| c == '(' || c.is_whitespace())
                .unwrap_or(part.len());
            if !part[..end].is_empty() {
                items.push(part[..end].to_string());
            }
        }
    }

    items
}

fn parse_string_literal(raw: &str) -> Option<String> {
    let mut chars = raw.chars();
    if chars.next()? != '"' {
        return None;
    }

    let mut out = String::new();
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if escaped {
            match c {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(c);
                    }
                }
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }

    Some(out)
}

fn derive_main_class(code: &str, file_path: &str) -> String {
    let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    let stem = file_name.strip_suffix(".java").unwrap_or(file_name);

    if let Some(package) = parse_package_declaration(code) {
        return format!("{}.{}", package, stem);
    }
    if let Some(pos) = file_path.find("src/main/java/") {
        let rel = &file_path[pos + "src/main/java/".len()..];
        let rel = rel.strip_suffix(".java").unwrap_or(rel);
        return rel.replace('/', ".");
    }
    stem.to_string()
}

fn parse_package_declaration(code: &str) -> Option<String> {
    static PACKAGE_RE: OnceLock<Regex> = OnceLock::new();
    let re = PACKAGE_RE.get_or_init(|| {
        Regex::new(
            r"(?m)^[ \t]*package[ \t]+([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)[ \t]*;",
        )
        .expect("valid package regex")
    });
    re.captures(code).map(|caps| caps[1].to_string())
}

/// 移除 Java 行注释与块注释,保留字符串/字符字面量原文
fn strip_java_comments(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Code,
        LineComment,
        BlockComment,
        Str,
        Char,
        TextBlock,
    }

    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut state = State::Code;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        match state {
            State::Code => match c {
                '/' if next == Some('/') => {
                    state = State::LineComment;
                    i += 2;
                }
                '/' if next == Some('*') => {
                    state = State::BlockComment;
                    i += 2;
                    out.push(' ');
                }
                '"' if next == Some('"') && chars.get(i + 2) == Some(&'"') => {
                    state = State::TextBlock;
                    out.push_str("\"\"\"");
                    i += 3;
                }
                '"' => {
                    state = State::Str;
                    out.push(c);
                    i += 1;
                }
                '\'' => {
                    state = State::Char;
                    out.push(c);
                    i += 1;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            },
            State::LineComment => {
                if c == '\n' {
                    state = State::Code;
                    out.push(c);
                }
                i += 1;
            }
            State::BlockComment => {
                if c == '*' && next == Some('/') {
                    state = State::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            State::Str => {
                out.push(c);
                if c == '\\'
                    && let Some(escaped) = next
                {
                    out.push(escaped);
                    i += 2;
                    continue;
                }
                if c == '"' {
                    state = State::Code;
                }
                i += 1;
            }
            State::Char => {
                out.push(c);
                if c == '\\'
                    && let Some(escaped) = next
                {
                    out.push(escaped);
                    i += 2;
                    continue;
                }
                if c == '\'' {
                    state = State::Code;
                }
                i += 1;
            }
            State::TextBlock => {
                out.push(c);
                if c == '"' && next == Some('"') && chars.get(i + 2) == Some(&'"') {
                    out.push_str("\"\"");
                    state = State::Code;
                    i += 3;
                } else {
                    i += 1;
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::parse_plugin_meta;

    const TEMPLATE_SOURCE: &str = r#"
package io.github.cooldev.example;

import org.powernukkitx.plugin.PluginBase;
import org.powernukkitx.plugin.annotation.PluginMeta;

// @PluginMeta(name = "Commented") 注释里的注解应被忽略
/* @PluginMeta(name = "BlockCommented") */
public class ExamplePlugin extends PluginBase {
}
"#;

    #[test]
    fn parses_template_style_annotation() {
        let source = r#"
package io.github.cooldev.example;

import org.powernukkitx.plugin.PluginBase;
import org.powernukkitx.plugin.annotation.PluginMeta;

@PluginMeta(
        name = "ExamplePlugin",
        version = "1.0.0",
        authors = {"cooldev"},
        api = {"3.0.0"},
        description = "An example plugin",
        website = "https://github.com/cooldev/example",
        depend = {"EconomyAPI"},
        softDepend = {"FormAPI", "ScoreBoardAPI"},
        order = PluginLoadOrder.POSTWORLD,
        features = {"new-feature"}
)
public class ExamplePlugin extends PluginBase {
}
"#;

        let manifest = parse_plugin_meta(
            source,
            "src/main/java/io/github/cooldev/example/ExamplePlugin.java",
        )
        .expect("annotation manifest should parse");

        assert_eq!(manifest.name, "ExamplePlugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.main, "io.github.cooldev.example.ExamplePlugin");
        assert_eq!(manifest.api.as_vec(), vec!["3.0.0"]);
        assert_eq!(manifest.all_authors(), vec!["cooldev"]);
        assert_eq!(manifest.description.as_deref(), Some("An example plugin"));
        assert_eq!(
            manifest.website.as_deref(),
            Some("https://github.com/cooldev/example")
        );
        assert_eq!(manifest.depend, vec!["EconomyAPI"]);
        assert_eq!(manifest.softdepend, vec!["FormAPI", "ScoreBoardAPI"]);
        assert_eq!(manifest.load.as_deref(), Some("POSTWORLD"));
    }

    #[test]
    fn parses_fully_qualified_annotation() {
        let source = r#"
package com.example;

@org.powernukkitx.plugin.annotation.PluginMeta(name = "FQPlugin", version = "2.0", api = "3.0.0")
public class FQPlugin extends org.powernukkitx.plugin.PluginBase {
}
"#;

        let manifest = parse_plugin_meta(source, "src/main/java/com/example/FQPlugin.java")
            .expect("fully qualified annotation should parse");

        assert_eq!(manifest.name, "FQPlugin");
        assert_eq!(manifest.main, "com.example.FQPlugin");
        assert_eq!(manifest.api.primary().as_deref(), Some("3.0.0"));
    }

    #[test]
    fn ignores_annotation_mentions_in_comments_and_imports() {
        // 只有注释中提到注解、没有实际标注的文件不应被当作清单
        let source = r#"
package com.example;

import org.powernukkitx.plugin.PluginBase;

/**
 * 参见 @PluginMeta 注解
 */
// @PluginMeta(name = "NotReal")
public class SomeListener extends PluginBase {
}
"#;

        let manifest = parse_plugin_meta(source, "src/main/java/com/example/SomeListener.java");
        assert!(manifest.is_none());
    }

    #[test]
    fn rejects_annotation_definition_file() {
        let source = r#"
package org.powernukkitx.plugin.annotation;

public @interface PluginMeta {
    String name();
}
"#;

        let manifest = parse_plugin_meta(
            source,
            "src/main/java/org/powernukkitx/plugin/annotation/PluginMeta.java",
        );
        assert!(manifest.is_none());
    }

    #[test]
    fn requires_name_and_plugin_base() {
        let without_name = "@PluginMeta(version = \"1.0.0\")\nclass A extends PluginBase {}\n";
        assert!(parse_plugin_meta(without_name, "src/main/java/A.java").is_none());

        let without_base = "@PluginMeta(name = \"X\")\nclass B {}\n";
        assert!(parse_plugin_meta(without_base, "src/main/java/B.java").is_none());
    }

    #[test]
    fn handles_escaped_strings_and_single_element_arrays() {
        let source = r#"
package com.example;

@PluginMeta(
        name = "Quote\"Plugin",
        version = "1.0.0",
        api = "3.0.0",
        authors = {"a", "b"},
        description = "line1\nline2"
)
public class QuotePlugin extends PluginBase {
}
"#;

        let manifest =
            parse_plugin_meta(source, "src/main/java/com/example/QuotePlugin.java").unwrap();

        assert_eq!(manifest.name, "Quote\"Plugin");
        assert_eq!(manifest.api.as_vec(), vec!["3.0.0"]);
        assert_eq!(manifest.all_authors(), vec!["a", "b"]);
        assert_eq!(manifest.description.as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn derives_main_from_package_over_path() {
        let source = r#"
@PluginMeta(name = "P", version = "1.0.0", api = "3.0.0")
public class P extends PluginBase {}
"#;

        let manifest = parse_plugin_meta(source, "src/main/java/com/example/P.java").unwrap();
        // 没有 package 声明时回退到路径推导
        assert_eq!(manifest.main, "com.example.P");
    }

    #[test]
    fn strips_comments_before_locating_annotation() {
        let source = TEMPLATE_SOURCE.replace(
            "public class ExamplePlugin extends PluginBase {",
            "@PluginMeta(name = \"Real\", version = \"1.0\", api = \"3.0.0\")\npublic class ExamplePlugin extends PluginBase {",
        );

        let manifest = parse_plugin_meta(
            &source,
            "src/main/java/io/github/cooldev/example/ExamplePlugin.java",
        )
        .unwrap();

        assert_eq!(manifest.name, "Real");
    }

    #[test]
    fn maps_startup_load_order() {
        let source = r#"
package com.example;

@PluginMeta(name = "Early", version = "1.0.0", api = "3.0.0", order = PluginLoadOrder.STARTUP)
public class Early extends PluginBase {}
"#;

        let manifest = parse_plugin_meta(source, "src/main/java/com/example/Early.java").unwrap();
        assert_eq!(manifest.load.as_deref(), Some("STARTUP"));
    }
}
