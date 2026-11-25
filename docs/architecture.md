# Architecture

Forsteri is three layers: a boot chain, a package manager, and a desktop. This
document follows a boot from firmware to login and explains how the pieces fit.

## Boot chain

```
firmware
  └─ GRUB  (installed system: themed, gold accent, 3s timeout)
       └─ linux + initramfs   quiet splash loglevel=3
            └─ plymouth        forsteri theme: emblem, comet progress line
                 └─ systemd
                      └─ NetworkManager, sddm
                           └─ GNOME | Plasma | Hyprland | MangoWC+Quickshell
```

The **live medium** is an archiso image: `syslinux` for BIOS, `systemd-boot`
for UEFI, both pointing at `install_dir=forsteri`. It boots the stock Arch
kernel from `colony` and autologins `root` on tty1, whose `.bash_profile`
starts `forsteri-install`.

The **installed system** uses GRUB for both firmwares: one code path, one menu,
and BIOS machines need no second configuration. `mkinitcpio` gets `plymouth`
inserted into `HOOKS` right after `udev`, so the splash starts before root is
mounted.

## Package manager: forst

### Shape

One binary, no crates (`pkg/forst/src`):

| file | responsibility |
|------|----------------|
| `util.rs` | SHA-256, decompression, tar/ar/rpm/cpio readers, safe extraction, temp files |
| `config.rs` | codenames: INI parser, mirror URLs, base vs asset |
| `index.rs` | normalising three upstream index formats into one flat record |
| `sync.rs` | downloading indexes (curl) and the repomd→primary hop for Fedora |
| `resolve.rs` | dependency graph, group expansion, foreign-package guard |
| `install.rs` | download, verify, extract, ledger, remove |
| `main.rs` | CLI |

`forst --help` for the command set. Humans type codenames; the binary never
needs to know which upstream tooling produced a package.

### Index normalisation

Each codename is cached at `/var/cache/forst/index/<codename>.idx` as one
tab-separated line per package:

```
name  version  arch  filename  size  sha256  provides  depends  groups  description
```

Upstream shapes it flattens:

* **pacman** -- `<repo>.db` is a gzip tar of `desc` files with `%FIELD%` blocks.
* **deb** -- `dists/<suite>/<component>/binary-<arch>/Packages.xz`, RFC822
  stanzas.
* **rpm** -- `repodata/repomd.xml` → the `primary` file (`primary.xml.zst`),
  XML `<package>` elements.

Decompression is delegated to `zstd`/`xz`/`gzip`/`bzip2`, chosen by magic
bytes; archive reading is done in-process so no `bsdtar` or `rpm2cpio` is
needed.

### Resolution

```
install spec
  ├─ codename pinned?  -> look only there (exact, error lists other codenames)
  ├─ @group?           -> expand from the first codename that has the group
  └─ otherwise         -> base codenames first (colony, ice, nest), then assets
```

Dependencies are expanded depth-first with a 64-deep limit, cycles tolerated,
and dependency-first install order. The rules that make multi-distro sane:

1. A **base** package resolves dependencies only inside base codenames. If the
   base cannot satisfy it, it is reported as unresolved (`--strict` makes that
   fatal) instead of silently becoming a Debian package.
2. An **asset** package resolves inside its own codename first, then the base,
   then other assets.
3. System-level requirements of asset packages (`libc6`, `bash`, ...) are
   considered provided by the base and their foreign copies are never pulled.
4. Explicit requests for foreign base-system packages are refused unless
   `--allow-foreign-system` is passed.

### Ledger

`/var/lib/forst/db/<name>.meta` records identity and deps; `<name>.files`
records every extracted path with a type tag (`f`, `d`, `l`, `h`). `remove`
walks the list backwards (files before their directories) and only deletes
directories that are empty, so shared trees survive.

### Extraction safety

Paths are normalised and `..`/absolute members are refused. Parent directories
are created component by component and the code refuses to write through a
symlink. Setuid bits are preserved only when the installer runs as root.
Metadata members of pacman packages (`.PKGINFO`, `.MTREE`, `.INSTALL`) are
skipped.

### What forst deliberately does not do

* Run install scriptlets or triggers (see below).
* Resolve sonames or file dependencies (`libx.so.7`, `/usr/bin/sh`).
* Track versions for upgrades beyond "replace the files, rewrite the ledger".
* Fetch from the AUR, or build anything from source.

## The installer

`forsteri-install` is bash plus `dialog`, split into `common`, `disk`,
`system`, `desktop`, `boot`. It never calls pacstrap: the base system is
`forst --root /mnt install` over `installer/base.list`, which is why the same
package lists that build the ISO's world also build yours.

Because forst does not execute Arch scriptlets, the installer performs the
first-boot work itself inside `arch-chroot`:

* timezone, `hwclock --systohc`
* locale generation, `/etc/locale.conf`, `/etc/vconsole.conf`
* hostname, `/etc/hosts`
* root and user passwords, `wheel` sudo, home skeleton
* `systemd-machine-id-setup`, `systemd-sysusers`, `systemctl preset-all`
* enabling NetworkManager, timesyncd, fstrim
* `plymouth` in `HOOKS`, then `mkinitcpio -P`

Every step is a line in `/var/log/forsteri-install.log`.

## Desktop layer

The desktop is data, not code: `desktop/profiles/<name>.list` lists package
specs, `desktop/skel/<name>/` is copied into `/etc/skel` and the new user's
home. Adding a desktop means adding two paths, no installer changes. See
`docs/desktop.md`.

## Branding

The emblem is code: `branding/render_assets.py` draws it from Bézier control
points with Pillow and writes every bitmap the boot chain needs (splash logo,
progress track and comet, GRUB background, wallpaper). The SVG in
`branding/logo/` is the same geometry for desktop use.
