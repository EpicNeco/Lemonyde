// Lemonyde — an unofficial community bootstrapper for Mocktail
// (Roblox on Linux: https://mocktail.bigrat.space/).
// Works with both install flavors: the Flathub flatpak
// (space.bigrat.mocktail) and native packages from distro repos / the AUR.
//
// Not affiliated with Roblox Corporation or the Mocktail project authors.
// Provided as-is.
//
// Rust + GTK4 + libadwaita rewrite of the original Python prototype.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::thread;

use glib::clone;
use gtk4::prelude::*;
use gtk4::{glib, gio};
use libadwaita as adw;
use adw::prelude::*;
use serde_json::{Map, Value};

const APP_ID: &str = "io.github.epicneco.Lemonyde";
const MOCKTAIL_APP_ID: &str = "space.bigrat.mocktail";

/// Flags confirmed to be on Roblox's client-config allowlist as of the
/// community-maintained guide at github.com/dyokism/sober-fastflags
/// (accurate as of July 2026 — Roblox may change the allowlist at any time).
///
/// Compiled against Sober's PC-client allowlist; Mocktail runs the Android
/// client behind a `pc-windows-11` device preset, so its allowlist may differ
/// slightly. Unlisted flags are simply ignored by Roblox, never harmful.
const KNOWN_FFLAGS: &[(&str, &str, &str)] = &[
    ("DFIntCSGLevelOfDetailSwitchingDistance", "400", "Master LOD culling distance for CSG models. Lower = better FPS."),
    ("DFIntCSGLevelOfDetailSwitchingDistanceL12", "200", "LOD distance for Graphics Quality 1-2."),
    ("DFIntCSGLevelOfDetailSwitchingDistanceL23", "350", "LOD distance for Graphics Quality 2-3."),
    ("DFIntCSGLevelOfDetailSwitchingDistanceL34", "500", "LOD distance for Graphics Quality 3-4."),
    ("FIntDebugForceMSAASamples", "2", "Forces MSAA anti-aliasing (1, 2 or 4)."),
    ("DFFlagDebugPauseVoxelizer", "true", "Pauses voxel lighting/shadows/AO. Big FPS boost."),
    ("FFlagDebugSkyGray", "false", "Replaces the skybox with flat gray to remove sky shader cost."),
    ("DFIntDebugFRMQualityLevelOverride", "10", "Overrides the graphics level slider (0-21)."),
    ("FIntFRMMaxGrassDistance", "200", "Max render distance for grass. 0 disables grass."),
    ("FIntFRMMinGrassDistance", "0", "Distance where grass starts rendering."),
    ("DFFlagTextureQualityOverrideEnabled", "true", "Enables manual control over texture resolution."),
    ("DFIntTextureQualityOverride", "2", "Texture quality: 0 (lowest) to 3 (max). 3 may crash GPUs with <=4GB VRAM."),
    ("FIntGrassMovementReducedMotionFactor", "50", "Grass sway intensity. 0 = frozen."),
];

fn presets() -> Vec<(&'static str, Map<String, Value>)> {
    let mk = |pairs: &[(&str, Value)]| -> Map<String, Value> {
        pairs.iter().cloned().map(|(k, v)| (k.to_string(), v)).collect()
    };
    vec![
        ("Low-End / VRAM Fix", mk(&[
            ("DFFlagTextureQualityOverrideEnabled", Value::Bool(true)),
            ("DFIntTextureQualityOverride", Value::from(1)),
            ("DFIntCSGLevelOfDetailSwitchingDistance", Value::from(100)),
            ("DFIntCSGLevelOfDetailSwitchingDistanceL12", Value::from(75)),
            ("DFIntCSGLevelOfDetailSwitchingDistanceL23", Value::from(100)),
            ("DFIntCSGLevelOfDetailSwitchingDistanceL34", Value::from(150)),
            ("DFFlagDebugPauseVoxelizer", Value::Bool(true)),
            ("FIntFRMMaxGrassDistance", Value::from(0)),
            ("FIntGrassMovementReducedMotionFactor", Value::from(0)),
            ("FFlagDebugSkyGray", Value::Bool(true)),
        ])),
        ("Balanced / Mid-Range", mk(&[
            ("DFFlagTextureQualityOverrideEnabled", Value::Bool(true)),
            ("DFIntTextureQualityOverride", Value::from(2)),
            ("DFIntCSGLevelOfDetailSwitchingDistance", Value::from(400)),
            ("DFIntCSGLevelOfDetailSwitchingDistanceL12", Value::from(200)),
            ("DFIntCSGLevelOfDetailSwitchingDistanceL23", Value::from(350)),
            ("DFIntCSGLevelOfDetailSwitchingDistanceL34", Value::from(500)),
            ("FIntDebugForceMSAASamples", Value::from(2)),
            ("FIntFRMMaxGrassDistance", Value::from(200)),
            ("FIntGrassMovementReducedMotionFactor", Value::from(50)),
        ])),
        ("Maximum Fidelity", mk(&[
            ("DFFlagTextureQualityOverrideEnabled", Value::Bool(true)),
            ("DFIntTextureQualityOverride", Value::from(3)),
            ("DFIntCSGLevelOfDetailSwitchingDistance", Value::from(1000)),
            ("DFIntCSGLevelOfDetailSwitchingDistanceL34", Value::from(1000)),
            ("FIntDebugForceMSAASamples", Value::from(4)),
            ("DFIntDebugFRMQualityLevelOverride", Value::from(21)),
        ])),
    ]
}

// --------------------------------------------------------------------------
// Paths & config I/O
// --------------------------------------------------------------------------

struct Paths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    /// `config/mocktail/fflags.json` — flat object Mocktail merges into
    /// Roblox's client settings on launch.
    fflags_file: PathBuf,
    /// `config/mocktail/config.yaml` — Mocktail's own settings file.
    settings_file: PathBuf,
    /// `config/mocktail/textures` — hash-named PNG texture overrides.
    texture_overrides: PathBuf,
    /// `config/mocktail/dumps` — where texture dumps land when Mocktail is
    /// launched with `MOCKTAIL_TEXTURE_DUMP_DIR` set.
    texture_dumps: PathBuf,
}

impl Paths {
    /// Locates Mocktail's config/data directories for the install flavor.
    /// Flatpak keeps everything under `~/.var/app/<id>/`; native builds follow
    /// the XDG spec (`~/.config/mocktail`, `$XDG_DATA_HOME/mocktail`).
    fn new(flavor: MocktailFlavor) -> Self {
        let (config_dir, data_dir) = match flavor {
            MocktailFlavor::Native => (xdg_config_home().join("mocktail"), xdg_data_home().join("mocktail")),
            // Flatpak, or nothing installed yet (default to the flatpak layout).
            _ => {
                let base = real_home().join(".var/app").join(MOCKTAIL_APP_ID);
                (base.join("config/mocktail"), base.join("data/mocktail"))
            }
        };
        Paths {
            fflags_file: config_dir.join("fflags.json"),
            settings_file: config_dir.join("config.yaml"),
            texture_overrides: config_dir.join("textures"),
            texture_dumps: config_dir.join("dumps"),
            config_dir,
            data_dir,
        }
    }
}

// --------------------------------------------------------------------------
// Multi-instance support
//
// Running a second Mocktail with the same $HOME makes both instances fight
// over the same config, Roblox payload, and login data. The fix is to give
// each extra instance its own fake $HOME (plus XDG_* redirection for native
// builds — see `spawn_slot_process`), so each slot gets a completely separate
// config (config/mocktail), data (data/mocktail), and login session.
//
// Note: Mocktail's own FAQ declines to support multi-instance launching;
// this is Lemonyde doing it from the outside. Roblox's ToS prohibit running
// multiple clients — the warning banner on the Home page applies.
// --------------------------------------------------------------------------

fn real_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn xdg_data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| real_home().join(".local/share"))
}

fn xdg_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| real_home().join(".config"))
}

fn xdg_cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| real_home().join(".cache"))
}

fn instances_dir() -> PathBuf {
    xdg_data_home().join("lemonyde/instances")
}

/// The isolated fake-$HOME directory for instance slot `n` (1-indexed).
/// Reused across launches so a slot's Mocktail login persists between runs.
fn slot_home(n: u32) -> PathBuf {
    instances_dir().join(format!("slot-{n}"))
}

/// Spawns Mocktail inside isolated instance slot `n`. Returns the child process
/// handle so the caller can track it (and detect when it exits) rather than
/// firing-and-forgetting — a double launch into the same slot would have both
/// processes sharing one config/data tree.
///
/// Flatpak: only HOME is faked (Flatpak resolves the app's private
/// `~/.var/app/<id>` inside the sandbox); XDG_* stay real so the `flatpak`
/// command can still find the installed app under the real user's
/// `~/.local/share/flatpak`.
///
/// Native: HOME *and* XDG_* are redirected into the slot, since a native
/// build reads `$XDG_CONFIG_HOME/mocktail` and friends directly.
fn spawn_slot_process(n: u32, flavor: MocktailFlavor) -> std::io::Result<std::process::Child> {
    let home = slot_home(n);
    std::fs::create_dir_all(&home)?;
    let mut cmd = match flavor {
        MocktailFlavor::Native => {
            let mut cmd = Command::new("mocktail");
            cmd.env("XDG_CONFIG_HOME", home.join(".config"))
                .env("XDG_DATA_HOME", home.join(".local/share"))
                .env("XDG_CACHE_HOME", home.join(".cache"));
            cmd
        }
        _ => {
            let mut cmd = Command::new("flatpak");
            cmd.env("XDG_DATA_HOME", xdg_data_home())
                .env("XDG_CONFIG_HOME", xdg_config_home())
                .env("XDG_CACHE_HOME", xdg_cache_home())
                .args(["run", MOCKTAIL_APP_ID]);
            cmd
        }
    };
    cmd.env("HOME", &home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn load_fflags(paths: &Paths) -> Map<String, Value> {
    std::fs::read_to_string(&paths.fflags_file)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn save_fflags(paths: &Paths, fflags: &Map<String, Value>) -> std::io::Result<()> {
    std::fs::create_dir_all(&paths.config_dir)?;
    if paths.fflags_file.exists() {
        let _ = std::fs::copy(&paths.fflags_file, paths.fflags_file.with_extension("json.bak"));
    }
    let text = serde_json::to_string_pretty(fflags).unwrap_or_default();
    std::fs::write(&paths.fflags_file, text + "\n")
}

// ---- Minimal line-oriented YAML editing for Mocktail's config.yaml ----
//
// Mocktail creates config.yaml once (mode 0600) and never replaces the
// user's edits, so we surgically replace individual `key: value` lines while
// leaving comments and unrelated keys untouched. Only the plain mapping
// shapes Mocktail itself writes are handled; anything unusual falls back to
// appending a fresh section at the end of the file.

/// Splits a line into `(indent, key, value-after-colon)` for plain mapping
/// entries. Comment lines, blank lines, and list items yield `None`.
fn yaml_line_key(line: &str) -> Option<(usize, &str, &str)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let indent = line.len() - trimmed.len();
    let (key, rest) = trimmed.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(' ') || key.starts_with('-') {
        return None;
    }
    Some((indent, key, rest.trim()))
}

/// Strips a trailing `# comment` and surrounding quotes from a raw value.
fn yaml_scalar(raw: &str) -> &str {
    let before_comment = match raw.find('#') {
        Some(i) if i == 0 || raw[..i].ends_with(' ') => &raw[..i],
        _ => raw,
    };
    before_comment.trim().trim_matches(|c| c == '"' || c == '\'')
}

/// Locates the line index of the final `keys` entry (e.g. `["graphics",
/// "backend"]`) in `lines`. Intermediate keys must be section headers.
fn yaml_locate(lines: &[String], keys: &[&str]) -> Option<usize> {
    let mut start = 0usize;
    let mut end = lines.len();
    let mut parent_indent: Option<usize> = None;
    for (idx, key) in keys.iter().enumerate() {
        let mut hit: Option<(usize, usize, String)> = None;
        let mut i = start;
        while i < end {
            if let Some((indent, k, rest)) = yaml_line_key(&lines[i]) {
                if let Some(pindent) = parent_indent {
                    if indent <= pindent {
                        break;
                    }
                }
                if k == *key {
                    hit = Some((i, indent, rest.to_string()));
                    break;
                }
            }
            i += 1;
        }
        let (line, indent, rest) = hit?;
        let is_last = idx + 1 == keys.len();
        if is_last {
            return Some(line);
        }
        // Intermediate keys must be section headers (`key:` with no value).
        if !yaml_scalar(&rest).is_empty() {
            return None;
        }
        parent_indent = Some(indent);
        start = line + 1;
        let mut j = start;
        while j < end {
            if let Some((ind2, _, _)) = yaml_line_key(&lines[j]) {
                if ind2 <= indent {
                    break;
                }
            }
            j += 1;
        }
        end = j;
    }
    None
}

fn yaml_get(text: &str, keys: &[&str]) -> Option<String> {
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let line = yaml_locate(&lines, keys)?;
    let (_, _, rest) = yaml_line_key(&lines[line])?;
    let value = yaml_scalar(rest);
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn yaml_set(text: &str, keys: &[&str], value: &str) -> String {
    if keys.is_empty() {
        return text.to_string();
    }
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let last_key = keys.last().copied().unwrap_or("");

    match yaml_locate(&lines, keys) {
        Some(line) => {
            // Pull the info we need out of the line before mutating `lines`.
            let (indent, is_section) = match yaml_line_key(&lines[line]) {
                Some((indent, _, rest)) => (indent, yaml_scalar(rest).is_empty()),
                None => (0, false),
            };
            lines[line] = format!("{}{}: {}", " ".repeat(indent), last_key, value);
            if is_section {
                // The key existed as a section (e.g. Mocktail's detailed
                // `device:` map) — drop its children so we don't leave a
                // half-orphaned mapping below the new scalar value.
                let j = line + 1;
                while j < lines.len() {
                    match yaml_line_key(&lines[j]) {
                        Some((ind, _, _)) if ind > indent => {
                            lines.remove(j);
                        }
                        _ => break,
                    }
                }
            }
        }
        None => {
            // Deepest existing prefix of the key path, if any.
            let mut prefix = keys.len() - 1;
            while prefix > 0 && yaml_locate(&lines, &keys[..prefix]).is_none() {
                prefix -= 1;
            }
            if prefix == 0 {
                // No existing ancestor — append the whole path as a new section.
                while lines.last().is_some_and(|l| l.trim().is_empty()) {
                    lines.pop();
                }
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                for (i, key) in keys.iter().enumerate() {
                    let indent = i * 2;
                    let line = if i + 1 == keys.len() {
                        format!("{}{}: {}", " ".repeat(indent), key, value)
                    } else {
                        format!("{}{}:", " ".repeat(indent), key)
                    };
                    lines.push(line);
                }
            } else {
                // Parent section exists — nest the remaining keys inside it.
                let parent_line = yaml_locate(&lines, &keys[..prefix]).unwrap();
                let parent_indent = yaml_line_key(&lines[parent_line]).map(|(i, _, _)| i).unwrap_or(0);
                let mut section_end = parent_line + 1;
                while section_end < lines.len() {
                    if let Some((ind, _, _)) = yaml_line_key(&lines[section_end]) {
                        if ind <= parent_indent {
                            break;
                        }
                    }
                    section_end += 1;
                }
                let child_indent = (parent_line + 1..section_end)
                    .find_map(|i| yaml_line_key(&lines[i]).map(|(ind, _, _)| ind))
                    .filter(|ind| *ind > parent_indent)
                    .unwrap_or(parent_indent + 2);
                let mut insert = Vec::new();
                for (i, key) in keys[prefix..].iter().enumerate() {
                    let indent = child_indent + i * 2;
                    if prefix + i + 1 == keys.len() {
                        insert.push(format!("{}{}: {}", " ".repeat(indent), key, value));
                    } else {
                        insert.push(format!("{}{}:", " ".repeat(indent), key));
                    }
                }
                for (offset, line) in insert.into_iter().enumerate() {
                    lines.insert(section_end + offset, line);
                }
            }
        }
    }
    lines.join("\n") + "\n"
}

fn coerce_value(text: &str) -> Value {
    let t = text.trim();
    match t.to_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        return Value::from(i);
    }
    // No float parsing: Mocktail only accepts string/bool/int values in
    // fflags.json — a single float rejects the entire file at load time.
    Value::String(t.to_string())
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// --------------------------------------------------------------------------
// Mocktail install-method detection & helpers
// --------------------------------------------------------------------------

/// Which Mocktail build the user actually has (or would end up with).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MocktailFlavor {
    /// Sandboxed Flathub build (`space.bigrat.mocktail`).
    Flatpak,
    /// Native package from a distro repo / AUR — a `mocktail` binary on $PATH.
    Native,
    /// Not installed (yet).
    None,
}

fn which_cmd(name: &str) -> bool {
    Command::new("which").arg(name).output().map(|o| o.status.success()).unwrap_or(false)
}

fn flatpak_present() -> bool {
    which_cmd("flatpak")
}

fn mocktail_installed() -> bool {
    Command::new("flatpak")
        .args(["info", MOCKTAIL_APP_ID])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Detects how Mocktail is installed: Flatpak first (it's the
/// upstream-recommended route and wins if both are present), then a native
/// `mocktail` binary from a distro repo/AUR package.
fn detect_mocktail_flavor() -> MocktailFlavor {
    if mocktail_installed() {
        MocktailFlavor::Flatpak
    } else if which_cmd("mocktail") {
        MocktailFlavor::Native
    } else {
        MocktailFlavor::None
    }
}

fn mocktail_version(flavor: MocktailFlavor) -> Option<String> {
    match flavor {
        MocktailFlavor::Flatpak => {
            let out = Command::new("flatpak").args(["info", MOCKTAIL_APP_ID]).output().ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("Version:") {
                    return Some(rest.trim().to_string());
                }
            }
            None
        }
        MocktailFlavor::Native => {
            let out = Command::new("mocktail").arg("--version").output().ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
        }
        MocktailFlavor::None => None,
    }
}

/// Counts how many Mocktail processes are currently running. Flatpak builds
/// are counted via `flatpak ps` (each `flatpak run` is its own sandbox
/// instance, which is also how we confirm a multi-instance launch took);
/// native builds are counted by process name.
fn running_instance_count(flavor: MocktailFlavor) -> usize {
    match flavor {
        MocktailFlavor::Flatpak => {
            Command::new("flatpak")
                .args(["ps", "--columns=application"])
                .output()
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter(|l| l.trim() == MOCKTAIL_APP_ID)
                        .count()
                })
                .unwrap_or(0)
        }
        MocktailFlavor::Native => Command::new("ps")
            .args(["-eo", "comm="])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| l.trim() == "mocktail")
                    .count()
            })
            .unwrap_or(0),
        MocktailFlavor::None => 0,
    }
}

/// argv for starting Mocktail under `flavor`, optionally with
/// `MOCKTAIL_TEXTURE_DUMP_DIR` set. Flatpak takes the variable as an
/// `--env=` flag; the native binary gets it through the `env` command so
/// `run_streaming` doesn't need per-spawn environment plumbing.
fn launch_argv(flavor: MocktailFlavor, texture_dump_dir: Option<&std::path::Path>) -> Vec<String> {
    match (flavor, texture_dump_dir) {
        (MocktailFlavor::Native, Some(dir)) => vec![
            "env".into(),
            format!("MOCKTAIL_TEXTURE_DUMP_DIR={}", dir.display()),
            "mocktail".into(),
        ],
        (MocktailFlavor::Native, None) => vec!["mocktail".into()],
        (_, Some(dir)) => vec![
            "flatpak".into(),
            "run".into(),
            format!("--env=MOCKTAIL_TEXTURE_DUMP_DIR={}", dir.display()),
            MOCKTAIL_APP_ID.into(),
        ],
        (_, None) => vec!["flatpak".into(), "run".into(), MOCKTAIL_APP_ID.into()],
    }
}

/// Package-manager operations against the native `mocktail` package.
#[derive(Clone, Copy)]
enum PkgOp {
    Install,
    Update,
    Remove,
}

/// Builds an argv for installing/updating/removing the native `mocktail`
/// package through whatever the user's distro uses. AUR helpers are checked
/// first (Mocktail lives in the AUR on Arch-likes) and escalate to root
/// themselves; other package managers get a sudo/doas prefix. Returns `None`
/// when nothing suitable is available.
fn native_pkg_argv(op: PkgOp) -> Option<Vec<String>> {
    const PKG: &str = "mocktail";

    for helper in ["paru", "yay"] {
        if which_cmd(helper) {
            let mut argv = vec![helper.to_string()];
            let flags: &[&str] = match op {
                PkgOp::Install => &["-S", "--needed", "--noconfirm"],
                PkgOp::Update => &["-Syu", "--noconfirm"],
                PkgOp::Remove => &["-Rns"],
            };
            argv.extend(flags.iter().map(|s| s.to_string()));
            argv.push(PKG.into());
            return Some(argv);
        }
    }

    let root = ["sudo", "doas", "sudo-rs"].into_iter().find(|c| which_cmd(c))?.to_string();
    let mut argv = vec![root];

    if which_cmd("dnf") || which_cmd("yum") {
        argv.push(if which_cmd("dnf") { "dnf" } else { "yum" }.into());
        argv.push(
            match op {
                PkgOp::Install => "install",
                PkgOp::Update => "upgrade",
                PkgOp::Remove => "remove",
            }
            .into(),
        );
        argv.extend(["-y", PKG].map(String::from));
    } else if which_cmd("pacman") {
        argv.push("pacman".into());
        let flags: &[&str] = match op {
            PkgOp::Install => &["-S", "--needed", "--noconfirm", PKG],
            PkgOp::Update => &["-Syu", "--noconfirm", PKG],
            PkgOp::Remove => &["-Rns", "--noconfirm", PKG],
        };
        argv.extend(flags.iter().map(|s| s.to_string()));
    } else if which_cmd("zypper") {
        argv.extend(
            [
                "zypper",
                "--non-interactive",
                match op {
                    PkgOp::Install => "install",
                    PkgOp::Update => "update",
                    PkgOp::Remove => "remove",
                },
                PKG,
            ]
            .map(String::from),
        );
    } else if which_cmd("apt-get") {
        argv.push("apt-get".into());
        argv.push("install".into());
        argv.push("-y".into());
        if matches!(op, PkgOp::Update) {
            argv.push("--only-upgrade".into());
        }
        argv.push(PKG.into());
    } else if which_cmd("apk") {
        argv.push("apk".into());
        argv.push(
            match op {
                PkgOp::Install => "add",
                PkgOp::Update => "upgrade",
                PkgOp::Remove => "del",
            }
            .into(),
        );
        argv.push(PKG.into());
    } else if which_cmd("xbps-install") || which_cmd("xbps-remove") {
        match op {
            PkgOp::Remove => {
                if !which_cmd("xbps-remove") {
                    return None;
                }
                argv.extend(["xbps-remove", "-R", PKG].map(String::from));
            }
            _ => {
                if !which_cmd("xbps-install") {
                    return None;
                }
                argv.push("xbps-install".into());
                argv.extend(if matches!(op, PkgOp::Install) { ["-Sy", PKG] } else { ["-Syu", PKG] }.map(String::from));
            }
        }
    } else {
        return None;
    }
    Some(argv)
}

enum StreamMsg {
    Line(String),
    Done(i32),
}

/// Runs `argv` in a background thread, streaming combined stdout/stderr
/// lines back to the GTK main thread via `on_line`, then `on_done(code)`.
fn run_streaming(
    argv: Vec<String>,
    on_line: impl Fn(String) + 'static,
    on_done: impl FnOnce(i32) + 'static,
) {
    let (sender, receiver) = async_channel::unbounded::<StreamMsg>();

    thread::spawn(move || {
        if argv.is_empty() {
            let _ = sender.send_blocking(StreamMsg::Done(-1));
            return;
        }
        let child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = sender.send_blocking(StreamMsg::Line(format!("Failed to start {}: {e}", argv[0])));
                let _ = sender.send_blocking(StreamMsg::Done(127));
                return;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let s1 = sender.clone();
        let h1 = stdout.map(|out| {
            thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if s1.send_blocking(StreamMsg::Line(line)).is_err() {
                        break;
                    }
                }
            })
        });
        let s2 = sender.clone();
        let h2 = stderr.map(|err| {
            thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if s2.send_blocking(StreamMsg::Line(line)).is_err() {
                        break;
                    }
                }
            })
        });
        if let Some(h) = h1 {
            let _ = h.join();
        }
        if let Some(h) = h2 {
            let _ = h.join();
        }
        let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = sender.send_blocking(StreamMsg::Done(code));
    });

    glib::spawn_future_local(async move {
        while let Ok(msg) = receiver.recv().await {
            match msg {
                StreamMsg::Line(l) => on_line(l),
                StreamMsg::Done(code) => {
                    on_done(code);
                    break;
                }
            }
        }
    });
}

// --------------------------------------------------------------------------
// Shared UI state
// --------------------------------------------------------------------------

struct AppState {
    paths: Paths,
    fflags: Map<String, Value>,
    /// How Mocktail is installed (Flatpak vs. native package), detected at
    /// startup. Launch/install/update/uninstall all branch on this.
    flavor: MocktailFlavor,
}

type SharedState = Rc<RefCell<AppState>>;

fn asset_dir() -> PathBuf {
    // Look next to the executable first (installed layout), then fall back
    // to the source tree's assets/ for `cargo run`.
    // The manifest lives in src/ (src/Cargo.toml), so the tree's assets/
    // are one level up from CARGO_MANIFEST_DIR; a root Cargo.toml layout
    // (assets next to the manifest) is also supported.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("assets/lemonyde.svg");
            if candidate.exists() {
                return dir.join("assets");
            }
        }
    }
    // Installed via install.sh: ~/.local/share/lemonyde/assets/lemonyde.svg
    // (i.e. $XDG_DATA_HOME/lemonyde/assets). Checked explicitly so the icon
    // and other assets resolve even if the binary is launched through a
    // symlink/wrapper in a different directory.
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")));
    if let Some(data_home) = data_home {
        let candidate = data_home.join("lemonyde/assets/lemonyde.svg");
        if candidate.exists() {
            return data_home.join("lemonyde/assets");
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sibling = manifest.join("../assets");
    if sibling.join("lemonyde.svg").exists() {
        return sibling;
    }
    manifest.join("assets")
}

fn logo_path() -> PathBuf {
    asset_dir().join("lemonyde.svg")
}

/// Icon name (without extension) of the proper app SVG:
/// `assets/lemonyde.svg` — installed as
/// `~/.local/share/lemonyde/assets/lemonyde.svg` and as
/// `~/.local/share/icons/hicolor/scalable/apps/lemonyde.svg`.
const APP_ICON_NAME_FALLBACK: &str = "lemonyde";

/// Points this window (and, via the default, all future windows/dialogs) at
/// the proper `lemonyde.svg` app icon. Without this the window falls back to
/// a generic icon in the dock/taskbar/Alt-Tab switcher.
///
/// The file itself is found through `logo_path()` (which covers the installed
/// `~/.local/share/lemonyde/assets/lemonyde.svg` layout as well as `cargo run`
/// from the source tree). Its parent dir is added to the icon theme search
/// path so the `"lemonyde"` icon name resolves to that exact SVG even when no
/// hicolor-installed copy exists. `APP_ID` is preferred when the theme already
/// provides an icon under the app-id name (proper Wayland compositor
/// matching); otherwise we use the SVG's own stem.
fn setup_app_icon(window: &adw::ApplicationWindow) {
    let display = WidgetExt::display(window);
    let icon_theme = gtk4::IconTheme::for_display(&display);

    let icon_file = logo_path();
    if icon_file.exists() {
        if let Some(dir) = icon_file.parent() {
            icon_theme.add_search_path(dir);
        }
    }

    let name = if icon_theme.has_icon(APP_ID) {
        APP_ID
    } else {
        // "lemonyde" resolves to assets/lemonyde.svg via the search path
        // above, or to the hicolor-installed copy when present.
        APP_ICON_NAME_FALLBACK
    };
    if icon_theme.has_icon(name) {
        window.set_icon_name(Some(name));
        gtk4::Window::set_default_icon_name(name);
    }
}

fn title_logo_path() -> PathBuf {
    asset_dir().join("lemonyde-title.svg")
}

/// Builds a "Lemonyde" wordmark Picture with a yellow-to-green gradient
/// fill baked into the SVG (GTK CSS can't gradient-fill text directly).
/// Falls back to a plain label if the SVG can't be loaded for some reason.
fn gradient_title_widget() -> gtk4::Widget {
    let path = title_logo_path();
    if path.exists() {
        let picture = gtk4::Picture::for_filename(&path);
        picture.set_content_fit(gtk4::ContentFit::Contain);
        picture.set_halign(gtk4::Align::Center);
        picture.set_size_request(220, 46);
        picture.set_can_shrink(true);
        picture.upcast()
    } else {
        gtk4::Label::builder().label("Lemonyde").css_classes(["title-1"]).build().upcast()
    }
}

/// Green-yellow gradient used for section headers throughout the app
/// (distinct from the pure-green wordmark above).
const GRADIENT_GREEN_YELLOW: &[(&str, &str)] = &[("0%", "#fff066"), ("55%", "#9be23f"), ("100%", "#2fae4a")];

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

/// Renders `text` as a small gradient-filled SVG (cached to a temp file by a
/// slug of the text+size so repeated calls for the same header reuse the
/// same file) and returns it as a left-aligned widget sized like a heading.
/// Falls back to a plain ".heading" label if writing/loading the SVG fails
/// for any reason — headers should never disappear just because a temp
/// directory wasn't writable.
fn gradient_text_widget(text: &str, font_size: u32, stops: &[(&str, &str)]) -> gtk4::Widget {
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let width = ((text.chars().count() as f32) * (font_size as f32) * 0.60 + 20.0).max(40.0) as u32;
    let height = ((font_size as f32) * 1.4) as u32;
    let y = (height as f32) * 0.74;

    let mut stops_svg = String::new();
    for (offset, color) in stops {
        stops_svg.push_str(&format!(r#"<stop offset="{offset}" stop-color="{color}"/>"#));
    }
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}"><defs><linearGradient id="g" x1="0%" y1="0%" x2="100%" y2="100%">{stops_svg}</linearGradient></defs><text x="2" y="{y:.0}" font-family="Sans, 'DejaVu Sans', sans-serif" font-weight="800" font-size="{font_size}" fill="url(#g)">{escaped}</text></svg>"#
    );

    let cache_id = format!("{}-{font_size}", slugify(text));
    let dir = std::env::temp_dir().join("lemonyde-gradient-text");
    let path = dir.join(format!("{cache_id}.svg"));
    let wrote_ok = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &svg)).is_ok();

    if wrote_ok {
        let picture = gtk4::Picture::for_filename(&path);
        picture.set_content_fit(gtk4::ContentFit::Contain);
        picture.set_halign(gtk4::Align::Start);
        picture.set_size_request((width / 2) as i32, (height / 2) as i32);
        picture.set_can_shrink(true);
        picture.upcast()
    } else {
        gtk4::Label::builder().label(text).css_classes(["heading"]).xalign(0.0).build().upcast()
    }
}

// --------------------------------------------------------------------------
// Small widget helpers
// --------------------------------------------------------------------------

fn page_scroller(child: &impl IsA<gtk4::Widget>) -> gtk4::ScrolledWindow {
    let clamp = adw::Clamp::builder()
        .maximum_size(760)
        .tightening_threshold(560)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(16)
        .margin_end(16)
        .child(child)
        .build();
    gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .child(&clamp)
        .build()
}

fn confirm_dialog(
    parent: &impl IsA<gtk4::Widget>,
    heading: &str,
    body: &str,
    ok_label: &str,
    destructive: bool,
    on_ok: impl Fn() + 'static,
) {
    let dialog = adw::AlertDialog::builder().heading(heading).body(body).build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", ok_label);
    dialog.set_response_appearance(
        "ok",
        if destructive { adw::ResponseAppearance::Destructive } else { adw::ResponseAppearance::Suggested },
    );
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_d, response| {
        if response == "ok" {
            on_ok();
        }
    });
    dialog.present(Some(parent));
}

fn log_line(view: &gtk4::TextView, text: &str) {
    let buf = view.buffer();
    let mut end = buf.end_iter();
    buf.insert(&mut end, &format!("{text}\n"));
    let mark = buf.create_mark(None, &buf.end_iter(), false);
    view.scroll_mark_onscreen(&mark);
}

// --------------------------------------------------------------------------
// main
// --------------------------------------------------------------------------

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    // Detect the install flavor once up front — every page (paths, launch
    // commands, install/update/uninstall) branches on it. Detection is a
    // couple of quick subprocess checks (`flatpak info`, `which mocktail`).
    let flavor = detect_mocktail_flavor();
    let state: SharedState = Rc::new(RefCell::new(AppState {
        paths: Paths::new(flavor),
        fflags: Map::new(),
        flavor,
    }));
    {
        let mut s = state.borrow_mut();
        s.fflags = load_fflags(&s.paths);
    }

    let toast_overlay = adw::ToastOverlay::new();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Lemonyde")
        .default_width(880)
        .default_height(640)
        .content(&toast_overlay)
        .build();
    window.add_css_class("lemonyde-window");

    // Use the proper assets/lemonyde.svg file for the window/app icon
    // (dock, taskbar, Alt-Tab) instead of a generic fallback.
    setup_app_icon(&window);

    // CSS: dark grey background, yellow text everywhere.
    let css = gtk4::CssProvider::new();
    css.load_from_data(include_str!("../assets/style.css"));
    gtk4::style_context_add_provider_for_display(
        &WidgetExt::display(&window),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let toolbar_view = adw::ToolbarView::new();
    toast_overlay.set_child(Some(&toolbar_view));

    let view_stack = adw::ViewStack::new();

    let switcher = adw::ViewSwitcher::builder()
        .stack(&view_stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let header = adw::HeaderBar::builder().title_widget(&switcher).build();
    toolbar_view.add_top_bar(&header);

    let switcher_bar = adw::ViewSwitcherBar::builder().stack(&view_stack).build();
    toolbar_view.add_bottom_bar(&switcher_bar);

    // libadwaita 1.4 replacement for the deprecated AdwViewSwitcherTitle:
    // a wide ViewSwitcher in the header, plus a breakpoint that removes it
    // and reveals the bottom switcher bar on narrow windows. The window
    // title ("Lemonyde") still shows in the header bar when the switcher
    // is unset.
    if let Ok(condition) = adw::BreakpointCondition::parse("max-width: 550sp") {
        let breakpoint = adw::Breakpoint::new(condition);
        breakpoint.add_setter(&header, "title-widget", None);
        breakpoint.add_setter(&switcher_bar, "reveal", Some(&true.to_value()));
        window.add_breakpoint(breakpoint);
    }

    toolbar_view.set_content(Some(&view_stack));

    let toast = Rc::new({
        let overlay = toast_overlay.clone();
        move |msg: &str| {
            overlay.add_toast(adw::Toast::builder().title(msg).timeout(3).build());
        }
    });

    build_home_page(&view_stack, window.clone(), state.clone(), toast.clone());
    build_fflags_page(&view_stack, window.clone(), state.clone(), toast.clone());
    build_textures_page(&view_stack, window.clone(), state.clone(), toast.clone());
    build_settings_page(&view_stack, window.clone(), state.clone(), toast.clone());
    build_advanced_page(&view_stack, window.clone(), state.clone(), toast.clone());
    build_about_page(&view_stack);

    window.present();
}

// --------------------------------------------------------------------------
// HOME
// --------------------------------------------------------------------------

fn build_home_page(
    stack: &adw::ViewStack,
    window: adw::ApplicationWindow,
    state: SharedState,
    toast: Rc<dyn Fn(&str)>,
) {
    let root = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(18).build();

    let logo_frame = gtk4::Box::builder().halign(gtk4::Align::Center).css_classes(["lemonyde-logo-frame"]).build();
    let picture = gtk4::Picture::for_filename(logo_path());
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_size_request(120, 120);
    logo_frame.append(&picture);
    root.append(&logo_frame);

    root.append(&gradient_title_widget());
    root.append(
        &gtk4::Label::builder()
            .label("A community bootstrapper for Mocktail (Roblox on Linux)")
            .css_classes(["dim-label"])
            .build(),
    );

    let status_group = adw::PreferencesGroup::new();
    let status_icon = gtk4::Image::from_icon_name("content-loading-symbolic");
    let status_row = adw::ActionRow::builder().title("Checking Mocktail installation…").build();
    status_row.add_prefix(&status_icon);
    status_group.add(&status_row);
    root.append(&status_group);

    let actions = gtk4::Box::builder().spacing(10).halign(gtk4::Align::Center).build();
    let btn_launch = gtk4::Button::builder().label("Launch Mocktail").css_classes(["suggested-action", "pill"]).build();
    let btn_install = gtk4::Button::builder().label("Install Mocktail").css_classes(["suggested-action", "pill"]).build();
    let btn_update = gtk4::Button::builder().label("Update").css_classes(["pill"]).build();
    let btn_uninstall = gtk4::Button::builder().label("Uninstall").css_classes(["destructive-action", "pill"]).build();
    for b in [&btn_launch, &btn_install, &btn_update, &btn_uninstall] {
        actions.append(b);
    }
    root.append(&actions);

    let multi_banner = adw::Banner::builder()
        .title("Running multiple instances goes against Roblox's rules and risks an anti-cheat flag on your account. Use at your own risk.")
        .revealed(true)
        .build();
    root.append(&multi_banner);

    let multi_group = adw::PreferencesGroup::builder()
        .description("Each numbered slot gets its own isolated Mocktail config, data & login folder, so instances never share state. Sign in once per slot and it's remembered next time.")
        .build();

    root.append(&gradient_text_widget("Multi-Instance Launch", 18, GRADIENT_GREEN_YELLOW));
    let instances_row = adw::ActionRow::builder()
        .title("Instances to launch")
        .subtitle("Slot 1, Slot 2, … — separate accounts, separate windows")
        .build();
    let instances_spin = gtk4::SpinButton::with_range(1.0, 6.0, 1.0);
    instances_spin.set_value(1.0);
    instances_spin.set_valign(gtk4::Align::Center);
    instances_row.add_suffix(&instances_spin);
    multi_group.add(&instances_row);

    let running_row = adw::ActionRow::builder().title("Currently running").subtitle("—").build();
    let refresh_running_btn = gtk4::Button::builder()
        .icon_name("view-refresh-symbolic")
        .css_classes(["flat"])
        .valign(gtk4::Align::Center)
        .tooltip_text("Refresh")
        .build();
    running_row.add_suffix(&refresh_running_btn);
    multi_group.add(&running_row);
    root.append(&multi_group);

    let launch_multi_btn = gtk4::Button::builder()
        .label("Launch Instances")
        .css_classes(["suggested-action", "pill"])
        .halign(gtk4::Align::Center)
        .build();
    root.append(&launch_multi_btn);

    root.append(&gradient_text_widget("Activity Log", 18, GRADIENT_GREEN_YELLOW));
    let log_group = adw::PreferencesGroup::builder().build();
    let log_scroll = gtk4::ScrolledWindow::builder().min_content_height(140).css_classes(["lemonyde-card"]).build();
    let home_log = gtk4::TextView::builder().editable(false).cursor_visible(false).monospace(true).css_classes(["lemonyde-log"]).build();
    log_scroll.set_child(Some(&home_log));
    log_group.add(&log_scroll);
    root.append(&log_group);

    root.append(&gradient_text_widget("Quick Actions", 18, GRADIENT_GREEN_YELLOW));
    let quick = adw::PreferencesGroup::builder().build();
    let row_ff = adw::ActionRow::builder().title("FastFlag Editor").subtitle("Tune graphics, LOD & stability flags").activatable(true).build();
    row_ff.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
    quick.add(&row_ff);
    let row_set = adw::ActionRow::builder().title("Wrapper Settings").subtitle("Graphics backend, HiDPI, device preset").activatable(true).build();
    row_set.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
    quick.add(&row_set);
    root.append(&quick);

    stack.add_titled_with_icon(&page_scroller(&root), Some("home"), "Home", "go-home-symbolic");

    {
        let stack = stack.clone();
        row_ff.connect_activated(move |_| stack.set_visible_child_name("fflags"));
    }
    {
        let stack = stack.clone();
        row_set.connect_activated(move |_| stack.set_visible_child_name("settings"));
    }

    let buttons = (btn_launch.clone(), btn_install.clone(), btn_update.clone(), btn_uninstall.clone());

    let refresh_status: Rc<dyn Fn()> = {
        let status_row = status_row.clone();
        let status_icon = status_icon.clone();
        let btn_launch = btn_launch.clone();
        let btn_install = btn_install.clone();
        let btn_update = btn_update.clone();
        let btn_uninstall = btn_uninstall.clone();
        let state = state.clone();
        Rc::new(move || {
            status_row.set_title("Checking Mocktail installation…");
            status_icon.set_icon_name(Some("content-loading-symbolic"));

            let (sender, receiver) = async_channel::bounded::<(MocktailFlavor, Option<String>)>(1);
            thread::spawn(move || {
                let flavor = detect_mocktail_flavor();
                let version = mocktail_version(flavor);
                let _ = sender.send_blocking((flavor, version));
            });

            // Manual clones (rather than the `clone!` macro) so this works
            // the same across glib macro syntax versions.
            let status_row = status_row.clone();
            let status_icon = status_icon.clone();
            let btn_launch = btn_launch.clone();
            let btn_install = btn_install.clone();
            let btn_update = btn_update.clone();
            let btn_uninstall = btn_uninstall.clone();
            let state = state.clone();
            glib::spawn_future_local(async move {
                if let Ok((flavor, version)) = receiver.recv().await {
                    // Keep the shared state in sync — the user may have
                    // installed/uninstalled between refreshes.
                    state.borrow_mut().flavor = flavor;
                    match flavor {
                        MocktailFlavor::Flatpak => {
                            status_row.set_title("Mocktail is installed (Flatpak)");
                            status_row.set_subtitle(&version.map(|v| format!("Version {v}")).unwrap_or_else(|| "Ready to launch".into()));
                            status_icon.set_icon_name(Some("emblem-ok-symbolic"));
                            btn_launch.set_visible(true);
                            btn_install.set_visible(false);
                            btn_update.set_sensitive(true);
                            btn_uninstall.set_sensitive(true);
                        }
                        MocktailFlavor::Native => {
                            status_row.set_title("Mocktail is installed (native package)");
                            status_row.set_subtitle(&version.map(|v| format!("Version {v}")).unwrap_or_else(|| "Ready to launch".into()));
                            status_icon.set_icon_name(Some("emblem-ok-symbolic"));
                            btn_launch.set_visible(true);
                            btn_install.set_visible(false);
                            btn_update.set_sensitive(true);
                            btn_uninstall.set_sensitive(true);
                        }
                        MocktailFlavor::None => {
                            if flatpak_present() {
                                status_row.set_title("Mocktail is not installed");
                                status_row.set_subtitle("Install it from Flathub to get started");
                            } else {
                                status_row.set_title("Mocktail is not installed");
                                status_row.set_subtitle("Install it via Flatpak or your distro's package manager");
                            }
                            status_icon.set_icon_name(Some("dialog-information-symbolic"));
                            btn_launch.set_visible(false);
                            btn_install.set_visible(true);
                            btn_update.set_sensitive(false);
                            btn_uninstall.set_sensitive(false);
                        }
                    }
                }
            });
        })
    };

    refresh_status();

    let refresh_running: Rc<dyn Fn()> = {
        let running_row = running_row.clone();
        let state = state.clone();
        Rc::new(move || {
            running_row.set_subtitle("Checking…");
            let flavor = state.borrow().flavor;
            let (sender, receiver) = async_channel::bounded::<usize>(1);
            thread::spawn(move || {
                let _ = sender.send_blocking(running_instance_count(flavor));
            });
            let running_row = running_row.clone();
            glib::spawn_future_local(async move {
                if let Ok(count) = receiver.recv().await {
                    let label = match count {
                        0 => "None running".to_string(),
                        1 => "1 instance running".to_string(),
                        n => format!("{n} instances running"),
                    };
                    running_row.set_subtitle(&label);
                }
            });
        })
    };
    refresh_running();

    refresh_running_btn.connect_clicked(clone!(#[strong] refresh_running, move |_| {
        refresh_running();
    }));

    // Tracks which slots we've personally launched and not yet seen exit, so
    // a click on "Launch Instances" never spawns a second process into a
    // slot that's already running — a double launch would have both copies
    // of Mocktail fighting over one config/data tree.
    let active_slots: Rc<RefCell<HashSet<u32>>> = Rc::new(RefCell::new(HashSet::new()));
    let (exit_tx, exit_rx) = async_channel::unbounded::<u32>();

    glib::spawn_future_local(clone!(#[strong] active_slots, #[strong] home_log, #[strong] refresh_running, async move {
        while let Ok(n) = exit_rx.recv().await {
            active_slots.borrow_mut().remove(&n);
            log_line(&home_log, &format!("Slot {n} closed — free to relaunch"));
            refresh_running();
        }
    }));

    fn launch_step(
        i: u32,
        n: u32,
        flavor: MocktailFlavor,
        home_log: gtk4::TextView,
        active_slots: Rc<RefCell<HashSet<u32>>>,
        exit_tx: async_channel::Sender<u32>,
        refresh_running: Rc<dyn Fn()>,
    ) {
        if i > n {
            return;
        }
        let already_running = active_slots.borrow().contains(&i);
        if already_running {
            log_line(&home_log, &format!("Slot {i} is already running — close it before relaunching, skipping"));
        } else {
            match spawn_slot_process(i, flavor) {
                Ok(child) => {
                    active_slots.borrow_mut().insert(i);
                    log_line(&home_log, &format!("Slot {i} launched — complete its one-time sign-in/setup, it'll go straight to Roblox next time"));
                    let exit_tx = exit_tx.clone();
                    thread::spawn(move || {
                        let mut child = child;
                        let _ = child.wait();
                        let _ = exit_tx.send_blocking(i);
                    });
                }
                Err(e) => {
                    log_line(&home_log, &format!("Slot {i} failed to launch: {e}"));
                }
            }
        }
        if i < n {
            let home_log = home_log.clone();
            let active_slots = active_slots.clone();
            let exit_tx = exit_tx.clone();
            let refresh_running = refresh_running.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
                launch_step(i + 1, n, flavor, home_log, active_slots, exit_tx, refresh_running);
            });
        } else {
            let refresh_running = refresh_running.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
                refresh_running();
            });
        }
    }

    launch_multi_btn.connect_clicked(clone!(#[strong] toast, #[strong] home_log, #[strong] instances_spin, #[strong] refresh_running, #[strong] active_slots, #[strong] exit_tx, #[strong] state, move |_| {
        let n = instances_spin.value() as u32;
        if n == 0 {
            return;
        }
        log_line(&home_log, &format!("$ launching up to {n} Mocktail instance(s)…"));
        toast(&format!("Launching up to {n} instance(s)…"));
        launch_step(1, n, state.borrow().flavor, home_log.clone(), active_slots.clone(), exit_tx.clone(), refresh_running.clone());
    }));

    let set_buttons_sensitive = {
        let (a, b, c, d) = buttons.clone();
        move |sensitive: bool| {
            for w in [&a, &b, &c, &d] {
                w.set_sensitive(sensitive);
            }
        }
    };

    btn_launch.connect_clicked(clone!(#[strong] toast, #[strong] state, move |_| {
        let flavor = state.borrow().flavor;
        let argv = launch_argv(flavor, None);
        let spawned = Command::new(&argv[0])
            .args(&argv[1..])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => toast("Launching Mocktail…"),
            Err(e) => toast(&format!("Couldn't launch Mocktail: {e}")),
        }
    }));

    btn_install.connect_clicked(clone!(#[strong] home_log, #[strong] toast, #[strong] refresh_status, #[strong] set_buttons_sensitive, #[strong] state, move |_| {
        let flavor = state.borrow().flavor;
        // Not installed: use whichever route is available — native package
        // first when flatpak isn't even present, else the Flathub flatpak.
        let argv = match native_pkg_argv(PkgOp::Install) {
            Some(argv) if (flavor == MocktailFlavor::Native || !flatpak_present()) => argv,
            _ => vec!["flatpak".into(), "install".into(), "--user".into(), "-y".into(), "flathub".into(), MOCKTAIL_APP_ID.into()],
        };
        set_buttons_sensitive(false);
        log_line(&home_log, &format!("$ {}", argv.join(" ")));
        run_streaming(argv,
            clone!(#[strong] home_log, move |l| log_line(&home_log, &l)),
            clone!(#[strong] toast, #[strong] refresh_status, #[strong] set_buttons_sensitive, move |code| {
                set_buttons_sensitive(true);
                toast(if code == 0 { "Mocktail installed" } else { "Install failed — see activity log" });
                refresh_status();
            }),
        );
    }));

    btn_update.connect_clicked(clone!(#[strong] home_log, #[strong] toast, #[strong] refresh_status, #[strong] set_buttons_sensitive, #[strong] state, move |_| {
        let flavor = state.borrow().flavor;
        set_buttons_sensitive(false);
        let argv = match flavor {
            MocktailFlavor::Native => match native_pkg_argv(PkgOp::Update) {
                Some(argv) => argv,
                None => {
                    set_buttons_sensitive(true);
                    toast("No supported package manager found — update via your distro");
                    return;
                }
            },
            _ => vec!["flatpak".into(), "update".into(), "-y".into(), MOCKTAIL_APP_ID.into()],
        };
        log_line(&home_log, &format!("$ {}", argv.join(" ")));
        run_streaming(argv,
            clone!(#[strong] home_log, move |l| log_line(&home_log, &l)),
            clone!(#[strong] toast, #[strong] refresh_status, #[strong] set_buttons_sensitive, move |code| {
                set_buttons_sensitive(true);
                toast(if code == 0 { "Mocktail updated" } else { "Update finished with errors" });
                refresh_status();
            }),
        );
    }));

    btn_uninstall.connect_clicked(clone!(#[strong] window, #[strong] home_log, #[strong] toast, #[strong] refresh_status, #[strong] set_buttons_sensitive, #[strong] state, move |_| {
        let flavor = state.borrow().flavor;
        let body = match flavor {
            MocktailFlavor::Native => "This removes the native mocktail package (via your package manager). Your fflags.json and config.yaml are kept unless you also clear them in Advanced.".to_string(),
            _ => "This removes the Mocktail flatpak. Your fflags.json and config.yaml are kept unless you also clear them in Advanced.".to_string(),
        };
        confirm_dialog(&window, "Uninstall Mocktail?",
            &body,
            "Uninstall", true,
            clone!(#[strong] home_log, #[strong] toast, #[strong] refresh_status, #[strong] set_buttons_sensitive, #[strong] state, move || {
                let flavor = state.borrow().flavor;
                let argv = match flavor {
                    MocktailFlavor::Native => match native_pkg_argv(PkgOp::Remove) {
                        Some(argv) => argv,
                        None => {
                            toast("No supported package manager found — uninstall via your distro");
                            return;
                        }
                    },
                    _ => vec!["flatpak".into(), "uninstall".into(), "-y".into(), MOCKTAIL_APP_ID.into()],
                };
                set_buttons_sensitive(false);
                log_line(&home_log, &format!("$ {}", argv.join(" ")));
                run_streaming(argv,
                    clone!(#[strong] home_log, move |l| log_line(&home_log, &l)),
                    clone!(#[strong] toast, #[strong] refresh_status, #[strong] set_buttons_sensitive, move |code| {
                        set_buttons_sensitive(true);
                        toast(if code == 0 { "Mocktail uninstalled" } else { "Uninstall finished with errors" });
                        refresh_status();
                    }),
                );
            }),
        );
    }));
}

// --------------------------------------------------------------------------
// FFLAGS
// --------------------------------------------------------------------------

fn build_fflags_page(
    stack: &adw::ViewStack,
    window: adw::ApplicationWindow,
    state: SharedState,
    toast: Rc<dyn Fn(&str)>,
) {
    let root = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(18).build();

    let banner = adw::Banner::builder()
        .title("Mocktail merges fflags.json into Roblox's client settings on launch. Only allowlisted flags take effect — unlisted flags are silently ignored by Roblox.")
        .revealed(true)
        .build();
    root.append(&banner);

    let presets_group = adw::PreferencesGroup::builder()
        .description("Apply a known-good starting point, then fine-tune below.")
        .build();
    root.append(&gradient_text_widget("Presets", 18, GRADIENT_GREEN_YELLOW));
    let preset_flow = gtk4::FlowBox::builder().selection_mode(gtk4::SelectionMode::None).max_children_per_line(3).build();
    presets_group.add(&preset_flow);
    root.append(&presets_group);

    let library_group = adw::PreferencesGroup::builder()
        .description("Tap to add a confirmed allowlisted flag with a sensible default.")
        .build();
    root.append(&gradient_text_widget("Known FastFlags", 18, GRADIENT_GREEN_YELLOW));
    let library_flow = gtk4::FlowBox::builder().selection_mode(gtk4::SelectionMode::None).max_children_per_line(2).build();
    library_group.add(&library_flow);
    root.append(&library_group);

    let active_header = gtk4::Box::builder().spacing(8).build();
    let active_title = gradient_text_widget("Active FastFlags", 18, GRADIENT_GREEN_YELLOW);
    active_title.set_hexpand(true);
    let add_btn = gtk4::Button::builder().icon_name("list-add-symbolic").css_classes(["flat", "circular"]).tooltip_text("Add custom flag").build();
    active_header.append(&active_title);
    active_header.append(&add_btn);
    root.append(&active_header);

    let fflags_group = adw::PreferencesGroup::new();
    root.append(&fflags_group);

    let save_bar = gtk4::Box::builder().spacing(10).halign(gtk4::Align::End).build();
    let clear_btn = gtk4::Button::builder().label("Clear All").css_classes(["destructive-action"]).build();
    let save_btn = gtk4::Button::builder().label("Save to fflags.json").css_classes(["suggested-action"]).build();
    save_bar.append(&clear_btn);
    save_bar.append(&save_btn);
    root.append(&save_bar);

    stack.add_titled_with_icon(&page_scroller(&root), Some("fflags"), "FFlags", "preferences-desktop-display-symbolic");

    let rows: Rc<RefCell<Vec<gtk4::Widget>>> = Rc::new(RefCell::new(Vec::new()));

    // `refresh_list` needs to be able to call itself from the per-row delete
    // button, so we stash a handle to it here before it's fully built.
    let refresh_list_holder: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let refresh_list: Rc<dyn Fn()> = {
        let state = state.clone();
        let fflags_group = fflags_group.clone();
        let rows = rows.clone();
        let toast = toast.clone();
        let refresh_list_holder = refresh_list_holder.clone();
        Rc::new(move || {
            for w in rows.borrow_mut().drain(..) {
                fflags_group.remove(&w);
            }
            let s = state.borrow();
            if s.fflags.is_empty() {
                let placeholder = adw::ActionRow::builder()
                    .title("No FastFlags configured yet")
                    .subtitle("Use a preset or add one above")
                    .build();
                fflags_group.add(&placeholder);
                rows.borrow_mut().push(placeholder.upcast());
                return;
            }
            let mut names: Vec<String> = s.fflags.keys().cloned().collect();
            names.sort();
            drop(s);
            for name in names {
                let value_str = {
                    let s = state.borrow();
                    value_to_string(s.fflags.get(&name).unwrap())
                };
                let row = adw::EntryRow::builder().title(name.as_str()).build();
                row.set_text(&value_str);

                row.connect_apply(clone!(#[strong] state, #[strong] toast, #[strong] name, move |r| {
                    let mut s = state.borrow_mut();
                    s.fflags.insert(name.clone(), coerce_value(&r.text()));
                    drop(s);
                    toast(&format!("{name} updated (remember to save)"));
                }));

                let del_btn = gtk4::Button::builder().icon_name("user-trash-symbolic").css_classes(["flat"]).valign(gtk4::Align::Center).build();
                del_btn.connect_clicked(clone!(#[strong] state, #[strong] refresh_list_holder, #[strong] name, move |_| {
                    state.borrow_mut().fflags.remove(&name);
                    if let Some(f) = refresh_list_holder.borrow().as_ref() {
                        f();
                    }
                }));
                row.add_suffix(&del_btn);

                fflags_group.add(&row);
                rows.borrow_mut().push(row.upcast());
            }
        })
    };

    *refresh_list_holder.borrow_mut() = Some(refresh_list.clone());

    refresh_list();

    for (name, default, tip) in KNOWN_FFLAGS {
        let btn = gtk4::Button::builder().label(*name).css_classes(["flat"]).tooltip_text(*tip).build();
        btn.connect_clicked(clone!(#[strong] state, #[strong] toast, #[strong] refresh_list, move |_| {
            let mut s = state.borrow_mut();
            if s.fflags.contains_key(*name) {
                drop(s);
                toast(&format!("{name} is already active"));
            } else {
                s.fflags.insert(name.to_string(), coerce_value(default));
                drop(s);
                refresh_list();
                toast(&format!("Added {name} (remember to save)"));
            }
        }));
        library_flow.append(&btn);
    }

    for (name, preset) in presets() {
        let btn = gtk4::Button::builder().label(name).css_classes(["pill"]).build();
        btn.connect_clicked(clone!(#[strong] state, #[strong] toast, #[strong] refresh_list, move |_| {
            let mut s = state.borrow_mut();
            for (k, v) in preset.iter() {
                s.fflags.insert(k.clone(), v.clone());
            }
            drop(s);
            refresh_list();
            toast(&format!("Applied preset: {name} (remember to save)"));
        }));
        preset_flow.append(&btn);
    }

    add_btn.connect_clicked(clone!(#[strong] window, #[strong] state, #[strong] refresh_list, move |_| {
        let dialog = adw::AlertDialog::builder().heading("Add Custom FastFlag").body("Flag names are case-sensitive.").build();
        let col = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(8).build();
        let name_entry = gtk4::Entry::builder().placeholder_text("e.g. DFIntTextureQualityOverride").build();
        let value_entry = gtk4::Entry::builder().placeholder_text("true / 2 / text").build();
        col.append(&name_entry);
        col.append(&value_entry);
        dialog.set_extra_child(Some(&col));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("add", "Add");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.connect_response(None, clone!(#[strong] state, #[strong] refresh_list, #[strong] name_entry, #[strong] value_entry, move |_d, response| {
            if response == "add" {
                let name = name_entry.text().trim().to_string();
                let value = value_entry.text().trim().to_string();
                if !name.is_empty() {
                    let v = if value.is_empty() { Value::Bool(true) } else { coerce_value(&value) };
                    state.borrow_mut().fflags.insert(name, v);
                    refresh_list();
                }
            }
        }));
        dialog.present(Some(&window));
    }));

    clear_btn.connect_clicked(clone!(#[strong] window, #[strong] state, #[strong] refresh_list, move |_| {
        confirm_dialog(&window, "Clear all FastFlags?",
            "This empties the in-memory list. Nothing is written to disk until you press Save.",
            "Clear", true,
            clone!(#[strong] state, #[strong] refresh_list, move || {
                state.borrow_mut().fflags.clear();
                refresh_list();
            }),
        );
    }));

    save_btn.connect_clicked(clone!(#[strong] state, #[strong] toast, move |_| {
        let s = state.borrow();
        match save_fflags(&s.paths, &s.fflags) {
            Ok(_) => toast("Saved to fflags.json"),
            Err(e) => toast(&format!("Couldn't save: {e}")),
        }
    }));
}

// --------------------------------------------------------------------------
// TEXTURE OVERRIDES
//
// Mocktail has no asset_overlay-style path-based overrides. Its one asset
// customization hook is MOCKTAIL_TEXTURE_OVERRIDE_DIR (config/mocktail/
// textures), scanned for `<hash>.png` files named by a 64-bit FNV-1a hash of
// the texture's bytes. The companion MOCKTAIL_TEXTURE_DUMP_DIR env var makes
// Mocktail write every texture it loads out as `<hash>_<w>x<h>.png` — which
// is what makes the workflow usable: dump → pick → import → restart.
// --------------------------------------------------------------------------

/// Recognizes Mocktail's dump filenames (`<16-hex-hash>_<w>x<h>.png`) and
/// returns the hash portion.
fn parse_dumped_texture(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".png")?;
    let (hash, dims) = stem.rsplit_once('_')?;
    if hash.len() != 16 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let (w, h) = dims.split_once('x')?;
    w.parse::<u32>().ok()?;
    h.parse::<u32>().ok()?;
    Some(hash.to_ascii_lowercase())
}

fn build_textures_page(
    stack: &adw::ViewStack,
    _window: adw::ApplicationWindow,
    state: SharedState,
    toast: Rc<dyn Fn(&str)>,
) {
    let root = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(18).build();

    let (texture_dir, dump_dir) = {
        let s = state.borrow();
        (s.paths.texture_overrides.clone(), s.paths.texture_dumps.clone())
    };

    let banner = adw::Banner::builder()
        .title("Texture overrides are hash-named PNG files in config/mocktail/textures, applied when Mocktail restarts. Clearing them (Advanced tab) removes every override at once.")
        .revealed(true)
        .build();
    root.append(&banner);

    // ---- Dump / import / manage workflow ----
    let workflow_group = adw::PreferencesGroup::builder()
        .description("Mocktail has no filename-based asset overrides — textures are addressed by content hash. Dump them first, then import renames your picks into place.")
        .build();
    root.append(&gradient_text_widget("Dump & Import", 18, GRADIENT_GREEN_YELLOW));

    let dump_row = adw::ActionRow::builder()
        .title("Launch With Texture Dumping")
        .subtitle(format!("Starts Mocktail writing every texture it loads to {}", dump_dir.display()))
        .build();
    let btn_dump_launch = gtk4::Button::builder()
        .label("Launch")
        .css_classes(["suggested-action"])
        .valign(gtk4::Align::Center)
        .build();
    dump_row.add_suffix(&btn_dump_launch);
    workflow_group.add(&dump_row);

    let import_row = adw::ActionRow::builder().title("Import Dumped Textures").build();
    let btn_import = gtk4::Button::builder().label("Import").valign(gtk4::Align::Center).build();
    import_row.add_suffix(&btn_import);
    workflow_group.add(&import_row);

    let active_row = adw::ActionRow::builder().title("Active Overrides").build();
    let btn_refresh = gtk4::Button::builder()
        .icon_name("view-refresh-symbolic")
        .css_classes(["flat"])
        .valign(gtk4::Align::Center)
        .tooltip_text("Refresh")
        .build();
    active_row.add_suffix(&btn_refresh);
    workflow_group.add(&active_row);

    let open_row = adw::ActionRow::builder()
        .title("Open Overrides Folder")
        .subtitle(texture_dir.display().to_string())
        .activatable(true)
        .build();
    open_row.add_suffix(&gtk4::Image::from_icon_name("folder-symbolic"));
    workflow_group.add(&open_row);
    root.append(&workflow_group);

    let log_scroll = gtk4::ScrolledWindow::builder().min_content_height(120).css_classes(["lemonyde-card"]).build();
    let tex_log = gtk4::TextView::builder().editable(false).cursor_visible(false).monospace(true).css_classes(["lemonyde-log"]).build();
    log_scroll.set_child(Some(&tex_log));
    root.append(&log_scroll);

    root.append(&gradient_text_widget("How It Works", 18, GRADIENT_GREEN_YELLOW));
    let how_group = adw::PreferencesGroup::builder()
        .description("1. Launch with dumping — Mocktail writes a hash-named PNG (hash_WxH.png) copy of every texture it loads. 2. Browse the dumps folder and edit the textures you want in any image editor. 3. Import — each dump becomes hash.png in the overrides folder. 4. Restart Mocktail; that hash's texture is replaced wherever it appears. Overrides are keyed by content, so a texture Roblox updates will no longer match its dump.")
        .build();
    root.append(&how_group);

    // Keep the waiting/active counts honest after every action.
    let refresh_counts: Rc<dyn Fn()> = {
        let import_row = import_row.clone();
        let active_row = active_row.clone();
        let dump_dir = dump_dir.clone();
        let texture_dir = texture_dir.clone();
        Rc::new(move || {
            let dumped = std::fs::read_dir(&dump_dir)
                .map(|d| {
                    d.filter_map(Result::ok)
                        .filter(|e| parse_dumped_texture(&e.file_name().to_string_lossy()).is_some())
                        .count()
                })
                .unwrap_or(0);
            let active = std::fs::read_dir(&texture_dir)
                .map(|d| {
                    d.filter_map(Result::ok)
                        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
                        .count()
                })
                .unwrap_or(0);
            if dumped == 0 {
                import_row.set_subtitle("No dumped textures waiting — launch with dumping first");
            } else {
                import_row.set_subtitle(&format!("{dumped} dumped texture(s) waiting — copies each to textures/ as hash.png"));
            }
            active_row.set_subtitle(&format!("{active} override file(s) in {}", texture_dir.display()));
        })
    };
    refresh_counts();

    btn_refresh.connect_clicked(clone!(#[strong] refresh_counts, move |_| {
        refresh_counts();
    }));

    open_row.connect_activated(clone!(#[strong] toast, #[strong] texture_dir, move |_| {
        let _ = std::fs::create_dir_all(&texture_dir);
        let uri = format!("file://{}", texture_dir.display());
        if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
            toast("Couldn't open folder");
        }
    }));

    btn_import.connect_clicked(clone!(#[strong] toast, #[strong] refresh_counts, #[strong] dump_dir, #[strong] texture_dir, move |_| {
        let mut imported = 0usize;
        if let Ok(entries) = std::fs::read_dir(&dump_dir) {
            let _ = std::fs::create_dir_all(&texture_dir);
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(hash) = parse_dumped_texture(&name) {
                    if std::fs::copy(entry.path(), texture_dir.join(format!("{hash}.png"))).is_ok() {
                        imported += 1;
                    }
                }
            }
        }
        refresh_counts();
        if imported == 0 {
            toast("No dumped textures found — launch with dumping first");
        } else {
            toast(&format!("Imported {imported} texture override(s) — restart Mocktail"));
        }
    }));

    btn_dump_launch.connect_clicked(clone!(#[strong] toast, #[strong] tex_log, #[strong] refresh_counts, #[strong] dump_dir, #[strong] btn_dump_launch, #[strong] state, move |_| {
        btn_dump_launch.set_sensitive(false);
        // Re-read the flavor at click time — the user may have installed
        // Mocktail after this page was built.
        let argv = launch_argv(state.borrow().flavor, Some(dump_dir.as_path()));
        log_line(&tex_log, &format!("$ {}", argv.join(" ")));
        run_streaming(argv,
            clone!(#[strong] tex_log, move |l| log_line(&tex_log, &l)),
            clone!(#[strong] toast, #[strong] refresh_counts, #[strong] btn_dump_launch, move |code| {
                btn_dump_launch.set_sensitive(true);
                if code == 0 {
                    toast("Mocktail closed — pick textures to import below");
                } else {
                    toast("Dump session ended — see the log above");
                }
                refresh_counts();
            }),
        );
    }));

    stack.add_titled_with_icon(&page_scroller(&root), Some("textures"), "Textures", "applications-graphics-symbolic");
}

// --------------------------------------------------------------------------
// SETTINGS
// --------------------------------------------------------------------------

fn build_settings_page(
    stack: &adw::ViewStack,
    _window: adw::ApplicationWindow,
    state: SharedState,
    toast: Rc<dyn Fn(&str)>,
) {
    let root = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(18).build();

    let banner = adw::Banner::builder()
        .title("These map to keys in Mocktail's config.yaml, which Mocktail reads on its next launch.")
        .revealed(true)
        .build();
    root.append(&banner);

    let settings_file = state.borrow().paths.settings_file.clone();
    let yaml_text = std::fs::read_to_string(&settings_file).unwrap_or_default();

    const BACKENDS: &[&str] = &["direct-vulkan", "opengl", "system", "angle-vulkan"];
    const THEMES: &[&str] = &["roblox", "dark", "light", "system"];

    let group = adw::PreferencesGroup::builder().build();
    root.append(&gradient_text_widget("Rendering & Display", 18, GRADIENT_GREEN_YELLOW));

    let current_backend = yaml_get(&yaml_text, &["graphics", "backend"]).unwrap_or_else(|| "direct-vulkan".into());
    let row_backend = adw::ComboRow::builder()
        .title("Graphics Backend")
        .subtitle("graphics.backend — try opengl if Vulkan causes black screens or crashes")
        .model(&gtk4::StringList::new(BACKENDS))
        .build();
    row_backend.set_selected(BACKENDS.iter().position(|b| *b == current_backend).unwrap_or(0) as u32);

    let row_hidpi = adw::SwitchRow::builder()
        .title("Enable HiDPI")
        .subtitle("window.high_dpi — render at physical display-pixel density for sharper high-DPI output")
        .build();
    row_hidpi.set_active(yaml_get(&yaml_text, &["window", "high_dpi"]).is_some_and(|v| v.eq_ignore_ascii_case("true")));

    group.add(&row_backend);
    group.add(&row_hidpi);
    root.append(&group);

    let group2 = adw::PreferencesGroup::builder()
        .description("The device line decides which client profile Roblox sees. Presets: pc-windows-11, mobile-pixel-7, console-ps5 — or the short aliases pc, mobile, console.")
        .build();
    root.append(&gradient_text_widget("Device & Appearance", 18, GRADIENT_GREEN_YELLOW));

    let row_device = adw::EntryRow::builder().title("Device preset").build();
    row_device.set_text(&yaml_get(&yaml_text, &["device"]).unwrap_or_default());

    let current_theme = yaml_get(&yaml_text, &["appearance", "theme"]).unwrap_or_else(|| "roblox".into());
    let row_theme = adw::ComboRow::builder()
        .title("Theme")
        .subtitle("appearance.theme — roblox keeps Roblox's saved account theme")
        .model(&gtk4::StringList::new(THEMES))
        .build();
    row_theme.set_selected(THEMES.iter().position(|t| *t == current_theme).unwrap_or(0) as u32);

    group2.add(&row_device);
    group2.add(&row_theme);
    root.append(&group2);

    let save_btn = gtk4::Button::builder().label("Save Settings").css_classes(["suggested-action", "pill"]).halign(gtk4::Align::End).build();
    root.append(&save_btn);

    save_btn.connect_clicked(clone!(#[strong] state, #[strong] toast, #[strong] row_backend, #[strong] row_hidpi, #[strong] row_device, #[strong] row_theme, move |_| {
        let s = state.borrow();
        // Re-read the file so edits made in a text editor since the page
        // loaded aren't clobbered — yaml_set only touches our own keys.
        let mut text = std::fs::read_to_string(&s.paths.settings_file).unwrap_or_default();
        let backend = BACKENDS.get(row_backend.selected() as usize).copied().unwrap_or("direct-vulkan");
        let theme = THEMES.get(row_theme.selected() as usize).copied().unwrap_or("roblox");
        text = yaml_set(&text, &["graphics", "backend"], backend);
        text = yaml_set(&text, &["window", "high_dpi"], if row_hidpi.is_active() { "true" } else { "false" });
        let device = row_device.text().trim().to_string();
        if !device.is_empty() {
            text = yaml_set(&text, &["device"], &device);
        }
        text = yaml_set(&text, &["appearance", "theme"], theme);

        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&s.paths.config_dir)?;
            if s.paths.settings_file.exists() {
                let _ = std::fs::copy(&s.paths.settings_file, s.paths.settings_file.with_extension("yaml.bak"));
            }
            std::fs::write(&s.paths.settings_file, text)
        })();
        match result {
            Ok(_) => toast("Settings saved to config.yaml"),
            Err(e) => toast(&format!("Couldn't save: {e}")),
        }
    }));

    stack.add_titled_with_icon(&page_scroller(&root), Some("settings"), "Settings", "preferences-system-symbolic");
}

// --------------------------------------------------------------------------
// ADVANCED
// --------------------------------------------------------------------------

fn build_advanced_page(
    stack: &adw::ViewStack,
    window: adw::ApplicationWindow,
    state: SharedState,
    toast: Rc<dyn Fn(&str)>,
) {
    let root = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(18).build();

    let paths_snapshot = {
        let s = state.borrow();
        (s.paths.config_dir.clone(), s.paths.data_dir.clone(),
         s.paths.texture_overrides.clone(), s.paths.texture_dumps.clone(),
         s.paths.fflags_file.clone(), s.paths.settings_file.clone())
    };
    let (config_dir, data_dir, texture_overrides, texture_dumps, fflags_file, settings_file) = paths_snapshot;

    let group = adw::PreferencesGroup::builder().build();
    root.append(&gradient_text_widget("Files & Folders", 18, GRADIENT_GREEN_YELLOW));
    let r1 = adw::ActionRow::builder().title("Open Config Folder").subtitle(config_dir.display().to_string()).activatable(true).build();
    r1.add_suffix(&gtk4::Image::from_icon_name("folder-symbolic"));
    group.add(&r1);
    let r2 = adw::ActionRow::builder().title("Open Data Folder").subtitle(data_dir.display().to_string()).activatable(true).build();
    r2.add_suffix(&gtk4::Image::from_icon_name("folder-symbolic"));
    group.add(&r2);
    let r_instances = adw::ActionRow::builder()
        .title("Open Instances Folder")
        .subtitle(instances_dir().display().to_string())
        .activatable(true)
        .build();
    r_instances.add_suffix(&gtk4::Image::from_icon_name("folder-symbolic"));
    group.add(&r_instances);
    root.append(&group);

    let group2 = adw::PreferencesGroup::builder().build();
    root.append(&gradient_text_widget("Maintenance", 18, GRADIENT_GREEN_YELLOW));
    let r3 = adw::ActionRow::builder().title("Clear Texture Overrides").subtitle("Removes custom texture overrides and dumped textures").activatable(true).build();
    r3.add_suffix(&gtk4::Image::from_icon_name("edit-clear-symbolic"));
    group2.add(&r3);
    let r4 = adw::ActionRow::builder()
        .title("Reset FFlags & Config")
        .subtitle("Deletes fflags.json and config.yaml — Mocktail regenerates defaults on next launch")
        .activatable(true)
        .build();
    r4.add_suffix(&gtk4::Image::from_icon_name("view-refresh-symbolic"));
    group2.add(&r4);
    root.append(&group2);

    let group3 = adw::PreferencesGroup::builder().build();
    root.append(&gradient_text_widget("Advanced Log", 18, GRADIENT_GREEN_YELLOW));
    let log_scroll = gtk4::ScrolledWindow::builder().min_content_height(160).css_classes(["lemonyde-card"]).build();
    let adv_log = gtk4::TextView::builder().editable(false).cursor_visible(false).monospace(true).css_classes(["lemonyde-log"]).build();
    log_scroll.set_child(Some(&adv_log));
    group3.add(&log_scroll);
    root.append(&group3);

    stack.add_titled_with_icon(&page_scroller(&root), Some("advanced"), "Advanced", "applications-engineering-symbolic");

    r1.connect_activated(clone!(#[strong] toast, #[strong] config_dir, move |_| {
        let _ = std::fs::create_dir_all(&config_dir);
        let uri = format!("file://{}", config_dir.display());
        if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
            toast("Couldn't open folder");
        }
    }));
    r2.connect_activated(clone!(#[strong] toast, #[strong] data_dir, move |_| {
        let _ = std::fs::create_dir_all(&data_dir);
        let uri = format!("file://{}", data_dir.display());
        if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
            toast("Couldn't open folder");
        }
    }));
    r_instances.connect_activated(clone!(#[strong] toast, move |_| {
        let dir = instances_dir();
        let _ = std::fs::create_dir_all(&dir);
        let uri = format!("file://{}", dir.display());
        if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
            toast("Couldn't open folder");
        }
    }));

    r3.connect_activated(clone!(#[strong] window, #[strong] toast, #[strong] adv_log, #[strong] texture_overrides, #[strong] texture_dumps, move |_| {
        let body = format!("This deletes:\n{}\n{}", texture_overrides.display(), texture_dumps.display());
        confirm_dialog(&window, "Clear texture overrides?", &body, "Clear", true,
            clone!(#[strong] toast, #[strong] adv_log, #[strong] texture_overrides, #[strong] texture_dumps, move || {
                let mut cleared = false;
                for dir in [&texture_overrides, &texture_dumps] {
                    if dir.exists() {
                        let _ = std::fs::remove_dir_all(dir);
                        log_line(&adv_log, &format!("Removed {}", dir.display()));
                        cleared = true;
                    }
                }
                if cleared {
                    toast("Texture overrides cleared");
                } else {
                    toast("Nothing to clear");
                }
            }),
        );
    }));

    r4.connect_activated(clone!(#[strong] window, #[strong] state, #[strong] toast, #[strong] adv_log, #[strong] fflags_file, #[strong] settings_file, move |_| {
        confirm_dialog(&window, "Reset FFlags & config?",
            "This deletes fflags.json and config.yaml. Mocktail will regenerate defaults on next launch.",
            "Reset", true,
            clone!(#[strong] state, #[strong] toast, #[strong] adv_log, #[strong] fflags_file, #[strong] settings_file, move || {
                for f in [&fflags_file, &settings_file] {
                    if f.exists() {
                        let _ = std::fs::remove_file(f);
                        log_line(&adv_log, &format!("Deleted {}", f.display()));
                    }
                }
                let mut s = state.borrow_mut();
                s.fflags.clear();
                toast("Mocktail config reset");
            }),
        );
    }));
}

// --------------------------------------------------------------------------
// ABOUT
// --------------------------------------------------------------------------

fn build_about_page(stack: &adw::ViewStack) {
    let root = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(16).build();
    root.append(&gradient_title_widget());
    root.append(&gtk4::Label::builder().label("version 2.0.0 (Rust) · unofficial community bootstrapper").css_classes(["dim-label"]).build());

    let group = adw::PreferencesGroup::new();
    for (label, uri) in [
        ("Mocktail website", "https://mocktail.bigrat.space/"),
        ("Mocktail on Flathub", "https://flathub.org/apps/space.bigrat.mocktail"),
        ("Mocktail source / issue tracker", "https://github.com/komaruworld/mocktail"),
        ("FastFlags allowlist reference (Sober guide)", "https://github.com/dyokism/sober-fastflags"),
    ] {
        let row = adw::ActionRow::builder().title(label).subtitle(uri).activatable(true).build();
        row.add_suffix(&gtk4::Image::from_icon_name("adw-external-link-symbolic"));
        row.connect_activated(move |_| {
            let _ = gio::AppInfo::launch_default_for_uri(uri, gio::AppLaunchContext::NONE);
        });
        group.add(&row);
    }
    root.append(&group);

    root.append(
        &gtk4::Label::builder()
            .label(
                "Lemonyde is an independent, unofficial tool and is not affiliated with, \
                 endorsed by, or supported by Roblox Corporation or the Mocktail project \
                 authors. FFlag allowlists are controlled by Roblox and may change at any \
                 time; flags outside the allowlist are ignored automatically.",
            )
            .wrap(true)
            .css_classes(["dim-label", "caption"])
            .justify(gtk4::Justification::Center)
            .build(),
    );

    stack.add_titled_with_icon(&page_scroller(&root), Some("about"), "About", "help-about-symbolic");
}
