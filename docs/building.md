# Building

Two artefacts can be built: the ISO, and the `nest` package repository.

## Requirements

| what | why |
|------|-----|
| Rust (cargo, 1.70+) | builds `forst`; no crates are downloaded |
| Python 3 + Pillow | scales the supplied brand artwork into install assets |
| archiso 92-1 + mkinitcpio 42.2-1 | pinned ISO tools; see `iso/tool-versions.env` |
| docker | alternative: builds the ISO inside `archlinux:latest` |
| pacman tools (`makepkg`, `repo-add`) | builds `nest` packages |
| `linux-headers` matching the live kernel | builds the NT personality module |
| a FosteriOS checkout | Frost source; defaults to `./FosteriOS`, then `../FosteriOS` |

## The ISO

```bash
scripts/build-iso.sh --docker      # any host with docker
sudo scripts/build-iso.sh          # on Arch with archiso installed
```

The script:

1. `scripts/build-forst.sh` -- `cargo build --release`, installs the binary to
   `iso/airootfs/usr/local/bin/forst` and prints its version.
2. `branding/render_assets.py` -- derives `branding/dist/*.png` from `branding/source/*.webp`.
3. `scripts/stage-share.sh` -- copies branding, profiles, skel homes, the SDDM
   theme and the session entry into `iso/airootfs/usr/share/forsteri/`.
4. `scripts/build-nt-module.sh` -- builds `ntpersonality.ko` from the Frost
   checkout against the live kernel's headers and installs it into
   `iso/airootfs/usr/lib/forsteri/nt/`.
5. `mkarchiso -v -w /tmp/forsteri-work -o out iso/`.

The host build refuses `archiso`/`mkinitcpio` versions other than the pins in
`iso/tool-versions.env`; `FORSTERI_ALLOW_TOOL_DRIFT=1` is the explicit override.
The Docker path installs those exact versions.

## Repository signatures

`forst` checks the trust anchor each upstream actually provides:

* **pacman (colony/ice/nest):** detached `.sig` for every package, checked
  with the Arch keyring, exactly as pacman does. Arch no longer signs `*.db`;
  if a mirror does publish a database signature, forst verifies that too.
* **Debian (tundra):** clearsigned `InRelease` with the archive keyring, whose
  SHA256 section binds `Packages.xz`; every package must match that checksum.
* **Fedora (aurora):** Fedora does not publish `repomd.xml.asc`, so forst
  verifies each RPM with `rpmkeys` against the pinned Fedora key. A mirror that
  does publish a repomd signature also gets it verified.

A missing signature, keyring, checksum or a bad digest is a hard error; there
is no unsigned fallback. The public archive keyrings live in
`pkg/forst/keys/` and are staged into `/usr/share/forsteri/keyrings/`.
Refresh the Debian/Fedora anchors with pinned fingerprints:

### Trust-model decision (2026-11-25)

Fedora definitely does not publish `repomd.xml.asc`; its documented model is
per-package RPM signatures plus HTTPS metadata. For Forsteri that is acceptable
**only for `aurora`, the asset codename**, because (a) `rpmkeys` must print a
positive signature line for every RPM, (b) `aurora` can never satisfy a base
package dependency, and (c) the foreign-system guard refuses `glibc`, `systemd`,
`rpm` and the rest unless the operator explicitly overrides it. Metadata replay
is still possible (an attacker can serve an older signed RPM), so this is not a
strong trust anchor for a base system; do not use `aurora` for one. A future
stronger anchor would be a pinned Fedora release checksum or a signed snapshot,
not an invented repomd signature.

`--allow-foreign-system` is retained as an explicit, unsupported escape hatch
for a deliberate foreign base install. It bypasses only the base-system guard;
it does not bypass signatures, digests, the ledger or transactional staging. The
installer never passes it. Keep the flag visible in `--help` so the refusal and
the override are both auditable.

```bash
scripts/fetch-keyrings.sh
```

`nest` must be signed too: use `repo-add --sign` (or an equivalent detached
`nest.db.sig`). An unsigned local repository will be rejected by design.

Output: `out/forsteri-<date>-x86_64.iso`, UEFI and BIOS.

### NT personality module

The live image loads `ntpersonality.ko` on every normal boot through
`forsteri-nt.service`, and exposes `/proc/forsteri_nt`. The module must be built
for exactly the kernel the ISO installs: the script defaults to the only
`/lib/modules/*/build` tree available, which is correct in the Docker build and
on an up-to-date Arch host. If several are installed, pass the live version:

```bash
FOSTERIOS_DIR=../FosteriOS FORSTERI_KVER=7.3.5-arch1-1 scripts/build-nt-module.sh
```

To verify the real image (not the synthetic kernel-test initramfs), boot the ISO
with the opt-in self-test and watch the serial log:

```bash
scripts/test-iso.sh        # needs qemu, xorriso, blkid
```

It extracts the ISO's own kernel and initramfs, boots the live squashfs with
`forsteri.nt_test=1`, and requires `FORSTERI-NT-TEST: PASS`.

What the ISO contains: the live root from `iso/packages.x86_64` (base, kernel,
firmware, GRUB, `arch-install-scripts`, `dialog`, plymouth), the installer, the
branding and `forst`. It does **not** embed a package repository: the live
medium expects a network.

## The nest repository

```bash
scripts/make-repo.sh               # writes out/repo/x86_64
sudo mkdir -p /srv/forsteri/repo
sudo cp -r out/repo/x86_64 /srv/forsteri/repo/
```

That path matches the `nest` codename's `file:///srv/forsteri/repo/x86_64`.
`mangowm` needs `wlroots0.20` and `scenefx0.5` built first (AUR packages); see
`docs/desktop.md`.

## Checks

```bash
scripts/check.sh              # offline: syntax, tests, data, assets
scripts/check.sh --network    # also install real packages and check the ledger
```

Offline, in order:

1. `bash -n` for every shell script and the installer libraries;
2. `python3 -m py_compile branding/render_assets.py`;
3. `cargo test` for `forst` (24 tests);
4. the installer's own `forsteri-install --check`, which lints every package
   list and theme, and asserts that every desktop profile exists;
5. the presence of every generated brand asset.

With `--network`, `scripts/check-forst.sh` runs as well. It builds the release
binary, syncs `colony`, `ice` and `tundra` from the real mirrors, then:

* installs `ice:tree` into a temporary root and proves the ledger lists it from
  `ice` with `glibc` resolved from `colony`, that `files tree` names
  `/usr/bin/tree`, that the file exists and is an ELF binary, and that
  re-installing does not duplicate the ledger;
* installs `tundra:hello` (a Debian `.deb`, whose `libc6` requirement is
  satisfied by the base), then **runs the installed binary** and checks it
  prints `Hello, world!`;
* confirms `tundra:libc6` is still refused by the foreign-system guard;
* removes both, and checks the ledger and the files agree at every step.

Nothing touches the host: roots, database and cache live under a `mktemp -d`
directory that is removed on exit.

## Manual verification of the package manager

These are the commands used while building this tree, against live mirrors:

```bash
cd pkg/forst && cargo build --release
export FORST_CACHE=/tmp/fcache
./target/release/forst sync colony ice tundra aurora
./target/release/forst repos

# real payloads from all three formats into a scratch root
./target/release/forst install --root /tmp/froot -y ice:tree
./target/release/forst install --root /tmp/froot -y tundra:hello
./target/release/forst install --root /tmp/froot -y aurora:hello
./target/release/forst list --root /tmp/froot
./target/release/forst files hello --root /tmp/froot
./target/release/forst remove --root /tmp/froot -y tree

# the guard and the group expansion
./target/release/forst install --root /tmp/froot -y tundra:libc6    # refused
./target/release/forst install --dry-run ice:@plasma                # 702 packages
```

## Known gaps

These are the parts that still need real hardware, a Qt host, or a built `nest`:

1. **The installer has not run end to end.** Its lint mode passes, the ISO it
   lives in boots, and every command it calls is a standard one, but
   partitioning, `arch-chroot`, `grub-install` and the first boot of a freshly
   installed system were not executed here (that needs write access to a block
   device, which the build host did not have).
2. **Qt code is only partly verified.** `desktop/sddm/forsteri/Main.qml` was
   rendered offscreen with Qt 6/PySide6 against mocked SDDM models (lockup,
   prefill and bottom form all present), but not in a live SDDM greeter.
   `desktop/skel/mangowc/.config/quickshell/shell.qml` is still uncompiled; it
   needs Quickshell, and the MangoWC profile needs a built `nest` first.
3. **`nest` is empty until you build it.** `mangowm` and `forst` packaging
   exist; the repository itself is generated by `scripts/make-repo.sh`. The
   MangoWC profile therefore fails at install time until that exists.
4. **The live splash is untested visually.** Plymouth is in the live initramfs
   (`HOOKS` includes `plymouth` before `archiso`), but no frame was inspected;
   the serial console only proves the boot reaches userspace.

## Testing the ISO in a VM

The medium logs to `tty0` and `ttyS0`, so a headless machine is enough:

```bash
qemu-img create -f qcow2 disk.qcow2 30G

# BIOS / syslinux
qemu-system-x86_64 -accel tcg -m 2G -smp 2 \
  -cdrom out/forsteri-*.iso -boot d -display none -nic none \
  -serial file:/tmp/forsteri-bios.log

# UEFI / systemd-boot
qemu-system-x86_64 -accel tcg -m 2G -smp 2 \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF.4m.fd \
  -cdrom out/forsteri-*.iso -boot d -display none -nic none \
  -serial file:/tmp/forsteri-uefi.log
```

Add `-enable-kvm` (and drop `-accel tcg`) when `/dev/kvm` is available: the
full ISO boot takes a few minutes under emulation and seconds with KVM. The
installer runs on tty1; watch the serial log for the boot and use the graphical
console (or `-display gtk`) for the installer itself.
