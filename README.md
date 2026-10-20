<div align="center">
  
<img width="800" height="800" alt="lemonyde-logo" src="/assets/lemonydetitlealt.svg" />

</div>

<div align="center">

  [![Discord](https://img.shields.io/discord/1541849281298829353?logo=discord&logoColor=white&label=discord&color=4d3dff)](https://dc.gg/lemonyde)

</div>

# 🍹🍋 Lemonyde 
An unofficial, community-made **bootstrapper for [Mocktail](<https://mocktail.bigrat.space>)** application
that runs Roblox on Linux. Lemonyde gives Sober a native GTK4 +
libadwaita GUI for the stuff you'd otherwise do by hand in a terminal or
text editor: installing/updating/launching Sober, editing FastFlags
(FFlags), and tweaking its wrapper settings.

## keep in mind : **!!!LEMONYDE IS IN DEVELOPMENT SO EXPECT CHANGES AND BUGS!!!**


# Install

```bash
curl -fsSL https://epicneco.github.io/Lemonyde/install.sh | bash
```

this installs the app to `~/.local/bin` and updates it if needed

**Not affiliated with Roblox Corporation or bigrat.space**

## Features

- **One-click install / update / launch / uninstall** for mocktail,
  with a live streaming log so you can see exactly what Lemonyde is doing.
- **Multi-instance launch** — open several *isolated* Mocktail windows at once
  (e.g. to play on separate accounts side by side). 
  **Heads up:** Roblox's rules treat running multiple clients at the
  same time as against policy, and community reports say it can trigger an
  anti-cheat flag on your account — Lemonyde shows this warning in the app
  too, so use it at your own discretion. Note the FFlag/Settings editors
  only manage your *main* Mocktail install, not the isolated instance slots.
- **FFlag editor** — add, edit, and remove entries in the `fflags.json` with:
  - Three ready-made presets (Low-End/VRAM Fix, Balanced, Maximum Fidelity)
  - A library of flags **confirmed to be on Roblox's client-config allowlist**
    (flags outside the allowlist are silently ignored by Roblox, so Lemonyde
    doesn't bother listing unconfirmed ones)
- **Texture Overrides** dumps the textures
  

## Where things live

| What | Path |
|---|---|
| Config | `~/.config/mocktail/config.yaml` |
| Mocktail | `~/.config/mocktail/fflags.json` |
| Custom asset overlay | `~/.config/mocktail/textures` |
| Multi Instance Slots | `~/.local/share/lemonyde/instances` |

## FFlags: know the limits

Since September 2025, Roblox enforces a strict **allowlist** for locally
overridden FastFlags — anything not on that list is ignored, even if it's
spelled correctly. Lemonyde's flag library only includes flags confirmed
to still be active (rendering/LOD, MSAA, texture quality, grass distance,
etc.). Don't attempt to bypass the allowlist or use cache/memory-editing
tricks.

## License

**Do whatever you like with this bootstrapper as long as you make it FOSS.**
