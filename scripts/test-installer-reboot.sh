#!/usr/bin/env bash
# Boot the live ISO twice with the same virtio disk. The first boot writes an
# installer state file to the target and reboots; the second boot must discover
# it from the unmounted disk and print PASS. This is a real VM reboot, not a
# process kill.
#
#   scripts/test-installer-reboot.sh
#   ISO=out/forsteri.iso TIMEOUT=300 scripts/test-installer-reboot.sh
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
ISO=${ISO:-$(ls -t "$ROOT"/out/*.iso 2>/dev/null | head -1)}
QEMU=${QEMU:-qemu-system-x86_64}
WORK=${WORK:-"$ROOT/build/installer-reboot-test"}
TIMEOUT=${TIMEOUT:-300}

[[ -n $ISO && -f $ISO ]] || { echo "installer-reboot: no ISO found" >&2; exit 1; }
command -v "$QEMU" >/dev/null || { echo "installer-reboot: $QEMU is not installed" >&2; exit 1; }
command -v qemu-img >/dev/null || { echo "installer-reboot: qemu-img is not installed" >&2; exit 1; }
command -v xorriso >/dev/null || { echo "installer-reboot: xorriso is not installed" >&2; exit 1; }

rm -rf "$WORK"
mkdir -p "$WORK"
xorriso -osirrox on -indev "$ISO" \
  -extract /forsteri/boot/x86_64/vmlinuz-linux "$WORK/vmlinuz-linux" \
  -extract /forsteri/boot/x86_64/initramfs-linux.img "$WORK/initramfs-linux.img" \
  >/dev/null 2>&1 || { echo "installer-reboot: cannot extract kernel/initramfs" >&2; exit 1; }

label=$(blkid -s LABEL -o value "$ISO")
[[ -n $label ]] || { echo "installer-reboot: ISO has no label" >&2; exit 1; }

qemu-img create -f raw "$WORK/disk.img" 3G >/dev/null
accel=(-accel tcg)
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
  accel=(-accel kvm)
fi

boot() {
  local log=$1
  set +e
  timeout "$TIMEOUT" "$QEMU" "${accel[@]}" -cpu max -m 2048 -smp 2 \
    -display none -no-reboot -boot order=d \
    -cdrom "$ISO" -kernel "$WORK/vmlinuz-linux" \
    -initrd "$WORK/initramfs-linux.img" \
    -drive file="$WORK/disk.img",if=virtio,format=raw,cache=unsafe \
    -append "archisobasedir=forsteri archisolabel=$label console=ttyS0 forsteri.install_reboot_test=1" \
    -serial "file:$log"
  local status=$?
  set -e
  if [[ $status -eq 124 ]]; then
    echo "installer-reboot: QEMU hit the ${TIMEOUT}s timeout ($log)" >&2
  fi
}

first="$WORK/first.log"
boot "$first"
if ! grep -q 'FORSTERI-INSTALL-REBOOT-TEST: REBOOTING' "$first"; then
  echo "installer-reboot: first boot did not reach the reboot marker ($first)" >&2
  grep -E 'FORSTERI-INSTALL-REBOOT-TEST|installer|sgdisk|mount|systemd' "$first" | tail -30 || true
  exit 1
fi

second="$WORK/second.log"
boot "$second"
if ! grep -q 'FORSTERI-INSTALL-REBOOT-TEST: PASS' "$second"; then
  echo "installer-reboot: second boot did not find/resume the state ($second)" >&2
  grep -E 'FORSTERI-INSTALL-REBOOT-TEST|resume state|phase=|FAIL|mount|systemd' "$second" | tail -40 || true
  exit 1
fi

echo "installer-reboot: PASS (real VM reboot; target state rediscovered)"
