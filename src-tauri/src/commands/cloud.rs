//! Commands for the MAI cloud providers: credentials, connection checks and
//! text-to-speech.

use crate::cloud::{
    self,
    tts::{Speaker, Voice},
};
use crate::settings::{get_settings, write_settings, CloudSettings};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
#[specta::specta]
pub fn set_cloud_settings(app: AppHandle, cloud: CloudSettings) -> Result<(), String> {
    if !matches!(cloud.transcribe_style.as_str(), "verbatim" | "clean") {
        return Err(format!(
            "Unknown transcription style '{}'",
            cloud.transcribe_style
        ));
    }
    let mut settings = get_settings(&app);
    settings.cloud = cloud;
    write_settings(&app, settings);
    Ok(())
}

/// Check a provider's credentials: `"openrouter"` validates the key and its
/// budget, `"azure"` opens and configures a realtime session.
#[tauri::command]
#[specta::specta]
pub async fn test_cloud_provider(app: AppHandle, provider: String) -> Result<String, String> {
    let settings = get_settings(&app);
    match provider.as_str() {
        "openrouter" => {
            let key = cloud::openrouter_api_key(&settings.cloud)
                .ok_or("OpenRouter API key is not set")?;
            cloud::openrouter::check_key(key).await
        }
        "azure" => {
            let cfg = cloud::azure_config(&settings.cloud)?;
            cloud::azure::check(cfg).await
        }
        other => Err(format!("Unknown provider '{}'", other)),
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_tts_voices() -> Vec<String> {
    cloud::tts::VOICES.iter().map(|v| v.to_string()).collect()
}

/// Speak `text` with MAI-Voice. Returns when playback ends or is stopped.
#[tauri::command]
#[specta::specta]
pub async fn speak_text(app: AppHandle, text: String) -> Result<(), String> {
    speak(&app, text).await
}

#[tauri::command]
#[specta::specta]
pub fn stop_speaking(app: AppHandle) {
    app.state::<Arc<Speaker>>().stop();
}

/// Folder where spoken text is saved: the configured one, or `<app data>/tts`.
fn tts_save_dir(app: &AppHandle, cloud: &CloudSettings) -> Result<PathBuf, String> {
    let custom = cloud.tts_save_dir.trim();
    if !custom.is_empty() {
        return Ok(PathBuf::from(custom));
    }
    crate::portable::app_data_dir(app)
        .map(|dir| dir.join("tts"))
        .map_err(|e| format!("Failed to get app data directory: {}", e))
}

/// The folder spoken text is saved to, for display in settings.
#[tauri::command]
#[specta::specta]
pub fn get_tts_save_dir(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    tts_save_dir(&app, &settings.cloud).map(|dir| dir.to_string_lossy().into_owned())
}

/// Open the folder spoken text is saved to, creating it first.
#[tauri::command]
#[specta::specta]
pub fn open_tts_save_dir(app: AppHandle) -> Result<(), String> {
    let settings = get_settings(&app);
    let dir = tts_save_dir(&app, &settings.cloud)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<String>)
        .map_err(|e| format!("Failed to open {}: {}", dir.display(), e))
}

pub async fn speak(app: &AppHandle, text: String) -> Result<(), String> {
    let settings = get_settings(app);
    let key = cloud::openrouter_api_key(&settings.cloud)
        .ok_or("MAI-Voice needs an OpenRouter API key (Cloud tab)")?;
    let speaker = Arc::clone(&app.state::<Arc<Speaker>>());
    let output_device = settings.selected_output_device.clone();
    let save_to = if settings.cloud.tts_save_audio {
        let name = chrono::Local::now()
            .format("tts-%Y-%m-%d_%H-%M-%S.mp3")
            .to_string();
        match tts_save_dir(app, &settings.cloud) {
            Ok(dir) => Some(dir.join(name)),
            Err(e) => {
                log::warn!("TTS: not saving audio: {}", e);
                None
            }
        }
    } else {
        None
    };
    let voice = Voice {
        model: settings.cloud.tts_model,
        name: settings.cloud.tts_voice,
        speed: settings.cloud.tts_speed,
    };
    tauri::async_runtime::spawn_blocking(move || {
        speaker.speak(key, voice, text, output_device, 1.0, save_to)
    })
    .await
    .map_err(|e| format!("Speech task failed: {}", e))?
}
