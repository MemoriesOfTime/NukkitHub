use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub primary_target: String,
    #[serde(default)]
    pub manifest_path: String,
    #[serde(default)]
    pub detection_confidence: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub license: License,
    #[serde(default)]
    pub links: Option<Links>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub stars: u64,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub icon_url: String,
    #[serde(default)]
    pub gallery: Vec<GalleryItem>,
    #[serde(default)]
    pub versions: Vec<Version>,
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default, skip_serializing)]
    pub preserved_fields: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Author {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub avatar_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct License {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Links {
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub wiki: String,
    #[serde(default)]
    pub discord: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GalleryItem {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Version {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub changelog: String,
    #[serde(default)]
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub published_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionFile {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependency {
    pub plugin_id: String,
    #[serde(default)]
    pub version_range: String,
    #[serde(default)]
    pub dependency_type: String,
}

fn repo_owner_from_id(id: &str) -> String {
    id.split('/').next().unwrap_or(id).trim().to_string()
}

fn github_username_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let rest = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))?;
    let username = rest.split('/').next()?.trim();

    if username.is_empty() {
        None
    } else {
        Some(username.to_string())
    }
}

impl Plugin {
    pub fn get_author_name(&self) -> String {
        let owner = repo_owner_from_id(&self.id);
        if !owner.is_empty()
            && let Some(author) = self.authors.iter().find(|author| {
                author.name.eq_ignore_ascii_case(&owner)
                    || github_username_from_url(&author.url)
                        .is_some_and(|username| username.eq_ignore_ascii_case(&owner))
            })
        {
            return author.name.clone();
        }

        self.authors
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{Author, License, Plugin};

    fn base_plugin() -> Plugin {
        Plugin {
            id: "owner/repo".to_string(),
            name: "repo".to_string(),
            source: String::new(),
            targets: Vec::new(),
            primary_target: String::new(),
            manifest_path: String::new(),
            detection_confidence: String::new(),
            summary: String::new(),
            description: String::new(),
            authors: Vec::new(),
            categories: Vec::new(),
            license: License::default(),
            links: None,
            downloads: 0,
            stars: 0,
            created_at: 0,
            updated_at: 0,
            icon_url: String::new(),
            gallery: Vec::new(),
            versions: Vec::new(),
            api_version: String::new(),
            server_version: String::new(),
            dependencies: Vec::new(),
            preserved_fields: Default::default(),
        }
    }

    #[test]
    fn prefers_owner_author_over_first_contributor() {
        let mut plugin = base_plugin();
        plugin.authors = vec![
            Author {
                name: "top-contributor".to_string(),
                url: "https://github.com/top-contributor".to_string(),
                avatar_url: String::new(),
            },
            Author {
                name: "owner".to_string(),
                url: "https://github.com/owner".to_string(),
                avatar_url: String::new(),
            },
        ];

        assert_eq!(plugin.get_author_name(), "owner");
    }
}
