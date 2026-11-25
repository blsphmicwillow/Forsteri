#!/usr/bin/env bash
# Build the Forsteri OS ISO.
#
#   scripts/build-iso.sh              build on this machine (needs archiso + root)
#   scripts/build-iso.sh --docker     build in the official Arch container
#
# The result lands in out/. archiso needs root because it mounts the live
# root when building the squashfs image.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
PROFILE="$ROOT/iso"
OUT="$ROOT/out"
WORK="${FORSTERI_WORK:-/tmp/forsteri-work}"
MODE=${1:-}
FOSTERIOS_DIR=${FOSTERIOS_DIR:-"$ROOT/../FosteriOS"}
if [[ ! -d $FOSTERIOS_DIR && -d $ROOT/FosteriOS ]]; then
  FOSTERIOS_DIR="$ROOT/FosteriOS"
fi

# shellcheck source=../iso/tool-versions.env
. "$PROFILE/tool-versions.env"
ARCHISO_VERSION=${ARCHISO_VERSION:-92-1}
MKINITCPIO_VERSION=${MKINITCPIO_VERSION:-42.2-1}

log() { printf '\n==> %s\n' "$*"; }

check_tool_version() {
  local pkg=$1 want=$2 got
  got=$(pacman -Q "$pkg" 2>/dev/null | awk '{print $2}')
  if [[ $got != "$want" ]]; then
    if [[ ${FORSTERI_ALLOW_TOOL_DRIFT:-0} == 1 ]]; then
      echo "build-iso: warning: $pkg $got differs from pinned $want" >&2
    else
      echo "build-iso: $pkg $got is installed, but this tree pins $want" >&2
      echo "build-iso: set FORSTERI_ALLOW_TOOL_DRIFT=1 to build anyway" >&2
      exit 1
    fi
  fi
}

log "building forst"
"$ROOT/scripts/build-forst.sh"

log "rendering brand assets"
if python3 -c 'import PIL' >/dev/null 2>&1; then
  python3 "$ROOT/branding/render_assets.py"
else
  echo "build-iso: Pillow is not available; using the committed branding/dist files" >&2
  [[ -f $ROOT/branding/dist/logo-512.png ]] || {
    echo "build-iso: branding/dist is empty and Pillow is missing; cannot continue" >&2
    exit 1
  }
fi

log "staging the live share"
"$ROOT/scripts/stage-share.sh"

mkdir -p "$OUT"

if [[ $MODE == "--docker" ]]; then
  command -v docker >/dev/null 2>&1 || {
    echo "build-iso: docker is not installed" >&2
    exit 1
  }
  log "building the ISO in archlinux:latest (this downloads ~1 GiB)"
  docker run --rm --privileged \
    -v "$ROOT:/build" \
    -v "$FOSTERIOS_DIR:/fosterios:ro" \
    -w /build \
    -e ARCHISO_VERSION="$ARCHISO_VERSION" -e MKINITCPIO_VERSION="$MKINITCPIO_VERSION" \
    archlinux:latest \
    bash -lc 'pacman -Syu --noconfirm --needed "archiso=$ARCHISO_VERSION" "mkinitcpio=$MKINITCPIO_VERSION" base-devel linux-headers rust &&
      FOSTERIOS_DIR=/fosterios /build/scripts/build-nt-module.sh &&
      mkarchiso -v -w /tmp/work -o /build/out /build/iso'
  log "done: $OUT"
  exit 0
fi

command -v mkarchiso >/dev/null 2>&1 || {
  cat >&2 <<'EOF'
build-iso: mkarchiso is not installed.

Build on Arch (pacman -S archiso) or use the container:

  scripts/build-iso.sh --docker
EOF
  exit 1
}

(( EUID == 0 )) || {
  echo "build-iso: mkarchiso needs root (it mounts the live root)" >&2
  exit 1
}

check_tool_version archiso "$ARCHISO_VERSION"
check_tool_version mkinitcpio "$MKINITCPIO_VERSION"

log "building the NT personality module for the live kernel"
FOSTERIOS_DIR="$FOSTERIOS_DIR" "$ROOT/scripts/build-nt-module.sh"

log "running mkarchiso"
mkarchiso -v -w "$WORK" -o "$OUT" "$PROFILE"

log "done"
ls -lh "$OUT"
