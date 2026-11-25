#!/usr/bin/env bash
# Build forst and place it in the live profile.
#
#   scripts/build-forst.sh            release build into iso/airootfs/usr/local/bin
#   scripts/build-forst.sh --test     run the unit tests as well
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
CRATE="$ROOT/pkg/forst"
DEST="$ROOT/iso/airootfs/usr/local/bin/forst"

command -v cargo >/dev/null 2>&1 || {
  echo "build-forst: cargo is required (https://rustup.rs)" >&2
  exit 1
}

if [[ ${1:-} == "--test" ]]; then
  (cd "$CRATE" && cargo test)
fi

(cd "$CRATE" && cargo build --release)
install -Dm755 "$CRATE/target/release/forst" "$DEST"

"$DEST" --version
echo "build-forst: installed $DEST"
