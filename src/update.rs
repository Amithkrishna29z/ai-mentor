//! In-app updates: check GitHub Releases for a newer build and install it in
//! place, replacing the running executable.

use anyhow::{anyhow, Result};
use std::sync::mpsc::Sender;

use crate::mentor::JobResult;

pub const REPO_OWNER: &str = "Amithkrishna29z";
pub const REPO_NAME: &str = "ai-mentor";
const BIN_NAME: &str = "ai-mentor";

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The platform slug used in release asset names, matching the packaging step
/// in `.github/workflows/release.yml`.
pub fn asset_target() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub name: String,
    pub notes: String,
    pub date: String,
}

/// Look for a release newer than the running build. `Ok(None)` means current.
pub fn check() -> Result<Option<ReleaseInfo>> {
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| anyhow!("{e}"))?
        .fetch()
        .map_err(|e| anyhow!("{e}"))?;

    let latest = releases
        .into_vec()
        .into_iter()
        // Only releases carrying a build for this platform can be installed.
        .find(|r| r.has_target_asset(asset_target()));

    let Some(latest) = latest else {
        return Ok(None);
    };

    let newer = self_update::version::bump_is_greater(current_version(), latest.version())
        .map_err(|e| anyhow!("{e}"))?;
    if !newer {
        return Ok(None);
    }

    Ok(Some(ReleaseInfo {
        version: latest.version().to_string(),
        name: latest.name().to_string(),
        notes: latest.body().unwrap_or_default().to_string(),
        date: latest.date().to_string(),
    }))
}

/// Download the newest build and replace the running executable. The new
/// version takes effect on the next launch. Returns the installed version.
pub fn install() -> Result<String> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .target(asset_target())
        .current_version(current_version())
        .show_download_progress(false)
        .show_output(false)
        .no_confirm(true)
        .build()
        .map_err(|e| anyhow!("{e}"))?
        .update()
        .map_err(|e| anyhow!("{e}"))?;

    Ok(status.version().to_string())
}

pub fn spawn_check(tx: Sender<JobResult>) {
    std::thread::spawn(move || {
        let outcome = check().map_err(|e| e.to_string());
        let _ = tx.send(JobResult::UpdateCheck { outcome });
    });
}

pub fn spawn_install(tx: Sender<JobResult>) {
    std::thread::spawn(move || {
        let outcome = install().map_err(|e| e.to_string());
        let _ = tx.send(JobResult::UpdateInstall { outcome });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_target_matches_the_release_workflow_slugs() {
        let target = asset_target();
        assert!(target == "windows-x86_64" || target == "linux-x86_64");
    }

    #[test]
    fn current_version_is_the_crate_version() {
        assert_eq!(current_version(), env!("CARGO_PKG_VERSION"));
        assert!(self_update::version::bump_is_greater(current_version(), "99.0.0").unwrap());
        assert!(!self_update::version::bump_is_greater(current_version(), current_version()).unwrap());
    }

    /// Hits the real GitHub API. Ignored by default:
    /// `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn published_release_carries_an_asset_for_this_platform() {
        let releases = self_update::backends::github::ReleaseList::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .build()
            .expect("builder")
            .fetch()
            .expect("fetched releases");

        let all = releases.into_vec();
        assert!(!all.is_empty(), "the repo has at least one release");
        let latest = &all[0];
        println!(
            "latest {} ({}), assets: {:?}",
            latest.version(),
            latest.date(),
            latest.assets().iter().map(|a| a.name()).collect::<Vec<_>>()
        );
        assert!(
            latest.has_target_asset(asset_target()),
            "latest release has an asset for {}",
            asset_target()
        );
    }
}

/// Downloads the real published asset and extracts it to a temp dir, proving
/// the whole update path short of the final self-replace. Ignored by default.
#[cfg(test)]
mod download_integration {
    use super::*;

    #[test]
    #[ignore]
    fn latest_asset_downloads_and_contains_the_binary() {
        let releases = self_update::backends::github::ReleaseList::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .build()
            .expect("builder")
            .fetch()
            .expect("releases");

        let latest = releases.into_vec().into_iter().next().expect("a release");
        let asset = latest
            .asset_for(asset_target(), None)
            .expect("an asset for this platform");
        println!("downloading {}", asset.name());

        let tmp = tempfile::TempDir::new().expect("temp dir");
        let archive = tmp.path().join(asset.name());
        let file = std::fs::File::create(&archive).expect("create archive");
        // GitHub's asset URL serves release JSON unless the request asks for
        // the binary; the real update path sets this header internally.
        self_update::Download::from_url(asset.download_url())
            .request_header(
                self_update::http_client::header::ACCEPT,
                "application/octet-stream",
            )
            .download_to(&file)
            .expect("download");

        let size = std::fs::metadata(&archive).expect("metadata").len();
        println!("{} bytes", size);
        assert!(size > 1_000_000, "archive looks too small: {size} bytes");

        let bin = if cfg!(windows) { "ai-mentor.exe" } else { "ai-mentor" };
        self_update::Extract::from_source(&archive)
            .extract_file(tmp.path(), bin)
            .expect("extract the binary");
        assert!(tmp.path().join(bin).exists(), "{bin} is in the archive");
    }

}
