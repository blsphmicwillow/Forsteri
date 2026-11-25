#!/usr/bin/env bash
# Forsteri installer: base system, branding, fstab and first-boot configuration.

install_base() {
  local -a specs=()
  mapfile -t specs < <(read_spec_list "$FORSTERI_SHARE/installer/base.list")
  (( ${#specs[@]} > 0 )) || die "the base package list is empty"

  ui_info "Installing the base system" \
    "Resolving and installing ${#specs[@]} package specs into $TARGET.

forst is doing the work; this takes a few minutes."
  # The live root is a small overlay; a base system does not fit in its
  # cache. Keep downloaded packages on the target, which has the real disk.
  run_logged "base install" \
    forst --root "$TARGET" --cache "$TARGET/var/cache/forst" \
      --conf /etc/forst/repos.conf --strict --yes install "${specs[@]}" ||
    die "the base system could not be installed (see $FORSTERI_LOG)"
}

copy_target_tools() {
  step "copying forst and system identity into the target"
  install -Dm755 /usr/local/bin/forst "$TARGET/usr/local/bin/forst"
  install -Dm644 /etc/forst/repos.conf "$TARGET/etc/forst/repos.conf"
  install -Dm644 /etc/os-release "$TARGET/etc/os-release"
  install -Dm644 /etc/issue "$TARGET/etc/issue"
}

install_branding() {
  step "installing branding and themes"
  install -d "$TARGET/usr/share/plymouth/themes" "$TARGET/usr/share/backgrounds/forsteri"
  cp -r "$FORSTERI_SHARE/plymouth" "$TARGET/usr/share/plymouth/themes/forsteri"
  cp -r "$FORSTERI_SHARE/wallpapers/." "$TARGET/usr/share/backgrounds/forsteri/"
  install -d "$TARGET/usr/share/sddm/themes"
  cp -r "$FORSTERI_SHARE/sddm" "$TARGET/usr/share/sddm/themes/forsteri"
  install -Dm644 /etc/plymouth/plymouthd.conf "$TARGET/etc/plymouth/plymouthd.conf"

  # Wayland session entry for MangoWC + Quickshell.
  install -Dm644 "$FORSTERI_SHARE/sessions/mango-forsteri.desktop" \
    "$TARGET/usr/share/wayland-sessions/mango-forsteri.desktop"

  # Signature trust anchors must exist on the installed system too; forst
  # fails closed on unsigned Debian/Fedora metadata without them.
  install -d "$TARGET/usr/share/forsteri/keyrings"
  cp -a "$FORSTERI_SHARE/keyrings/." "$TARGET/usr/share/forsteri/keyrings/"

  # Theme codenames and the single helper that renders one into a session.
  cp -r "$FORSTERI_SHARE/themes" "$TARGET/usr/share/forsteri/themes"
  install -Dm755 "$FORSTERI_SHARE/theme/apply.sh" "$TARGET/usr/local/bin/forsteri-theme-apply"
  install -d "$TARGET/etc/forsteri"
  printf '%s\n' "${CFG[theme]}" >"$TARGET/etc/forsteri/theme"
}

write_fstab() {
  step "writing fstab"
  genfstab -U "$TARGET" >"$TARGET/etc/fstab" || die "genfstab failed"
}

# Values the chroot script needs. Written into the target with mode 600 and
# removed by the chroot script itself.
write_chroot_env() {
  local env_file="$TARGET/root/forsteri-install.env"
  {
    printf 'TARGET_HOSTNAME=%q\n' "${CFG[hostname]}"
    printf 'TARGET_TIMEZONE=%q\n' "${CFG[timezone]}"
    printf 'TARGET_LOCALE=%q\n' "${CFG[locale]}"
    printf 'TARGET_KEYMAP=%q\n' "${CFG[keymap]}"
    printf 'TARGET_USER=%q\n' "${CFG[user]}"
    printf 'TARGET_USER_PASSWORD=%q\n' "$USER_PASSWORD"
    printf 'TARGET_ROOT_PASSWORD=%q\n' "$ROOT_PASSWORD"
    printf 'TARGET_DESKTOP=%q\n' "${CFG[desktop]}"
  } >"$env_file"
  chmod 600 "$env_file"
}

write_chroot_script() {
  cat >"$TARGET/root/forsteri-setup.sh" <<'CHROOT_SCRIPT'
#!/usr/bin/env bash
# Runs inside the new system, once.
set -euo pipefail
# shellcheck source=/dev/null
. /root/forsteri-install.env

echo "forsteri: timezone $TARGET_TIMEZONE"
ln -sf "/usr/share/zoneinfo/$TARGET_TIMEZONE" /etc/localtime
hwclock --systohc

echo "forsteri: locale $TARGET_LOCALE"
sed -i "s/^#\(${TARGET_LOCALE}\)/\1/" /etc/locale.gen
locale-gen
printf 'LANG=%s\n' "$TARGET_LOCALE" >/etc/locale.conf
printf 'KEYMAP=%s\n' "$TARGET_KEYMAP" >/etc/vconsole.conf

printf '%s\n' "$TARGET_HOSTNAME" >/etc/hostname
cat >/etc/hosts <<HOSTS
127.0.0.1   localhost
::1         localhost
127.0.1.1   $TARGET_HOSTNAME.localdomain $TARGET_HOSTNAME
HOSTS

echo "forsteri: accounts"
printf 'root:%s\n' "$TARGET_ROOT_PASSWORD" | chpasswd
if ! id -u "$TARGET_USER" >/dev/null 2>&1; then
  useradd -m -G wheel,video,audio,input,storage,optical -s /bin/bash "$TARGET_USER"
fi
printf '%s:%s\n' "$TARGET_USER" "$TARGET_USER_PASSWORD" | chpasswd
install -d -m 750 /etc/sudoers.d
printf '%%wheel ALL=(ALL:ALL) ALL\n' >/etc/sudoers.d/10-wheel
chmod 440 /etc/sudoers.d/10-wheel

echo "forsteri: services"
systemd-machine-id-setup
systemd-sysusers
systemctl preset-all >/dev/null
systemctl enable NetworkManager.service
systemctl enable systemd-timesyncd.service
systemctl enable fstrim.timer

echo "forsteri: initramfs (with plymouth)"
if ! grep -qE '^HOOKS=\([^)]*\bplymouth\b' /etc/mkinitcpio.conf; then
  sed -i -E 's/^HOOKS=\((base)( udev)? /HOOKS=(\1\2 plymouth /' /etc/mkinitcpio.conf
fi
mkinitcpio -P

rm -f /root/forsteri-install.env
echo "forsteri: chroot setup complete"
CHROOT_SCRIPT
  chmod 700 "$TARGET/root/forsteri-setup.sh"
}

chroot_setup() {
  step "configuring the new system"
  write_chroot_env
  write_chroot_script
  run_logged "chroot setup" arch-chroot "$TARGET" /usr/bin/env bash /root/forsteri-setup.sh ||
    die "configuring the new system failed (see $FORSTERI_LOG)"
  rm -f "$TARGET/root/forsteri-setup.sh"
}
