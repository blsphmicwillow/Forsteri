#!/usr/bin/env bash
# Refresh the OpenPGP trust anchors for the Debian and Fedora asset codenames.
#
# These are public archive keys, not secrets. The fingerprints are pinned here
# so a mirror compromise cannot silently replace a key with one that signs
# malicious metadata. Arch's keyring comes from pacman-key and is copied
# separately (pkg/forst/keys/arch.gpg).
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
KEYS="$ROOT/pkg/forst/keys"
DEBIAN_URL=${DEBIAN_URL:-https://ftp-master.debian.org/keys/archive-key-12.asc}
DEBIAN_FP=${DEBIAN_FP:-B8B80B5B623EAB6AD8775C45B7C5D7D6350947F8}
FEDORA_URL=${FEDORA_URL:-https://src.fedoraproject.org/rpms/fedora-repos/raw/rawhide/f/RPM-GPG-KEY-fedora-43-primary}
FEDORA_FP=${FEDORA_FP:-C6E7F081CF80E13146676E88829B606631645531}

command -v curl >/dev/null 2>&1 || {
  echo "fetch-keyrings: curl is required" >&2
  exit 1
}
command -v gpg >/dev/null 2>&1 || {
  echo "fetch-keyrings: gpg is required" >&2
  exit 1
}

fetch() {
  local name=$1 url=$2 expected=$3 tmp actual
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' RETURN
  echo "fetch-keyrings: $name <- $url"
  curl -fsSL --retry 3 --connect-timeout 20 "$url" | gpg --dearmor >"$tmp"
  actual=$(gpg --no-default-keyring --keyring "$tmp" --list-keys --with-colons 2>/dev/null |
    awk -F: '/^fpr:/ { print $10 }')
  grep -qx "$expected" <<<"$actual" || {
    echo "fetch-keyrings: $name fingerprint mismatch" >&2
    echo "  expected $expected" >&2
    echo "  got      $(tr '\n' ' ' <<<"$actual")" >&2
    exit 1
  }
  install -Dm644 "$tmp" "$KEYS/$name.gpg"
  rm -f "$tmp"
  trap - RETURN
}

mkdir -p "$KEYS"
fetch debian "$DEBIAN_URL" "$DEBIAN_FP"
fetch fedora "$FEDORA_URL" "$FEDORA_FP"
echo "fetch-keyrings: wrote $KEYS/{debian,fedora}.gpg"
echo "fetch-keyrings: refresh pkg/forst/keys/arch.gpg from pacman-key when Arch keys rotate"
