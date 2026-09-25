use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use tracing::{debug, info, warn};

const JENKINS_API_URL: &str = "https://motci.cn/api/json?tree=jobs[name,url,color,lastSuccessfulBuild[number,timestamp,artifacts[fileName,relativePath],actions[_class,remoteUrls]],jobs[name,url,color,lastSuccessfulBuild[number,timestamp,artifacts[fileName,relativePath],actions[_class,remoteUrls]]]]";

// Jenkins API response types

#[derive(Deserialize)]
struct JenkinsResponse {
    #[serde(default)]
    jobs: Vec<JenkinsJob>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JenkinsJob {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    last_successful_build: Option<JenkinsBuild>,
    // Folder jobs contain nested jobs
    #[serde(default)]
    jobs: Option<Vec<JenkinsJob>>,
}

#[derive(Deserialize)]
struct JenkinsBuild {
    number: u64,
    #[serde(default)]
    timestamp: u64,
    #[serde(default)]
    artifacts: Vec<JenkinsArtifact>,
    #[serde(default)]
    actions: Vec<JenkinsAction>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JenkinsArtifact {
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    relative_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JenkinsAction {
    #[serde(rename = "_class", default)]
    class: String,
    #[serde(default)]
    remote_urls: Option<Vec<String>>,
}

// Parsed build info

pub struct JenkinsBuildInfo {
    pub job_url: String,
    pub build_number: u64,
    pub timestamp: u64,
    pub artifacts: Vec<(String, String)>, // (filename, relative_path)
}

/// Jenkins 构建产物直链: {job_url}{build}/artifact/{relative_path}
/// (无需认证;job_url 带或不带尾部斜杠均可)
pub fn artifact_url(job_url: &str, build_number: u64, relative_path: &str) -> String {
    format!(
        "{}/{}/artifact/{}",
        job_url.trim_end_matches('/'),
        build_number,
        relative_path
    )
}

pub struct JenkinsIndex {
    builds: HashMap<String, JenkinsBuildInfo>, // repo_full_name -> build info
}

impl JenkinsIndex {
    pub fn get(&self, repo_full_name: &str) -> Option<&JenkinsBuildInfo> {
        self.builds.get(&repo_full_name.to_lowercase())
    }

    pub fn repo_names(&self) -> impl Iterator<Item = &str> {
        self.builds.keys().map(|s| s.as_str())
    }
}

static INDEX: OnceLock<JenkinsIndex> = OnceLock::new();

pub fn init_jenkins() {
    INDEX.get_or_init(|| match fetch_jenkins_index() {
        Ok(index) => {
            info!(count = index.builds.len(), "Jenkins index loaded");
            index
        }
        Err(e) => {
            warn!(error = %e, "Failed to load Jenkins index, continuing without it");
            JenkinsIndex {
                builds: HashMap::new(),
            }
        }
    });
}

pub fn jenkins_index() -> &'static JenkinsIndex {
    INDEX.get_or_init(|| {
        warn!("Jenkins index accessed before init, returning empty");
        JenkinsIndex {
            builds: HashMap::new(),
        }
    })
}

fn fetch_jenkins_index() -> Result<JenkinsIndex, String> {
    info!("Fetching Jenkins build data from motci.cn");

    let response: JenkinsResponse = ureq::get(JENKINS_API_URL)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("JSON parse failed: {}", e))?;

    let mut builds = HashMap::new();

    for job in &response.jobs {
        process_job(job, &mut builds);
    }

    Ok(JenkinsIndex { builds })
}

fn process_job(job: &JenkinsJob, builds: &mut HashMap<String, JenkinsBuildInfo>) {
    // Folder job: pick "master" sub-job, else first with a successful build
    if let Some(sub_jobs) = &job.jobs {
        let chosen = sub_jobs
            .iter()
            .find(|j| j.name == "master" && j.last_successful_build.is_some())
            .or_else(|| sub_jobs.iter().find(|j| j.last_successful_build.is_some()));

        if let Some(sub) = chosen {
            process_leaf_job(sub, builds);
        }
        return;
    }

    process_leaf_job(job, builds);
}

fn process_leaf_job(job: &JenkinsJob, builds: &mut HashMap<String, JenkinsBuildInfo>) {
    let build = match &job.last_successful_build {
        Some(b) => b,
        None => return,
    };

    let repo_key = match extract_repo_from_build(build) {
        Some(key) => key,
        None => {
            debug!(job = %job.name, "No Git SCM URL found");
            return;
        }
    };

    let artifacts: Vec<(String, String)> = build
        .artifacts
        .iter()
        .map(|a| (a.file_name.clone(), a.relative_path.clone()))
        .collect();

    if artifacts.is_empty() {
        debug!(job = %job.name, "No artifacts");
        return;
    }

    debug!(repo = %repo_key, job = %job.name, build = build.number, "Indexed Jenkins build");

    builds.insert(
        repo_key,
        JenkinsBuildInfo {
            job_url: job.url.clone(),
            build_number: build.number,
            timestamp: build.timestamp,
            artifacts,
        },
    );
}

fn extract_repo_from_build(build: &JenkinsBuild) -> Option<String> {
    for action in &build.actions {
        if action.class != "hudson.plugins.git.util.BuildData" {
            continue;
        }
        if let Some(urls) = &action.remote_urls {
            if let Some(url) = urls.first() {
                return normalize_scm_url(url);
            }
        }
    }
    None
}

fn normalize_scm_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);

    // https://github.com/owner/repo or http://...
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(format!("{}/{}", parts[0], parts[1]).to_lowercase());
        }
    }

    // git@github.com:owner/repo
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(format!("{}/{}", parts[0], parts[1]).to_lowercase());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        JenkinsAction, JenkinsArtifact, JenkinsBuild, JenkinsBuildInfo, JenkinsJob, artifact_url,
        extract_repo_from_build, normalize_scm_url, process_job,
    };
    use std::collections::HashMap;

    #[test]
    fn normalizes_github_scm_urls() {
        assert_eq!(
            normalize_scm_url("https://github.com/LT-Name/Radio.git"),
            Some("lt-name/radio".to_string())
        );
        assert_eq!(
            normalize_scm_url("http://github.com/Owner/Repo/"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            normalize_scm_url("git@github.com:MemoriesOfTime/EconomyAPI.git"),
            Some("memoriesoftime/economyapi".to_string())
        );
        assert_eq!(normalize_scm_url("https://gitlab.com/foo/bar"), None);
        assert_eq!(normalize_scm_url("https://github.com/only-owner"), None);
    }

    #[test]
    fn artifact_url_handles_trailing_slash() {
        assert_eq!(
            artifact_url(
                "https://motci.cn/job/Radio/",
                9,
                "target/Radio-1.2.0-SNAPSHOT.jar"
            ),
            "https://motci.cn/job/Radio/9/artifact/target/Radio-1.2.0-SNAPSHOT.jar"
        );
        assert_eq!(
            artifact_url(
                "https://motci.cn/job/EconomyAPI",
                23,
                "target/EconomyAPI.jar"
            ),
            "https://motci.cn/job/EconomyAPI/23/artifact/target/EconomyAPI.jar"
        );
    }

    fn build_with(remote_url: &str, artifacts: &[&str]) -> JenkinsBuild {
        JenkinsBuild {
            number: 5,
            timestamp: 1_000,
            artifacts: artifacts
                .iter()
                .map(|a| JenkinsArtifact {
                    file_name: (*a).to_string(),
                    relative_path: format!("target/{a}"),
                })
                .collect(),
            actions: vec![JenkinsAction {
                class: "hudson.plugins.git.util.BuildData".to_string(),
                remote_urls: Some(vec![remote_url.to_string()]),
            }],
        }
    }

    #[test]
    fn extracts_repo_from_build_data() {
        let build = build_with("https://github.com/LT-Name/Radio.git", &["Radio.jar"]);
        assert_eq!(
            extract_repo_from_build(&build),
            Some("lt-name/radio".to_string())
        );
    }

    #[test]
    fn folder_job_prefers_master_sub_job() {
        let mut master = JenkinsJob {
            name: "master".to_string(),
            url: "https://motci.cn/job/Foo/master/".to_string(),
            last_successful_build: Some(build_with(
                "https://github.com/foo/bar.git",
                &["Bar-1.0.jar"],
            )),
            jobs: None,
        };
        master.last_successful_build.as_mut().unwrap().number = 42;

        let dev = JenkinsJob {
            name: "dev".to_string(),
            url: "https://motci.cn/job/Foo/dev/".to_string(),
            last_successful_build: Some(build_with(
                "https://github.com/foo/bar.git",
                &["Bar-2.0.jar"],
            )),
            jobs: None,
        };

        let folder = JenkinsJob {
            name: "Foo".to_string(),
            url: "https://motci.cn/job/Foo/".to_string(),
            last_successful_build: None,
            jobs: Some(vec![dev, master]),
        };

        let mut builds = HashMap::<String, JenkinsBuildInfo>::new();
        process_job(&folder, &mut builds);

        let info = builds.get("foo/bar").expect("indexed");
        assert_eq!(info.build_number, 42);
    }

    #[test]
    fn skips_jobs_without_artifacts_or_scm() {
        let no_artifacts = JenkinsJob {
            name: "Empty".to_string(),
            url: "https://motci.cn/job/Empty/".to_string(),
            last_successful_build: Some(build_with("https://github.com/a/b.git", &[])),
            jobs: None,
        };
        let no_scm = JenkinsJob {
            name: "NoScm".to_string(),
            url: "https://motci.cn/job/NoScm/".to_string(),
            last_successful_build: Some(JenkinsBuild {
                number: 1,
                timestamp: 0,
                artifacts: vec![JenkinsArtifact {
                    file_name: "x.jar".to_string(),
                    relative_path: "x.jar".to_string(),
                }],
                actions: Vec::new(),
            }),
            jobs: None,
        };

        let mut builds = HashMap::<String, JenkinsBuildInfo>::new();
        process_job(&no_artifacts, &mut builds);
        process_job(&no_scm, &mut builds);
        assert!(builds.is_empty());
    }
}
