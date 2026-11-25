#!/usr/bin/env bash
# Copy everything the installed system needs into the live profile tree:
#
#   iso/airootfs/usr/share/forsteri/...
#
# Idempotent: safe to run on every build. Also used by scripts/check.sh, which
# passes its own destination to lint the installer without touching the profile.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
SHARE=${1:-"$ROOT/iso/airootfs/usr/share/forsteri"}
DIST="$ROOT/branding/dist"

log() { printf 'stage-share: %s\n' "$*"; }

required() {
  [[ -f $1 ]] || {
    echo "stage-share: missing $1" >&2
    echo "stage-share: run 'python3 branding/render_assets.py' first" >&2
    exit 1
  }
}

for asset in logo-512.png plymouth-logo.png plymouth-bullet.png plymouth-track.png \
  plymouth-comet.png grub-background.png sddm-background.png wallpaper-aurora.png; do
  required "$DIST/$asset"
done

rm -rf "$SHARE"
install -d "$SHARE"/{installer/profiles,plymouth,grub,wallpapers,sddm,skel,sessions,theme,themes,keyrings}

# installer data
install -m644 "$ROOT/installer/base.list" "$SHARE/installer/base.list"
for profile in "$ROOT"/desktop/profiles/*.list; do
  install -m644 "$profile" "$SHARE/installer/profiles/"
done

# splash
install -m644 "$ROOT/branding/plymouth/forsteri/forsteri.plymouth" "$SHARE/plymouth/"
install -m644 "$ROOT/branding/plymouth/forsteri/forsteri.script" "$SHARE/plymouth/"
for asset in plymouth-logo.png plymouth-bullet.png plymouth-track.png plymouth-comet.png; do
  install -m644 "$DIST/$asset" "$SHARE/plymouth/$asset"
done

# boot menu
install -m644 "$ROOT/branding/grub/forsteri/theme.txt" "$SHARE/grub/theme.txt"
install -m644 "$DIST/grub-background.png" "$SHARE/grub/background.png"
install -m644 "$DIST/logo-512.png" "$SHARE/grub/logo.png"

# desktop artwork
install -m644 "$DIST/wallpaper-aurora.png" "$SHARE/wallpapers/wallpaper-aurora.png"
install -m644 "$DIST/logo-512.png" "$SHARE/wallpapers/forsteri-logo.png"

# login screen
install -m644 "$ROOT/desktop/sddm/forsteri/theme.conf" "$SHARE/sddm/theme.conf"
install -m644 "$ROOT/desktop/sddm/forsteri/Main.qml" "$SHARE/sddm/Main.qml"
install -m644 "$DIST/sddm-background.png" "$SHARE/sddm/background.png"
install -m644 "$DIST/logo-512.png" "$SHARE/sddm/logo.png"

# sessions
install -m644 "$ROOT/desktop/session/mango-forsteri.desktop" "$SHARE/sessions/mango-forsteri.desktop"

# skeleton homes
cp -a "$ROOT/desktop/skel/." "$SHARE/skel/"

# theme codenames and the one script that renders them
install -m755 "$ROOT/desktop/theme/apply.sh" "$SHARE/theme/apply.sh"
for theme in "$ROOT"/desktop/themes/*/; do
  name=$(basename "$theme")
  install -d "$SHARE/themes/$name"
  install -m644 "$theme/theme.conf" "$SHARE/themes/$name/theme.conf"
done

# OpenPGP trust anchors for the Debian and Fedora asset codenames. forst
# refuses unsigned metadata, so these are part of the live medium, not an
# optional extra.
for key in "$ROOT"/pkg/forst/keys/*.gpg; do
  [[ -f $key ]] || {
    echo "stage-share: no forst keyrings found (run scripts/fetch-keyrings.sh)" >&2
    exit 1
  }
  install -m644 "$key" "$SHARE/keyrings/"
done

log "share staged at $SHARE"
