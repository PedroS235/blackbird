//! Is there a newer Blackbird than this one. One unauthenticated GET against
//! GitHub's releases API at startup, resolved to the asset that `release.yml`
//! built for the platform this binary is actually running on.
//!
//! It only ever *offers*: the release assets are bare, unsigned binaries, so a
//! self-replacing updater would have to defeat Gatekeeper on macOS, rename a
//! running `.exe` on Windows, and guess whether a Linux install came from a
//! package manager. Opening the download is the pilot's call.

use std::time::Duration;

use semver::Version;
use serde::Deserialize;

/// What `cargo build` stamped in. `release.yml` refuses to publish a tag that
/// disagrees with it, so a binary can always name its own version honestly.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/PedroS235/blackbird/releases/latest";

/// GitHub 403s a request that arrives without one.
const USER_AGENT: &str = concat!("blackbird/", env!("CARGO_PKG_VERSION"));

const TIMEOUT: Duration = Duration::from_secs(5);

/// A release body carries a whole changelog, and nothing on our side bounds it.
const BODY_LIMIT: u64 = 1 << 20;

/// A release newer than this build, and where to get it.
pub struct UpdateInfo {
    pub current: Version,
    pub latest: Version,
    /// The binary built for this platform — or the release page, where none was.
    pub download_url: String,
    pub release_url: String,
}

/// Only the fields used. Everything else GitHub sends is ignored, so the shape
/// of the rest of their payload is not something this has to track.
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// The check. `None` for every failure — offline, rate-limited, a renamed repo,
/// a tag that is not a version, a release older than this build. The pilot
/// opened the app to read a log, not to hear that GitHub is down, so nothing
/// reaches the UI but a genuine newer release.
pub fn check_for_update() -> Option<UpdateInfo> {
    let current = Version::parse(CURRENT).ok()?;
    let release = fetch_latest()
        .inspect_err(|err| tracing::debug!("update check failed: {err}"))
        .ok()?;
    let latest = newer(&current, &release.tag_name)?;

    let download_url = asset_name(
        &release.tag_name,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .and_then(|name| release.assets.iter().find(|asset| asset.name == name))
    .map(|asset| asset.browser_download_url.clone())
    .unwrap_or_else(|| release.html_url.clone());

    tracing::info!("Blackbird {latest} is available (running {current})");
    Some(UpdateInfo {
        current,
        latest,
        download_url,
        release_url: release.html_url,
    })
}

fn fetch_latest() -> Result<Release, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();

    agent
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", USER_AGENT)
        .call()?
        .body_mut()
        .with_config()
        .limit(BODY_LIMIT)
        .read_json()
}

/// Strictly newer, and silent about anything else: a tag that will not parse,
/// and a working build already ahead of the last release, are both `None`.
fn newer(current: &Version, tag_name: &str) -> Option<Version> {
    let latest = Version::parse(tag_name.strip_prefix('v').unwrap_or(tag_name)).ok()?;
    (latest > *current).then_some(latest)
}

/// The asset `release.yml` publishes for this platform, e.g.
/// `blackbird-v0.7.0-linux-arm64`. Note the names say `arm64` where Rust says
/// `aarch64` — this is a mapping, not a passthrough.
///
/// `os` and `arch` are parameters rather than `env::consts` reads so every
/// target's name is testable from one machine. `None` where nothing is built:
/// 32-bit, BSD, anything outside the release matrix — the caller then falls
/// back to the release page.
///
/// An x86_64 build running under Rosetta on an arm64 Mac reports `x86_64` and
/// is offered the x86_64 asset again. That build works, so the answer is not
/// wrong, only not the best one available.
fn asset_name(tag: &str, os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let (os, ext) = match os {
        "linux" => ("linux", ""),
        "macos" => ("macos", ""),
        "windows" => ("windows", ".exe"),
        _ => return None,
    };
    Some(format!("blackbird-{tag}-{os}-{arch}{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn asset_names_cover_the_release_matrix() {
        let name = |os, arch| asset_name("v0.7.0", os, arch);
        assert_eq!(
            name("linux", "x86_64").as_deref(),
            Some("blackbird-v0.7.0-linux-x86_64")
        );
        assert_eq!(
            name("linux", "aarch64").as_deref(),
            Some("blackbird-v0.7.0-linux-arm64")
        );
        assert_eq!(
            name("macos", "x86_64").as_deref(),
            Some("blackbird-v0.7.0-macos-x86_64")
        );
        assert_eq!(
            name("macos", "aarch64").as_deref(),
            Some("blackbird-v0.7.0-macos-arm64")
        );
        assert_eq!(
            name("windows", "x86_64").as_deref(),
            Some("blackbird-v0.7.0-windows-x86_64.exe")
        );
        assert_eq!(
            name("windows", "aarch64").as_deref(),
            Some("blackbird-v0.7.0-windows-arm64.exe")
        );
    }

    #[test]
    fn unbuilt_platforms_have_no_asset() {
        assert_eq!(asset_name("v0.7.0", "freebsd", "x86_64"), None);
        assert_eq!(asset_name("v0.7.0", "linux", "x86"), None);
    }

    #[test]
    fn only_a_strictly_newer_tag_is_an_update() {
        assert_eq!(newer(&v("0.7.0"), "v0.8.0"), Some(v("0.8.0")));
        assert_eq!(newer(&v("0.7.0"), "0.8.0"), Some(v("0.8.0")), "no v prefix");
        assert_eq!(newer(&v("0.7.0"), "v0.7.0"), None, "same release");
        assert_eq!(newer(&v("0.7.0"), "v0.6.0"), None, "build ahead of release");
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_silence() {
        assert_eq!(newer(&v("0.7.0"), "nightly"), None);
        assert_eq!(newer(&v("0.7.0"), "v1.2"), None);
    }

    /// The only test that touches the network — headers, TLS and the payload
    /// shape are not things the pure functions can vouch for. Ignored by
    /// default so `cargo test` stays offline.
    #[test]
    #[ignore = "hits api.github.com"]
    fn github_answers_with_a_tag_that_parses() {
        let release = fetch_latest().expect("the releases endpoint answers");
        assert!(newer(&v("0.0.1"), &release.tag_name).is_some());
        assert!(!release.assets.is_empty(), "a release ships binaries");
    }

    #[test]
    fn a_prerelease_sorts_below_the_release_it_precedes() {
        assert_eq!(newer(&v("0.8.0-rc.1"), "v0.8.0"), Some(v("0.8.0")));
        assert_eq!(newer(&v("0.8.0"), "v0.8.0-rc.1"), None);
    }
}
