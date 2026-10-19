//! Tray "Transcribe Recording…": pick an audio or video file and transcribe
//! it like a dictation.
//!
//! The file is decoded to 16 kHz mono, split into chunks at pauses (VAD),
//! each chunk goes to the active model, and the joined text then takes the
//! microphone's path: the audio is saved to Recordings, a history entry is
//! added and the text is pasted into the focused field. The tray shows the
//! busy icon and a percentage for the whole job; tray "Cancel" stops it.
//!
//! With speaker labels on (the default) the file is diarized locally first
//! and each speaker turn is transcribed separately; the text then reads
//! "[00:01:23] Speaker 1: …". A recording with a single speaker gets no
//! labels.
//!
//! Plan: docs/plans/file-transcription.md.

mod chunk;
mod decode;
pub mod diarize;
pub(crate) mod ffmpeg;
mod focus;
mod turns;

pub use decode::{decode_file, TARGET_SAMPLE_RATE};

use crate::audio_toolkit::save_wav_file;
use crate::managers::history::HistoryManager;
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings;
use crate::tray::{set_tray_progress, set_tray_state, TrayIconState, TrayProgress};
use crate::tray_i18n::get_tray_translations;
use decode::{decode_file_with, DecodeError};
use diarize::DownloadError;
use focus::FrontmostApp;
use log::{error, info, warn};
use std::ops::{ControlFlow, Range};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use turns::{format_transcript, speaker_count, speaker_turns, Turn, TurnText};

/// Set by [`cancel`]; checked by the running file job between steps.
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

/// A picked file is being processed.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Shares of the overall progress; transcription takes the rest.
const DECODE_SHARE: f32 = 0.10;
const DIARIZE_SHARE: f32 = 0.15;
const VAD_SHARE: f32 = 0.10;

/// Assumed diarization speed (× real time) for the progress estimate; the
/// prototype measured 72-134× on an M4.
const DIARIZE_SPEED_ESTIMATE: f32 = 60.0;

/// Stops the file being processed, if any. Wired into the app-wide cancel
/// (tray "Cancel", cancel shortcut).
pub fn cancel() {
    CANCEL_REQUESTED.store(true, Ordering::SeqCst);
}

/// Whether a picked file is being processed. Dictation does not start
/// meanwhile: both would compete for the model, the tray and the paste.
pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

/// Offered in the picker. Symphonia reads some of these, ffmpeg the rest.
const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "wav", "wave", "flac", "ogg", "oga", "opus", "wma", "aif", "aiff",
    "caf", "amr", "alac", "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg",
    "ts", "mts", "m2ts", "3gp",
];

/// Speaker labels for one file, decided before the picker opens.
enum Speakers {
    Off,
    /// Diarize with the weights in `dir`, downloading them first if needed.
    Label {
        dir: PathBuf,
        download: bool,
    },
}

/// Opens the file picker; a picked file is processed on a background thread.
/// Called from the tray menu handler, i.e. on the main thread.
///
/// If speaker labels are on but the diarization model isn't downloaded yet,
/// asks first whether to download it (or go without labels this time).
pub fn pick_and_start(app: &AppHandle) {
    if is_busy() {
        info!("A file is already being processed");
        return;
    }
    let settings = settings::get_settings(app);
    let strings = get_tray_translations(Some(settings.app_language.clone()));
    // Captured before Popizdim activates itself for the dialogs.
    let previous_app = FrontmostApp::capture();
    focus::activate_self();

    let models_dir = if settings.diarization_enabled && diarize::SUPPORTED {
        diarize::models_dir(app)
            .map_err(|e| warn!("No place for the diarization model: {e:#}"))
            .ok()
    } else {
        None
    };
    match models_dir {
        Some(dir) if !diarize::models_ready(&dir) => {
            let size_mb = (diarize::total_size() as f64 / 1e6).round().to_string();
            let app_handle = app.clone();
            app.dialog()
                .message(
                    strings
                        .diarization_prompt_body
                        .replace("{{size}}", &size_mb),
                )
                .title(&strings.diarization_prompt_title)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    strings.diarization_prompt_download.clone(),
                    strings.diarization_prompt_skip.clone(),
                ))
                .show(move |download| {
                    let speakers = if download {
                        Speakers::Label {
                            dir,
                            download: true,
                        }
                    } else {
                        Speakers::Off
                    };
                    open_picker(&app_handle, previous_app, speakers);
                });
        }
        Some(dir) => open_picker(
            app,
            previous_app,
            Speakers::Label {
                dir,
                download: false,
            },
        ),
        None => open_picker(app, previous_app, Speakers::Off),
    }
}

fn open_picker(app: &AppHandle, previous_app: Option<FrontmostApp>, speakers: Speakers) {
    let strings = get_tray_translations(Some(settings::get_settings(app).app_language));
    let app_handle = app.clone();
    app.dialog()
        .file()
        .set_title(&strings.transcribe_file_dialog_title)
        .add_filter(&strings.media_files, MEDIA_EXTENSIONS)
        .pick_file(move |picked| {
            if let Some(previous_app) = previous_app {
                // After the panel has fully closed, from the main thread.
                if let Err(e) = app_handle.run_on_main_thread(move || previous_app.restore()) {
                    error!("Failed to schedule focus restore: {}", e);
                }
            }
            let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
                info!("File picker dismissed");
                return;
            };
            let app = app_handle.clone();
            std::thread::spawn(move || process_picked(&app, &path, speakers));
        });
}

enum JobError {
    Cancelled,
    Failed(String),
}

impl From<DecodeError> for JobError {
    fn from(e: DecodeError) -> Self {
        match e {
            DecodeError::Cancelled => JobError::Cancelled,
            other => JobError::Failed(other.to_string()),
        }
    }
}

/// Processes a picked file while the tray shows it: the busy icon (whose menu
/// offers Cancel and disables "Transcribe Recording…") plus the percentage.
fn process_picked(app: &AppHandle, path: &Path, speakers: Speakers) {
    if BUSY.swap(true, Ordering::SeqCst) {
        info!(
            "A file is already being processed; ignoring {}",
            path.display()
        );
        return;
    }
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    set_tray_state(app, TrayIconState::Transcribing);
    set_tray_progress(app, Some(TrayProgress::Indeterminate));

    let start = Instant::now();
    let outcome = transcribe_file(app, path, speakers);

    set_tray_progress(app, None);
    set_tray_state(app, TrayIconState::Idle);
    BUSY.store(false, Ordering::SeqCst);

    match outcome {
        Ok(()) => info!("Transcribed {} in {:.1?}", path.display(), start.elapsed()),
        Err(JobError::Cancelled) => info!("Processing of {} cancelled", path.display()),
        Err(JobError::Failed(reason)) => {
            error!("Failed to transcribe {}: {}", path.display(), reason);
            let _ = app.emit("transcription-error", reason);
        }
    }
}

/// Throttles progress to whole-percent tray updates and checks for cancel.
struct TrayReporter<'a> {
    app: &'a AppHandle,
    shown: TrayProgress,
}

impl TrayReporter<'_> {
    fn report(&mut self, overall: Option<f32>) -> ControlFlow<()> {
        if CANCEL_REQUESTED.load(Ordering::SeqCst) {
            return ControlFlow::Break(());
        }
        let progress = tray_progress(overall);
        if progress != self.shown {
            self.shown = progress;
            set_tray_progress(self.app, Some(progress));
        }
        ControlFlow::Continue(())
    }
}

fn cancelled() -> bool {
    CANCEL_REQUESTED.load(Ordering::SeqCst)
}

fn transcribe_file(app: &AppHandle, path: &Path, speakers: Speakers) -> Result<(), JobError> {
    let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
    let settings = settings::get_settings(app);

    // Same guard as dictation: don't decode anything if no model can
    // transcribe it. The load runs while the file is decoded.
    if !tm.is_model_loaded() {
        if let Err(e) = app
            .state::<Arc<ModelManager>>()
            .get_model_path(&settings.selected_model)
        {
            return Err(JobError::Failed(format!(
                "no model can transcribe the file ({e})"
            )));
        }
    }
    tm.initiate_model_load();

    let mut reporter = TrayReporter {
        app,
        shown: TrayProgress::Indeterminate,
    };

    // The speaker model, when it still has to be downloaded, shows its own
    // 0-100% before the file's progress starts.
    let speakers_dir = match speakers {
        Speakers::Off => None,
        Speakers::Label {
            dir,
            download: false,
        } => Some(dir),
        Speakers::Label {
            dir,
            download: true,
        } => match diarize::download_models(app, &dir, &mut |f| reporter.report(Some(f))) {
            Ok(()) => Some(dir),
            Err(DownloadError::Cancelled) => return Err(JobError::Cancelled),
            Err(e) => {
                warn!("Diarization model download failed ({e}); no speaker labels");
                None
            }
        },
    };

    let samples = decode_file_with(path, &mut |fraction| {
        reporter.report(fraction.map(|f| f * DECODE_SHARE))
    })?;
    let audio_secs = samples.len() as f32 / TARGET_SAMPLE_RATE as f32;
    info!("Decoded {} ({:.1} s)", path.display(), audio_secs);
    let max_len = (chunk::MAX_CHUNK_SECS * TARGET_SAMPLE_RATE as f32) as usize;

    let mut progress_start = DECODE_SHARE;
    if let Some(dir) = speakers_dir {
        match diarize_turns(&dir, &samples, &mut reporter, progress_start) {
            Ok(turns) if speaker_count(&turns) >= 2 => {
                info!("{} speakers, {} turns", speaker_count(&turns), turns.len());
                let label =
                    get_tray_translations(Some(settings.app_language.clone())).speaker_label;
                let text = transcribe_turns(
                    &tm,
                    &samples,
                    &turns,
                    max_len,
                    &label,
                    &mut reporter,
                    progress_start + DIARIZE_SHARE,
                )?;
                return deliver(app, path, &samples, text);
            }
            Ok(turns) => info!(
                "{} speaker(s) found; transcribing without labels",
                speaker_count(&turns)
            ),
            Err(JobError::Cancelled) => return Err(JobError::Cancelled),
            Err(JobError::Failed(reason)) => {
                warn!("{reason}; transcribing without speaker labels")
            }
        }
        progress_start += DIARIZE_SHARE;
    }

    let chunks = match crate::managers::audio::create_vad_detector(settings.vad_backend, app) {
        Ok(mut vad) => {
            let flags = chunk::detect_speech(&samples, vad.as_mut(), &mut |f| {
                reporter.report(Some(progress_start + f * VAD_SHARE))
            })
            .map_err(|e| JobError::Failed(format!("voice activity detection failed: {e:#}")))?
            .ok_or(JobError::Cancelled)?;
            let regions = chunk::speech_regions(&flags, vad.frame_samples(), samples.len());
            chunk::pack_chunks(&regions, &samples, max_len)
        }
        Err(e) => {
            warn!("VAD unavailable ({e:#}); splitting at quiet points instead");
            chunk::split_without_vad(&samples, max_len)
        }
    };
    progress_start += VAD_SHARE;
    let mut transcriber = ChunkTranscriber::new(&tm, &samples, &chunks, progress_start);
    info!(
        "{} chunk(s), {:.1} s to transcribe of {:.1} s",
        chunks.len(),
        transcriber.total as f32 / TARGET_SAMPLE_RATE as f32,
        audio_secs
    );
    let mut texts = Vec::new();
    for range in &chunks {
        let text = transcriber.transcribe(range.clone(), &mut reporter)?;
        if !text.trim().is_empty() {
            texts.push(text);
        }
    }
    if cancelled() {
        return Err(JobError::Cancelled);
    }
    let _ = reporter.report(Some(1.0));
    let text = crate::cloud::audio::join_phrases(texts.iter().map(String::as_str));
    deliver(app, path, &samples, text)
}

/// Diarizes the whole file and cleans the segments up into turns. speakrs
/// can't report progress or stop early, so the tray shows an estimate from
/// the expected speed and a cancel takes effect when it returns.
fn diarize_turns(
    dir: &Path,
    samples: &[f32],
    reporter: &mut TrayReporter,
    progress_start: f32,
) -> Result<Vec<Turn>, JobError> {
    let audio_secs = samples.len() as f32 / TARGET_SAMPLE_RATE as f32;
    let estimate = 1.0 + audio_secs / DIARIZE_SPEED_ESTIMATE;
    let started = Instant::now();
    let outcome = std::thread::scope(|scope| {
        let worker = scope.spawn(|| diarize::diarize(dir, samples));
        while !worker.is_finished() {
            let fraction = (started.elapsed().as_secs_f32() / estimate).min(0.95);
            let _ = reporter.report(Some(progress_start + fraction * DIARIZE_SHARE));
            std::thread::sleep(Duration::from_millis(200));
        }
        worker.join()
    });
    if cancelled() {
        return Err(JobError::Cancelled);
    }
    let segments = outcome
        .map_err(|_| JobError::Failed("diarization panicked".to_string()))?
        .map_err(|e| JobError::Failed(format!("{e:#}")))?;
    info!("Diarized {:.1} s in {:.1?}", audio_secs, started.elapsed());
    Ok(speaker_turns(&segments, samples.len()))
}

/// Transcribes each turn (split at pauses when it is longer than `max_len`)
/// and formats the labelled transcript.
fn transcribe_turns(
    tm: &TranscriptionManager,
    samples: &[f32],
    turns: &[Turn],
    max_len: usize,
    label: &str,
    reporter: &mut TrayReporter,
    progress_start: f32,
) -> Result<String, JobError> {
    let pieces: Vec<Vec<Range<usize>>> = turns
        .iter()
        .map(|turn| chunk::pack_chunks(std::slice::from_ref(&turn.range), samples, max_len))
        .collect();
    let all: Vec<Range<usize>> = pieces.iter().flatten().cloned().collect();
    let mut transcriber = ChunkTranscriber::new(tm, samples, &all, progress_start);

    let mut texts = Vec::with_capacity(turns.len());
    for (turn, ranges) in turns.iter().zip(&pieces) {
        let mut parts = Vec::new();
        for range in ranges {
            let text = transcriber.transcribe(range.clone(), reporter)?;
            if !text.trim().is_empty() {
                parts.push(text);
            }
        }
        texts.push(TurnText {
            start: turn.range.start,
            speaker: turn.speaker,
            text: crate::cloud::audio::join_phrases(parts.iter().map(String::as_str)),
        });
    }
    if cancelled() {
        return Err(JobError::Cancelled);
    }
    let _ = reporter.report(Some(1.0));
    Ok(format_transcript(&texts, label))
}

/// Sends sample ranges to the active model one by one, reporting progress
/// from `progress_start` to 100% over their total length.
struct ChunkTranscriber<'a> {
    tm: &'a TranscriptionManager,
    samples: &'a [f32],
    progress_start: f32,
    total: usize,
    done: usize,
}

impl<'a> ChunkTranscriber<'a> {
    fn new(
        tm: &'a TranscriptionManager,
        samples: &'a [f32],
        ranges: &[Range<usize>],
        progress_start: f32,
    ) -> Self {
        Self {
            tm,
            samples,
            progress_start,
            total: ranges.iter().map(|r| r.len()).sum(),
            done: 0,
        }
    }

    fn transcribe(
        &mut self,
        range: Range<usize>,
        reporter: &mut TrayReporter,
    ) -> Result<String, JobError> {
        let fraction = self.done as f32 / self.total.max(1) as f32;
        if reporter
            .report(Some(stage_progress(self.progress_start, fraction)))
            .is_break()
        {
            return Err(JobError::Cancelled);
        }
        // A no-op while the model is loaded; reloads it if the "unload
        // immediately" setting dropped it after the previous chunk.
        self.tm.initiate_model_load();
        let text = self
            .tm
            .transcribe(self.samples[range.clone()].to_vec())
            .map_err(|e| {
                if cancelled() {
                    JobError::Cancelled
                } else {
                    JobError::Failed(format!("{e:#}"))
                }
            })?;
        self.done += range.len();
        Ok(text)
    }
}

/// From here on it is a finished dictation: Recordings, history, paste.
fn deliver(app: &AppHandle, path: &Path, samples: &[f32], text: String) -> Result<(), JobError> {
    let hm = app.state::<Arc<HistoryManager>>();
    let file_name = format!("popizdim-{}.wav", chrono::Utc::now().timestamp());
    match save_wav_file(hm.recordings_dir().join(&file_name), samples) {
        Ok(()) => {
            if let Err(e) = hm.save_entry(file_name, text.clone(), false, None, None) {
                error!("Failed to save history entry: {}", e);
            }
        }
        Err(e) => error!("Failed to save {} to Recordings: {}", file_name, e),
    }

    if text.is_empty() {
        info!("No speech found in {}", path.display());
        return Ok(());
    }
    let paste_app = app.clone();
    app.run_on_main_thread(move || {
        if cancelled() {
            return;
        }
        if let Err(e) = crate::utils::paste(text, paste_app.clone()) {
            error!("Failed to paste transcription: {}", e);
            let _ = paste_app.emit("paste-error", ());
        }
    })
    .map_err(|e| JobError::Failed(format!("failed to paste on the main thread: {e}")))
}

/// Overall progress for a fraction of the last stage, which starts at
/// `start` and runs to 100%.
fn stage_progress(start: f32, fraction: f32) -> f32 {
    start + fraction.clamp(0.0, 1.0) * (1.0 - start)
}

fn tray_progress(fraction: Option<f32>) -> TrayProgress {
    match fraction {
        Some(f) => TrayProgress::Percent((f.clamp(0.0, 1.0) * 100.0).floor() as u8),
        None => TrayProgress::Indeterminate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_progress_maps_fractions_to_whole_percent() {
        assert_eq!(tray_progress(None), TrayProgress::Indeterminate);
        assert_eq!(tray_progress(Some(0.0)), TrayProgress::Percent(0));
        assert_eq!(tray_progress(Some(0.349)), TrayProgress::Percent(34));
        assert_eq!(tray_progress(Some(1.0)), TrayProgress::Percent(100));
        assert_eq!(tray_progress(Some(1.7)), TrayProgress::Percent(100));
        assert_eq!(tray_progress(Some(-0.2)), TrayProgress::Percent(0));
    }

    #[test]
    fn the_last_stage_takes_the_rest_of_the_progress() {
        let start = DECODE_SHARE + VAD_SHARE;
        assert!((stage_progress(start, 0.0) - start).abs() < 1e-6);
        assert!((stage_progress(start, 1.0) - 1.0).abs() < 1e-6);
        assert!((stage_progress(start, 0.5) - 0.6).abs() < 1e-6);
        assert!((stage_progress(start, 7.0) - 1.0).abs() < 1e-6);
    }
}
