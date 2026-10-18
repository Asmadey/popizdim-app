//! Text-to-speech with `MAI-Voice-2.1` / `MAI-Voice-2.1-Flash` via OpenRouter.
//!
//! Requests raw PCM (`audio/pcm;rate=24000;channels=1`, signed 16-bit LE) so
//! playback needs no decoder. Long text is split into sentence groups —
//! MAI-Voice-2.1-Flash renders at most ~45 s per request — and the next group is
//! synthesized while the current one plays.

use super::{APP_TITLE, APP_URL, OPENROUTER_BASE_URL};
use log::{debug, info, warn};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_TTS_MODEL: &str = "microsoft/mai-voice-2.1-flash";
pub const DEFAULT_TTS_VOICE: &str = "ru-RU-Masha";

/// Characters per request: roughly 30 s of speech, under the 45 s cap.
const MAX_CHARS_PER_REQUEST: usize = 450;

/// Voices published for both MAI-Voice-2.1 models (OpenRouter, 2026-10). The
/// API id is `<voice>:MAI-Voice-2.1` or `<voice>:MAI-Voice-2.1-Flash`.
pub const VOICES: &[&str] = &[
    "cs-CZ-Grant",
    "cs-CZ-Harper",
    "da-DK-Grant",
    "da-DK-Harper",
    "de-DE-Grant",
    "de-DE-Harper",
    "de-DE-Klaus",
    "de-DE-Mia",
    "en-AU-Isla",
    "en-GB-Emily",
    "en-GB-Harry",
    "en-IN-Dhruv",
    "en-IN-Priya",
    "en-US-Ethan",
    "en-US-Grant",
    "en-US-Harper",
    "en-US-Iris",
    "en-US-Jasper",
    "en-US-Olivia",
    "en-US-Sage",
    "es-ES-Marta",
    "es-MX-Alejo",
    "es-MX-Grant",
    "es-MX-Harper",
    "es-MX-Valeria",
    "fi-FI-Grant",
    "fi-FI-Harper",
    "fr-FR-Grant",
    "fr-FR-Harper",
    "fr-FR-Marc",
    "fr-FR-Soleil",
    "hi-IN-Arjun",
    "hi-IN-Dhruv",
    "hi-IN-Grant",
    "hi-IN-Harper",
    "hi-IN-Kavya",
    "hi-IN-Priya",
    "hu-HU-Bence",
    "hu-HU-Grant",
    "hu-HU-Harper",
    "hu-HU-Levente",
    "hu-HU-Lilla",
    "hu-HU-Reka",
    "id-ID-Grant",
    "id-ID-Harper",
    "it-IT-Grant",
    "it-IT-Harper",
    "it-IT-Luca",
    "it-IT-Rosa",
    "ko-KR-Grant",
    "ko-KR-Haena",
    "ko-KR-Harper",
    "ko-KR-Junho",
    "nb-NO-Grant",
    "nb-NO-Harper",
    "nl-NL-Grant",
    "nl-NL-Harper",
    "nl-NL-Sander",
    "pl-PL-Grant",
    "pl-PL-Harper",
    "pt-BR-Caio",
    "pt-BR-Grant",
    "pt-BR-Harper",
    "pt-BR-Luana",
    "pt-BR-Pedro",
    "pt-BR-Rafael",
    "pt-PT-Grant",
    "pt-PT-Harper",
    "pt-PT-Rui",
    "ro-RO-Andrei",
    "ro-RO-Elena",
    "ro-RO-Grant",
    "ro-RO-Harper",
    "ro-RO-Ioana",
    "ro-RO-Radu",
    "ru-RU-Grant",
    "ru-RU-Harper",
    "ru-RU-Lev",
    "ru-RU-Masha",
    "sv-SE-Grant",
    "sv-SE-Harper",
    "th-TH-Grant",
    "th-TH-Harper",
    "th-TH-Krit",
    "th-TH-Nattapong",
    "tr-TR-Aydin",
    "tr-TR-Elif",
    "tr-TR-Grant",
    "tr-TR-Harper",
    "vi-VN-Grant",
    "vi-VN-Harper",
    "zh-CN-Bo",
    "zh-CN-Grant",
    "zh-CN-Harper",
    "zh-CN-Lan",
    "zh-CN-Mei",
    "zh-CN-Wei",
];

/// `ru-RU-Masha` + `microsoft/mai-voice-2.1-flash` → `ru-RU-Masha:MAI-Voice-2.1-Flash`.
pub fn voice_id(voice: &str, model: &str) -> String {
    if voice.contains(':') {
        return voice.to_string();
    }
    let suffix = if model.ends_with("-flash") {
        "MAI-Voice-2.1-Flash"
    } else {
        "MAI-Voice-2.1"
    };
    format!("{}:{}", voice, suffix)
}

/// Split text into request-sized pieces at sentence (then word) boundaries.
pub fn split_text(text: &str) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？' | '\n') {
            sentences.push(std::mem::take(&mut current));
        }
    }
    sentences.push(current);

    let mut pieces: Vec<String> = Vec::new();
    let mut piece = String::new();
    for sentence in sentences {
        if sentence.trim().is_empty() {
            continue;
        }
        if piece.chars().count() + sentence.chars().count() > MAX_CHARS_PER_REQUEST
            && !piece.trim().is_empty()
        {
            pieces.push(std::mem::take(&mut piece));
        }
        if sentence.chars().count() > MAX_CHARS_PER_REQUEST {
            // One very long sentence: fall back to word boundaries.
            for word in sentence.split_inclusive(char::is_whitespace) {
                if piece.chars().count() + word.chars().count() > MAX_CHARS_PER_REQUEST
                    && !piece.trim().is_empty()
                {
                    pieces.push(std::mem::take(&mut piece));
                }
                piece.push_str(word);
            }
        } else {
            piece.push_str(&sentence);
        }
    }
    if !piece.trim().is_empty() {
        pieces.push(piece);
    }
    pieces
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

pub struct Pcm {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// What to synthesize with: MAI-Voice model, voice and speech rate.
pub struct Voice {
    pub model: String,
    /// Voice name without the model suffix, e.g. `ru-RU-Lev`.
    pub name: String,
    /// 1.0 = normal. MAI-Voice re-synthesizes faster speech (the timbre stays);
    /// it stops speeding up near 2.0.
    pub speed: f32,
}

/// Speech-rate range offered to the service.
pub const SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.0;

/// Request body for `/audio/speech`. `speed` is only sent when it differs
/// from normal: models without rate control may reject a non-default value.
fn speech_request(voice: &Voice, text: &str) -> serde_json::Value {
    let mut body = json!({
        "model": voice.model,
        "input": text,
        "voice": voice_id(&voice.name, &voice.model),
        "response_format": "pcm",
    });
    let speed = voice.speed.clamp(*SPEED_RANGE.start(), *SPEED_RANGE.end());
    if (speed - 1.0).abs() > 0.01 {
        body["speed"] = json!(speed);
    }
    body
}

/// Save spoken audio as MP3 through the system ffmpeg, or as WAV next to
/// the requested path when ffmpeg is missing or fails. Returns the file
/// written.
pub fn save_spoken_audio(
    mp3_path: &std::path::Path,
    sample_rate: u32,
    samples: &[f32],
) -> Result<std::path::PathBuf, String> {
    if let Some(dir) = mp3_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    }
    match crate::file_transcription::ffmpeg::find() {
        Some(ffmpeg) => match encode_mp3(&ffmpeg, mp3_path, sample_rate, samples) {
            Ok(()) => return Ok(mp3_path.to_path_buf()),
            Err(e) => warn!("TTS: MP3 encoding failed, saving WAV: {}", e),
        },
        None => warn!("TTS: ffmpeg not found (brew install ffmpeg), saving WAV"),
    }
    let wav_path = mp3_path.with_extension("wav");
    save_wav(&wav_path, sample_rate, samples)?;
    Ok(wav_path)
}

/// Encode mono s16le PCM to MP3 (44.1 kHz stereo, 128 kbps). Writes to a
/// `.part` file first so a failed run leaves no broken MP3 behind.
fn encode_mp3(
    ffmpeg: &std::path::Path,
    path: &std::path::Path,
    sample_rate: u32,
    samples: &[f32],
) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    let partial = std::path::PathBuf::from(partial);
    let mut child = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .args(["-f", "s16le", "-ar", &sample_rate.to_string(), "-ac", "1"])
        .args(["-i", "pipe:0"])
        .args([
            "-ar",
            "44100",
            "-ac",
            "2",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
        ])
        .args(["-f", "mp3"])
        .arg(&partial)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {}", ffmpeg.display(), e))?;
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|&s| ((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
        .collect();
    let written = child
        .stdin
        .take()
        .ok_or_else(|| "ffmpeg stdin unavailable".to_string())
        .and_then(|mut stdin| stdin.write_all(&pcm).map_err(|e| e.to_string()));
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if let Err(e) = written.and_then(|()| {
        output.status.success().then_some(()).ok_or_else(|| {
            String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(300)
                .collect()
        })
    }) {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, path).map_err(|e| format!("{}: {}", path.display(), e))
}

/// Write mono float samples as a 16-bit WAV, creating the folder if needed.
pub fn save_wav(path: &std::path::Path, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| format!("{}: {}", path.display(), e))?;
    for &sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(value)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("{}: {}", path.display(), e))
}

fn rate_from_content_type(content_type: &str) -> u32 {
    content_type
        .split(';')
        .filter_map(|part| part.trim().strip_prefix("rate="))
        .find_map(|rate| rate.parse().ok())
        .unwrap_or(24_000)
}

pub async fn synthesize(
    client: &reqwest::Client,
    api_key: &str,
    voice: &Voice,
    text: &str,
) -> Result<Pcm, String> {
    let response = client
        .post(format!("{}/audio/speech", OPENROUTER_BASE_URL))
        .bearer_auth(api_key)
        .header("HTTP-Referer", APP_URL)
        .header("X-Title", APP_TITLE)
        .json(&speech_request(voice, text))
        .send()
        .await
        .map_err(|e| format!("OpenRouter TTS request failed: {}", e))?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("OpenRouter TTS response failed: {}", e))?;
    if !status.is_success() {
        return Err(format!(
            "OpenRouter TTS HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&bytes)
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    let samples = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0)
        .collect();
    Ok(Pcm {
        samples,
        sample_rate: rate_from_content_type(&content_type),
    })
}

/// Process-wide playback state: one utterance at a time, newest wins.
#[derive(Default)]
pub struct Speaker {
    generation: AtomicU64,
    /// Generation of the utterance being played, or 0.
    playing: Arc<AtomicU64>,
}

impl Speaker {
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Acquire) != 0
    }

    /// Stop the current utterance (if any).
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Synthesize and play `text`, blocking until playback ends or is stopped.
    /// With `save_to` (an `.mp3` path), the synthesized audio is also saved
    /// (whatever was synthesized before a stop), see [`save_spoken_audio`].
    pub fn speak(
        &self,
        api_key: String,
        voice: Voice,
        text: String,
        output_device: Option<String>,
        volume: f32,
        save_to: Option<std::path::PathBuf>,
    ) -> Result<(), String> {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let pieces = split_text(&text);
        if pieces.is_empty() {
            return Ok(());
        }
        info!(
            "TTS: {} chars in {} request(s) with {} / {}",
            text.chars().count(),
            pieces.len(),
            voice.model,
            voice.name
        );

        let stream = open_output(output_device).map_err(|e| format!("Audio output: {}", e))?;
        let sink = rodio::Sink::connect_new(stream.mixer());
        sink.set_volume(volume);
        self.playing.store(generation, Ordering::Release);
        let _playing = PlayingGuard(Arc::clone(&self.playing), generation);

        // Synthesis runs ahead on its own thread; playback appends in order.
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Pcm, String>>(2);
        std::thread::spawn(move || {
            super::block_on(async move {
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(Duration::from_secs(60))
                    .build()
                    .unwrap_or_default();
                for piece in pieces {
                    let result = synthesize(&client, &api_key, &voice, &piece).await;
                    let failed = result.is_err();
                    if tx.send(result).is_err() || failed {
                        break;
                    }
                }
            })
        });

        let current = || self.generation.load(Ordering::Acquire) == generation;
        let mut saved: Vec<f32> = Vec::new();
        let mut saved_rate = 0;
        let mut failure = None;
        for result in rx {
            if !current() {
                break;
            }
            let pcm = match result {
                Ok(pcm) => pcm,
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            };
            debug!(
                "TTS: queued {:.1}s of audio",
                pcm.samples.len() as f32 / pcm.sample_rate as f32
            );
            if save_to.is_some() && (saved_rate == 0 || saved_rate == pcm.sample_rate) {
                saved_rate = pcm.sample_rate;
                saved.extend_from_slice(&pcm.samples);
            }
            sink.append(rodio::buffer::SamplesBuffer::new(
                1,
                pcm.sample_rate,
                pcm.samples,
            ));
        }
        if let Some(path) = save_to.filter(|_| !saved.is_empty()) {
            match save_spoken_audio(&path, saved_rate, &saved) {
                Ok(written) => info!("TTS: saved {}", written.display()),
                Err(e) => warn!("TTS: could not save audio: {}", e),
            }
        }
        if let Some(e) = failure {
            return Err(e);
        }
        while current() && !sink.empty() {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !current() {
            sink.stop();
        }
        Ok(())
    }
}

struct PlayingGuard(Arc<AtomicU64>, u64);

impl Drop for PlayingGuard {
    fn drop(&mut self) {
        // Only clear our own flag: a newer utterance may already be playing.
        let _ = self
            .0
            .compare_exchange(self.1, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

fn open_output(
    device_name: Option<String>,
) -> Result<rodio::OutputStream, Box<dyn std::error::Error>> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let builder = match device_name.filter(|d| d != "Default") {
        Some(name) => {
            let host = crate::audio_toolkit::get_cpal_host();
            let found = host
                .output_devices()?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false));
            match found {
                Some(device) => rodio::OutputStreamBuilder::from_device(device)?,
                None => {
                    warn!("Output device '{}' not found, using default", name);
                    rodio::OutputStreamBuilder::from_default_device()?
                }
            }
        }
        None => rodio::OutputStreamBuilder::from_default_device()?,
    };
    Ok(builder.open_stream()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_id_suffix_follows_model() {
        assert_eq!(
            voice_id("ru-RU-Masha", "microsoft/mai-voice-2.1-flash"),
            "ru-RU-Masha:MAI-Voice-2.1-Flash"
        );
        assert_eq!(
            voice_id("ru-RU-Lev", "microsoft/mai-voice-2.1"),
            "ru-RU-Lev:MAI-Voice-2.1"
        );
        assert_eq!(voice_id("x:custom", "anything"), "x:custom");
    }

    #[test]
    fn split_text_respects_limit_and_keeps_words() {
        let text = "Первое предложение. ".repeat(60);
        let pieces = split_text(&text);
        assert!(pieces.len() > 1);
        assert!(pieces
            .iter()
            .all(|p| p.chars().count() <= MAX_CHARS_PER_REQUEST));
        assert_eq!(
            pieces.join(" ").split_whitespace().count(),
            text.split_whitespace().count()
        );
        assert_eq!(split_text("  "), Vec::<String>::new());
        assert_eq!(split_text("Hi"), vec!["Hi".to_string()]);
    }

    fn voice(speed: f32) -> Voice {
        Voice {
            model: DEFAULT_TTS_MODEL.to_string(),
            name: "ru-RU-Lev".to_string(),
            speed,
        }
    }

    #[test]
    fn speed_is_sent_only_when_not_normal_and_is_clamped() {
        assert!(speech_request(&voice(1.0), "x").get("speed").is_none());
        assert_eq!(speech_request(&voice(1.5), "x")["speed"], json!(1.5));
        assert_eq!(speech_request(&voice(3.0), "x")["speed"], json!(2.0));
        assert_eq!(
            speech_request(&voice(1.0), "x")["voice"],
            json!("ru-RU-Lev:MAI-Voice-2.1-Flash")
        );
    }

    #[test]
    fn saved_wav_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("tts.wav");
        save_wav(&path, 24_000, &[0.0, 0.5, -0.5, 1.0]).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 24_000);
        assert_eq!(reader.len(), 4);
    }

    #[test]
    fn spoken_audio_is_saved_as_mp3_with_ffmpeg() {
        let Some(ffmpeg) = crate::file_transcription::ffmpeg::find() else {
            eprintln!("ffmpeg not installed; skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tts.mp3");
        let tone: Vec<f32> = (0..24_000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 24_000.0).sin() * 0.3)
            .collect();
        encode_mp3(&ffmpeg, &path, 24_000, &tone).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // ID3 tag or a bare MPEG frame sync.
        assert!(bytes.starts_with(b"ID3") || (bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0));
        assert!(!dir.path().join("tts.mp3.part").exists());
    }

    #[test]
    fn failed_mp3_encoding_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tts.mp3");
        let missing = dir.path().join("no-ffmpeg");
        assert!(encode_mp3(&missing, &path, 24_000, &[0.0; 10]).is_err());
        assert!(!path.exists());
        assert!(!dir.path().join("tts.mp3.part").exists());
    }

    #[test]
    fn content_type_rate() {
        assert_eq!(
            rate_from_content_type("audio/pcm;rate=24000;channels=1"),
            24000
        );
        assert_eq!(rate_from_content_type("audio/pcm; rate=16000"), 16000);
        assert_eq!(rate_from_content_type("audio/pcm"), 24000);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// `OPENROUTER_API_KEY=… cargo test --lib live_tts -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_tts_returns_pcm_at_reported_rate() {
        let key = std::env::var("OPENROUTER_API_KEY").expect("set OPENROUTER_API_KEY");
        let pcm = crate::cloud::block_on(async move {
            let client = reqwest::Client::new();
            let voice = Voice {
                model: DEFAULT_TTS_MODEL.to_string(),
                name: "ru-RU-Lev".to_string(),
                speed: 1.0,
            };
            synthesize(&client, &key, &voice, "Проверка синтеза речи.").await
        })
        .unwrap();
        let secs = pcm.samples.len() as f32 / pcm.sample_rate as f32;
        println!(
            "{} samples @ {} Hz = {:.2}s",
            pcm.samples.len(),
            pcm.sample_rate,
            secs
        );
        assert_eq!(pcm.sample_rate, 24_000);
        assert!((0.8..10.0).contains(&secs));
        assert!(
            pcm.samples.iter().any(|s| s.abs() > 0.05),
            "audio is not silent"
        );
    }
}
