# Desktops

Forsteri treats a desktop as data: one package list, one skel home. The
installer's selector is just a menu over `desktop/profiles/*.list`.

## The four sessions

### GNOME (`gnome.list`)

`ice:@gnome` (706 packages at the time of writing), `gnome-tweaks`, the
PipeWire stack and fonts. `forsteri-theme-apply` runs on first login and sets:

* window buttons to the left (`close,minimize,maximize:`)
* dark colour scheme, gold accent
* Inter for UI, JetBrains Mono for monospace

That is the whole GNOME "macOS feel": button placement, spacing and colour.
No shell extensions are installed, so nothing is pretending to be something it
is not. With the `drift` theme selected (see below) the same helper also sets
the amber accent and the 12px radii.

### KDE Plasma (`kde.list`)

`ice:@plasma` plus Dolphin, Kate, Ark, `plasma-nm` and SDDM.
`forsteri-theme-apply` renders the selected theme as a KDE colour scheme in
`~/.local/share/color-schemes/<Name>.colors` and selects it with `kwriteconfig6`,
so it appears in *System Settings → Colours*. Plasma's default layout (panel at
the bottom, Breeze) is left alone; move it to the top for the Mac arrangement.

### Hyprland (`hyprland.list`)

Hyprland plus Waybar, wofi, kitty, grim/slurp, wl-clipboard, brightnessctl,
polkit-gnome, the hyprland portal and the PipeWire stack.

`~/.config/hypr/hyprland.conf` ships with:

```ini
$mod = SUPER
bind = $mod, Return, exec, kitty
bind = $mod, D, exec, wofi --show drun
bind = $mod, Q, killactive
bind = $mod, L, exec, loginctl lock-session
```

Round corners (12), soft blur, warm shadow, gold active border
`rgba(f2c14edd)`, and `swaybg` with the Forsteri wallpaper. Waybar is a
floating rounded bar with the emblem, workspaces, clock, tray, volume,
brightness, battery and a power button -- styled in
`~/.config/waybar/style.css`.

There is no lock screen binary in the profile: `Super+L` asks logind to lock,
and a locker such as `hyprlock` can be added later.

### MangoWC + Quickshell (`mangowc.list`)

MangoWC is a dwl-derived compositor: tags instead of workspaces, animations,
blur, window rules, XWayland, and a `mmsg` IPC client. It is not in Arch's
repositories, so it is packaged in `packages/mangowm/PKGBUILD` and served from
`nest`.

MangoWC has no shell of its own, which is where Quickshell comes in:
`~/.config/quickshell/shell.qml` draws one floating bar with the emblem, clock,
system tray, battery and a power button. The session entry
`/usr/share/wayland-sessions/mango-forsteri.desktop` starts both:

```
Exec=sh -c 'quickshell >/dev/null 2>&1 & exec mango'
```

MangoWC reads `/etc/mango/config.conf` (shipped by the package) and
`~/.config/mango/config.conf` (create your own). Autostart commands go in
`config.conf` as `exec-once = ...`.

**Caveats, stated plainly:**

* `mangowm` depends on `wlroots0.20` and `scenefx0.5`, which are AUR packages.
  Build those into `nest` (or replace them with matching git builds) before
  installing the MangoWC profile; mixing versions is the usual cause of a
  compositor that refuses to start.
* The Quickshell QML here is a starting point written against Quickshell's
  documented API. It has not been run in this tree -- check it on a real
  session with `quickshell` in a terminal, which prints QML errors.
* No MangoWC config is shipped: upstream's default config is installed to
  `/etc/mango/config.conf` by the package, and inventing keybindings here would
  just be a second, worse default.

## Themes: seven look-and-feel codenames

Themes are a second, separate codename namespace from the package codenames
(`colony`, `ice`, `nest`, `tundra`, `aurora`). A theme is one file:

```
desktop/themes/<codename>/theme.conf
```

and one renderer, `/usr/local/bin/forsteri-theme-apply`, which turns it into
GTK3/GTK4 CSS, a KDE colour scheme, GNOME `gsettings` keys and the Hyprland
variables the shipped config sources.

| codename | family | what it does |
|----------|--------|--------------|
| `ember` | Forsteri | the default: warm ivory on near-black, gold accent, 12px corners |
| `drift` | macOS | translucent dark panels, amber accent, controls on the left, 12px |
| `monolith` | Nothing OS | pure black, a single red accent, 2px corners, monospace UI, no blur |
| `shelf` | Windows | flat grey, blue accent, controls on the right, 4px, no blur |
| `glass` | macOS | cool-grey translucent panes, blue accent, left traffic lights, 14px |
| `void` | Nothing OS | pure black monochrome, dot-matrix mono UI, 2px, no blur |
| `panes` | Windows | layered flat panes, Windows blue, controls on the right, 4px |

The three new bundles (`glass`, `void`, `panes`) are independent of the
package codenames and of the older four looks. Each is presentation data only:
GTK and Qt colours, an icon-theme name, button placement, a Quickshell layout
hint (`.config/quickshell/forsteri-theme.json`) and optional `ICONS`/`LAYOUT`/
`QT_STYLE` fields. No theme package contains kernel, boot or security code.
The `void` bundle deliberately inherits the monochrome look of `monolith`,
and `glass`/`panes` the macOS/Windows shapes of `drift`/`shelf`; the older
names remain valid.

```bash
forsteri-theme-apply --list        # installed codenames
forsteri-theme-apply monolith      # apply now
forsteri-theme-apply               # re-apply /etc/forsteri/theme
```

The installer asks for a codename and writes the answer to
`/etc/forsteri/theme`; the autostart entry in `desktop/skel/common/` applies it
on first login. The same file is re-read by every later run.

What a theme can change is deliberately narrow: colours, radii, fonts, button
placement, blur, the Hyprland accent and, for Quickshell, a layout hint. It does
not install packages or rearrange KDE/GNOME panels, so a theme never turns KDE
into a macOS clone; it makes Forsteri *read* like one. Per-desktop layout
remains the desktop's own business.

### Shared skel

`desktop/skel/common/` is copied into every user's home:

```
.config/autostart/forsteri-theme.desktop
.config/hypr/theme-vars.conf          the ember defaults, rewritten per theme
```

`/usr/local/bin/forsteri-theme-apply` is the single place that writes any of
this; it is safe to re-run and skips keys the running desktop does not have.

## Login screen

SDDM uses `/usr/share/sddm/themes/forsteri`: the wallpaper, the emblem, two
fields and a gold sign-in button. `/etc/sddm.conf.d/10-forsteri.conf` selects
the theme and enables numlock. The theme's QML is small on purpose (one
centred column, `sddm.login(user, password, sessionIndex)`); it has not been
rendered in this tree, so verify it on first boot with
`sddm --example-config` or by reading `~/.local/state/sddm.log` if the login
screen stays blank.

## Adding a desktop

1. `desktop/profiles/<name>.list` -- package specs, one per line.
2. `desktop/skel/<name>/` -- anything that belongs in the user's home.
3. Add the choice to `pick_desktop` in
   `iso/airootfs/usr/lib/forsteri/installer/desktop.sh`.

A new theme is one directory under `desktop/themes/`; `forsteri-install
--check` validates that `CODENAME` matches the directory and that every key the
renderer needs is present.

Nothing else changes: the installer already copies skel homes, enables SDDM for
any graphical profile, and `scripts/check.sh` will complain if a profile file is
missing.
