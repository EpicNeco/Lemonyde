#!/usr/bin/env bash
#
# Lemonyde installer — works both ways:
#   ./install.sh                          # local checkout
#   curl -fsSL https://raw.githubusercontent.com/epicneco/lemonyde/main/install.sh | bash
#
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

ASSUME_YES=0
INSTALL_SOBER_PROMPT=1
SRC_DIR_OVERRIDE="${LEMONYDE_SRC_DIR:-}"

usage() {
  cat <<'USAGE'
Usage: install.sh [--yes] [--no-sober] [--repo URL] [--ref BRANCH] [--help]

  --yes, -y      Assume "yes" to all prompts (non-interactive / curl-pipe friendly)
  --no-sober    Don't offer to install Sober via Flatpak at the end
  --repo URL    Git repo to clone when no local checkout is found
  --ref REF     Branch/tag to clone (default: repo default branch)
  --help, -h    Show this help
Env:
  LEMONYDE_REPO, LEMONYDE_REF, LEMONYDE_SRC_DIR, LEMONYDE_INSTALL_DIR,
  LEMONYDE_BIN_DIR, LEMONYDE_DESKTOP_DIR, LEMONYDE_ICON_DIR
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes) ASSUME_YES=1; shift ;;
    --no-sober) INSTALL_SOBER_PROMPT=0; shift ;;
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
c_red() { printf '\033[1;31m%s\033[0m\n' "$1" >&2; }
die() { c_red "$1"; exit "${2:-1}"; }

# ask "prompt" [default] -> returns 0 for yes, 1 for no.
# Never reads from stdin (stdin is the script itself when piped via curl);
# reads from /dev/tty when available, otherwise defaults to No
# (or Yes when --yes was given, handled above).
ask() {
  local prompt="$1" def="${2:-N}" reply=""
  if [[ "${ASSUME_YES}" == "1" ]]; then
    return 0
  fi
  # Try to talk to the controlling terminal. The group redirect silences
  # "No such device or address" when there is no tty (e.g. CI / curl | bash).
  if { exec 3<>/dev/tty; } 2>/dev/null; then
    printf '%s [y/N] ' "${prompt}" >&3 || true
    IFS= read -r reply <&3 || reply="${def}"
    exec 3>&- 3<&- || true
    [[ "${reply:-$def}" =~ ^[Yy]$ ]]
  else
    # Non-interactive (e.g. `curl ... | bash` without --yes): default to No.
    return 1
  fi
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

sed_escape_replacement() {
  # Escape \, &, and | (our sed delimiter) for use in a replacement string.
  printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'
}

echo "🍋 Lemonyde bootstrapper"
echo "--------------------------------"

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
  if [[ -n "${CANDIDATE_DIR}" && -f "${CANDIDATE_DIR}/Cargo.toml" && -f "${CANDIDATE_DIR}/lemonyde.desktop" ]]; then
    SRC_DIR="${CANDIDATE_DIR}"
  else
    # Piped via curl, or script shipped without sources: clone the repo.
    if [[ -z "${REPO_URL}" ]]; then
      die "No local sources found and no repo URL configured. Set --repo or LEMONYDE_REPO."
    fi
    need_cmd git || die "git not found, but it is needed to download Lemonyde. Install git and re-run, or clone ${REPO_URL} manually."
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

[[ -f "${SRC_DIR}/Cargo.toml" ]] || die "Cargo.toml not found in ${SRC_DIR} (repo layout unexpected)."
[[ -f "${SRC_DIR}/lemonyde.desktop" ]] || die "lemonyde.desktop not found in ${SRC_DIR}."

# 1. Rust toolchain
need_cmd cargo || die "cargo/rustc not found. Install Rust first: https://rustup.rs"

# 2. GTK4 / libadwaita dev headers (needed to build)
missing_pkgs=()
if need_cmd pkg-config; then
  pkg-config --exists gtk4 2>/dev/null || missing_pkgs+=("gtk4")
  pkg-config --exists libadwaita-1 2>/dev/null || missing_pkgs+=("libadwaita-1")
else
  missing_pkgs+=("gtk4" "libadwaita-1")
fi

if [[ "${#missing_pkgs[@]}" -gt 0 ]]; then
  c_yellow "Missing dev packages: ${missing_pkgs[*]}"
  echo "Install them for your distro, then re-run this script:"
  echo
  echo "  Debian/Ubuntu:  sudo apt-get install libgtk-4-dev libadwaita-1-dev build-essential pkg-config"
  echo "  Fedora:         sudo dnf install gtk4-devel libadwaita-devel gcc pkg-config"
  echo "  Arch:           sudo pacman -S --needed gtk4 libadwaita base-devel pkg-config"
  echo "  openSUSE:       sudo zypper install gtk4-devel libadwaita-devel patterns-devel-base-devel_basis pkg-config"
  echo
  if ask "Try to install these automatically now?"; then
    if need_cmd apt-get || need_cmd apt; then
      sudo apt-get update && sudo apt-get install -y libgtk-4-dev libadwaita-1-dev build-essential pkg-config
    elif need_cmd dnf; then
      sudo dnf install -y gtk4-devel libadwaita-devel gcc pkg-config
    elif need_cmd pacman; then
      if [[ "${ASSUME_YES}" == "1" ]]; then
        sudo pacman -S --needed --noconfirm gtk4 libadwaita base-devel pkg-config
      else
        sudo pacman -S --needed gtk4 libadwaita base-devel pkg-config
      fi
    elif need_cmd zypper; then
      sudo zypper install -y gtk4-devel libadwaita-devel patterns-devel-base-devel_basis pkg-config
    else
      die "Unrecognized package manager — please install the packages manually."
    fi
    # Re-check after attempted install.
    if need_cmd pkg-config; then
      pkg-config --exists gtk4 2>/dev/null || die "gtk4 dev files still missing after install."
      pkg-config --exists libadwaita-1 2>/dev/null || die "libadwaita-1 dev files still missing after install."
    fi
  else
    if [[ -t 0 ]] || [[ "${ASSUME_YES}" == "1" ]]; then
      die "Cannot continue without GTK4/libadwaita dev packages."
    else
      die "Cannot continue without GTK4/libadwaita dev packages (non-interactive; re-run with --yes to auto-install where supported, or install manually)."
    fi
  fi
fi

# 3. Flatpak + Flathub (needed to install/run Sober itself)
if ! need_cmd flatpak; then
  c_yellow "Flatpak isn't installed. Lemonyde can still open, but it can't install/launch Sober."
  echo "See https://flatpak.org/setup/ for instructions for your distro."
else
  if ! flatpak remote-list 2>/dev/null | grep -qi '^flathub[[:space:]]'; then
    c_yellow "Adding the Flathub remote (needed to install Sober)…"
    flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo \
      || c_yellow "Warning: could not add the Flathub remote. You can add it later; see https://flatpak.org/setup/."
  fi
fi

# 4. Build
echo "Building Lemonyde (release, this can take a couple of minutes)…"
(cd "${SRC_DIR}" && env -u RUSTFLAGS -u CARGO_BUILD_RUSTFLAGS cargo build --release)

BIN_SRC="${SRC_DIR}/target/release/lemonyde"
[[ -x "${BIN_SRC}" || -f "${BIN_SRC}" ]] || die "Build finished but ${BIN_SRC} is missing."
[[ -f "${SRC_DIR}/style.css" ]] || die "style.css not found in ${SRC_DIR}."
[[ -d "${SRC_DIR}/assets" ]] || die "assets/ directory not found in ${SRC_DIR}."
[[ -f "${SRC_DIR}/assets/lemonyde.svg" ]] || die "assets/lemonyde.svg not found in ${SRC_DIR}."

# 5. Install files
echo "Installing Lemonyde to ${INSTALL_DIR}"
mkdir -p "${INSTALL_DIR}/assets" "${BIN_DIR}" "${DESKTOP_DIR}" "${ICON_DIR}"
cp -f "${BIN_SRC}" "${INSTALL_DIR}/lemonyde-bin"
chmod +x "${INSTALL_DIR}/lemonyde-bin"
cp -f "${SRC_DIR}/style.css" "${INSTALL_DIR}/style.css"
cp -rf "${SRC_DIR}/assets/." "${INSTALL_DIR}/assets/"
cp -f "${SRC_DIR}/assets/lemonyde.svg" "${ICON_DIR}/lemonyde.svg"

printf '#!/usr/bin/env bash\nexec "%s" "$@"\n' "${INSTALL_DIR}/lemonyde-bin" > "${BIN_DIR}/lemonyde"
chmod +x "${BIN_DIR}/lemonyde"

esc_exec="$(sed_escape_replacement "${BIN_DIR}/lemonyde")"
esc_icon="$(sed_escape_replacement "${ICON_DIR}/lemonyde.svg")"
sed "s|^Exec=.*|Exec=${esc_exec}|; s|^Icon=.*|Icon=${esc_icon}|" \
  "${SRC_DIR}/lemonyde.desktop" > "${DESKTOP_DIR}/lemonyde.desktop"
chmod +x "${DESKTOP_DIR}/lemonyde.desktop"
if need_cmd update-desktop-database; then
  update-desktop-database "${DESKTOP_DIR}" >/dev/null 2>&1 || true
fi
if need_cmd gtk-update-icon-cache; then
  gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

c_green "Done!"
echo
if [[ "${INSTALL_SOBER_PROMPT}" == "1" ]] && need_cmd flatpak && ! flatpak info org.vinegarhq.Sober >/dev/null 2>&1; then
  if ask "Sober isn't installed yet — install it now via Flathub?"; then
    flatpak install --user -y flathub org.vinegarhq.Sober \
      || c_yellow "Warning: Sober install failed. You can install it later with: flatpak install --user flathub org.vinegarhq.Sober"
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
