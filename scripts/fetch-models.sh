#!/bin/zsh
# Download Whisper / VAD models into the same folder the app uses.
# Usage: scripts/fetch-models.sh [-y] [model-id ...]   (default: the recommended set)
set -euo pipefail
DEST="${MIITING_MODELS_DIR:-$HOME/Library/Application Support/Miiting/models}"
HF="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
YES=0
[[ "${1:-}" == "-y" ]] && { YES=1; shift; }
IDS=("$@")
(( ${#IDS} == 0 )) && IDS=(large-v3-turbo-q5_0 large-v3 silero-vad)

typeset -A URL SIZE
URL[large-v3-turbo-q5_0]="$HF/ggml-large-v3-turbo-q5_0.bin"; SIZE[large-v3-turbo-q5_0]=574041195
URL[large-v3-turbo]="$HF/ggml-large-v3-turbo.bin";           SIZE[large-v3-turbo]=1624555275
URL[large-v3]="$HF/ggml-large-v3.bin";                       SIZE[large-v3]=3095033483
URL[large-v3-q5_0]="$HF/ggml-large-v3-q5_0.bin";             SIZE[large-v3-q5_0]=1081140203
URL[large-v2-q5_0]="$HF/ggml-large-v2-q5_0.bin";             SIZE[large-v2-q5_0]=1080732091
URL[large-v3-turbo-q8_0]="$HF/ggml-large-v3-turbo-q8_0.bin"; SIZE[large-v3-turbo-q8_0]=874188075
URL[small]="$HF/ggml-small.bin";                             SIZE[small]=487601967
URL[silero-vad]="https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"; SIZE[silero-vad]=885098

total=0
for id in "${IDS[@]}"; do
  [[ -z "${URL[$id]:-}" ]] && { echo "unknown model: $id"; exit 1; }
  total=$(( total + SIZE[$id] ))
  printf "  %-22s %6d MB  %s\n" $id $(( SIZE[$id] / 1048576 )) "${URL[$id]}"
done
echo "Total: $(( total / 1048576 )) MB → $DEST"
if (( ! YES )); then
  read -q "?Download now? [y/N] " || { echo; exit 0; }
  echo
fi
mkdir -p "$DEST"
for id in "${IDS[@]}"; do
  file="$DEST/${URL[$id]:t}"
  if [[ -f "$file" && $(stat -f%z "$file") -eq ${SIZE[$id]} ]]; then
    echo "✓ $id already downloaded"; continue
  fi
  curl -L --fail --progress-bar -o "$file.part" "${URL[$id]}"
  [[ $(stat -f%z "$file.part") -eq ${SIZE[$id]} ]] || { echo "size mismatch for $id"; rm -f "$file.part"; exit 1; }
  mv "$file.part" "$file"; echo "✓ $id"
done
