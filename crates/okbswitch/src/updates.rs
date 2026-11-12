//! «Проверять обновления»: asks the GitHub API for the project's releases and
//! reports a newer one. Only the public release list is requested; the
//! program sends no data about the user or the computer. The time of the
//! last successful check is kept next to `config.toml`.

use anyhow::{Context, Result};
use okbs_core::config::UpdateCheck;
use okbs_core::version::Version;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RELEASES: &str =
    "https://api.github.com/repos/coinman-dev/own-keyboard-switch/releases?per_page=20";
/// A failed check is retried after this time, not at the next interval.
pub const RETRY: Duration = Duration::from_secs(6 * 60 * 60);
const STATE_FILE: &str = "update-check.txt";

/// A release newer than the running program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without the `v` of the tag, e.g. `0.7.0-beta`.
    pub version: String,
    /// The release page on GitHub.
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The newest published release above `current`. Pre-releases count only
/// while the running version is a pre-release itself.
fn newest(releases: &[ApiRelease], current: &Version) -> Option<Release> {
    releases
        .iter()
        .filter(|release| !release.draft)
        .filter(|release| current.is_prerelease() || !release.prerelease)
        .filter(|release| release.html_url.starts_with("https://github.com/"))
        .filter_map(|release| {
            let version = Version::parse(&release.tag_name)?;
            (version > *current).then_some((version, release))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, release)| Release {
            version: release.tag_name.trim_start_matches('v').to_owned(),
            url: release.html_url.clone(),
        })
}

/// Downloads the release list and returns a newer release, if any.
pub fn check() -> Result<Option<Release>> {
    let current = Version::parse(okbs_core::VERSION).context("invalid program version")?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    let mut response = agent
        .get(RELEASES)
        .header("Accept", "application/vnd.github+json")
        .header(
            "User-Agent",
            concat!("okbswitch/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .context("cannot read the release list")?;
    let body = response
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_to_string()
        .context("cannot read the release list")?;
    let releases: Vec<ApiRelease> =
        serde_json::from_str(&body).context("unexpected release list")?;
    Ok(newest(&releases, &current))
}

/// Where the time of the last successful check is kept.
pub fn state_path(config: &Path) -> PathBuf {
    config
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(STATE_FILE)
}

/// The last successful check, if it was recorded.
pub fn last_check(path: &Path) -> Option<SystemTime> {
    let seconds: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

/// Records a successful check.
pub fn record_check(path: &Path, at: SystemTime) -> Result<()> {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    std::fs::write(path, format!("{seconds}\n"))
        .with_context(|| format!("cannot write {}", path.display()))
}

/// Whether a scheduled check is due now. A clock moved back counts as due.
pub fn due(setting: UpdateCheck, last: Option<SystemTime>, now: SystemTime) -> bool {
    let Some(interval) = setting.interval() else {
        return false;
    };
    last.is_none_or(|last| {
        now.duration_since(last)
            .map_or(true, |elapsed| elapsed >= interval)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, draft: bool) -> ApiRelease {
        ApiRelease {
            tag_name: tag.into(),
            html_url: format!(
                "https://github.com/coinman-dev/own-keyboard-switch/releases/tag/{tag}"
            ),
            draft,
            prerelease,
        }
    }

    #[test]
    fn newest_release_respects_drafts_and_prereleases() {
        let list = [
            release("v0.9.0-beta", true, true),
            release("v0.8.0-beta", true, false),
            release("v0.7.0", false, false),
            release("v0.6.0-beta", true, false),
            release("not-a-version", false, false),
        ];
        let beta = Version::parse("0.6.0-beta").unwrap();
        assert_eq!(
            newest(&list, &beta).map(|r| r.version),
            Some("0.8.0-beta".into()),
            "a beta user gets the newest published pre-release, never a draft"
        );
        let stable = Version::parse("0.6.0").unwrap();
        assert_eq!(
            newest(&list, &stable).map(|r| r.version),
            Some("0.7.0".into()),
            "a stable user gets stable releases only"
        );
        assert_eq!(newest(&list, &Version::parse("0.8.0").unwrap()), None);
        let foreign = [ApiRelease {
            html_url: "https://example.com/x".into(),
            ..release("v9.0.0", false, false)
        }];
        assert_eq!(newest(&foreign, &stable), None, "only GitHub release pages");
    }

    #[test]
    fn real_api_shape_parses() {
        let body = r#"[{"tag_name":"v0.6.0-beta","html_url":"https://github.com/coinman-dev/own-keyboard-switch/releases/tag/v0.6.0-beta","draft":false,"prerelease":true,"assets":[]}]"#;
        let list: Vec<ApiRelease> = serde_json::from_str(body).unwrap();
        assert_eq!(list[0].tag_name, "v0.6.0-beta");
        assert!(list[0].prerelease);
    }

    #[test]
    fn checks_are_due_by_interval_and_the_time_is_kept() {
        let now = SystemTime::now();
        let day = Duration::from_secs(24 * 60 * 60);
        assert!(due(UpdateCheck::Weekly, None, now));
        assert!(!due(UpdateCheck::Weekly, Some(now - day), now));
        assert!(due(UpdateCheck::Weekly, Some(now - 7 * day), now));
        assert!(due(UpdateCheck::Daily, Some(now - day), now));
        assert!(!due(UpdateCheck::Never, None, now));
        assert!(
            due(UpdateCheck::Monthly, Some(now + day), now),
            "clock moved back"
        );
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory.path().join("config.toml"));
        assert_eq!(last_check(&path), None);
        record_check(&path, now).unwrap();
        let read = last_check(&path).unwrap();
        assert!(now.duration_since(read).unwrap() < Duration::from_secs(1));
    }

    #[test]
    #[ignore = "network: reads the real GitHub release list"]
    fn real_release_list_is_read() {
        check().unwrap();
    }
}
