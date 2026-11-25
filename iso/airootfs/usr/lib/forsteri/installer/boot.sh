#!/usr/bin/env bash
# Forsteri installer: GRUB, themed with the Forsteri splash artwork.
#
# GRUB is used for both firmwares on purpose: one code path, one menu look, and
# it boots BIOS machines without a separate configuration.

write_grub_default() {
  cat >"$TARGET/etc/default/grub" <<'GRUB'
GRUB_DEFAULT=0
GRUB_TIMEOUT=3
GRUB_DISTRIBUTOR="Forsteri"
GRUB_CMDLINE_LINUX_DEFAULT="quiet splash loglevel=3"
GRUB_CMDLINE_LINUX=""
GRUB_THEME="/boot/grub/themes/forsteri/theme.txt"
GRUB_BACKGROUND="/boot/grub/themes/forsteri/background.png"
GRUB_DISABLE_OS_PROBER=true
GRUB_PRELOAD_MODULES="part_gpt part_msdos"
GRUB
}

write_grub_script() {
  cat >"$TARGET/root/forsteri-boot.sh" <<'BOOT_SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=/dev/null
. /root/forsteri-boot.env

install -d /boot/grub/themes/forsteri
install -d /boot/grub

if [[ $TARGET_FIRMWARE == uefi ]]; then
  grub-install --target=x86_64-efi --efi-directory=/efi \
    --bootloader-id=Forsteri --recheck
else
  grub-install --target=i386-pc --recheck "$TARGET_DISK"
fi

grub-mkconfig -o /boot/grub/grub.cfg
rm -f /root/forsteri-boot.env
echo "forsteri: bootloader installed"
BOOT_SCRIPT
  chmod 700 "$TARGET/root/forsteri-boot.sh"
}

install_bootloader() {
  step "installing the bootloader (${CFG[firmware]})"

  install -d "$TARGET/boot/grub/themes/forsteri"
  cp -a "$FORSTERI_SHARE/grub/." "$TARGET/boot/grub/themes/forsteri/"
  write_grub_default

  {
    printf 'TARGET_FIRMWARE=%q\n' "${CFG[firmware]}"
    printf 'TARGET_DISK=%q\n' "${CFG[disk]}"
  } >"$TARGET/root/forsteri-boot.env"
  chmod 600 "$TARGET/root/forsteri-boot.env"

  write_grub_script
  run_logged "grub install" arch-chroot "$TARGET" /usr/bin/env bash /root/forsteri-boot.sh ||
    die "the bootloader could not be installed (see $FORSTERI_LOG)"
  rm -f "$TARGET/root/forsteri-boot.sh"
}
