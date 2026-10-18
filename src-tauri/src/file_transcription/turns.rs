//! Speaker turns: diarization segments → ranges to transcribe → labelled text.
//!
//! Diarization runs first and the active model then transcribes each turn, so
//! any model works, including cloud ones that return no timestamps. Segments
//! are cleaned up before that: same-speaker segments separated by a short
//! pause are merged, blips too short to hold a word join a neighbouring turn,
//! and turns get a little padding (never into a neighbour) so word edges are
//! not clipped.

use super::decode::TARGET_SAMPLE_RATE;
use std::collections::HashMap;
use std::ops::Range;

/// Same-speaker segments closer than this are one turn.
const MERGE_GAP_SECS: f64 = 1.0;

/// Shorter segments are too short for a word on their own: they join the
/// neighbouring turn (if it is close) or are dropped.
const MIN_TURN_SECS: f64 = 0.3;

/// Audio added before and after a turn, limited by the neighbouring turns.
const PAD_SECS: f64 = 0.25;

/// One diarization segment, as the diarizer reports it.
#[derive(Clone, Debug)]
pub struct SpeakerSegment {
    pub start_secs: f64,
    pub end_secs: f64,
    pub speaker: String,
}

/// A turn to transcribe: a sample range and the speaker's number (0-based,
/// in order of first appearance).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub range: Range<usize>,
    pub speaker: usize,
}

fn to_samples(secs: f64) -> usize {
    (secs.max(0.0) * TARGET_SAMPLE_RATE as f64).round() as usize
}

/// Cleans up diarization segments into turns over audio of `total_len`
/// samples. Speakers are renumbered in order of first appearance.
pub fn speaker_turns(segments: &[SpeakerSegment], total_len: usize) -> Vec<Turn> {
    let merge_gap = to_samples(MERGE_GAP_SECS);
    let min_len = to_samples(MIN_TURN_SECS);
    let pad = to_samples(PAD_SECS);

    let mut sorted: Vec<&SpeakerSegment> = segments.iter().collect();
    sorted.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));

    // Sample ranges, clipped to the audio and to the previous segment (the
    // diarizer's exclusive segments don't overlap; this keeps it that way).
    let mut ids: HashMap<&str, usize> = HashMap::new();
    let mut raw: Vec<Turn> = Vec::new();
    let mut cursor = 0;
    for seg in sorted {
        let start = to_samples(seg.start_secs).max(cursor);
        let end = to_samples(seg.end_secs).min(total_len);
        if end <= start {
            continue;
        }
        cursor = end;
        let next_id = ids.len();
        let speaker = *ids.entry(seg.speaker.as_str()).or_insert(next_id);
        raw.push(Turn {
            range: start..end,
            speaker,
        });
    }

    let mut turns = merge_same_speaker(raw, merge_gap);

    // Fold blips into a close neighbour, preferring the previous turn.
    let mut kept: Vec<Turn> = Vec::with_capacity(turns.len());
    for i in 0..turns.len() {
        let turn = turns[i].clone();
        if turn.range.len() < min_len {
            if let Some(prev) = kept.last_mut() {
                if turn.range.start.saturating_sub(prev.range.end) <= merge_gap {
                    prev.range.end = turn.range.end;
                    continue;
                }
            }
            if let Some(next) = turns.get_mut(i + 1) {
                if next.range.start.saturating_sub(turn.range.end) <= merge_gap {
                    next.range.start = turn.range.start;
                    continue;
                }
            }
            continue;
        }
        kept.push(turn);
    }
    let mut turns = merge_same_speaker(kept, merge_gap);

    // Pad into the silence around each turn; neighbours split the pause
    // between them, so turns never overlap.
    let bounds: Vec<Range<usize>> = turns.iter().map(|t| t.range.clone()).collect();
    let midpoint = |left: usize, right: usize| left + right.saturating_sub(left) / 2;
    for (i, turn) in turns.iter_mut().enumerate() {
        let floor = if i > 0 {
            midpoint(bounds[i - 1].end, bounds[i].start)
        } else {
            0
        };
        let ceiling = bounds
            .get(i + 1)
            .map_or(total_len, |next| midpoint(bounds[i].end, next.start));
        turn.range.start = turn.range.start.saturating_sub(pad).max(floor);
        turn.range.end = (turn.range.end + pad).min(ceiling);
    }

    // Speakers who only had blips are gone: renumber the rest.
    let mut renumber: HashMap<usize, usize> = HashMap::new();
    for turn in &mut turns {
        let next_id = renumber.len();
        turn.speaker = *renumber.entry(turn.speaker).or_insert(next_id);
    }
    turns
}

/// Merges neighbouring turns of one speaker separated by at most `gap`.
fn merge_same_speaker(turns: Vec<Turn>, gap: usize) -> Vec<Turn> {
    let mut merged: Vec<Turn> = Vec::with_capacity(turns.len());
    for turn in turns {
        match merged.last_mut() {
            Some(last)
                if last.speaker == turn.speaker
                    && turn.range.start.saturating_sub(last.range.end) <= gap =>
            {
                last.range.end = turn.range.end;
            }
            _ => merged.push(turn),
        }
    }
    merged
}

/// Number of distinct speakers in `turns`.
pub fn speaker_count(turns: &[Turn]) -> usize {
    turns.iter().map(|t| t.speaker + 1).max().unwrap_or(0)
}

/// A transcribed turn: where it starts (samples), who speaks, what was said.
pub struct TurnText {
    pub start: usize,
    pub speaker: usize,
    pub text: String,
}

/// Formats transcribed turns as paragraphs `[00:01:23] Speaker 1: text`.
/// Consecutive turns of one speaker share a paragraph; empty turns are
/// skipped. `label` holds the localized "Speaker {{n}}".
pub fn format_transcript(turns: &[TurnText], label: &str) -> String {
    let mut paragraphs: Vec<(usize, usize, Vec<&str>)> = Vec::new();
    for turn in turns {
        let text = turn.text.trim();
        if text.is_empty() {
            continue;
        }
        match paragraphs.last_mut() {
            Some((_, speaker, texts)) if *speaker == turn.speaker => texts.push(text),
            _ => paragraphs.push((turn.start, turn.speaker, vec![text])),
        }
    }
    paragraphs
        .into_iter()
        .map(|(start, speaker, texts)| {
            format!(
                "[{}] {}: {}",
                timestamp(start),
                label.replace("{{n}}", &(speaker + 1).to_string()),
                crate::cloud::audio::join_phrases(texts)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `HH:MM:SS` for a sample offset.
fn timestamp(samples: usize) -> String {
    let secs = samples / TARGET_SAMPLE_RATE as usize;
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = TARGET_SAMPLE_RATE as usize;

    fn seg(start: f64, end: f64, speaker: &str) -> SpeakerSegment {
        SpeakerSegment {
            start_secs: start,
            end_secs: end,
            speaker: speaker.to_string(),
        }
    }

    fn secs(samples: usize) -> f64 {
        samples as f64 / SR as f64
    }

    /// (start, end, speaker) in seconds, rounded to 0.01 s.
    fn summary(turns: &[Turn]) -> Vec<(f64, f64, usize)> {
        let round = |s: usize| (secs(s) * 100.0).round() / 100.0;
        turns
            .iter()
            .map(|t| (round(t.range.start), round(t.range.end), t.speaker))
            .collect()
    }

    #[test]
    fn speakers_are_numbered_by_first_appearance() {
        let segments = [
            seg(0.0, 3.0, "SPEAKER_01"),
            seg(4.0, 7.0, "SPEAKER_00"),
            seg(8.0, 10.0, "SPEAKER_01"),
        ];
        let turns = speaker_turns(&segments, 12 * SR);
        let speakers: Vec<usize> = turns.iter().map(|t| t.speaker).collect();
        assert_eq!(speakers, vec![0, 1, 0]);
        assert_eq!(speaker_count(&turns), 2);
    }

    #[test]
    fn close_segments_of_one_speaker_merge() {
        // 0.5 s pause merges; 2 s pause does not.
        let segments = [seg(0.0, 2.0, "A"), seg(2.5, 4.0, "A"), seg(6.0, 8.0, "A")];
        let turns = speaker_turns(&segments, 10 * SR);
        assert_eq!(summary(&turns), vec![(0.0, 4.25, 0), (5.75, 8.25, 0)]);
    }

    #[test]
    fn padding_stops_at_neighbours_and_audio_edges() {
        // A and B touch: no padding between them. B and the second A split
        // their 0.1 s pause. Audio edges are clamped.
        let segments = [seg(0.1, 3.0, "A"), seg(3.0, 5.0, "B"), seg(5.1, 6.0, "A")];
        let turns = speaker_turns(&segments, 6 * SR);
        assert_eq!(
            summary(&turns),
            vec![(0.0, 3.0, 0), (3.0, 5.05, 1), (5.05, 6.0, 0)]
        );
        assert!(turns.windows(2).all(|w| w[0].range.end <= w[1].range.start));
    }

    #[test]
    fn blips_join_the_neighbour_and_vanish_as_speakers() {
        // B's 0.2 s blip inside A's speech is folded into A: one turn, and B
        // (who only had the blip) is not counted.
        let segments = [seg(0.0, 3.0, "A"), seg(3.1, 3.3, "B"), seg(3.4, 6.0, "A")];
        let turns = speaker_turns(&segments, 7 * SR);
        assert_eq!(summary(&turns), vec![(0.0, 6.25, 0)]);
        assert_eq!(speaker_count(&turns), 1);
    }

    #[test]
    fn leading_blip_joins_the_next_turn_isolated_blip_is_dropped() {
        let segments = [
            seg(1.0, 1.2, "B"),
            seg(1.5, 4.0, "A"),
            seg(8.0, 8.1, "C"),
            seg(12.0, 14.0, "B"),
        ];
        let turns = speaker_turns(&segments, 15 * SR);
        // B's leading blip extends A's turn back to 1.0 s; C's blip, 4 s from
        // anything, is dropped. Renumbered: A → 0, B → 1.
        assert_eq!(summary(&turns), vec![(0.75, 4.25, 0), (11.75, 14.25, 1)]);
    }

    #[test]
    fn overlapping_or_out_of_range_segments_are_clipped() {
        let segments = [
            seg(0.0, 3.0, "A"),
            seg(2.0, 5.0, "B"),   // overlaps A
            seg(-1.0, -0.5, "C"), // before the audio
            seg(9.0, 20.0, "A"),  // past the end
        ];
        let turns = speaker_turns(&segments, 10 * SR);
        assert_eq!(
            summary(&turns),
            vec![(0.0, 3.0, 0), (3.0, 5.25, 1), (8.75, 10.0, 0)]
        );
    }

    #[test]
    fn no_segments_no_turns() {
        assert!(speaker_turns(&[], 10 * SR).is_empty());
        assert_eq!(speaker_count(&[]), 0);
    }

    #[test]
    fn transcript_groups_consecutive_turns_and_skips_empty_ones() {
        let turns = [
            TurnText {
                start: 83 * SR,
                speaker: 0,
                text: "Привет.".into(),
            },
            TurnText {
                start: 86 * SR,
                speaker: 1,
                text: "  ".into(),
            },
            TurnText {
                start: 90 * SR,
                speaker: 0,
                text: "Как дела?".into(),
            },
            TurnText {
                start: 3725 * SR,
                speaker: 1,
                text: "Хорошо.".into(),
            },
        ];
        assert_eq!(
            format_transcript(&turns, "Спикер {{n}}"),
            "[00:01:23] Спикер 1: Привет. Как дела?\n\n[01:02:05] Спикер 2: Хорошо."
        );
    }

    #[test]
    fn empty_transcript_is_empty() {
        assert_eq!(format_transcript(&[], "Speaker {{n}}"), "");
    }
}
