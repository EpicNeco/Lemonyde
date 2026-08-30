<img width="850" height="423" alt="githubbanner" src="https://github.com/user-attachments/assets/121751f1-c4ed-4236-ad62-5d8b79254a02" />

  [![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?style=for-the-badge\&logo=discord\&logoColor=white)](https://discord.gg/6h28MNTXTS)

</div>

# 🍹🍋 Lemonyde 
An unofficial, community-made **bootstrapper for Sober** — the [VinegarHQ](https://sober.vinegarhq.org)
flatpak that runs Roblox on Linux. Lemonyde gives Sober a native GTK4 +
libadwaita GUI for the stuff you'd otherwise do by hand in a terminal or
text editor: installing/updating/launching Sober, editing FastFlags
(FFlags), and tweaking its wrapper settings.

keep in mind : **LEMONYDE IS IN DEVELOPMENT SO EXPECT CHANGES!**

**Not affiliated with Roblox Corporation or VinegarHQ.** Sober itself stays
exactly as VinegarHQ ships it — Lemonyde only edits Sober's own config files
and calls `flatpak` on your behalf.

## Features

- **One-click install / update / launch / uninstall** for `org.vinegarhq.Sober`,
  with a live streaming log so you can see exactly what Flatpak is doing.
- **Multi-instance launch** — open several *isolated* Sober windows at once
  (e.g. to play on separate accounts side by side). Sober enforces its own
  single-instance lock, so Lemonyde gives each numbered slot its own fake
  `$HOME` (stored under `~/.local/share/lemonyde/instances/slot-N`) — that's
  what a launched Sober instance uses to find its own `~/.var/app/...` data,
  so each slot gets a fully separate lock file, config, and login session
  that persists between launches. Lemonyde also tracks which slots it has
  launched and won't spawn a second process into a slot that's already
  running — that double-launch (e.g. clicking Launch again before the first
  onboarding wizard finished) was the actual cause of the "frozen instance"
  crash. **Heads up:** Roblox's rules treat running multiple clients at the
  same time as against policy, and community reports say it can trigger an
  anti-cheat flag on your account — Lemonyde shows this warning in the app
  too, so use it at your own discretion. Note the FFlag/Settings editors
  only manage your *main* Sober install, not the isolated instance slots.
- **FFlag editor** — add, edit, and remove entries in the `fflags` section of
  `~/.var/app/org.vinegarhq.Sober/config/sober/config.json`, with:
  - Three ready-made presets (Low-End/VRAM Fix, Balanced, Maximum Fidelity)
  - A library of flags **confirmed to be on Roblox's client-config allowlist**
    (flags outside the allowlist are silently ignored by Roblox, so Lemonyde
    doesn't bother listing unconfirmed ones)
- **Customize tab** — swap Roblox's in-game cursor (arrow, zoomed-out arrow,
  text cursor) via Sober's documented `asset_overlay` mechanism, with
  one-click file picking and a reset-to-default button per cursor. Any
  source image works — Lemonyde automatically resizes and centers it on a
  transparent 64×64 canvas (the exact dimensions Roblox's cursor format
  expects, per Bloxstrap's modding docs), so all three cursors render at a
  consistent size in-game instead of whatever size the source file happened
  to be. It also warns you if the picked image has no transparency, since
  Roblox renders a solid box around a fully-opaque cursor. Also includes an
  **experimental** font override using the same `asset_overlay` mechanism —
  flagged as experimental because, unlike the cursor paths (which VinegarHQ's
  own docs confirm), Sober doesn't document the exact filename its
  Android-based client expects for a font override, so you may need to try
  a few names.
- **Wrapper settings** — toggle `use_opengl`, `enable_hidpi`, `close_on_leave`,
  and edit `touch_mode`, all stored at the root of `config.json`.
- **Advanced tools** — open the config/data folders, clear the asset overlay,
  and do a full FFlag/config reset (mirrors VinegarHQ's official reset steps).
- **And many more.**
  
## Requirements

To **build**:
- Rust (stable, via [rustup](https://rustup.rs) — the crates this depends on
  need a fairly current toolchain)
- GTK 4 and libadwaita development headers:
  - Debian/Ubuntu: `sudo apt install libgtk-4-dev libadwaita-1-dev build-essential`
  - Fedora: `sudo dnf install gtk4-devel libadwaita-devel`
  - Arch: `sudo pacman -S --needed gtk4 libadwaita base-devel`

To **run**: GTK 4.14+ and libadwaita 1.5+ runtime libraries (already on
most current GNOME/KDE desktops), plus Flatpak (with the Flathub remote) to
install/launch Sober itself.

## Install

```bash
chmod +x install.sh
./install.sh
```

This builds Lemonyde in release mode, copies the binary + your lemon logo to
`~/.local/share/lemonyde`, adds a `lemonyde` launcher to `~/.local/bin`,
installs the logo into your icon theme, and adds a desktop entry so it shows
up (with its icon) in your app menu. It offers to install any missing dev
packages and to add the Flathub remote, but never runs `sudo` without asking.

## Build & run without installing

```bash
cargo run --release
```

## A note on the build

The `Cargo.toml` here pins `gtk4`, `libadwaita`, `glib`, and `gio` to a
known-compatible version line (gtk4 ~0.9 / libadwaita ~0.7 / glib+gio ~0.20).
If you want to move to a newer gtk4-rs release later, bump all four together
— they're versioned as a set, and mixing versions across them is the most
common cause of build errors in gtk4-rs projects.

**If `cargo build` fails for you**, please paste me the error — this was
hand-verified but not compiled end-to-end in the environment it was written
in (which was stuck on an old Rust toolchain that predates some of gtk4-rs's
current dependencies), so there's a real chance of a small fix needed for
your exact toolchain/crate versions.

## About Sober's first-run wizard

The first time each slot launches, Sober shows its own "Welcome to Sober" /
"Optional Sober Configuration" onboarding — that's Sober's own one-time
ToS/privacy acknowledgment and feature-permission setup, not something
Lemonyde adds, and there's no known flag to skip it. Click through it once
per slot; Sober should remember it and open straight into Roblox on
subsequent launches of that same slot. If a slot keeps landing back on the
wizard, its config likely isn't being saved — check that
`~/.local/share/lemonyde/instances/slot-N` is actually writable.

## Where things live

| What | Path |
|---|---|
| Sober's wrapper + FFlag config | `~/.var/app/org.vinegarhq.Sober/config/sober/config.json` |
| Roblox's own generated settings | `~/.var/app/org.vinegarhq.Sober/data/sober/exe/ClientSettings/ClientAppSettings.json` |
| Custom asset overlay | `~/.var/app/org.vinegarhq.Sober/data/sober/asset_overlay` |

## FFlags: know the limits

Since September 2025, Roblox enforces a strict **allowlist** for locally
overridden FastFlags — anything not on that list is ignored, even if it's
spelled correctly. Lemonyde's flag library only includes flags confirmed
to still be active (rendering/LOD, MSAA, texture quality, grass distance,
etc.). Don't attempt to bypass the allowlist or use cache/memory-editing
tricks — VinegarHQ and the community flag that as a ban risk under Roblox's
Hyperion anti-cheat.

## License

Do whatever you like with this bootstrapper. Sober itself is closed-source
and owned by VinegarHQ; Lemonyde does not redistribute it, only automates
`flatpak install/run/update` calls against the public Flathub package.
