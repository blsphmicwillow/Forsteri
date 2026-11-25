#!/usr/bin/env bash
# Build the FosteriOS ntpersonality module for the live kernel and install it
# into the ISO's airootfs. The ISO's linux package and the headers must match;
# the default picks the only kernel build tree available.
#
#   scripts/build-nt-module.sh
#   FOSTERIOS_DIR=../FosteriOS FORSTERI_KVER=7.3.5-arch1-1 scripts/build-nt-module.sh
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
FOSTERIOS_DIR=${FOSTERIOS_DIR:-"$ROOT/../FosteriOS"}
DEST="$ROOT/iso/airootfs/usr/lib/forsteri/nt"
KVER=${FORSTERI_KVER:-}

[[ -f $FOSTERIOS_DIR/kernel/nt/pe.c ]] || {
	echo "build-nt-module: FosteriOS source not found at $FOSTERIOS_DIR" >&2
	echo "build-nt-module: set FOSTERIOS_DIR to a FosteriOS checkout" >&2
	exit 1
}

if [[ -z $KVER ]]; then
	builds=(/lib/modules/*/build)
	[[ ${#builds[@]} -eq 1 ]] || {
		echo "build-nt-module: expected one kernel build tree, found ${#builds[@]}" >&2
		echo "build-nt-module: set FORSTERI_KVER to the live kernel version" >&2
		exit 1
	}
	KVER=$(basename "$(dirname "${builds[0]}")")
fi

KDIR="/lib/modules/$KVER/build"
[[ -d $KDIR ]] || {
	echo "build-nt-module: no headers at $KDIR" >&2
	echo "build-nt-module: install linux-headers=$KVER" >&2
	exit 1
}

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp "$FOSTERIOS_DIR/kernel/nt/Makefile" \
   "$FOSTERIOS_DIR/kernel/nt/ntpersonality_main.c" \
   "$FOSTERIOS_DIR/kernel/nt/pe.c" \
   "$FOSTERIOS_DIR/kernel/nt/pe.h" "$STAGE/"

make -C "$KDIR" M="$STAGE" modules
install -Dm644 "$STAGE/ntpersonality.ko" "$DEST/ntpersonality.ko"
echo "build-nt-module: installed $DEST/ntpersonality.ko for $KVER"
