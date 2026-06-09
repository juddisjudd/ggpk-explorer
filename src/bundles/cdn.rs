use std::fs;
use std::path::{Path, PathBuf};
use std::io::Write;
use reqwest::blocking::Client;
use std::error::Error;

#[derive(Clone)]
pub struct CdnBundleLoader {
    cache_dir: PathBuf,
    client: Client,
    patch_ver: String,
}

impl CdnBundleLoader {
    pub fn new(cache_root: &Path, patch_ver: Option<&str>) -> Self {
        let cache_dir = cache_root.join("Bundles2");
        if !cache_dir.exists() {
            let _ = fs::create_dir_all(&cache_dir);
        }
        CdnBundleLoader {
            cache_dir,
            client: Client::new(),
            patch_ver: patch_ver.unwrap_or("4.5.1.1.4").to_string(),
        }
    }

    pub fn patch_version(&self) -> &str {
        &self.patch_ver
    }

    pub fn fetch_bundle(&self, bundle_name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        // Reject bundle names that are clearly index-internal paths, not real CDN bundles.
        // The index uses "Folders/" prefix for directory-structure metadata bundles that
        // don't exist as individual files on the CDN.
        if bundle_name.starts_with("Folders/") || bundle_name.starts_with("folders/") {
            return Err(format!(
                "Bundle name '{}' starts with 'Folders/' which is an index-internal path, not a valid CDN bundle",
                bundle_name
            ).into());
        }

        // 1. Check Local Cache
        let safe_name = bundle_name.replace("/", "@");
        let cache_path = self.cache_dir.join(&safe_name);

        if cache_path.exists() {
            let data = fs::read(&cache_path)?;
            return Ok(data);
        }

        // 2. Download from CDN
        let url = if self.patch_ver.starts_with("4.") {
             format!("https://patch-poe2.poecdn.com/{}/Bundles2/{}", self.patch_ver, bundle_name)
        } else {
             format!("https://patch.poecdn.com/{}/Bundles2/{}", self.patch_ver, bundle_name)
        };

        println!("[CDN] Downloading: {}", url);
        let resp = self.client.get(&url).send()?;

        let status = resp.status();
        if !status.is_success() {
            if status.as_u16() == 404 {
                return Err(format!(
                    "CDN 404 Not Found: {} — patch version '{}' may be incorrect or the bundle doesn't exist on CDN",
                    url, self.patch_ver
                ).into());
            }
            return Err(format!(
                "CDN Request Failed: {} ({}) — using patch version '{}'",
                url, status, self.patch_ver
            ).into());
        }

        let bytes = resp.bytes()?;
        let data = bytes.to_vec();

        // 3. Save to Cache
        let mut f = fs::File::create(&cache_path)?;
        f.write_all(&data)?;
        
        Ok(data)
    }

    pub fn set_patch_version(&mut self, ver: &str) {
        self.patch_ver = ver.to_string();
    }
}
