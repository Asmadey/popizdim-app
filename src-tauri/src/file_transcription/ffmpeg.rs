//! System ffmpeg fallback for formats symphonia cannot decode.
//!
//! ffmpeg is not bundled: v1 uses the user's installation (Homebrew). Apps
//! launched from Finder do not inherit the shell's `PATH`, so the Homebrew
//! locations are checked explicitly before `PATH`.

use super::decode::{Cancelled, Progress, TARGET_SAMPLE_RATE};
use anyhow::{bail, Context};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const KNOWN_LOCATIONS: &[&str] = &["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"];

/// Path to an ffmpeg executable, if one is installed.
pub fn find() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|dir| dir.join("ffmpeg"));
    KNOWN_LOCATIONS
        .iter()
        .map(PathBuf::from)
        .chain(on_path)
        .find(|candidate| candidate.is_file())
}

/// `file:` URL for `input`: stops ffmpeg/ffprobe from reading a protocol
/// prefix out of the name.
fn file_url(input: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(input.as_os_str());
    url
}

/// Duration of `input` in seconds via the `ffprobe` installed next to
/// `ffmpeg`; `None` when it is missing or the container doesn't say.
fn probe_duration(ffmpeg: &Path, input: &Path) -> Option<f64> {
    let ffprobe = ffmpeg.with_file_name("ffprobe");
    if !ffprobe.is_file() {
        return None;
    }
    let output = Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration"])
        .args(["-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(file_url(input))
        .stdin(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|secs| secs.is_finite() && *secs > 0.0)
}

/// Decodes the first audio stream of `input` to 16 kHz mono f32.
pub fn decode(ffmpeg: &Path, input: &Path, progress: Progress) -> anyhow::Result<Vec<f32>> {
    let total_secs = probe_duration(ffmpeg, input);
    let url = file_url(input);

    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(&url)
        .args(["-map", "0:a:0", "-vn", "-ac", "1", "-ar"])
        .arg(TARGET_SAMPLE_RATE.to_string())
        .args(["-f", "f32le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {}", ffmpeg.display()))?;

    // Drain stderr concurrently so a chatty ffmpeg can't block on a full pipe.
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let stderr_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut samples = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut carry = 0usize;
    loop {
        let n = stdout
            .read(&mut chunk[carry..])
            .context("failed to read ffmpeg output")?;
        if n == 0 {
            break;
        }
        let filled = carry + n;
        let (frames, rest) = chunk[..filled].as_chunks::<4>();
        samples.extend(frames.iter().map(|b| f32::from_le_bytes(*b)));
        // Keep a partial trailing sample for the next read.
        carry = rest.len();
        chunk.copy_within(filled - carry..filled, 0);

        let fraction = total_secs.map(|total| {
            (samples.len() as f64 / TARGET_SAMPLE_RATE as f64 / total).min(1.0) as f32
        });
        if progress(fraction).is_break() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Cancelled.into());
        }
    }

    let status = child.wait().context("failed to wait for ffmpeg")?;
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        let detail = stderr.lines().last().unwrap_or("no details").trim();
        bail!("ffmpeg failed ({status}): {detail}");
    }
    if samples.is_empty() {
        bail!("ffmpeg produced no audio");
    }
    Ok(samples)
}
