//! Where releases and their downloads come from: GitHub, or in debug builds the `SKYELAB_UPDATE_FAKE*` stand-ins.

use super::{Download, Fetch};
use skyelab_client::release::Release;
use skyelab_core::update::{self, Asset, LATEST_RELEASE_URL};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The real fetch, or in debug builds `SKYELAB_UPDATE_FAKE=<tag>`'s release (no network), listing the files in
/// `SKYELAB_UPDATE_FAKE_DIR` (`fake_dir`) as its assets. `SKYELAB_UPDATE_FAKE=none` answers like a repo with no release.
pub fn fetcher(fake: Option<String>, fake_dir: Option<PathBuf>, debug: bool) -> Fetch {
    match fake.filter(|_| debug) {
        Some(none) if none == "none" => Arc::new(|| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "api.github.com answered 404",
            ))
        }),
        Some(tag) => Arc::new(move || {
            let names = fake_dir
                .as_deref()
                .and_then(|d| std::fs::read_dir(d).ok())
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok()?.file_name().into_string().ok());
            Ok(Release {
                url: format!("{}tag/{tag}", update::RELEASES_PAGE),
                assets: names
                    .map(|name| Asset {
                        url: format!("{}download/{tag}/{name}", update::RELEASES_PAGE),
                        name,
                    })
                    .collect(),
                tag: tag.clone(),
            })
        }),
        None => Arc::new(|| skyelab_client::release::latest_release(LATEST_RELEASE_URL)),
    }
}

/// The real download, or in debug builds with `SKYELAB_UPDATE_FAKE_DIR` the file of that name from the folder,
/// slowed so the progress shows.
pub fn downloader(fake_dir: Option<PathBuf>, debug: bool) -> Download {
    match fake_dir.filter(|_| debug) {
        Some(dir) => Arc::new(move |url, out, progress| {
            let name = url.rsplit('/').next().unwrap_or_default();
            let bytes = std::fs::read(dir.join(name))?;
            let total = bytes.len() as u64;
            let mut done = 0;
            for chunk in bytes.chunks(bytes.len().div_ceil(40).max(1)) {
                out.write_all(chunk)?;
                done += chunk.len() as u64;
                progress(done, Some(total));
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        }),
        None => {
            Arc::new(|url, out, progress| skyelab_client::release::download(url, out, progress))
        }
    }
}
