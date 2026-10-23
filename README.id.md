# Miiting

**Notulen rapat otomatis untuk macOS, dioptimalkan untuk Bahasa Indonesia. Tanpa bot yang ikut rapat.**

Miiting merekam rapat Zoom, Google Meet, Teams, atau rapat tatap muka langsung dari Mac (mikrofon + audio sistem), mentranskripnya dengan Whisper yang berjalan di chip Apple, lalu menyusun notulen berisi ringkasan, keputusan, dan action item. Semua diproses lokal secara default. API pihak ketiga bersifat opsional, dan setiap rapat menunjukkan apakah datanya pernah keluar dari Mac.

[English](README.md)

## Unduh

Unduh `.dmg` terbaru dari [Releases](https://github.com/mrasyid22/miiting/releases). Versi *Nightly* dibuat otomatis dari setiap push ke `main`. Miiting butuh Mac Apple Silicon dengan macOS 26 atau lebih baru.

Selama rilis belum dinotarisasi, macOS akan memblokir app saat pertama dibuka: buka Pengaturan Sistem › Privasi & Keamanan lalu klik *Tetap Buka* (atau jalankan `xattr -dr com.apple.quarantine /Applications/Miiting.app`).

## Fitur

- **Tanpa bot.** Suara peserta diambil lewat Core Audio Process Tap (macOS 14.2+). Tidak perlu BlackHole atau driver virtual, dan tidak ada peserta tambahan di rapat.
- **Dua channel, dua pembicara.** Mikrofon diberi label *Saya* dan audio sistem *Peserta*, tanpa perlu diarization. Gema suara peserta yang tertangkap mikrofon dibuang.
- **Draf live, lalu transkrip final yang akurat.** Whisper Large v3 Turbo mentranskrip secara live. Setelah rapat, seluruh rekaman ditranskrip ulang dengan Large v3 (beam search + Silero VAD) dan menggantikan draf.
- **Dioptimalkan untuk Bahasa Indonesia.** Bahasa rapat dipilih di sebelah tombol Rekam (Indonesia, Inggris, Jawa, Sunda, atau Melayu) dan dikunci selama rapat, halusinasi khas Whisper ("Terima kasih telah menonton", kredit subtitle, pengulangan) disaring, glosarium membantu ejaan nama dan istilah, dan template notulen tersedia dalam Bahasa Indonesia dan Inggris.
- **Menyesuaikan Mac kamu.** Miiting membaca chip, RAM, dan sisa disk, lalu menyarankan model, beam size, dan LLM lokal yang pas — dari MacBook Air 8 GB sampai MacBook Pro 64 GB.
- **Penyedia bebas.**
  - Transkripsi: Whisper lokal, atau server apa pun yang kompatibel dengan `/audio/transcriptions` OpenAI (Groq, OpenAI, speaches, LocalAI).
  - Notulen: Ollama, LM Studio, Claude, OpenAI, OpenRouter, Groq, Gemini, atau endpoint OpenAI-compatible lain.
- **Privasi yang terlihat.** Setiap rapat mencatat host mana saja yang menerima datanya. API key disimpan di Keychain. Tanpa telemetri.
- **Lainnya:** pencarian full-text untuk judul, transkrip, dan notulen; edit dan ekspor Markdown; impor audio dan video; kontrol dari menu bar; aksi URL `miiting://`; pemulihan setelah crash; tampilan dalam Bahasa Inggris dan Indonesia.

## Build

Kebutuhan: Mac Apple Silicon dengan macOS 26 atau lebih baru, Rust (`brew install rustup && rustup default stable`), `cmake`, dan Swift 6.2 (Command Line Tools sudah cukup).

```bash
make app        # build/Miiting.app
make run        # build lalu buka
make install    # salin ke /Applications
make test       # unit test Rust
make demo       # buka app dengan data contoh, data asli tidak tersentuh
```

## Model

Model diunduh dari dalam app (Pengaturan › Model) atau lewat `make models`. Rekomendasi diterapkan saat pertama dibuka dan bisa diubah kapan saja. `miiting-cli doctor` menampilkannya di terminal.

| Kelas Mac | Live | Final | Notulen lokal (Ollama) |
|---|---|---|---|
| Ringan (8 GB) | Turbo Q5 | Large v3 Q5, beam 3 | `gemma3:4b` (disarankan pakai cloud) |
| Standar (16 GB) | Turbo Q5 | Large v3, beam 5* | `qwen3:8b` |
| Tinggi (18–36 GB) | Turbo Q5 | Large v3, beam 5* | `gemma3:12b` |
| Maksimal (48 GB+) | Large v3, tanpa beam | Large v3, beam 5 | `gemma3:27b` |

\* Chip M-series biasa (bukan Pro/Max/Ultra) memakai beam 3 supaya transkrip final tidak terlalu lama. Kalau disk hampir penuh, Large v3 diganti Large v3 Q5.

## Akurasi

Di FLEURS Indonesia, Large v3 dengan beam 5 mencapai WER 5,5% dan Turbo Q5 7,1%. Angka lengkap dan metodenya ada di [docs/benchmarks.md](docs/benchmarks.md).

## Notulen lokal dengan Ollama

Pasang Ollama dari [ollama.com](https://ollama.com/download) (atau `brew install ollama`). Cukup itu saja: saat menulis notulen, Miiting menyalakan Ollama kalau belum berjalan dan mengunduh model yang dibutuhkan, lengkap dengan progresnya. Pengaturan › Notulen menampilkan status Ollama dan bisa dipakai untuk mengunduh model lebih dulu.

## Otomasi

```bash
open "miiting://record/start"                 # juga: stop, toggle, pause
open "miiting://record/start?language=en"     # pilih bahasa rapat
open "miiting://meeting/<id>/retranscribe"    # atau /summarize
```

## Kontribusi

Lihat [CONTRIBUTING.md](CONTRIBUTING.md) (dalam bahasa Inggris). Kode, komentar, dan dokumentasi ditulis dalam bahasa Inggris supaya siapa pun bisa berkontribusi. Tampilan app dilokalisasi ke bahasa Inggris dan Indonesia.

## Lisensi

MIT — lihat [LICENSE](LICENSE). Model dan dataset memiliki lisensi masing-masing (Whisper: MIT, Silero VAD: MIT, FLEURS: CC BY 4.0).
