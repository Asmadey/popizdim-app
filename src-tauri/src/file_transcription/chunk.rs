//! Split a decoded file into transcription-sized chunks at pauses.
//!
//! Voice activity detection marks speech frames; speech is padded, nearby
//! regions are merged, and the regions are packed greedily into chunks of at
//! most [`MAX_CHUNK_SECS`]. Long silences between chunks are never sent to the
//! model (no compute spent on them, no hallucinated text). A single region
//! longer than the limit is cut at its quietest point.

use super::decode::TARGET_SAMPLE_RATE;
use crate::audio_toolkit::VoiceActivityDetector;
use crate::cloud::audio::quietest_cut;
use std::ops::{ControlFlow, Range};

/// Upper bound for one model call. GigaAM was trained on ~25 s windows and
/// warns beyond that; Whisper's window is 30 s.
pub const MAX_CHUNK_SECS: f32 = 25.0;

/// Audio kept before and after detected speech so word edges aren't clipped.
/// Regions closer than twice this are merged.
const PAD_SECS: f32 = 0.3;

/// Isolated speech blips shorter than this are treated as noise.
const MIN_SPEECH_SECS: f32 = 0.09;

/// How far back from the limit an over-long region looks for a pause to cut.
const CUT_SEARCH_SECS: f32 = 5.0;

fn secs_to_samples(secs: f32) -> usize {
    (secs * TARGET_SAMPLE_RATE as f32) as usize
}

/// Runs `vad` over `samples` frame by frame. `progress` gets the fraction
/// done and may cancel (`Break` → `None`).
pub fn detect_speech(
    samples: &[f32],
    vad: &mut dyn VoiceActivityDetector,
    progress: &mut dyn FnMut(f32) -> ControlFlow<()>,
) -> anyhow::Result<Option<Vec<bool>>> {
    let frame = vad.frame_samples();
    vad.reset();
    let total = samples.len().div_ceil(frame).max(1);
    let mut flags = Vec::with_capacity(total);
    let mut padded = vec![0.0f32; frame];
    for (i, chunk) in samples.chunks(frame).enumerate() {
        let speech = if chunk.len() == frame {
            vad.is_voice(chunk)?
        } else {
            padded[..chunk.len()].copy_from_slice(chunk);
            padded[chunk.len()..].fill(0.0);
            vad.is_voice(&padded)?
        };
        flags.push(speech);
        // Report every ~1 s of audio.
        if i % 32 == 0 && progress(i as f32 / total as f32).is_break() {
            return Ok(None);
        }
    }
    Ok(Some(flags))
}

/// Sample ranges of speech: runs of speech frames, padded, clamped to the
/// audio and merged where they overlap.
pub fn speech_regions(flags: &[bool], frame: usize, total_len: usize) -> Vec<Range<usize>> {
    let pad = secs_to_samples(PAD_SECS);
    let min_frames = (secs_to_samples(MIN_SPEECH_SECS) / frame.max(1)).max(1);

    let mut regions: Vec<Range<usize>> = Vec::new();
    let mut i = 0;
    while i < flags.len() {
        if !flags[i] {
            i += 1;
            continue;
        }
        let run_start = i;
        while i < flags.len() && flags[i] {
            i += 1;
        }
        if i - run_start < min_frames {
            continue;
        }
        let start = (run_start * frame).saturating_sub(pad);
        let end = (i * frame + pad).min(total_len);
        match regions.last_mut() {
            Some(last) if start <= last.end => last.end = last.end.max(end),
            _ => regions.push(start..end),
        }
    }
    regions
}

/// Packs speech regions into chunks no longer than `max_len` samples.
pub fn pack_chunks(regions: &[Range<usize>], samples: &[f32], max_len: usize) -> Vec<Range<usize>> {
    let search = secs_to_samples(CUT_SEARCH_SECS);
    let mut chunks: Vec<Range<usize>> = Vec::new();
    let mut current: Option<Range<usize>> = None;

    for region in regions {
        // Over-long speech: flush, then cut it into pieces at quiet points.
        if region.len() > max_len {
            chunks.extend(current.take());
            let mut start = region.start;
            while region.end - start > max_len {
                let limit = start + max_len;
                let cut = quietest_cut(samples, limit.saturating_sub(search).max(start + 1), limit);
                chunks.push(start..cut);
                start = cut;
            }
            current = Some(start..region.end);
            continue;
        }
        current = match current.take() {
            Some(chunk) if region.end - chunk.start <= max_len => Some(chunk.start..region.end),
            Some(chunk) => {
                chunks.push(chunk);
                Some(region.clone())
            }
            None => Some(region.clone()),
        };
    }
    chunks.extend(current);
    chunks
}

/// Fallback when no VAD is available: fixed-size chunks cut at quiet points.
pub fn split_without_vad(samples: &[f32], max_len: usize) -> Vec<Range<usize>> {
    pack_chunks(std::slice::from_ref(&(0..samples.len())), samples, max_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: usize = 480; // 30 ms, as Silero

    fn flags(pattern: &[(bool, usize)]) -> Vec<bool> {
        pattern
            .iter()
            .flat_map(|&(speech, frames)| std::iter::repeat_n(speech, frames))
            .collect()
    }

    fn secs(samples: usize) -> f32 {
        samples as f32 / TARGET_SAMPLE_RATE as f32
    }

    #[test]
    fn regions_are_padded_and_clamped() {
        // 1 s silence, 1 s speech, 1 s silence.
        let f = flags(&[(false, 33), (true, 33), (false, 34)]);
        let total = f.len() * FRAME;
        let regions = speech_regions(&f, FRAME, total);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start, 33 * FRAME - secs_to_samples(PAD_SECS));
        assert_eq!(regions[0].end, 66 * FRAME + secs_to_samples(PAD_SECS));

        // Speech touching both edges stays inside the audio.
        let edge = flags(&[(true, 10)]);
        assert_eq!(
            speech_regions(&edge, FRAME, 10 * FRAME),
            vec![0..10 * FRAME]
        );
    }

    #[test]
    fn short_pauses_merge_long_pauses_split() {
        // Pause of 0.3 s (< 2 * pad) merges; pause of 2 s splits.
        let f = flags(&[(true, 30), (false, 10), (true, 30), (false, 66), (true, 30)]);
        let regions = speech_regions(&f, FRAME, f.len() * FRAME);
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn blips_are_ignored() {
        let f = flags(&[(false, 20), (true, 1), (false, 20)]);
        assert!(speech_regions(&f, FRAME, f.len() * FRAME).is_empty());
    }

    #[test]
    fn silence_has_no_chunks() {
        let samples = vec![0.0f32; 16_000 * 5];
        let f = vec![false; samples.len() / FRAME];
        let regions = speech_regions(&f, FRAME, samples.len());
        assert!(pack_chunks(&regions, &samples, secs_to_samples(MAX_CHUNK_SECS)).is_empty());
    }

    #[test]
    fn regions_pack_up_to_the_limit() {
        let max = secs_to_samples(10.0);
        let samples = vec![0.1f32; secs_to_samples(60.0)];
        // Regions of 4 s with 2 s gaps: two fit in 10 s, a third does not.
        let regions: Vec<Range<usize>> = (0..5)
            .map(|i| secs_to_samples(6.0 * i as f32)..secs_to_samples(6.0 * i as f32 + 4.0))
            .collect();
        let chunks = pack_chunks(&regions, &samples, max);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], regions[0].start..regions[1].end);
        assert!(chunks.iter().all(|c| c.len() <= max));
        // Every region is covered by some chunk.
        for r in &regions {
            assert!(chunks.iter().any(|c| c.start <= r.start && r.end <= c.end));
        }
    }

    #[test]
    fn long_region_is_cut_at_the_quietest_point() {
        let max = secs_to_samples(10.0);
        // 25 s of "speech" with a quiet dip at 8 s.
        let mut samples = vec![0.5f32; secs_to_samples(25.0)];
        let dip = secs_to_samples(8.0);
        samples[dip..dip + secs_to_samples(0.2)].fill(0.0);
        let chunks = pack_chunks(std::slice::from_ref(&(0..samples.len())), &samples, max);
        assert!(chunks.iter().all(|c| c.len() <= max), "{chunks:?}");
        assert!(
            (secs(chunks[0].end) - 8.1).abs() < 0.15,
            "cut at {}",
            secs(chunks[0].end)
        );
        // Contiguous and complete.
        assert_eq!(chunks.first().unwrap().start, 0);
        assert_eq!(chunks.last().unwrap().end, samples.len());
        assert!(chunks.windows(2).all(|w| w[0].end == w[1].start));
    }

    #[test]
    fn fallback_split_covers_everything() {
        let samples = vec![0.2f32; secs_to_samples(61.0)];
        let chunks = split_without_vad(&samples, secs_to_samples(MAX_CHUNK_SECS));
        assert_eq!(chunks.first().unwrap().start, 0);
        assert_eq!(chunks.last().unwrap().end, samples.len());
        assert!(chunks
            .iter()
            .all(|c| c.len() <= secs_to_samples(MAX_CHUNK_SECS)));
    }
}
