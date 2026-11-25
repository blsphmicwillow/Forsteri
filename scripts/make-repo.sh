#!/usr/bin/env bash
# Build the `nest` codename: Forsteri's own package repository.
#
# Run on Arch (or the archlinux container). For each packages/*/PKGBUILD:
# build it, sign nothing, and add the result to out/repo/x86_64 with repo-add.
#
#   scripts/make-repo.sh [output-dir]
#
# Then point the nest codename at the result, for example:
#   sudo mkdir -p /srv/forsteri/repo
#   sudo cp -r out/repo/x86_64 /srv/forsteri/repo/
#
# which is exactly the file:///srv/forsteri/repo/x86_64 URL in repos.conf.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
OUT=${1:-"$ROOT/out/repo/x86_64"}

for tool in makepkg repo-add; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "make-repo: $tool is required (install base-devel / pacman)" >&2
    exit 1
  }
done

BUILD="$ROOT/out/nest-build"
rm -rf "$BUILD"
mkdir -p "$BUILD" "$OUT"

# forst's PKGBUILD builds from a source tarball; stage the crate and the
# repository config next to it.
cp "$ROOT/iso/airootfs/etc/forst/repos.conf" "$ROOT/packages/forst/repos.conf"
# The PKGBUILD expects $srcdir/forst-0.1.0, so rename the top directory.
tar -C "$ROOT/pkg" --exclude='forst/target' \
  --transform='s,^forst,forst-0.1.0,' \
  -czf "$ROOT/packages/forst/forst-0.1.0.tar.gz" forst

for dir in "$ROOT"/packages/*/; do
  name=$(basename "$dir")
  [[ -f $dir/PKGBUILD ]] || continue
  echo "==> building $name"
  (cd "$dir" && PKGDEST="$BUILD" makepkg --force --noconfirm --cleanbuild)
done

shopt -s nullglob
pkgs=("$BUILD"/*.pkg.tar.*)
(( ${#pkgs[@]} > 0 )) || {
  echo "make-repo: no packages were built" >&2
  exit 1
}

echo "==> adding ${#pkgs[@]} package(s) to $OUT"
repo-add -n -R "$OUT/nest.db.tar.gz" "${pkgs[@]}"

ls -lh "$OUT"
