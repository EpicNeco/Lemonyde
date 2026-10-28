#!/usr/bin/env bash

set -euo pipefail

if [[ -z "${BASH_VERSION:-}" ]]; then
  echo "Error: this script requires bash. Run with: bash install.sh" >&2
  exit 1
fi

: "${HOME:?"HOME is not set — cannot determine install locations."}"

REPO_URL="${LEMONYDE_REPO:-https://github.com/epicneco/lemonyde.git}"
REF="${LEMONYDE_REF:-}"

INSTALL_DIR="${LEMONYDE_INSTALL_DIR:-${HOME}/.local/share/lemonyde}"
BIN_DIR="${LEMONYDE_BIN_DIR:-${HOME}/.local/bin}"
DESKTOP_DIR="${LEMONYDE_DESKTOP_DIR:-${HOME}/.local/share/applications}"
ICON_DIR="${LEMONYDE_ICON_DIR:-${HOME}/.local/share/icons/hicolor/scalable/apps}"

AUTO_DEPS="${LEMONYDE_NO_DEPS:+0}"
AUTO_DEPS="${AUTO_DEPS:-1}"
AUTO_RUST="${LEMONYDE_NO_RUST:+0}"
AUTO_RUST="${AUTO_RUST:-1}"
AUTO_FLATPAK="${LEMONYDE_NO_FLATPAK:+0}"
AUTO_FLATPAK="${AUTO_FLATPAK:-1}"
INSTALL_MOCKTAIL_PROMPT=1
INTERACTIVE=0
SRC_DIR_OVERRIDE="${LEMONYDE_SRC_DIR:-}"

if [[ -n "${LEMONYDE_NO_MOCKTAIL:-}" || -n "${LEMONYDE_NO_SOBER:-}" ]]; then
  INSTALL_MOCKTAIL_PROMPT=0
fi

usage() {
  cat <<'USAGE'
Usage: install.sh [options]

Fully automated by default — just run it, no flags needed.
  curl -fsSL https://raw.githubusercontent.com/epicneco/lemonyde/main/install.sh | bash

Options:
  -y, --yes        Accepted for backwards compatibility (automation is now
                   the default, so this is a no-op)
  -i, --interactive
                   Ask before each install step instead of just doing it
  --no-mocktail    Don't auto-install Mocktail at the end (Flatpak or native)
                   (alias: --no-sober)
  --no-deps        Don't auto-install GTK4/libadwaita dev packages (fail if missing)
  --no-rust        Don't auto-install Rust via rustup (fail if cargo missing)
  --no-flatpak     Don't auto-install Flatpak (used only when Mocktail
                   isn't already installed natively)
  --repo URL       Git repo to clone when no local checkout is found
  --ref REF        Branch/tag to clone (default: repo default branch)
  -h, --help       Show this help
Env:
  LEMONYDE_REPO, LEMONYDE_REF, LEMONYDE_SRC_DIR, LEMONYDE_INSTALL_DIR,
  LEMONYDE_BIN_DIR, LEMONYDE_DESKTOP_DIR, LEMONYDE_ICON_DIR,
  LEMONYDE_NO_MOCKTAIL=1, LEMONYDE_NO_DEPS=1, LEMONYDE_NO_RUST=1,
  LEMONYDE_NO_FLATPAK=1
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes) shift ;; 
    -i|--interactive) INTERACTIVE=1; shift ;;
    --no-mocktail|--no-sober) INSTALL_MOCKTAIL_PROMPT=0; shift ;;
    --no-deps) AUTO_DEPS=0; shift ;;
    --no-rust) AUTO_RUST=0; shift ;;
    --no-flatpak) AUTO_FLATPAK=0; shift ;;
    --repo) REPO_URL="${2:?--repo needs a URL}"; shift 2 ;;
    --ref) REF="${2:?--ref needs a value}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    --) shift; break ;;
    -*) echo "Unknown option: $1 (see --help)" >&2; exit 1 ;;
    *) break ;;
  esac
done

c_green() { printf '\033[1;32m%s\033[0m\n' "$1"; }
c_yellow() { printf '\033[1;33m%s\033[0m\n' "$1"; }
c_blue() { printf '\033[1;34m%s\033[0m\n' "$1"; }
c_red() { printf '\033[1;31m%s\033[0m' "$1" >&2; }
die() { c_red "$1"; echo; exit "${2:-1}"; }
info() { printf '%s\n' "$1"; }


ask() {
  local prompt="$1" def="${2:-N}" reply=""
  if [[ "${INTERACTIVE}" != "1" ]]; then
    return 0
  fi
  if { exec 3<>/dev/tty; } 2>/dev/null; then
    printf '%s [y/N] ' "${prompt}" >&3 || true
    IFS= read -r reply <&3 || reply="${def}"
    exec 3>&- 3<&- || true
    [[ "${reply:-$def}" =~ ^[Yy]$ ]]
  else
    return 1
  fi
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}


# Find and set an available root command (sudo, doas, etc.)
find_root_cmd() {
  # Check common privilege escalation commands in order of preference
  for cmd in sudo doas opendoas sudo-rs; do
    if need_cmd "$cmd"; then
      echo "$cmd"
      return 0
    fi
  done
  return 1
}

SUDO=""
if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
  SUDO=""
else
  if ROOT_CMD="$(find_root_cmd)"; then
    SUDO="$ROOT_CMD"
  else
    c_yellow "Warning: not running as root and no privilege escalation command found."
    c_yellow "Checked for: sudo, doas, opendoas, sudo-rs"
    c_yellow "System package installs will fail. Re-run as root, install one of the above, or install dependencies manually (see --help output on failure)."
  fi
fi

sed_escape_replacement() {
  # Escape \, &, and | (our sed delimiter) for use in a replacement string.
  printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'
}

# Print the detected package manager: apt, dnf, yum, pacman, zypper, apk, xbps, eopkg, or "".
detect_pkg_mgr() {
  if need_cmd apt-get; then echo "apt";
  elif need_cmd dnf; then echo "dnf";
  elif need_cmd yum; then echo "yum";
  elif need_cmd pacman; then echo "pacman";
  elif need_cmd zypper; then echo "zypper";
  elif need_cmd apk; then echo "apk";
  elif need_cmd xbps-install; then echo "xbps";
  elif need_cmd eopkg; then echo "eopkg";
  else echo "";
  fi
}

pkg_install() {
  local mgr
  mgr="$(detect_pkg_mgr)"
  case "${mgr}" in
    apt)
      ${SUDO:+$SUDO }apt-get update && ${SUDO:+$SUDO }apt-get install -y "$@"
      ;;
    dnf)
      ${SUDO:+$SUDO }dnf install -y "$@"
      ;;
    yum)
      ${SUDO:+$SUDO }yum install -y "$@"
      ;;
    pacman)
      ${SUDO:+$SUDO }pacman -S --needed --noconfirm "$@"
      ;;
    zypper)
      ${SUDO:+$SUDO }zypper install -y "$@"
      ;;
    apk)
      ${SUDO:+$SUDO }apk add "$@"
      ;;
    xbps)
      ${SUDO:+$SUDO }xbps-install -Sy "$@"
      ;;
    eopkg)
      ${SUDO:+$SUDO }eopkg install -y "$@"
      ;;
    *)
      return 1
      ;;
  esac
}

print_manual_deps() {
  echo "Install them for your distro, then re-run this script (or re-run with automation):"
  echo
  echo "  Debian/Ubuntu:  sudo apt-get install libgtk-4-dev libadwaita-1-dev build-essential pkg-config curl git flatpak"
  echo "  Fedora/RHEL:    sudo dnf install gtk4-devel libadwaita-devel gcc gcc-c++ make pkg-config curl git flatpak"
  echo "  Arch/CachyOS:   sudo pacman -S --needed gtk4 libadwaita base-devel pkg-config curl git flatpak"
  echo "  openSUSE:       sudo zypper install gtk4-devel libadwaita-devel patterns-devel-base-devel_basis pkg-config curl git flatpak"
  echo "  Alpine:         sudo apk add gtk4.0-dev libadwaita-dev build-base pkgconfig curl git flatpak"
  echo "  Void:           sudo xbps-install -Sy gtk4-devel libadwaita-devel base-devel pkg-config curl git flatpak"
  echo
}


deps_for() {
  local kind="$1" mgr="$2"
  case "${mgr}" in
    apt)
      case "${kind}" in
        dev) echo "libgtk-4-dev libadwaita-1-dev build-essential pkg-config" ;;
        git) echo "git" ;;
        curl) echo "curl ca-certificates" ;;
        flatpak) echo "flatpak" ;;
      esac
      ;;
    dnf|yum)
      case "${kind}" in
        dev) echo "gtk4-devel libadwaita-devel gcc gcc-c++ make pkg-config" ;;
        git) echo "git" ;;
        curl) echo "curl ca-certificates" ;;
        flatpak) echo "flatpak" ;;
      esac
      ;;
    pacman)
      case "${kind}" in
        dev) echo "gtk4 libadwaita base-devel pkg-config" ;;
        git) echo "git" ;;
        curl) echo "curl ca-certificates" ;;
        flatpak) echo "flatpak" ;;
      esac
      ;;
    zypper)
      case "${kind}" in
        dev) echo "gtk4-devel libadwaita-devel patterns-devel-base-devel_basis pkg-config" ;;
        git) echo "git" ;;
        curl) echo "curl ca-certificates" ;;
        flatpak) echo "flatpak" ;;
      esac
      ;;
    apk)
      case "${kind}" in
        dev) echo "gtk4.0-dev libadwaita-dev build-base pkgconfig" ;;
        git) echo "git" ;;
        curl) echo "curl ca-certificates" ;;
        flatpak) echo "flatpak" ;;
      esac
      ;;
    xbps)
      case "${kind}" in
        dev) echo "gtk4-devel libadwaita-devel base-devel pkg-config" ;;
        git) echo "git" ;;
        curl) echo "curl ca-certificates" ;;
        flatpak) echo "flatpak" ;;
      esac
      ;;
    eopkg)
      case "${kind}" in
        dev) echo "libgtk-4-devel libadwaita-devel system.devel pkg-config" ;;
        git) echo "git" ;;
        curl) echo "curl ca-certificates" ;;
        flatpak) echo "flatpak" ;;
      esac
      ;;
  esac
}


ensure_pkg() {
  local kind="$1" desc="$2" mgr pkgs
  mgr="$(detect_pkg_mgr)"
  
  pkgs=($(deps_for "${kind}" "${mgr}"))
  if [[ "${#pkgs[@]}" -eq 0 ]]; then
    die "Unrecognized package manager — please install ${desc} manually, then re-run."
  fi
  if [[ "${INTERACTIVE}" == "1" ]]; then
    ask "Install ${desc} now (${pkgs[*]})?" || die "Cannot continue without ${desc}."
  else
    c_blue "→ Installing ${desc} (${pkgs[*]})…"
  fi
  pkg_install "${pkgs[@]}" || die "Failed to install ${desc}. Try manually, then re-run."
}


try_install_pkg() {
  local kind="$1" desc="$2" mgr pkgs
  mgr="$(detect_pkg_mgr)"

  pkgs=($(deps_for "${kind}" "${mgr}"))
  if [[ "${#pkgs[@]}" -eq 0 ]]; then
    c_yellow "Warning: unrecognized package manager — install ${desc} manually; see https://flatpak.org/setup/."
    return 1
  fi
  if [[ "${INTERACTIVE}" == "1" ]]; then
    ask "Install ${desc} now (${pkgs[*]})?" || { c_yellow "Skipping ${desc} install."; return 1; }
  else
    c_blue "→ Installing ${desc} (${pkgs[*]})…"
  fi
  if pkg_install "${pkgs[@]}"; then
    return 0
  else
    c_yellow "Warning: could not install ${desc}."
    return 1
  fi
}
echo -e "\033[33m


	██╗      ████████╗██╗     ██╗  ████╗  ██╗     ██╗██╗     ██╗██████╗  ████████╗
	██║      ██╔═════╝████╗ ████║██╔═══██╗████╗   ██║╚═██╗ ██╔═╝██╔═══██╗██╔═════╝
	██║      ██████╗  ██╔═██╔═██║██║   ██║██╔═██╗ ██║  ╚═██╔═╝  ██║   ██║██████╗  
	██║      ██╔═══╝  ██║ ╚═╝ ██║██║   ██║██║ ╚═████║    ██║    ██║   ██║██╔═══╝  
	████████╗████████╗██║     ██║╚═████╔═╝██║   ╚═██║    ██║    ██████╔═╝████████╗
	╚═══════╝╚═══════╝╚═╝     ╚═╝  ╚═══╝  ╚═╝     ╚═╝    ╚═╝    ╚═════╝  ╚═══════╝
                                                                              
                                                                                                                                  
\033[0m"

# 0. Resolve sources: prefer a local checkout, otherwise clone.
SRC_DIR=""
WORKDIR=""
cleanup() {
  if [[ -n "${WORKDIR}" && -d "${WORKDIR}" ]]; then
    rm -rf "${WORKDIR}"
  fi
}
trap cleanup EXIT

if [[ -n "${SRC_DIR_OVERRIDE}" ]]; then
  SRC_DIR="${SRC_DIR_OVERRIDE}"
else
  SCRIPT_SOURCE="${BASH_SOURCE[0]:-$0}"
  CANDIDATE_DIR=""
  if [[ -f "${SCRIPT_SOURCE}" ]] && [[ "${SCRIPT_SOURCE}" != /dev/fd/* ]] && [[ "${SCRIPT_SOURCE}" != /proc/self/fd/* ]]; then
    CANDIDATE_DIR="$(cd "$(dirname "${SCRIPT_SOURCE}")" && pwd)"
  fi
  if [[ -n "${CANDIDATE_DIR}" && ( -f "${CANDIDATE_DIR}/Cargo.toml" || -f "${CANDIDATE_DIR}/src/Cargo.toml" ) ]]; then
    SRC_DIR="${CANDIDATE_DIR}"
  else
    # Piped via curl, or script shipped without sources: clone the repo.
    if [[ -z "${REPO_URL}" ]]; then
      die "No local sources found and no repo URL configured. Set --repo or LEMONYDE_REPO."
    fi
    if ! need_cmd git; then
      if [[ "${AUTO_DEPS}" == "1" ]]; then
        c_yellow "git not found — installing it automatically…"
        ensure_pkg git "git"
      else
        die "git not found (needed to download Lemonyde). Install git and re-run, or clone ${REPO_URL} manually. (--no-deps disables auto-install)"
      fi
    fi
    need_cmd git || die "git still not found after install attempt."
    WORKDIR="$(mktemp -d -t lemonyde-install.XXXXXX)"
    echo "No local sources detected — cloning ${REPO_URL}…"
    if [[ -n "${REF}" ]]; then
      git clone --depth 1 --branch "${REF}" "${REPO_URL}" "${WORKDIR}/lemonyde"
    else
      git clone --depth 1 "${REPO_URL}" "${WORKDIR}/lemonyde"
    fi
    SRC_DIR="${WORKDIR}/lemonyde"
  fi
fi

# Resolve the Cargo manifest automatically (supports both layouts):
#   ./Cargo.toml        (classic root layout)
#   ./src/Cargo.toml    (current Lemonyde layout, manifest lives in src/)
MANIFEST_DIR=""
if [[ -f "${SRC_DIR}/Cargo.toml" ]]; then
  MANIFEST_DIR="${SRC_DIR}"
elif [[ -f "${SRC_DIR}/src/Cargo.toml" ]]; then
  MANIFEST_DIR="${SRC_DIR}/src"
else
  die "Cargo.toml not found in ${SRC_DIR} nor ${SRC_DIR}/src (repo layout unexpected)."
fi
MANIFEST_PATH="${MANIFEST_DIR}/Cargo.toml"
info "Using Cargo manifest: ${MANIFEST_PATH}"

# Resolve .desktop file (root, then AppDir/ — where it currently lives).
DESKTOP_SRC=""
for _cand in "${SRC_DIR}/lemonyde.desktop" "${SRC_DIR}/AppDir/lemonyde.desktop"; do
  if [[ -f "${_cand}" ]]; then DESKTOP_SRC="${_cand}"; break; fi
done
[[ -n "${DESKTOP_SRC}" ]] || die "lemonyde.desktop not found in ${SRC_DIR} (checked ./lemonyde.desktop and ./AppDir/lemonyde.desktop)."

# Resolve style.css (root, then assets/ — where it currently lives).
STYLE_SRC=""
for _cand in "${SRC_DIR}/style.css" "${SRC_DIR}/assets/style.css"; do
  if [[ -f "${_cand}" ]]; then STYLE_SRC="${_cand}"; break; fi
done
[[ -n "${STYLE_SRC}" ]] || die "style.css not found in ${SRC_DIR} (checked ./style.css and ./assets/style.css)."

ASSETS_DIR="${SRC_DIR}/assets"
[[ -d "${ASSETS_DIR}" ]] || die "assets/ directory not found in ${SRC_DIR}."
[[ -f "${ASSETS_DIR}/lemonyde.svg" ]] || die "assets/lemonyde.svg not found in ${SRC_DIR}."

# 1. Rust toolchain — auto-install via rustup when cargo is missing.
if ! need_cmd cargo; then
  if [[ "${AUTO_RUST}" != "1" ]]; then
    die "cargo/rustc not found. Install Rust first: https://rustup.rs (or re-run without --no-rust to auto-install via rustup)."
  fi
  c_yellow "Rust toolchain not found — installing automatically via rustup…"
  if ! need_cmd curl; then
    if need_cmd wget; then
      c_yellow "curl not found, using wget to fetch rustup…"
    else
      c_yellow "curl not found — installing it first…"
      ensure_pkg curl "curl"
    fi
  fi
  if need_cmd curl; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  elif need_cmd wget; then
    wget -qO- https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  else
    die "Neither curl nor wget is available to fetch rustup. Install one and re-run."
  fi
  # Pick up cargo for the rest of this script run.
  export PATH="${HOME}/.cargo/bin:${PATH}"
  if [[ -f "${HOME}/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    . "${HOME}/.cargo/env" || true
  fi
  need_cmd cargo || die "rustup install finished but cargo is still not on PATH. Add \$HOME/.cargo/bin to PATH and re-run."
  c_green "Rust installed: $(cargo --version)"
else
  info "Found $(cargo --version 2>/dev/null || echo cargo)."
fi

# 2. GTK4 / libadwaita dev headers (needed to build) — auto-install.
missing_pkgs=()
if need_cmd pkg-config; then
  pkg-config --exists gtk4 2>/dev/null || missing_pkgs+=("gtk4")
  pkg-config --exists libadwaita-1 2>/dev/null || missing_pkgs+=("libadwaita-1")
else
  missing_pkgs+=("pkg-config" "gtk4" "libadwaita-1")
fi

if [[ "${#missing_pkgs[@]}" -gt 0 ]]; then
  if [[ "${AUTO_DEPS}" != "1" ]]; then
    c_yellow "Missing dev packages: ${missing_pkgs[*]}"
    print_manual_deps
    die "Cannot continue without GTK4/libadwaita dev packages. (--no-deps disables auto-install)"
  fi
  c_yellow "Missing dev packages: ${missing_pkgs[*]} — installing automatically…"
  ensure_pkg dev "GTK4/libadwaita dev packages"
  # Re-check after install.
  need_cmd pkg-config || die "pkg-config still missing after install."
  pkg-config --exists gtk4 2>/dev/null || die "gtk4 dev files still missing after install."
  pkg-config --exists libadwaita-1 2>/dev/null || die "libadwaita-1 dev files still missing after install."
  c_green "Dev dependencies satisfied."
else
  info "Found GTK4 + libadwaita dev files."
fi

# 3. Flatpak + Flathub (only needed when Mocktail comes from Flatpak) —
#    auto-install unless the user already has a native Mocktail package
#    (distro repo / AUR — a `mocktail` binary on PATH) or --no-flatpak.
MOCKTAIL_FLAVOR="none"
if need_cmd flatpak && flatpak info space.bigrat.mocktail >/dev/null 2>&1; then
  MOCKTAIL_FLAVOR="flatpak"
elif need_cmd mocktail; then
  MOCKTAIL_FLAVOR="native"
fi

if [[ "${MOCKTAIL_FLAVOR}" == "native" ]]; then
  info "Detected native Mocktail package ($(command -v mocktail)) — Flatpak setup not needed."
elif ! need_cmd flatpak; then
  if [[ "${AUTO_FLATPAK}" != "1" ]]; then
    c_yellow "Flatpak isn't installed (auto-install disabled via --no-flatpak)."
    c_yellow "Lemonyde can still open, but it can't install/launch the Flatpak build of Mocktail."
    echo "See https://flatpak.org/setup/ for instructions for your distro."
  else
    c_yellow "Flatpak isn't installed — installing automatically…"
    if try_install_pkg flatpak "Flatpak"; then
      c_green "Flatpak installed."
    else
      c_yellow "Lemonyde can still open, but it can't install/launch the Flatpak build of Mocktail."
      echo "See https://flatpak.org/setup/ for instructions for your distro."
    fi
  fi
fi
if [[ "${MOCKTAIL_FLAVOR}" != "native" ]] && need_cmd flatpak; then
  if ! flatpak remote-list 2>/dev/null | grep -qi '^flathub[[:space:]]'; then
    c_yellow "Adding the Flathub remote (needed to install Mocktail)…"
    flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo \
      || c_yellow "Warning: could not add the Flathub remote. You can add it later; see https://flatpak.org/setup/."
  fi
fi

# 4. Build (manifest auto-detected above — no manual Cargo.toml handling needed)
echo "Building Lemonyde (release, this can take a couple of minutes)…"
(cd "${MANIFEST_DIR}" && env -u RUSTFLAGS -u CARGO_BUILD_RUSTFLAGS cargo build --release)

# Binary lands under the manifest dir (src/target/... for the src/ layout);
# also check the repo root for the classic layout / custom CARGO_TARGET_DIR.
BIN_SRC=""
for _cand in "${MANIFEST_DIR}/target/release/lemonyde" "${SRC_DIR}/target/release/lemonyde"; do
  if [[ -x "${_cand}" || -f "${_cand}" ]]; then BIN_SRC="${_cand}"; break; fi
done
[[ -n "${BIN_SRC}" ]] || die "Build finished but target/release/lemonyde is missing (checked under ${MANIFEST_DIR} and ${SRC_DIR})."

# 5. Install files
echo "Installing Lemonyde to ${INSTALL_DIR}"
mkdir -p "${INSTALL_DIR}/assets" "${BIN_DIR}" "${DESKTOP_DIR}" "${ICON_DIR}"
cp -f "${BIN_SRC}" "${INSTALL_DIR}/lemonyde-bin"
chmod +x "${INSTALL_DIR}/lemonyde-bin"
cp -f "${STYLE_SRC}" "${INSTALL_DIR}/style.css"
cp -rf "${ASSETS_DIR}/." "${INSTALL_DIR}/assets/"
cp -f "${ASSETS_DIR}/lemonyde.svg" "${ICON_DIR}/lemonyde.svg"

printf '#!/usr/bin/env bash\nexec "%s" "$@"\n' "${INSTALL_DIR}/lemonyde-bin" > "${BIN_DIR}/lemonyde"
chmod +x "${BIN_DIR}/lemonyde"

esc_exec="$(sed_escape_replacement "${BIN_DIR}/lemonyde")"
esc_icon="$(sed_escape_replacement "${ICON_DIR}/lemonyde.svg")"
sed "s|^Exec=.*|Exec=${esc_exec}|; s|^Icon=.*|Icon=${esc_icon}|" \
  "${DESKTOP_SRC}" > "${DESKTOP_DIR}/lemonyde.desktop"
chmod +x "${DESKTOP_DIR}/lemonyde.desktop"
if need_cmd update-desktop-database; then
  update-desktop-database "${DESKTOP_DIR}" >/dev/null 2>&1 || true
fi
if need_cmd gtk-update-icon-cache; then
  gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

c_green "Done!"
echo
# 6. Mocktail itself — auto-install, no prompt (unless --no-mocktail).
#    Respects whichever flavor the user already has (flatpak or native) and
#    picks a sensible route when neither is installed.
mocktail_native_install() {
  # Prefer an AUR helper on Arch-likes — Mocktail lives in the AUR there.
  local helper=""
  if need_cmd paru; then helper="paru"
  elif need_cmd yay; then helper="yay"
  fi
  if [[ -n "${helper}" ]]; then
    "${helper}" -S --needed --noconfirm mocktail
    return $?
  fi
  pkg_install mocktail
}

if [[ "${INSTALL_MOCKTAIL_PROMPT}" != "1" ]]; then
  info "Skipping Mocktail install (--no-mocktail)."
elif [[ "${MOCKTAIL_FLAVOR}" == "flatpak" ]]; then
  info "Mocktail is already installed (Flatpak: space.bigrat.mocktail)."
elif [[ "${MOCKTAIL_FLAVOR}" == "native" ]]; then
  info "Mocktail is already installed (native package: $(command -v mocktail))."
elif need_cmd flatpak; then
  # No Mocktail anywhere, but Flatpak is available — Flathub is upstream's
  # recommended route.
  if [[ "${INTERACTIVE}" == "1" ]]; then
    if ask "Mocktail isn't installed yet — install it now via Flathub?"; then
      flatpak install --user -y flathub space.bigrat.mocktail \
        || c_yellow "Warning: Mocktail install failed. You can install it later with: flatpak install --user -y flathub space.bigrat.mocktail"
    fi
  else
    c_blue "→ Installing Mocktail via Flathub (automatic)…"
    flatpak install --user -y flathub space.bigrat.mocktail \
      || c_yellow "Warning: Mocktail install failed. You can install it later with: flatpak install --user -y flathub space.bigrat.mocktail"
  fi
else
  # No Flatpak — try the distro's package manager for a native build.
  MGR="$(detect_pkg_mgr)"
  if [[ -z "${MGR}" ]] && ! need_cmd paru && ! need_cmd yay; then
    c_yellow "No Flatpak and no supported package manager found — install Mocktail manually."
  else
    if [[ "${INTERACTIVE}" == "1" ]]; then
      if ask "Flatpak isn't available — install Mocktail natively via your package manager?"; then
        mocktail_native_install \
          || c_yellow "Warning: native Mocktail install failed (it may not exist in your repos — e.g. the AUR needs paru/yay)."
      fi
    else
      c_blue "→ Installing Mocktail natively (automatic)…"
      mocktail_native_install \
        || c_yellow "Warning: native Mocktail install failed (it may not exist in your repos — e.g. the AUR needs paru/yay)."
    fi
  fi
fi

case ":${PATH:-}:" in
  *":${BIN_DIR}:"*) ;;
  *)
    c_yellow "Note: ${BIN_DIR} isn't on your PATH yet. Add this to your shell rc file:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac

echo "Launch Lemonyde with: lemonyde"
echo "…or find it in your app menu."
