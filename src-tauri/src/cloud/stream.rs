//! Live cloud transcription sessions driven by the recorder's real-time frames.
//!
//! A [`CloudStream`] runs on its own thread and runtime. Frames are pushed with
//! [`CloudStream::feed`] as they are captured; preview text is reported through
//! the `on_text(committed, tentative)` callback; [`CloudStream::finalize`]
//! flushes and returns the final text.
//!
//! Both providers commit at speech pauses. A pause is either trailing silence
//! in the feed or — when Handy's VAD drops silent frames before they reach us —
//! a wall-clock gap with no frames at all.

use super::audio::{join_phrases, quietest_cut, rms, seconds, PauseTracker, SAMPLE_RATE};
use super::azure::{self, ServerEvent, MIN_COMMIT_SAMPLES};
use super::openrouter::{self, SttOptions, SttResult};
use super::{AzureConfig, CloudProvider};
use futures_util::stream::FuturesUnordered;
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Silence (or a gap in frames) that marks the end of a phrase.
const PAUSE: Duration = Duration::from_millis(650);
const TICK: Duration = Duration::from_millis(100);
/// Upper bound on waiting for the last results after the user stops.
const FINALIZE_TIMEOUT: Duration = Duration::from_secs(45);

/// OpenRouter phrases shorter than this wait for more speech.
const MIN_PHRASE_SECS: f32 = 1.0;
/// OpenRouter phrases are force-cut at this length so the preview keeps moving
/// and requests stay well under the 60 s limit.
const MAX_PHRASE_SECS: f32 = 25.0;
/// Azure pause commits need at least this much uncommitted audio.
const MIN_AZURE_PAUSE_COMMIT_SECS: f32 = 0.5;

pub type TextCallback = Box<dyn FnMut(&str, &str) + Send>;

enum Input {
    Frame(Vec<f32>),
    Finalize,
    Cancel,
}

pub struct StreamParams {
    pub provider: CloudProvider,
    pub azure: Option<AzureConfig>,
    pub openrouter_key: Option<String>,
    pub stt: SttOptions,
}

#[derive(Debug, Default)]
pub struct CloudFinal {
    pub text: String,
    pub language: Option<String>,
}

pub struct CloudStream {
    tx: mpsc::UnboundedSender<Input>,
    done: std_mpsc::Receiver<Result<CloudFinal, String>>,
}

impl CloudStream {
    pub fn start(params: StreamParams, on_text: TextCallback) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let (done_tx, done) = std_mpsc::channel();
        std::thread::Builder::new()
            .name("cloud-stream".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = done_tx.send(Err(format!("Failed to start runtime: {}", e)));
                        return;
                    }
                };
                let result = runtime.block_on(run(params, rx, on_text));
                let _ = done_tx.send(result);
            })
            .expect("failed to spawn cloud stream thread");
        Self { tx, done }
    }

    pub fn feed(&self, frame: Vec<f32>) {
        let _ = self.tx.send(Input::Frame(frame));
    }

    /// Flush buffered audio and wait for the final text.
    pub fn finalize(self) -> Result<CloudFinal, String> {
        let _ = self.tx.send(Input::Finalize);
        self.done
            .recv_timeout(FINALIZE_TIMEOUT + Duration::from_secs(5))
            .unwrap_or_else(|_| Err("Cloud transcription did not finish in time".into()))
    }

    pub fn cancel(self) {
        let _ = self.tx.send(Input::Cancel);
    }
}

async fn run(
    params: StreamParams,
    rx: mpsc::UnboundedReceiver<Input>,
    on_text: TextCallback,
) -> Result<CloudFinal, String> {
    match params.provider {
        CloudProvider::AzureRealtime => {
            let cfg = params.azure.ok_or("Azure is not configured")?;
            run_azure(cfg, params.stt.language, rx, on_text).await
        }
        CloudProvider::OpenRouter => {
            let key = params
                .openrouter_key
                .ok_or("OpenRouter API key is not set")?;
            run_openrouter(key, params.stt, rx, on_text).await
        }
    }
}

/// Keep draining frames after a fatal error so the caller's finalize still
/// gets an answer (it falls back to batch transcription).
async fn fail_after_drain(
    mut rx: mpsc::UnboundedReceiver<Input>,
    error: String,
) -> Result<CloudFinal, String> {
    warn!("Cloud stream failed: {}", error);
    while let Some(input) = rx.recv().await {
        if matches!(input, Input::Finalize | Input::Cancel) {
            break;
        }
    }
    Err(error)
}

struct PauseClock {
    tracker: PauseTracker,
    last_frame: Option<Instant>,
}

impl PauseClock {
    fn new() -> Self {
        Self {
            tracker: PauseTracker::default(),
            last_frame: None,
        }
    }

    fn push(&mut self, frame: &[f32]) {
        self.tracker.push(frame);
        self.last_frame = Some(Instant::now());
    }

    /// Speech was heard and has since stopped (silent tail or no frames).
    fn at_pause(&self) -> bool {
        let gap = self.last_frame.is_some_and(|t| t.elapsed() >= PAUSE);
        self.tracker.heard_speech()
            && (gap || self.tracker.trailing_silence_secs() >= PAUSE.as_secs_f32())
    }

    fn reset(&mut self) {
        self.tracker.reset();
    }
}

/* ───────────────────────────── Azure ───────────────────────────── */

async fn run_azure(
    cfg: AzureConfig,
    language: Option<String>,
    mut rx: mpsc::UnboundedReceiver<Input>,
    mut on_text: TextCallback,
) -> Result<CloudFinal, String> {
    let started = Instant::now();
    // Frames captured while connecting queue on `rx` and are sent right after.
    let socket = match azure::connect(&cfg, language.as_deref()).await {
        Ok(socket) => socket,
        Err(e) => return fail_after_drain(rx, e).await,
    };
    info!(
        "Azure MAI-Transcribe-2-Streaming session ready in {} ms",
        started.elapsed().as_millis()
    );
    let (mut sink, mut source) = socket.split();

    let mut committed = String::new();
    let mut tentative = String::new();
    let mut completed: Vec<String> = Vec::new();
    let mut pending_commits = 0usize;
    let mut uncommitted = 0usize;
    let mut sent_samples = 0usize;
    let mut pause = PauseClock::new();
    let mut finalize_deadline: Option<tokio::time::Instant> = None;
    let mut ticker = tokio::time::interval(TICK);
    let mut error: Option<String> = None;

    macro_rules! send {
        ($msg:expr) => {
            if let Err(e) = sink.send($msg).await {
                error = Some(format!("Azure send failed: {}", e));
                break;
            }
        };
    }

    loop {
        tokio::select! {
            input = rx.recv(), if finalize_deadline.is_none() => match input {
                Some(Input::Frame(frame)) => {
                    if frame.is_empty() {
                        continue;
                    }
                    pause.push(&frame);
                    uncommitted += frame.len();
                    sent_samples += frame.len();
                    send!(azure::append_message(&frame));
                }
                Some(Input::Finalize) => {
                    if uncommitted > 0 {
                        if uncommitted < MIN_COMMIT_SAMPLES {
                            send!(azure::append_message(&vec![0.0; MIN_COMMIT_SAMPLES - uncommitted]));
                        }
                        send!(azure::commit_message());
                        pending_commits += 1;
                        uncommitted = 0;
                    }
                    if pending_commits == 0 {
                        break;
                    }
                    finalize_deadline = Some(tokio::time::Instant::now() + FINALIZE_TIMEOUT);
                }
                Some(Input::Cancel) | None => {
                    let _ = sink.close().await;
                    return Err("cancelled".into());
                }
            },
            message = source.next() => {
                let text = match message {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(frame))) => {
                        error = Some(format!("Azure closed the session: {:?}", frame));
                        break;
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => {
                        error = Some(format!("Azure connection error: {}", e));
                        break;
                    }
                    None => {
                        error = Some("Azure connection ended unexpectedly".into());
                        break;
                    }
                };
                match azure::parse_event(text.as_str()) {
                    ServerEvent::Delta(delta) => {
                        committed.push_str(&delta);
                        // The intermediate was relative to the previous delta.
                        tentative.clear();
                        on_text(committed.trim_start(), &tentative);
                    }
                    ServerEvent::Intermediate(suffix) => {
                        tentative = suffix;
                        on_text(committed.trim_start(), &tentative);
                    }
                    ServerEvent::Completed(transcript) => {
                        completed.push(transcript);
                        pending_commits = pending_commits.saturating_sub(1);
                        if finalize_deadline.is_some() && pending_commits == 0 {
                            break;
                        }
                    }
                    ServerEvent::Error(e) => {
                        error = Some(format!("Azure error: {}", e));
                        break;
                    }
                    ServerEvent::Committed | ServerEvent::Other(_) => {}
                }
            }
            _ = ticker.tick() => {
                if let Some(deadline) = finalize_deadline {
                    if tokio::time::Instant::now() >= deadline {
                        error = Some("Timed out waiting for the final Azure transcript".into());
                        break;
                    }
                } else if seconds(uncommitted) >= MIN_AZURE_PAUSE_COMMIT_SECS && pause.at_pause() {
                    debug!("Azure: committing {:.2}s at a pause", seconds(uncommitted));
                    send!(azure::commit_message());
                    pending_commits += 1;
                    uncommitted = 0;
                    pause.reset();
                }
            }
        }
    }

    let _ = sink.close().await;
    let text = if committed.trim().is_empty() {
        join_phrases(completed.iter().map(String::as_str))
    } else {
        committed.trim().to_string()
    };
    info!(
        "Azure stream finished: {:.1}s audio, {} chars, {} ms since start",
        seconds(sent_samples),
        text.chars().count(),
        started.elapsed().as_millis()
    );
    match error {
        // Keep what was already finalized if only the tail failed.
        Some(e) if text.is_empty() || finalize_deadline.is_none() => {
            if finalize_deadline.is_none() {
                return fail_after_drain(rx, e).await;
            }
            Err(e)
        }
        Some(e) => {
            warn!(
                "Azure stream ended with an error after partial results: {}",
                e
            );
            Ok(CloudFinal { text, language })
        }
        None => Ok(CloudFinal { text, language }),
    }
}

/* ─────────────────────────── OpenRouter ─────────────────────────── */

type PhraseFuture = Pin<Box<dyn Future<Output = (usize, Result<SttResult, String>)> + Send>>;

async fn run_openrouter(
    api_key: String,
    opts: SttOptions,
    mut rx: mpsc::UnboundedReceiver<Input>,
    mut on_text: TextCallback,
) -> Result<CloudFinal, String> {
    let client = openrouter::client();
    let mut phrase: Vec<f32> = Vec::new();
    let mut pause = PauseClock::new();
    let mut next_index = 0usize;
    let mut results: BTreeMap<usize, Result<SttResult, String>> = BTreeMap::new();
    let mut inflight: FuturesUnordered<PhraseFuture> = FuturesUnordered::new();
    let mut finalize_deadline: Option<tokio::time::Instant> = None;
    let mut ticker = tokio::time::interval(TICK);
    let started = Instant::now();

    let mut dispatch = |samples: Vec<f32>, inflight: &mut FuturesUnordered<PhraseFuture>| {
        let index = next_index;
        next_index += 1;
        let client = client.clone();
        let key = api_key.clone();
        let opts = opts.clone();
        debug!(
            "OpenRouter: sending phrase #{} ({:.2}s)",
            index,
            seconds(samples.len())
        );
        inflight.push(Box::pin(async move {
            (
                index,
                openrouter::transcribe_chunk(&client, &key, &samples, &opts).await,
            )
        }));
    };

    // Committed preview = every finished phrase up to the first one still out.
    let preview = |results: &BTreeMap<usize, Result<SttResult, String>>| {
        let mut parts = Vec::new();
        for (expected, (index, result)) in results.iter().enumerate() {
            if *index != expected {
                break;
            }
            if let Ok(r) = result {
                parts.push(r.text.as_str());
            }
        }
        join_phrases(parts)
    };
    // While more audio is pending, the "…" indicator replaces the ellipsis the
    // model put on a phrase cut at a pause (instead of showing "только...…").
    let mut on_text = move |committed: String, pending: bool| {
        if pending {
            let trimmed = committed.trim_end();
            let trimmed = trimmed
                .strip_suffix("...")
                .or_else(|| trimmed.strip_suffix('…'))
                .unwrap_or(trimmed);
            on_text(trimmed, "…");
        } else {
            on_text(&committed, "");
        }
    };

    loop {
        tokio::select! {
            input = rx.recv(), if finalize_deadline.is_none() => match input {
                Some(Input::Frame(frame)) => {
                    pause.push(&frame);
                    phrase.extend_from_slice(&frame);
                    if seconds(phrase.len()) >= MAX_PHRASE_SECS {
                        let search = 5 * SAMPLE_RATE as usize;
                        let cut = quietest_cut(&phrase, phrase.len() - search, phrase.len());
                        let tail = phrase.split_off(cut);
                        dispatch(std::mem::replace(&mut phrase, tail), &mut inflight);
                    }
                }
                Some(Input::Finalize) => {
                    // The tail goes out even if the RMS gate never fired: a quiet
                    // mic must not lose the last phrase. Only digital silence is
                    // skipped.
                    if seconds(phrase.len()) >= 0.2 && rms(&phrase) > 1e-4 {
                        dispatch(std::mem::take(&mut phrase), &mut inflight);
                    }
                    if inflight.is_empty() {
                        break;
                    }
                    on_text(preview(&results), true);
                    finalize_deadline = Some(tokio::time::Instant::now() + FINALIZE_TIMEOUT);
                }
                Some(Input::Cancel) | None => return Err("cancelled".into()),
            },
            Some((index, result)) = inflight.next(), if !inflight.is_empty() => {
                if let Err(e) = &result {
                    warn!("OpenRouter phrase #{} failed: {}", index, e);
                }
                results.insert(index, result);
                let pending = !inflight.is_empty() || !phrase.is_empty();
                on_text(preview(&results), pending);
                if finalize_deadline.is_some() && inflight.is_empty() {
                    break;
                }
            }
            _ = ticker.tick() => {
                if let Some(deadline) = finalize_deadline {
                    if tokio::time::Instant::now() >= deadline {
                        return Err("Timed out waiting for OpenRouter".into());
                    }
                } else if seconds(phrase.len()) >= MIN_PHRASE_SECS && pause.at_pause() {
                    dispatch(std::mem::take(&mut phrase), &mut inflight);
                    pause.reset();
                    on_text(preview(&results), true);
                }
            }
        }
    }

    // Any failed phrase invalidates the stitched result: the caller then
    // re-transcribes the whole recording in one batch request.
    let mut texts = Vec::with_capacity(results.len());
    let mut language = None;
    for (index, result) in results {
        let result = result.map_err(|e| format!("phrase #{} failed: {}", index, e))?;
        language = language.or(result.language);
        texts.push(result.text);
    }
    let text = join_phrases(texts.iter().map(String::as_str));
    info!(
        "OpenRouter stream finished: {} phrase(s), {} chars, {} ms since start",
        texts.len(),
        text.chars().count(),
        started.elapsed().as_millis()
    );
    Ok(CloudFinal {
        text,
        language: language.or(opts.language),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::audio::SAMPLE_RATE;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;

    fn tone(secs: f32) -> Vec<f32> {
        (0..(secs * SAMPLE_RATE as f32) as usize)
            .map(|i| 0.2 * (i as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin())
            .collect()
    }

    fn feed_paced(stream: &CloudStream, samples: &[f32]) {
        for frame in samples.chunks(320) {
            stream.feed(frame.to_vec());
        }
    }

    /// A local server speaking the documented MAI realtime protocol: an
    /// intermediate per append, and a delta + completed per commit.
    fn spawn_mock_azure() -> (String, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let server_log = Arc::clone(&log);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    listener.set_nonblocking(true).unwrap();
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    let (tcp, _) = listener.accept().await.unwrap();
                    let check_key = |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                                     resp| {
                        assert_eq!(req.headers().get("api-key").unwrap(), "test-key");
                        Ok(resp)
                    };
                    let mut ws = tokio_tungstenite::accept_hdr_async(tcp, check_key).await.unwrap();
                    let send = |v: Value| Message::Text(v.to_string().into());
                    ws.send(send(json!({"type": "session.created", "session": {"id": "s1"}})))
                        .await
                        .unwrap();
                    let mut item = 0;
                    let mut appends = 0;
                    while let Some(Ok(msg)) = ws.next().await {
                        let Message::Text(text) = msg else { continue };
                        let v: Value = serde_json::from_str(text.as_str()).unwrap();
                        let kind = v["type"].as_str().unwrap().to_string();
                        server_log.lock().unwrap().push(kind.clone());
                        match kind.as_str() {
                            "session.update" => {
                                let input = &v["session"]["audio"]["input"];
                                assert_eq!(input["transcription"]["model"], "dep");
                                assert_eq!(input["format"]["rate"], 16000);
                                ws.send(send(json!({"type": "session.updated"}))).await.unwrap();
                            }
                            "input_audio_buffer.append" => {
                                appends += 1;
                                if appends % 10 == 0 {
                                    ws.send(send(json!({
                                        "type": "conversation.item.input_audio_transcription.intermediate",
                                        "intermediate": format!(" draft{}", item + 1)
                                    })))
                                    .await
                                    .unwrap();
                                }
                            }
                            "input_audio_buffer.commit" => {
                                item += 1;
                                for event in [
                                    json!({"type": "conversation.item.input_audio_transcription.delta",
                                           "delta": if item == 1 { "Привет,".to_string() } else { format!(" фраза {}.", item) }}),
                                    json!({"type": "input_audio_buffer.committed"}),
                                    json!({"type": "conversation.item.input_audio_transcription.completed",
                                           "transcript": format!("item {}", item)}),
                                ] {
                                    ws.send(send(event)).await.unwrap();
                                }
                            }
                            _ => {}
                        }
                    }
                });
        });
        (
            format!(
                "ws://127.0.0.1:{}/mai/v1/realtime?intent=transcription",
                port
            ),
            log,
        )
    }

    #[test]
    fn azure_stream_commits_at_pauses_and_finalizes() {
        let (url, log) = spawn_mock_azure();
        let previews: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let seen = Arc::clone(&previews);
        let stream = CloudStream::start(
            StreamParams {
                provider: CloudProvider::AzureRealtime,
                azure: Some(AzureConfig {
                    url,
                    api_key: "test-key".into(),
                    deployment: "dep".into(),
                }),
                openrouter_key: None,
                stt: SttOptions::default(),
            },
            Box::new(move |c, t| seen.lock().unwrap().push((c.to_string(), t.to_string()))),
        );

        feed_paced(&stream, &tone(1.0));
        sleep(Duration::from_millis(1000)); // a gap in frames = a pause → commit
        feed_paced(&stream, &tone(0.6));
        let result = stream.finalize().expect("stream should finalize");

        assert_eq!(result.text, "Привет, фраза 2.");
        let log = log.lock().unwrap();
        assert_eq!(
            log.iter()
                .filter(|k| *k == "input_audio_buffer.commit")
                .count(),
            2
        );
        assert_eq!(log[0], "session.update");
        let previews = previews.lock().unwrap();
        assert!(
            previews.iter().any(|(_, t)| t == " draft1"),
            "intermediate shown as tentative"
        );
        assert!(
            previews.iter().any(|(c, t)| c == "Привет," && t.is_empty()),
            "delta clears tentative"
        );
    }

    #[test]
    fn azure_connect_failure_reports_error_after_finalize() {
        // Nothing listens on this port.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let stream = CloudStream::start(
            StreamParams {
                provider: CloudProvider::AzureRealtime,
                azure: Some(AzureConfig {
                    url: format!("ws://127.0.0.1:{}/mai/v1/realtime", port),
                    api_key: "k".into(),
                    deployment: "dep".into(),
                }),
                openrouter_key: None,
                stt: SttOptions::default(),
            },
            Box::new(|_, _| {}),
        );
        feed_paced(&stream, &tone(0.3));
        let err = stream.finalize().unwrap_err();
        assert!(err.contains("Could not connect"), "{}", err);
    }

    /// Live OpenRouter run over a real recording, paced in real time.
    /// `MAI_TEST_WAV=/path/to/16k-mono.wav OPENROUTER_API_KEY=… cargo test --lib live_openrouter -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_openrouter_pseudo_streaming() {
        let path = std::env::var("MAI_TEST_WAV").expect("set MAI_TEST_WAV");
        let key = std::env::var("OPENROUTER_API_KEY").expect("set OPENROUTER_API_KEY");
        let samples: Vec<f32> = hound::WavReader::open(path)
            .unwrap()
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect();
        let started = Instant::now();
        let stream = CloudStream::start(
            StreamParams {
                provider: CloudProvider::OpenRouter,
                azure: None,
                openrouter_key: Some(key),
                stt: SttOptions::default(),
            },
            Box::new(move |c, t| println!("[{:>5} ms] {}{}", started.elapsed().as_millis(), c, t)),
        );
        for frame in samples.chunks(320) {
            stream.feed(frame.to_vec());
            sleep(Duration::from_millis(20));
        }
        let audio_end = started.elapsed();
        let result = stream.finalize().unwrap();
        println!(
            "FINAL after {} ms (audio ended at {} ms, finalize latency {} ms): {:?} lang={:?}",
            started.elapsed().as_millis(),
            audio_end.as_millis(),
            (started.elapsed() - audio_end).as_millis(),
            result.text,
            result.language
        );
        assert!(!result.text.is_empty());
    }
}
