#!/bin/zsh
# Download the Indonesian FLEURS split (CC-BY 4.0) for benchmarking.
# Usage: scripts/fetch-fleurs.sh [-y] [split]   (split: test (default) | dev | train)
set -euo pipefail
ROOT="${0:A:h:h}"
YES=0
[[ "${1:-}" == "-y" ]] && { YES=1; shift; }
SPLIT="${1:-test}"
BASE="https://huggingface.co/datasets/google/fleurs/resolve/main/data/id_id"
DEST="$ROOT/data/fleurs/id_id"

size=$(curl -sIL "$BASE/audio/$SPLIT.tar.gz" | grep -i '^content-length' | tail -1 | tr -d '\r' | awk '{print $2}')
echo "FLEURS id_id/$SPLIT: ~$(( ${size:-0} / 1048576 )) MB of audio + transcripts → $DEST"
echo "License: CC BY 4.0 (Google). Read sentences, not meeting conversation."
if (( ! YES )); then
  read -q "?Download now? [y/N] " || { echo; exit 0; }
  echo
fi
mkdir -p "$DEST/audio"
curl -L --fail -o "$DEST/$SPLIT.tsv" "$BASE/$SPLIT.tsv"
curl -L --fail --progress-bar -o "$DEST/audio/$SPLIT.tar.gz" "$BASE/audio/$SPLIT.tar.gz"
tar -xzf "$DEST/audio/$SPLIT.tar.gz" -C "$DEST/audio"
rm "$DEST/audio/$SPLIT.tar.gz"
echo "✓ $(ls "$DEST/audio/$SPLIT" | wc -l | tr -d ' ') audio files in $DEST/audio/$SPLIT"
