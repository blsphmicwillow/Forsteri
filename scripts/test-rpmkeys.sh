#!/usr/bin/env bash
# Real rpmkeys gate: build an unsigned RPM with rpmbuild, then run the Rust
# test that checks the unsigned and tampered files against the actual binary.
# This is separate from the synthetic string fixtures in sign.rs.
#
#   scripts/test-rpmkeys.sh
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

for tool in rpmbuild rpmkeys; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "test-rpmkeys: $tool is required (install rpm)" >&2
    exit 1
  }
done

mkdir -p "$WORK/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
cat >"$WORK/rpmbuild/SPECS/forsteri-rpmkeys-fixture.spec" <<'SPEC'
Name:           forsteri-rpmkeys-fixture
Version:        1
Release:        1
Summary:        Forsteri rpmkeys regression fixture
License:        MIT
BuildArch:      noarch

%description
A tiny unsigned RPM used to test forst's rpmkeys verdict parser.

%install
mkdir -p %{buildroot}/usr/share/forsteri
printf 'rpmkeys fixture\n' > %{buildroot}/usr/share/forsteri/rpmkeys-fixture

%files
/usr/share/forsteri/rpmkeys-fixture
SPEC

rpmbuild --define "_topdir $WORK/rpmbuild" --define '_build_id_links none' \
  -bb "$WORK/rpmbuild/SPECS/forsteri-rpmkeys-fixture.spec" >/dev/null

fixture=$(find "$WORK/rpmbuild/RPMS" -type f -name '*.rpm' | head -1)
[[ -n $fixture ]] || { echo "test-rpmkeys: rpmbuild produced no RPM" >&2; exit 1; }

echo "test-rpmkeys: built unsigned $fixture"
(
  cd "$ROOT/pkg/forst"
  FORST_RPMKEYS_FIXTURE="$fixture" cargo test --quiet \
    real_rpmkeys_rejects_unsigned_and_tampered -- --nocapture
)
