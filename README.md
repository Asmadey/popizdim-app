<p align="center">
  <img src="branding/popizdim-banner.png" width="100%" alt="Попиздим — речь в текст и текст в речь">
</p>

<h1 align="center">Попиздим — речь в текст</h1>

<p align="center">
  Приложение для macOS, которое превращает голос в текст и текст в голос.<br>
  Облачные модели <b>Microsoft MAI</b> и открытые локальные модели.
</p>

<p align="center">
  <a href="https://github.com/Asmadey/popizdim-app/releases/latest"><b>Скачать DMG</b></a> ·
  <a href="#установка">Установка</a> ·
  <a href="#настройка">Настройка</a> ·
  <a href="MAI.md">Как это устроено</a> ·
  <a href="#english">English</a>
</p>

---

**Речь в текст (Speech-to-Text).** Нажмите горячую клавишу, говорите, отпустите — текст появится в поле, где стоит курсор: в мессенджере, редакторе, браузере или терминале. Пока вы говорите, расшифровка показывается в окошке поверх экрана. Можно распознавать и готовые записи: любой аудио- или видеофайл, с подписями, кто и когда говорил.

**Текст в речь (Text-to-Speech).** Выделите текст в любом приложении, нажмите сочетание клавиш — и приложение прочитает его вслух голосом MAI-Voice-2.1. Длинный текст озвучивается потоком: начало звучит, пока синтезируется продолжение.

> **Неофициальный проект.** «Попиздим» не связан с Microsoft и не одобрен ею. «MAI-Transcribe» и «MAI-Voice» — названия моделей Microsoft и упоминаются здесь только для описания совместимости.

## Возможности

- **Диктовка в любое приложение.** Работает везде, где есть поле ввода. Режим удержания или переключения, 60 языков с автоопределением, в том числе смешанная речь.
- **Облачные модели Microsoft MAI** — распознавание `MAI-Transcribe-2` и озвучка `MAI-Voice-2.1` (подробнее ниже).
- **Открытые локальные модели** для работы без интернета и без отправки звука в облако:
  - **GigaAM** — очень хорошо распознаёт **русский язык**, лучший выбор для русской диктовки;
  - **Parakeet** — отлично распознаёт **английский язык**, если он вам нужен;
  - Whisper и другие модели на выбор.
- **Распознавание файлов и видео.** Иконка в строке меню → «Распознать запись…». MP3, M4A, WAV, FLAC, MP4, MOV, MKV и другие: звук извлекается из видео, режется по паузам и распознаётся активной моделью. Результат сохраняется в историю и вставляется в поле с курсором. Прогресс виден рядом с иконкой в строке меню.
- **Диаризация: кто что говорил.** Для файлов реплики подписываются по говорящим и снабжаются временными метками с минутами и секундами:
  `[00:01:23] Спикер 1: …`. Диаризация работает локально (pyannote community-1 на CoreML): модель (273 МБ) скачивается один раз, звук не покидает ваш Mac. Включается в «Продвинутые → Подписи говорящих».
- **Словарь.** Отдельная вкладка с вашими терминами, именами и брендами: слова передаются модели как подсказки, поэтому `Kling`, `Seedance` или `OpenRouter` пишутся правильно. Новое слово добавляется, не открывая настроек: выделите его в любом приложении и выберите в меню иконки **«Добавить в словарь»**. Работает в браузерах на Chromium (проверено в Chrome). Результат («Добавлено в словарь», «Уже в словаре», «Нет выделенного текста») появится в окошке записи.
- **Чтение вслух.** «Озвучить текст» в меню иконки или по сочетанию клавиш; на вкладке TTS выбираются голос и скорость, озвучки можно сохранять в MP3.
- **Постобработка.** Вкладка с главным переключателем: когда он включён, текст после расшифровки уходит в языковую модель на той же клавише диктовки. Переключатель «Отдельное сочетание клавиш» даёт постобработке собственную клавишу, а обычная тогда вставляет сырую расшифровку. Запрос готовый или свой: «Улучшение транскрипции» (орфография, цифры, знаки препинания, без слов-паразитов), «Профессионально», «Вежливо», «ASD-STE100» (упрощённый технический английский). Поставщики: OpenAI, OpenRouter, Anthropic, Groq, Cerebras, Z.AI, **Ollama** (`https://ollama.com/v1`), AWS Bedrock, Apple Intelligence и любой совместимый с OpenAI адрес.
- **Чистая расшифровка.** Стиль «Чисто» убирает «э-э», оговорки и запинки на стороне модели. Плюс локальный фильтр слов-паразитов с русским списком.
- **Резерв.** Если Azure недоступен, запись автоматически распознаётся через OpenRouter.

## Модели Microsoft MAI

### MAI-Transcribe-2 — речь в текст

Модель Microsoft для распознавания речи на 60 языках. Что она даёт на практике:

- **Диктовка вместо набора.** Говорить быстрее, чем печатать: письма, заметки, сообщения, код-комментарии, документы.
- **Текст появляется по ходу речи.** В потоковом варианте `MAI-Transcribe-2-Streaming` (Azure Foundry, Realtime WebSocket) черновой текст приходит примерно через 100 мс, а на паузах подтверждается. Вы видите результат сразу и можете поправить себя, не дожидаясь конца.
- **Много языков и смешанная речь.** Автоопределение языка, переключение «на English и обратно» внутри одной фразы.
- **Стоит недорого.** Через OpenRouter (`microsoft/mai-transcribe-2`) около $0.1 за час аудио; нужен только один ключ.
- **Работает и с записями.** Интервью, созвоны и лекции превращаются в текст; вместе с локальной диаризацией получается стенограмма по говорящим.

| Провайдер                          | Модель                       | Как работает                                                  |
| ---------------------------------- | ---------------------------- | ------------------------------------------------------------- |
| Azure Foundry (Realtime WebSocket) | `MAI-Transcribe-2-Streaming` | Настоящий стриминг: черновой текст через ~100 мс              |
| OpenRouter                         | `microsoft/mai-transcribe-2` | Каждая фраза распознаётся на паузе, текст появляется по фразам |

### MAI-Voice-2.1 — текст в речь

Модель Microsoft для синтеза речи (`MAI-Voice-2.1` и быстрая `MAI-Voice-2.1-Flash`, через OpenRouter). Что она даёт на практике:

- **Читает вслух что угодно.** Статью, письмо, чужой ответ, собственный черновик: выделили текст и нажали сочетание клавиш.
- **Потоковое воспроизведение.** Звук идёт потоком (PCM 24 кГц), а длинный текст синтезируется кусками, пока играет предыдущий. Не нужно ждать, пока озвучится весь текст.
- **Вычитка на слух.** Услышав собственный текст, проще заметить корявые фразы и ошибки.
- **Глаза отдыхают.** Прослушивание длинных материалов, пока заняты другим делом; удобно и людям с нарушением зрения.
- **Выбор голоса и скорости**, сохранение озвучки в MP3. `Flash` отвечает быстрее, `MAI-Voice-2.1` звучит качественнее.

## Установка

1. Скачайте `Popizdim_1.0.dmg` со страницы [Releases](https://github.com/Asmadey/popizdim-app/releases/latest). Готовый DMG собран только для Apple Silicon; на Intel сборка из исходников не проверялась.
2. Откройте DMG и перетащите **Popizdim** в «Программы».
3. Приложение подписано без сертификата Apple Developer, поэтому при первом запуске macOS предупредит о неизвестном разработчике. Разрешите запуск в «Системные настройки» → «Конфиденциальность и безопасность» → «Всё равно открыть» или командой:
   ```bash
   xattr -dr com.apple.quarantine "/Applications/Popizdim.app"
   ```
4. Дайте доступ к **микрофону** и **Универсальному доступу**: без второго приложение не сможет вставлять текст, а «Добавить в словарь» и «Озвучить текст» читать выделенное. Включённое право приложение замечает само, без нажатий.
5. Распознавание файлов в форматах, которые не читаются встроенным декодером (например, Opus в MKV/WebM), использует ffmpeg: `brew install ffmpeg`.

## Настройка

Откройте вкладку **«Облако»**.

### OpenRouter (самый простой вариант)

1. Создайте ключ на [openrouter.ai/settings/keys](https://openrouter.ai/settings/keys). Нужен обычный ключ для инференса: management-ключи распознавать речь не могут.
2. Вставьте его в поле «API-ключ» и нажмите «Проверить».
3. Нажмите «Использовать для диктовки».

Распознавание стоит около $0.1 за час аудио. Тот же ключ используется для MAI-Voice.

### Azure Foundry (настоящий стриминг)

1. Создайте ресурс Microsoft Foundry в регионе, где доступна модель: Sweden Central, Central US, South India и др. (см. [документацию](https://learn.microsoft.com/azure/ai-services/speech-service/mai-transcribe-2-streaming-realtime)).
2. Разверните модель: Build → Models → Deploy a base model → `MAI-Transcribe-2-Streaming`.
3. Вставьте в приложение endpoint ресурса (`https://<ресурс>.services.ai.azure.com`, можно со ссылкой на проект), ключ и имя деплоя.
4. «Проверить» отправляет короткую тестовую расшифровку. Если деплоя нет, приложение так и скажет.

### Полезные настройки

- **Стиль расшифровки → «Чисто»**: без «э-э» и оговорок. Хорошо работает вместе со словарём терминов.
- **Словарь**: имена, бренды, термины. Быстрее всего добавлять их из меню иконки: «Добавить в словарь».
- **Постобработка**: включите переключатель вверху, выберите поставщика, модель и запрос. По умолчанию она работает на обычной клавише диктовки; отдельное сочетание включается переключателем ниже.
- **Продвинутые → Подписи говорящих**: включить или выключить диаризацию для файлов.
- **Общие → Горячая клавиша**: по умолчанию `Option+Space`. Удерживайте для записи или нажмите один раз, чтобы переключить режим.

Ключи хранятся только локально в настройках приложения (`~/Library/Application Support/com.popizdim.desktop/`). В сборке их нет. При первом запуске настройки, история и записи прежней версии переносятся из `com.maidictate.desktop` автоматически.

## Сборка из исходников

Нужны [Rust](https://rustup.rs), [Bun](https://bun.sh), cmake (`brew install cmake`) и Xcode Command Line Tools.

```bash
bun install
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx

CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev          # разработка
CI=true bun run tauri build --bundles dmg \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'     # DMG

cd src-tauri && cargo test --lib                            # тесты, включая mock-сервер Azure
```

Чтобы права на микрофон и Универсальный доступ не сбрасывались после каждой пересборки, подпишите локальные сборки постоянным сертификатом: `scripts/create-local-signing-identity.sh` создаёт «Popizdim Local Signing», а затем `APPLE_SIGNING_IDENTITY="<SHA-1>" bun run tauri build --bundles app`. Подпись без него (ad-hoc) меняется при каждой сборке.

Архитектура облачных провайдеров, протоколы и живые тесты описаны в [MAI.md](MAI.md), распознавание файлов и диаризация — в [docs/plans/file-transcription.md](docs/plans/file-transcription.md). Иконки генерируются из [`branding/`](branding).

## Основано на Handy

Проект построен на основе [Handy](https://github.com/cjpais/Handy) (MIT, © CJ Pais). Лицензия [MIT](LICENSE).

---

## English

**Popizdim** ("Попиздим — speech to text") is a macOS app that turns your voice into text and text into speech, using Microsoft's MAI cloud models and open local models.

- **Speech-to-text:** press a shortcut, speak, and the text is typed into whatever app has focus.
  - **MAI-Transcribe-2-Streaming** (Azure Foundry): true streaming over the Realtime WebSocket API, live text while you speak.
  - **MAI-Transcribe-2** (OpenRouter `microsoft/mai-transcribe-2`): phrase-by-phrase transcription at speech pauses. Only an OpenRouter key is needed.
  - **Open local models:** **GigaAM** (excellent for Russian) and **Parakeet** (excellent for English), plus Whisper and others. Fully offline.
- **Text-to-speech:** select text in any app and press a shortcut; **MAI-Voice-2.1 / Flash** reads it aloud with streaming playback (long text is synthesized in chunks while the previous one plays).
- **Transcribe Recording…** in the menu bar icon: any audio or video file, with local **speaker diarization** (pyannote on CoreML) and minute:second timestamps: `[00:01:23] Speaker 1: …`.
- **Dictionary** tab for your own terms, plus **Add to Dictionary** in the menu bar icon: select a word in any app, click it, and it is added.
- **Post-processing** with a master switch (it runs on the regular dictation hotkey; an optional "Separate hotkey" switch gives it its own); providers include OpenAI, OpenRouter, Anthropic, Groq, Cerebras, **Ollama**, AWS Bedrock, Apple Intelligence and any OpenAI-compatible endpoint. Ready-made prompts: cleanup, professional, polite, ASD-STE100.
- 60 languages with auto-detection, a "clean" style that drops fillers, and Azure → OpenRouter fallback.

Download the DMG from [Releases](https://github.com/Asmadey/popizdim-app/releases/latest) (the ready-made DMG is built for Apple Silicon only, building from source on Intel is untested; ad-hoc signed: allow it in _Privacy & Security_ or run `xattr -dr com.apple.quarantine "/Applications/Popizdim.app"`). Configure keys in **Cloud**.

_Unofficial project, not affiliated with or endorsed by Microsoft. MIT licensed._
