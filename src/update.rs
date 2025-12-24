use std::sync::mpsc::{channel, Receiver};
use std::thread;
use serde::Deserialize;
use semver::Version;

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

pub struct UpdateState {
    pub pending: bool,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    receiver: Receiver<Option<(String, String)>>,
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
            receiver: rx,
        }
    }

    pub fn poll(&mut self) {
        if self.pending {
            if let Ok(result) = self.receiver.try_recv() {
                self.pending = false;
                if let Some((ver, url)) = result {
                    self.latest_version = Some(ver);
                    self.release_url = Some(url);
                }
            }
        }
    }
}

fn check_update_impl() -> Option<(String, String)> {
    let current_version_str = env!("CARGO_PKG_VERSION");
    let current_version = Version::parse(current_version_str).ok()?;

    let client = reqwest::blocking::Client::builder()
        .user_agent("ggpk-explorer-update-checker")
        .build()
        .ok()?;

    let resp = client.get("https://api.github.com/repos/juddisjudd/ggpk-explorer/releases/latest")
        .send()
        .ok()?;
        
    if !resp.status().is_success() {
        return None;
    }

    let release: GitHubRelease = resp.json().ok()?;
    let tag_name = release.tag_name.trim_start_matches('v');
    
    let latest_version = Version::parse(tag_name).ok()?;
    
    if latest_version > current_version {
        return Some((release.tag_name, release.html_url));
    }

    None
}
