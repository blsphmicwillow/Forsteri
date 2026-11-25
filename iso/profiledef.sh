#!/usr/bin/env bash
# shellcheck disable=SC2034
#
# archiso profile: Forsteri OS live medium.
#   sudo mkarchiso -v -w /tmp/forsteri-work -o out iso/
#
# The live system boots a stock Arch kernel (from `colony`) with a Plymouth
# splash, then autologins on tty1 into forsteri-install.

iso_name="forsteri"
iso_label="FORSTERI_$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y%m)"
iso_publisher="Forsteri OS"
iso_application="Forsteri OS Live"
iso_version="$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y.%m.%d)"
install_dir="forsteri"
buildmodes=('iso')
bootmodes=('bios.syslinux'
           'uefi.systemd-boot')
arch="x86_64"
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'zstd' '-Xcompression-level' '15' '-b' '1M')
bootstrap_tarball_compression=('zstd' '-c' '-T0' '--auto-threads=logical' '--long' '-19')
file_permissions=(
  ["/etc/shadow"]="0:0:400"
  ["/etc/gshadow"]="0:0:400"
  ["/root"]="0:0:750"
  ["/root/.bash_profile"]="0:0:644"
  ["/usr/local/bin/forst"]="0:0:755"
  ["/usr/local/bin/forsteri-install"]="0:0:755"
  ["/usr/local/bin/forsteri-nt-selftest"]="0:0:755"
)
