# Installing Forsteri

The live medium boots to a TUI installer on tty1. Other ttys and
`FORSTERI_NO_INSTALL=1` give a root shell; `forsteri-install` starts it again.

## What the installer asks

1. **Welcome** -- explains what will happen.
2. **Sync** -- `forst sync` for all five codenames. Needs a network. If every
   codename fails, the installer stops rather than installing a half system.
3. **Disk** -- the list comes from `lsblk --nodeps --paths`. The installer
   refuses a disk that has mounted partitions (that is usually the live medium).
4. **Confirmation** -- you type `ERASE`.
5. **Firmware** -- detected: `/sys/firmware/efi`.
6. **Filesystem** -- ext4 (default) or btrfs without subvolumes.
7. **Hostname**, **user**, **two passwords** (user, root), **timezone**,
   **locale**, **console keymap**.
8. **Desktop** -- GNOME, KDE Plasma, Hyprland, MangoWC + Quickshell, or minimal.
9. **Review** -- a summary of the choices above, then the point of no return.

## Partition layout

GPT, created with `sgdisk`:

| firmware | partition 1 | partition 2 |
|----------|-------------|-------------|
| UEFI | 512 MiB, EF00, partition `FORSTERI-EFI`, FAT32 label `FORSTERIEFI`, mounted at `/efi` | rest, 8304, `FORSTERI-ROOT` |
| BIOS | 2 MiB, EF02, `FORSTERI-BOOT` (no filesystem) | rest, 8304, `FORSTERI-ROOT` |

Root is labelled `Forsteri`; the EFI *filesystem* is labelled `FORSTERIEFI`
(11 characters, the FAT32 limit), while the GPT partition keeps the longer
name `FORSTERI-EFI`. `genfstab -U` writes `/etc/fstab` with UUIDs. There is no
swap partition: add a swapfile later if you need one.

## What happens during the install

```
resume_scan         mount each disk partition read-only, look for install.state
partition_disk      wipefs + sgdisk, partprobe, udevadm settle
format_partitions   mkfs.fat -F32 -n FORSTERIEFI, mkfs.ext4 (or mkfs.btrfs)
mount_target        root at /mnt, EFI at /mnt/efi
install_base        forst --root /mnt --strict install $(installer/base.list)
write_fstab         genfstab -U /mnt
copy_target_tools   forst binary, repos.conf, os-release, issue
install_branding    plymouth theme, wallpapers, SDDM theme, grub theme
chroot_setup        locale, users, services, plymouth hook, mkinitcpio -P
install_desktop     forst --root /mnt install <profile list>
configure_desktop   skel, SDDM config, enable sddm, bluetooth if present
install_bootloader  grub-install + grub-mkconfig with the Forsteri theme
```

Everything is appended to `/var/log/forsteri-install.log`; `run_logged` keeps
each command line as well as its output.

### Resume state and dry run

The installer keeps `/var/lib/forsteri/install.state` on the live root while it
runs, but also copies it to the target as soon as the target is mounted:
`<root>/.forsteri-install.state`, mirrored to
`<root>/efi/.forsteri-install.state` when an EFI partition exists. The state
records `disk`, `phase`, `rootpart`, `efipart` and `fs` and is updated after
each phase (`partitioned`, `formatted`, `mounted`, `base-installing`,
`base-installed`, `configured`, `complete`).

On startup, before any destructive work, `forsteri-install` mounts each
partition of the selected disk read-only and scans for that file. If it finds
one, it shows the recorded disk, partitions and phase and offers to resume. A
resume reuses the existing partitions and filesystems, skips completed phases,
and re-runs only the interrupted one. Because the target or EFI copy survives a
power cycle, this works across a real reboot, not only after a `kill -9`.

The file is written through a temporary name and `mv`, so a crash cannot leave
a torn phase line. `forsteri-install --resume-probe /dev/vda` performs only the
read-only scan and prints the discovered state; it is the seam used by CI.
`scripts/test-installer-reboot.sh` exercises the path with two QEMU boots of the
same disk, and the ISO's opt-in service removes the live copy before rebooting
so only the target/EFI copy can satisfy the second boot.

`forsteri-install --validate-only` runs the tool checks, package-list lint,
GPU/NTFS detection and (with `FORSTERI_DISK=/dev/...`) a read-only disk
inspection. It writes nothing. `--check` remains the narrower data-file lint.

### Why the installer configures things itself

`forst` never runs upstream scriptlets, by design (see
`docs/architecture.md`). Arch packages normally do their first-boot work in
`.INSTALL` scripts; the installer performs the equivalent steps explicitly in
`/root/forsteri-setup.sh` inside the target, which is generated from
`iso/airootfs/usr/lib/forsteri/installer/system.sh` and removed afterwards.

### Hardware discovery

`iso/airootfs/usr/lib/forsteri/installer/hardware.sh` holds the read-only
probes: `gpu_vendor` (nvidia/amd/intel/unknown), `gpu_packages` (advice only),
`ntfs_partitions` and `detect_dual_boot`. They are unit-tested with fake
`lspci`/`lsblk` probes, so a wrong driver is never installed automatically and
a dual-boot warning does not depend on parsing a TUI.

The account password and root password are passed through
`/root/forsteri-install.env` (mode 600), sourced by that script, then deleted.

## Desktop selection

| choice | packages | session |
|--------|----------|---------|
| GNOME | `ice:@gnome` + tweaks + fonts | GNOME on Wayland |
| KDE Plasma | `ice:@plasma` + dolphin/kate/ark | Plasma on Wayland |
| Hyprland | Hyprland, Waybar, wofi, portal, kitty | Hyprland |
| MangoWC | `nest:mangowm`, `ice:quickshell`, kitty | `mango-forsteri` (compositor + bar) |
| minimal | two comfort tools | TTY only |

All four graphical choices use SDDM with the Forsteri theme.

If `nest` is unreachable, the MangoWC install fails on `nest:mangowm` and the
installer offers to install Hyprland instead rather than leaving you without a
session.

## After the first boot

```bash
forst repos                       # codenames and their package counts
forst install tundra:neofetch     # an asset from Debian
forst install ice:@plasma         # a whole desktop group
forst remove htop
forst files forst | head
```

`forst` lives at `/usr/bin/forst` (from `nest`) and is also copied into
`/usr/local/bin` by the installer so the very first boot has it even before
`nest` is reachable.

## When something goes wrong

* The log is `/var/log/forsteri-install.log`; the installer shows it before
  dying and never reboots on failure.
* If the installer exits, the target stays mounted at `/mnt` so you can inspect
  it; `umount -R /mnt` when done.
* A failed base install (`--strict`) means the base codenames could not satisfy
  a dependency. Run the same command by hand to see which:

  ```bash
  forst --root /mnt --strict install $(grep -v '^#' /usr/share/forsteri/installer/base.list)
  ```
