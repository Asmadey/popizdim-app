//! Azure `MAI-Transcribe-2-Streaming` over the Realtime WebSocket API.
//!
//! Protocol (OpenAI-Realtime-like, see Microsoft Learn
//! "Use MAI-Transcribe-2-Streaming with the Realtime API"):
//!
//! 1. Connect to `wss://<resource>.services.ai.azure.com/mai/v1/realtime`
//!    with an `api-key` header; the server sends `session.created`.
//! 2. Send `session.update` (PCM16 mono 16 kHz, model = deployment name,
//!    language or `null` for auto, no server-side turn detection) and wait for
//!    `session.updated`. Settings are frozen after the first audio append.
//! 3. Stream `input_audio_buffer.append` with base64 PCM16 (10–100 ms chunks).
//! 4. Read concurrently: `…transcription.delta` appends finalized text,
//!    `…transcription.intermediate` replaces the provisional suffix after the
//!    last delta.
//! 5. `input_audio_buffer.commit` at pauses / end of input; each commit is
//!    answered by `…transcription.completed` once its audio is finalized.

use super::audio::{pcm16_le, SAMPLE_RATE};
use super::AzureConfig;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use log::debug;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const TRANSCRIPTION_EVENT: &str = "conversation.item.input_audio_transcription.";

/// The service rejects commits of less than ~100 ms of audio.
pub const MIN_COMMIT_SAMPLES: usize = (SAMPLE_RATE / 10) as usize;

#[derive(Debug, PartialEq)]
pub enum ServerEvent {
    /// Newly finalized text; append verbatim (spacing included).
    Delta(String),
    /// The whole provisional suffix after the last delta; replaces the previous one.
    Intermediate(String),
    /// Final transcript of the audio covered by one commit.
    Completed(String),
    Committed,
    Error(String),
    Other(String),
}

pub fn parse_event(text: &str) -> ServerEvent {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return ServerEvent::Other("unparseable".into());
    };
    let kind = value["type"].as_str().unwrap_or_default();
    let str_field = |key: &str| value[key].as_str().unwrap_or_default().to_string();
    match kind.strip_prefix(TRANSCRIPTION_EVENT) {
        Some("delta") => ServerEvent::Delta(str_field("delta")),
        Some("intermediate") => ServerEvent::Intermediate(str_field("intermediate")),
        Some("completed") => ServerEvent::Completed(str_field("transcript")),
        Some("failed") => ServerEvent::Error(error_message(&value)),
        _ => match kind {
            "input_audio_buffer.committed" => ServerEvent::Committed,
            "error" => ServerEvent::Error(error_message(&value)),
            other => ServerEvent::Other(other.to_string()),
        },
    }
}

fn error_message(value: &Value) -> String {
    let err = &value["error"];
    match (err["code"].as_str(), err["message"].as_str()) {
        (Some(code), Some(msg)) => format!("{}: {}", code, msg),
        (None, Some(msg)) => msg.to_string(),
        _ => value.to_string(),
    }
}

pub fn append_message(samples: &[f32]) -> Message {
    let audio = base64::engine::general_purpose::STANDARD.encode(pcm16_le(samples));
    Message::Text(
        json!({ "type": "input_audio_buffer.append", "audio": audio })
            .to_string()
            .into(),
    )
}

pub fn commit_message() -> Message {
    Message::Text(r#"{"type":"input_audio_buffer.commit"}"#.into())
}

fn session_update(deployment: &str, language: Option<&str>) -> Message {
    // Auto-detect = no `language` key. The docs describe `null` as auto, but
    // the live service rejects an explicit null ("expected one of 'af', …").
    let mut transcription = json!({ "model": deployment });
    if let Some(language) = language {
        transcription["language"] = json!(language);
    }
    Message::Text(
        json!({
            "type": "session.update",
            "event_id": "configure-1",
            "session": {
                "type": "transcription",
                "audio": {
                    "input": {
                        "format": { "type": "audio/pcm", "rate": SAMPLE_RATE },
                        "transcription": transcription,
                        "turn_detection": null,
                        "noise_reduction": null
                    }
                }
            }
        })
        .to_string()
        .into(),
    )
}

/// Wait for a server event of `wanted` type, failing on `error` events.
async fn wait_for(socket: &mut Socket, wanted: &str) -> Result<(), String> {
    while let Some(message) = socket.next().await {
        let message = message.map_err(|e| format!("Azure connection error: {}", e))?;
        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_str(text.as_str()).unwrap_or_default();
        match value["type"].as_str() {
            Some(kind) if kind == wanted => return Ok(()),
            Some("error") => return Err(format!("Azure error: {}", error_message(&value))),
            _ => {}
        }
    }
    Err(format!("Azure closed the connection before {}", wanted))
}

/// Open and configure a transcription session.
pub async fn connect(cfg: &AzureConfig, language: Option<&str>) -> Result<Socket, String> {
    let mut request = cfg
        .url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("Invalid Azure endpoint: {}", e))?;
    request.headers_mut().insert(
        "api-key",
        HeaderValue::from_str(&cfg.api_key).map_err(|_| "Azure API key has invalid characters")?,
    );

    let handshake = async {
        let (mut socket, response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(describe_connect_error)?;
        debug!("Azure realtime connected (HTTP {})", response.status());
        wait_for(&mut socket, "session.created").await?;
        socket
            .send(session_update(&cfg.deployment, language))
            .await
            .map_err(|e| format!("Azure send failed: {}", e))?;
        wait_for(&mut socket, "session.updated")
            .await
            .map_err(|e| explain_deployment_error(e, &cfg.deployment))?;
        Ok::<_, String>(socket)
    };
    tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake)
        .await
        .map_err(|_| "Timed out connecting to Azure MAI-Transcribe".to_string())?
}

/// The service only accepts 16 kHz audio once `model` resolves to a MAI
/// transcription deployment, and reports a missing deployment as a sample-rate
/// error (or `DeploymentNotFound` once audio is committed). Say what it means.
fn explain_deployment_error(error: String, deployment: &str) -> String {
    if error.contains("for MAI transcription") || error.contains("DeploymentNotFound") {
        format!(
            "This Azure resource has no MAI-Transcribe-2-Streaming deployment named '{}'. \
             Deploy the model in Foundry (Models → Deploy a base model) on a resource in \
             Sweden Central, Central US or South India, and enter its deployment name. ({})",
            deployment, error
        )
    } else {
        error
    }
}

fn describe_connect_error(e: tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match e {
        Error::Http(response) => {
            let status = response.status().as_u16();
            let hint = match status {
                401 | 403 => " — check the Azure API key and that it belongs to this resource",
                404 => {
                    " — check the endpoint and that the region serves MAI-Transcribe-2-Streaming"
                }
                _ => "",
            };
            format!("Azure rejected the connection (HTTP {}){}", status, hint)
        }
        other => format!("Could not connect to Azure: {}", other),
    }
}

/// Transcribe a complete recording through the realtime endpoint (used when
/// there was no live stream, e.g. re-transcribing a history entry).
pub async fn transcribe_buffer(
    cfg: AzureConfig,
    language: Option<String>,
    samples: Vec<f32>,
) -> Result<String, String> {
    let mut socket = connect(&cfg, language.as_deref()).await?;
    let mut padded = samples;
    if padded.len() < MIN_COMMIT_SAMPLES {
        padded.resize(MIN_COMMIT_SAMPLES, 0.0);
    }
    // 100 ms chunks; the service buffers independently of packet boundaries.
    for chunk in padded.chunks(MIN_COMMIT_SAMPLES) {
        socket
            .feed(append_message(chunk))
            .await
            .map_err(|e| format!("Azure send failed: {}", e))?;
    }
    socket
        .send(commit_message())
        .await
        .map_err(|e| format!("Azure send failed: {}", e))?;

    let mut deltas = String::new();
    let deadline = Duration::from_secs(30)
        + Duration::from_secs_f32(padded.len() as f32 / SAMPLE_RATE as f32 / 4.0);
    let result = tokio::time::timeout(deadline, async {
        while let Some(message) = socket.next().await {
            let message = message.map_err(|e| format!("Azure connection error: {}", e))?;
            let Message::Text(text) = message else {
                continue;
            };
            match parse_event(text.as_str()) {
                ServerEvent::Delta(d) => deltas.push_str(&d),
                ServerEvent::Completed(transcript) => {
                    return Ok(if deltas.trim().is_empty() {
                        transcript
                    } else {
                        deltas.clone()
                    });
                }
                ServerEvent::Error(e) => return Err(format!("Azure error: {}", e)),
                _ => {}
            }
        }
        Err("Azure closed the connection before the transcript completed".to_string())
    })
    .await
    .map_err(|_| "Timed out waiting for the Azure transcript".to_string())?;
    let _ = socket.close(None).await;
    result.map(|t| t.trim().to_string())
}

/// Connect, configure, and close — validates endpoint, key and deployment.
/// Connect, configure, and transcribe a moment of silence — a handshake alone
/// succeeds even when the deployment doesn't exist.
pub async fn check(cfg: AzureConfig) -> Result<String, String> {
    let started = std::time::Instant::now();
    let mut socket = connect(&cfg, None).await?;
    let connected_ms = started.elapsed().as_millis();
    let probe = async {
        socket
            .feed(append_message(&vec![0.0; MIN_COMMIT_SAMPLES * 3]))
            .await
            .map_err(|e| format!("Azure send failed: {}", e))?;
        socket
            .send(commit_message())
            .await
            .map_err(|e| format!("Azure send failed: {}", e))?;
        while let Some(message) = socket.next().await {
            let message = message.map_err(|e| format!("Azure connection error: {}", e))?;
            let Message::Text(text) = message else {
                continue;
            };
            match parse_event(text.as_str()) {
                ServerEvent::Completed(_) => return Ok(()),
                ServerEvent::Error(e) => return Err(format!("Azure error: {}", e)),
                _ => {}
            }
        }
        Err("Azure closed the connection during the check".to_string())
    };
    let verified = tokio::time::timeout(Duration::from_secs(20), probe)
        .await
        .map_err(|_| "Timed out waiting for Azure to transcribe a test".to_string())?
        .map_err(|e| explain_deployment_error(e, &cfg.deployment));
    let _ = socket.close(None).await;
    verified?;
    Ok(format!(
        "Deployment '{}' works (connected in {} ms, test transcription in {} ms)",
        cfg.deployment,
        connected_ms,
        started.elapsed().as_millis()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_events() {
        assert_eq!(
            parse_event(
                r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"item_1","content_index":0,"delta":" there!"}"#
            ),
            ServerEvent::Delta(" there!".into())
        );
        assert_eq!(
            parse_event(
                r#"{"type":"conversation.item.input_audio_transcription.intermediate","item_id":"item_1","content_index":0,"intermediate":" world"}"#
            ),
            ServerEvent::Intermediate(" world".into())
        );
        assert_eq!(
            parse_event(
                r#"{"type":"conversation.item.input_audio_transcription.completed", "transcript":"Hello there!"}"#
            ),
            ServerEvent::Completed("Hello there!".into())
        );
        assert_eq!(
            parse_event(r#"{"type":"input_audio_buffer.committed"}"#),
            ServerEvent::Committed
        );
        assert_eq!(
            parse_event(
                r#"{"type":"error","error":{"code":"invalid_value","message":"bad audio"}}"#
            ),
            ServerEvent::Error("invalid_value: bad audio".into())
        );
    }

    #[test]
    fn missing_deployment_is_explained() {
        // What the live service returned for a resource without the deployment.
        let rate = "Azure error: invalid_value: PCM input rate must be 24000, or 16000 for MAI transcription.";
        let msg = explain_deployment_error(rate.into(), "MAI-Transcribe-2-Streaming");
        assert!(msg.contains("no MAI-Transcribe-2-Streaming deployment named"));
        assert!(msg.contains("Sweden Central"));
        let missing = "Azure error: DeploymentNotFound: Input transcription failed for item 'x'.";
        assert!(explain_deployment_error(missing.into(), "d").contains("deployment named 'd'"));
        assert_eq!(explain_deployment_error("other".into(), "d"), "other");
    }

    #[test]
    fn session_update_matches_docs() {
        let Message::Text(text) = session_update("MAI-Transcribe-2-Streaming", None) else {
            panic!("expected text frame");
        };
        let v: Value = serde_json::from_str(text.as_str()).unwrap();
        let input = &v["session"]["audio"]["input"];
        assert_eq!(v["session"]["type"], "transcription");
        assert_eq!(input["format"]["rate"], 16000);
        assert_eq!(
            input["transcription"]["model"],
            "MAI-Transcribe-2-Streaming"
        );
        assert!(input["transcription"].get("language").is_none());
        let Message::Text(text) = session_update("dep", Some("ru")) else {
            panic!("expected text frame");
        };
        let v: Value = serde_json::from_str(text.as_str()).unwrap();
        assert_eq!(
            v["session"]["audio"]["input"]["transcription"]["language"],
            "ru"
        );
        assert!(input["turn_detection"].is_null());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Connect, configure and transcribe a WAV through the real service.
    /// `AZURE_MAI_ENDPOINT=… AZURE_MAI_API_KEY=… [AZURE_MAI_DEPLOYMENT_NAME=…] MAI_TEST_WAV=…
    ///  cargo test --lib live_azure -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_azure_realtime() {
        let cfg = crate::cloud::azure_config(&crate::settings::CloudSettings::default())
            .expect("set AZURE_MAI_ENDPOINT and AZURE_MAI_API_KEY");
        println!("URL: {}  deployment: {}", cfg.url, cfg.deployment);
        let check = crate::cloud::block_on(check(cfg.clone()));
        println!("CHECK: {:?}", check);
        let check = check.expect("session handshake");
        assert!(check.contains("works"));
        if let Ok(path) = std::env::var("MAI_TEST_WAV") {
            let samples: Vec<f32> = hound::WavReader::open(path)
                .unwrap()
                .into_samples::<i16>()
                .map(|s| s.unwrap() as f32 / 32768.0)
                .collect();
            let started = std::time::Instant::now();
            let text = crate::cloud::block_on(transcribe_buffer(cfg, None, samples));
            println!(
                "TRANSCRIPT ({} ms): {:?}",
                started.elapsed().as_millis(),
                text
            );
            assert!(!text.unwrap().is_empty());
        }
    }
}
