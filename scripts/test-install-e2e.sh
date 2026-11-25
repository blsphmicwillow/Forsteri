#!/usr/bin/env bash
# Build-free end-to-end installer gate:
#
#   1. boot the ISO's kernel/initramfs under UEFI with a fresh virtio disk
#   2. run forsteri-install unattended with a fixed answer set; the installer
#      itself syncs, partitions, formats, installs the base and Hyprland,
#      configures the chroot and installs GRUB
#   3. assert the installed disk contents, then boot that disk through its own
#      GRUB entry
#   4. wait for SDDM's greeter, then autologin the created user into Hyprland
#      and assert the Wayland session is active
#
# The harness never writes an installer phase file. The answers file only
# replaces dialog input; every install step is performed by forsteri-install.
#
#   scripts/test-install-e2e.sh
#   ISO=out/forsteri.iso E2E_TIMEOUT=3600 scripts/test-install-e2e.sh
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
ISO=${1:-${ISO:-$(ls -t "$ROOT"/out/*.iso 2>/dev/null | head -1)}}
QEMU=${QEMU:-qemu-system-x86_64}
WORK=${WORK:-"$ROOT/build/installer-e2e"}
TIMEOUT=${E2E_TIMEOUT:-3600}
BOOT_TIMEOUT=${E2E_BOOT_TIMEOUT:-600}
DISK_SIZE=${E2E_DISK_SIZE:-10G}
E2E_RAM=${E2E_RAM:-6144}
E2E_USER=${E2E_USER:-forsteri}
E2E_SESSION=${E2E_SESSION:-hyprland.desktop}

fail() {
  printf 'installer-e2e: FAIL: %s\n' "$*" >&2
  exit 1
}
ok() { printf 'ok   %s\n' "$*"; }

[[ -n $ISO && -f $ISO ]] || fail "no ISO found"
for tool in "$QEMU" qemu-img xorriso blkid timeout sed grep awk python3; do
  command -v "$tool" >/dev/null 2>&1 || fail "$tool is not installed"
done
if (( EUID == 0 )); then
  SUDO=()
else
  SUDO=(sudo -n)
  "${SUDO[@]}" true 2>/dev/null || fail "loop-mounting the installed disk needs root or passwordless sudo"
fi

ovmf_code=${OVMF_CODE:-}
ovmf_vars=${OVMF_VARS:-}
if [[ -z $ovmf_code ]]; then
  for candidate in \
    /usr/share/OVMF/OVMF_CODE_4M.fd \
    /usr/share/OVMF/OVMF_CODE.fd \
    /usr/share/edk2/x64/OVMF.4m.fd \
    /usr/share/edk2/x64/OVMF_CODE.fd \
    /usr/share/edk2-ovmf/x64/OVMF_CODE.fd; do
    [[ -r $candidate ]] && {
      ovmf_code=$candidate
      break
    }
  done
fi
[[ -n $ovmf_code && -r $ovmf_code ]] ||
  fail "no OVMF firmware found (install ovmf, or set OVMF_CODE)"
if [[ -z $ovmf_vars ]]; then
  case $ovmf_code in
    *OVMF_CODE_4M.fd) ovmf_vars=${ovmf_code%OVMF_CODE_4M.fd}OVMF_VARS_4M.fd ;;
    *OVMF.4m.fd) ovmf_vars=${ovmf_code%OVMF.4m.fd}OVMF_VARS.4m.fd ;;
    *OVMF_CODE.fd) ovmf_vars=${ovmf_code%OVMF_CODE.fd}OVMF_VARS.fd ;;
    *) ovmf_vars=${ovmf_code%/*}/OVMF_VARS.fd ;;
  esac
fi
[[ -r $ovmf_vars ]] || fail "OVMF variables template not found: $ovmf_vars"

rm -rf "$WORK"
mkdir -p "$WORK/mnt"
LOOP=""
cleanup() {
  set +e
  "${SUDO[@]}" umount -R "$WORK/mnt" 2>/dev/null
  [[ -n $LOOP ]] && "${SUDO[@]}" losetup -d "$LOOP" 2>/dev/null
  set -e
}
trap cleanup EXIT

echo "== extracting the ISO kernel and initramfs"
xorriso -osirrox on -indev "$ISO" \
  -extract /forsteri/boot/x86_64/vmlinuz-linux "$WORK/vmlinuz-linux" \
  -extract /forsteri/boot/x86_64/initramfs-linux.img "$WORK/initramfs-linux.img" \
  >/dev/null 2>&1 || fail "could not extract the archiso kernel/initramfs from $ISO"
label=$(blkid -s LABEL -o value "$ISO")
[[ -n $label ]] || fail "the ISO has no label"
ok "kernel, initramfs and label ($label)"

qemu-img create -f raw "$WORK/disk.img" "$DISK_SIZE" >/dev/null
cp "$ovmf_vars" "$WORK/OVMF_VARS.fd"
accel=(-accel tcg)
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
  accel=(-accel kvm)
fi

# qemu_run <log> <timeout> <extra qemu args...>
qemu_run() {
  local log=$1 limit=$2
  shift 2
  set +e
  timeout "$limit" "$QEMU" "${accel[@]}" -cpu max -m "$E2E_RAM" -smp 2 \
    -display none -no-reboot -vga qxl \
    -drive if=pflash,format=raw,readonly=on,file="$ovmf_code" \
    -drive if=pflash,format=raw,file="$WORK/OVMF_VARS.fd" \
    "$@" -serial "file:$log" 2>>"$log"
  QEMU_STATUS=$?
  set -e
}

live_log="$WORK/live-install.log"
echo "== booting the ISO and running the installer (UEFI, ${DISK_SIZE} disk)"
qemu_run "$live_log" "$TIMEOUT" \
  -boot order=c \
  -kernel "$WORK/vmlinuz-linux" \
  -initrd "$WORK/initramfs-linux.img" \
  -append "archisobasedir=forsteri archisolabel=$label console=ttyS0,115200 forsteri.install_e2e=1" \
  -drive file="$WORK/disk.img",if=virtio,format=raw,cache=unsafe \
  -drive file="$ISO",if=none,format=raw,readonly=on,id=isodisk \
  -device virtio-blk-pci,drive=isodisk \
  -nic user,model=virtio-net-pci

if [[ -f $live_log ]] && grep -q 'FORSTERI-INSTALL: UNATTENDED FAIL' "$live_log"; then
  grep -E 'FORSTERI-INSTALL|fatal:|ERROR|error:' "$live_log" | tail -40 || true
  fail "the installer reported a failure (see $live_log)"
fi
if ! grep -q 'FORSTERI-INSTALL: UNATTENDED COMPLETE' "$live_log" 2>/dev/null; then
  echo "--- last 60 lines of $live_log ---" >&2
  tail -60 "$live_log" >&2 || true
  fail "the live boot did not finish the unattended install (QEMU status $QEMU_STATUS)"
fi
ok "installer completed partition -> base -> desktop -> bootloader"

echo "== checking the installed disk"
"${SUDO[@]}" mkdir -p "$WORK/mnt"
LOOP=$("${SUDO[@]}" losetup --find --show --partscan "$WORK/disk.img")
for _ in $(seq 1 50); do
  [[ -b ${LOOP}p1 && -b ${LOOP}p2 ]] && break
  "${SUDO[@]}" partx -u "$LOOP" >/dev/null 2>&1 || true
  sleep 0.2
done
[[ -b ${LOOP}p1 && -b ${LOOP}p2 ]] || fail "installed disk partitions did not appear on $LOOP"
"${SUDO[@]}" mount "${LOOP}p2" "$WORK/mnt"
"${SUDO[@]}" mount "${LOOP}p1" "$WORK/mnt/efi"

checked=0
failed=0
check() {
  local description=$1
  shift
  checked=$((checked + 1))
  if "$@"; then
    ok "$description"
  else
    printf 'FAIL %s\n' "$description" >&2
    failed=$((failed + 1))
  fi
}
has_file() { "${SUDO[@]}" test -e "$WORK/mnt/$1"; }
has_grep() { "${SUDO[@]}" grep -Eq -- "$2" "$WORK/mnt/$1"; }
has_line() { "${SUDO[@]}" grep -qx -- "$2" "$WORK/mnt/$1"; }
is_symlink_to_sddm() {
  local target
  target=$("${SUDO[@]}" readlink "$WORK/mnt/etc/systemd/system/display-manager.service" 2>/dev/null || true)
  [[ $target == *sddm* ]]
}
grub_efi_exists() {
  "${SUDO[@]}" find "$WORK/mnt/efi/EFI" -iname grubx64.efi -print -quit 2>/dev/null | grep -q .
}

check "resume state says complete" has_line .forsteri-install.state phase=complete
check "EFI mirror of the state exists" has_file efi/.forsteri-install.state
check "fstab mounts / by UUID" has_grep etc/fstab '^UUID=[^ ]+ +/ +'
check "fstab mounts /efi by UUID" has_grep etc/fstab '^UUID=[^ ]+ +/efi +'
check "installed kernel" has_file boot/vmlinuz-linux
check "installed initramfs" has_file boot/initramfs-linux.img
check "GRUB config" has_file boot/grub/grub.cfg
check "GRUB EFI binary on the ESP" grub_efi_exists
check "Hyprland binary" has_file usr/bin/Hyprland
check "SDDM binary" has_file usr/bin/sddm
check "Hyprland session entry" has_file usr/share/wayland-sessions/hyprland.desktop
check "SDDM theme" has_file usr/share/sddm/themes/forsteri/Main.qml
check "SDDM theme selected" has_grep etc/sddm.conf.d/10-forsteri.conf '^Current=forsteri$'
check "display-manager enabled for sddm" is_symlink_to_sddm
check "user home" has_file "home/$E2E_USER"
check "Hyprland user config" has_file "home/$E2E_USER/.config/hypr/hyprland.conf"
check "Waybar user config" has_file "home/$E2E_USER/.config/waybar/config.jsonc"
check "hostname" has_line etc/hostname forsteri-e2e
check "locale" has_line etc/locale.conf LANG=en_US.UTF-8
check "theme codename" has_line etc/forsteri/theme ember
check "user account" has_grep etc/shadow "^$E2E_USER:"
check "wheel sudoers rule" has_grep etc/sudoers.d/10-wheel '^%wheel ALL='
has_no_file() { ! "${SUDO[@]}" test -e "$WORK/mnt/$1"; }
check "installer credentials removed" has_no_file root/forsteri-install.env

"${SUDO[@]}" cp "$WORK/mnt/var/log/forsteri-install.log" "$WORK/install.log" 2>/dev/null || true
"${SUDO[@]}" chown "$(id -u):$(id -g)" "$WORK/install.log" 2>/dev/null || true

if (( failed > 0 )); then
  echo "installer-e2e: $failed of $checked installed-system checks failed" >&2
  exit 1
fi
ok "all $checked installed-system checks passed"

# The installed system is complete. Add a read-only observer and a serial
# console so the second boot can prove SDDM and the desktop session start.
echo "== instrumenting the installed disk for the boot check"
grub_cfg="$WORK/mnt/boot/grub/grub.cfg"
"${SUDO[@]}" sed -i -E \
  's|^([[:space:]]*linux(efi)?[[:space:]].*)$|\1 console=ttyS0,115200|' "$grub_cfg"
{
  printf '%s\n' 'insmod serial'
  printf '%s\n' 'serial --unit=0 --speed=115200'
  printf '%s\n' 'terminal_output console serial'
  "${SUDO[@]}" cat "$grub_cfg"
} >"$WORK/grub.cfg"
"${SUDO[@]}" cp "$WORK/grub.cfg" "$grub_cfg"

cat >"$WORK/forsteri-e2e-probe" <<'PROBE'
#!/usr/bin/env bash
# Observer installed only by scripts/test-install-e2e.sh. It does not install
# anything; it waits for the installed system's own SDDM and session.
set -uo pipefail
exec >>/dev/ttyS0 2>&1

USER_NAME=${FORSTERI_E2E_USER:?}
SESSION=${FORSTERI_E2E_SESSION:?}

marker() { printf 'FORSTERI-E2E-BOOT: %s\n' "$*"; }
fail() {
  marker "FAIL: $*"
  systemctl --no-pager status sddm.service >/dev/null 2>&1 || true
  journalctl -b --no-pager -n 120 || true
  systemctl poweroff -f || true
  exit 1
}

marker "probe started"
for _ in $(seq 1 180); do
  systemctl is-active --quiet sddm.service && break
  sleep 1
done
systemctl is-active --quiet sddm.service ||
  fail "sddm.service did not become active"
marker "SDDM ACTIVE"

for _ in $(seq 1 90); do
  pgrep -f sddm-greeter >/dev/null && break
  sleep 1
done
pgrep -af sddm-greeter || fail "no SDDM greeter process"
marker "GREETER RUNNING"

sleep 5
install -d -m 755 /etc/sddm.conf.d
cat >/etc/sddm.conf.d/20-e2e-autologin.conf <<EOF
[Autologin]
User=$USER_NAME
Session=$SESSION
Relogin=false
EOF
systemctl restart sddm.service || fail "could not restart sddm"

session_id=""
session_type=""
for _ in $(seq 1 180); do
  session_id=$(loginctl list-sessions --no-legend 2>/dev/null |
    awk -v u="$USER_NAME" '$3 == u { print $1; exit }')
  if [[ -n $session_id ]]; then
    session_type=$(loginctl show-session "$session_id" -p Type --value 2>/dev/null || true)
    active=$(loginctl show-session "$session_id" -p Active --value 2>/dev/null || true)
    [[ $session_type == wayland && $active == yes ]] && break
  fi
  sleep 1
done
[[ -n $session_id ]] || fail "no session for $USER_NAME after autologin"
[[ $session_type == wayland ]] ||
  fail "session $session_id is ${session_type:-unknown}, not wayland"
pgrep -x Hyprland >/dev/null || fail "Hyprland is not running"
pgrep -f waybar >/dev/null || fail "Waybar is not running"

marker "PASS (sddm greeter rendered; $USER_NAME wayland session; Hyprland and Waybar running)"
sync
systemctl poweroff -f || true
PROBE
"${SUDO[@]}" install -Dm755 "$WORK/forsteri-e2e-probe" \
  "$WORK/mnt/usr/local/bin/forsteri-e2e-probe"

cat >"$WORK/forsteri-e2e-probe.service" <<UNIT
[Unit]
Description=Forsteri end-to-end boot probe
After=sddm.service
Wants=sddm.service

[Service]
Type=oneshot
Environment=FORSTERI_E2E_USER=$E2E_USER
Environment=FORSTERI_E2E_SESSION=$E2E_SESSION
ExecStart=/usr/local/bin/forsteri-e2e-probe
TimeoutStartSec=0
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
UNIT
# QXL has KMS but no 3D accelerator in this CI VM. Force Mesa's software
# renderer for SDDM and the session, and tell aquamarine not to require DRM
# buffer modifiers. This is test instrumentation, not part of the install.
printf '%s\n' 'LIBGL_ALWAYS_SOFTWARE=1' 'AQ_NO_MODIFIERS=1' >"$WORK/e2e-environment"
"${SUDO[@]}" install -Dm644 "$WORK/e2e-environment" "$WORK/mnt/etc/environment"
cat >"$WORK/10-e2e-software.conf" <<'DROPIN'
[Service]
Environment=LIBGL_ALWAYS_SOFTWARE=1
Environment=AQ_NO_MODIFIERS=1
DROPIN
"${SUDO[@]}" install -Dm644 "$WORK/10-e2e-software.conf" \
  "$WORK/mnt/etc/systemd/system/sddm.service.d/10-e2e-software.conf"

"${SUDO[@]}" install -Dm644 "$WORK/forsteri-e2e-probe.service" \
  "$WORK/mnt/etc/systemd/system/forsteri-e2e-probe.service"
"${SUDO[@]}" ln -sf ../forsteri-e2e-probe.service \
  "$WORK/mnt/etc/systemd/system/multi-user.target.wants/forsteri-e2e-probe.service"

sync
"${SUDO[@]}" umount -R "$WORK/mnt"
"${SUDO[@]}" losetup -d "$LOOP"
LOOP=""

boot_log="$WORK/installed-boot.log"
echo "== booting the installed disk through its own UEFI GRUB entry"
qemu_run "$boot_log" "$BOOT_TIMEOUT" \
  -boot order=c \
  -drive file="$WORK/disk.img",if=virtio,format=raw,cache=unsafe \
  -nic user,model=virtio-net-pci

if grep -q 'FORSTERI-E2E-BOOT: FAIL' "$boot_log"; then
  grep -E 'FORSTERI-E2E-BOOT|sddm|Hyprland|waybar|Failed' "$boot_log" | tail -60 || true
  fail "the installed system did not reach a working SDDM session (see $boot_log)"
fi
if ! grep -q 'FORSTERI-E2E-BOOT: PASS' "$boot_log"; then
  echo "--- last 80 lines of $boot_log ---" >&2
  tail -80 "$boot_log" >&2 || true
  fail "the installed disk did not boot to the login/session check (QEMU status $QEMU_STATUS)"
fi

echo "installer-e2e: PASS"
echo "  live install log:     $live_log"
echo "  installed boot log:   $boot_log"
echo "  installer log copy:   $WORK/install.log"
echo "  covered: UEFI, ext4, Hyprland profile, en_US.UTF-8, UTC, GRUB,"
echo "           SDDM greeter process and a Wayland Hyprland session"
