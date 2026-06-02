use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::env;
use std::time::Duration;
use tracing::{debug, warn};

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_TIMEOUT_SECONDS: u64 = 45;
const MAX_README_CHARS: usize = 16_000;
const MAX_CATEGORIES: usize = 3;

#[derive(Debug, Clone)]
struct AiConfig {
    api_key: String,
    base_url: String,
    model: String,
    timeout_seconds: u64,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u16,
    response_format: ResponseFormat,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    #[serde(default)]
    content: String,
}

impl AiConfig {
    fn from_env() -> Option<Self> {
        let api_key = first_env(&["NUKKITHUB_AI_API_KEY", "DEEPSEEK_API_KEY"])?;
        let base_url = first_env(&["NUKKITHUB_AI_BASE_URL", "DEEPSEEK_BASE_URL"])
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = first_env(&["NUKKITHUB_AI_MODEL", "DEEPSEEK_MODEL"])
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let timeout_seconds = first_env(&["NUKKITHUB_AI_TIMEOUT_SECONDS"])
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS);

        Some(Self {
            api_key,
            base_url,
            model,
            timeout_seconds,
        })
    }

    fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

fn first_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

pub fn classify_readme_categories(readme: &str, allowed_categories: &[&str]) -> Vec<String> {
    if readme.trim().is_empty() {
        return Vec::new();
    }

    let Some(config) = AiConfig::from_env() else {
        return Vec::new();
    };

    match request_categories(&config, readme, allowed_categories) {
        Ok(categories) => categories,
        Err(e) => {
            warn!(error = %e, "AI category classification failed");
            Vec::new()
        }
    }
}

pub fn category_classification_enabled() -> bool {
    AiConfig::from_env().is_some()
}

fn request_categories(
    config: &AiConfig,
    readme: &str,
    allowed_categories: &[&str],
) -> Result<Vec<String>, String> {
    let prompt = build_category_prompt(readme, allowed_categories);
    let request = ChatCompletionRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You classify Nukkit server plugins. Return only a JSON object."
                    .to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ],
        temperature: 0.0,
        max_tokens: 160,
        response_format: ResponseFormat {
            kind: "json_object".to_string(),
        },
    };

    let mut response = ureq::post(config.chat_completions_url())
        .header("Authorization", &format!("Bearer {}", config.api_key))
        .header("User-Agent", "NukkitIndexer")
        .config()
        .timeout_global(Some(Duration::from_secs(config.timeout_seconds)))
        .http_status_as_error(false)
        .build()
        .send_json(&request)
        .map_err(|e| format!("HTTP error: {}", e))?;

    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Read error: {}", e))?;

    if status >= 400 {
        return Err(format!("HTTP status {}: {}", status, truncate_error(&body)));
    }

    let completion: ChatCompletionResponse =
        serde_json::from_str(&body).map_err(|e| format!("Parse error: {}", e))?;
    let content = completion
        .choices
        .first()
        .map(|choice| choice.message.content.trim())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| "Missing response content".to_string())?;

    let categories = parse_category_response(content, allowed_categories);
    debug!(categories = ?categories, "AI category classification complete");
    Ok(categories)
}

fn build_category_prompt(readme: &str, allowed_categories: &[&str]) -> String {
    let truncated = truncate_chars(readme, MAX_README_CHARS);
    format!(
        "Classify this Nukkit plugin README into up to {MAX_CATEGORIES} category IDs.\n\
         Use only these category IDs: {}.\n\
         Base the classification only on the README text. If unclear, return an empty array.\n\
         Respond as JSON exactly like {{\"categories\":[\"utility\"]}}.\n\n\
         README:\n{}",
        allowed_categories.join(", "),
        truncated
    )
}

fn parse_category_response(content: &str, allowed_categories: &[&str]) -> Vec<String> {
    let allowed: BTreeSet<&str> = allowed_categories.iter().copied().collect();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };

    let Some(items) = value.get("categories").and_then(|value| value.as_array()) else {
        return Vec::new();
    };

    let mut seen = BTreeSet::new();
    let mut categories = Vec::new();

    for item in items {
        let Some(category) = item.as_str().map(str::trim) else {
            continue;
        };
        if allowed.contains(category) && seen.insert(category.to_string()) {
            categories.push(category.to_string());
        }
        if categories.len() >= MAX_CATEGORIES {
            break;
        }
    }

    categories
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn truncate_error(value: &str) -> String {
    truncate_chars(value.trim(), 512)
}

#[cfg(test)]
mod tests {
    use super::{AiConfig, parse_category_response, truncate_chars};

    const ALLOWED: &[&str] = &["economy", "management", "minigame", "utility"];

    #[test]
    fn parses_allowed_categories_from_json_response() {
        let categories = parse_category_response(
            r#"{"categories":["economy","unknown","utility","economy"]}"#,
            ALLOWED,
        );

        assert_eq!(categories, vec!["economy", "utility"]);
    }

    #[test]
    fn limits_ai_categories() {
        let categories = parse_category_response(
            r#"{"categories":["economy","management","minigame","utility"]}"#,
            ALLOWED,
        );

        assert_eq!(categories, vec!["economy", "management", "minigame"]);
    }

    #[test]
    fn ignores_non_json_category_responses() {
        assert!(parse_category_response("economy, utility", ALLOWED).is_empty());
    }

    #[test]
    fn builds_chat_completions_url_from_base_url() {
        let config = AiConfig {
            api_key: "key".to_string(),
            base_url: "https://api.deepseek.com/".to_string(),
            model: "deepseek-v4-flash".to_string(),
            timeout_seconds: 45,
        };

        assert_eq!(
            config.chat_completions_url(),
            "https://api.deepseek.com/chat/completions"
        );
    }

    #[test]
    fn truncates_by_chars_not_bytes() {
        assert_eq!(truncate_chars("经济管理", 2), "经济");
    }
}
