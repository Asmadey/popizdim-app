//! Cloud transcription and speech providers for Microsoft's MAI models.
//!
//! Two speech-to-text providers plug into the same model registry and
//! streaming pipeline as the local engines:
//!
//! - **Azure Realtime** ([`azure`]): `MAI-Transcribe-2-Streaming` over the
//!   OpenAI-Realtime-like WebSocket at `/mai/v1/realtime`. True streaming:
//!   intermediate (tentative) and delta (committed) text arrive while the user
//!   speaks.
//! - **OpenRouter** ([`openrouter`]): `microsoft/mai-transcribe-2` over the
//!   batch `/audio/transcriptions` endpoint. OpenRouter has no streaming STT,
//!   so live preview is approximated by shipping each phrase at a speech pause
//!   ([`stream`]).
//!
//! Text-to-speech ([`tts`]) uses `MAI-Voice-2.1` / `MAI-Voice-2.1-Flash` through
//! OpenRouter's `/audio/speech` endpoint.
//!
//! Credentials come from [`CloudSettings`]; empty fields fall back to the
//! environment (`OPENROUTER_API_KEY`, `AZURE_MAI_ENDPOINT`, `AZURE_MAI_API_KEY`,
//! `AZURE_MAI_DEPLOYMENT_NAME` — the names Microsoft's samples use).

pub mod audio;
pub mod azure;
pub mod openrouter;
pub mod stream;
pub mod tts;

use crate::managers::model::{EngineType, ModelInfo, ModelSource};
use crate::settings::{AppSettings, CloudSettings};
use std::future::Future;

pub const OPENROUTER_MODEL_ID: &str = "cloud/openrouter/mai-transcribe-2";
pub const AZURE_MODEL_ID: &str = "cloud/azure/mai-transcribe-2-streaming";

pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const OPENROUTER_STT_MODEL: &str = "microsoft/mai-transcribe-2";
pub const DEFAULT_AZURE_DEPLOYMENT: &str = "MAI-Transcribe-2-Streaming";

/// Sent as `X-Title` / `HTTP-Referer` so requests are attributable on OpenRouter.
pub const APP_TITLE: &str = "Popizdim";
pub const APP_URL: &str = "https://github.com/Asmadey/popizdim-app";

/// Languages MAI-Transcribe-2 transcribes (Microsoft Learn, 2026-09-30).
pub const MAI_LANGUAGES: &[&str] = &[
    "af", "ar", "as", "az", "bg", "bn", "bs", "ca", "cs", "da", "de", "el", "en", "es", "et", "fa",
    "fi", "fil", "fr", "gl", "gu", "he", "hi", "hu", "hy", "id", "is", "it", "ja", "kk", "kn",
    "ko", "lt", "lv", "mk", "ml", "mr", "ms", "nb", "ne", "nl", "or", "pa", "pl", "pt", "ro", "ru",
    "sk", "sl", "sv", "sw", "ta", "te", "th", "tr", "uk", "ur", "vi", "yue", "zh",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudProvider {
    OpenRouter,
    AzureRealtime,
}

impl CloudProvider {
    pub fn from_model_id(model_id: &str) -> Option<Self> {
        match model_id {
            OPENROUTER_MODEL_ID => Some(Self::OpenRouter),
            AZURE_MODEL_ID => Some(Self::AzureRealtime),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::AzureRealtime => "azure",
        }
    }
}

/// Registry entries for the cloud models. They are always "downloaded": there
/// is no file, and missing credentials surface as a transcription error that
/// names the setting to fill in.
pub fn cloud_model_infos() -> Vec<ModelInfo> {
    let languages: Vec<String> = MAI_LANGUAGES.iter().map(|l| l.to_string()).collect();
    let entry =
        |id: &str, provider: CloudProvider, name: &str, description: &str, speed: f32| ModelInfo {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            filename: String::new(),
            source: ModelSource::Cloud {
                provider: provider.as_str().to_string(),
            },
            size_mb: 0,
            is_downloaded: true,
            is_downloading: false,
            partial_size: 0,
            is_directory: false,
            engine_type: EngineType::Cloud,
            accuracy_score: 0.97,
            speed_score: speed,
            supports_translation: false,
            is_recommended: false,
            supported_languages: languages.clone(),
            supports_language_selection: true,
            is_custom: false,
            supports_streaming: true,
            supports_language_detection: true,
        };
    vec![
        entry(
            AZURE_MODEL_ID,
            CloudProvider::AzureRealtime,
            "MAI-Transcribe-2 Streaming (Azure)",
            "Microsoft's real-time cloud model: live text while you speak, 60 languages with auto-detection. Needs an Azure Foundry deployment.",
            0.98,
        ),
        entry(
            OPENROUTER_MODEL_ID,
            CloudProvider::OpenRouter,
            "MAI-Transcribe-2 (OpenRouter)",
            "Microsoft's cloud model via OpenRouter: each phrase is transcribed at a pause, 60 languages with auto-detection. Needs an OpenRouter API key.",
            0.9,
        ),
    ]
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn setting_or_env(value: &str, env: &str) -> Option<String> {
    non_empty(value).or_else(|| std::env::var(env).ok().and_then(|v| non_empty(&v)))
}

pub fn openrouter_api_key(cloud: &CloudSettings) -> Option<String> {
    setting_or_env(&cloud.openrouter_api_key, "OPENROUTER_API_KEY")
}

/// Resolved connection details for the Azure Realtime endpoint.
#[derive(Clone)]
pub struct AzureConfig {
    /// `wss://<resource>.services.ai.azure.com/mai/v1/realtime?intent=transcription`
    pub url: String,
    pub api_key: String,
    pub deployment: String,
}

impl std::fmt::Debug for AzureConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureConfig")
            .field("url", &self.url)
            .field("api_key", &"[REDACTED]")
            .field("deployment", &self.deployment)
            .finish()
    }
}

pub fn azure_config(cloud: &CloudSettings) -> Result<AzureConfig, String> {
    let endpoint = setting_or_env(&cloud.azure_endpoint, "AZURE_MAI_ENDPOINT").ok_or(
        "Azure endpoint is not set (Cloud → Azure Foundry → Endpoint, e.g. https://my-resource.services.ai.azure.com)",
    )?;
    let api_key = setting_or_env(&cloud.azure_api_key, "AZURE_MAI_API_KEY")
        .ok_or("Azure API key is not set (Cloud → Azure Foundry → API key)")?;
    let deployment = setting_or_env(&cloud.azure_deployment, "AZURE_MAI_DEPLOYMENT_NAME")
        .unwrap_or_else(|| DEFAULT_AZURE_DEPLOYMENT.to_string());
    Ok(AzureConfig {
        url: realtime_url(&endpoint)?,
        api_key,
        deployment,
    })
}

/// Turn a resource root (`https://x.services.ai.azure.com`, with or without a
/// trailing slash or an explicit `/mai/v1/realtime` path) into the realtime
/// WebSocket URL.
fn realtime_url(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    let rest = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("wss://"))
        .ok_or("Azure endpoint must start with https:// or wss://")?;
    let host = rest.split(['/', '?']).next().unwrap_or_default();
    if host.is_empty() {
        return Err("Azure endpoint has no host name".to_string());
    }
    Ok(format!(
        "wss://{}/mai/v1/realtime?intent=transcription",
        host
    ))
}

/// The language hint for a cloud request: `None` lets the model auto-detect
/// (including code switching).
pub fn language_hint(effective_language: &str) -> Option<String> {
    match effective_language {
        "" | "auto" => None,
        "zh-Hans" | "zh-Hant" => Some("zh".to_string()),
        other => Some(other.split(['-', '_']).next().unwrap_or(other).to_string()),
    }
}

/// Run a future to completion on a dedicated thread with its own runtime.
///
/// Transcription entry points are synchronous and are called both from plain
/// threads and from inside Tauri's async runtime; blocking on a nested runtime
/// there would panic. A short-lived thread sidesteps both cases.
pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build cloud runtime")
            .block_on(future)
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Phrase hints for the recognizer: the user's custom words.
pub fn phrase_hints(settings: &AppSettings) -> Vec<String> {
    settings
        .custom_words
        .iter()
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .take(100)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_url_from_resource_root() {
        assert_eq!(
            realtime_url("https://my-res.services.ai.azure.com/").unwrap(),
            "wss://my-res.services.ai.azure.com/mai/v1/realtime?intent=transcription"
        );
        assert_eq!(
            realtime_url("wss://my-res.services.ai.azure.com/mai/v1/realtime").unwrap(),
            "wss://my-res.services.ai.azure.com/mai/v1/realtime?intent=transcription"
        );
        assert!(realtime_url("my-res.services.ai.azure.com").is_err());
    }

    #[test]
    fn language_hint_maps_auto_and_regions() {
        assert_eq!(language_hint("auto"), None);
        assert_eq!(language_hint("ru"), Some("ru".into()));
        assert_eq!(language_hint("en-US"), Some("en".into()));
        assert_eq!(language_hint("zh-Hant"), Some("zh".into()));
    }

    #[test]
    fn cloud_ids_round_trip() {
        for info in cloud_model_infos() {
            assert!(CloudProvider::from_model_id(&info.id).is_some());
        }
        assert!(CloudProvider::from_model_id("small").is_none());
    }
}
