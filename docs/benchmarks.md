# Benchmarks

Hardware: MacBook Pro, Apple M1 Pro, 32 GB · whisper.cpp with Metal · October 2026

## FLEURS Indonesian

The first 200 utterances of the FLEURS `id_id` test split (read speech, CC BY 4.0), one utterance per inference (the same path live transcription uses). WER and CER are computed after Indonesian normalization: lowercase, punctuation removed, numbers spelled out ("2024" → "dua ribu dua puluh empat"), "%" → "persen", "Rp" → "rupiah", hyphens split.

| Model | Beam | WER | CER | RTF |
|---|---|---|---|---|
| Large v3 Turbo Q5 | 1 | 7.13% | 2.19% | 0.084 |
| Large v3 Turbo Q5 | 5 | 6.28% | 1.86% | 0.118 |
| Large v3 Turbo Q8 | 1 | 7.00% | 2.11% | 0.081 |
| Large v3 Turbo Q8 | 5 | 6.51% | 1.96% | 0.091 |
| Large v3 | 1 | 5.69% | 1.61% | 0.150 |
| **Large v3** | **5** | **5.46%** | **1.55%** | 0.148 |

RTF is processing time divided by audio duration (below 1 is faster than real time).

What we took from it:
- Turbo Q8 is no more accurate than Q5, so the live default is the smaller Q5.
- Large v3 makes about 19% fewer errors than Turbo, so it does the final transcript.
- Most remaining "errors" are spelling variants rather than mishearings: *samudra / samudera*, *termal / thermal*, "(USGS)" read as "dalam kurung USGS".
- Short utterances are relatively expensive because Whisper's encoder always processes a 30 s window; on whole meetings the final pass runs at an RTF of about 0.2–0.3.

Reproduce:

```bash
make models fleurs
make bench BENCH_MODELS="large-v3-turbo-q5_0 large-v3" BEAMS=1,5 LIMIT=200
```

## End-to-end meeting

`make e2e` synthesizes an 85-second, 13-sentence Indonesian meeting with macOS's Damayanti voice (two channels: *Me* on the microphone, *Them* on system audio; numbers, names and English terms included) and streams it through the engine the way the app does (48 kHz device buffers, at 4× real time).

| Pass | WER | Time |
|---|---|---|
| Live (Turbo Q5, greedy, Silero gate) | 3.1% | 1–2 s behind speech |
| Final (Large v3, beam 5, Silero windows) | 2.5% | 18 s for the 85 s meeting |
| Notes (Gemma 3 12B via Ollama) | — | ~36 s |

The same audio played through the speakers of a MacBook in a noisy room and captured by the app (microphone + process tap) gave a final WER of 1.9%; the Silero gate removed the echo and room noise picked up by the microphone.

FLEURS and the synthetic meeting are clean read speech. Real meetings — overlapping talk, accents, jargon — will score worse; measure your own recordings with `miiting-cli bench --manifest recordings.tsv` (`path<TAB>reference transcript` per line).
