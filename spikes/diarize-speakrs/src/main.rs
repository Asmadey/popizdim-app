//! Phase 1 spike: run speakrs (pyannote community-1, CoreML) on a 16 kHz mono
//! WAV and report speed and speaker turns.
//!
//! Usage: diarize-speakrs-spike <models_dir> <audio.wav> [ahc_threshold]

use std::path::PathBuf;
use std::time::Instant;

use speakrs::{AhcConfig, ExecutionMode, PipelineBuilder, PipelineConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(models_dir), Some(wav_path)) = (args.next(), args.next()) else {
        eprintln!("usage: diarize-speakrs-spike <models_dir> <audio.wav>");
        std::process::exit(2);
    };
    let models_dir = PathBuf::from(models_dir);

    let mut reader = hound::WavReader::open(&wav_path)?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 {
        return Err(format!(
            "expected 16 kHz mono WAV, got {} Hz / {} ch",
            spec.sample_rate, spec.channels
        )
        .into());
    }
    let audio: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<_, _>>()?,
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
    };
    let audio_secs = audio.len() as f64 / 16_000.0;

    // Optional third argument: AHC threshold (speakrs default 0.6; lower splits more).
    let mut config = PipelineConfig::for_mode(ExecutionMode::CoreMl);
    if let Some(threshold) = args.next() {
        config.clustering = config
            .clustering
            .with_ahc(AhcConfig::new(threshold.parse()?)?);
    }

    let load_start = Instant::now();
    let mut pipeline = PipelineBuilder::from_dir(&models_dir, ExecutionMode::CoreMl)?
        .pipeline(config)
        .build()?;
    let load_secs = load_start.elapsed().as_secs_f64();

    let run_start = Instant::now();
    let result = pipeline.run(&audio)?;
    let run_secs = run_start.elapsed().as_secs_f64();

    let mut speakers: Vec<&str> = result
        .exclusive_segments
        .iter()
        .map(|s| s.speaker.as_str())
        .collect();
    speakers.sort_unstable();
    speakers.dedup();

    println!("audio: {audio_secs:.1} s");
    println!("load: {load_secs:.2} s");
    println!(
        "diarize: {run_secs:.2} s ({:.0}x realtime)",
        audio_secs / run_secs
    );
    println!("speakers: {} {:?}", speakers.len(), speakers);
    println!("turns (exclusive):");
    for seg in &result.exclusive_segments {
        println!("  {:7.2} - {:7.2}  {}", seg.start, seg.end, seg.speaker);
    }
    Ok(())
}
