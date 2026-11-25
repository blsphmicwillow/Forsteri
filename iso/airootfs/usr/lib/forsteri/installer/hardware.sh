#!/usr/bin/env bash
# Forsteri installer: read-only hardware discovery.
#
# Kept separate from the TUI so the decisions are small, testable functions.
# Every probe degrades to "unknown" and never fails the installer.

# Vendor of the primary display controller: nvidia, amd, intel or unknown.
gpu_vendor() {
  local lspci_bin=${LSPCI:-lspci} out
  out=$("$lspci_bin" -nn 2>/dev/null | grep -Ei 'VGA compatible|3D controller|Display controller' | head -n 1 || true)
  case ${out,,} in
    *nvidia*) echo nvidia ;;
    *"advanced micro devices"* | *[[:space:]]amd[[:space:]]* | *[[:space:]]ati[[:space:]]*) echo amd ;;
    *intel*) echo intel ;;
    *) echo unknown ;;
  esac
}

# Suggested base packages for the detected GPU. This is advice shown to the
# user, not an automatic install: a wrong driver is worse than no extra driver.
gpu_packages() {
  case $(gpu_vendor) in
    nvidia) echo "ice:nvidia ice:nvidia-utils" ;;
    amd) echo "ice:mesa ice:vulkan-radeon" ;;
    intel) echo "ice:mesa ice:vulkan-intel" ;;
    *) echo "" ;;
  esac
}

# NTFS partitions currently visible, one block device per line.
ntfs_partitions() {
  local lsblk_bin=${LSBLK:-lsblk}
  "$lsblk_bin" -lnpo NAME,FSTYPE 2>/dev/null |
    awk '$2 ~ /^ntfs/ { print $1 }'
}

# A Windows/dual-boot installation is likely when NTFS is present or the
# firmware exposes a Windows Boot Manager entry.
detect_dual_boot() {
  if [[ -n $(ntfs_partitions) ]]; then
    echo yes
    return
  fi
  if command -v efibootmgr >/dev/null 2>&1 &&
    efibootmgr 2>/dev/null | grep -qi 'Windows Boot Manager'; then
    echo yes
    return
  fi
  echo no
}

hardware_summary() {
  local gpu dual ntfs
  gpu=$(gpu_vendor)
  dual=$(detect_dual_boot)
  ntfs=$(ntfs_partitions | paste -sd, -)
  printf 'GPU: %s\nDual-boot/NTFS: %s%s\n' \
    "$gpu" "$dual" "${ntfs:+ ($ntfs)}"
  if [[ $gpu == nvidia ]]; then
    printf 'Suggested (optional): %s\n' "$(gpu_packages)"
  fi
}
