//! Settings commands for speaker labels in transcribed files.

use crate::file_transcription::{self, diarize};
use crate::settings;
use serde::Serialize;
use specta::Type;
use std::ops::ControlFlow;
use tauri::AppHandle;

/// State of the diarization model for the settings page.
#[derive(Serialize, Type)]
pub struct DiarizationModelStatus {
    /// This build can diarize (macOS).
    pub supported: bool,
    /// All weight files are in place.
    pub ready: bool,
    /// A download is running (from here or from the tray).
    pub downloading: bool,
    /// Download size of the weights.
    pub size_bytes: u64,
}

#[tauri::command]
#[specta::specta]
pub fn change_diarization_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.diarization_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_diarization_model_status(app: AppHandle) -> Result<DiarizationModelStatus, String> {
    let dir = diarize::models_dir(&app).map_err(|e| format!("{e:#}"))?;
    Ok(DiarizationModelStatus {
        supported: diarize::SUPPORTED,
        ready: diarize::models_ready(&dir),
        downloading: diarize::is_downloading(),
        size_bytes: diarize::total_size(),
    })
}

/// Downloads the weights; progress arrives as `diarization-model-progress`.
#[tauri::command]
#[specta::specta]
pub async fn download_diarization_model(app: AppHandle) -> Result<(), String> {
    if !diarize::SUPPORTED {
        return Err("speaker diarization is only available on macOS".to_string());
    }
    let dir = diarize::models_dir(&app).map_err(|e| format!("{e:#}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        diarize::download_models(&app, &dir, &mut |_| ControlFlow::Continue(()))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn delete_diarization_model(app: AppHandle) -> Result<(), String> {
    // The weights may be in use: being written, or loaded by a running job.
    if diarize::is_downloading() || file_transcription::is_busy() {
        return Err("the speaker model is in use; try again when it is idle".to_string());
    }
    let dir = diarize::models_dir(&app).map_err(|e| format!("{e:#}"))?;
    diarize::delete_models(&dir).map_err(|e| e.to_string())
}
