#!/usr/bin/env bash
# Forsteri installer: the desktop and theme selectors, and their setup.

pick_desktop() {
  local choice
  choice=$(ui_menu "Desktop" \
    "Pick how Forsteri should look. Every choice is a starting point: all of
these are ordinary packages you can add to or remove later with forst." \
    gnome "GNOME  -- overview, top bar, macOS-ish spacing" \
    kde "KDE Plasma 6  -- panel, widgets, closest to a Mac layout" \
    hyprland "Hyprland + Waybar  -- tiling, animated, keyboard-first" \
    mangowc "MangoWC + Quickshell  -- dwl-style tags, Forsteri's own shell" \
    minimal "Minimal  -- no graphical session, just a TTY") || return 1
  CFG[desktop]=$choice
  log "desktop: $choice"
}

# Themes are data: one directory per codename under $FORSTERI_SHARE/themes.
pick_theme() {
  local conf name summary items=()
  for conf in "$FORSTERI_SHARE"/themes/*/theme.conf; do
    [[ -f $conf ]] || continue
    name=$(sed -n 's/^NAME=//p' "$conf")
    summary=$(sed -n 's/^SUMMARY=//p' "$conf")
    items+=("$(basename "$(dirname "$conf")")" "${name:-?}  -- ${summary:-}")
  done
  (( ${#items[@]} > 0 )) || {
    log "no themes staged; keeping ${CFG[theme]}"
    return 0
  }

  local choice
  choice=$(ui_menu "Theme" \
    "Choose a look. It sets colours, corner radii, fonts and button placement
for GTK, KDE, GNOME and Hyprland; change it later with
  forsteri-theme-apply <codename>" "${items[@]}") || return 1
  CFG[theme]=$choice
  log "theme: $choice"
}

install_desktop() {
  local -a specs=()
  mapfile -t specs < <(read_spec_list "$FORSTERI_SHARE/installer/profiles/${CFG[desktop]}.list")
  (( ${#specs[@]} > 0 )) || die "${CFG[desktop]} has no packages"

  ui_info "Installing ${CFG[desktop]}" \
    "Installing ${#specs[@]} package specs.

The download can be large; the log is at $FORSTERI_LOG."
  run_logged "desktop ${CFG[desktop]}" \
    forst --root "$TARGET" --cache "$TARGET/var/cache/forst" \
      --conf /etc/forst/repos.conf --yes install "${specs[@]}" ||
    die "the ${CFG[desktop]} desktop could not be installed (see $FORSTERI_LOG)"
}

copy_skel() {
  local skel="$FORSTERI_SHARE/skel"
  step "installing desktop configuration"

  install -d "$TARGET/etc/skel"
  if [[ -d $skel/common ]]; then
    cp -a "$skel/common/." "$TARGET/etc/skel/"
  fi
  if [[ -d $skel/${CFG[desktop]} ]]; then
    cp -a "$skel/${CFG[desktop]}/." "$TARGET/etc/skel/"
  fi

  local home="$TARGET/home/${CFG[user]}"
  if [[ -d $home ]]; then
    cp -an "$TARGET/etc/skel/." "$home/" 2>/dev/null || true
    arch-chroot "$TARGET" chown -R "${CFG[user]}:${CFG[user]}" "/home/${CFG[user]}" >>"$FORSTERI_LOG" 2>&1 || true
  fi
}

configure_desktop() {
  copy_skel

  if [[ ${CFG[desktop]} != "minimal" ]]; then
    install -d "$TARGET/etc/sddm.conf.d"
    cat >"$TARGET/etc/sddm.conf.d/10-forsteri.conf" <<SDDM
[Theme]
Current=forsteri

[General]
Numlock=on
HaltCommand=/usr/bin/systemctl poweroff
RebootCommand=/usr/bin/systemctl reboot
SDDM
    run_logged "enable sddm" arch-chroot "$TARGET" systemctl enable sddm.service ||
      die "could not enable the display manager"
  fi

  # Bluetooth only when the desktop brought a stack for it.
  if [[ -x "$TARGET/usr/bin/bluetoothctl" ]]; then
    run_logged "enable bluetooth" arch-chroot "$TARGET" systemctl enable bluetooth.service || true
  fi
}
