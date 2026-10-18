//! Audio helpers shared by the cloud providers. All input is Handy's internal
//! format: 16 kHz mono `f32` in `[-1, 1]`.

use std::collections::HashSet;
use std::io::Cursor;

pub const SAMPLE_RATE: u32 = 16_000;
/// 10 ms of audio at [`SAMPLE_RATE`].
pub const SAMPLES_PER_10MS: usize = (SAMPLE_RATE / 100) as usize;

/// Below this RMS a 10 ms window counts as silence. Speech typically sits well
/// above 0.02; room noise from a laptop mic around 0.002–0.005.
const SILENCE_RMS: f32 = 0.008;

/// Signed 16-bit little-endian PCM, the wire format both providers accept.
pub fn pcm16_le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// A complete in-memory WAV file (16 kHz, mono, 16-bit).
pub fn wav_bytes(samples: &[f32]) -> Result<Vec<u8>, hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for &s in samples {
            writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

pub fn seconds(samples: usize) -> f32 {
    samples as f32 / SAMPLE_RATE as f32
}

/// Tracks how long the tail of a live feed has been silent, so the streaming
/// workers can commit at natural pauses. With VAD enabled most silence never
/// arrives (the recorder drops it), which the workers detect separately as a
/// wall-clock gap between frames.
#[derive(Default)]
pub struct PauseTracker {
    pending: Vec<f32>,
    trailing_silence: usize,
    heard_speech: bool,
}

impl PauseTracker {
    pub fn push(&mut self, frame: &[f32]) {
        self.pending.extend_from_slice(frame);
        let whole = self.pending.len() / SAMPLES_PER_10MS * SAMPLES_PER_10MS;
        for window in self.pending[..whole].chunks(SAMPLES_PER_10MS) {
            if rms(window) < SILENCE_RMS {
                self.trailing_silence += window.len();
            } else {
                self.trailing_silence = 0;
                self.heard_speech = true;
            }
        }
        self.pending.drain(..whole);
    }

    /// Seconds of silence at the end of the feed.
    pub fn trailing_silence_secs(&self) -> f32 {
        seconds(self.trailing_silence)
    }

    /// Whether any non-silent audio arrived since the last [`Self::reset`].
    pub fn heard_speech(&self) -> bool {
        self.heard_speech
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.trailing_silence = 0;
        self.heard_speech = false;
    }
}

/// Index of the quietest 100 ms window inside `samples[from..to]`, used to cut
/// long audio between words instead of through one.
pub fn quietest_cut(samples: &[f32], from: usize, to: usize) -> usize {
    let window = SAMPLES_PER_10MS * 10;
    let to = to.min(samples.len());
    if to <= from + window {
        return to;
    }
    let mut best = to;
    let mut best_rms = f32::MAX;
    let mut start = from;
    while start + window <= to {
        let r = rms(&samples[start..start + window]);
        if r < best_rms {
            best_rms = r;
            best = start + window / 2;
        }
        start += SAMPLES_PER_10MS * 5;
    }
    best
}

/// Split audio into chunks no longer than `max_secs`, cutting at the quietest
/// point of each chunk's last few seconds. Short audio is returned whole.
pub fn split_for_upload(samples: &[f32], max_secs: f32) -> Vec<&[f32]> {
    let max = (max_secs * SAMPLE_RATE as f32) as usize;
    let search = (5.0 * SAMPLE_RATE as f32) as usize;
    let mut chunks = Vec::new();
    let mut start = 0;
    while samples.len() - start > max {
        let end = start + max;
        let cut = quietest_cut(samples, end.saturating_sub(search).max(start + 1), end);
        chunks.push(&samples[start..cut]);
        start = cut;
    }
    if start < samples.len() {
        chunks.push(&samples[start..]);
    }
    chunks
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0x0E00..=0x0E7F // Thai
        | 0x3000..=0x303F // CJK punctuation
        | 0xFF00..=0xFFEF) // Full-width forms
}

/// Join independently transcribed phrases.
///
/// Each phrase is transcribed on its own, so the model closes one cut at a
/// pause with "..." and capitalizes the next as if a sentence began: "сможет
/// создать только... Какое-то". When the previous phrase doesn't end a
/// sentence, the seam is repaired — the trailing ellipsis is dropped and the
/// next word lowercased, unless it looks like a name (see [`looks_like_name`]).
/// Scripts written without spaces (Chinese, Japanese, Thai) are concatenated
/// directly.
pub fn join_phrases<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let parts: Vec<&str> = parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let names = capitalized_mid_sentence(&parts);
    let mut out = String::new();
    for part in parts {
        let mut part = part.to_string();
        if !out.is_empty() {
            let had_ellipsis = strip_trailing_ellipsis(&mut out);
            // The model may mark the cut on the other side: "...моего".
            let had_leading_ellipsis = strip_leading_ellipsis(&mut part);
            let continues = had_ellipsis
                || had_leading_ellipsis
                || out.chars().last().is_some_and(|c| {
                    c.is_alphanumeric() || matches!(c, ',' | ';' | ':' | '—' | '-')
                });
            if continues {
                lowercase_continuation(&mut part, &out, &names);
            }
            let needs_space = match (out.chars().last(), part.chars().next()) {
                (Some(prev), Some(next)) => !(is_cjk(prev) && is_cjk(next)),
                _ => false,
            };
            if needs_space {
                out.push(' ');
            }
        }
        out.push_str(&part);
    }
    out
}

/// Remove a trailing "..." / "…" (and the whitespace around it). Returns
/// whether one was removed.
fn strip_trailing_ellipsis(text: &mut String) -> bool {
    let mut stripped = false;
    loop {
        let trimmed = text.trim_end();
        let rest = trimmed
            .strip_suffix("...")
            .or_else(|| trimmed.strip_suffix('…'));
        match rest {
            Some(rest) => {
                text.truncate(rest.len());
                stripped = true;
            }
            None => {
                text.truncate(trimmed.len());
                return stripped;
            }
        }
    }
}

/// Remove a leading "..." / "…" (and the whitespace after it). Returns
/// whether one was removed.
fn strip_leading_ellipsis(text: &mut String) -> bool {
    let mut stripped = false;
    loop {
        let trimmed = text.trim_start();
        let rest = trimmed
            .strip_prefix("...")
            .or_else(|| trimmed.strip_prefix('…'));
        match rest {
            Some(rest) => {
                *text = rest.to_string();
                stripped = true;
            }
            None => {
                *text = trimmed.to_string();
                return stripped;
            }
        }
    }
}

fn leading_word(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, c)| !c.is_alphabetic())
        .map_or(text.len(), |(i, _)| i);
    &text[..end]
}

#[derive(PartialEq, Clone, Copy)]
enum Script {
    Latin,
    Cyrillic,
    Other,
}

fn script(c: char) -> Script {
    match c {
        'a'..='z' | 'A'..='Z' | '\u{00C0}'..='\u{024F}' => Script::Latin,
        '\u{0400}'..='\u{04FF}' => Script::Cyrillic,
        _ => Script::Other,
    }
}

/// The script most of the recent letters in `text` are written in.
fn dominant_script(text: &str) -> Script {
    let (mut latin, mut cyrillic) = (0, 0);
    for c in text.chars().rev().filter(|c| c.is_alphabetic()).take(40) {
        match script(c) {
            Script::Latin => latin += 1,
            Script::Cyrillic => cyrillic += 1,
            Script::Other => {}
        }
    }
    match (latin, cyrillic) {
        (0, 0) => Script::Other,
        (l, c) if l > c => Script::Latin,
        _ => Script::Cyrillic,
    }
}

/// A capitalized word that should keep its capital at a phrase seam:
/// an acronym or camel case ("API", "OpenRouter"), English "I", a word in a
/// different script than the text before it (a "Kling" inside Russian), or a
/// word the model capitalized mid-sentence elsewhere (a known proper noun).
fn looks_like_name(word: &str, before: &str, names: &HashSet<String>) -> bool {
    let Some(first) = word.chars().next() else {
        return true;
    };
    word.chars().filter(|c| c.is_uppercase()).count() >= 2
        || word == "I"
        || {
            let context = dominant_script(before);
            let own = script(first);
            context != Script::Other && own != Script::Other && own != context
        }
        || names.contains(word)
}

fn lowercase_continuation(part: &mut String, before: &str, names: &HashSet<String>) {
    let word = leading_word(part);
    let Some(first) = word.chars().next() else {
        return;
    };
    if !first.is_uppercase() || looks_like_name(word, before, names) {
        return;
    }
    let lowered: String = first.to_lowercase().collect();
    part.replace_range(..first.len_utf8(), &lowered);
}

/// Words the model capitalized in the middle of a sentence — proper nouns.
fn capitalized_mid_sentence(parts: &[&str]) -> HashSet<String> {
    let mut names = HashSet::new();
    for part in parts {
        let mut previous: Option<&str> = None;
        for token in part.split_whitespace() {
            let opens_sentence = previous.is_none_or(|p| p.ends_with(['.', '!', '?', '…']));
            let word = leading_word(token);
            if !opens_sentence && word.chars().next().is_some_and(char::is_uppercase) {
                names.insert(word.to_string());
            }
            previous = Some(token);
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_clamps_and_is_little_endian() {
        assert_eq!(
            pcm16_le(&[0.0, 1.0, -1.0, 2.0]),
            vec![0, 0, 0xff, 0x7f, 0x01, 0x80, 0xff, 0x7f]
        );
    }

    #[test]
    fn wav_has_header_and_samples() {
        let wav = wav_bytes(&[0.0; 160]).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 320);
    }

    #[test]
    fn pause_tracker_measures_trailing_silence() {
        let mut t = PauseTracker::default();
        t.push(&vec![0.2; 1600]);
        assert!(t.heard_speech());
        assert_eq!(t.trailing_silence_secs(), 0.0);
        t.push(&vec![0.0; 8000]);
        assert!((t.trailing_silence_secs() - 0.5).abs() < 0.011);
        t.push(&vec![0.2; 160]);
        assert_eq!(t.trailing_silence_secs(), 0.0);
    }

    #[test]
    fn split_keeps_every_sample() {
        let mut samples = vec![0.3f32; 16_000 * 100];
        for s in &mut samples[16_000 * 42..16_000 * 42 + 3200] {
            *s = 0.0;
        }
        let chunks = split_for_upload(&samples, 45.0);
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), samples.len());
        assert!(chunks.iter().all(|c| c.len() <= 16_000 * 45));
        // The first cut lands in the silent gap.
        let first = chunks[0].len();
        assert!((16_000 * 42..16_000 * 42 + 3200).contains(&first));
    }

    #[test]
    fn join_phrases_spacing() {
        assert_eq!(
            join_phrases(["Привет.", " Как дела? "]),
            "Привет. Как дела?"
        );
        assert_eq!(join_phrases(["你好", "世界"]), "你好世界");
        assert_eq!(join_phrases(["", "one", "  "]), "one");
    }

    #[test]
    fn join_repairs_seams_inside_a_sentence() {
        // The case from a real dictation.
        assert_eq!(
            join_phrases([
                "поэтому пользователь сможет создать только...",
                "Какое-то ограниченное количество видео.",
            ]),
            "поэтому пользователь сможет создать только какое-то ограниченное количество видео."
        );
        assert_eq!(
            join_phrases(["токенов за...", "какую-то сумму"]),
            "токенов за какую-то сумму"
        );
        assert_eq!(
            join_phrases(["если у нас будет,", "Несколько версий"]),
            "если у нас будет, несколько версий"
        );
        assert_eq!(join_phrases(["and then", "I said"]), "and then I said");
    }

    #[test]
    fn join_keeps_sentence_starts_and_names() {
        assert_eq!(
            join_phrases(["Это конец.", "Новое предложение."]),
            "Это конец. Новое предложение."
        );
        // Latin names inside Russian, acronyms, camel case.
        assert_eq!(
            join_phrases(["несколько моделей...", "Kling и Seedance"]),
            "несколько моделей Kling и Seedance"
        );
        assert_eq!(
            join_phrases(["нужна поддержка", "API и", "OpenRouter"]),
            "нужна поддержка API и OpenRouter"
        );
        // A proper noun the model capitalized mid-sentence elsewhere.
        assert_eq!(
            join_phrases(["Мы были в...", "Казани", "и снова в Казани."]),
            "Мы были в Казани и снова в Казани."
        );
    }

    #[test]
    fn join_phrases_drops_a_leading_ellipsis_on_the_next_piece() {
        assert_eq!(
            join_phrases(["небольшое количество", "...моего диалога."]),
            "небольшое количество моего диалога."
        );
    }
}
