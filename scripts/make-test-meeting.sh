#!/bin/zsh
# Synthesize a two-channel Indonesian test meeting with macOS's Damayanti voice.
# Output (data/test-meeting/): mic.wav, system.wav (48 kHz, aligned), mixed.wav, reference*.txt
set -euo pipefail
ROOT="${0:A:h:h}"
SCRIPT="${1:-$ROOT/tests/fixtures/weekly-meeting.id.tsv}"
OUT="$ROOT/data/test-meeting"
say -v '?' | grep -q '^Damayanti' || { echo "The Damayanti (id_ID) voice isn't installed: System Settings › Accessibility › Spoken Content › System Voice."; exit 1; }
mkdir -p "$OUT"
TMP="$(mktemp -d)"
i=0
while IFS=$'\t' read -r who text; do
  [[ -z "$who" ]] && continue
  i=$((i + 1))
  rate=175; [[ "$who" == system ]] && rate=190
  say -v Damayanti -r $rate --file-format=WAVE --data-format=LEI16@48000 -o "$TMP/$(printf %02d $i)-$who.wav" "$text"
done < "$SCRIPT"
python3 -I - "$SCRIPT" "$TMP" "$OUT" <<'PY'
import sys, wave, struct, random, os, glob
script, tmp, out = sys.argv[1:4]
rate = 48000
lines = [l.rstrip('\n').split('\t') for l in open(script, encoding='utf-8') if l.strip()]
mic, sysch = [], []
def pad(sec):
    n = int(sec * rate); mic.extend([0] * n); sysch.extend([0] * n)
pad(1.0)
for (who, _), f in zip(lines, sorted(glob.glob(os.path.join(tmp, '*.wav')))):
    w = wave.open(f, 'rb'); data = struct.unpack('<%dh' % w.getnframes(), w.readframes(w.getnframes()))
    (mic if who == 'mic' else sysch).extend(data)
    (sysch if who == 'mic' else mic).extend([0] * len(data))
    pad(0.8)
pad(1.0)
random.seed(7)
mic = [max(-32768, min(32767, int(s * 0.8 + random.gauss(0, 30)))) for s in mic]  # quiet room noise
def write(name, ch, samples):
    w = wave.open(os.path.join(out, name), 'wb'); w.setnchannels(ch); w.setsampwidth(2); w.setframerate(rate)
    w.writeframes(struct.pack('<%dh' % len(samples), *samples)); w.close()
write('mic.wav', 1, mic)
write('system.wav', 2, [v for s in sysch for v in (s, s)])
write('mixed.wav', 1, [max(-32768, min(32767, a + b)) for a, b in zip(mic, sysch)])
for suffix, keep in (('', None), ('-mic', 'mic'), ('-system', 'system')):
    text = ' '.join(t for w_, t in lines if keep is None or w_ == keep)
    open(os.path.join(out, f'reference{suffix}.txt'), 'w', encoding='utf-8').write(text + '\n')
print(f"✓ {len(mic) / rate:.1f} s → {out}")
PY
rm -rf "$TMP"
