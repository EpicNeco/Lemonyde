// Lemonyde — an unofficial community bootstrapper for the Sober flatpak
// (Roblox on Linux, by VinegarHQ: https://sober.vinegarhq.org).
//
// Not affiliated with Roblox Corporation or VinegarHQ. Provided as-is.
//
// Rust + GTK4 + libadwaita rewrite of the original Python prototype.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::thread;

use gio::prelude::*;
use glib::clone;
use gtk4::prelude::*;
use gtk4::{glib, gio};
use libadwaita as adw;
use adw::prelude::*;
use serde_json::{Map, Value};

const APP_ID: &str = "org.lemonyde.Bootstrapper";
const SOBER_APP_ID: &str = "org.vinegarhq.Sober";

/// Flags confirmed to be on Roblox's client-config allowlist as of the
/// community-maintained guide at github.com/dyokism/sober-fastflags
/// (accurate as of July 2026 — Roblox may change the allowlist at any time).
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
    config_file: PathBuf,
    data_dir: PathBuf,
    client_settings: PathBuf,
    asset_overlay: PathBuf,
}

impl Paths {
    fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base = home.join(".var/app").join(SOBER_APP_ID);
        let config_dir = base.join("config/sober");
        let data_dir = base.join("data/sober");
        Paths {
            config_file: config_dir.join("config.json"),
            config_dir,
            client_settings: data_dir.join("exe/ClientSettings/ClientAppSettings.json"),
            asset_overlay: data_dir.join("asset_overlay"),
            data_dir,
        }
    }
}

// --------------------------------------------------------------------------
// Multi-instance support
//
// Sober enforces its own single-instance lock, so launching a second
// `flatpak run org.vinegarhq.Sober` with the same $HOME crashes with
// "An instance of Sober is already running." The fix (a well-known Flatpak
// trick, not specific to Sober) is to give each extra instance its own
// fake $HOME — Flatpak resolves an app's private data at $HOME/.var/app/<id>,
// so a different $HOME means a completely separate lock file, config, and
// login session. We keep XDG_DATA_HOME/XDG_CONFIG_HOME/XDG_CACHE_HOME
// pointed at their real locations so the `flatpak` command itself can still
// find the installed app (which lives under the *real* user's
// ~/.local/share/flatpak, untouched by the fake HOME).
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
/// Reused across launches so a slot's Sober login persists between runs.
fn slot_home(n: u32) -> PathBuf {
    instances_dir().join(format!("slot-{n}"))
}

/// Spawns Sober inside isolated instance slot `n`. Returns the child process
/// handle so the caller can track it (and detect when it exits) rather than
/// firing-and-forgetting — launching into a slot that's already running is
/// exactly what triggers Sober's "already running" / frozen-instance crash.
fn spawn_slot_process(n: u32) -> std::io::Result<std::process::Child> {
    let home = slot_home(n);
    std::fs::create_dir_all(&home)?;
    Command::new("flatpak")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", xdg_data_home())
        .env("XDG_CONFIG_HOME", xdg_config_home())
        .env("XDG_CACHE_HOME", xdg_cache_home())
        .args(["run", SOBER_APP_ID])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn load_config(paths: &Paths) -> Map<String, Value> {
    std::fs::read_to_string(&paths.config_file)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn save_config(paths: &Paths, cfg: &Map<String, Value>) -> std::io::Result<()> {
    std::fs::create_dir_all(&paths.config_dir)?;
    if paths.config_file.exists() {
        let _ = std::fs::copy(&paths.config_file, paths.config_file.with_extension("json.bak"));
    }
    let text = serde_json::to_string_pretty(cfg).unwrap_or_default();
    std::fs::write(&paths.config_file, text + "\n")
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
    if let Ok(f) = t.parse::<f64>() {
        return Value::from(f);
    }
    Value::String(t.to_string())
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// --------------------------------------------------------------------------
// Flatpak helpers
// --------------------------------------------------------------------------

fn flatpak_present() -> bool {
    Command::new("which").arg("flatpak").output().map(|o| o.status.success()).unwrap_or(false)
}

fn sober_installed() -> bool {
    Command::new("flatpak")
        .args(["info", SOBER_APP_ID])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn sober_version() -> Option<String> {
    let out = Command::new("flatpak").args(["info", SOBER_APP_ID]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("Version:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Counts how many sandboxed Sober processes are currently running, via
/// `flatpak ps`. Each `flatpak run` spawns its own sandbox instance, so
/// this is also how we confirm a multi-instance launch actually took.
fn running_instance_count() -> usize {
    Command::new("flatpak")
        .args(["ps", "--columns=application"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.trim() == SOBER_APP_ID)
                .count()
        })
        .unwrap_or(0)
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
    other: Map<String, Value>,
}

type SharedState = Rc<RefCell<AppState>>;

fn asset_dir() -> PathBuf {
    // Look next to the executable first (installed layout), then fall back
    // to the source tree's ./assets for `cargo run`.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("assets/lemonyde.svg");
            if candidate.exists() {
                return dir.join("assets");
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

fn logo_path() -> PathBuf {
    asset_dir().join("lemonyde.svg")
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

/// Opens a native file picker and copies whatever local file the user
/// selects to `dest` (creating parent directories as needed). Used by the
/// Customize page for font asset_overlay overrides. Silently does nothing
/// if the user cancels.
fn pick_and_copy_file(
    window: &adw::ApplicationWindow,
    title: &str,
    dest: PathBuf,
    on_done: impl Fn(Result<(), String>) + 'static,
) {
    let dialog = gtk4::FileDialog::builder().title(title).build();
    dialog.open(Some(window), gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else { return };
        let Some(src) = file.path() else {
            on_done(Err("Only local files are supported".into()));
            return;
        };
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let result = std::fs::copy(&src, &dest).map(|_| ()).map_err(|e| e.to_string());
        on_done(result);
    });
}

/// Normalizes an arbitrary source image into the format Roblox actually
/// expects for a cursor override: a 64×64 transparent-canvas PNG (the
/// dimensions Bloxstrap's own modding docs confirm for ArrowCursor /
/// ArrowFarCursor). Without this, cursors picked at different source
/// resolutions render at inconsistent sizes in-game relative to each other
/// — that's the "inconsistent appearance" this fixes. The source image is
/// scaled to fit (preserving aspect ratio, never stretched) and centered on
/// a transparent 64×64 canvas rather than squished to a square.
///
/// Returns `Ok(Some(warning))` if it succeeded but the source had little or
/// no transparency (Roblox renders a solid box around a fully-opaque
/// cursor), `Ok(None)` on a clean success, or `Err` on failure.
fn normalize_cursor_image(src: &Path, dest: &Path) -> Result<Option<&'static str>, String> {
    const CANVAS: u32 = 64;

    let img = image::open(src).map_err(|e| format!("Couldn't read image: {e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return Err("Image has no pixels".into());
    }

    let scale = CANVAS as f32 / w.max(h) as f32;
    let new_w = ((w as f32) * scale).round().max(1.0) as u32;
    let new_h = ((h as f32) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&rgba, new_w, new_h, image::imageops::FilterType::Lanczos3);

    let mut canvas = image::RgbaImage::new(CANVAS, CANVAS); // fully transparent by default
    let x_off = ((CANVAS - new_w) / 2) as i64;
    let y_off = ((CANVAS - new_h) / 2) as i64;
    image::imageops::overlay(&mut canvas, &resized, x_off, y_off);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    canvas.save(dest).map_err(|e| format!("Couldn't save image: {e}"))?;

    let total = (new_w * new_h) as f32;
    let opaque_count = resized.pixels().filter(|p| p.0[3] > 250).count() as f32;
    if total > 0.0 && opaque_count / total > 0.98 {
        Ok(Some("this image has little/no transparency — Roblox may show a solid box around it in-game"))
    } else {
        Ok(None)
    }
}

/// Like `pick_and_copy_file`, but for cursors specifically: runs the picked
/// image through `normalize_cursor_image` instead of a raw byte copy.
fn pick_and_apply_cursor(
    window: &adw::ApplicationWindow,
    title: &str,
    dest: PathBuf,
    on_done: impl Fn(Result<Option<&'static str>, String>) + 'static,
) {
    let dialog = gtk4::FileDialog::builder().title(title).build();
    dialog.open(Some(window), gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else { return };
        let Some(src) = file.path() else {
            on_done(Err("Only local files are supported".into()));
            return;
        };
        on_done(normalize_cursor_image(&src, &dest));
    });
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
    let state: SharedState = Rc::new(RefCell::new(AppState {
        paths: Paths::new(),
        fflags: Map::new(),
        other: Map::new(),
    }));
    {
        let mut s = state.borrow_mut();
        let cfg = load_config(&s.paths);
        let mut cfg = cfg;
        let fflags = cfg.remove("fflags").and_then(|v| v.as_object().cloned()).unwrap_or_default();
        s.fflags = fflags;
        s.other = cfg;
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

    // CSS: dark grey background, yellow text everywhere.
    let css = gtk4::CssProvider::new();
    css.load_from_data(include_str!("../style.css"));
    gtk4::style_context_add_provider_for_display(
        &WidgetExt::display(&window),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let toolbar_view = adw::ToolbarView::new();
    toast_overlay.set_child(Some(&toolbar_view));

    let view_stack = adw::ViewStack::new();

    let switcher_title = adw::ViewSwitcherTitle::builder().stack(&view_stack).title("Lemonyde").build();
    let header = adw::HeaderBar::builder().title_widget(&switcher_title).build();
    toolbar_view.add_top_bar(&header);

    let switcher_bar = adw::ViewSwitcherBar::builder().stack(&view_stack).build();
    switcher_title
        .bind_property("title-visible", &switcher_bar, "reveal")
        .sync_create()
        .build();
    toolbar_view.add_bottom_bar(&switcher_bar);

    toolbar_view.set_content(Some(&view_stack));

    let toast = Rc::new({
        let overlay = toast_overlay.clone();
        move |msg: &str| {
            overlay.add_toast(adw::Toast::builder().title(msg).timeout(3).build());
        }
    });

    build_home_page(&view_stack, window.clone(), state.clone(), toast.clone());
    build_fflags_page(&view_stack, window.clone(), state.clone(), toast.clone());
    build_customize_page(&view_stack, window.clone(), state.clone(), toast.clone());
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
            .label("A community bootstrapper for Sober (Roblox on Linux)")
            .css_classes(["dim-label"])
            .build(),
    );

    let status_group = adw::PreferencesGroup::new();
    let status_icon = gtk4::Image::from_icon_name("content-loading-symbolic");
    let status_row = adw::ActionRow::builder().title("Checking Sober installation…").build();
    status_row.add_prefix(&status_icon);
    status_group.add(&status_row);
    root.append(&status_group);

    let actions = gtk4::Box::builder().spacing(10).halign(gtk4::Align::Center).build();
    let btn_launch = gtk4::Button::builder().label("Launch Sober").css_classes(["suggested-action", "pill"]).build();
    let btn_install = gtk4::Button::builder().label("Install Sober").css_classes(["suggested-action", "pill"]).build();
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
        .description("Each numbered slot gets its own isolated Sober login & data folder, so it won't hit Sober's \"already running\" lock. Sign in once per slot and it's remembered next time.")
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
    let row_set = adw::ActionRow::builder().title("Wrapper Settings").subtitle("Rendering backend, HiDPI, touch mode").activatable(true).build();
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
        Rc::new(move || {
            status_row.set_title("Checking Sober installation…");
            status_icon.set_icon_name(Some("content-loading-symbolic"));

            let (sender, receiver) = async_channel::bounded::<(bool, Option<String>)>(1);
            thread::spawn(move || {
                let installed = sober_installed();
                let version = if installed { sober_version() } else { None };
                let _ = sender.send_blocking((installed, version));
            });

            // Manual clones (rather than the `clone!` macro) so this works
            // the same across glib macro syntax versions.
            let status_row = status_row.clone();
            let status_icon = status_icon.clone();
            let btn_launch = btn_launch.clone();
            let btn_install = btn_install.clone();
            let btn_update = btn_update.clone();
            let btn_uninstall = btn_uninstall.clone();
            glib::spawn_future_local(async move {
                if let Ok((installed, version)) = receiver.recv().await {
                    if !flatpak_present() {
                        status_row.set_title("Flatpak is not installed on this system");
                        status_row.set_subtitle("Install flatpak from your distro's package manager, then reopen Lemonyde.");
                        status_icon.set_icon_name(Some("dialog-warning-symbolic"));
                        for b in [&btn_launch, &btn_install, &btn_update, &btn_uninstall] {
                            b.set_sensitive(false);
                        }
                        return;
                    }
                    if installed {
                        status_row.set_title("Sober is installed");
                        status_row.set_subtitle(&version.map(|v| format!("Version {v}")).unwrap_or_else(|| "Ready to launch".into()));
                        status_icon.set_icon_name(Some("emblem-ok-symbolic"));
                        btn_launch.set_visible(true);
                        btn_install.set_visible(false);
                        btn_update.set_sensitive(true);
                        btn_uninstall.set_sensitive(true);
                    } else {
                        status_row.set_title("Sober is not installed");
                        status_row.set_subtitle("Install it from Flathub to get started");
                        status_icon.set_icon_name(Some("dialog-information-symbolic"));
                        btn_launch.set_visible(false);
                        btn_install.set_visible(true);
                        btn_update.set_sensitive(false);
                        btn_uninstall.set_sensitive(false);
                    }
                }
            });
        })
    };

    refresh_status();

    let refresh_running: Rc<dyn Fn()> = {
        let running_row = running_row.clone();
        Rc::new(move || {
            running_row.set_subtitle("Checking…");
            let (sender, receiver) = async_channel::bounded::<usize>(1);
            thread::spawn(move || {
                let _ = sender.send_blocking(running_instance_count());
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

    refresh_running_btn.connect_clicked(clone!(@strong refresh_running => move |_| {
        refresh_running();
    }));

    // Tracks which slots we've personally launched and not yet seen exit, so
    // a click on "Launch Instances" never spawns a second process into a
    // slot that's already running — that double-launch is exactly what
    // triggers Sober's "already running" / frozen-instance crash.
    let active_slots: Rc<RefCell<HashSet<u32>>> = Rc::new(RefCell::new(HashSet::new()));
    let (exit_tx, exit_rx) = async_channel::unbounded::<u32>();

    glib::spawn_future_local(clone!(@strong active_slots, @strong home_log, @strong refresh_running => async move {
        while let Ok(n) = exit_rx.recv().await {
            active_slots.borrow_mut().remove(&n);
            log_line(&home_log, &format!("Slot {n} closed — free to relaunch"));
            refresh_running();
        }
    }));

    fn launch_step(
        i: u32,
        n: u32,
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
            match spawn_slot_process(i) {
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
                launch_step(i + 1, n, home_log, active_slots, exit_tx, refresh_running);
            });
        } else {
            let refresh_running = refresh_running.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
                refresh_running();
            });
        }
    }

    launch_multi_btn.connect_clicked(clone!(@strong toast, @strong home_log, @strong instances_spin,
        @strong refresh_running, @strong active_slots, @strong exit_tx => move |_| {
        let n = instances_spin.value() as u32;
        if n == 0 {
            return;
        }
        log_line(&home_log, &format!("$ launching up to {n} Sober instance(s)…"));
        toast(&format!("Launching up to {n} instance(s)…"));
        launch_step(1, n, home_log.clone(), active_slots.clone(), exit_tx.clone(), refresh_running.clone());
    }));

    let set_buttons_sensitive = {
        let (a, b, c, d) = buttons.clone();
        move |sensitive: bool| {
            for w in [&a, &b, &c, &d] {
                w.set_sensitive(sensitive);
            }
        }
    };

    btn_launch.connect_clicked(clone!(@strong toast => move |_| {
        let spawned = Command::new("flatpak")
            .args(["run", SOBER_APP_ID])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => toast("Launching Sober…"),
            Err(e) => toast(&format!("Couldn't launch Sober: {e}")),
        }
    }));

    btn_install.connect_clicked(clone!(@strong home_log, @strong toast, @strong refresh_status,
        @strong set_buttons_sensitive => move |_| {
        set_buttons_sensitive(false);
        log_line(&home_log, "$ flatpak install --user -y flathub org.vinegarhq.Sober");
        let argv = vec!["flatpak".into(), "install".into(), "--user".into(), "-y".into(), "flathub".into(), SOBER_APP_ID.into()];
        run_streaming(argv,
            clone!(@strong home_log => move |l| log_line(&home_log, &l)),
            clone!(@strong toast, @strong refresh_status, @strong set_buttons_sensitive => move |code| {
                set_buttons_sensitive(true);
                toast(if code == 0 { "Sober installed" } else { "Install failed — see activity log" });
                refresh_status();
            }),
        );
    }));

    btn_update.connect_clicked(clone!(@strong home_log, @strong toast, @strong refresh_status,
        @strong set_buttons_sensitive => move |_| {
        set_buttons_sensitive(false);
        log_line(&home_log, "$ flatpak update -y org.vinegarhq.Sober");
        let argv = vec!["flatpak".into(), "update".into(), "-y".into(), SOBER_APP_ID.into()];
        run_streaming(argv,
            clone!(@strong home_log => move |l| log_line(&home_log, &l)),
            clone!(@strong toast, @strong refresh_status, @strong set_buttons_sensitive => move |code| {
                set_buttons_sensitive(true);
                toast(if code == 0 { "Sober updated" } else { "Update finished with errors" });
                refresh_status();
            }),
        );
    }));

    btn_uninstall.connect_clicked(clone!(@strong window, @strong home_log, @strong toast,
        @strong refresh_status, @strong set_buttons_sensitive => move |_| {
        confirm_dialog(&window, "Uninstall Sober?",
            "This removes the Sober flatpak. Your FFlag config is kept unless you also clear data in Advanced.",
            "Uninstall", true,
            clone!(@strong home_log, @strong toast, @strong refresh_status, @strong set_buttons_sensitive => move || {
                set_buttons_sensitive(false);
                log_line(&home_log, "$ flatpak uninstall -y org.vinegarhq.Sober");
                let argv = vec!["flatpak".into(), "uninstall".into(), "-y".into(), SOBER_APP_ID.into()];
                run_streaming(argv,
                    clone!(@strong home_log => move |l| log_line(&home_log, &l)),
                    clone!(@strong toast, @strong refresh_status, @strong set_buttons_sensitive => move |code| {
                        set_buttons_sensitive(true);
                        toast(if code == 0 { "Sober uninstalled" } else { "Uninstall finished with errors" });
                        refresh_status();
                    }),
                );
            }),
        );
    }));

    let _ = state; // state is used by other pages; home page only reads flatpak/sober status
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
        .title("Only allowlisted FFlags take effect in Sober. Unlisted flags are silently ignored by Roblox.")
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
    let save_btn = gtk4::Button::builder().label("Save to config.json").css_classes(["suggested-action"]).build();
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

                row.connect_apply(clone!(@strong state, @strong toast, @strong name => move |r| {
                    let mut s = state.borrow_mut();
                    s.fflags.insert(name.clone(), coerce_value(&r.text()));
                    drop(s);
                    toast(&format!("{name} updated (remember to save)"));
                }));

                let del_btn = gtk4::Button::builder().icon_name("user-trash-symbolic").css_classes(["flat"]).valign(gtk4::Align::Center).build();
                del_btn.connect_clicked(clone!(@strong state, @strong refresh_list_holder, @strong name => move |_| {
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
        btn.connect_clicked(clone!(@strong state, @strong toast, @strong refresh_list
            => move |_| {
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
        btn.connect_clicked(clone!(@strong state, @strong toast, @strong refresh_list => move |_| {
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

    add_btn.connect_clicked(clone!(@strong window, @strong state, @strong refresh_list => move |_| {
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
        dialog.connect_response(None, clone!(@strong state, @strong refresh_list, @strong name_entry, @strong value_entry
            => move |_d, response| {
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

    clear_btn.connect_clicked(clone!(@strong window, @strong state, @strong refresh_list => move |_| {
        confirm_dialog(&window, "Clear all FastFlags?",
            "This empties the in-memory list. Nothing is written to disk until you press Save.",
            "Clear", true,
            clone!(@strong state, @strong refresh_list => move || {
                state.borrow_mut().fflags.clear();
                refresh_list();
            }),
        );
    }));

    save_btn.connect_clicked(clone!(@strong state, @strong toast => move |_| {
        let s = state.borrow();
        let mut cfg = s.other.clone();
        cfg.insert("fflags".into(), Value::Object(s.fflags.clone()));
        match save_config(&s.paths, &cfg) {
            Ok(_) => toast("Saved to config.json"),
            Err(e) => toast(&format!("Couldn't save: {e}")),
        }
    }));
}

// --------------------------------------------------------------------------
// CUSTOMIZE
// --------------------------------------------------------------------------

fn build_customize_page(
    stack: &adw::ViewStack,
    window: adw::ApplicationWindow,
    state: SharedState,
    toast: Rc<dyn Fn(&str)>,
) {
    let root = gtk4::Box::builder().orientation(gtk4::Orientation::Vertical).spacing(18).build();

    let asset_overlay = state.borrow().paths.asset_overlay.clone();
    let cursor_dir = asset_overlay.join("content/textures/Cursors/KeyboardMouse");
    let font_dir = asset_overlay.join("content/fonts");

    let banner = adw::Banner::builder()
        .title("Overrides live in Sober's asset_overlay folder and take effect after you restart Sober. Clearing it (Advanced tab) removes every customization at once.")
        .revealed(true)
        .build();
    root.append(&banner);

    // ---- Cursor: documented, exact paths confirmed by VinegarHQ's own docs ----
    let cursor_group = adw::PreferencesGroup::builder()
        .description("Replaces the three cursor images Sober actually keeps locally. Any image works — Lemonyde resizes and centers it on a transparent 64×64 canvas, the size Roblox actually expects, so all three look consistent in-game. A few cursors (the gun/drag cursors) are streamed from Roblox's servers and can't be overridden this way.")
        .build();
    root.append(&gradient_text_widget("Cursor", 18, GRADIENT_GREEN_YELLOW));

    let cursor_targets: [(&str, &str, &str); 3] = [
        ("ArrowCursor.png", "Pointer", "The default arrow cursor"),
        ("ArrowFarCursor.png", "Pointer (zoomed out)", "Used when the camera is far from your character"),
        ("IBeamCursor.png", "Text Cursor", "Shown when hovering over text fields"),
    ];

    for (filename, title, subtitle) in cursor_targets {
        let dest = cursor_dir.join(filename);
        let row = adw::ActionRow::builder().title(title).subtitle(subtitle).build();

        let status = gtk4::Label::builder()
            .label(if dest.exists() { "Custom" } else { "Default" })
            .css_classes(["dim-label", "caption"])
            .valign(gtk4::Align::Center)
            .build();
        row.add_suffix(&status);

        let choose_btn = gtk4::Button::builder().label("Choose Image…").valign(gtk4::Align::Center).build();
        row.add_suffix(&choose_btn);

        let reset_btn = gtk4::Button::builder()
            .icon_name("edit-undo-symbolic")
            .css_classes(["flat"])
            .valign(gtk4::Align::Center)
            .tooltip_text("Reset to default")
            .build();
        row.add_suffix(&reset_btn);

        choose_btn.connect_clicked(clone!(@strong window, @strong toast, @strong status, @strong dest, @strong title
            => move |_| {
            pick_and_apply_cursor(&window, "Choose a cursor image (any size — it'll be normalized to 64×64)", dest.clone(),
                clone!(@strong toast, @strong status, @strong title => move |result| {
                match result {
                    Ok(None) => {
                        status.set_label("Custom");
                        toast(&format!("{title} updated — restart Sober to see it"));
                    }
                    Ok(Some(warning)) => {
                        status.set_label("Custom");
                        toast(&format!("{title} updated, but {warning}"));
                    }
                    Err(e) => toast(&format!("Couldn't set {title}: {e}")),
                }
            }));
        }));

        reset_btn.connect_clicked(clone!(@strong toast, @strong status, @strong dest, @strong title => move |_| {
            if dest.exists() {
                let _ = std::fs::remove_file(&dest);
                status.set_label("Default");
                toast(&format!("{title} reset to Roblox's default"));
            } else {
                toast(&format!("{title} is already default"));
            }
        }));

        cursor_group.add(&row);
    }
    root.append(&cursor_group);

    // ---- Font: same asset_overlay mechanism, but the exact filename Sober's
    // Android-based client expects isn't documented the way cursors are, so
    // this is offered as best-effort rather than a guaranteed-working preset. ----
    let font_group = adw::PreferencesGroup::builder()
        .description("Uses the same content/fonts/ convention Roblox uses on other platforms, but Sober doesn't officially document the exact filename its Android-based client expects — you may need to try a few names, or inspect the installed app's assets to confirm one.")
        .build();
    root.append(&gradient_text_widget("In-Game Font (experimental)", 18, GRADIENT_GREEN_YELLOW));

    let font_name_row = adw::EntryRow::builder()
        .title("Target filename inside content/fonts/")
        .build();
    font_group.add(&font_name_row);

    let font_status_row = adw::ActionRow::builder()
        .title("Font file")
        .subtitle(if font_dir.exists() && font_dir.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false) {
            "A custom font is currently applied"
        } else {
            "No custom font applied"
        })
        .build();
    let font_choose_btn = gtk4::Button::builder().label("Choose Font…").valign(gtk4::Align::Center).build();
    font_status_row.add_suffix(&font_choose_btn);
    let font_reset_btn = gtk4::Button::builder()
        .icon_name("edit-undo-symbolic")
        .css_classes(["flat"])
        .valign(gtk4::Align::Center)
        .tooltip_text("Remove all custom fonts")
        .build();
    font_status_row.add_suffix(&font_reset_btn);
    font_group.add(&font_status_row);
    root.append(&font_group);

    font_choose_btn.connect_clicked(clone!(@strong window, @strong toast, @strong font_name_row,
        @strong font_status_row, @strong font_dir => move |_| {
        let name = font_name_row.text().trim().to_string();
        if name.is_empty() {
            toast("Type the target filename first, e.g. a name you found by inspecting Sober's assets");
            return;
        }
        let dest = font_dir.join(&name);
        pick_and_copy_file(&window, "Choose a font file (.ttf / .otf)", dest,
            clone!(@strong toast, @strong font_status_row, @strong name => move |result| {
            match result {
                Ok(()) => {
                    font_status_row.set_subtitle(&format!("Applied as {name} — restart Sober and check if it took"));
                    toast("Font copied — this is experimental, so double-check in game");
                }
                Err(e) => toast(&format!("Couldn't apply font: {e}")),
            }
        }));
    }));

    font_reset_btn.connect_clicked(clone!(@strong toast, @strong font_status_row, @strong font_dir => move |_| {
        if font_dir.exists() {
            let _ = std::fs::remove_dir_all(&font_dir);
            font_status_row.set_subtitle("No custom font applied");
            toast("Custom fonts removed");
        } else {
            toast("No custom font to remove");
        }
    }));

    stack.add_titled_with_icon(&page_scroller(&root), Some("customize"), "Customize", "applications-graphics-symbolic");
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

    let banner = adw::Banner::builder().title("These map to root-level keys in Sober's config.json wrapper settings.").revealed(true).build();
    root.append(&banner);

    let group = adw::PreferencesGroup::builder().build();
    root.append(&gradient_text_widget("Rendering & Display", 18, GRADIENT_GREEN_YELLOW));

    let row_opengl = adw::SwitchRow::builder().title("Force OpenGL").subtitle("use_opengl — try this if Vulkan causes black screens or crashes").build();
    let row_hidpi = adw::SwitchRow::builder().title("Enable HiDPI").subtitle("enable_hidpi — sharper UI on high-resolution displays (Wayland only)").build();
    let row_close = adw::SwitchRow::builder().title("Close on Leave").subtitle("close_on_leave — exit Sober automatically when you leave a game").build();
    group.add(&row_opengl);
    group.add(&row_hidpi);
    group.add(&row_close);
    root.append(&group);

    let group2 = adw::PreferencesGroup::builder()
        .description("Community-reported setting — verify against the Vinegar docs if it seems to have no effect.")
        .build();
    root.append(&gradient_text_widget("Input", 18, GRADIENT_GREEN_YELLOW));
    let row_touch = adw::EntryRow::builder().title("touch_mode").build();
    group2.add(&row_touch);
    root.append(&group2);

    {
        let s = state.borrow();
        row_opengl.set_active(s.other.get("use_opengl").and_then(Value::as_bool).unwrap_or(false));
        row_hidpi.set_active(s.other.get("enable_hidpi").and_then(Value::as_bool).unwrap_or(false));
        row_close.set_active(s.other.get("close_on_leave").and_then(Value::as_bool).unwrap_or(false));
        row_touch.set_text(&s.other.get("touch_mode").map(value_to_string).unwrap_or_default());
    }

    row_opengl.connect_active_notify(clone!(@strong state => move |r| {
        state.borrow_mut().other.insert("use_opengl".into(), Value::Bool(r.is_active()));
    }));
    row_hidpi.connect_active_notify(clone!(@strong state => move |r| {
        state.borrow_mut().other.insert("enable_hidpi".into(), Value::Bool(r.is_active()));
    }));
    row_close.connect_active_notify(clone!(@strong state => move |r| {
        state.borrow_mut().other.insert("close_on_leave".into(), Value::Bool(r.is_active()));
    }));
    row_touch.connect_apply(clone!(@strong state => move |r| {
        state.borrow_mut().other.insert("touch_mode".into(), Value::String(r.text().to_string()));
    }));

    let save_btn = gtk4::Button::builder().label("Save Settings").css_classes(["suggested-action", "pill"]).halign(gtk4::Align::End).build();
    root.append(&save_btn);

    save_btn.connect_clicked(clone!(@strong state, @strong toast => move |_| {
        let s = state.borrow();
        let mut cfg = s.other.clone();
        cfg.insert("fflags".into(), Value::Object(s.fflags.clone()));
        match save_config(&s.paths, &cfg) {
            Ok(_) => toast("Settings saved"),
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
        (s.paths.config_dir.clone(), s.paths.data_dir.clone(), s.paths.asset_overlay.clone(),
         s.paths.config_file.clone(), s.paths.client_settings.clone())
    };
    let (config_dir, data_dir, asset_overlay, config_file, client_settings) = paths_snapshot;

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
    let r3 = adw::ActionRow::builder().title("Clear Asset Overlay").subtitle("Removes custom texture/asset replacements").activatable(true).build();
    r3.add_suffix(&gtk4::Image::from_icon_name("edit-clear-symbolic"));
    group2.add(&r3);
    let r4 = adw::ActionRow::builder()
        .title("Reset FFlags & Config")
        .subtitle("Deletes config.json and ClientAppSettings.json (per VinegarHQ's official reset steps)")
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

    r1.connect_activated(clone!(@strong toast, @strong config_dir => move |_| {
        let _ = std::fs::create_dir_all(&config_dir);
        let uri = format!("file://{}", config_dir.display());
        if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
            toast("Couldn't open folder");
        }
    }));
    r2.connect_activated(clone!(@strong toast, @strong data_dir => move |_| {
        let _ = std::fs::create_dir_all(&data_dir);
        let uri = format!("file://{}", data_dir.display());
        if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
            toast("Couldn't open folder");
        }
    }));
    r_instances.connect_activated(clone!(@strong toast => move |_| {
        let dir = instances_dir();
        let _ = std::fs::create_dir_all(&dir);
        let uri = format!("file://{}", dir.display());
        if gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE).is_err() {
            toast("Couldn't open folder");
        }
    }));

    r3.connect_activated(clone!(@strong window, @strong toast, @strong adv_log, @strong asset_overlay => move |_| {
        let body = format!("This deletes:\n{}", asset_overlay.display());
        confirm_dialog(&window, "Clear asset overlay?", &body, "Clear", true,
            clone!(@strong toast, @strong adv_log, @strong asset_overlay => move || {
                if asset_overlay.exists() {
                    let _ = std::fs::remove_dir_all(&asset_overlay);
                    log_line(&adv_log, &format!("Removed {}", asset_overlay.display()));
                    toast("Asset overlay cleared");
                } else {
                    toast("Nothing to clear");
                }
            }),
        );
    }));

    r4.connect_activated(clone!(@strong window, @strong state, @strong toast, @strong adv_log,
        @strong config_file, @strong client_settings => move |_| {
        confirm_dialog(&window, "Reset FFlags & config?",
            "This deletes config.json and ClientAppSettings.json. Sober will regenerate defaults on next launch.",
            "Reset", true,
            clone!(@strong state, @strong toast, @strong adv_log, @strong config_file, @strong client_settings => move || {
                for f in [&config_file, &client_settings] {
                    if f.exists() {
                        let _ = std::fs::remove_file(f);
                        log_line(&adv_log, &format!("Deleted {}", f.display()));
                    }
                }
                let mut s = state.borrow_mut();
                s.fflags.clear();
                s.other.clear();
                toast("Sober config reset");
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
        ("Sober (VinegarHQ)", "https://sober.vinegarhq.org/"),
        ("Sober on Flathub", "https://flathub.org/apps/org.vinegarhq.Sober"),
        ("Sober source / issue tracker", "https://github.com/vinegarhq/sober"),
        ("Allowlisted FastFlags reference", "https://github.com/dyokism/sober-fastflags"),
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
                 endorsed by, or supported by Roblox Corporation or the VinegarHQ team. \
                 Sober's FFlag allowlist is controlled by Roblox and may change at any time; \
                 flags outside the allowlist are ignored automatically.",
            )
            .wrap(true)
            .css_classes(["dim-label", "caption"])
            .justify(gtk4::Justification::Center)
            .build(),
    );

    stack.add_titled_with_icon(&page_scroller(&root), Some("about"), "About", "help-about-symbolic");
}
