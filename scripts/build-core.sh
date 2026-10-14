#!/bin/zsh
# Build the Rust core as a static library and regenerate the Swift bindings.
set -euo pipefail
ROOT="${0:A:h:h}"
CORE="$ROOT/core"
APP="$ROOT/apps/macos"
PROFILE="${PROFILE:-release}"

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=26.0

echo "› cargo build ($PROFILE)"
if [[ "$PROFILE" == "release" ]]; then
  cargo build --manifest-path "$CORE/Cargo.toml" -p miiting-ffi --release
else
  cargo build --manifest-path "$CORE/Cargo.toml" -p miiting-ffi
fi
TARGET_DIR="$CORE/target/$PROFILE"

echo "› uniffi-bindgen"
GEN="$(mktemp -d)"
# Library mode runs `cargo metadata`, which must see the workspace.
(cd "$CORE" && cargo run -q -p uniffi-bindgen -- \
  generate --library "$TARGET_DIR/libmiiting_ffi.dylib" --language swift --out-dir "$GEN")

mkdir -p "$APP/Sources/MiitingCore" "$APP/Sources/miiting_ffiFFI"
cp "$GEN/miiting_ffi.swift" "$APP/Sources/MiitingCore/miiting_ffi.swift"
cp "$GEN/miiting_ffiFFI.h" "$APP/Sources/miiting_ffiFFI/miiting_ffiFFI.h"
cp "$GEN/miiting_ffiFFI.modulemap" "$APP/Sources/miiting_ffiFFI/module.modulemap"
rm -rf "$GEN"

# Only the static library goes in lib/ so the linker can't pick the dylib.
mkdir -p "$APP/lib"
cp "$TARGET_DIR/libmiiting_ffi.a" "$APP/lib/libmiiting_ffi.a"
echo "✓ core ready"
