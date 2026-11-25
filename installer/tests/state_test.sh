#!/usr/bin/env bash
# Unit tests for the installer's crash-safe phase file. No root, no disks.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=../../iso/airootfs/usr/lib/forsteri/installer/state.sh
. "$HERE/../../iso/airootfs/usr/lib/forsteri/installer/state.sh"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
FORSTERI_STATE_FILE=$tmp/state

fail=0
ok() { printf 'ok   %s\n' "$*"; }
bad() {
  printf 'FAIL %s\n' "$*"
  fail=1
}

state_set disk /dev/vda
state_set phase partitioned
[[ $(state_get disk) == /dev/vda ]] && ok "disk round-trip" || bad "disk round-trip"
[[ $(state_phase) == partitioned ]] && ok "phase round-trip" || bad "phase round-trip"
[[ $(phase_rank partitioned) == 1 ]] && ok "phase order" || bad "phase order"
state_reached mounted && bad "mounted must not be reached at partitioned" || ok "phase gating"
state_same_disk /dev/vda && ok "same-disk detection" || bad "same-disk detection"
state_same_disk /dev/vdb && bad "wrong disk matched" || ok "wrong-disk rejection"

state_set phase mounted
state_reached partitioned && ok "mounted implies partitioned" || bad "mounted rank"
[[ $(state_get missing) == "" ]] && ok "missing key is empty" || bad "missing key"

# Writes are atomic and replace the old key instead of appending duplicates.
state_set phase unexpected
[[ $(phase_rank "$(state_phase)") == 0 ]] && ok "unknown phase reruns safely" ||
  bad "unknown phase rank"
state_set phase mounted
[[ $(grep -c '^phase=' "$FORSTERI_STATE_FILE") == 1 ]] && ok "state_set replaces keys" ||
  bad "duplicate phase keys"
[[ ! -e $FORSTERI_STATE_FILE.tmp.$$ ]] && ok "atomic state leaves no temp file" ||
  bad "state temp file left behind"

state_clear
[[ ! -e $FORSTERI_STATE_FILE ]] && ok "state_clear" || bad "state_clear"
[[ $(state_phase) == "" ]] && ok "cleared state has no phase" || bad "cleared state"

if (( fail == 0 )); then
  echo "state tests passed"
else
  echo "state tests failed"
fi
exit "$fail"
