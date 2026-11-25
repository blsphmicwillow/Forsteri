#!/usr/bin/env bash
# Run the deterministic index-parser fuzz corpus for longer than `cargo test`.
#
#   scripts/fuzz-forst.sh         5000 iterations
#   scripts/fuzz-forst.sh 50000   a longer soak
#
# This is an AFL-style mutation loop built into the test (truncated real
# structures plus pseudo-random noise), not a libFuzzer target: forst has no
# crate dependencies, and the three parsers are pure safe Rust.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
ITERS=${1:-5000}
[[ $ITERS =~ ^[0-9]+$ ]] || {
  echo "fuzz-forst: iteration count must be a positive integer" >&2
  exit 2
}

cd "$ROOT/pkg/forst"
FORST_FUZZ_ITERS=$ITERS cargo test --quiet parsers_survive_hostile_input -- --nocapture
echo "fuzz-forst: $ITERS iterations, no panics"
