# Codenames

A codename is a named collection of packages. It is the only repository
identifier a user types:

```bash
forst install htop              # spec without a codename
forst install colony:htop       # Arch core, exact
forst install tundra:htop       # Debian's build
forst install ice:@plasma       # a group, in one codename
forst repos                     # what is configured, with package counts
```

## The five

| codename | upstream | format | kind | notes |
|----------|----------|--------|------|-------|
| `colony` | `https://geo.mirror.pkgbuild.com/core/os/x86_64` | pacman | base | glibc, systemd, pacman, filesystem |
| `ice` | `.../extra/os/x86_64` | pacman | base | desktops, apps, fonts |
| `nest` | `file:///srv/forsteri/repo/x86_64` | pacman | base | Forsteri's own builds |
| `tundra` | `http://deb.debian.org/debian`, suite `bookworm` | deb | asset | `.deb` files |
| `aurora` | Fedora 43 Everything | rpm | asset | `.rpm` files |

Names come from the emperor penguin: colonies on the ice, nests for its own
work, the tundra and the aurora for the two foreign sources.

`nest` is a file:// URL on purpose: it is meant to sit on the installed system
or a LAN mirror, built from `packages/*/PKGBUILD` by `scripts/make-repo.sh`.
A missing `nest` is not an error: `forst sync` reports it and continues, and
installing the MangoWC profile fails with a clear message until the repository
exists.

These five are *package* codenames. The look-and-feel codenames (`ember`,
`drift`, `monolith`, `shelf`) are a separate namespace described in
`docs/desktop.md`.

## The three kinds of foreign package

Forsteri does not try to be a Debian or a Fedora. It keeps one base and treats
other formats as sources of *files*:

1. **Assets** -- fonts, themes, docs, static tools, art. Their system
   dependencies (`libc6`, `bash`, `rtld`) are treated as provided by the base,
   so nothing foreign replaces Forsteri's libc.
2. **Base-system packages** -- `glibc`, `libc6`, `systemd`, `pacman`, `apt`,
   `rpm`, kernels, `sudo`, `mesa`, `wayland`. Explicitly installing one of
   these from a foreign codename is refused:

   ```
   forst: error: tundra:libc6 is a base-system package; installing a deb system
   package over Forsteri's base would break the system. Use
   `--allow-foreign-system` if you really mean it.
   ```
3. **Base packages** -- anything from `colony`, `ice`, `nest`. These behave
   like a normal pacman install, and their dependencies never leave the base.

## Dependency rules

| package from | dependency lookup order |
|--------------|-------------------------|
| base codename | base codenames only (`colony`, `ice`, `nest`) |
| asset codename | same codename → base codenames → other assets |

Consequences worth knowing:

* `colony:tree` does not exist; the error says
  `no package "tree" in codename colony (it exists in: ice, tundra, aurora)`.
  Pinning is exact.
* `ice:tree` resolves `glibc` inside the base even though Debian also has a
  package called `glibc`.
* `colony:foo` whose dependency is missing from the base is reported as
  unresolved. Add `--strict` to make that fatal (the installer uses it for the
  base system).

## Adding a codename

Add a section to `/etc/forst/repos.conf`:

```ini
[ice-testing]
format      = pacman
url         = https://geo.mirror.pkgbuild.com/extra-testing/os/x86_64
db          = extra-testing
arch        = x86_64
enabled     = yes
description = Arch extra testing
```

`format` is one of `pacman`, `deb`, `rpm` (aliases: `arch`, `debian`/`apt`,
`fedora`/`dnf`). Debian sections need `suite` and `component`; pacman sections
need `db`. Then `forst sync ice-testing`. If a codename is not synced, `forst
install` warns and skips it.

A second Debian release is just another section:

```ini
[tundra-next]
format    = deb
url       = http://deb.debian.org/debian
suite     = trixie
component = main
arch      = amd64
```

## Index format on disk

`/var/cache/forst/index/<codename>.idx`, one line per package:

```
name  version  arch  filename  size  sha256  provides  depends  groups  description
```

It is a cache: delete it, or run `forst sync`, whenever an upstream changes.
`forst` refuses an index whose header is not `#forst2` and asks for a sync.
