//! Local speaker diarization with speakrs (pyannote community-1 on CoreML).
//!
//! The weights (~273 MB, 68 files) are not bundled: they are downloaded once
//! into app data from the ungated mirror `avencera/speakrs-models` at a
//! pinned revision, and every file is checked against the SHA-256 recorded in
//! `speakrs_models.json`. No Hugging Face token is involved.
//!
//! speakrs offers no progress or cancellation; a run takes about 1/70 of the
//! audio length on Apple silicon.

use super::turns::SpeakerSegment;
use crate::cloud::APP_TITLE;
use futures_util::{StreamExt, TryStreamExt};
use log::info;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::io::Write;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Whether this build can diarize (speakrs is built for macOS only).
pub const SUPPORTED: bool = cfg!(target_os = "macos");

/// Emitted while weights download: [`DownloadProgress`].
pub const PROGRESS_EVENT: &str = "diarization-model-progress";

/// Parallel file downloads.
const PARALLEL_DOWNLOADS: usize = 4;

/// Attempts per file before the download fails.
const ATTEMPTS: usize = 3;

#[derive(Deserialize)]
struct Manifest {
    repo: String,
    revision: String,
    files: Vec<WeightFile>,
}

#[derive(Clone, Deserialize)]
struct WeightFile {
    path: String,
    size: u64,
    sha256: String,
}

static MANIFEST: Lazy<Manifest> = Lazy::new(|| {
    serde_json::from_str(include_str!("speakrs_models.json"))
        .expect("embedded speakrs_models.json is valid (checked by a unit test)")
});

/// One download at a time, whoever starts it (tray or settings).
static DOWNLOAD_LOCK: Mutex<()> = Mutex::new(());
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

pub enum DownloadError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Cancelled => write!(f, "download cancelled"),
            DownloadError::Failed(reason) => write!(f, "{reason}"),
        }
    }
}

fn failed(e: impl std::fmt::Display) -> DownloadError {
    DownloadError::Failed(e.to_string())
}

/// Total size of the weights in bytes.
pub fn total_size() -> u64 {
    MANIFEST.files.iter().map(|f| f.size).sum()
}

/// Where the weights live: one folder per pinned revision.
pub fn models_dir(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let revision = &MANIFEST.revision[..8.min(MANIFEST.revision.len())];
    Ok(crate::portable::app_data_dir(app)
        .map_err(|e| anyhow::anyhow!("failed to get app data dir: {e}"))?
        .join("models")
        .join(format!("speakrs-{revision}")))
}

fn is_complete(dir: &Path, file: &WeightFile) -> bool {
    // Contents were verified when the file was downloaded; the size catches
    // truncation and replacement by another version.
    dir.join(&file.path)
        .metadata()
        .is_ok_and(|m| m.len() == file.size)
}

/// Whether every weight file is in place.
pub fn models_ready(dir: &Path) -> bool {
    MANIFEST.files.iter().all(|f| is_complete(dir, f))
}

/// Whether a download is running.
pub fn is_downloading() -> bool {
    DOWNLOADING.load(Ordering::SeqCst)
}

/// Removes the downloaded weights.
pub fn delete_models(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Clears [`DOWNLOADING`] however the download ends.
struct DownloadingFlag;

impl Drop for DownloadingFlag {
    fn drop(&mut self) {
        DOWNLOADING.store(false, Ordering::SeqCst);
    }
}

/// Downloads the missing weight files into `dir`, verifying each one.
/// Blocks; `progress` gets the fraction done every 200 ms and may cancel.
/// Progress is also emitted as [`PROGRESS_EVENT`] for the settings page.
pub fn download_models(
    app: &AppHandle,
    dir: &Path,
    progress: &mut dyn FnMut(f32) -> ControlFlow<()>,
) -> Result<(), DownloadError> {
    // A download started elsewhere finishes first; then little is missing.
    let _lock = DOWNLOAD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    DOWNLOADING.store(true, Ordering::SeqCst);
    let _flag = DownloadingFlag;

    let missing: Vec<WeightFile> = MANIFEST
        .files
        .iter()
        .filter(|f| !is_complete(dir, f))
        .cloned()
        .collect();
    let total = total_size();
    let downloaded = Arc::new(AtomicU64::new(
        total - missing.iter().map(|f| f.size).sum::<u64>(),
    ));
    if missing.is_empty() {
        return Ok(());
    }
    info!(
        "Downloading {} diarization weight file(s) into {}",
        missing.len(),
        dir.display()
    );

    let cancel = Arc::new(AtomicBool::new(false));
    let worker = {
        let dir = dir.to_path_buf();
        let downloaded = Arc::clone(&downloaded);
        let cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(failed)?
                .block_on(fetch_all(dir, missing, downloaded, cancel))
        })
    };

    let report = |done: u64| {
        let _ = app.emit(
            PROGRESS_EVENT,
            DownloadProgress {
                downloaded: done,
                total,
            },
        );
        done as f32 / total.max(1) as f32
    };
    while !worker.is_finished() {
        let fraction = report(downloaded.load(Ordering::SeqCst));
        if progress(fraction).is_break() {
            cancel.store(true, Ordering::SeqCst);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let outcome = worker
        .join()
        .unwrap_or_else(|_| Err(failed("the download thread panicked")));
    report(downloaded.load(Ordering::SeqCst));
    outcome
}

async fn fetch_all(
    dir: PathBuf,
    files: Vec<WeightFile>,
    downloaded: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
) -> Result<(), DownloadError> {
    let client = reqwest::Client::builder()
        .user_agent(APP_TITLE)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(failed)?;
    futures_util::stream::iter(files.iter().map(Ok))
        .try_for_each_concurrent(PARALLEL_DOWNLOADS, |file| {
            fetch_with_retries(&client, &dir, file, &downloaded, &cancel)
        })
        .await
}

async fn fetch_with_retries(
    client: &reqwest::Client,
    dir: &Path,
    file: &WeightFile,
    downloaded: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let dest = dir.join(&file.path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let mut partial = OsString::from(dest.as_os_str());
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        MANIFEST.repo, MANIFEST.revision, file.path
    );

    let mut last_error = String::new();
    for _ in 0..ATTEMPTS {
        let mut written = 0;
        let outcome = fetch_once(
            client,
            &url,
            &partial,
            file,
            downloaded,
            cancel,
            &mut written,
        )
        .await;
        if outcome.is_ok() {
            return std::fs::rename(&partial, &dest).map_err(failed);
        }
        downloaded.fetch_sub(written, Ordering::SeqCst);
        let _ = std::fs::remove_file(&partial);
        match outcome {
            Err(DownloadError::Failed(reason)) => last_error = reason,
            other => return other,
        }
    }
    Err(DownloadError::Failed(format!(
        "{}: {last_error}",
        file.path
    )))
}

async fn fetch_once(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    file: &WeightFile,
    downloaded: &AtomicU64,
    cancel: &AtomicBool,
    written: &mut u64,
) -> Result<(), DownloadError> {
    let response = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(failed)?;
    let mut out = std::fs::File::create(partial).map_err(failed)?;
    let mut hasher = Sha256::new();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Err(DownloadError::Cancelled);
        }
        let chunk = chunk.map_err(failed)?;
        out.write_all(&chunk).map_err(failed)?;
        hasher.update(&chunk);
        *written += chunk.len() as u64;
        downloaded.fetch_add(chunk.len() as u64, Ordering::SeqCst);
        if *written > file.size {
            return Err(failed("larger than expected"));
        }
    }
    out.flush().map_err(failed)?;
    if *written != file.size {
        return Err(failed(format!("got {} of {} bytes", written, file.size)));
    }
    if format!("{:x}", hasher.finalize()) != file.sha256 {
        return Err(failed("checksum mismatch"));
    }
    Ok(())
}

/// Runs diarization over 16 kHz mono `samples` with the weights in `dir`.
/// Returns speaker segments without overlaps.
#[cfg(target_os = "macos")]
pub fn diarize(dir: &Path, samples: &[f32]) -> anyhow::Result<Vec<SpeakerSegment>> {
    use speakrs::{ExecutionMode, PipelineBuilder};

    let mut pipeline = PipelineBuilder::from_dir(dir, ExecutionMode::CoreMl)
        .and_then(|builder| builder.build())
        .map_err(|e| anyhow::anyhow!("failed to load the diarization model: {e}"))?;
    let result = pipeline
        .run(samples)
        .map_err(|e| anyhow::anyhow!("diarization failed: {e}"))?;
    Ok(result
        .exclusive_segments
        .into_iter()
        .map(|s| SpeakerSegment {
            start_secs: s.start,
            end_secs: s.end,
            speaker: s.speaker,
        })
        .collect())
}

#[cfg(not(target_os = "macos"))]
pub fn diarize(_dir: &Path, _samples: &[f32]) -> anyhow::Result<Vec<SpeakerSegment>> {
    anyhow::bail!("speaker diarization is only available on macOS")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_is_pinned_and_complete() {
        assert_eq!(MANIFEST.repo, "avencera/speakrs-models");
        assert_eq!(MANIFEST.revision.len(), 40, "a full commit hash");
        assert_eq!(MANIFEST.files.len(), 68);
        assert!(MANIFEST.files.iter().all(|f| f.sha256.len() == 64
            && f.sha256.chars().all(|c| c.is_ascii_hexdigit())
            && f.size > 0
            && !f.path.starts_with('/')
            && !f.path.contains("..")));
        // ~273 MB: the size the download prompt announces.
        assert_eq!((total_size() as f64 / 1e6).round(), 273.0);
    }

    #[test]
    fn readiness_checks_every_file_size() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!models_ready(dir.path()));
        for f in &MANIFEST.files {
            let path = dir.path().join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create(&path)
                .unwrap()
                .set_len(f.size)
                .unwrap();
        }
        assert!(models_ready(dir.path()));
        // A truncated file makes the set incomplete again.
        let first = &MANIFEST.files[0];
        std::fs::File::create(dir.path().join(&first.path))
            .unwrap()
            .set_len(first.size - 1)
            .unwrap();
        assert!(!models_ready(dir.path()));
        delete_models(dir.path()).unwrap();
        assert!(!dir.path().exists());
        delete_models(dir.path()).unwrap(); // already gone: not an error
    }
}
