use std::{
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{api_error::ApiError, state::AppState};

const REPOSITORY: &str = "saeta-eth/OpenKrisp";
pub(crate) const PUBLIC_KEY: &str = match option_env!("OPENKRISP_UPDATER_PUBLIC_KEY") {
    Some(key) => key,
    None => "",
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    revision: u64,
    phase: UpdatePhase,
    version: Option<String>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdatePhase {
    Disabled,
    Idle,
    Checking,
    Downloading,
    Ready,
    Installing,
    Error,
}

trait UpdateInstaller: Send + Sync {
    fn install(&self, bytes: &[u8]) -> Result<(), String>;
}
impl UpdateInstaller for Update {
    fn install(&self, bytes: &[u8]) -> Result<(), String> {
        Update::install(self, bytes).map_err(|error| error.to_string())
    }
}

struct DownloadedUpdate {
    installer: Box<dyn UpdateInstaller>,
    bytes: Vec<u8>,
}

struct Session {
    status: UpdateStatus,
    pending: Option<DownloadedUpdate>,
}

pub struct UpdateState {
    stopped: AtomicBool,
    session: Mutex<Session>,
    scheduler: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl UpdateState {
    pub fn new(enabled: bool) -> Self {
        Self {
            stopped: AtomicBool::new(false),
            session: Mutex::new(Session {
                status: UpdateStatus {
                    revision: 0,
                    phase: if enabled {
                        UpdatePhase::Idle
                    } else {
                        UpdatePhase::Disabled
                    },
                    version: None,
                    downloaded_bytes: 0,
                    total_bytes: None,
                    error: None,
                },
                pending: None,
            }),
            scheduler: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Session> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn change(&self, change: impl FnOnce(&mut Session)) -> UpdateStatus {
        let mut session = self.lock();
        if !self.stopped.load(Ordering::Acquire) {
            change(&mut session);
            session.status.revision += 1;
        }
        session.status.clone()
    }

    pub(crate) fn stop_scheduler(&self) {
        self.stopped.store(true, Ordering::Release);
        self.lock().pending = None;
        if let Some(task) = self
            .scheduler
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            task.abort();
        }
    }
}

impl Drop for UpdateState {
    fn drop(&mut self) {
        self.stop_scheduler();
    }
}

pub(crate) fn start_scheduler<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<UpdateState>();
    if state.lock().status.phase == UpdatePhase::Disabled {
        return;
    }
    let handle = app.clone();
    // Native scheduling continues when the hidden WebView throttles JavaScript timers.
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let _ = prepare_update(handle.clone(), handle.state()).await;
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
    *state
        .scheduler
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(task);
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

// GitHub's /releases/latest excludes betas. Select by SemVer, independent of the
// GitHub "Latest" flag, and never move a stable installation onto the beta channel.
fn newest_manifest(releases: &[Release], current: &Version) -> Option<(Version, String)> {
    releases
        .iter()
        .filter_map(|release| {
            let version = Version::parse(release.tag_name.strip_prefix('v')?).ok()?;
            if release.draft
                || version <= *current
                || (current.pre.is_empty() && (release.prerelease || !version.pre.is_empty()))
            {
                return None;
            }
            let asset = release
                .assets
                .iter()
                .find(|asset| asset.name == "latest.json")?;
            let expected = format!(
                "https://github.com/{REPOSITORY}/releases/download/{}/latest.json",
                release.tag_name
            );
            (asset.browser_download_url == expected).then_some((version, expected))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
}

#[tauri::command]
pub fn get_update_status(state: State<'_, UpdateState>) -> UpdateStatus {
    state.lock().status.clone()
}

#[tauri::command]
pub async fn prepare_update<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdateState>,
) -> Result<UpdateStatus, ApiError> {
    Ok(prepare_with(&state, download_update(&app, &state)).await)
}

async fn prepare_with(
    state: &UpdateState,
    download: impl Future<Output = Result<Option<DownloadedUpdate>, String>>,
) -> UpdateStatus {
    {
        let mut session = state.lock();
        if state.stopped.load(Ordering::Acquire)
            || !matches!(session.status.phase, UpdatePhase::Idle | UpdatePhase::Error)
        {
            return session.status.clone();
        }
        session.status.phase = UpdatePhase::Checking;
        session.status.error = None;
        session.status.version = None;
        session.status.downloaded_bytes = 0;
        session.status.total_bytes = None;
        session.status.revision += 1;
    }
    let result = download.await;
    state.change(|session| match result {
        Ok(Some(pending)) => {
            session.status.phase = UpdatePhase::Ready;
            session.pending = Some(pending);
        }
        Ok(None) => session.status.phase = UpdatePhase::Idle,
        Err(error) => {
            session.status.phase = UpdatePhase::Error;
            session.status.error = Some(error);
        }
    })
}

async fn download_update<R: Runtime>(
    app: &AppHandle<R>,
    state: &UpdateState,
) -> Result<Option<DownloadedUpdate>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("OpenKrisp/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())?;
    let releases: Vec<Release> = client
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/releases?per_page=100"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    let Some((version, endpoint)) = newest_manifest(&releases, &app.package_info().version) else {
        return Ok(None);
    };
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(15 * 60))
        .endpoints(vec![
            reqwest::Url::parse(&endpoint).map_err(|error| error.to_string())?,
        ])
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    if update.version != version.to_string() {
        return Err("The update manifest does not match its release version.".into());
    }
    state.change(|session| {
        session.status.phase = UpdatePhase::Downloading;
        session.status.version = Some(update.version.clone());
    });
    // The plugin verifies the signature before returning any installable bytes.
    let bytes = update
        .download(
            |chunk, total| {
                state.change(|session| {
                    session.status.downloaded_bytes += chunk as u64;
                    session.status.total_bytes = total;
                });
            },
            || {},
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(DownloadedUpdate {
        installer: Box::new(update),
        bytes,
    }))
}

#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> Result<(), ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let updates = app.state::<UpdateState>();
        install_prepared(&app.state::<AppState>(), &updates)?;
        app.restart();
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?
}

// Both IPC and tests run the same installation, retry and shutdown orchestration.
fn install_prepared(state: &AppState, updates: &UpdateState) -> Result<(), ApiError> {
    {
        let session = updates.lock();
        if session.status.phase != UpdatePhase::Ready {
            return Err(ApiError::invalid_configuration(
                "No verified update is ready to install.",
            ));
        }
    }
    state.begin_update()?;
    let pending = updates.change(|session| session.status.phase = UpdatePhase::Installing);
    let downloaded = updates.lock().pending.take();
    let Some(downloaded) = downloaded else {
        state.end_update();
        updates.change(|session| session.status.phase = UpdatePhase::Error);
        return Err(ApiError::internal("The downloaded update is unavailable."));
    };
    if let Err(error) = downloaded.installer.install(&downloaded.bytes) {
        // Keep verified bytes for retry after cancelled administrator authentication.
        updates.change(|session| {
            session.status.phase = UpdatePhase::Ready;
            session.status.error = Some(format!(
                "Could not install {}: {error}",
                pending.version.unwrap_or_default()
            ));
            session.pending = Some(downloaded);
        });
        state.end_update();
        return Err(ApiError::internal(
            "The update could not be installed. You can retry.",
        ));
    }
    state
        .shutdown_started
        .store(true, std::sync::atomic::Ordering::Release);
    updates.stop_scheduler();
    // Shutdown joins audio workers and flushes accepted settings writes even
    // when teardown reports an error. Restart only after all joins complete.
    let _ = state.shutdown();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(version: &str, prerelease: bool, draft: bool) -> Release {
        serde_json::from_value(json!({
            "tag_name": format!("v{version}"), "draft": draft, "prerelease": prerelease,
            "assets": [{ "name": "latest.json", "browser_download_url": format!("https://github.com/{REPOSITORY}/releases/download/v{version}/latest.json") }]
        })).expect("fixture")
    }

    #[test]
    fn channels_use_semver_and_ignore_drafts_old_versions_and_unrelated_assets() {
        let mut releases = vec![
            release("1.1.0-beta.10", true, false),
            release("1.1.0-beta.2", true, false),
            release("1.0.0", false, false),
            release("2.0.0", false, true),
        ];
        let stable = Version::parse("0.9.0").expect("version");
        assert_eq!(
            newest_manifest(&releases, &stable)
                .expect("stable")
                .0
                .to_string(),
            "1.0.0"
        );
        let beta = Version::parse("1.1.0-beta.1").expect("version");
        assert_eq!(
            newest_manifest(&releases, &beta)
                .expect("beta")
                .0
                .to_string(),
            "1.1.0-beta.10"
        );
        assert!(newest_manifest(&releases, &Version::parse("1.1.0").expect("version")).is_none());
        releases[0].assets[0].browser_download_url = "https://example.com/latest.json".into();
        assert_eq!(
            newest_manifest(&releases, &beta)
                .expect("beta")
                .0
                .to_string(),
            "1.1.0-beta.2"
        );
        releases[1].assets.clear();
        assert!(newest_manifest(&releases, &beta).is_none());
    }
}

#[cfg(test)]
pub(crate) fn contract_statuses() -> Vec<UpdateStatus> {
    [
        UpdatePhase::Disabled,
        UpdatePhase::Idle,
        UpdatePhase::Checking,
        UpdatePhase::Downloading,
        UpdatePhase::Ready,
        UpdatePhase::Installing,
        UpdatePhase::Error,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, phase)| UpdateStatus {
        revision: index as u64,
        phase,
        version: Some("0.2.0".into()),
        downloaded_bytes: 1024,
        total_bytes: Some(2048),
        error: (phase == UpdatePhase::Error).then(|| "Fixture signature rejected.".into()),
    })
    .collect()
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod lifecycle_tests;
