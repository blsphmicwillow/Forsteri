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

# Target attachment is the reboot-safe half: state lives on the target after
# the root is mounted, not only on the live tmpfs.
FORSTERI_STATE_FILE=$tmp/live
state_set disk /dev/vda
root=$tmp/target
mkdir -p "$root"
state_attach_target "$root" && ok "attach target state" || bad "attach target state"
[[ $FORSTERI_STATE_FILE == "$root/.forsteri-install.state" ]] &&
  ok "target state path" || bad "target state path"
[[ $(state_get disk) == /dev/vda ]] && ok "target state round-trip" ||
  bad "target state round-trip"
state_set phase formatted
[[ $(state_get_file "$root/.forsteri-install.state" phase) == formatted ]] &&
  ok "target state updates" || bad "target state updates"

# Simulate the read-only scanner handing back a state file.
STATE_SCAN_FILE=$tmp/scan.state
STATE_SCAN_PART=/dev/vda2
printf 'disk=/dev/vda\nphase=base-installing\nrootpart=/dev/vda2\nfs=ext4\n' \
  >"$STATE_SCAN_FILE"
FORSTERI_STATE_FILE=$tmp/imported
state_import_scan && ok "import scanned state" || bad "import scanned state"
[[ $(state_phase) == base-installing ]] && ok "imported phase" ||
  bad "imported phase"
STATE_SCAN_TMPDIR=$tmp/scan-dir
mkdir -p "$STATE_SCAN_TMPDIR"
state_scan_cleanup
[[ ! -e $STATE_SCAN_TMPDIR && -z $STATE_SCAN_FILE ]] &&
  ok "scan cleanup" || bad "scan cleanup"

if (( fail == 0 )); then
  echo "state tests passed"
else
  echo "state tests failed"
fi
exit "$fail"
