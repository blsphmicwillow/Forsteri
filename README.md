# Forsteri OS

An Arch-derived distribution built from scratch around **one package manager,
five codenames, and a quiet boot**. Forsteri keeps a single Arch base, then
lets you pull packages from Arch, Debian and Fedora with a codename in front of
the name:

```
forst install htop            # base order: colony, ice, nest
forst install tundra:htop     # Debian's build
forst install aurora:ripgrep  # Fedora's build
forst install ice:@plasma     # a whole Arch group
```

The name is *Aptenodytes forsteri*, the emperor penguin. The codenames follow
suit:

| codename | upstream | format | role |
|----------|----------|--------|------|
| `colony` | Arch core | pacman | base system |
| `ice` | Arch extra | pacman | desktops, applications |
| `nest` | Forsteri's own repo | pacman | branding, mangowm, forst updates |
| `tundra` | Debian stable main | deb | `.deb` assets |
| `aurora` | Fedora Everything | rpm | `.rpm` assets |

## What you get

* **`forst`** -- a ~4k-line Rust binary with no crate dependencies: its own
  SHA-256, tar/ar/cpio readers, three index parsers, a cross-codename resolver
  and a file ledger. It installs Arch `.pkg.tar.zst`, Debian `.deb` and Fedora
  `.rpm` payloads into one database. Metadata and packages are signature-checked
  (gpgv for Debian metadata and Arch packages, rpmkeys for Fedora), installs are
  staged before an atomic ledger commit, and `forst verify` re-hashes every
  installed file against the ledger.
* **A live ISO** (archiso profile) that boots straight into a TUI installer.
* **An installer** with a desktop selector: GNOME, KDE Plasma, Hyprland, or
  MangoWC with a Quickshell bar -- plus a minimal TTY option.
* **Seven theme codenames**: `ember` (Forsteri's warm default), `drift` and
  `glass` (macOS looks: translucent, controls left), `monolith` and `void`
  (Nothing OS: black, square, monochrome) and `shelf` and `panes` (Windows:
  flat panes, blue, controls right) -- chosen at install time and re-applied
  with `forsteri-theme-apply <codename>`.
* **Frost** -- the NT-compatibility work inside the Linux kernel, kept in
  the separate `FosteriOS` tree. The live image loads its `ntpersonality`
  module; it recognises PE images and parses them, and does not yet execute
  them.
* **A quiet boot**: an SDDM login screen with the emblem, and a Plymouth splash
  that fades the mark in over a thin progress line.

## Status

### Kernel: Frost

Frost is the kernel-side name for the NT personality. M0 (module load,
`/proc/forsteri_nt`, PE magic at `execve`) and M1 (bounds-checked PE/COFF
parser with real mingw and malformed fixtures) are complete and tested. It does
not run PE code yet; M2 is image mapping and the first static hello-world.

Verified in this tree:

* `cargo test` -- 24 tests covering SHA-256, archive readers, all three index
  parsers, the resolver's cross-codename rules, the foreign-package guard and
  the ledger.
* Live repository sync against real mirrors: `colony` 300 packages,
  `ice` 15,303, `tundra` 63,440, `aurora` 68,586 (November 2026 snapshot).
* Real installs into a scratch root: an Arch `.pkg.tar.zst` with its
  dependencies, a Debian `.deb`, a Fedora `.rpm`; `forst list`, `files`,
  `remove` and the checksum path all exercised.
* Group expansion against real data: `ice:@plasma` resolves to 702 packages,
  `ice:@gnome` to 706.
* `bash -n` on every shell script, plus the installer's own
  `forsteri-install --check` lint (package lists, profiles and themes).
* `scripts/check.sh --network`: a real Arch package and a real Debian package
  installed into scratch roots, the installed `.deb` binary executed, and the
  ledger checked across install, re-install and remove.
* The ISO itself: built with `mkarchiso` (archiso 92) and booted in QEMU under
  BIOS, reaching userspace with the live root mounted. See the build notes in
  `docs/building.md`.

Not yet verified (see `docs/building.md` for the exact gaps):

* The installer end to end on a disk: partitioning, `grub-install` and the
  first boot of an installed system.
* The QML in the SDDM theme and the Quickshell bar -- they compile only on a
  machine with Qt, which this tree was not built on.
* MangoWC: `nest` needs `mangowm` built with matching `wlroots0.20`/`scenefx0.5`.

## Try the package manager

```bash
cd pkg/forst
cargo build --release
./target/release/forst repos
sudo ./target/release/forst sync
./target/release/forst search quickshell
./target/release/forst install --root /tmp/root --dry-run ice:hyprland
```

`forst --help` lists everything. The ledger lives in `/var/lib/forst`, the
index cache in `/var/cache/forst`, the codenames in `/etc/forst/repos.conf`.

## Build the ISO

```bash
scripts/build-iso.sh --docker     # or: sudo scripts/build-iso.sh on Arch
```

The result is `out/forsteri-YYYY.MM.DD-x86_64.iso`. See `docs/building.md`.

## Repository layout

```
pkg/forst/          the package manager (Rust, std only)
iso/                archiso profile: boot menus, live root, installer
installer/          base package list used by the installer
desktop/            per-desktop profiles, skel homes, Hyprland/Waybar,
                    Quickshell, SDDM theme, GTK + KDE theming
branding/           supplied artwork + generated PNGs, Plymouth, GRUB, SDDM
packages/           PKGBUILDs for the `nest` codename
scripts/            build-forst, stage-share, build-iso, make-repo, check
docs/               architecture, codenames, install, desktop, building
```

## Design decisions worth knowing

1. **One base, many sources.** Only `pacman`-format codenames may satisfy a
   base package's dependencies. `deb`/`rpm` codenames are asset sources: their
   system-level requirements (`libc6`, `bash`) are treated as provided by the
   base instead of overwriting it, and a guard refuses explicit attempts to
   install `glibc`, `systemd`, `pacman` and friends from a foreign codename.
2. **No upstream scriptlets.** `forst` extracts payloads and records files; it
   never runs dpkg/rpm/post-install scripts. The installer therefore performs
   the handful of steps Arch scriptlets would have done (locale, users,
   `mkinitcpio -P`, `systemd-sysusers`, presets) explicitly, where you can read
   them.
3. **Names, not sonames.** Dependencies are reduced to package names at index
   time: version constraints and alternatives are dropped, and rpm sonames
   (`libm.so.6()(64bit)`) are ignored rather than guessed at.
4. **Pinned codenames are exact.** `colony:tree` fails with "it exists in: ice,
   tundra, aurora" rather than silently installing a different distribution's
   build.

## Licence

The packaging, branding and documentation in this repository are MIT. Packages
installed by Forsteri keep their own upstream licences.
