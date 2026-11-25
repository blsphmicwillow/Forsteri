#!/usr/bin/env bash
# Real SIGKILL fault test for the transactional installer.
#
# Builds a debug forst, a tiny signed pacman package and a local repository.
# It then pauses the installer at deterministic test-only points, kills it with
# SIGKILL, and checks the on-disk state before rerunning the install:
#
#   staged       extraction and hashing finished, nothing committed
#   mid-commit   one file has been renamed into the target, no ledger
#   committed    every file has been renamed, no ledger yet
#
# The pause hook is compiled only into debug builds, so release forst has no
# environment-controlled stop.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TARGET=${CARGO_TARGET_DIR:-"$ROOT/pkg/forst/target"}
FORST="$TARGET/debug/forst"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

fail() { echo "fault-test: FAIL: $*" >&2; exit 1; }
ok() { echo "fault-test: ok: $*"; }

command -v gpg >/dev/null || fail "gpg is required"
command -v gpgv >/dev/null || fail "gpgv is required"
command -v curl >/dev/null || fail "curl is required"
command -v tar >/dev/null || fail "tar is required"

echo "fault-test: building debug forst"
(cd "$ROOT/pkg/forst" && cargo build --quiet)
[[ -x $FORST ]] || fail "missing debug binary: $FORST"
if "$FORST" --version >/dev/null; then :; else fail "debug forst cannot run"; fi

REPO="$WORK/repo"
GPG_HOME="$WORK/gnupg"
CACHE="$WORK/cache"
PKGROOT="$WORK/pkgroot"
mkdir -p "$REPO" "$GPG_HOME" "$CACHE/index" "$PKGROOT/usr/bin"
chmod 700 "$GPG_HOME"
printf 'fault one\n' >"$PKGROOT/usr/bin/fault-one"
printf 'fault two\n' >"$PKGROOT/usr/bin/fault-two"

PKG="$REPO/fault-test.pkg.tar.gz"
tar -C "$PKGROOT" -czf "$PKG" usr
SHA=$(sha256sum "$PKG" | awk '{print $1}')
SIZE=$(stat -c '%s' "$PKG")

gpg --homedir "$GPG_HOME" --batch --yes --pinentry-mode loopback --passphrase '' \
  --quick-gen-key "Forst Fault Test <fault@forst.invalid>" rsa2048 sign 0 >/dev/null 2>&1
gpg --homedir "$GPG_HOME" --batch --yes --export >"$REPO/keyring.gpg"
gpg --homedir "$GPG_HOME" --batch --yes --pinentry-mode loopback --passphrase '' \
  --detach-sign --output "$PKG.sig" "$PKG" >/dev/null 2>&1
gpgv --keyring "$REPO/keyring.gpg" "$PKG.sig" "$PKG" >/dev/null 2>&1 ||
  fail "test package signature did not verify"

printf '#forst2\ttest\t1\nfault-test\t1.0\tx86_64\t%s\t%s\t%s\t\t\t\tfault test package\n' \
  "$(basename "$PKG")" "$SIZE" "$SHA" >"$CACHE/index/test.idx"
cat >"$WORK/repos.conf" <<EOF
[test]
format      = pacman
url         = file://$REPO
db          = test
arch        = x86_64
enabled     = yes
keyring     = $REPO/keyring.gpg
description = local fault-test repository
EOF

run_forst() {
  local root=$1 db=$2
  shift 2
  "$FORST" --root "$root" --db "$db" --cache "$CACHE" \
    --conf "$WORK/repos.conf" --yes install test:fault-test "$@"
}

start_paused() {
  local point=$1 root=$2 db=$3 log=$4
  FORST_FAULT_PAUSE="$point" "$FORST" --root "$root" --db "$db" \
    --cache "$CACHE" --conf "$WORK/repos.conf" --yes \
    install test:fault-test >"$log" 2>&1 &
  local pid=$!
  local i
  for i in $(seq 1 200); do
    grep -q "fault-test pause at $point" "$log" 2>/dev/null && {
      printf '%s\n' "$pid"
      return 0
    }
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.05
  done
  kill -9 "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  fail "$point pause was not reached (see $log)"
}

kill_and_assert() {
  local pid=$1 root=$2 db=$3
  kill -9 "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  # A SIGKILL must not leave a committed ledger.
  [[ ! -e "$db/db/fault-test.meta" ]] || fail "killed install left a committed ledger"
  [[ ! -e "$db/db/fault-test.files" ]] || fail "killed install left a files ledger"
  # The target may contain a partial commit, but it must not be recorded as
  # installed. The next install must be able to finish cleanly.
  ok "no ledger after SIGKILL"
}

verify() {
  local root=$1 db=$2 out
  out=$("$FORST" --root "$root" --db "$db" verify) || {
    printf '%s\n' "$out"
    fail "post-recovery verify failed"
  }
  grep -q 'no drift' <<<"$out" || fail "post-recovery verify reported no-drift mismatch"
  ok "post-recovery verify has no drift"
}

# 1. killed after extraction/hashing, before the first live-root rename
case1="$WORK/case1"; mkdir -p "$case1/root"
pid=$(start_paused staged "$case1/root" "$case1/db" "$case1/log")
kill_and_assert "$pid" "$case1/root" "$case1/db"
[[ ! -e "$case1/root/usr/bin/fault-one" ]] || fail "staged install wrote before commit"
rm -rf "$case1/root/.forst-stage"
run_forst "$case1/root" "$case1/db" >/dev/null
[[ -f "$case1/db/db/fault-test.meta" ]] || fail "clean rerun did not commit the ledger"
[[ -f "$case1/root/usr/bin/fault-one" && -f "$case1/root/usr/bin/fault-two" ]] ||
  fail "clean rerun did not install both files"
verify "$case1/root" "$case1/db"

# 2. killed after the first file rename, before the rest of the commit
case2="$WORK/case2"; mkdir -p "$case2/root"
pid=$(start_paused mid-commit "$case2/root" "$case2/db" "$case2/log")
kill_and_assert "$pid" "$case2/root" "$case2/db"
count=$(find "$case2/root/usr/bin" -type f 2>/dev/null | wc -l)
(( count == 1 )) || fail "mid-commit expected exactly one live file, got $count"
rm -rf "$case2/root/.forst-stage"
run_forst "$case2/root" "$case2/db" >/dev/null
[[ -f "$case2/db/db/fault-test.meta" ]] || fail "mid-commit rerun did not commit the ledger"
[[ -f "$case2/root/usr/bin/fault-one" && -f "$case2/root/usr/bin/fault-two" ]] ||
  fail "mid-commit rerun did not install both files"
verify "$case2/root" "$case2/db"

# 3. killed after all file renames, just before the ledger commit
case3="$WORK/case3"; mkdir -p "$case3/root"
pid=$(start_paused committed "$case3/root" "$case3/db" "$case3/log")
kill_and_assert "$pid" "$case3/root" "$case3/db"
[[ -f "$case3/root/usr/bin/fault-one" && -f "$case3/root/usr/bin/fault-two" ]] ||
  fail "committed stage did not reach the target before SIGKILL"
rm -rf "$case3/root/.forst-stage"
run_forst "$case3/root" "$case3/db" >/dev/null
[[ -f "$case3/db/db/fault-test.meta" ]] || fail "committed rerun did not commit the ledger"
verify "$case3/root" "$case3/db"

echo "fault-test: PASS"
