#!/usr/bin/env bash
# Unit tests for installer hardware discovery, using fake probes.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=../../iso/airootfs/usr/lib/forsteri/installer/hardware.sh
. "$HERE/../../iso/airootfs/usr/lib/forsteri/installer/hardware.sh"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat >"$tmp/lspci" <<'EOF'
#!/usr/bin/env bash
echo "01:00.0 VGA compatible controller [0300]: NVIDIA Corporation GA104 [GeForce RTX 3070] [10de:2484]"
EOF
cat >"$tmp/lsblk" <<'EOF'
#!/usr/bin/env bash
printf '/dev/nvme0n1p1 ntfs\n/dev/nvme0n1p2 ext4\n'
EOF
chmod +x "$tmp/lspci" "$tmp/lsblk"

fail=0
ok() { printf 'ok   %s\n' "$*"; }
bad() {
  printf 'FAIL %s\n' "$*"
  fail=1
}

LSPCI=$tmp/lspci LSBLK=$tmp/lsblk
[[ $(LSPCI=$tmp/lspci gpu_vendor) == nvidia ]] && ok "nvidia detection" || bad "nvidia detection"
[[ $(LSPCI=$tmp/lspci gpu_packages) == *nvidia-utils* ]] && ok "nvidia advice" || bad "nvidia advice"
[[ $(LSBLK=$tmp/lsblk ntfs_partitions) == /dev/nvme0n1p1 ]] && ok "NTFS partition detection" ||
  bad "NTFS partition detection"
[[ $(LSPCI=$tmp/lspci LSBLK=$tmp/lsblk detect_dual_boot) == yes ]] && ok "dual-boot detection" ||
  bad "dual-boot detection"

cat >"$tmp/lspci" <<'EOF'
#!/usr/bin/env bash
echo "00:02.0 VGA compatible controller [0300]: Intel Corporation UHD Graphics 620 [8086:5917]"
EOF
cat >"$tmp/lsblk" <<'EOF'
#!/usr/bin/env bash
printf '/dev/sda1 ext4\n'
EOF
chmod +x "$tmp/lspci" "$tmp/lsblk"
[[ $(LSPCI=$tmp/lspci gpu_vendor) == intel ]] && ok "intel detection" || bad "intel detection"
[[ $(LSPCI=$tmp/lspci LSBLK=$tmp/lsblk detect_dual_boot) == no ]] && ok "single-boot detection" ||
  bad "single-boot detection"

cat >"$tmp/lspci" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "$tmp/lspci"
[[ $(LSPCI=$tmp/lspci gpu_vendor) == unknown ]] && ok "probe failure is unknown" || bad "probe failure"

if (( fail == 0 )); then
  echo "hardware tests passed"
else
  echo "hardware tests failed"
fi
exit "$fail"
