//! Auto-update checks for Xavier releases.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/iberi22/xavier/releases/latest";

#[derive(Debug, Clone)]
pub struct AutoUpdateService {
    client: reqwest::Client,
    current_version: String,
    latest_release_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate {
        current_version: String,
        latest_version: String,
    },
    UpdateAvailable {
        current_version: String,
        latest_version: String,
        release_url: String,
    },
    CurrentAhead {
        current_version: String,
        latest_version: String,
    },
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

impl AutoUpdateService {
    /// New.
    pub fn new() -> Self {
        Self::with_current_version(env!("CARGO_PKG_VERSION"))
    }

    /// With current version.
    pub fn with_current_version(version: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("xavier-mesh/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            current_version: version.into(),
            latest_release_url: GITHUB_LATEST_RELEASE_URL.to_string(),
        }
    }

    /// With latest release url.
    pub fn with_latest_release_url(mut self, url: impl Into<String>) -> Self {
        self.latest_release_url = url.into();
        self
    }

    /// Check for updates.
    pub async fn check_for_updates(&self) -> Result<UpdateStatus> {
        let release: GitHubRelease = self
            .client
            .get(&self.latest_release_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .context("Failed to query GitHub releases")?
            .error_for_status()
            .context("GitHub release check returned an error")?
            .json()
            .await
            .context("Failed to parse GitHub release response")?;

        Ok(self.compare_versions_with_url(&release.tag_name, release.html_url))
    }

    /// Compare versions.
    pub fn compare_versions(&self, latest_version: &str) -> UpdateStatus {
        self.compare_versions_with_url(latest_version, String::new())
    }

    fn compare_versions_with_url(&self, latest_version: &str, release_url: String) -> UpdateStatus {
        let current_normalized = normalize_version(&self.current_version);
        let latest_normalized = normalize_version(latest_version);

        match compare_version_parts(&current_normalized, &latest_normalized) {
            std::cmp::Ordering::Less => UpdateStatus::UpdateAvailable {
                current_version: self.current_version.clone(),
                latest_version: latest_version.to_string(),
                release_url,
            },
            std::cmp::Ordering::Equal => UpdateStatus::UpToDate {
                current_version: self.current_version.clone(),
                latest_version: latest_version.to_string(),
            },
            std::cmp::Ordering::Greater => UpdateStatus::CurrentAhead {
                current_version: self.current_version.clone(),
                latest_version: latest_version.to_string(),
            },
        }
    }
}

impl Default for AutoUpdateService {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_version(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(|c: char| !(c.is_ascii_digit()))
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn compare_version_parts(current: &[u64], latest: &[u64]) -> std::cmp::Ordering {
    for index in 0..current.len().max(latest.len()) {
        match current
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&latest.get(index).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    std::cmp::Ordering::Equal
}
