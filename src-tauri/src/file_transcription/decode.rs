//! Decode a picked media file to the 16 kHz mono f32 samples the
//! transcription pipeline expects.
//!
//! Symphonia handles the common containers and codecs in-process (MP3, AAC in
//! MP4/M4A/MOV, FLAC, Vorbis, WAV, AIFF, CAF, ALAC, MKV with supported codecs).
//! Anything it cannot read — Opus, AC-3, DTS, WMA, ... — goes through the
//! system ffmpeg when one is installed.

use super::ffmpeg;
use anyhow::{anyhow, bail, Context};
use log::{debug, warn};
use rubato::{FftFixedIn, Resampler};
use std::fmt;
use std::fs::File;
use std::ops::ControlFlow;
use std::path::Path;
use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Sample rate of everything this module returns.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Input chunk size for the FFT resampler.
const RESAMPLE_CHUNK: usize = 1024;

/// Progress callback: receives the decoded fraction (0.0-1.0) when the total
/// length is known, `None` otherwise. Returning `Break` cancels decoding.
pub type Progress<'a> = &'a mut dyn FnMut(Option<f32>) -> ControlFlow<()>;

#[derive(Debug)]
pub enum DecodeError {
    /// Symphonia could not read the file and no ffmpeg binary was found to
    /// fall back to. `reason` is symphonia's error.
    FfmpegNotFound { reason: String },
    /// Neither decoder produced audio (unreadable file, no audio track, ...).
    Failed(String),
    /// The progress callback asked to stop.
    Cancelled,
}

/// Marker carried through `anyhow` when the progress callback cancels, so a
/// cancelled symphonia run does not fall back to ffmpeg.
#[derive(Debug)]
pub(super) struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::FfmpegNotFound { reason } => write!(
                f,
                "this format needs ffmpeg, which was not found (install it with `brew install ffmpeg`): {reason}"
            ),
            DecodeError::Failed(reason) => write!(f, "could not decode audio: {reason}"),
            DecodeError::Cancelled => f.write_str("decoding cancelled"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes the first audio track of `path` to 16 kHz mono.
pub fn decode_file(path: &Path) -> Result<Vec<f32>, DecodeError> {
    decode_file_with(path, &mut |_| ControlFlow::Continue(()))
}

/// [`decode_file`] with progress reporting and cancellation.
pub fn decode_file_with(path: &Path, progress: Progress) -> Result<Vec<f32>, DecodeError> {
    let symphonia_error = match decode_with_symphonia(path, progress) {
        Ok(samples) => return Ok(samples),
        Err(e) if e.is::<Cancelled>() => return Err(DecodeError::Cancelled),
        Err(e) => e,
    };
    debug!(
        "Symphonia could not decode {}: {:#}; trying ffmpeg",
        path.display(),
        symphonia_error
    );

    let Some(ffmpeg) = ffmpeg::find() else {
        return Err(DecodeError::FfmpegNotFound {
            reason: format!("{symphonia_error:#}"),
        });
    };
    ffmpeg::decode(&ffmpeg, path, progress).map_err(|e| {
        if e.is::<Cancelled>() {
            DecodeError::Cancelled
        } else {
            DecodeError::Failed(format!("{e:#}"))
        }
    })
}

fn decode_with_symphonia(path: &Path, progress: Progress) -> anyhow::Result<Vec<f32>> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    // Video tracks show up with a null codec or without a sample rate.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let params = &track.codec_params;
    let source_rate = params.sample_rate.unwrap_or(TARGET_SAMPLE_RATE).max(1) as f64;
    let time_base = params.time_base;
    // Track length, when the container states it (MP4, WAV, FLAC, ...).
    let total_secs = params
        .n_frames
        .map(|frames| frames as f64 / source_rate)
        .filter(|secs| *secs > 0.0);
    let mut decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;

    // Created from the first decoded packet's spec, which is authoritative
    // over the container's codec parameters.
    let mut resampler: Option<MonoResampler> = None;
    let mut sample_buf: Option<(SignalSpec, SampleBuffer<f32>)> = None;
    let mut mono = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            // A chained stream changed its parameters; keep what we have.
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }

        let position_secs = match time_base {
            Some(tb) => {
                let time = tb.calc_time(packet.ts());
                time.seconds as f64 + time.frac
            }
            None => packet.ts() as f64 / source_rate,
        };
        let fraction = total_secs.map(|total| (position_secs / total).clamp(0.0, 1.0) as f32);
        if progress(fraction).is_break() {
            return Err(Cancelled.into());
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A corrupt packet: skip it, as media players do.
            Err(SymphoniaError::DecodeError(reason)) => {
                warn!("Skipping undecodable packet: {}", reason);
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        let spec = *decoded.spec();
        let frames = decoded.capacity();
        let buf = match &mut sample_buf {
            Some((cached, buf)) if *cached == spec && buf.capacity() >= frames => buf,
            slot => {
                &mut slot
                    .insert((spec, SampleBuffer::new(frames as u64, spec)))
                    .1
            }
        };
        buf.copy_interleaved_ref(decoded);

        let channels = spec.channels.count().max(1);
        mono.clear();
        mono.extend(
            buf.samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );

        let resampler = match &mut resampler {
            Some(r) => r,
            slot => slot.insert(MonoResampler::new(spec.rate)?),
        };
        resampler.push(&mono)?;
    }

    let samples = match resampler {
        Some(r) => r.finish()?,
        None => Vec::new(),
    };
    if samples.is_empty() {
        bail!("no audio decoded");
    }
    Ok(samples)
}

/// Whole-stream mono resampler to [`TARGET_SAMPLE_RATE`]. Unlike the
/// recorder's frame resampler, the output is aligned to the input (the FFT
/// delay is trimmed) and has exactly `input_len * 16000 / in_rate` samples, so
/// timestamps computed from it match the file.
struct MonoResampler {
    inner: Option<FftFixedIn<f32>>,
    in_rate: u32,
    in_count: usize,
    in_buf: Vec<f32>,
    out: Vec<f32>,
}

impl MonoResampler {
    fn new(in_rate: u32) -> anyhow::Result<Self> {
        if in_rate == 0 {
            bail!("invalid sample rate 0");
        }
        let inner = if in_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            Some(FftFixedIn::<f32>::new(
                in_rate as usize,
                TARGET_SAMPLE_RATE as usize,
                RESAMPLE_CHUNK,
                1,
                1,
            )?)
        };
        Ok(Self {
            inner,
            in_rate,
            in_count: 0,
            in_buf: Vec::with_capacity(RESAMPLE_CHUNK),
            out: Vec::new(),
        })
    }

    fn push(&mut self, mut src: &[f32]) -> anyhow::Result<()> {
        self.in_count += src.len();
        let Some(inner) = self.inner.as_mut() else {
            self.out.extend_from_slice(src);
            return Ok(());
        };
        while !src.is_empty() {
            let take = (RESAMPLE_CHUNK - self.in_buf.len()).min(src.len());
            self.in_buf.extend_from_slice(&src[..take]);
            src = &src[take..];
            if self.in_buf.len() == RESAMPLE_CHUNK {
                let out = inner.process(&[&self.in_buf[..]], None)?;
                self.out.extend_from_slice(&out[0]);
                self.in_buf.clear();
            }
        }
        Ok(())
    }

    fn finish(mut self) -> anyhow::Result<Vec<f32>> {
        let Some(mut inner) = self.inner.take() else {
            return Ok(self.out);
        };
        if !self.in_buf.is_empty() {
            let out = inner.process_partial(Some(&[&self.in_buf[..]]), None)?;
            self.out.extend_from_slice(&out[0]);
        }

        let delay = inner.output_delay();
        let expected =
            (self.in_count as u64 * TARGET_SAMPLE_RATE as u64 / self.in_rate as u64) as usize;
        // The delay line still holds the last real samples; flush it with
        // silence until all of them are out.
        let mut rounds = 0;
        while self.out.len() < expected + delay && rounds < 8 {
            rounds += 1;
            let out = inner.process_partial::<&[f32]>(None, None)?;
            if out[0].is_empty() {
                break;
            }
            self.out.extend_from_slice(&out[0]);
        }

        let mut out = self.out;
        out.drain(..delay.min(out.len()));
        out.truncate(expected);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sine(rate: u32, freq: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// Positive-going zero crossings per second ≈ frequency.
    fn crossings_per_sec(samples: &[f32], rate: u32) -> f32 {
        let n = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        n as f32 * rate as f32 / samples.len() as f32
    }

    fn resample_all(input: &[f32], rate: u32, push_size: usize) -> Vec<f32> {
        let mut r = MonoResampler::new(rate).unwrap();
        for chunk in input.chunks(push_size) {
            r.push(chunk).unwrap();
        }
        r.finish().unwrap()
    }

    #[test]
    fn resampler_output_has_exact_length_and_keeps_the_signal() {
        for rate in [48_000, 44_100, 22_050, 8_000] {
            let input = sine(rate, 440.0, 1.3);
            let out = resample_all(&input, rate, 777);
            let expected = input.len() * 16_000 / rate as usize;
            assert_eq!(out.len(), expected, "length at {rate} Hz");
            assert!((rms(&out) - rms(&input)).abs() < 0.02, "level at {rate} Hz");
            assert!(
                (crossings_per_sec(&out, 16_000) - 440.0).abs() < 5.0,
                "pitch at {rate} Hz"
            );
        }
    }

    #[test]
    fn resampler_is_aligned_with_the_input() {
        // Silence then a burst at exactly 0.5 s: the burst must start at
        // sample 8000 of the output, not shifted by the FFT delay.
        let rate = 48_000;
        let mut input = vec![0.0f32; rate as usize];
        input[rate as usize / 2..].fill(0.5);
        let out = resample_all(&input, rate, 4096);
        let onset = out.iter().position(|s| *s > 0.25).unwrap();
        assert!((onset as i64 - 8000).abs() <= 4, "onset at {onset}");
    }

    #[test]
    fn resampler_passes_16k_through() {
        let input = sine(16_000, 300.0, 0.5);
        assert_eq!(resample_all(&input, 16_000, 100), input);
    }

    #[test]
    fn decodes_stereo_44k_wav_to_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stereo.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in sine(44_100, 440.0, 2.0) {
            let v = (s * i16::MAX as f32) as i16;
            writer.write_sample(v).unwrap();
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();

        let out = decode_file(&path).unwrap();
        assert_eq!(out.len(), 32_000);
        assert!((crossings_per_sec(&out, 16_000) - 440.0).abs() < 5.0);
        assert!((rms(&out) - 0.5 / 2f32.sqrt()).abs() < 0.02);
    }

    fn write_mono_wav(dir: &Path, secs: f32) -> std::path::PathBuf {
        let path = dir.join("mono.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in sine(44_100, 440.0, secs) {
            writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    fn collect_progress(path: &Path) -> Vec<Option<f32>> {
        let mut seen = Vec::new();
        decode_file_with(path, &mut |fraction| {
            seen.push(fraction);
            ControlFlow::Continue(())
        })
        .unwrap();
        seen
    }

    fn assert_progress_runs_to_the_end(seen: &[Option<f32>], what: &str) {
        let fractions: Vec<f32> = seen.iter().map(|f| f.expect("known length")).collect();
        assert!(
            fractions.windows(2).all(|w| w[0] <= w[1]),
            "{what}: not monotonic"
        );
        let last = *fractions.last().expect("at least one report");
        assert!(last >= 0.9, "{what}: ends at {last}");
    }

    #[test]
    fn reports_progress_while_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_mono_wav(dir.path(), 3.0);
        assert_progress_runs_to_the_end(&collect_progress(&path), "wav");
    }

    #[test]
    fn progress_callback_cancels_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_mono_wav(dir.path(), 3.0);
        let result = decode_file_with(&path, &mut |_| ControlFlow::Break(()));
        assert!(matches!(result, Err(DecodeError::Cancelled)));
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(decode_file(Path::new("/nonexistent/recording.mp3")).is_err());
    }

    /// Generates a 2 s, 440 Hz test file with the system ffmpeg; `None` when
    /// ffmpeg is not installed (the test is then skipped).
    fn ffmpeg_fixture(dir: &Path, name: &str, codec_args: &[&str]) -> Option<std::path::PathBuf> {
        let ffmpeg = ffmpeg::find()?;
        let path = dir.join(name);
        let status = Command::new(ffmpeg)
            .args(["-nostdin", "-loglevel", "error", "-y"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
            ])
            .args(codec_args)
            .arg(&path)
            .status()
            .ok()?;
        status.success().then_some(path)
    }

    fn assert_two_seconds_of_440(out: &[f32], what: &str) {
        let secs = out.len() as f32 / 16_000.0;
        assert!((secs - 2.0).abs() < 0.1, "{what}: {secs} s");
        assert!(
            (crossings_per_sec(out, 16_000) - 440.0).abs() < 10.0,
            "{what}: pitch"
        );
    }

    #[test]
    fn decodes_aac_in_mp4_video_container_with_symphonia() {
        let dir = tempfile::tempdir().unwrap();
        let Some(path) = ffmpeg_fixture(
            dir.path(),
            "clip.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:d=2",
                "-shortest",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
            ],
        ) else {
            eprintln!("ffmpeg not installed; skipping");
            return;
        };
        let out = decode_with_symphonia(&path, &mut |_| ControlFlow::Continue(()))
            .expect("symphonia should read AAC in MP4");
        assert_two_seconds_of_440(&out, "mp4/aac");
    }

    #[test]
    fn opus_in_mkv_falls_back_to_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        let Some(path) = ffmpeg_fixture(dir.path(), "clip.mkv", &["-c:a", "libopus"]) else {
            eprintln!("ffmpeg (with libopus) not installed; skipping");
            return;
        };
        assert!(
            decode_with_symphonia(&path, &mut |_| ControlFlow::Continue(())).is_err(),
            "symphonia 0.5 has no Opus decoder"
        );
        let out = decode_file(&path).unwrap();
        assert_two_seconds_of_440(&out, "mkv/opus");

        // ffprobe ships with ffmpeg; without it the length is unknown.
        let seen = collect_progress(&path);
        if seen.iter().all(Option::is_some) {
            assert_progress_runs_to_the_end(&seen, "mkv/opus via ffmpeg");
        }
        let result = decode_file_with(&path, &mut |_| ControlFlow::Break(()));
        assert!(matches!(result, Err(DecodeError::Cancelled)));
    }
}
