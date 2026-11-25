#!/usr/bin/env bash
# Everything that can be checked without building an ISO or touching a disk.
#
#   scripts/check.sh              the offline checks
#   scripts/check.sh --network    also install real packages into temp roots
#                                 and verify the ledger (scripts/check-forst.sh)
#
# 1. shell syntax for every script in the tree
# 2. Python syntax for the asset renderer
# 3. forst unit tests
# 4. the installer's own --check (profiles and package lists)
# 5. presence of every asset the boot and desktop paths reference
# 6. --network only: real Arch and Debian payloads plus ledger consistency
set -uo pipefail

NETWORK=0
for arg in "$@"; do
  case $arg in
    --network) NETWORK=1 ;;
    -h | --help)
      sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "check.sh: unknown argument: $arg (try --network)" >&2
      exit 2
      ;;
  esac
done

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT" || exit 1

fail=0
note() { printf '%s\n' "$*"; }
bad() {
  printf 'FAIL %s\n' "$*"
  fail=1
}
ok() { printf 'ok   %s\n' "$*"; }

note "== shell syntax"
while IFS= read -r script; do
  if bash -n "$script" 2>/tmp/forsteri-bashn; then
    ok "$script"
  else
    bad "$script: $(cat /tmp/forsteri-bashn)"
  fi
done < <(find . -type f \( -name '*.sh' -o -path './iso/airootfs/usr/local/bin/*' \) \
  -not -path './pkg/forst/target/*' -not -path './out/*' \
  -not -path './iso/airootfs/usr/local/bin/forst' | sort)

note "== python"
if python3 -m py_compile branding/render_assets.py 2>/tmp/forsteri-py; then
  ok "branding/render_assets.py"
else
  bad "branding/render_assets.py: $(cat /tmp/forsteri-py)"
fi

note "== forst"
if command -v cargo >/dev/null 2>&1; then
  if (cd pkg/forst && cargo test --quiet); then
    ok "cargo test"
  else
    bad "cargo test"
  fi
else
  note "--   cargo not installed; skipping forst tests"
fi

note "== installer data"
STAGE=$(mktemp -d)
if ./scripts/stage-share.sh "$STAGE/share" >/dev/null 2>&1; then
  if FORSTERI_SHARE="$STAGE/share" FORSTERI_LOG="$STAGE/install.log" \
    bash iso/airootfs/usr/local/bin/forsteri-install --check; then
    ok "forsteri-install --check"
  else
    bad "forsteri-install --check"
  fi
  if FORSTERI_SHARE="$STAGE/share" FORSTERI_LOG="$STAGE/install.log" \
    bash iso/airootfs/usr/local/bin/forsteri-install --validate-only >/dev/null; then
    ok "forsteri-install --validate-only"
  else
    bad "forsteri-install --validate-only"
  fi
else
  bad "scripts/stage-share.sh"
fi
rm -rf "$STAGE"

note "== installer unit tests"
for test in installer/tests/*_test.sh; do
  [[ -f $test ]] || continue
  if bash "$test"; then
    ok "$test"
  else
    bad "$test"
  fi
done

if (( NETWORK )); then
  note "== forst install and ledger (network)"
  if ./scripts/check-forst.sh; then
    ok "scripts/check-forst.sh"
  else
    bad "scripts/check-forst.sh"
  fi
else
  note "== forst install and ledger"
  note "--   skipped; run scripts/check.sh --network to install real packages"
fi

note "== assets"
for asset in logo-512.png plymouth-logo.png plymouth-track.png plymouth-comet.png \
  plymouth-bullet.png grub-background.png wallpaper-aurora.png; do
  if [[ -s branding/dist/$asset ]]; then
    ok "branding/dist/$asset"
  else
    bad "branding/dist/$asset is missing (run python3 branding/render_assets.py)"
  fi
done

if (( fail == 0 )); then
  note ""
  note "all checks passed"
else
  note ""
  note "some checks failed"
fi
exit "$fail"
