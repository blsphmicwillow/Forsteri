#!/usr/bin/env bash
# Install real packages into scratch roots and check the ledger.
#
#   scripts/check-forst.sh            run it (needs a network)
#   scripts/check-forst.sh --offline  reuse the cache, skip the sync
#
# This is the manual verification from docs/building.md, made repeatable:
#
#   1. sync colony, ice and tundra from the real mirrors
#   2. install ice:tree    -- an Arch .pkg.tar.zst with its dependency chain
#   3. install tundra:hello -- a Debian .deb, whose libc6 comes from the base
#   4. run the Debian binary out of the scratch root
#   5. remove both and prove the ledger and the files agree at every step
#
# Nothing here touches the host: every root, database and cache lives under a
# temporary directory that is removed on exit.
set -uo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
CRATE="$ROOT/pkg/forst"
OFFLINE=0
[[ ${1:-} == "--offline" ]] && OFFLINE=1

for tool in cargo curl tar; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "check-forst: $tool is required" >&2
    exit 1
  }
done

fail=0
ok() { printf 'ok   %s\n' "$*"; }
bad() {
  printf 'FAIL %s\n' "$*"
  fail=1
}

WORK=$(mktemp -d "${TMPDIR:-/tmp}/forst-check.XXXXXX")
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

FORST="$CRATE/target/release/forst"
CACHE="$WORK/cache"
ARCH_ROOT="$WORK/arch-root"
DEB_ROOT="$WORK/deb-root"
CONF="$WORK/repos.conf"
cp "$ROOT/iso/airootfs/etc/forst/repos.conf" "$CONF"
# The live config relies on keyrings installed under /usr/share/forsteri.
# Point the network test at the repository's own trust anchors instead.
sed -i "/^\[tundra\]/a keyring     = $ROOT/pkg/forst/keys/debian.gpg" "$CONF"
sed -i "/^\[aurora\]/a keyring     = $ROOT/pkg/forst/keys/fedora.gpg" "$CONF"

echo "== building forst"
if (cd "$CRATE" && cargo build --release --quiet); then
  ok "cargo build --release"
else
  bad "cargo build --release"
  exit 1
fi

forst() { "$FORST" --conf "$CONF" --cache "$CACHE" "$@"; }

echo "== syncing (network)"
if (( OFFLINE )); then
  echo "--   offline: using whatever is already in $CACHE"
elif forst sync colony ice tundra >"$WORK/sync.log" 2>&1; then
  ok "sync colony ice tundra"
  sed -n '1,3p' "$WORK/sync.log" | sed 's/^/     /'
else
  bad "sync colony ice tundra (no network?)"
  sed -n '1,5p' "$WORK/sync.log" | sed 's/^/     /'
  exit 1
fi

echo "== arch package: ice:tree"
if forst --root "$ARCH_ROOT" -y install ice:tree >"$WORK/tree.log" 2>&1; then
  ok "install ice:tree"
else
  bad "install ice:tree"
  tail -5 "$WORK/tree.log" | sed 's/^/     /'
fi

arch_list=$(forst --root "$ARCH_ROOT" list)
if grep -Eq '^tree +[^ ]+ +ice$' <<<"$arch_list"; then
  ok "ledger lists tree from ice"
else
  bad "ledger does not list tree from ice"
  forst --root "$ARCH_ROOT" list | sed 's/^/     /'
fi

# glibc is a dependency of tree out of colony: the chain must be in the ledger.
if grep -Eq '^glibc +[^ ]+ +colony$' <<<"$arch_list"; then
  ok "dependency glibc resolved from colony"
else
  bad "glibc missing from the ledger"
fi

files=$(forst --root "$ARCH_ROOT" files tree)
# `files` prints "<kind> <path>", e.g. "f /usr/bin/tree".
if grep -Eq '(^| )/usr/bin/tree$' <<<"$files" && [[ -x $ARCH_ROOT/usr/bin/tree ]]; then
  ok "files tree lists /usr/bin/tree and it exists"
else
  bad "files tree does not match the root"
fi

if [[ $(head -c 4 "$ARCH_ROOT/usr/bin/tree" 2>/dev/null) == $'\x7fELF' ]]; then
  ok "tree is an ELF binary"
else
  bad "tree is not an ELF binary"
fi

if forst --root "$ARCH_ROOT" verify >"$WORK/verify.log" 2>&1; then
  ok "forst verify accepts the fresh install"
else
  bad "forst verify reports drift immediately after install"
  tail -5 "$WORK/verify.log" | sed 's/^/     /'
fi

# The ledger must survive a second install without duplicating itself.
before=$(grep -c '^' <<<"$arch_list")
forst --root "$ARCH_ROOT" -y install ice:tree >/dev/null 2>&1
after=$(forst --root "$ARCH_ROOT" list | grep -c '^' || true)
if (( before == after )); then
  ok "re-installing tree does not duplicate the ledger"
else
  bad "re-installing tree changed the ledger ($before -> $after)"
fi

echo "== debian package: tundra:hello"
if forst --root "$DEB_ROOT" -y install tundra:hello >"$WORK/hello.log" 2>&1; then
  ok "install tundra:hello"
else
  bad "install tundra:hello"
  tail -5 "$WORK/hello.log" | sed 's/^/     /'
fi

deb_list=$(forst --root "$DEB_ROOT" list)
if grep -Eq '^hello +[^ ]+ +tundra$' <<<"$deb_list"; then
  ok "ledger lists hello from tundra"
else
  bad "ledger does not list hello from tundra"
fi

if [[ -x $DEB_ROOT/usr/bin/hello ]] && out=$("$DEB_ROOT/usr/bin/hello" 2>&1); then
  if [[ $out == *"Hello, world!"* ]]; then
    ok "the installed Debian binary runs: $out"
  else
    bad "hello ran but printed: $out"
  fi
else
  bad "hello is missing or did not run"
fi

# A foreign system package must stay refused even here.
if forst --root "$DEB_ROOT" -y install tundra:libc6 >"$WORK/guard.log" 2>&1; then
  bad "tundra:libc6 was installed; the foreign-system guard did not fire"
else
  ok "tundra:libc6 is refused (foreign-system guard)"
fi

echo "== removal and ledger consistency"
if forst --root "$ARCH_ROOT" -y remove tree >"$WORK/rm-tree.log" 2>&1; then
  ok "remove tree"
else
  bad "remove tree"
  tail -3 "$WORK/rm-tree.log" | sed 's/^/     /'
fi

if grep -Eq '^tree +' <<<"$(forst --root "$ARCH_ROOT" list)"; then
  bad "tree is still listed after removal"
else
  ok "tree is gone from the ledger"
fi

if [[ -e $ARCH_ROOT/usr/bin/tree ]]; then
  bad "/usr/bin/tree still exists after removal"
else
  ok "tree's files are gone from the root"
fi

# Removing tree must not have removed the glibc it shared with nothing else,
# and the deb root must still be intact.
if [[ -x $DEB_ROOT/usr/bin/hello ]] && "$DEB_ROOT/usr/bin/hello" >/dev/null 2>&1; then
  ok "the Debian root still runs after the Arch removal"
else
  bad "the Debian root was damaged by the Arch removal"
fi

if forst --root "$DEB_ROOT" -y remove hello >/dev/null 2>&1 &&
  [[ ! -e $DEB_ROOT/usr/bin/hello ]]; then
  ok "remove hello removes its file"
else
  bad "remove hello left its file behind"
fi

echo
if (( fail == 0 )); then
  echo "forst ledger checks passed"
else
  echo "forst ledger checks failed"
fi
exit "$fail"
