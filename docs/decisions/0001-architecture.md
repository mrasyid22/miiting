# 0001 · Initial architecture

Status: accepted · 2026-10-14

## Context

We want a meeting transcriber that:
- never adds a bot to the call,
- is accurate for meetings held almost entirely in Indonesian,
- is local-first, with third-party APIs as an option,
- feels native on the Mac and follows Apple's current design language (Liquid Glass), and
- can be used and extended by others (open source), with Windows as the next platform.

## Decisions

### 1. SwiftUI in front, a Rust core behind it (UniFFI)

Liquid Glass is only fully available through SwiftUI/AppKit; a web view (Tauri, Electron) can only imitate it. Everything that does not depend on the platform lives in `miiting-core`: the audio pipeline, speech-to-text, storage, notes and model downloads. The Swift shell only does what needs Apple APIs: the interface, audio capture, the Keychain and permissions.

Consequences:
- Two languages and a binding-generation step (`scripts/build-core.sh`).
- A Windows app needs a new shell and WASAPI capture; the core stays the same.

### 2. System audio through a Core Audio process tap

`CATapDescription` plus an aggregate device (macOS 14.2+) captures every app's output without a virtual driver and without the Screen Recording permission; only "System Audio Recording" is needed. Microphone and system audio are written to two WAV files on a shared timeline. That gives "Me / Them" labels for free and a cleaner signal per channel for speech-to-text.

### 3. Two passes: a live draft, then a final transcript

- **Live:** an energy VAD cuts the stream into utterances (at most 15 s). Silero VAD then confirms someone is actually speaking, and Whisper Large v3 Turbo transcribes greedily for low latency.
- **Final:** after the meeting each channel is transcribed again in full with Large v3, beam 5. Silero finds the speech, which is packed into windows of at most 28 s with timestamps kept on the original timeline. The channels are merged by time, microphone echo is dropped, and the result replaces the draft.

The live draft may be less accurate; the notes are written from the final transcript.

### 4. Whisper as the default local engine

Whisper Large v3 is the strongest open model for Indonesian we measured (5.5% WER on FLEURS; see [benchmarks](../benchmarks.md)). NVIDIA Parakeet, which Meetily-style apps use for speed, does not support Indonesian.

Indonesian-specific measures:
- The language is pinned (`id`); auto-detection drifts to Malay or English.
- `no_context = true`, so one hallucination does not spread to the next window.
- Hallucination filters for YouTube-subtitle phrases emitted on silence, and for repetition loops.
- The glossary is used both as Whisper's initial prompt and as context for the notes model.

### 5. Two wire formats cover every provider

- **Cloud STT:** OpenAI `/audio/transcriptions` (Groq, OpenAI, speaches, LocalAI). Long recordings are split at silences into pieces of at most 10 minutes.
- **LLMs:** OpenAI `/chat/completions` (OpenAI, OpenRouter, Groq, Gemini, LM Studio), Ollama's native `/api/chat` (to raise `num_ctx`, whose default silently truncates long transcripts), and the Anthropic Messages API.

Long transcripts are summarized map-reduce: notes per part, then minutes from the notes.

### 6. Device-aware defaults

The core detects chip, RAM and free disk space and recommends live/final models, beam size and a local LLM per tier (8 / 16 / 18–36 / 48+ GB). The live model is chosen to stay light next to a video call; the final pass and the notes run after the meeting, when more memory is free. Local LLM context size and chunking also follow the tier.

### 7. Local-first you can verify

- Every meeting stores an `egress` list of hosts that received its audio or text, shown in the interface.
- API keys live in the Keychain, not in the database or preferences.
- No telemetry. The log file contains events and errors, never transcript text or keys.

### 8. Open source (MIT), models downloaded separately

Models are not bundled, so their licenses do not affect Miiting's code; defaults are permissively licensed (Whisper and Silero VAD: MIT).

### 9. English in code, localized interface

Code, comments and docs are in English so anyone can contribute. User-facing text is localized: Swift uses English source strings with `<lang>.lproj/Localizable.strings` (String Catalogs need Xcode to compile, and the project builds with the Command Line Tools); the Rust core uses a small `i18n` module (`Text`, `tr!`). The interface language is independent of the meeting language, which decides the transcription language and the language of the notes.

## Open questions

- A diarization engine for several remote participants (candidate: sherpa-onnx with pyannote segmentation and 3D-Speaker embeddings).
- Whether to fine-tune Whisper for Indonesian, pending benchmarks on real meetings.
- Distribution: Developer ID signing, notarization, Sparkle for updates.
