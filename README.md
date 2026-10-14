# Miiting

**Local-first meeting notes for macOS, tuned for Indonesian. No bot joins your call.**

Miiting records Zoom, Google Meet, Teams or in-person meetings straight from your Mac (microphone + system audio), transcribes them with Whisper running on your Apple chip, and writes meeting notes with a summary, decisions and action items. Everything runs locally by default; third-party APIs are optional and every meeting shows whether its data ever left your Mac.

[Bahasa Indonesia](README.id.md)

## Download

Get the latest `.dmg` from [Releases](https://github.com/mrasyid22/miiting/releases). *Nightly* is built from every push to `main`. Miiting needs an Apple Silicon Mac with macOS 26 or later.

Until releases are notarized, macOS blocks the app on first launch: open System Settings › Privacy & Security and click *Open Anyway* (or run `xattr -dr com.apple.quarantine /Applications/Miiting.app`).

## Features

- **No bot.** Other participants are captured with a Core Audio process tap (macOS 14.2+). No BlackHole or virtual driver, and no extra attendee in the call.
- **Two channels, two speakers.** Your microphone is labelled *Me* and system audio *Them*, without diarization. Echo of the other participants picked up by your microphone is removed.
- **Live draft, then an accurate final transcript.** Whisper Large v3 Turbo transcribes live; after the meeting the whole recording is transcribed again with Large v3 (beam search + Silero VAD) and replaces the draft.
- **Tuned for Indonesian.** The language is pinned, Whisper's typical hallucinations ("Terima kasih telah menonton", subtitle credits, repetition loops) are filtered, a glossary helps with names and jargon, and the notes templates exist in Indonesian and English.
- **Fits your Mac.** Miiting reads your chip, RAM and free disk space and recommends the models, beam size and local LLM that fit — from an 8 GB MacBook Air to a 64 GB MacBook Pro.
- **Bring your own provider.**
  - Speech-to-text: local Whisper, or any server that speaks OpenAI's `/audio/transcriptions` (Groq, OpenAI, speaches, LocalAI).
  - Notes: Ollama, LM Studio, Claude, OpenAI, OpenRouter, Groq, Gemini or any OpenAI-compatible endpoint.
- **Visible privacy.** Each meeting records which hosts received its data. API keys live in the Keychain. No telemetry.
- **Also:** full-text search over titles, transcripts and notes; Markdown editing and export; audio and video import; menu bar controls; `miiting://` URL actions; crash recovery; English and Indonesian interface.

## Architecture

```
apps/macos/          SwiftUI app (macOS 26, Liquid Glass): UI, microphone + system audio capture, Keychain
   │  UniFFI
core/                Rust workspace, shared with a future Windows app
   crates/miiting-core   resampling, VAD, chunking, whisper.cpp, cloud adapters, hallucination
                        filters, SQLite + FTS5, LLM notes, model catalog, device recommendations, i18n
   crates/miiting-ffi    UniFFI bindings for Swift
   tools/miiting-cli     benchmark (WER/CER/RTF), transcribe, simulate, doctor, seed-demo
```

The reasoning behind these choices is in [docs/decisions/0001-architecture.md](docs/decisions/0001-architecture.md).

## Build

Requirements: an Apple Silicon Mac on macOS 26 or later, Rust (`brew install rustup && rustup default stable`), `cmake`, and Swift 6.2 (the Command Line Tools are enough; Xcode is optional).

```bash
make app        # builds build/Miiting.app
make run        # builds and opens it
make install    # copies it to /Applications
make dmg        # packages it as build/Miiting-<version>.dmg
make test       # Rust unit tests
make demo       # opens the app with sample meetings, leaving your data untouched
```

To work in Xcode, run `scripts/build-core.sh` once (it builds the Rust static library and generates the Swift bindings), then `open apps/macos/Package.swift`.

App data lives in `~/Library/Application Support/Miiting/`: `miiting.db`, `meetings/<id>/*.wav` and `models/`.

## Models

Models are downloaded from inside the app (Settings › Models) or with `make models`. The recommendation is applied on first launch and can be changed at any time; `miiting-cli doctor` prints it in the terminal.

| Mac | Live | Final | Local notes (Ollama) |
|---|---|---|---|
| Light (8 GB) | Turbo Q5 | Large v3 Q5, beam 3 | `gemma3:4b` (a cloud provider is suggested) |
| Standard (16 GB) | Turbo Q5 | Large v3, beam 5* | `qwen3:8b` |
| High (18–36 GB) | Turbo Q5 | Large v3, beam 5* | `gemma3:12b` |
| Maximum (48 GB+) | Large v3, no beam | Large v3, beam 5 | `gemma3:27b` |

\* Base M-series chips (not Pro/Max/Ultra) use beam 3 to keep the final pass quick. With little free disk space, Large v3 Q5 replaces Large v3.

All Whisper and Silero models are MIT licensed. NVIDIA Parakeet is not offered because it does not support Indonesian.

## Accuracy

On the Indonesian FLEURS test set, Large v3 with beam 5 reaches 5.5% WER and Turbo Q5 7.1%. Full numbers and method: [docs/benchmarks.md](docs/benchmarks.md).

```bash
make models fleurs     # ~4 GB, asks before downloading
make bench             # WER / CER / RTF per model
make e2e               # synthetic two-speaker meeting → live → final → notes
```

## Local notes with Ollama

```bash
brew install ollama
brew services start ollama      # start at login
ollama pull gemma3:12b          # or the model Settings › Notes suggests for your Mac
```

Miiting calls Ollama's native `/api/chat` so it can raise the context window; Ollama's default (2–4k tokens) would silently cut long transcripts.

## Automation

```bash
open "miiting://record/start"                 # also: stop, toggle, pause
open "miiting://meeting/<id>/retranscribe"    # or /summarize
```

Works from Shortcuts, Raycast, Stream Deck or scripts. Events and errors (never transcript text) are logged to `~/Library/Logs/Miiting/miiting.log`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Code, comments and docs are in English; the interface is localized (English and Indonesian so far). CI and releases are described in [docs/releasing.md](docs/releasing.md).

## Roadmap

- Local diarization to tell several remote participants apart (sherpa-onnx)
- Playback from transcript timestamps
- Detect when a meeting starts (Zoom/Meet using the microphone)
- Custom notes templates, PDF/DOCX export
- Ask questions about a meeting
- Optional Whisper fine-tuning from your own corrections
- Windows app (same Rust core, WASAPI capture)
- Developer ID signing, notarization and auto-update

## License

MIT — see [LICENSE](LICENSE). Models and datasets have their own licenses (Whisper: MIT, Silero VAD: MIT, FLEURS: CC BY 4.0).
