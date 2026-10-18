# Popizdim — облачные модели Microsoft MAI

Popizdim (Tauri 2 + Rust + React) поддерживает облачные провайдеры:

| Что                | Модель                                                     | Провайдер                          | Режим                                                                   |
| ------------------ | ---------------------------------------------------------- | ---------------------------------- | ----------------------------------------------------------------------- |
| Диктовка / STT     | `MAI-Transcribe-2-Streaming`                               | Azure Foundry, Realtime WebSocket  | Настоящий стриминг: черновой текст ~100 мс, коммит на паузах            |
| Диктовка / STT     | `microsoft/mai-transcribe-2`                               | OpenRouter `/audio/transcriptions` | Псевдо-стриминг: каждая фраза отправляется на паузе                     |
| Чтение вслух (TTS) | `microsoft/mai-voice-2.1-flash`, `microsoft/mai-voice-2.1` | OpenRouter `/audio/speech`         | PCM 24 кГц, длинный текст синтезируется кусками, пока играет предыдущий |

Горячие клавиши, вставка текста в активное приложение, live-оверлей, история, VAD и словарь работают одинаково для облачных и локальных моделей.

## Почему так

- **OpenRouter не умеет стримить STT.** У них только batch-эндпоинт, без WebSocket. Поэтому для OpenRouter реализован псевдо-стриминг: аудио режется на паузах речи, фразы распознаются параллельно, текст появляется по фразам. Когда вы отпускаете клавишу, остаётся дождаться только последней фразы: в тесте это ~0,7 с против ~3 с для всей записи целиком.
- **Настоящий стриминг MAI-Transcribe-2-Streaming есть только в Azure Foundry** (и в Vercel AI Gateway). Клиент реализует [документированный протокол](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/mai-transcribe-2-streaming-realtime): `session.update`, затем `input_audio_buffer.append`, ответные `…transcription.intermediate` / `…delta` и `input_audio_buffer.commit` на паузах.
- **Резерв:** если Azure недоступен и задан ключ OpenRouter, запись автоматически распознаётся через OpenRouter.

## Настройка

1. **Облако (MAI)** в боковом меню:
   - _OpenRouter → API-ключ_ — **inference**-ключ с openrouter.ai/settings/keys. Management-ключ распознавать речь не может (`401 User not found`). Этот же ключ используется для MAI-Voice.
   - _Azure Foundry_ — endpoint (`https://<ресурс>.services.ai.azure.com`), ключ и имя деплоя `MAI-Transcribe-2-Streaming`. Регионы: Sweden Central, Central US, South India.
   - Кнопка **«Проверить»** проверяет ключи настоящим запросом.
2. **«Использовать для диктовки»** у нужного провайдера. Модель также можно выбрать в разделе «Модели».
3. Диктовка: `Option+Space`. Чтение выделенного текста вслух: `Option+Shift+R`, повторное нажатие останавливает.
4. _Custom Words_ (раздел «Продвинутые») передаются в OpenRouter как `phraseList`, то есть подсказки для имён и терминов. Например, «OpenRouter» вместо «open router».
5. Слова-паразиты убираются в два слоя. Стиль OpenRouter «Чисто» вырезает их на стороне модели вместе с оговорками и многоточиями. Локальный фильтр «Удалять слова-паразиты» работает с любым провайдером: в форк добавлен русский список (`э`, `э-э`, `эм`, `м-м`…; «мм» не входит, это миллиметры). Лучший результат на реальной записи дал «Чисто» вместе с Custom Words для названий.

Пустые поля берутся из окружения: `OPENROUTER_API_KEY`, `AZURE_MAI_ENDPOINT`, `AZURE_MAI_API_KEY`, `AZURE_MAI_DEPLOYMENT_NAME` (см. `.env.example`).

## Код

```
src-tauri/src/cloud/
  mod.rs         реестр облачных моделей, учётные данные, URL Azure, block_on
  audio.rs       PCM16/WAV, детектор пауз, нарезка длинного аудио, склейка фраз (CJK без пробелов)
  azure.rs       протокол Realtime: handshake, события, batch через WebSocket
  openrouter.rs  STT (ретраи, чанки ≤45 с параллельно), проверка ключа
  stream.rs      живые сессии: Azure (коммит на паузах) и OpenRouter (фраза на паузе)
  tts.rs         MAI-Voice: разбиение текста, синтез наперёд, воспроизведение через rodio
src-tauri/src/commands/cloud.rs           команды: настройки, проверка, озвучка
src/components/settings/cloud/            страница «Облако (MAI)»
```

Точки встраивания: `EngineType::Cloud` / `ModelSource::Cloud` (`managers/model.rs`); в `managers/transcription.rs` — `run_cloud_stream_worker` (тот же контракт `StreamCmd`, что у локальных стриминговых моделей) и `transcribe_cloud` (batch и fallback); действие `speak_selection` (`actions.rs`).

## Сборка и тесты

```bash
bun install
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev          # разработка
bun run tauri build --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'
CI=true bun run tauri build --bundles dmg --config '{"bundle":{"createUpdaterArtifacts":false}}'  # установочный DMG
cd src-tauri && cargo test --lib                            # все тесты, включая mock-сервер Azure
```

Живые тесты (тратят пару центов):

```bash
set -a; . ./.env; set +a; cd src-tauri
MAI_TEST_WAV=/path/16k-mono.wav cargo test --lib live_openrouter -- --ignored --nocapture
cargo test --lib live_tts -- --ignored --nocapture
./target/debug/handy --transcribe-file file.wav --model cloud/openrouter/mai-transcribe-2 --json
```

## Ограничения

- Работа с Azure проверена против mock-сервера, построенного по документации Microsoft. С реальным ресурсом её проверят первые запросы с вашими ключами.
- Сессия Azure длится не больше часа (лимит сервиса). Для диктовки это не ограничение.
- Апдейтер отключён: его фид указывает не на релизы этого приложения.
- У приложения свой `productName` (Popizdim) и `identifier` (`com.popizdim.desktop`); данные прежнего `com.maidictate.desktop` переносятся при первом запуске (`portable::migrate_legacy_app_data`).
