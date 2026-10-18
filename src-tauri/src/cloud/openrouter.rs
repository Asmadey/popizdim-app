//! OpenRouter speech-to-text (`microsoft/mai-transcribe-2`) and key checks.
//!
//! The endpoint is request/response only: base64 audio in, JSON text out, with
//! a 60 s upstream processing limit per request. Long recordings are split at
//! quiet points and transcribed concurrently.

use super::audio::{join_phrases, split_for_upload, wav_bytes};
use super::{APP_TITLE, APP_URL, OPENROUTER_BASE_URL, OPENROUTER_STT_MODEL};
use base64::Engine as _;
use futures_util::{stream, StreamExt};
use log::{debug, warn};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Upload chunk length. Leaves headroom under OpenRouter's 60 s upstream
/// timeout, since processing time grows with audio length.
const MAX_CHUNK_SECS: f32 = 45.0;
const MAX_PARALLEL_CHUNKS: usize = 4;
const MAX_ATTEMPTS: u32 = 3;

#[derive(Clone, Debug, Default)]
pub struct SttOptions {
    /// ISO-639-1 hint; `None` auto-detects (with code switching).
    pub language: Option<String>,
    /// Keyword biasing for names and domain terms.
    pub phrases: Vec<String>,
    /// `"clean"` drops fillers and false starts; `"verbatim"` keeps them.
    pub transcribe_style: String,
}

#[derive(Clone, Debug, Default)]
pub struct SttResult {
    pub text: String,
    /// Language the model detected, when the provider reports it.
    pub language: Option<String>,
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: String,
}

/// A user-facing explanation for an OpenRouter failure.
fn describe_status(status: reqwest::StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<ErrorBody>(body)
        .map(|b| b.error.message)
        .unwrap_or_else(|_| body.chars().take(200).collect());
    let hint = match status.as_u16() {
        401 => " — the OpenRouter API key is invalid (management keys can't transcribe; create an inference key)",
        402 => " — the OpenRouter account is out of credits",
        403 => " — the key's spend limit is reached or the key is disabled",
        404 => " — no OpenRouter provider can serve microsoft/mai-transcribe-2 right now",
        429 => " — rate limited by OpenRouter",
        _ => "",
    };
    format!("OpenRouter HTTP {}: {}{}", status.as_u16(), detail, hint)
}

fn retryable(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

fn request_body(audio_b64: String, opts: &SttOptions) -> Value {
    let mut body = json!({
        "model": OPENROUTER_STT_MODEL,
        "input_audio": { "data": audio_b64, "format": "wav" },
        "response_format": "verbose_json",
    });
    if let Some(language) = &opts.language {
        body["language"] = json!(language);
    }
    let mut azure = serde_json::Map::new();
    if !opts.phrases.is_empty() {
        azure.insert("phraseList".into(), json!({ "phrases": opts.phrases }));
    }
    if opts.transcribe_style == "clean" {
        azure.insert(
            "enhancedMode".into(),
            json!({ "modelOptions": { "transcribeStyle": "clean" } }),
        );
    }
    if !azure.is_empty() {
        body["provider"] = json!({ "options": { "azure": Value::Object(azure) } });
    }
    body
}

/// Transcribe one chunk (≤ ~60 s) of 16 kHz mono audio.
pub async fn transcribe_chunk(
    client: &reqwest::Client,
    api_key: &str,
    samples: &[f32],
    opts: &SttOptions,
) -> Result<SttResult, String> {
    let wav = wav_bytes(samples).map_err(|e| format!("Failed to encode audio: {}", e))?;
    let body = request_body(base64::engine::general_purpose::STANDARD.encode(wav), opts);
    let started = Instant::now();

    let mut attempt = 0;
    loop {
        attempt += 1;
        let response = client
            .post(format!("{}/audio/transcriptions", OPENROUTER_BASE_URL))
            .bearer_auth(api_key)
            .header("HTTP-Referer", APP_URL)
            .header("X-Title", APP_TITLE)
            .json(&body)
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) if attempt < MAX_ATTEMPTS && (e.is_timeout() || e.is_connect()) => {
                warn!("OpenRouter request failed (attempt {}): {}", attempt, e);
                tokio::time::sleep(Duration::from_millis(400 * attempt as u64)).await;
                continue;
            }
            Err(e) => return Err(format!("OpenRouter request failed: {}", e)),
        };

        let status = response.status();
        let generation = response
            .headers()
            .get("x-generation-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let text = response
            .text()
            .await
            .map_err(|e| format!("OpenRouter response failed: {}", e))?;

        if !status.is_success() {
            if retryable(status) && attempt < MAX_ATTEMPTS {
                warn!(
                    "OpenRouter HTTP {} (attempt {}), retrying",
                    status.as_u16(),
                    attempt
                );
                tokio::time::sleep(Duration::from_millis(600 * attempt as u64)).await;
                continue;
            }
            return Err(describe_status(status, &text));
        }

        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("OpenRouter returned invalid JSON: {}", e))?;
        let result = SttResult {
            text: value["text"].as_str().unwrap_or_default().to_string(),
            language: value["language"].as_str().map(str::to_string),
        };
        debug!(
            "OpenRouter STT: {:.2}s audio in {:?} (generation {}, cost {})",
            super::audio::seconds(samples.len()),
            started.elapsed(),
            generation,
            value["usage"]["cost"]
        );
        return Ok(result);
    }
}

/// Transcribe arbitrarily long audio, splitting it for the 60 s request limit.
pub async fn transcribe(
    api_key: String,
    samples: Vec<f32>,
    opts: SttOptions,
) -> Result<SttResult, String> {
    let client = client();
    let chunks: Vec<Vec<f32>> = split_for_upload(&samples, MAX_CHUNK_SECS)
        .into_iter()
        .map(<[f32]>::to_vec)
        .collect();
    if chunks.len() == 1 {
        return transcribe_chunk(&client, &api_key, &chunks[0], &opts).await;
    }
    debug!(
        "OpenRouter STT: splitting audio into {} chunks",
        chunks.len()
    );
    let results: Vec<Result<SttResult, String>> = stream::iter(chunks)
        .map(|chunk| {
            let (client, api_key, opts) = (client.clone(), api_key.clone(), opts.clone());
            async move { transcribe_chunk(&client, &api_key, &chunk, &opts).await }
        })
        .buffered(MAX_PARALLEL_CHUNKS)
        .collect()
        .await;
    let mut parts = Vec::with_capacity(results.len());
    let mut language = None;
    for result in results {
        let result = result?;
        language = language.or(result.language);
        parts.push(result.text);
    }
    Ok(SttResult {
        text: join_phrases(parts.iter().map(String::as_str)),
        language,
    })
}

/// Validate a key and describe its remaining budget.
pub async fn check_key(api_key: String) -> Result<String, String> {
    let response = client()
        .get(format!("{}/key", OPENROUTER_BASE_URL))
        .bearer_auth(&api_key)
        .send()
        .await
        .map_err(|e| format!("OpenRouter request failed: {}", e))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(describe_status(status, &body));
    }
    let value: Value = serde_json::from_str(&body).unwrap_or_default();
    let data = &value["data"];
    if data["is_management_key"].as_bool() == Some(true) {
        return Err(
            "This is an OpenRouter management key: it can create keys but can't transcribe. \
             Create an inference key at openrouter.ai/settings/keys."
                .to_string(),
        );
    }
    let remaining = match data["limit_remaining"].as_f64() {
        Some(r) => format!("${:.2} left on this key", r),
        None => "no spend limit".to_string(),
    };
    Ok(format!(
        "Key {} is valid ({}, ${:.4} used)",
        data["label"].as_str().unwrap_or("?"),
        remaining,
        data["usage"].as_f64().unwrap_or(0.0)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_omits_language_for_auto_and_passes_azure_options() {
        let body = request_body("AAAA".into(), &SttOptions::default());
        assert!(body.get("language").is_none());
        assert!(body.get("provider").is_none());

        let body = request_body(
            "AAAA".into(),
            &SttOptions {
                language: Some("ru".into()),
                phrases: vec!["OpenRouter".into()],
                transcribe_style: "clean".into(),
            },
        );
        assert_eq!(body["language"], "ru");
        assert_eq!(
            body["provider"]["options"]["azure"]["phraseList"]["phrases"][0],
            "OpenRouter"
        );
        assert_eq!(
            body["provider"]["options"]["azure"]["enhancedMode"]["modelOptions"]["transcribeStyle"],
            "clean"
        );
    }

    #[test]
    fn status_hint_for_management_key() {
        let msg = describe_status(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"User not found.","code":401}}"#,
        );
        assert!(msg.contains("User not found."));
        assert!(msg.contains("management keys"));
    }
}
