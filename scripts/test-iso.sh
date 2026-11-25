#!/usr/bin/env bash
# Boot the real ISO's kernel, initramfs and squashfs in QEMU and run the
# ntpersonality serial self-test (forsteri.nt_test=1). This is not the
# synthetic M0 initramfs: the module comes from the live airootfs.
#
#   scripts/test-iso.sh
#   ISO=out/forsteri.iso scripts/test-iso.sh
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
ISO=${ISO:-$(ls -t "$ROOT"/out/*.iso 2>/dev/null | head -1)}
QEMU=${QEMU:-qemu-system-x86_64}
WORK=${WORK:-"$ROOT/build/iso-test"}
TIMEOUT=${TIMEOUT:-420}

[[ -n $ISO && -f $ISO ]] || { echo "test-iso: no ISO found (build one first)" >&2; exit 1; }
command -v "$QEMU" >/dev/null || { echo "test-iso: $QEMU not installed" >&2; exit 1; }

rm -rf "$WORK"
mkdir -p "$WORK"
xorriso -osirrox on -indev "$ISO" \
	-extract /forsteri/boot/x86_64/vmlinuz-linux "$WORK/vmlinuz-linux" \
	-extract /forsteri/boot/x86_64/initramfs-linux.img "$WORK/initramfs-linux.img" \
	>/dev/null 2>&1 || {
	echo "test-iso: could not extract the archiso kernel/initramfs from $ISO" >&2
	exit 1
}

label=$(blkid -s LABEL -o value "$ISO")
[[ -n $label ]] || { echo "test-iso: ISO has no label" >&2; exit 1; }

LOG="$WORK/serial.log"
accel=(-accel tcg)
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
	accel=(-accel kvm)
fi

set +e
timeout "$TIMEOUT" "$QEMU" "${accel[@]}" -cpu max -m 2048 -smp 2 \
	-display none -no-reboot -boot order=d \
	-cdrom "$ISO" -kernel "$WORK/vmlinuz-linux" \
	-initrd "$WORK/initramfs-linux.img" \
	-append "archisobasedir=forsteri archisolabel=$label console=ttyS0 forsteri.nt_test=1" \
	-serial "file:$LOG"
status=$?
set -e

if [[ $status -eq 124 ]]; then
	echo "test-iso: QEMU hit the ${TIMEOUT}s timeout" >&2
fi

if grep -q 'FORSTERI-NT-TEST: PASS' "$LOG"; then
	echo "test-iso: PASS ($LOG)"
	exit 0
fi

echo "test-iso: FAIL ($LOG)"
grep -E 'FORSTERI-NT-TEST|ntpersonality|archiso|Kernel panic|not syncing' "$LOG" | tail -40 || true
exit 1
