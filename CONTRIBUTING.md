# Contributing to Miiting

Thanks for helping! Miiting is built for Indonesian meetings, but the code, comments, docs, issues and pull requests are in English so anyone can take part.

## Getting set up

```bash
brew install rustup cmake && rustup default stable
make app          # Rust core + SwiftUI app → build/Miiting.app
make test         # Rust unit tests
make demo         # the app with sample meetings in a separate data folder
```

The Command Line Tools are enough to build. With Xcode, run `scripts/build-core.sh` once and open `apps/macos/Package.swift`.

To test transcription end to end you need models (`make models`, ~3.5 GB) and, for notes, [Ollama](https://ollama.com):

```bash
make e2e          # synthetic two-speaker meeting → live → final → notes
```

## Where things live

| Path | What |
|---|---|
| `core/crates/miiting-core` | Everything platform-independent: audio pipeline, Whisper, cloud adapters, storage, notes, device recommendations, i18n |
| `core/crates/miiting-ffi` | UniFFI bindings (the API the app uses) |
| `core/tools/miiting-cli` | `bench`, `transcribe`, `simulate`, `doctor`, `seed-demo` |
| `apps/macos` | SwiftUI app: UI, audio capture, Keychain |
| `docs/decisions` | Architecture decisions and why |

After changing the FFI surface, run `scripts/build-core.sh` to regenerate the Swift bindings.

## Guidelines

- **Privacy first.** Nothing leaves the Mac unless the user picked a cloud provider, and then the host must be recorded in the meeting's `egress`. Never log transcript text, notes or API keys.
- **No telemetry**, analytics or crash reporting that phones home.
- **Keep the core portable.** Anything that isn't tied to an Apple API belongs in Rust so a Windows app can reuse it.
- **Measure accuracy changes.** If you touch transcription, run `make bench` (FLEURS) and `make e2e` and include the numbers in the pull request.
- **Tests.** Add unit tests for core logic. CI runs `make lint` (rustfmt, clippy with warnings as errors, localization check) and `make test` on every pull request and builds a `.dmg` you can download from the run.
- **Commits.** Small, focused commits with an imperative subject line ("Add Silero gate to live chunks").

## Localization

The interface is in English and Indonesian.

**Swift.** Write interface strings in English directly in SwiftUI (`Text("Start recording")`) or with `String(localized:)` for strings built in code. English is the source; translations live in `apps/macos/Resources/<lang>.lproj/Localizable.strings`, keyed by the English text.

- Only interpolate `String` values into localized strings (convert numbers with `String(n)` or a formatter), so every placeholder is `%@`.
- A ternary of two literals (`cond ? "A" : "B"`) is a plain `String` and is **not** translated. Use `String(localized:)` on both branches.
- Use `Text(verbatim:)` for text that must never be translated (commands, product names).
- Run `scripts/check-localization.py` before opening a pull request; it lists missing, unused and mismatched keys.

**Rust.** User-facing text in the core (errors, model descriptions, recommendations) uses `i18n::Text` for fixed strings and `tr!(lang, "English {x}", "Indonesia {x}")` for formatted ones. Errors keep an English `Display` for logs and a `localized(lang)` for the interface.

**Adding a language.** Add a `<lang>.lproj` folder (`scripts/check-localization.py --keys` prints every key), add a variant to `i18n::Lang` (the compiler then points at every Rust string that needs a translation), and list the language in `CFBundleLocalizations` in `Info.plist`.

Language *data* is a different thing: the Indonesian hallucination phrases, number normalization for scoring, and the notes templates per output language are intentionally in Indonesian.

## Reporting bugs

Include your Mac model and RAM (`miiting-cli doctor` prints both), macOS version, the models and providers you use, and the relevant lines from `~/Library/Logs/Miiting/miiting.log`. Please don't attach recordings of real meetings without everyone's consent.
