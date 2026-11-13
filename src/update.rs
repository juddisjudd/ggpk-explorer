use std::sync::mpsc::{channel, Receiver};
use std::thread;
use serde::Deserialize;
use semver::Version;

const REPO: &str = "juddisjudd/ggpk-explorer";

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

/// Only a copy the installer set up can update itself; any other copy links to the release page.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    Unavailable,
    Available,
    Downloading(u8),
    Ready,
    Failed(String),
}

struct Found {
    tag: String,
    url: String,
    #[cfg(windows)]
    update: Option<installed::Update>,
}

pub struct UpdateState {
    pub pending: bool,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub error_msg: Option<String>,
    install: Install,
    receiver: Receiver<Result<Option<Found>, String>>,
    #[cfg(windows)]
    update: Option<std::sync::Arc<installed::Update>>,
    #[cfg(windows)]
    download: Option<Download>,
}

#[cfg(windows)]
struct Download {
    progress: Receiver<i16>,
    done: Receiver<Result<(), String>>,
}

impl UpdateState {
    pub fn new() -> Self {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = check_update_impl();
            let _ = tx.send(result);
        });

        Self {
            pending: true,
            latest_version: None,
            release_url: None,
            error_msg: None,
            install: Install::Unavailable,
            receiver: rx,
            #[cfg(windows)]
            update: None,
            #[cfg(windows)]
            download: None,
        }
    }

    pub fn install(&self) -> &Install {
        &self.install
    }

    pub fn poll(&mut self) {
        if self.pending {
            if let Ok(result) = self.receiver.try_recv() {
                self.pending = false;
                match result {
                    Ok(Some(found)) => {
                        #[cfg(windows)]
                        if let Some(update) = found.update {
                            self.update = Some(std::sync::Arc::new(update));
                            self.install = Install::Available;
                        }
                        self.latest_version = Some(found.tag);
                        self.release_url = Some(found.url);
                    },
                    Ok(None) => {
                        // No update available
                    },
                    Err(e) => {
                        self.error_msg = Some(e);
                    }
                }
            }
        }
        #[cfg(windows)]
        self.poll_download();
    }

    /// Downloads the update in the background; `poll` follows its progress.
    pub fn start_install(&mut self) {
        #[cfg(windows)]
        if let Some(update) = self.update.clone() {
            let (progress_tx, progress) = channel();
            let (done_tx, done) = channel();
            thread::spawn(move || {
                let _ = done_tx.send(update.download(progress_tx));
            });
            self.download = Some(Download { progress, done });
            self.install = Install::Downloading(0);
        }
    }

    /// True when the installer is waiting for this process to exit to apply the update and restart.
    pub fn apply_on_exit(&mut self) -> bool {
        #[cfg(windows)]
        if let Some(update) = &self.update {
            match update.apply_after_exit() {
                Ok(()) => return true,
                Err(e) => self.install = Install::Failed(e),
            }
        }
        false
    }

    #[cfg(windows)]
    fn poll_download(&mut self) {
        let Some(download) = &self.download else { return };
        if let Some(percent) = download.progress.try_iter().last() {
            self.install = Install::Downloading(percent.clamp(0, 100) as u8);
        }
        if let Ok(result) = download.done.try_recv() {
            self.install = match result {
                Ok(()) => Install::Ready,
                Err(e) => Install::Failed(e),
            };
            self.download = None;
        }
    }
}

fn check_update_impl() -> Result<Option<Found>, String> {
    #[cfg(windows)]
    if let Some(result) = installed::check() {
        return result.map(|update| {
            update.map(|update| {
                let tag = format!("v{}", update.version());
                Found { url: format!("https://github.com/{}/releases/tag/{}", REPO, tag), tag, update: Some(update) }
            })
        });
    }

    let current_version_str = env!("CARGO_PKG_VERSION");
    let current_version = Version::parse(current_version_str).map_err(|e| format!("Failed to parse local version: {}", e))?;

    let client = reqwest::blocking::Client::builder()
        .user_agent("ggpk-explorer-update-checker")
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    let resp = client.get(format!("https://api.github.com/repos/{}/releases/latest", REPO))
        .send()
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API Error: {}", resp.status()));
    }

    let release: GitHubRelease = resp.json().map_err(|e| format!("Failed to parse JSON: {}", e))?;
    let tag_name = release.tag_name.trim_start_matches('v');

    let latest_version = Version::parse(tag_name).map_err(|e| format!("Failed to parse remote tag '{}': {}", release.tag_name, e))?;

    if latest_version > current_version {
        return Ok(Some(Found {
            tag: release.tag_name,
            url: release.html_url,
            #[cfg(windows)]
            update: None,
        }));
    }

    Ok(None)
}

/// Updates for a copy the Windows installer set up, from the feed each release carries.
#[cfg(windows)]
mod installed {
    use std::sync::mpsc::Sender;
    use velopack::{sources::AutoSource, UpdateCheck, UpdateInfo, UpdateManager};

    pub struct Update {
        manager: UpdateManager,
        info: UpdateInfo,
    }

    /// None when this copy was not set up by the installer.
    pub fn check() -> Option<Result<Option<Update>, String>> {
        // A local folder or URL of packed releases, to try an update before it is published.
        let feed = std::env::var("GGPK_EXPLORER_UPDATE_FEED").unwrap_or_else(|_| format!("https://github.com/{}", super::REPO));
        let manager = UpdateManager::new(AutoSource::new(&feed), None, None).ok()?;
        Some(match manager.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(info)) => Ok(Some(Update { manager, info: *info })),
            Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => Ok(None),
            Err(e) => Err(format!("Update check failed: {}", e)),
        })
    }

    impl Update {
        pub fn version(&self) -> String {
            self.info.TargetFullRelease.Version.clone()
        }

        pub fn download(&self, progress: Sender<i16>) -> Result<(), String> {
            self.manager.download_updates(&self.info, Some(progress)).map_err(|e| format!("Download failed: {}", e))
        }

        pub fn apply_after_exit(&self) -> Result<(), String> {
            self.manager
                .wait_exit_then_apply_updates(&self.info, false, true, Vec::<String>::new())
                .map_err(|e| format!("Could not start the installer: {}", e))
        }
    }
}
