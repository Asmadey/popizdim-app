"""Build synthetic multi-speaker test dialogues with macOS `say` + ffmpeg.

Writes audio/<name>.wav (16 kHz mono) and audio/<name>.truth.txt (start end speaker).
"""
import subprocess, sys, wave, os

PAUSE = 0.5
RU = [
    ("A", "Milena", 1.0, "Добрый день. Давайте обсудим план на следующую неделю и решим, кто за что отвечает."),
    ("B", "Milena", 0.78, "Согласен. Я предлагаю начать с распознавания файлов, потому что пользователи давно его просят."),
    ("A", "Milena", 1.0, "Хорошо. А что с диаризацией? Она будет работать локально или через облако?"),
    ("C", "Milena", 1.22, "Локально. Мы проверили модель на маке, и она работает быстрее реального времени."),
    ("B", "Milena", 0.78, "Тогда нужно ещё протестировать длинные записи, например часовой подкаст."),
    ("A", "Milena", 1.0, "Отлично. Я подготовлю тестовые файлы к среде."),
    ("C", "Milena", 1.22, "А я посмотрю, как вставлять текст в активное окно после выбора файла."),
]
EN = [
    ("A", "Samantha", 1.0, "Good morning. Let's go over the release plan for next week."),
    ("B", "Daniel", 1.0, "Sure. I think file transcription should ship first, users keep asking for it."),
    ("C", "Karen", 1.0, "Agreed. Diarization runs locally now, which is great for privacy."),
    ("A", "Samantha", 1.0, "What about long recordings, like an hour long podcast?"),
    ("B", "Daniel", 1.0, "We still need to test that. I will prepare a few long files."),
    ("C", "Karen", 1.0, "And I will check pasting into the active window after the file dialog."),
]

def render(idx, voice, pitch, text, out):
    aiff = f"audio/parts/{idx}.aiff"
    subprocess.run(["say", "-v", voice, "-o", aiff, text], check=True)
    # pitch shift without changing duration: asetrate scales pitch+speed, atempo restores speed
    af = f"asetrate=22050*{pitch},aresample=16000,atempo={1/pitch:.4f}" if pitch != 1.0 else "aresample=16000"
    subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-i", aiff, "-af", af,
                    "-ac", "1", "-ar", "16000", "-sample_fmt", "s16", out], check=True)

def build(name, lines):
    frames, truth, t = [], [], 0.0
    silence = b"\x00\x00" * int(16000 * PAUSE)
    for i, (spk, voice, pitch, text) in enumerate(lines):
        part = f"audio/parts/{name}-{i}.wav"
        render(f"{name}-{i}", voice, pitch, text, part)
        with wave.open(part) as w:
            data = w.readframes(w.getnframes())
        dur = len(data) / 2 / 16000
        truth.append(f"{t:7.2f} {t+dur:7.2f} {spk}")
        frames += [data, silence]
        t += dur + PAUSE
    with wave.open(f"audio/{name}.wav", "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
        w.writeframes(b"".join(frames))
    open(f"audio/{name}.truth.txt", "w").write("\n".join(truth) + "\n")
    print(name, f"{t:.1f} s")

build("ru3", RU)
build("en3", EN)
